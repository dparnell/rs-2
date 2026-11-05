//! CONFIG.SYS processing.
//!
//! Read from the root of the boot drive at start-up, as OS/2 does. The
//! statements that matter to the kernel today are applied; device driver
//! statements (BASEDEV, DEVICE, IFS) are recorded because native drivers
//! take their place.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Default, Debug)]
pub struct Config {
    pub libpath: Option<String>,
    /// SET statements, in order.
    pub env: Vec<String>,
    /// PROTSHELL program and arguments.
    pub protshell: Option<(String, String)>,
    /// CALL= programs run (synchronously) before the shell.
    pub call: Vec<(String, String)>,
    /// RUN= programs started detached.
    pub run: Vec<(String, String)>,
    pub codepage: Vec<u16>,
    /// COUNTRY=nnn[,file]: the country code.
    pub country: Option<u16>,
    /// DEVINFO=KBD,layout[,file]: the keyboard layout id.
    pub keyboard: Option<String>,
    pub pause_on_error: bool,
    /// PRIORITY=ABSOLUTE turns the starvation boost off.
    pub priority_absolute: bool,
    /// MAXWAIT=seconds before a starved thread is boosted.
    pub maxwait: Option<u32>,
    /// TIMESLICE=min[,max] in milliseconds (the minimum is used).
    pub timeslice: Option<u32>,
    /// NETWORK=off|dhcp|address/prefix[,gateway[,dns...]] (rs2).
    pub network: Option<String>,
    /// VIDEO=WxH (rs2): the screen resolution, set when the display
    /// driver can (written by the Display settings, DISPLAY.EXE).
    pub video: Option<(u32, u32)>,
    /// VIDEO=WxHxCOLORS: 256 makes PM's display a palette-managed
    /// 256-colour one.
    pub video_colors: Option<u32>,
    /// VIRTUALADDRESSLIMIT=MB: the end of the high memory arenas.
    pub virtual_address_limit: Option<String>,
    /// Driver statements we do not load (BASEDEV/DEVICE/IFS).
    pub drivers: Vec<String>,
    /// DISKCACHE=, whose block cache is the one under every volume.
    pub diskcache: Option<String>,
}

/// `IFS=...HPFS.IFS /CACHE:n /CRECL:n`: the size of the block cache and
/// the largest request it keeps, in KB (`/CACHE:D` is the default). IBM's
/// limit of 2048 KB is not kept: it was a 16-bit driver's.
pub fn apply_cache(c: &Config) {
    let Some(ifs) = c.drivers.iter().find(|d| d.to_ascii_uppercase().contains("HPFS.IFS")) else { return };
    let arg = |name: &str| {
        ifs.split_ascii_whitespace().find_map(|w| {
            let (k, v) = w.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.parse::<u64>().ok()).flatten()
        })
    };
    crate::fs::cache::configure(arg("/CACHE").map(|k| k.max(64)), arg("/CRECL"));
}

/// `IFS=...HPFS.IFS /AUTOCHECK:DE`, `IFS=...JFS.IFS /AUTOCHECK:*`,
/// `DISKCACHE=...,AC:C` for FAT: the drives CHKDSK looks at during boot if
/// they were not shut down cleanly (`*`: all).
pub fn autocheck(c: &Config) -> Vec<u8> {
    let mut drives = Vec::new();
    for line in c.drivers.iter().filter(|d| d.to_ascii_uppercase().contains(".IFS")).chain(&c.diskcache) {
        for w in line.split([' ', '\t', ',']) {
            let Some((k, v)) = w.split_once(':') else { continue };
            if !["/AUTOCHECK", "-AUTOCHECK", "AC"].iter().any(|n| k.eq_ignore_ascii_case(n)) {
                continue;
            }
            if v.contains('*') {
                drives.extend(2..26);
            }
            drives.extend(v.bytes().filter(u8::is_ascii_alphabetic).map(|l| l.to_ascii_uppercase() - b'A'));
        }
    }
    drives.sort_unstable();
    drives.dedup();
    drives
}

fn split_cmd(v: &str) -> (String, String) {
    let v = v.trim();
    match v.split_once(char::is_whitespace) {
        Some((p, a)) => (p.to_string(), a.trim().to_string()),
        None => (v.to_string(), String::new()),
    }
}

pub fn parse(text: &str) -> Config {
    let mut c = Config::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let up = line.to_ascii_uppercase();
        if up.starts_with("REM ") || up == "REM" {
            continue;
        }
        if let Some(rest) = up.strip_prefix("SET ") {
            // Keep the original case of the value.
            let _ = rest;
            let orig = line[4..].trim();
            if let Some((k, v)) = orig.split_once('=') {
                c.env.push(alloc::format!("{}={}", k.trim().to_ascii_uppercase(), v.trim()));
            }
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_uppercase();
        let val = val.trim();
        match key.as_str() {
            "LIBPATH" => c.libpath = Some(val.to_string()),
            "PROTSHELL" => c.protshell = Some(split_cmd(val)),
            "CALL" => c.call.push(split_cmd(val)),
            "RUN" => c.run.push(split_cmd(val)),
            "DISKCACHE" => c.diskcache = Some(val.to_string()),
            "CODEPAGE" => {
                c.codepage = val.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            }
            "COUNTRY" => c.country = val.split(',').next().and_then(|v| v.trim().parse().ok()),
            "DEVINFO" => {
                let mut parts = val.split(',').map(str::trim);
                if parts.next().is_some_and(|d| d.eq_ignore_ascii_case("KBD")) {
                    c.keyboard = parts.next().filter(|l| !l.is_empty()).map(|l| l.to_ascii_uppercase());
                }
            }
            "PAUSEONERROR" => c.pause_on_error = val.eq_ignore_ascii_case("YES"),
            "PRIORITY" => c.priority_absolute = val.eq_ignore_ascii_case("ABSOLUTE"),
            "MAXWAIT" => c.maxwait = val.parse().ok().filter(|&v| (1..=255).contains(&v)),
            "TIMESLICE" => c.timeslice = val.split(',').next().and_then(|v| v.trim().parse().ok()).filter(|&v: &u32| v >= 1),
            "NETWORK" => c.network = Some(val.to_string()),
            "VIDEO" => {
                let v = alloc::format!("video={val}");
                c.video = bootinfo::video_mode(&v);
                c.video_colors = bootinfo::video_colors(&v);
            }
            "VIRTUALADDRESSLIMIT" => c.virtual_address_limit = Some(val.to_string()),
            "BASEDEV" | "DEVICE" | "IFS" => c.drivers.push(alloc::format!("{key}={val}")),
            _ => {}
        }
    }
    c
}

/// VIDEO=WxH: switch the display (it wins over the loader's `video=`,
/// which picks among the firmware's modes).
pub fn apply_video(c: &Config) {
    if let Some(n) = c.video_colors {
        crate::display::set_colors256(n == 256);
    }
    if let Some((w, h)) = c.video {
        if !crate::display::set_mode(w, h) {
            crate::println!("CONFIG.SYS: VIDEO={w}x{h}: the display cannot switch to that");
        }
    }
}

/// Apply the national language statements: COUNTRY, CODEPAGE and the
/// keyboard layout of DEVINFO=KBD.
pub fn apply_nls(c: &Config) {
    if let Some(id) = &c.keyboard {
        if !crate::kbd::layout::set_layout(id) {
            crate::println!("config.sys: unknown keyboard layout {id}");
        }
    }
    crate::nls::configure(c.country, &c.codepage);
}

/// VIRTUALADDRESSLIMIT=n (MB): as IBM's kernel takes it, a decimal number
/// from 512 (no high memory) up, rounded up to a multiple of 64; a value
/// out of range leaves the default (1024). IBM's kernels stop at 3072,
/// rs2 accepts up to 4032 (docs/ARCHITECTURE.md "Memory layout").
pub fn apply_memory(c: &Config) {
    let Some(v) = &c.virtual_address_limit else { return };
    let n = v.trim().parse::<u32>().ok();
    match n.map(crate::vm::set_limit_mb) {
        Some(Ok(_)) => {}
        _ => crate::println!(
            "CONFIG.SYS: VIRTUALADDRESSLIMIT={}: not a number from {} to {}; {} is used",
            v.trim(),
            crate::vm::LIMIT_MIN_MB,
            crate::vm::LIMIT_MAX_MB,
            crate::vm::limit() >> 20
        ),
    }
}

/// Apply PRIORITY=, MAXWAIT= and TIMESLICE=.
pub fn apply_sched(c: &Config) {
    crate::task::sched::configure(!c.priority_absolute, c.maxwait.unwrap_or(3), c.timeslice.unwrap_or(32));
}

/// Read and parse CONFIG.SYS from drive C:, if present.
pub fn load() -> Option<Config> {
    let data = crate::fs::read_file("C:\\CONFIG.SYS")?;
    let text: String = data.iter().map(|&b| b as char).collect();
    Some(parse(&text))
}
