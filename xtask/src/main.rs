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
//!                  [--net] [--virtio-net] [--net-model MODEL] [--hostfwd RULE]... [--tftp DIR]
//!                  [--sound [hda|ac97|es1370]] [--sound-wav FILE]
//!                  [-- QEMU ARGS]
//! cargo xtask iso [--release] [-o FILE]    build the hybrid install/live ISO
//! cargo xtask disk [--hpfs|--fat32] [-o FILE]  test hard disk from work/disk.txt
//! ```
//!
//! `--desktop` boots into the desktop shell (`init=OS2\RS2SHELL.EXE` in place
//! of work/cmdline.txt's `init=`), `--init "PROGRAM ARGS"` into another
//! program. `--type-file` reads the keys for `--type`
//! from a file (its line ends are not typed).
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
//! (`e1000`) unless `--net-model` names another QEMU device (`e1000e`,
//! `e1000-82545em`, `virtio-net-pci`); `--virtio-net` is short for
//! `--net --net-model virtio-net-pci`. Host port 5555 is forwarded to
//! guest port 23 (when free), and each `--hostfwd tcp::HOSTPORT-:GUESTPORT` adds a
//! rule; `--tftp DIR` has QEMU serve DIR by TFTP at 10.0.2.2. Without
//! these options the machine has no network adapter.
//!
//! `--sound` adds an ICH9 HD Audio controller with a duplex codec
//! (`ich9-intel-hda`, `hda-duplex`; `hda-output` on CoreAudio, which
//! cannot record) playing through the host's sound output (CoreAudio on
//! macOS; nothing when `--headless`); `--sound ac97` an Intel ICH AC'97
//! controller (`AC97`) and `--sound es1370` (or `es1371`) an Ensoniq
//! AudioPCI ES1370 (`ES1370`: QEMU has no ES1371) in its place;
//! `--sound-wav FILE` records what it plays to a WAV file instead.

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
    /// Directory QEMU's user network serves by TFTP (10.0.2.2).
    tftp: Option<String>,
    /// Boot into the desktop shell (init=OS2\\RS2SHELL.EXE instead of
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
        monitor: None,
        monitor_at: None,
        iso: None,
        hdd: None,
        nvme: None,
        smp: 2,
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
        tftp: None,
        desktop: false,
        init: None,
        sound: false,
        sound_card: "hda",
        sound_wav: None,
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
                o.net.get_or_insert_with(|| "e1000".into());
            }
            "--virtio-net" => o.net = Some("virtio-net-pci".into()),
            "--net-model" => {
                i += 1;
                o.net = Some(args[i].clone());
            }
            "--tftp" => {
                i += 1;
                o.tftp = Some(args[i].clone());
            }
            "--hostfwd" => {
                i += 1;
                o.hostfwd.push(args[i].clone());
            }
            "--hdd" => {
                i += 1;
                o.hdd = Some(PathBuf::from(&args[i]));
            }
            "--nvme" => {
                i += 1;
                o.nvme = Some(PathBuf::from(&args[i]));
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
                image::disk(&files, "RS2DISK", 64, o.fat32.then_some(fat::FatType::Fat32))
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
            None => "init=OS2\\RS2SHELL.EXE".into(),
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
        let mut files = image::manifest(&root(), &text);
        files.extend(userland);
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
        ("audtest", "RS2\\AUDTEST.EXE"),
        ("desktest", "RS2\\DESKTEST.EXE"),
        ("testdll", "RS2\\TESTDLL.DLL"),
        ("cmd", "OS2\\CMD.EXE"),
        ("more", "OS2\\MORE.EXE"),
        ("find", "OS2\\FIND.EXE"),
        ("sort", "OS2\\SORT.EXE"),
        ("attrib", "OS2\\ATTRIB.EXE"),
        ("tree", "OS2\\TREE.EXE"),
        ("xcopy", "OS2\\XCOPY.EXE"),
        ("sysdlls", "OS2\\SYSDLLS.EXE"),
        ("install", "OS2\\INSTALL.EXE"),
        ("pmwin", "OS2\\DLL\\PMWIN.DLL"),
        ("pmgpi", "OS2\\DLL\\PMGPI.DLL"),
        ("pmsrv", "OS2\\PMSRV.EXE"),
        ("pmhang", "RS2\\PMHANG.EXE"),
        ("pmbusy", "RS2\\PMBUSY.EXE"),
        ("pmviowin", "OS2\\PMVIOWIN.EXE"),
        ("rs2shell", "OS2\\RS2SHELL.EXE"),
        ("e", "OS2\\E.EXE"),
        ("pmctls", "OS2\\DLL\\PMCTLS.DLL"),
        ("helpmgr", "OS2\\DLL\\HELPMGR.DLL"),
        ("view", "OS2\\VIEW.EXE"),
        ("appinst", "OS2\\APPINST.EXE"),
        ("hmtest", "RS2\\HMTEST.EXE"),
        ("mmiotest", "RS2\\MMIOTEST.EXE"),
        ("label", "OS2\\LABEL.EXE"),
        ("chkdsk", "OS2\\CHKDSK.EXE"),
        ("format", "OS2\\FORMAT.EXE"),
        ("mode", "OS2\\MODE.EXE"),
        ("help", "OS2\\HELP.EXE"),
        ("keyb", "OS2\\KEYB.EXE"),
        ("ping", "OS2\\PING.EXE"),
        ("ifconfig", "OS2\\IFCONFIG.EXE"),
        ("netstat", "OS2\\NETSTAT.EXE"),
        ("hostname", "OS2\\HOSTNAME.EXE"),
        ("host", "OS2\\HOST.EXE"),
        ("tcpip32", "OS2\\DLL\\TCPIP32.DLL"),
        ("tnls32", "OS2\\DLL\\TNLS32.DLL"),
        ("pmshapi", "OS2\\DLL\\PMSHAPI.DLL"),
        ("so32dll", "OS2\\DLL\\SO32DLL.DLL"),
        ("tcp32dll", "OS2\\DLL\\TCP32DLL.DLL"),
        ("tcpipdll", "OS2\\DLL\\TCPIPDLL.DLL"),
        ("tnls16", "OS2\\DLL\\TNLS16.DLL"),
        ("pmdrag", "OS2\\DLL\\PMDRAG.DLL"),
        ("pmmerge", "OS2\\DLL\\PMMERGE.DLL"),
        ("pmwp", "OS2\\DLL\\PMWP.DLL"),
        ("libuni", "OS2\\DLL\\LIBUNI.DLL"),
        ("uconv", "OS2\\DLL\\UCONV.DLL"),
        ("mmio", "OS2\\DLL\\MMIO.DLL"),
        ("mdm", "OS2\\DLL\\MDM.DLL"),
    ];

/// Help files compiled from IPF source in userland/, with their names.
const HELP_FILES: &[(&str, &str)] = &[("e/E.IPF", "OS2\\HELP\\E.HLP"), ("hmtest/HMTEST.IPF", "OS2\\HELP\\HMTEST.HLP"), ("helpmgr/HMHELP.IPF", "OS2\\HELP\\HMHELP.HLP")];

/// Programs marked as PM applications (WINDOWAPI).
const PM_PROGRAMS: &[&str] = &["pmsrv", "pmhang", "pmbusy", "pmviowin", "rs2shell", "e", "view", "appinst", "hmtest"];

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
    for (src, dest) in HELP_FILES {
        let path = ul.join(src);
        let text = std::fs::read(&path).ok()?;
        let help = match ipf::compile::compile(&text, true) {
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
    c.args(["-machine", "q35", "-cpu", "max", "-m", "512M", "-no-reboot", "-smp", &o.smp.to_string()])
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
    if o.headless {
        c.args(["-display", "none"]).arg("-serial").arg(format!("file:{}", serial.display()));
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
    let Some(timeout) = o.timeout else {
        return child.wait().map(|s| s.success()).unwrap_or(false);
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
    if o.headless {
        // Programs print code page 850 text: not necessarily UTF-8.
        if let Ok(log) = std::fs::read(&serial) {
            print!("{}", String::from_utf8_lossy(&log));
        }
    }
    true
}

/// QEMU key names for a string. "^X" is Ctrl+X, "{name}" a QEMU key name
/// as is (e.g. {ctrl-esc}, {alt-esc}, {down}), "{sleep}" a one second pause,
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
