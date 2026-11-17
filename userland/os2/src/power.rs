//! The processor power plan (RS2API 27, `Rs2PowerControl`; the kernel's
//! cpupower.rs) and CONFIG.SYS's `POWERPLAN=`, for POWERCFG and the
//! Power object.

use alloc::vec::Vec;

pub const PERFORMANCE: u32 = 0;
pub const BALANCED: u32 = 1;
pub const POWER_SAVER: u32 = 2;

/// The plans' names, as POWERCFG shows them.
pub const NAMES: [&str; 3] = ["Performance", "Balanced", "Power saver"];
/// The plans' words in CONFIG.SYS (`POWERPLAN=`).
pub const WORDS: [&str; 3] = ["PERFORMANCE", "BALANCED", "POWERSAVER"];

// RS2POWERINFO flags.
pub const F_MWAIT: u32 = 0x1;
pub const F_HWP: u32 = 0x2;
pub const F_SPEEDSTEP: u32 = 0x4;
pub const F_EPB: u32 = 0x8;
pub const F_RAPL_PKG: u32 = 0x10;
pub const F_RAPL_CORE: u32 = 0x20;
pub const F_RAPL_DRAM: u32 = 0x40;
pub const F_TICKLESS: u32 = 0x80;
pub const F_APERF: u32 = 0x100;
pub const F_HYPERVISOR: u32 = 0x200;
pub const F_ARAT: u32 = 0x400;
pub const F_INVARIANT_TSC: u32 = 0x800;

/// RS2POWERINFO.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct PowerInfo {
    pub cb: u32,
    pub plan: u32,
    pub flags: u32,
    /// The deepest idle C-state the plan uses (1: C1, HLT).
    pub cstate: u32,
    pub pkg_mw: u32,
    pub core_mw: u32,
    pub dram_mw: u32,
    /// The boot CPU's average clock while running, last second (0 unknown).
    pub cpu0_mhz: u32,
    pub min_mhz: u32,
    pub base_mhz: u32,
    pub max_mhz: u32,
    pub cpus: u32,
    /// CPUs idle with their tick stopped right now.
    pub tickless_now: u32,
    /// Timer interrupts all CPUs took in the last second.
    pub timer_irqs: u32,
}

unsafe extern "C" {
    #[link_name = "RS2API.27"]
    pub fn Rs2PowerControl(func: u32, param: u32, buf: *mut u8, cb: u32) -> u32;
}

pub fn query() -> Option<PowerInfo> {
    let mut i = PowerInfo::default();
    let rc = unsafe { Rs2PowerControl(0, 0, &mut i as *mut PowerInfo as *mut u8, core::mem::size_of::<PowerInfo>() as u32) };
    (rc == 0).then_some(i)
}

/// Change the plan now (until the next boot).
pub fn set_plan(plan: u32) -> u32 {
    unsafe { Rs2PowerControl(1, plan, core::ptr::null_mut(), 0) }
}

/// Parse a plan name as POWERCFG and CONFIG.SYS take it.
pub fn parse(s: &str) -> Option<u32> {
    let u: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).map(|b| b.to_ascii_uppercase()).collect();
    match u.as_slice() {
        b"PERFORMANCE" | b"HIGH" | b"MAX" | b"0" => Some(PERFORMANCE),
        b"BALANCED" | b"1" => Some(BALANCED),
        b"POWERSAVER" | b"SAVER" | b"LOW" | b"2" => Some(POWER_SAVER),
        _ => None,
    }
}

/// CONFIG.SYS's text with its POWERPLAN= statement set to `plan` (any
/// there replaced; the new one goes where the first was, else at the
/// end).
pub fn with_plan(text: &[u8], plan: u32) -> Vec<u8> {
    let text = match text.iter().position(|&b| b == 0x1a) {
        Some(i) => &text[..i],
        None => text,
    };
    let stmt = alloc::format!("POWERPLAN={}\r\n", WORDS[plan.min(2) as usize]);
    let mut out = Vec::new();
    let mut put = false;
    for line in text.split_inclusive(|&b| b == b'\n') {
        let t: Vec<u8> = line.iter().copied().skip_while(|&b| b == b' ' || b == b'\t').collect();
        if t.len() >= 10 && t[..10].eq_ignore_ascii_case(b"POWERPLAN=") {
            if !put {
                out.extend_from_slice(stmt.as_bytes());
                put = true;
            }
            continue;
        }
        out.extend_from_slice(line);
    }
    if !put {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(stmt.as_bytes());
    }
    out
}

/// The plan CONFIG.SYS's POWERPLAN= names, if any.
pub fn saved_plan(text: &[u8]) -> Option<u32> {
    text.split(|&b| b == b'\n').find_map(|l| {
        let t: Vec<u8> = l.iter().copied().skip_while(|&b| b == b' ' || b == b'\t').collect();
        if t.len() >= 10 && t[..10].eq_ignore_ascii_case(b"POWERPLAN=") {
            let v: alloc::string::String = t[10..].iter().map(|&b| b as char).collect();
            parse(&v)
        } else {
            None
        }
    })
}

/// The boot drive's CONFIG.SYS path ("C:\CONFIG.SYS"), NUL-terminated.
pub fn config_path() -> [u8; 14] {
    let mut d = 3u32;
    unsafe { crate::dos::DosQuerySysInfo(5, 5, &mut d, 4) };
    let mut p = *b"C:\\CONFIG.SYS\0";
    p[0] = b'A' + (d.clamp(1, 26) - 1) as u8;
    p
}

fn read(path: &[u8]) -> Option<Vec<u8>> {
    use crate::dos::*;
    let (mut hf, mut act) = (0u32, 0u32);
    if unsafe { DosOpen(path.as_ptr(), &mut hf, &mut act, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut got = 0;
        if unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got as usize]);
    }
    unsafe { DosClose(hf) };
    Some(out)
}

/// The plan saved in CONFIG.SYS (the plan at the next boot).
pub fn query_saved() -> Option<u32> {
    saved_plan(&read(&config_path())?)
}

/// Write POWERPLAN= into CONFIG.SYS; 0 or the error.
pub fn save(plan: u32) -> u32 {
    use crate::dos::*;
    let path = config_path();
    let Some(old) = read(&path) else { return 2 };
    let data = with_plan(&old, plan);
    let (mut hf, mut act) = (0u32, 0u32);
    let rc = unsafe { DosOpen(path.as_ptr(), &mut hf, &mut act, data.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) };
    if rc != 0 {
        return rc;
    }
    let mut done = 0;
    let rc = unsafe { DosWrite(hf, data.as_ptr(), data.len() as u32, &mut done) };
    unsafe { DosClose(hf) };
    if rc != 0 {
        rc
    } else if done as usize != data.len() {
        112
    } else {
        0
    }
}

/// The power status `APM$` reports (category 0xC function 0x60, the
/// packet of IBM's APM.SYS): the AC line and the batteries, which rs2's
/// kernel reads from the ACPI namespace.
#[derive(Default, Clone, Copy, Debug)]
pub struct ApmStatus {
    /// 0 off line, 1 on line, 2 backup power, 0xFF unknown.
    pub ac: u8,
    /// 0 high, 1 low, 2 critical, 3 charging, 0xFF unknown.
    pub status: u8,
    /// Percent, 0xFF unknown.
    pub life: u8,
    /// 0 seconds, 1 minutes, 0xFF unknown.
    pub time_unit: u8,
    pub time: u16,
    /// Bit 0 high, 1 low, 2 critical, 3 charging, 7 no system battery.
    pub flags: u8,
    pub batteries: u8,
}

/// Ask `APM$` for the power status of battery `unit` (0: all of them).
pub fn apm_status(unit: u8) -> Option<ApmStatus> {
    use crate::dos::{DosClose, DosDevIOCtl, DosOpen};
    let (mut h, mut act) = (0u32, 0u32);
    if unsafe { DosOpen(b"APM$\0".as_ptr(), &mut h, &mut act, 0, 0, 1, 0x42, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut p = [0u8; 13];
    p[0] = 13;
    p[11] = unit;
    let mut d = [0u8; 2];
    let (mut cbp, mut cbd) = (13u32, 2u32);
    let rc = unsafe { DosDevIOCtl(h, 0xc, 0x60, p.as_mut_ptr(), 13, &mut cbp, d.as_mut_ptr(), 2, &mut cbd) };
    unsafe { DosClose(h) };
    (rc == 0).then(|| ApmStatus {
        ac: p[4],
        status: p[5],
        life: p[6],
        time_unit: p[7],
        time: u16::from_le_bytes([p[8], p[9]]),
        flags: p[10],
        batteries: p[12],
    })
}

impl ApmStatus {
    pub fn has_battery(&self) -> bool {
        self.flags != 0xff && self.flags & 0x80 == 0
    }

    pub fn charging(&self) -> bool {
        self.status == 3 || (self.flags != 0xff && self.flags & 8 != 0)
    }

    /// "AC line", "battery" or "unknown".
    pub fn source_text(&self) -> &'static str {
        match self.ac {
            0 => "battery",
            1 => "AC line",
            2 => "backup power",
            _ => "unknown",
        }
    }

    /// "72%, charging", "35%, 1 h 24 min left", "none found".
    pub fn battery_text(&self) -> alloc::string::String {
        use alloc::format;
        if !self.has_battery() {
            return alloc::string::String::from("none found");
        }
        let mut s = if self.life <= 100 { format!("{}%", self.life) } else { alloc::string::String::from("charge unknown") };
        if self.charging() {
            s += ", charging";
        } else if self.ac == 1 {
            s += ", not charging";
        }
        if self.time_unit <= 1 {
            let mins = if self.time_unit == 1 { self.time as u32 } else { self.time as u32 / 60 };
            s += &format!(", {} h {:02} min left", mins / 60, mins % 60);
        }
        match self.status {
            2 => s += " (critical)",
            1 => s += " (low)",
            _ => {}
        }
        s
    }
}
