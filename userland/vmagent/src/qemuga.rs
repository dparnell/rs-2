//! QEMUGA.EXE: the QEMU guest agent (docs/AGENT.md), for Proxmox VE,
//! libvirt and `virsh qemu-agent-command`: shutting down and restarting
//! cleanly, the network addresses, the OS, host name, time and processors,
//! file system "freezes" around backups (the caches written out), and files
//! read and written. The protocol is guestagent::qga; this is the system's
//! side. CONFIG.SYS starts it (`RUN=C:\OS2\QEMUGA.EXE`); without the port
//! `org.qemu.guest_agent.0` (`\DEV\QEMUGA$`) it ends at once.
//!
//! A shutdown goes through the Workplace Shell when it runs (programs are
//! asked to close, as with the desktop's Shut down), else the file systems
//! are flushed and the machine turned off or restarted at once.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use guestagent::qga::{self, Agent, Guest, Iface, OsInfo, Shutdown};

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "DOSCALLS.415"]
    fn DosShutdown(reserved: u32) -> u32;
    #[link_name = "DOSCALLS.254"]
    fn DosResetBuffer(h: u32) -> u32;
    #[link_name = "RS2API.7"]
    fn Rs2PowerOff(action: u32) -> u32;
}

struct Os2 {
    files: Vec<u32>,
}

fn rc_text(what: &str, rc: u32) -> String {
    let why = match rc {
        2 => "The system cannot find the file specified.",
        3 => "The system cannot find the path specified.",
        5 => "Access is denied.",
        6 => "The handle is invalid.",
        32 => "The file is in use by another process.",
        _ => "",
    };
    alloc::format!("{what}: error {rc}. {why}")
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// Minutes west of UTC of the system clock (0 when not set).
fn tz_west(dt: &os2::DateTime) -> i64 {
    if dt.timezone == -1 { 0 } else { dt.timezone as i64 }
}

fn sysinfo(i: u32) -> u32 {
    let mut v = 0;
    unsafe { os2::DosQuerySysInfo(i, i, &mut v, 4) };
    v
}

impl Os2 {
    fn handle(&self, h: u32) -> Result<u32, String> {
        if self.files.contains(&h) { Ok(h) } else { Err(String::from("Invalid parameter 'handle'")) }
    }
}

impl Guest for Os2 {
    fn hostname(&mut self) -> String {
        os2::net::hostname()
    }

    fn interfaces(&mut self) -> Vec<Iface> {
        os2::net::os2_interfaces()
            .into_iter()
            .map(|(name, i)| Iface {
                mac: (i.flags & os2::net::IF_LOOPBACK == 0).then_some(i.mac),
                ipv4: if i.addr != [0; 4] { alloc::vec![(i.addr, qga::prefix(i.mask))] } else { Vec::new() },
                rx_bytes: i.rx_bytes as u64,
                rx_packets: i.rx_packets as u64,
                tx_bytes: i.tx_bytes as u64,
                tx_packets: i.tx_packets as u64,
                rx_dropped: i.rx_dropped as u64,
                name,
            })
            .collect()
    }

    fn os_info(&mut self) -> OsInfo {
        OsInfo { version: String::from(env!("CARGO_PKG_VERSION")), machine: "x86_64" }
    }

    fn time_ns(&mut self) -> i64 {
        let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
        unsafe { os2::DosGetDateTime(&mut dt) };
        let days = days_from_civil(dt.year as i64, dt.month as i64, dt.day as i64);
        let secs = days * 86400 + dt.hours as i64 * 3600 + dt.minutes as i64 * 60 + dt.seconds as i64 + tz_west(&dt) * 60;
        secs * 1_000_000_000 + dt.hundredths as i64 * 10_000_000
    }

    fn set_time_ns(&mut self, ns: Option<i64>) -> Result<(), String> {
        let Some(ns) = ns else { return Ok(()) };
        let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
        unsafe { os2::DosGetDateTime(&mut dt) };
        let local = ns.div_euclid(1_000_000_000) - tz_west(&dt) * 60;
        let (y, m, d) = civil_from_days(local.div_euclid(86400));
        let t = local.rem_euclid(86400);
        dt.year = y as u16;
        dt.month = m as u8;
        dt.day = d as u8;
        dt.hours = (t / 3600) as u8;
        dt.minutes = (t / 60 % 60) as u8;
        dt.seconds = (t % 60) as u8;
        dt.hundredths = (ns.rem_euclid(1_000_000_000) / 10_000_000) as u8;
        dt.weekday = ((local.div_euclid(86400) + 4).rem_euclid(7)) as u8;
        match unsafe { os2::DosSetDateTime(&dt) } {
            0 => Ok(()),
            rc => Err(rc_text("DosSetDateTime", rc)),
        }
    }

    fn timezone(&mut self) -> (i64, Option<String>) {
        let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
        unsafe { os2::DosGetDateTime(&mut dt) };
        (-tz_west(&dt) * 60, None)
    }

    fn cpus(&mut self) -> u32 {
        // QSV_NUMPROCESSORS
        sysinfo(26).max(1)
    }

    fn shutdown(&mut self, how: Shutdown) -> Result<(), String> {
        let restart = (how == Shutdown::Reboot) as u32;
        os2::println!("QEMUGA: {}", if restart == 1 { "restarting" } else { "shutting down" });
        // Through the Workplace Shell, if it runs (PMWP's Rs2ShutdownSystem).
        let mut hmod = 0;
        let mut fail = [0u8; 16];
        if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 16, b"PMWP\0".as_ptr(), &mut hmod) } == 0 {
            let mut pfn = 0usize;
            if unsafe { os2::DosQueryProcAddr(hmod, 9101, core::ptr::null(), &mut pfn) } == 0 && pfn != 0 {
                let f: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(pfn) };
                if f(restart) != 0 {
                    os2::println!("QEMUGA: the Workplace Shell closes the programs");
                    return Ok(());
                }
            }
        }
        unsafe {
            DosShutdown(0);
            Rs2PowerOff(restart);
        }
        Err(String::from("The machine could not be turned off"))
    }

    fn flush(&mut self) -> u32 {
        unsafe { DosResetBuffer(0xffff_ffff) };
        1
    }

    fn file_open(&mut self, path: &str, mode: &str) -> Result<u32, String> {
        // OPEN_ACTION_*: fail/open/replace if exists, create if new.
        let (action, write, append) = match mode.trim_end_matches('b').trim_end_matches("b+") {
            "r" => (0x01, false, false),
            "r+" => (0x01, true, false),
            "w" | "w+" => (0x12, true, false),
            "a" | "a+" => (0x11, true, true),
            m if m.starts_with('r') && m.ends_with('+') => (0x01, true, false),
            _ => return Err(alloc::format!("Invalid parameter 'mode': {mode}")),
        };
        let mut name: Vec<u8> = os2::text_bytes(path).into_owned();
        name.push(0);
        let (mut h, mut act) = (0u32, 0u32);
        // Deny none; read-only or read/write.
        let open_mode = 0x40 | if write { 2 } else { 0 };
        let rc = unsafe { os2::DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, action, open_mode, core::ptr::null_mut()) };
        if rc != 0 {
            return Err(rc_text(path, rc));
        }
        if append {
            let mut pos = 0;
            unsafe { os2::DosSetFilePtr(h, 0, 2, &mut pos) };
        }
        self.files.push(h);
        Ok(h)
    }

    fn file_read(&mut self, h: u32, count: usize) -> Result<(Vec<u8>, bool), String> {
        let h = self.handle(h)?;
        let mut buf = alloc::vec![0u8; count];
        let mut got = 0;
        let rc = unsafe { os2::DosRead(h, buf.as_mut_ptr(), count as u32, &mut got) };
        if rc != 0 {
            return Err(rc_text("DosRead", rc));
        }
        buf.truncate(got as usize);
        Ok((buf, (got as usize) < count))
    }

    fn file_write(&mut self, h: u32, data: &[u8]) -> Result<usize, String> {
        let h = self.handle(h)?;
        let mut n = 0;
        match unsafe { os2::DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) } {
            0 => Ok(n as usize),
            rc => Err(rc_text("DosWrite", rc)),
        }
    }

    fn file_seek(&mut self, h: u32, offset: i64, whence: u32) -> Result<(u64, bool), String> {
        let h = self.handle(h)?;
        if whence > 2 {
            return Err(String::from("Invalid parameter 'whence'"));
        }
        let (mut pos, mut end) = (0u32, 0u32);
        let rc = unsafe { os2::DosSetFilePtr(h, offset as i32, whence, &mut pos) };
        if rc != 0 {
            return Err(rc_text("DosSetFilePtr", rc));
        }
        // The size, then back.
        unsafe {
            os2::DosSetFilePtr(h, 0, 2, &mut end);
            let mut p = 0;
            os2::DosSetFilePtr(h, pos as i32, 0, &mut p);
        }
        Ok((pos as u64, pos >= end))
    }

    fn file_flush(&mut self, h: u32) -> Result<(), String> {
        let h = self.handle(h)?;
        match unsafe { DosResetBuffer(h) } {
            0 => Ok(()),
            rc => Err(rc_text("DosResetBuffer", rc)),
        }
    }

    fn file_close(&mut self, h: u32) -> Result<(), String> {
        let h = self.handle(h)?;
        self.files.retain(|&f| f != h);
        match unsafe { os2::DosClose(h) } {
            0 => Ok(()),
            rc => Err(rc_text("DosClose", rc)),
        }
    }
}

fn main(_s: &os2::Startup) -> u32 {
    let (mut h, mut action) = (0u32, 0u32);
    if unsafe { os2::DosOpen(b"\\DEV\\QEMUGA$\0".as_ptr(), &mut h, &mut action, 0, 0, 0x01, 0x0042, core::ptr::null_mut()) } != 0 {
        return 1;
    }
    // One agent.
    let mut mtx = 0;
    if unsafe { os2::DosCreateMutexSem(b"\\SEM32\\RS2\\QEMUGA\0".as_ptr(), &mut mtx, 0, 0) } != 0 {
        return 0;
    }
    let mut g = Os2 { files: Vec::new() };
    let mut a = Agent::new();
    let mut buf = alloc::vec![0u8; 4096];
    loop {
        let mut n = 0;
        let rc = unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 || n == 0 {
            unsafe { os2::DosSleep(500) };
            continue;
        }
        let out = a.feed(&mut g, &buf[..n as usize]);
        if !out.is_empty() {
            let mut w = 0;
            unsafe { os2::DosWrite(h, out.as_ptr(), out.len() as u32, &mut w) };
        }
    }
}
