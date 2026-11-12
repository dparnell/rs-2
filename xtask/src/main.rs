//! rs2 build tool.
//!
//! ```text
//! cargo xtask build [--release]            build loader + kernel, stage work/esp
//! cargo xtask run [--release] [--headless] [--timeout S] [--screenshot PNG] [--desktop]
//!                  [--type KEYS | --type-file FILE] [--type-at S]
//!                  [--hdd IMG] [--nvme IMG] [--usb-kbd] [--usb-mouse] [--usb-disk IMG]...
//!                  [--usb-hub]... [--boot-hdd] [--monitor-at S --monitor 'CMD;CMD']
//!                  [--vmware] [--pvscsi IMG] [--pvscsi-cd ISO] [--scsi-hba MODEL]
//!                  [--virtio-disk IMG]... [--virtio-scsi IMG]... [--virtio-cd ISO]...
//!                  [--virtio-input|--virtio-kbd|--virtio-mouse|--virtio-tablet]
//!                  [--virtio-rng] [--virtio-console PATH] [--virtio-gpu|--virtio-gpu-pci]
//!                  [--virtio-legacy] [--boot-virtio]
//!                  [--net [MODEL]] [--virtio-net] [--net-model MODEL] [--hostfwd RULE]... [--guestfwd RULE]... [--tftp DIR]
//!                  [--sound [hda|ac97|es1370]] [--sound-wav FILE] [--sound-in FILE|tone:HZ]
//!                  [--smp N] [--mem SIZE]
//!                  [-- QEMU ARGS]
//! cargo xtask iso [--release] [-o FILE]    build the hybrid install/live ISO
//! cargo xtask ir                            write the interface repositories
//!                                           (target/SOM.IR, WPSH.IR, ...)
//! cargo xtask disk [--hpfs|--fat32] [--system] [--size MB] [-o FILE]
//!                                           test hard disk from work/disk.txt
//!                                           (64 MB unless --size)
//! ```
//!
//! `--desktop` boots into the Workplace Shell (`init=OS2\PMSHELL.EXE` in place
//! of work/cmdline.txt's `init=`), `--init "PROGRAM ARGS"` into another
//! program. `--type-file` reads the keys for `--type`
//! from a file (its line ends are not typed). In the keys, `{keydown K}` and
//! `{keyup K}` hold and release a key (QEMU qcode, e.g. `alt`).
//!
//! Extra files for `\RS2\MODULES` are listed in `work/modules.txt`, one path
//! per line (relative to the repository root).
//!
//! `--usb-disk` attaches a raw image as a USB stick (repeatable). Each
//! `--usb-hub` adds a tier of USB hubs: the first sits on root port 1, each
//! further one on port 8 of the previous, and the USB devices go behind
//! the last (ids `usbkbd`, `usbmouse`, `usbdisk0`... for `--monitor`
//! device_del). `--boot-hdd` leaves out the ESP directory, so the machine boots
//! from the attached disks (e.g. the ISO as a USB stick).
//!
//! `--vmware` uses QEMU's VMware device models: the SVGA II display and
//! the backdoor port with its absolute pointer (vmmouse). `--pvscsi` and
//! `--pvscsi-cd` attach a disk image / CD image to a VMware PVSCSI
//! controller (`--scsi-hba lsi53c895a|mptsas1068|megasas` picks another
//! SCSI controller model instead).
//! The virtio options attach paravirtual devices: `--virtio-disk` a
//! virtio-blk disk (repeatable), `--virtio-scsi`/`--virtio-cd` disks and
//! CD-ROMs on one virtio-scsi controller, `--virtio-input` a keyboard, a
//! mouse and a tablet (or one of them with `--virtio-kbd`,
//! `--virtio-mouse`, `--virtio-tablet`), `--virtio-rng` an entropy source,
//! `--virtio-console PATH` a console whose output goes to file PATH, or,
//! if the FIFOs PATH.in and PATH.out exist, connected to them.
//! `--virtio-gpu` replaces the standard VGA with `virtio-vga` (the
//! firmware draws on its VGA side until the kernel's virtio-gpu driver
//! takes over), `--virtio-gpu-pci` with `virtio-gpu-pci` (no firmware
//! framebuffer: the screen starts with the driver). `--virtio-legacy` makes the block, SCSI, entropy
//! and console devices legacy-only (virtio 0.9.5). `--boot-virtio` boots
//! from the first virtio disk (no ESP directory).
//!
//! `--net` adds a network adapter on QEMU's user-mode network (guest
//! 10.0.2.15, gateway/host 10.0.2.2, DNS 10.0.2.3): an Intel 82540EM
//! (`e1000`) unless a model follows (`--net vmxnet3|e1000|82545em|e1000e|pcnet|virtio`:
//! VMware's paravirtual NIC, the 82540EM, the 82545EM of VMware's
//! "e1000", the 82574L of its "e1000e", the AMD PCnet of its "vlance",
//! virtio-net) or `--net-model`
//! names any QEMU device; `--virtio-net` is short for `--net virtio`. Host port 5555 is forwarded to
//! guest port 23 (when free), and each `--hostfwd tcp::HOSTPORT-:GUESTPORT` adds a
//! rule; `--guestfwd tcp:10.0.2.100:79-tcp:127.0.0.1:7979` sends the guest's
//! connections to that address to a host port (servers on ports a user
//! cannot bind); `--tftp DIR` has QEMU serve DIR by TFTP at 10.0.2.2. Without
//! these options the machine has no network adapter.
//!
//! `--sound` adds an ICH9 HD Audio controller with a duplex codec
//! (`ich9-intel-hda`, `hda-duplex`; `hda-output` on CoreAudio, which
//! cannot record) playing through the host's sound output (CoreAudio on
//! macOS; nothing when `--headless`); `--sound ac97` an Intel ICH AC'97
//! controller (`AC97`) and `--sound es1370` (or `es1371`) an Ensoniq
//! AudioPCI ES1370 (`ES1370`: QEMU has no ES1371) in its place;
//! `--sound-wav FILE` records what it plays to a WAV file instead.
//! `--sound-in FILE` gives the card's input a WAV file (looped) or
//! `tone:HZ`, a sine: QEMU's D-Bus audio backend and display, fed by
//! tools/scripts/soundin.py through the QMP socket (no window; with
//! `--sound-wav` the script writes what is played).

mod hpfsimg;
mod image;
mod iso;
mod png;
mod somir;

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
    /// `disk --size MB`: the image's size (64 MB by default).
    disk_mb: u64,
    qemu_extra: Vec<String>,
    /// Keystrokes to send (\n = Enter), and when (seconds after start).
    typing: Option<String>,
    type_at: Option<u64>,
    /// QEMU monitor commands (separated by ';', "sleep N" pauses) to send,
    /// and when.
    monitor: Option<String>,
    monitor_at: Option<u64>,
    /// Boot from this ISO (as a CD) instead of the ESP directory.
    iso: Option<PathBuf>,
    /// Attach this raw disk image as an extra hard disk.
    hdd: Option<PathBuf>,
    /// Attach this raw disk image as an NVMe drive.
    nvme: Option<PathBuf>,
    /// `run`: number of processors.
    smp: u32,
    /// Guest RAM (QEMU -m), "512M" unless --mem.
    mem: String,
    /// `disk`: make an installed-system disk.
    system: bool,
    /// `run`: boot from the --hdd disk only (no ESP directory).
    boot_hdd: bool,
    /// Build the distributable system (no development files).
    dist: bool,
    /// `disk`: an HPFS partition instead of FAT.
    hpfs: bool,
    fat32: bool,
    /// `run`: add an xHCI controller with a USB keyboard / mouse.
    usb_kbd: bool,
    usb_mouse: bool,
    /// `run`: raw images attached as USB mass storage devices.
    usb_disks: Vec<PathBuf>,
    /// `run`: tiers of USB hubs in front of the USB devices.
    usb_hubs: u32,
    /// `run`: VMware SVGA II display and vmport (vmmouse).
    vmware: bool,
    /// `run`: images on a SCSI controller (PVSCSI unless `scsi_hba`).
    pvscsi: Option<PathBuf>,
    pvscsi_cd: Option<PathBuf>,
    scsi_hba: Option<String>,
    /// `run`: virtio devices (see the module documentation).
    virtio_disks: Vec<PathBuf>,
    virtio_scsi: Vec<PathBuf>,
    virtio_cds: Vec<PathBuf>,
    virtio_kbd: bool,
    virtio_mouse: bool,
    virtio_tablet: bool,
    virtio_rng: bool,
    virtio_console: Option<PathBuf>,
    virtio_legacy: bool,
    boot_virtio: bool,
    /// `run`: virtio-gpu display: "virtio-vga" or "virtio-gpu-pci".
    virtio_gpu: Option<&'static str>,
    /// `run`: a NIC on QEMU user networking (device model), extra
    /// host forwarding rules.
    net: Option<String>,
    hostfwd: Vec<String>,
    guestfwd: Vec<String>,
    /// Directory QEMU's user network serves by TFTP (10.0.2.2).
    tftp: Option<String>,
    /// Boot into the Workplace Shell (init=OS2\\PMSHELL.EXE instead of
    /// work/cmdline.txt's init=).
    desktop: bool,
    /// `--init "PROGRAM ARGS"`: boot program in place of work/cmdline.txt's
    /// init= (its other options are kept).
    init: Option<String>,
    /// `run`: an HD Audio controller and codec (`--sound`), playing to
    /// the host (or to this WAV file, `--sound-wav`).
    sound: bool,
    /// The sound card: hda, ac97 or es1370 (`--sound CARD`).
    sound_card: &'static str,
    sound_wav: Option<PathBuf>,
    /// What the sound card records (`--sound-in`).
    sound_in: Option<String>,
}

fn parse(args: &[String]) -> Opts {
    let mut o = Opts {
        release: false,
        headless: false,
        timeout: None,
        screenshot: None,
        output: None,
        disk_mb: 64,
        qemu_extra: vec![],
        typing: None,
        type_at: None,
        monitor: None,
        monitor_at: None,
        iso: None,
        hdd: None,
        nvme: None,
        smp: 2,
        mem: "512M".into(),
        system: false,
        boot_hdd: false,
        dist: false,
        hpfs: false,
        fat32: false,
        usb_kbd: false,
        usb_mouse: false,
        usb_disks: vec![],
        usb_hubs: 0,
        vmware: false,
        pvscsi: None,
        pvscsi_cd: None,
        scsi_hba: None,
        virtio_disks: vec![],
        virtio_scsi: vec![],
        virtio_cds: vec![],
        virtio_kbd: false,
        virtio_mouse: false,
        virtio_tablet: false,
        virtio_rng: false,
        virtio_console: None,
        virtio_legacy: false,
        boot_virtio: false,
        virtio_gpu: None,
        net: None,
        hostfwd: vec![],
        guestfwd: vec![],
        tftp: None,
        desktop: false,
        init: None,
        sound: false,
        sound_card: "hda",
        sound_wav: None,
        sound_in: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--release" => o.release = true,
            "--headless" => o.headless = true,
            "--sound" => {
                o.sound = true;
                let card = match args.get(i + 1).map(String::as_str) {
                    Some("hda") => Some("hda"),
                    Some("ac97") => Some("ac97"),
                    Some("es1370" | "es1371") => Some("es1370"),
                    _ => None,
                };
                if let Some(card) = card {
                    o.sound_card = card;
                    i += 1;
                }
            }
            "--sound-wav" => {
                i += 1;
                o.sound = true;
                o.sound_wav = Some(PathBuf::from(&args[i]));
            }
            "--sound-in" => {
                i += 1;
                o.sound = true;
                o.sound_in = Some(args[i].clone());
            }
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
            "--fat32" => o.fat32 = true,
            "--usb-kbd" => o.usb_kbd = true,
            "--usb-mouse" => o.usb_mouse = true,
            "--usb-hub" => o.usb_hubs += 1,
            "--vmware" => o.vmware = true,
            "--pvscsi" => {
                i += 1;
                o.pvscsi = Some(PathBuf::from(&args[i]));
            }
            "--pvscsi-cd" => {
                i += 1;
                o.pvscsi_cd = Some(PathBuf::from(&args[i]));
            }
            "--scsi-hba" => {
                i += 1;
                o.scsi_hba = Some(args[i].clone());
            }
            "--usb-disk" => {
                i += 1;
                o.usb_disks.push(PathBuf::from(&args[i]));
            }
            "--virtio-disk" => {
                i += 1;
                o.virtio_disks.push(PathBuf::from(&args[i]));
            }
            "--virtio-scsi" => {
                i += 1;
                o.virtio_scsi.push(PathBuf::from(&args[i]));
            }
            "--virtio-cd" => {
                i += 1;
                o.virtio_cds.push(PathBuf::from(&args[i]));
            }
            "--virtio-input" => (o.virtio_kbd, o.virtio_mouse, o.virtio_tablet) = (true, true, true),
            "--virtio-kbd" => o.virtio_kbd = true,
            "--virtio-mouse" => o.virtio_mouse = true,
            "--virtio-tablet" => o.virtio_tablet = true,
            "--virtio-rng" => o.virtio_rng = true,
            "--virtio-console" => {
                i += 1;
                o.virtio_console = Some(PathBuf::from(&args[i]));
            }
            "--virtio-legacy" => o.virtio_legacy = true,
            "--virtio-gpu" => o.virtio_gpu = Some("virtio-vga"),
            "--virtio-gpu-pci" => o.virtio_gpu = Some("virtio-gpu-pci"),
            "--boot-virtio" => o.boot_virtio = true,
            "--net" => {
                // An optional model: `--net vmxnet3`.
                match args.get(i + 1).and_then(|m| nic_model(m)) {
                    Some(m) => {
                        i += 1;
                        o.net = Some(m.into());
                    }
                    None => {
                        o.net.get_or_insert_with(|| "e1000".into());
                    }
                }
            }
            "--virtio-net" => o.net = Some("virtio-net-pci".into()),
            "--net-model" => {
                i += 1;
                o.net = Some(nic_model(&args[i]).map_or_else(|| args[i].clone(), String::from));
            }
            "--tftp" => {
                i += 1;
                o.tftp = Some(args[i].clone());
            }
            "--hostfwd" => {
                i += 1;
                o.hostfwd.push(args[i].clone());
            }
            "--guestfwd" => {
                i += 1;
                o.guestfwd.push(args[i].clone());
            }
            "--hdd" => {
                i += 1;
                o.hdd = Some(PathBuf::from(&args[i]));
            }
            "--nvme" => {
                i += 1;
                o.nvme = Some(PathBuf::from(&args[i]));
            }
            "--mem" => {
                i += 1;
                o.mem = args[i].clone();
            }
            "--smp" => {
                i += 1;
                o.smp = args[i].parse().expect("--smp count");
            }
            "--iso" => {
                i += 1;
                o.iso = Some(PathBuf::from(&args[i]));
            }
            "--type" => {
                i += 1;
                o.typing = Some(args[i].replace("\\n", "\n"));
            }
            // The keys from a file (line ends are not typed; "\n" is Enter).
            "--type-file" => {
                i += 1;
                let t = std::fs::read_to_string(&args[i]).expect("--type-file");
                o.typing = Some(t.lines().collect::<String>().replace("\\n", "\n"));
            }
            "--desktop" => o.desktop = true,
            "--init" => {
                i += 1;
                o.init = Some(args[i].clone());
            }
            "--monitor" => {
                i += 1;
                o.monitor = Some(args[i].clone());
            }
            "--monitor-at" => {
                i += 1;
                o.monitor_at = Some(args[i].parse().expect("--monitor-at seconds"));
            }
            "--type-at" => {
                i += 1;
                o.type_at = Some(args[i].parse().expect("--type-at seconds"));
            }
            "-o" => {
                i += 1;
                o.output = Some(PathBuf::from(&args[i]));
            }
            "--size" => {
                i += 1;
                o.disk_mb = args[i].parse().expect("--size MB");
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
        "ir" => !interface_repositories().is_empty(),
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
                hpfsimg::disk(&files, "RS2HPFS", o.disk_mb)
            } else {
                image::disk(&files, "RS2DISK", o.disk_mb, o.fat32.then_some(fat::FatType::Fat32))
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
    let profile = if release { "release" } else { "checked" };
    c.current_dir(root()).args(["build", "-p", package, "--target", target, "--profile", profile]);
    if !c.status().ok()?.success() {
        return None;
    }
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
    if (o.desktop || o.init.is_some()) && !o.dist {
        let old = std::fs::read_to_string(&cmdline).unwrap_or_default();
        let mut words: Vec<&str> = old.split_whitespace().filter(|w| !w.starts_with("init=")).collect();
        // The kernel takes the arguments after " -- " for init=.
        if let Some(i) = words.iter().position(|&w| w == "--") {
            words.truncate(i);
        }
        let init = match &o.init {
            Some(i) => match i.split_once(' ') {
                Some((prog, args)) => format!("init={prog} -- {args}"),
                None => format!("init={i}"),
            },
            None => "init=OS2\\PMSHELL.EXE".into(),
        };
        words.push(&init);
        std::fs::create_dir_all(esp.join("RS2")).unwrap();
        std::fs::write(esp.join("RS2/CMDLINE"), words.join(" ")).unwrap();
    } else if cmdline.exists() && !o.dist {
        copy(&cmdline, &esp.join("RS2/CMDLINE"));
    }
    // rs2 user programs (Rust, i686) converted to LX.
    let userland = build_userland()?;
    // Drive C: as a FAT image built from work/modules.txt (development)
    // or dist/manifest.txt (distribution: rs2's own files only).
    let list = if o.dist { root().join("dist/manifest.txt") } else { root().join("work/modules.txt") };
    if let Ok(text) = std::fs::read_to_string(&list) {
        // rs2's interface repositories (first, so that a modules.txt line
        // can put another in its place).
        let mut files = interface_repositories();
        files.extend(image::manifest(&root(), &text));
        files.extend(userland);
        files.extend(win16_tests());
        if !o.dist {
            files.extend(dos_tests());
        }
        // The version the installer shows (and an update compares).
        let version = root().join("target/rs2-version.txt");
        std::fs::write(&version, version_text()).unwrap();
        files.push((version, "RS2\\VERSION".to_string()));
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

/// rs2's interface repositories (`\OS2\ETC\SOM.IR`, `WPSH.IR`, ...),
/// written into target/ from userland/somir/*-ir.txt: the files and their
/// paths on the boot volume.
fn interface_repositories() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    for (txt, name) in somir::REPOSITORIES {
        let ir = root().join("target").join(name);
        let src = std::fs::read_to_string(root().join("userland/somir").join(txt)).expect(txt);
        std::fs::write(&ir, somir::build(&src)).unwrap();
        files.push((ir, format!("OS2\\ETC\\{name}")));
    }
    files
}

/// `rs2 COMMIT[+] built YYYY-MM-DD`: the commit the build is from ("+" when
/// the checkout had changes) and the build date (UTC).
fn version_text() -> String {
    let git = |args: &[&str]| {
        Command::new("git").args(args).current_dir(root()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let days = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) / 86400) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("rs2 {commit}{} built {y:04}-{m:02}-{d:02}\r\n", if dirty { "+" } else { "" })
}

/// Programs in userland/ to package, with their LX names.
const USER_PROGRAMS: &[(&str, &str)] =
    &[
        ("apitest", "RS2\\APITEST.EXE"),
        ("stress", "RS2\\STRESS.EXE"),
        ("nettest", "RS2\\NETTEST.EXE"),
        ("gpitest", "RS2\\GPITEST.EXE"),
        ("ctltest", "RS2\\CTLTEST.EXE"),
        ("nbtest", "RS2\\NBTEST.EXE"),
        ("shelltest", "RS2\\SHELLTST.EXE"),
        ("initest", "RS2\\INITEST.EXE"),
        ("somtest", "RS2\\SOMTEST.EXE"),
        ("odtest", "RS2\\ODTEST.EXE"),
        ("odshell", "OS2\\ODSHELL.EXE"),
        ("wpstest", "RS2\\WPSTEST.EXE"),
        ("wpshell", "RS2\\WPSHELL.EXE"),
        ("audtest", "RS2\\AUDTEST.EXE"),
        ("netcli", "RS2\\NETCLI.EXE"),
        ("desktest", "RS2\\DESKTEST.EXE"),
        ("testdll", "RS2\\TESTDLL.DLL"),
        ("pmxtest", "RS2\\PMXTEST.EXE"),
        ("prntest", "RS2\\PRNTEST.EXE"),
        ("pmxhook", "RS2\\PMXHOOK.DLL"),
        ("cmd", "OS2\\CMD.EXE"),
        ("vdm", "OS2\\MDOS\\VDM.EXE"),
        ("more", "OS2\\MORE.EXE"),
        ("find", "OS2\\FIND.EXE"),
        ("sort", "OS2\\SORT.EXE"),
        ("attrib", "OS2\\ATTRIB.EXE"),
        ("tree", "OS2\\TREE.EXE"),
        ("xcopy", "OS2\\XCOPY.EXE"),
        ("sysdlls", "OS2\\SYSDLLS.EXE"),
        ("install", "OS2\\INSTALL.EXE"),
        ("pminstal", "OS2\\PMINSTAL.EXE"),
        ("display", "OS2\\DISPLAY.EXE"),
        ("pmwin", "OS2\\DLL\\PMWIN.DLL"),
        ("pmgpi", "OS2\\DLL\\PMGPI.DLL"),
        ("pmsrv", "OS2\\PMSRV.EXE"),
        ("pmhang", "RS2\\PMHANG.EXE"),
        ("pmbusy", "RS2\\PMBUSY.EXE"),
        ("pmviowin", "OS2\\PMVIOWIN.EXE"),
        ("rs2shell", "OS2\\RS2SHELL.EXE"),
        ("pmshell", "OS2\\PMSHELL.EXE"),
        ("rescue", "OS2\\RESCUE.EXE"),
        ("e", "OS2\\E.EXE"),
        ("pmctls", "OS2\\DLL\\PMCTLS.DLL"),
        ("helpmgr", "OS2\\DLL\\HELPMGR.DLL"),
        ("view", "OS2\\VIEW.EXE"),
        ("appinst", "OS2\\APPINST.EXE"),
        ("hmtest", "RS2\\HMTEST.EXE"),
        ("mmiotest", "RS2\\MMIOTEST.EXE"),
        ("label", "OS2\\LABEL.EXE"),
        ("chkdsk", "OS2\\CHKDSK.EXE"),
        // IBM's name: PMCHKDSK runs `CHKDSK.COM drive /P`.
        ("chkdsk", "OS2\\CHKDSK.COM"),
        ("format", "OS2\\FORMAT.EXE"),
        // IBM's name: PMFORMAT runs `FORMAT.COM d: /P ...`.
        ("format", "OS2\\FORMAT.COM"),
        ("mode", "OS2\\MODE.EXE"),
        ("help", "OS2\\HELP.EXE"),
        ("keyb", "OS2\\KEYB.EXE"),
        ("comp", "OS2\\COMP.EXE"),
        ("ansi", "OS2\\ANSI.EXE"),
        ("eject", "OS2\\EJECT.EXE"),
        ("bldlevel", "OS2\\BLDLEVEL.EXE"),
        ("syslevel", "OS2\\SYSLEVEL.EXE"),
        ("patch", "OS2\\PATCH.EXE"),
        ("setboot", "OS2\\SETBOOT.EXE"),
        ("cache", "OS2\\CACHE.EXE"),
        ("replace", "OS2\\REPLACE.EXE"),
        ("backup", "OS2\\BACKUP.EXE"),
        ("restore", "OS2\\RESTORE.EXE"),
        ("pstat", "OS2\\PSTAT.EXE"),
        ("eautil", "OS2\\EAUTIL.EXE"),
        ("utiltest", "RS2\\UTILTEST.EXE"),
        ("cachetest", "RS2\\CACHETST.EXE"),
        ("lvm", "OS2\\LVM.EXE"),
        ("diskcopy", "OS2\\DISKCOPY.EXE"),
        ("diskcomp", "OS2\\DISKCOMP.EXE"),
        ("print", "OS2\\PRINT.EXE"),
        ("recover", "OS2\\RECOVER.EXE"),
        ("undelete", "OS2\\UNDELETE.EXE"),
        // FDISK was replaced by LVM in Warp 4.5; the old name runs it.
        ("lvm", "OS2\\FDISK.EXE"),
        ("unpack", "OS2\\UNPACK.EXE"),
        ("unpack", "OS2\\UNPACK2.EXE"),
        ("pack", "OS2\\PACK.EXE"),
        ("pack", "OS2\\PACK2.EXE"),
        // IBM's HELP.CMD hands message numbers to HELPMSG.
        ("help", "OS2\\HELPMSG.EXE"),
        ("ping", "OS2\\PING.EXE"),
        ("ifconfig", "OS2\\IFCONFIG.EXE"),
        ("netstat", "OS2\\NETSTAT.EXE"),
        ("route", "OS2\\ROUTE.EXE"),
        ("arp", "OS2\\ARP.EXE"),
        ("hostname", "OS2\\HOSTNAME.EXE"),
        ("host", "OS2\\HOST.EXE"),
        ("ftp", "OS2\\FTP.EXE"),
        ("ftpd", "OS2\\FTPD.EXE"),
        ("telnet", "OS2\\TELNET.EXE"),
        ("telnetd", "OS2\\TELNETD.EXE"),
        ("lpr", "OS2\\LPR.EXE"),
        ("lpq", "OS2\\LPQ.EXE"),
        ("lprm", "OS2\\LPRM.EXE"),
        ("finger", "OS2\\FINGER.EXE"),
        ("pkgsetup", "OS2\\PKGSETUP.EXE"),
        ("tftp", "OS2\\TFTP.EXE"),
        ("tftpd", "OS2\\TFTPD.EXE"),
        ("rexec", "OS2\\REXEC.EXE"),
        ("rsh", "OS2\\RSH.EXE"),
        ("lpd", "OS2\\LPD.EXE"),
        ("rexecd", "OS2\\REXECD.EXE"),
        ("rshd", "OS2\\RSHD.EXE"),
        ("talk", "OS2\\TALK.EXE"),
        ("talkd", "OS2\\TALKD.EXE"),
        ("tcpcfg2", "OS2\\TCPCFG2.EXE"),
        ("net", "OS2\\NET.EXE"),
        ("lantest", "RS2\\LANTEST.EXE"),
        ("netapi32", "OS2\\DLL\\NETAPI32.DLL"),
        ("netapi", "OS2\\DLL\\NETAPI.DLL"),
        ("tcpip32", "OS2\\DLL\\TCPIP32.DLL"),
        ("tnls32", "OS2\\DLL\\TNLS32.DLL"),
        ("pmshapi", "OS2\\DLL\\PMSHAPI.DLL"),
        ("so32dll", "OS2\\DLL\\SO32DLL.DLL"),
        ("tcp32dll", "OS2\\DLL\\TCP32DLL.DLL"),
        ("tcpipdll", "OS2\\DLL\\TCPIPDLL.DLL"),
        ("tnls16", "OS2\\DLL\\TNLS16.DLL"),
        ("pmdrag", "OS2\\DLL\\PMDRAG.DLL"),
        ("pmmerge", "OS2\\DLL\\PMMERGE.DLL"),
        ("registry", "OS2\\DLL\\REGISTRY.DLL"),
        ("pmwinx", "OS2\\DLL\\PMWINX.DLL"),
        ("pmbidi", "OS2\\DLL\\PMBIDI.DLL"),
        ("pmddeml", "OS2\\DLL\\PMDDEML.DLL"),
        ("pmviop", "OS2\\DLL\\PMVIOP.DLL"),
        ("pmgre", "OS2\\DLL\\PMGRE.DLL"),
        ("thailib", "OS2\\DLL\\THAILIB.DLL"),
        ("pmwp", "OS2\\DLL\\PMWP.DLL"),
        ("wpconfig", "OS2\\DLL\\WPCONFIG.DLL"),
        ("wincfg", "OS2\\DLL\\WINCFG.DLL"),
        ("rexx", "OS2\\DLL\\REXX.DLL"),
        ("rexxapi", "OS2\\DLL\\REXXAPI.DLL"),
        ("rexxutil", "OS2\\DLL\\REXXUTIL.DLL"),
        ("rexxinit", "OS2\\DLL\\REXXINIT.DLL"),
        ("rxsock", "OS2\\DLL\\RXSOCK.DLL"),
        // Object REXX (docs/REXX.md): SWITCHRX.CMD makes these REXX.DLL and
        // REXXUTIL.DLL (and REXXSOM, REXXWPS).
        ("orexx", "OS2\\DLL\\OREXX.DLL"),
        ("rexxutil", "OS2\\DLL\\OREXUTIL.DLL"),
        ("rexxsom", "OS2\\DLL\\OREXXSOM.DLL"),
        ("rexxwps", "OS2\\DLL\\OREXXWPS.DLL"),
        ("libuni", "OS2\\DLL\\LIBUNI.DLL"),
        ("uconv", "OS2\\DLL\\UCONV.DLL"),
        ("mmio", "OS2\\DLL\\MMIO.DLL"),
        ("mdm", "OS2\\DLL\\MDM.DLL"),
        ("mciapi", "OS2\\DLL\\MCIAPI.DLL"),
        ("mmioproc", "OS2\\DLL\\MMIOPROC.DLL"),
        ("mppm", "MMOS2\\MPPM.EXE"),
        ("wepm", "MMOS2\\WEPM.EXE"),
        ("cdpm", "MMOS2\\CDPM.EXE"),
        ("mmvolume", "MMOS2\\MMVOLUME.EXE"),
        ("stpm", "MMOS2\\STPM.EXE"),
        ("pmspl", "OS2\\DLL\\PMSPL.DLL"),
        ("pmspool", "OS2\\PMSPOOL.EXE"),
        ("pmprint", "OS2\\DLL\\PMPRINT.QPR"),
        ("som", "OS2\\DLL\\SOM.DLL"),
        ("somtc", "OS2\\DLL\\SOMTC.DLL"),
        ("somu", "OS2\\DLL\\SOMU.DLL"),
        ("somir", "OS2\\DLL\\SOMIR.DLL"),
        ("somd", "OS2\\DLL\\SOMD.DLL"),
        ("somuc", "OS2\\DLL\\SOMUC.DLL"),
        ("opendoc", "OS2\\DLL\\OPENDOC.DLL"),
        ("win16", "OS2\\DLL\\WIN16.DLL"),
        // rs2's own PM applets (userland/pmkit is what they share).
        ("clipos2", "OS2\\CLIPOS2.EXE"),
        ("mpcpumon", "OS2\\MPCPUMON.EXE"),
        ("pmchkdsk", "OS2\\PMCHKDSK.EXE"),
        ("pmformat", "OS2\\PMFORMAT.EXE"),
        ("iconedit", "OS2\\ICONEDIT.EXE"),
        ("glocale", "OS2\\GLOCALE.EXE"),
        ("picview", "OS2\\APPS\\PICVIEW.EXE"),
        ("pmseek", "OS2\\APPS\\PMSEEK.EXE"),
        ("pkgmgr", "OS2\\APPS\\PKGMGR.EXE"),
        ("klondike", "OS2\\APPS\\KLONDIKE.EXE"),
        ("mahjongg", "OS2\\APPS\\MAHJONGG.EXE"),
        ("os2chess", "OS2\\APPS\\OS2CHESS.EXE"),
        ("rxqueue", "OS2\\RXQUEUE.EXE"),
        ("rxsubcom", "OS2\\RXSUBCOM.EXE"),
        ("rexxc", "OS2\\REXXC.EXE"),
        ("pmrexxio", "OS2\\DLL\\PMREXXIO.DLL"),
        ("pmrexx", "OS2\\PMREXX.EXE"),
    ];

/// Programs whose LX module name is not their file name: Object REXX's
/// files are the modules REXX, REXXUTIL, REXXSOM and REXXWPS (IBM's are so)
/// and become those files when SWITCHRX.CMD switches.
const LX_NAMES: &[(&str, &str)] = &[
    ("OS2\\DLL\\OREXX.DLL", "REXX"),
    ("OS2\\DLL\\OREXUTIL.DLL", "REXXUTIL"),
    ("OS2\\DLL\\OREXXSOM.DLL", "REXXSOM"),
    ("OS2\\DLL\\OREXXWPS.DLL", "REXXWPS"),
];

/// Help files compiled from IPF source in userland/, with their names.
/// Names ending in .INF are books.
const HELP_FILES: &[(&str, &str)] = &[
    ("e/E.IPF", "OS2\\HELP\\E.HLP"),
    ("hmtest/HMTEST.IPF", "OS2\\HELP\\HMTEST.HLP"),
    ("helpmgr/HMHELP.IPF", "OS2\\HELP\\HMHELP.HLP"),
    ("help/WPHELP.IPF", "OS2\\HELP\\WPHELP.HLP"),
    ("help/CMDREF.IPF", "OS2\\BOOK\\CMDREF.INF"),
    ("help/START.IPF", "OS2\\BOOK\\START.INF"),
    ("clipos2/CLIPOS2.IPF", "OS2\\HELP\\CLIPOS2.HLP"),
    ("mpcpumon/MPCPUMON.IPF", "OS2\\HELP\\MPCPUMON.HLP"),
    ("pmchkdsk/PMCHKDSK.IPF", "OS2\\HELP\\PMCHKDSK.HLP"),
    ("pmformat/PMFORMAT.IPF", "OS2\\HELP\\PMFORMAT.HLP"),
    ("iconedit/ICONEDIT.IPF", "OS2\\HELP\\ICONEDIT.HLP"),
    ("glocale/GLOCALE.IPF", "OS2\\HELP\\GLOCALE.HLP"),
    ("picview/PICVIEW.IPF", "OS2\\HELP\\PICVIEW.HLP"),
    ("pmseek/PMSEEK.IPF", "OS2\\HELP\\PMSEEK.HLP"),
    ("klondike/KLONDIKE.IPF", "OS2\\HELP\\KLONDIKE.HLP"),
    ("mahjongg/MAHJONGG.IPF", "OS2\\HELP\\MAHJONGG.HLP"),
    ("os2chess/OS2CHESS.IPF", "OS2\\HELP\\OS2CHESS.HLP"),
    ("pmrexxio/PMREXXIO.IPF", "OS2\\HELP\\PMREXXIO.HLP"),
];

/// Message files compiled from MKMSGF source in userland/msg (crates/msgf):
/// source, name on the disk, code pages, language family and sublanguage.
const MSG_FILES: &[(&str, &str, &[u16], (u16, u16))] = &[
    ("OSO001.TXT", "OS2\\SYSTEM\\OSO001.MSG", &[850, 437], (9, 1)),
    ("NET.TXT", "OS2\\SYSTEM\\NET.MSG", &[850, 437], (9, 1)),
    ("OSO001H.TXT", "OS2\\SYSTEM\\OSO001H.MSG", &[850, 437], (9, 1)),
    ("PMWP.TXT", "OS2\\SYSTEM\\PMWP.MSG", &[850], (0, 0)),
    ("REGISTRY.TXT", "OS2\\SYSTEM\\REGISTRY.MSG", &[850], (9, 1)),
    ("PMCTL.TXT", "OS2\\SYSTEM\\PMCTL.MSG", &[850], (0, 0)),
    ("SYSLEVEL.TXT", "OS2\\SYSLEVEL.MSG", &[850, 437], (0, 0)),
    ("XDF.TXT", "OS2\\XDF.MSG", &[850, 437], (0, 0)),
    ("XDFH.TXT", "OS2\\XDFH.MSG", &[850, 437], (0, 0)),
    ("JFS.TXT", "OS2\\JFS.MSG", &[850, 437], (9, 1)),
    ("JFSH.TXT", "OS2\\JFSH.MSG", &[850, 437], (9, 1)),
    ("PSERROR.TXT", "OS2\\PSERROR.MSG", &[850], (9, 1)),
    ("PSACT.TXT", "OS2\\PSACT.MSG", &[850], (9, 1)),
    ("PSCAUSE.TXT", "OS2\\PSCAUSE.MSG", &[850], (9, 1)),
    ("ULSERR01.TXT", "OS2\\SYSTEM\\ULSERR01.MSG", &[850], (0, 0)),
    ("ULSACT01.TXT", "OS2\\SYSTEM\\ULSACT01.MSG", &[850], (0, 0)),
    ("ULSCAU01.TXT", "OS2\\SYSTEM\\ULSCAU01.MSG", &[850], (0, 0)),
    ("ULSDET01.TXT", "OS2\\SYSTEM\\ULSDET01.MSG", &[850], (0, 0)),
    ("UCDFS.TXT", "OS2\\SYSTEM\\UCDFS.MSG", &[850], (0, 0)),
    ("REX.TXT", "OS2\\SYSTEM\\REX.MSG", &[437], (0, 0)),
    ("REXH.TXT", "OS2\\SYSTEM\\REXH.MSG", &[437], (0, 0)),
    // Object REXX's, REX.MSG and REXH.MSG once SWITCHRX.CMD switches.
    ("OREX.TXT", "OS2\\SYSTEM\\OREX.MSG", &[437], (0, 0)),
    ("OREXH.TXT", "OS2\\SYSTEM\\OREXH.MSG", &[437], (0, 0)),
];

/// The applets' data files in userland/, with their names.
const APP_DATA: &[(&str, &str)] = &[
    ("mahjongg/layouts/DEFAULT.MAH", "OS2\\APPS\\DEFAULT.MAH"),
    ("mahjongg/layouts/PYRAMID.MAH", "OS2\\APPS\\PYRAMID.MAH"),
    ("mahjongg/layouts/FORTRESS.MAH", "OS2\\APPS\\FORTRESS.MAH"),
    ("mahjongg/layouts/TWINPEAK.MAH", "OS2\\APPS\\TWINPEAK.MAH"),
];

/// Programs marked as PM applications (WINDOWAPI).
const PM_PROGRAMS: &[&str] = &["mppm", "wepm", "cdpm", "mmvolume", "stpm", "pmsrv", "pmhang", "pmbusy", "pmviowin", "rs2shell", "e", "view", "appinst", "pminstal", "hmtest", "display", "wpshell", "pmshell", "pmspool", "tcpcfg2", "clipos2", "mpcpumon", "pmchkdsk", "pmformat", "iconedit", "glocale", "picview", "pmseek", "pkgmgr", "klondike", "mahjongg", "os2chess", "pmrexx", "odshell"];

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
        if PM_PROGRAMS.contains(bin) {
            cmd.arg("--pm");
        }
        // The program's icon (crates/pmwin appicons) as RT_POINTER 1.
        let stem = dest.rsplit('\\').next().unwrap().split('.').next().unwrap();
        if dest.ends_with(".EXE") {
            if let Some(ico) = pmwin::appicons::icon_file(stem) {
                let path = out_dir.join(format!("{stem}.ICO"));
                std::fs::write(&path, ico).unwrap();
                cmd.arg("--res").arg("1").arg("1").arg(&path);
            }
        }
        // Object REXX's files have the module names of the files they
        // replace when SWITCHRX.CMD makes Object REXX the system's REXX.
        if let Some((_, n)) = LX_NAMES.iter().find(|(d, _)| d == dest) {
            cmd.arg("--name").arg(n);
        }
        if dest.ends_with(".DLL") || dest.ends_with(".QPR") || dest.ends_with(".DRV") {
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
        // TESTDLL again, marked as having its internal fixups removed:
        // the loader must still move it, its bases being taken by TESTDLL
        // (APITEST; IBM DLLs such as VisualAge's EMNTV50 are built so).
        if *bin == "testdll" {
            let lx = out_dir.join("TESTDLLB.DLL");
            let ok = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
                .current_dir(root())
                .args(["run", "-q", "--release", "-p", "elf2lx", "--"])
                .arg(&elf)
                .arg(&lx)
                .args(["--dll", "--fixups-removed", "--name", "TESTDLLB", "--exports"])
                .arg(ul.join(bin).join("exports.def"))
                .status()
                .ok()?
                .success();
            if !ok {
                return None;
            }
            files.push((lx, "RS2\\TESTDLLB.DLL".to_string()));
            // And marked for high memory (OBJHIGHMEM): it loads above
            // 512 MB (APITEST).
            let lx = out_dir.join("TESTDLLH.DLL");
            let ok = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
                .current_dir(root())
                .args(["run", "-q", "--release", "-p", "elf2lx", "--"])
                .arg(&elf)
                .arg(&lx)
                .args(["--dll", "--highmem", "--name", "TESTDLLH", "--exports"])
                .arg(ul.join(bin).join("exports.def"))
                .status()
                .ok()?
                .success();
            if !ok {
                return None;
            }
            files.push((lx, "RS2\\TESTDLLH.DLL".to_string()));
            // And with global initialisation and no termination routine, as
            // many DLLs of other compilers are (APITEST).
            let lx = out_dir.join("TESTDLLN.DLL");
            let ok = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
                .current_dir(root())
                .args(["run", "-q", "--release", "-p", "elf2lx", "--"])
                .arg(&elf)
                .arg(&lx)
                .args(["--dll", "--global-init", "--name", "TESTDLLN", "--exports"])
                .arg(ul.join(bin).join("exports.def"))
                .status()
                .ok()?
                .success();
            if !ok {
                return None;
            }
            files.push((lx, "RS2\\TESTDLLN.DLL".to_string()));
        }
    }
    check_dll_ranges(&ul)?;
    // rs2's COMMAND.COM for DOS sessions: the stub of crates/dos.
    let command = out_dir.join("COMMAND.COM");
    std::fs::write(&command, dos::shell::command_com()).unwrap();
    files.push((command, "OS2\\MDOS\\COMMAND.COM".to_string()));
    for (src, dest) in HELP_FILES {
        let path = ul.join(src);
        let text = std::fs::read(&path).ok()?;
        // `:artwork.` files: beside the IPF source, or made here.
        let dir = path.parent().unwrap().to_path_buf();
        let load = |name: &[u8]| -> Option<Vec<u8>> {
            let name = String::from_utf8_lossy(name).into_owned();
            if name.eq_ignore_ascii_case("HMART.MET") {
                return Some(hmtest_metafile());
            }
            std::fs::read(dir.join(&name)).ok()
        };
        let help = match ipf::compile::compile_with(&text, !dest.ends_with(".INF"), &load) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return None;
            }
        };
        let out = out_dir.join(dest.rsplit('\\').next().unwrap());
        std::fs::write(&out, help).unwrap();
        files.push((out, dest.to_string()));
    }
    for (src, dest, cps, lang) in MSG_FILES {
        let path = ul.join("msg").join(src);
        let text = std::fs::read(&path).ok()?;
        let name = dest.rsplit('\\').next().unwrap();
        let opts = msgf::Options { code_pages: cps.to_vec(), language: *lang, dbcs: Vec::new(), name: name.to_ascii_lowercase().into_bytes() };
        let msg = match msgf::compile(&text, &opts) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return None;
            }
        };
        let out = out_dir.join(name);
        std::fs::write(&out, msg).unwrap();
        files.push((out, dest.to_string()));
    }
    let ini = out_dir.join("MMPMMMIO.INI");
    std::fs::write(&ini, mmio_ini(&std::fs::read_to_string(ul.join("mmio/ioprocs.txt")).ok()?)).unwrap();
    files.push((ini, "MMOS2\\MMPMMMIO.INI".to_string()));
    for (src, dest) in APP_DATA {
        files.push((ul.join(src), dest.to_string()));
    }
    files.extend(build_userland_std()?);
    Some(files)
}

/// DLLs that share an address range on purpose: only one of each pair is
/// loaded (Object REXX replaces REXX).
const DLL_SHARED_RANGES: &[(&str, &str)] = &[("rexx", "orexx")];

/// The DLLs' images are linked at fixed bases (each build.rs): fail the
/// build when one has grown into the next, which otherwise breaks only at
/// run time in a process that loads both (PMWP into PMSPL, TCPIPDLL into
/// PMSHAPI).
fn check_dll_ranges(ul: &Path) -> Option<()> {
    let mut ranges = Vec::new();
    for (bin, dest) in USER_PROGRAMS {
        // A crate installed under two names (REXXUTIL, OREXUTIL) once.
        if !(dest.ends_with(".DLL") || dest.ends_with(".QPR") || dest.ends_with(".DRV")) || ranges.iter().any(|r: &(u32, u32, &str)| r.2 == *bin) {
            continue;
        }
        let elf = std::fs::read(ul.join("target/i686-unknown-linux-gnu/release").join(bin)).ok()?;
        if let Some((start, end)) = elf32_alloc_range(&elf) {
            ranges.push((start, end, *bin));
        }
    }
    ranges.sort();
    let mut ok = true;
    // The range reaching highest so far, and whose it is.
    let mut top = (0, 0, "");
    for &(s, e, b) in &ranges {
        let (s0, e0, a) = top;
        let shared = DLL_SHARED_RANGES.iter().any(|&(x, y)| (x, y) == (a, b) || (y, x) == (a, b));
        if e0 > s && !(shared && s0 == s) {
            eprintln!("{a}.dll ({s0:#x}..{e0:#x}) overlaps {b}.dll at {s:#x}: move one of them (__image_base in its build.rs)");
            ok = false;
        }
        if e > e0 {
            top = (s, e, b);
        }
    }
    ok.then_some(())
}

/// The lowest and highest addresses of an ELF32 file's allocated sections.
fn elf32_alloc_range(elf: &[u8]) -> Option<(u32, u32)> {
    let u16_at = |o: usize| Some(u16::from_le_bytes(elf.get(o..o + 2)?.try_into().ok()?));
    let u32_at = |o: usize| Some(u32::from_le_bytes(elf.get(o..o + 4)?.try_into().ok()?));
    if elf.get(..5)? != b"\x7fELF\x01" {
        return None;
    }
    let (shoff, shentsize, shnum) = (u32_at(0x20)? as usize, u16_at(0x2e)? as usize, u16_at(0x30)? as usize);
    let (mut lo, mut hi) = (u32::MAX, 0);
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        const SHF_ALLOC: u32 = 2;
        let (flags, addr, size) = (u32_at(sh + 8)?, u32_at(sh + 12)?, u32_at(sh + 20)?);
        if flags & SHF_ALLOC != 0 && addr != 0 {
            lo = lo.min(addr);
            hi = hi.max(addr + size);
        }
    }
    (lo < hi).then_some((lo, hi))
}

/// Programs in userland-std/ (Rust's std on rs2libc, docs/BROWSER.md):
/// cargo package, LX name, PM program.
const STD_PROGRAMS: &[(&str, &str, bool)] = &[
    ("stdtest", "RS2\\STDTEST.EXE", false),
    ("webtext", "OS2\\APPS\\WEBTEXT.EXE", false),
    ("web", "OS2\\APPS\\WEB.EXE", true),
];

/// Build the userland-std workspace and convert its programs to LX.
fn build_userland_std() -> Option<image::FileList> {
    let dir = root().join("userland-std");
    let status = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .current_dir(&dir)
        .args(["build", "--release"])
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let out_dir = root().join("work/userland");
    let mut files = Vec::new();
    for (bin, dest, pm) in STD_PROGRAMS {
        let elf = dir.join("target/i686-unknown-linux-gnu/release").join(bin);
        let lx = out_dir.join(dest.rsplit('\\').next().unwrap());
        let mut cmd = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
        // A 4 MB stack for thread 1 (Linux gives programs 8 MB).
        cmd.current_dir(root())
            .args(["run", "-q", "--release", "-p", "elf2lx", "--"])
            .arg(&elf)
            .arg(&lx)
            .args(["--stack", "4194304"]);
        if *pm {
            cmd.arg("--pm");
        }
        let stem = dest.rsplit('\\').next().unwrap().split('.').next().unwrap();
        if let Some(ico) = pmwin::appicons::icon_file(stem) {
            let path = out_dir.join(format!("{stem}.ICO"));
            std::fs::write(&path, ico).unwrap();
            cmd.arg("--res").arg("1").arg("1").arg(&path);
        }
        if !cmd.status().ok()?.success() {
            return None;
        }
        files.push((lx, dest.to_string()));
    }
    Some(files)
}

/// MMIO's list of file-format I/O procedures (MMOS2\MMPMMMIO.INI, as
/// IBM's installer writes it: `MMIOINIT` `IOCOUNT` and `IOMODINFO`, an
/// array of 316-byte MMINIFILEINFO records) from userland/mmio/ioprocs.txt.
fn mmio_ini(list: &str) -> Vec<u8> {
    let mut data = Vec::new();
    let mut count = 0u16;
    for line in list.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.len() < 6 {
            continue;
        }
        let mut rec = [0u8; 316];
        let text = |rec: &mut [u8; 316], off: usize, s: &str| rec[off..off + s.len()].copy_from_slice(s.as_bytes());
        text(&mut rec, 0, &w[0].replace('_', " "));
        text(&mut rec, 4, w[1]);
        text(&mut rec, 264, w[2]);
        rec[296..300].copy_from_slice(&w[3].parse::<u32>().unwrap().to_le_bytes());
        rec[300..304].copy_from_slice(&16u32.to_le_bytes());
        rec[304..308].copy_from_slice(&w[4].parse::<u32>().unwrap().to_le_bytes());
        rec[308..312].copy_from_slice(&2u32.to_le_bytes());
        text(&mut rec, 312, w[5]);
        data.extend_from_slice(&rec);
        count += 1;
    }
    let mut prof = inifile::Profile::default();
    prof.set(b"MMIOINIT", b"VERSION", b"MULTIMEDIA MMIO INI FILE VERSION 1.0\0").unwrap();
    prof.set(b"MMIOINIT", b"IOCOUNT", &count.to_le_bytes()).unwrap();
    prof.set(b"MMIOINIT", b"IOMODINFO", &data).unwrap();
    prof.to_bytes()
}

/// The DOS test programs (tests/dos, assembled when clang is there) as
/// C:\\DOS\\*.
fn dos_tests() -> image::FileList {
    let dir = root().join("tests/dos");
    let _ = Command::new("sh").arg(dir.join("build.sh")).status();
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("bin")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_uppercase();
            v.push((e.path(), format!("DOS\\{name}")));
        }
    }
    v
}

/// The Windows 3.x test programs (userland/win16test, built with Open
/// Watcom when it is installed) as C:\\RS2\\*.EXE.
fn win16_tests() -> image::FileList {
    let dir = root().join("userland/win16test");
    let _ = Command::new("sh").arg(dir.join("build.sh")).status();
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("bin")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_uppercase();
            v.push((e.path(), format!("RS2\\{name}")));
        }
    }
    v
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

/// QEMU's UEFI firmware (OVMF): a code image and a variable store, from
/// `$RS2_OVMF_DIR` or the usual places: QEMU's own copies (Homebrew, most
/// Linux QEMU packages, QEMU's source tree `pc-bios/`) or the
/// distributions' OVMF packages (Debian/Ubuntu `ovmf`, Fedora and Arch
/// `edk2-ovmf`). Only non-Secure-Boot builds are picked.
fn ovmf() -> (PathBuf, PathBuf) {
    const PAIRS: [(&str, &str); 4] = [
        ("edk2-x86_64-code.fd", "edk2-i386-vars.fd"),
        ("OVMF_CODE_4M.fd", "OVMF_VARS_4M.fd"),
        ("OVMF_CODE.4m.fd", "OVMF_VARS.4m.fd"),
        ("OVMF_CODE.fd", "OVMF_VARS.fd"),
    ];
    let dirs: Vec<PathBuf> = std::env::var_os("RS2_OVMF_DIR")
        .map(PathBuf::from)
        .into_iter()
        .chain(
            [
                "/opt/homebrew/share/qemu",
                "/usr/local/share/qemu",
                "/usr/share/qemu",
                "/usr/share/OVMF",
                "/usr/share/edk2/ovmf",
                "/usr/share/edk2/x64",
                "/usr/share/edk2-ovmf/x64",
            ]
            .map(PathBuf::from),
        )
        .collect();
    let found = dirs.iter().find_map(|d| {
        PAIRS.iter().find(|(c, v)| d.join(c).exists() && d.join(v).exists()).map(|(c, v)| (d.join(c), d.join(v)))
    });
    let Some((code, vars_src)) = found else {
        let tried: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        panic!(
            "QEMU's UEFI firmware (OVMF) not found in {}.\nInstall QEMU (brew install qemu) or your distribution's OVMF package, \
             or set RS2_OVMF_DIR to a folder with edk2-x86_64-code.fd and edk2-i386-vars.fd (README, Requirements).",
            tried.join(", ")
        );
    };
    // A fresh variable store every run, so stale boot entries from other
    // configurations never get in the way.
    let vars = root().join("work/ovmf-vars.fd");
    copy(&vars_src, &vars);
    (code, vars)
}

/// The xHCI controller, hubs and USB devices.
fn usb_devices(o: &Opts, c: &mut Command) {
    if !o.usb_kbd && !o.usb_mouse && o.usb_disks.is_empty() && o.usb_hubs == 0 {
        return;
    }
    c.args(["-device", "qemu-xhci,id=xhci"]);
    // Port path of the hub the devices go behind ("" = root ports).
    let mut hub = String::new();
    for n in 0..o.usb_hubs {
        let port = if n == 0 { "1".to_string() } else { format!("{hub}.8") };
        c.arg("-device").arg(format!("usb-hub,bus=xhci.0,port={port},id=hub{n}"));
        hub = port;
    }
    let mut next = 0;
    let mut port = || {
        next += 1;
        if hub.is_empty() { String::new() } else { format!(",port={hub}.{next}") }
    };
    if o.usb_kbd {
        c.arg("-device").arg(format!("usb-kbd,bus=xhci.0,id=usbkbd{}", port()));
    }
    if o.usb_mouse {
        c.arg("-device").arg(format!("usb-mouse,bus=xhci.0,id=usbmouse{}", port()));
    }
    for (n, d) in o.usb_disks.iter().enumerate() {
        c.arg("-drive").arg(format!("if=none,id=ud{n},format=raw,file={}", d.display()));
        c.arg("-device").arg(format!("usb-storage,bus=xhci.0,drive=ud{n},id=usbdisk{n}{}", port()));
    }
}

/// The virtio devices.
fn virtio_devices(o: &Opts, c: &mut Command) {
    // Legacy-only (0.9.5) devices for the transport's legacy path.
    let legacy = if o.virtio_legacy { ",disable-modern=on,disable-legacy=off" } else { "" };
    for (n, d) in o.virtio_disks.iter().enumerate() {
        let boot = if n == 0 && o.boot_virtio { ",bootindex=0" } else { "" };
        c.arg("-drive").arg(format!("file={},if=none,id=vd{n},format=raw", d.display()));
        c.arg("-device").arg(format!("virtio-blk-pci,drive=vd{n},id=vblk{n}{boot}{legacy}"));
    }
    if !o.virtio_scsi.is_empty() || !o.virtio_cds.is_empty() {
        c.arg("-device").arg(format!("virtio-scsi-pci,id=vscsi{legacy}"));
        for (n, d) in o.virtio_scsi.iter().enumerate() {
            c.arg("-drive").arg(format!("file={},if=none,id=vs{n},format=raw", d.display()));
            c.arg("-device").arg(format!("scsi-hd,bus=vscsi.0,drive=vs{n},id=vsdisk{n}"));
        }
        for (n, d) in o.virtio_cds.iter().enumerate() {
            c.arg("-drive").arg(format!("file={},if=none,id=vc{n},format=raw,media=cdrom,readonly=on", d.display()));
            c.arg("-device").arg(format!("scsi-cd,bus=vscsi.0,drive=vc{n},id=vscd{n}"));
        }
    }
    // The keyboard last, so QEMU's sendkey goes to it.
    if o.virtio_mouse {
        c.args(["-device", "virtio-mouse-pci,id=vmouse"]);
    }
    if o.virtio_tablet {
        c.args(["-device", "virtio-tablet-pci,id=vtablet"]);
    }
    if o.virtio_kbd {
        c.args(["-device", "virtio-keyboard-pci,id=vkbd"]);
    }
    if let Some(model) = o.virtio_gpu {
        c.args(["-vga", "none", "-device", model]);
    }
    if o.virtio_rng {
        c.arg("-device").arg(format!("virtio-rng-pci{legacy}"));
    }
    if let Some(out) = &o.virtio_console {
        let fifos = Path::new(&format!("{}.in", out.display())).exists();
        let backend = if fifos { "pipe" } else { "file" };
        c.arg("-chardev").arg(format!("{backend},id=vcon,path={}", out.display()));
        c.arg("-device").arg(format!("virtio-serial-pci{legacy}"));
        c.args(["-device", "virtconsole,chardev=vcon"]);
    }
}

/// QEMU's device for a `--net` model name.
fn nic_model(m: &str) -> Option<&'static str> {
    Some(match m {
        "e1000" | "82540em" => "e1000",
        "82545em" | "e1000-82545em" => "e1000-82545em",
        "e1000e" | "82574l" => "e1000e",
        "vmxnet3" => "vmxnet3",
        "pcnet" | "vlance" => "pcnet",
        "virtio" | "virtio-net" | "virtio-net-pci" => "virtio-net-pci",
        _ => return None,
    })
}

/// A NIC on QEMU's user-mode network, or none at all (QEMU would add
/// its default adapter otherwise).
fn net_devices(o: &Opts, c: &mut Command) {
    let Some(model) = &o.net else {
        c.args(["-nic", "none"]);
        return;
    };
    // Host port 5555 to guest port 23, unless another machine (a parallel
    // run) has the port already.
    let mut netdev = String::from("user,id=n0");
    if std::net::TcpListener::bind("0.0.0.0:5555").is_ok() {
        netdev.push_str(",hostfwd=tcp::5555-:23");
    } else {
        eprintln!("xtask: host port 5555 is in use; not forwarding it to guest port 23");
    }
    for r in &o.hostfwd {
        netdev.push_str(",hostfwd=");
        netdev.push_str(r);
    }
    for r in &o.guestfwd {
        netdev.push_str(",guestfwd=");
        netdev.push_str(r);
    }
    if let Some(d) = &o.tftp {
        netdev.push_str(",tftp=");
        netdev.push_str(d);
    }
    c.arg("-netdev").arg(netdev);
    let legacy = if o.virtio_legacy && model.starts_with("virtio") { ",disable-modern=on,disable-legacy=off" } else { "" };
    c.arg("-device").arg(format!("{model},netdev=n0{legacy}"));
}

/// A QEMU socket in work/, or in the temporary directory (named after
/// work/'s path) when that path is too long for a Unix socket (deep
/// checkouts such as git worktrees).
fn socket_path(work: &Path, name: &str) -> PathBuf {
    let p = work.join(name);
    if p.as_os_str().len() < 100 {
        return p;
    }
    let hash = work.to_string_lossy().bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    std::env::temp_dir().join(format!("rs2-{hash:016x}-{name}"))
}

/// `--sound`: ICH9 HD Audio with a codec (or AC'97, or an ES1370), to
/// the host's sound output (none when headless) or to a WAV file.
fn sound_devices(o: &Opts, c: &mut Command) {
    if !o.sound {
        return;
    }
    let backend = match &o.sound_wav {
        _ if o.sound_in.is_some() => "dbus,id=snd0,in.frequency=48000,in.channels=2".to_string(),
        Some(f) => format!("wav,id=snd0,path={}", f.display()),
        None if o.headless => "none,id=snd0".to_string(),
        None if cfg!(target_os = "macos") => "coreaudio,id=snd0".to_string(),
        None => "pa,id=snd0".to_string(),
    };
    c.arg("-audiodev").arg(&backend);
    match o.sound_card {
        "ac97" => {
            c.args(["-device", "AC97,audiodev=snd0"]);
        }
        "es1370" => {
            c.args(["-device", "ES1370,audiodev=snd0"]);
        }
        _ => {
            // QEMU's CoreAudio driver cannot record: an output-only codec
            // there.
            let codec = if backend.starts_with("coreaudio") { "hda-output" } else { "hda-duplex" };
            c.args(["-device", "ich9-intel-hda", "-device"]).arg(format!("{codec},audiodev=snd0"));
        }
    }
}

fn run(o: &Opts) -> bool {
    let (code, vars) = ovmf();
    let work = root().join("work");
    let serial = work.join("serial.log");
    let sock = socket_path(&work, "qemu-monitor.sock");
    let _ = std::fs::remove_file(&sock);
    // QMP, for input events HMP cannot send (absolute pointer positions).
    let qmp_sock = socket_path(&work, "qemu-qmp.sock");
    let _ = std::fs::remove_file(&qmp_sock);
    let mut c = Command::new("qemu-system-x86_64");
    c.args(["-machine", "q35", "-cpu", "max", "-m", &o.mem, "-no-reboot", "-smp", &o.smp.to_string()])
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,readonly=on,file={}", code.display()))
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars.display()))
        .args(match &o.iso {
            Some(iso) => vec!["-cdrom".to_string(), iso.display().to_string()],
            None if o.boot_hdd || o.boot_virtio => vec![],
            // A bootable virtio CD would come first in the firmware's boot
            // order: give the ESP directory boot index 0.
            None if !o.virtio_cds.is_empty() => vec![
                "-drive".to_string(),
                format!("if=none,id=esp,format=raw,file=fat:rw:{}", work.join("esp").display()),
                "-device".to_string(),
                "ide-hd,drive=esp,bootindex=0".to_string(),
            ],
            None => vec!["-drive".to_string(), format!("format=raw,file=fat:rw:{}", work.join("esp").display())],
        })
        .arg("-monitor")
        .arg(format!("unix:{},server,nowait", sock.display()))
        .arg("-qmp")
        .arg(format!("unix:{},server,nowait", qmp_sock.display()));
    if o.sound_in.is_some() {
        // The D-Bus audio backend needs the D-Bus display.
        c.args(["-display", "dbus,p2p=yes,audiodev=snd0"]);
    }
    if o.headless {
        if o.sound_in.is_none() {
            c.args(["-display", "none"]);
        }
        c.arg("-serial").arg(format!("file:{}", serial.display()));
    } else {
        c.args(["-serial", "stdio"]);
    }
    if let Some(d) = &o.hdd {
        c.arg("-drive").arg(format!("format=raw,file={}", d.display()));
    }
    if let Some(d) = &o.nvme {
        c.arg("-drive").arg(format!("file={},format=raw,if=none,id=nvm0", d.display()));
        c.args(["-device", "nvme,drive=nvm0,serial=rs2nvme"]);
    }
    usb_devices(o, &mut c);
    vmware_devices(o, &mut c);
    virtio_devices(o, &mut c);
    net_devices(o, &mut c);
    sound_devices(o, &mut c);
    // Last, so extra devices can refer to the controllers above.
    c.args(&o.qemu_extra);
    let mut child = c.spawn().expect("start qemu");
    let mut feeder = o.sound_in.as_ref().map(|spec| {
        let mut f = Command::new("python3");
        f.arg("-I").arg(root().join("tools/scripts/soundin.py")).arg(&qmp_sock).arg(spec);
        if let Some(w) = &o.sound_wav {
            f.arg(w);
        }
        f.spawn().expect("start tools/scripts/soundin.py")
    });
    let Some(timeout) = o.timeout else {
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        if let Some(f) = feeder.as_mut() {
            let _ = f.kill();
        }
        return ok;
    };
    let start = Instant::now();
    let deadline = start + Duration::from_secs(timeout);
    let mut typed = o.typing.is_none();
    let type_at = o.type_at.unwrap_or(timeout.saturating_sub(8));
    let mut monitored = o.monitor.is_none();
    while Instant::now() < deadline {
        if !typed && start.elapsed() >= Duration::from_secs(type_at) {
            typed = true;
            for key in keys(o.typing.as_deref().unwrap()) {
                if key == "sleep" {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                // {sleep N}: N seconds.
                if let Some(n) = key.strip_prefix("sleep ").and_then(|n| n.parse::<f64>().ok()) {
                    std::thread::sleep(Duration::from_secs_f64(n));
                    continue;
                }
                // {shot NAME}: a screenshot now, as work/NAME.png.
                if let Some(name) = key.strip_prefix("shot ") {
                    let ppm = work.join(format!("{name}.ppm"));
                    monitor(&sock, &format!("screendump {}", ppm.display()));
                    std::thread::sleep(Duration::from_millis(500));
                    match std::fs::read(&ppm) {
                        Ok(data) => png::ppm_to_png(&data, &work.join(format!("{name}.png"))),
                        Err(e) => eprintln!("no screendump: {e}"),
                    }
                    let _ = std::fs::remove_file(&ppm);
                    continue;
                }
                // Absolute pointer position: {mouse_abs x y}, 0..32767
                // across the screen (tablets).
                if let Some(xy) = key.strip_prefix("mouse_abs ") {
                    let v: Vec<&str> = xy.split_whitespace().collect();
                    let ev = |axis: &str, val: &str| format!(r#"{{"type":"abs","data":{{"axis":"{axis}","value":{val}}}}}"#);
                    let events = format!("[{},{}]", ev("x", v[0]), ev("y", v[1]));
                    qmp(&qmp_sock, &format!(r#"{{"execute":"input-send-event","arguments":{{"events":{events}}}}}"#));
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                // A key held: {keydown alt} ... {keyup alt} (QEMU qcodes).
                if let Some((down, k)) = key.strip_prefix("keydown ").map(|k| (true, k)).or_else(|| key.strip_prefix("keyup ").map(|k| (false, k))) {
                    let ev = format!(r#"[{{"type":"key","data":{{"down":{down},"key":{{"type":"qcode","data":"{k}"}}}}}}]"#);
                    qmp(&qmp_sock, &format!(r#"{{"execute":"input-send-event","arguments":{{"events":{ev}}}}}"#));
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                // Mouse input: {mouse_move dx dy}, {mouse_button n}.
                if key.starts_with("mouse_") {
                    monitor(&sock, &key);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                // "~" pauses; Enter waits a moment so a program started by
                // the line is reading before the next keys arrive.
                if key == "pause" {
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                }
                monitor(&sock, &format!("sendkey {key}"));
                std::thread::sleep(Duration::from_millis(if key == "ret" { 1000 } else { 60 }));
            }
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if !monitored && start.elapsed() >= Duration::from_secs(o.monitor_at.unwrap_or(0)) {
            monitored = true;
            for cmd in o.monitor.as_deref().unwrap().split(';').map(str::trim) {
                // "sleep N" waits N seconds between commands.
                match cmd.strip_prefix("sleep ") {
                    Some(n) => std::thread::sleep(Duration::from_secs(n.parse().expect("sleep seconds"))),
                    None => monitor(&sock, cmd),
                }
                std::thread::sleep(Duration::from_millis(500));
            }
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
    if let Some(f) = feeder.as_mut() {
        let _ = f.kill();
        let _ = f.wait();
    }
    if o.headless {
        // Programs print code page 850 text: not necessarily UTF-8.
        if let Ok(log) = std::fs::read(&serial) {
            print!("{}", String::from_utf8_lossy(&log));
        }
    }
    true
}

/// QEMU key names for a string. "^X" is Ctrl+X, "{name}" a QEMU key name
/// as is (e.g. {ctrl-esc}, {alt-esc}, {down}), "{sleep}" a one second pause
/// ("{sleep N}" N seconds),
/// "{mouse_move dx dy}" / "{mouse_button n}" QEMU mouse commands,
/// "{mouse_abs x y}" an absolute pointer position (0..32767, via QMP),
/// "{shot NAME}" a screenshot as work/NAME.png.
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
            '~' => "pause".into(),
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

/// Run one QMP command (after the capabilities handshake).
fn qmp(sock: &Path, cmd: &str) {
    match std::os::unix::net::UnixStream::connect(sock) {
        Ok(mut s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = writeln!(s, r#"{{"execute":"qmp_capabilities"}}"#);
            let _ = s.read(&mut buf);
            let _ = writeln!(s, "{cmd}");
            if let Ok(n) = s.read(&mut buf) {
                let reply = String::from_utf8_lossy(&buf[..n]);
                if reply.contains("error") {
                    eprintln!("qmp {cmd}: {reply}");
                }
            }
        }
        Err(e) => eprintln!("qmp: {e}"),
    }
}

fn monitor(sock: &Path, cmd: &str) {
    match std::os::unix::net::UnixStream::connect(sock) {
        Ok(mut s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = writeln!(s, "{cmd}");
            // Show what queries ("info registers", ...) answer.
            if cmd.starts_with("info") || cmd.starts_with("x ") || cmd.starts_with("xp ") {
                std::thread::sleep(Duration::from_millis(300));
                let mut out = Vec::new();
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    out.extend_from_slice(&buf[..n]);
                }
                eprintln!("monitor {cmd}:
{}", String::from_utf8_lossy(&out));
            } else {
                let _ = s.read(&mut buf);
            }
        }
        Err(e) => eprintln!("monitor: {e}"),
    }
}

/// VMware device models: SVGA II display, backdoor port (vmmouse) and
/// SCSI controllers.
fn vmware_devices(o: &Opts, c: &mut Command) {
    if o.vmware {
        c.args(["-vga", "none", "-device", "vmware-svga", "-machine", "vmport=on"]);
    }
    if o.pvscsi.is_none() && o.pvscsi_cd.is_none() {
        return;
    }
    let hba = o.scsi_hba.as_deref().unwrap_or("pvscsi");
    c.arg("-device").arg(format!("{hba},id=scsi0"));
    if let Some(d) = &o.pvscsi {
        c.arg("-drive").arg(format!("if=none,id=sd0,format=raw,file={}", d.display()));
        c.args(["-device", "scsi-hd,bus=scsi0.0,drive=sd0,scsi-id=0"]);
    }
    if let Some(d) = &o.pvscsi_cd {
        c.arg("-drive").arg(format!("if=none,id=scd0,media=cdrom,readonly=on,format=raw,file={}", d.display()));
        c.args(["-device", "scsi-cd,bus=scsi0.0,drive=scd0,scsi-id=1"]);
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

/// HMTEST.HLP's metafile artwork (`:artwork name='HMART.MET'.`), rs2's
/// own picture built here with crates/gpi: a sky, a sun, hills and a
/// house, as GPI orders in one segment of a PU_PELS page.
fn hmtest_metafile() -> Vec<u8> {
    use gpi::goca::{Attr, Order};
    use gpi::Point;
    let p = |x: i32, y: i32| Point { x, y };
    // CLR_ indices: 1 blue, 2 red, 5 cyan, 6 yellow, -1 black, 12 dark
    // green, 14 brown, 15 pale grey.
    let colour = |c: i32| Order::Set(Attr::Color(c));
    let filled = |c: i32, a: Point, b: Point| [colour(c), Order::Box { at: Some(a), flags: 0x40, corner: b, h: 0, v: 0 }];
    let mut o: Vec<Order> = Vec::new();
    o.extend(filled(5, p(0, 0), p(400, 300)));
    // The sun: a filled full arc.
    o.push(colour(6));
    o.push(Order::Set(Attr::Position(p(330, 240))));
    o.push(Order::BeginArea(0x40));
    o.push(Order::FullArc { at: None, mult: 30 << 16 });
    o.push(Order::EndArea);
    // Hills: an area under a curve.
    o.push(colour(12));
    o.push(Order::BeginArea(0x40));
    o.push(Order::Line { at: Some(p(0, 0)), pts: vec![p(0, 110)] });
    o.push(Order::Fillet { at: None, pts: vec![p(100, 170), p(220, 60), p(320, 150), p(400, 90)] });
    o.push(Order::Line { at: None, pts: vec![p(400, 0), p(0, 0)] });
    o.push(Order::EndArea);
    // The house: walls, a roof, a door and two windows.
    o.extend(filled(15, p(140, 40), p(260, 130)));
    o.push(colour(2));
    o.push(Order::BeginArea(0x40));
    o.push(Order::Line { at: Some(p(130, 130)), pts: vec![p(200, 185), p(270, 130), p(130, 130)] });
    o.push(Order::EndArea);
    o.extend(filled(14, p(185, 40), p(215, 95)));
    o.extend(filled(1, p(150, 95), p(175, 120)));
    o.extend(filled(1, p(225, 95), p(250, 120)));
    // Outlines and a caption.
    o.push(colour(-1));
    o.push(Order::Box { at: Some(p(140, 40)), flags: 0x20, corner: p(260, 130), h: 0, v: 0 });
    o.push(Order::Line { at: Some(p(130, 130)), pts: vec![p(200, 185), p(270, 130)] });
    o.push(Order::CharString { at: Some(p(12, 280)), moving: false, text: b"rs2 metafile artwork".to_vec() });
    let body = gpi::goca::encode_all(&o);
    let mut data = Vec::new();
    gpi::goca::encode(&Order::BeginSegment { id: 1, flags: 0x7000, len: body.len() as u32 }, &mut data);
    data.extend_from_slice(&body);
    gpi::metafile::build(&gpi::metafile::Metafile { units: gpi::xform::PU_PELS, page: (400, 300), data, ..Default::default() })
}
