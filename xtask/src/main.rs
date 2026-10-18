//! rs2 build tool.
//!
//! ```text
//! cargo xtask build [--release]            build loader + kernel, stage work/esp
//! cargo xtask run [--release] [--headless] [--timeout S] [--screenshot PNG]
//!                  [--hdd IMG] [--nvme IMG] [--usb-kbd] [--usb-mouse] [-- QEMU ARGS]
//! cargo xtask iso [--release] [-o FILE]    build the hybrid install/live ISO
//! cargo xtask disk [--hpfs] [-o FILE]       test hard disk from work/disk.txt
//! ```
//!
//! Extra files for `\RS2\MODULES` are listed in `work/modules.txt`, one path
//! per line (relative to the repository root).

mod hpfsimg;
mod image;
mod iso;
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
    /// Boot from this ISO (as a CD) instead of the ESP directory.
    iso: Option<PathBuf>,
    /// Attach this raw disk image as an extra hard disk.
    hdd: Option<PathBuf>,
    /// Attach this raw disk image as an NVMe drive.
    nvme: Option<PathBuf>,
    /// `disk`: make an installed-system disk.
    system: bool,
    /// `run`: boot from the --hdd disk only (no ESP directory).
    boot_hdd: bool,
    /// Build the distributable system (no development files).
    dist: bool,
    /// `disk`: an HPFS partition instead of FAT.
    hpfs: bool,
    /// `run`: add an xHCI controller with a USB keyboard / mouse.
    usb_kbd: bool,
    usb_mouse: bool,
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
        iso: None,
        hdd: None,
        nvme: None,
        system: false,
        boot_hdd: false,
        dist: false,
        hpfs: false,
        usb_kbd: false,
        usb_mouse: false,
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
            "--system" => o.system = true,
            "--boot-hdd" => o.boot_hdd = true,
            "--dist" => o.dist = true,
            "--hpfs" => o.hpfs = true,
            "--usb-kbd" => o.usb_kbd = true,
            "--usb-mouse" => o.usb_mouse = true,
            "--hdd" => {
                i += 1;
                o.hdd = Some(PathBuf::from(&args[i]));
            }
            "--nvme" => {
                i += 1;
                o.nvme = Some(PathBuf::from(&args[i]));
            }
            "--iso" => {
                i += 1;
                o.iso = Some(PathBuf::from(&args[i]));
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
        "disk" => {
            // A test hard disk from work/disk.txt (same manifest format);
            // with --system, an installed system: the ESP tree plus the
            // boot volume files in one bootable FAT partition.
            let mut files = std::fs::read_to_string(root().join("work/disk.txt"))
                .map(|t| image::manifest(&root(), &t))
                .unwrap_or_default();
            if o.system {
                let Some(esp) = build(&o) else { return ExitCode::FAILURE };
                files.clear();
                walk(&esp, "", &mut files);
                files.retain(|(_, d)| !d.ends_with("C.IMG"));
                files.extend(build_userland().unwrap_or_default());
                if let Ok(t) = std::fs::read_to_string(root().join("work/modules.txt")) {
                    files.extend(image::manifest(&root(), &t));
                }
            }
            let out = o.output.clone().unwrap_or_else(|| root().join("work/disk.img"));
            let img = if o.hpfs && !o.system {
                hpfsimg::disk(&files, "RS2HPFS", 64)
            } else {
                image::disk(&files, "RS2DISK", 64)
            };
            std::fs::write(&out, img).unwrap();
            println!("wrote {}", out.display());
            true
        }
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
    strip(&esp.join("RS2/KERNEL"));
    let cmdline = root().join("work/cmdline.txt");
    if cmdline.exists() && !o.dist {
        copy(&cmdline, &esp.join("RS2/CMDLINE"));
    }
    // rs2 user programs (Rust, i686) converted to LX.
    let userland = build_userland()?;
    // Drive C: as a FAT image built from work/modules.txt (development)
    // or dist/manifest.txt (distribution: rs2's own files only).
    let list = if o.dist { root().join("dist/manifest.txt") } else { root().join("work/modules.txt") };
    if let Ok(text) = std::fs::read_to_string(&list) {
        let mut files = image::manifest(&root(), &text);
        files.extend(userland);
        // Boot files for the installer.
        files.push((esp.join("EFI/BOOT/BOOTX64.EFI"), "RS2\\BOOT\\BOOTX64.EFI".to_string()));
        files.push((esp.join("RS2/KERNEL"), "RS2\\BOOT\\KERNEL".to_string()));
        let img = image::build(&files, "RS2", 16, None);
        let out = esp.join("RS2/MODULES/C.IMG");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, img).unwrap();
    }
    Some(esp)
}

/// Programs in userland/ to package, with their LX names.
const USER_PROGRAMS: &[(&str, &str)] =
    &[
        ("apitest", "RS2\\APITEST.EXE"),
        ("testdll", "RS2\\TESTDLL.DLL"),
        ("cmd", "OS2\\CMD.EXE"),
        ("more", "OS2\\MORE.EXE"),
        ("find", "OS2\\FIND.EXE"),
        ("sort", "OS2\\SORT.EXE"),
        ("attrib", "OS2\\ATTRIB.EXE"),
        ("tree", "OS2\\TREE.EXE"),
        ("xcopy", "OS2\\XCOPY.EXE"),
        ("install", "OS2\\INSTALL.EXE"),
    ];

/// Build the userland workspace and convert each program to LX.
fn build_userland() -> Option<image::FileList> {
    let ul = root().join("userland");
    let status = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .current_dir(&ul)
        .args(["build", "--release"])
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let out_dir = root().join("work/userland");
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut files = Vec::new();
    for (bin, dest) in USER_PROGRAMS {
        let elf = ul.join("target/i686-unknown-linux-gnu/release").join(bin);
        let lx = out_dir.join(dest.rsplit('\\').next().unwrap());
        let mut cmd = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
        cmd.current_dir(root()).args(["run", "-q", "--release", "-p", "elf2lx", "--"]).arg(&elf).arg(&lx);
        if dest.ends_with(".DLL") {
            let defs = ul.join(bin).join("exports.def");
            cmd.arg("--dll").arg("--exports").arg(defs);
        }
        let ok = cmd
            .status()
            .ok()?
            .success();
        if !ok {
            return None;
        }
        files.push((lx, dest.to_string()));
    }
    Some(files)
}

/// Remove debug information (keeps the boot image small).
fn strip(file: &Path) {
    let sysroot = Command::new("rustc").args(["--print", "sysroot"]).output().ok();
    let Some(out) = sysroot else { return };
    let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let host = Command::new("rustc").args(["-vV"]).output().ok().map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
            .unwrap_or_default()
    });
    let objcopy = Path::new(&sysroot).join("lib/rustlib").join(host.unwrap_or_default()).join("bin/rust-objcopy");
    if objcopy.exists() {
        let _ = Command::new(objcopy).arg("--strip-debug").arg(file).status();
    }
}

fn ovmf() -> (PathBuf, PathBuf) {
    let share = PathBuf::from("/opt/homebrew/share/qemu");
    let code = share.join("edk2-x86_64-code.fd");
    // A fresh variable store every run, so stale boot entries from other
    // configurations never get in the way.
    let vars = root().join("work/ovmf-vars.fd");
    copy(&share.join("edk2-i386-vars.fd"), &vars);
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
        .args(match &o.iso {
            Some(iso) => vec!["-cdrom".to_string(), iso.display().to_string()],
            None if o.boot_hdd => vec![],
            None => vec!["-drive".to_string(), format!("format=raw,file=fat:rw:{}", work.join("esp").display())],
        })
        .arg("-monitor")
        .arg(format!("unix:{},server,nowait", sock.display()));
    if o.headless {
        c.args(["-display", "none"]).arg("-serial").arg(format!("file:{}", serial.display()));
    } else {
        c.args(["-serial", "stdio"]);
    }
    c.args(&o.qemu_extra);
    if let Some(d) = &o.hdd {
        c.arg("-drive").arg(format!("format=raw,file={}", d.display()));
    }
    if let Some(d) = &o.nvme {
        c.arg("-drive").arg(format!("file={},format=raw,if=none,id=nvm0", d.display()));
        c.args(["-device", "nvme,drive=nvm0,serial=rs2nvme"]);
    }
    if o.usb_kbd || o.usb_mouse {
        c.args(["-device", "qemu-xhci,id=xhci"]);
    }
    if o.usb_kbd {
        c.args(["-device", "usb-kbd,bus=xhci.0"]);
    }
    if o.usb_mouse {
        c.args(["-device", "usb-mouse,bus=xhci.0"]);
    }
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
                if key == "sleep" {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
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

/// QEMU key names for a string. "^X" is Ctrl+X, "{name}" a QEMU key name
/// as is (e.g. {ctrl-esc}, {alt-esc}, {down}), "{sleep}" a one second pause.
fn keys(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut ctrl = false;
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            v.push(name);
            continue;
        }
        // "^X" sends Ctrl+X.
        if ch == '^' && !ctrl {
            ctrl = true;
            continue;
        }
        if ctrl {
            ctrl = false;
            v.push(format!("ctrl-{}", ch.to_ascii_lowercase()));
            continue;
        }
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
            '@' => "shift-2".into(),
            '#' => "shift-3".into(),
            '$' => "shift-4".into(),
            '(' => "shift-9".into(),
            ')' => "shift-0".into(),
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

/// All files below `dir`, with their paths relative to it.
fn walk(dir: &Path, rel: &str, out: &mut image::FileList) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() { name.clone() } else { format!("{rel}\\{name}") };
        if e.path().is_dir() {
            walk(&e.path(), &r, out);
        } else {
            out.push((e.path(), r));
        }
    }
}

fn iso(o: &Opts, esp: &Path) -> bool {
    let mut files = Vec::new();
    walk(esp, "", &mut files);
    let img = image::build(&files, "RS2BOOT", 1, None);
    let out = o.output.clone().unwrap_or_else(|| root().join("work/rs2.iso"));
    let readme = b"rs2 - an OS/2 Warp 4.52 compatible operating system written in Rust.\r\n\
Boot this disc (or a USB stick it was written to) on a UEFI machine.\r\n"
        .to_vec();
    iso::write(&out, "RS2_LIVE", &img, &[("README.TXT".to_string(), readme)]);
    println!("wrote {} ({} MB)", out.display(), std::fs::metadata(&out).map(|m| m.len() >> 20).unwrap_or(0));
    let _ = Stdio::null();
    true
}
