//! rs2 build tool.
//!
//! ```text
//! cargo xtask build [--release]            build loader + kernel, stage work/esp
//! cargo xtask run [--release] [--headless] [--timeout S] [--screenshot PNG] [-- QEMU ARGS]
//! cargo xtask iso [--release] [-o FILE]    build the hybrid install/live ISO
//! ```
//!
//! Extra files for `\RS2\MODULES` are listed in `work/modules.txt`, one path
//! per line (relative to the repository root).

mod image;
mod png;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

struct Opts {
    release: bool,
    headless: bool,
    timeout: Option<u64>,
    screenshot: Option<PathBuf>,
    output: Option<PathBuf>,
    qemu_extra: Vec<String>,
    /// Keystrokes to send (\n = Enter), and when (seconds after start).
    typing: Option<String>,
    type_at: Option<u64>,
}

fn parse(args: &[String]) -> Opts {
    let mut o = Opts {
        release: false,
        headless: false,
        timeout: None,
        screenshot: None,
        output: None,
        qemu_extra: vec![],
        typing: None,
        type_at: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--release" => o.release = true,
            "--headless" => o.headless = true,
            "--timeout" => {
                i += 1;
                o.timeout = Some(args[i].parse().expect("--timeout seconds"));
            }
            "--screenshot" => {
                i += 1;
                o.screenshot = Some(PathBuf::from(&args[i]));
            }
            "--type" => {
                i += 1;
                o.typing = Some(args[i].replace("\\n", "\n"));
            }
            "--type-at" => {
                i += 1;
                o.type_at = Some(args[i].parse().expect("--type-at seconds"));
            }
            "-o" => {
                i += 1;
                o.output = Some(PathBuf::from(&args[i]));
            }
            "--" => {
                o.qemu_extra = args[i + 1..].to_vec();
                break;
            }
            a => panic!("unknown option {a}"),
        }
        i += 1;
    }
    o
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("usage: cargo xtask build|run|iso [options]");
        return ExitCode::FAILURE;
    };
    let o = parse(&args[1..]);
    let ok = match cmd.as_str() {
        "build" => build(&o).is_some(),
        "run" => build(&o).is_some_and(|_| run(&o)),
        "iso" => build(&o).is_some_and(|esp| iso(&o, &esp)),
        _ => {
            eprintln!("unknown command {cmd}");
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn cargo_build(package: &str, target: &str, release: bool) -> Option<PathBuf> {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
    c.current_dir(root()).args(["build", "-p", package, "--target", target]);
    if release {
        c.arg("--release");
    }
    if !c.status().ok()?.success() {
        return None;
    }
    let profile = if release { "release" } else { "debug" };
    Some(root().join("target").join(target).join(profile))
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap_or_else(|e| panic!("copy {} -> {}: {e}", from.display(), to.display()));
}

/// Build everything and lay out the EFI system partition tree.
fn build(o: &Opts) -> Option<PathBuf> {
    let boot = cargo_build("rs2-boot", "x86_64-unknown-uefi", o.release)?;
    let kernel = cargo_build("rs2-kernel", "x86_64-unknown-none", o.release)?;
    let esp = root().join("work/esp");
    let _ = std::fs::remove_dir_all(&esp);
    copy(&boot.join("rs2-boot.efi"), &esp.join("EFI/BOOT/BOOTX64.EFI"));
    copy(&kernel.join("rs2-kernel"), &esp.join("RS2/KERNEL"));
    let cmdline = root().join("work/cmdline.txt");
    if cmdline.exists() {
        copy(&cmdline, &esp.join("RS2/CMDLINE"));
    }
    // Drive C: as a FAT image built from work/modules.txt.
    let list = root().join("work/modules.txt");
    if let Ok(text) = std::fs::read_to_string(&list) {
        let files = image::manifest(&root(), &text);
        let img = image::build(&files, "RS2", 16, None);
        let out = esp.join("RS2/MODULES/C.IMG");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, img).unwrap();
    }
    Some(esp)
}

fn ovmf() -> (PathBuf, PathBuf) {
    let share = PathBuf::from("/opt/homebrew/share/qemu");
    let code = share.join("edk2-x86_64-code.fd");
    let vars = root().join("work/ovmf-vars.fd");
    if !vars.exists() {
        copy(&share.join("edk2-i386-vars.fd"), &vars);
    }
    (code, vars)
}

fn run(o: &Opts) -> bool {
    let (code, vars) = ovmf();
    let work = root().join("work");
    let serial = work.join("serial.log");
    let sock = work.join("qemu-monitor.sock");
    let _ = std::fs::remove_file(&sock);
    let mut c = Command::new("qemu-system-x86_64");
    c.args(["-machine", "q35", "-cpu", "max", "-m", "512M", "-no-reboot"])
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,readonly=on,file={}", code.display()))
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars.display()))
        .arg("-drive")
        .arg(format!("format=raw,file=fat:rw:{}", work.join("esp").display()))
        .arg("-monitor")
        .arg(format!("unix:{},server,nowait", sock.display()));
    if o.headless {
        c.args(["-display", "none"]).arg("-serial").arg(format!("file:{}", serial.display()));
    } else {
        c.args(["-serial", "stdio"]);
    }
    c.args(&o.qemu_extra);
    let mut child = c.spawn().expect("start qemu");
    let Some(timeout) = o.timeout else {
        return child.wait().map(|s| s.success()).unwrap_or(false);
    };
    let start = Instant::now();
    let deadline = start + Duration::from_secs(timeout);
    let mut typed = o.typing.is_none();
    let type_at = o.type_at.unwrap_or(timeout.saturating_sub(8));
    while Instant::now() < deadline {
        if !typed && start.elapsed() >= Duration::from_secs(type_at) {
            typed = true;
            for key in keys(o.typing.as_deref().unwrap()) {
                monitor(&sock, &format!("sendkey {key}"));
                std::thread::sleep(Duration::from_millis(60));
            }
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if child.try_wait().ok().flatten().is_none() {
        if let Some(png) = &o.screenshot {
            let ppm = work.join("screen.ppm");
            monitor(&sock, &format!("screendump {}", ppm.display()));
            std::thread::sleep(Duration::from_millis(500));
            match std::fs::read(&ppm) {
                Ok(data) => png::ppm_to_png(&data, png),
                Err(e) => eprintln!("no screendump: {e}"),
            }
        }
        monitor(&sock, "quit");
        std::thread::sleep(Duration::from_millis(300));
        let _ = child.kill();
    }
    let _ = child.wait();
    if o.headless {
        if let Ok(log) = std::fs::read_to_string(&serial) {
            print!("{log}");
        }
    }
    true
}

/// QEMU key names for a string.
fn keys(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    for ch in s.chars() {
        let k = match ch {
            'a'..='z' | '0'..='9' => ch.to_string(),
            'A'..='Z' => format!("shift-{}", ch.to_ascii_lowercase()),
            ' ' => "spc".into(),
            '\n' => "ret".into(),
            '\\' => "backslash".into(),
            ':' => "shift-semicolon".into(),
            ';' => "semicolon".into(),
            '.' => "dot".into(),
            ',' => "comma".into(),
            '-' => "minus".into(),
            '_' => "shift-minus".into(),
            '=' => "equal".into(),
            '+' => "shift-equal".into(),
            '/' => "slash".into(),
            '?' => "shift-slash".into(),
            '*' => "shift-8".into(),
            '>' => "shift-dot".into(),
            '<' => "shift-comma".into(),
            '|' => "shift-backslash".into(),
            '"' => "shift-apostrophe".into(),
            '\'' => "apostrophe".into(),
            '%' => "shift-5".into(),
            '!' => "shift-1".into(),
            '&' => "shift-7".into(),
            '[' => "bracket_left".into(),
            ']' => "bracket_right".into(),
            _ => continue,
        };
        v.push(k);
    }
    v
}

fn monitor(sock: &Path, cmd: &str) {
    match std::os::unix::net::UnixStream::connect(sock) {
        Ok(mut s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = writeln!(s, "{cmd}");
            let _ = s.read(&mut buf);
        }
        Err(e) => eprintln!("monitor: {e}"),
    }
}

fn iso(_o: &Opts, _esp: &Path) -> bool {
    eprintln!("iso: not implemented yet");
    let _ = Stdio::null();
    false
}
