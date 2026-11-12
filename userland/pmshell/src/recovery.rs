//! The Recovery Choices, rs2's counterpart of OS/2's Alt+F1 screen.
//!
//! OS/2 shows its Recovery Choices from the boot loader (Alt+F1, or at
//! every start with the Desktop's Archive page set so): the Workplace
//! Shell keeps the list of archives for it (IBM's PMWP writes
//! `\OS2\BOOT\ALTF1MID.SCR` and a `CONFIG.n` per archive, studied with
//! Ghidra: DoArchiveWork, WriteAltF1Screen). rs2's boot loader runs from
//! the EFI partition and knows nothing of the system's files, so it only
//! passes the wish on: R (or F1) while it waits adds "recovery" to the
//! kernel command line, and the kernel gives the boot programs
//! `RS2RECOVERY=YES`. The first PMSHELL (CONFIG.SYS's PROTSHELL) then
//! shows this text-mode screen before Presentation Manager starts, when
//! asked so or when the Archive page's "Display recovery choices at each
//! system startup" is on (`PM_Workplace:Archive` Recovery), for the
//! page's timeout (RecoveryTimeout seconds, 0: until a key):
//!
//! - Enter or Esc (or the timeout): the current desktop;
//! - a number: restore that archive (the newest first) and start with
//!   it. The archive's OS2.INI and OS2SYS.INI go back to `\OS2`, its
//!   CONFIG.SYS and STARTUP.CMD to the root, and its copy of the desktop
//!   directory (the files with their extended attributes, which hold the
//!   objects' settings) replaces the desktop. What they replace is kept
//!   in the archive location's `REPLACED` directory. When CONFIG.SYS
//!   changed the system restarts, so that it is the one in use;
//! - C: a command prompt first (the desktop starts when it ends).
//!
//! The archives are the Archive page's (made by PMWP when the shell
//! starts): numbered directories in the archive location, listed with
//! their dates in its ARCHIVES.LOG.

use alloc::vec::Vec;

use os2::*;

use crate::fileops;

unsafe extern "C" {
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "KBDCALLS.4"]
    fn KBDCHARIN();
    #[link_name = "DOSCALLS.415"]
    fn DosShutdown(reserved: u32) -> u32;
    #[link_name = "RS2API.7"]
    fn Rs2PowerOff(action: u32) -> u32;
}

const HINI_USERPROFILE: u32 = 0xffff_ffff;
const APP: &[u8] = b"PM_Workplace:Archive\0";

/// What the screen decided.
pub enum Choice {
    /// Start the desktop (restored or not).
    Desktop,
    /// A command prompt first.
    CommandPrompt,
}

fn profile(key: &[u8]) -> Vec<u8> {
    let mut buf = [0u8; 260];
    let k = crate::z(key);
    let n = unsafe { PrfQueryProfileString(HINI_USERPROFILE, APP.as_ptr(), k.as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    buf[..n.min(buf.len())].iter().copied().take_while(|&c| c != 0).collect()
}

fn boot_drive() -> u8 {
    let mut d = [3u32];
    // QSV_BOOT_DRIVE
    unsafe { DosQuerySysInfo(5, 5, d.as_mut_ptr(), 4) };
    b'A' + (d[0] as u8).clamp(1, 26) - 1
}

fn location() -> Vec<u8> {
    let l = profile(b"Location");
    if l.is_empty() { alloc::format!("{}:\\OS2\\ARCHIVES", boot_drive() as char).into_bytes() } else { l }
}

/// Does this start show the Recovery Choices?
fn wanted() -> bool {
    let mut p: *const u8 = core::ptr::null();
    let asked = unsafe { DosScanEnv(b"RS2RECOVERY\0".as_ptr(), &mut p) } == 0;
    asked || profile(b"Recovery").first() == Some(&b'1')
}

/// The archives: (directory, "date time"), the newest first.
fn archives() -> Vec<(Vec<u8>, Vec<u8>)> {
    let loc = location();
    let log = read_file(&fileops::join(&loc, b"ARCHIVES.LOG")).unwrap_or_default();
    let mut v: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for line in log.split(|&c| c == b'\n') {
        let line: Vec<u8> = line.iter().copied().filter(|&c| c != b'\r').collect();
        let mut f = line.split(|&c| c == b' ').filter(|w| !w.is_empty());
        let (Some(n), Some(d), Some(t)) = (f.next(), f.next(), f.next()) else { continue };
        if n.is_empty() || !n.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let dir = fileops::join(&loc, n);
        let when = [d, b" ", t].concat();
        v.retain(|a| a.0 != dir);
        v.push((dir, when));
    }
    v.retain(|a| fileops::is_dir(&a.0));
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = crate::z(path);
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; read only, deny none.
    if unsafe { DosOpen(p.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0u32;
        if unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(h) };
    Some(out)
}

#[derive(Default, Clone, Copy)]
#[repr(C)]
struct KeyInfo {
    ch: u8,
    scan: u8,
    status: u8,
    nls: u8,
    state: u16,
    time: u32,
}

/// A key if one was typed (KbdCharIn, IO_NOWAIT).
fn key() -> Option<KeyInfo> {
    let mut k = KeyInfo::default();
    let p = os2::vio::ptr_words(&k);
    let rc = unsafe { os2::vio::call16(KBDCHARIN as *const () as usize, &[p[0], p[1], 1, 0]) };
    let k = unsafe { core::ptr::read_volatile(&k) };
    (rc == 0 && k.status & 0x40 != 0).then_some(k)
}

fn out(s: &str) {
    os2::write(s.as_bytes());
}

/// Show the Recovery Choices if this start asks for them.
pub fn run() -> Choice {
    if !wanted() {
        return Choice::Desktop;
    }
    let list = archives();
    let timeout = profile(b"RecoveryTimeout").iter().try_fold(0u32, |n, &c| c.is_ascii_digit().then(|| n * 10 + (c - b'0') as u32)).unwrap_or(0);
    // Not set: as IBM's default, three seconds; 0 waits for a key.
    let timeout = if profile(b"RecoveryTimeout").is_empty() { 3 } else { timeout };
    out("\x1b[2J\x1b[H");
    out("rs2 Recovery Choices\r\n\r\n");
    if list.is_empty() {
        out("There are no archives of the desktop yet (the Archive page of the\r\n");
        out("Desktop's settings makes one at each start when it is set to).\r\n\r\n");
    } else {
        out("Choose an archive to start with the desktop and the system files\r\n");
        out("it holds (the files they replace are kept):\r\n\r\n");
        for (i, (dir, when)) in list.iter().take(9).enumerate() {
            let name = core::str::from_utf8(fileops::name_of(dir)).unwrap_or("?");
            out(&alloc::format!("   {}  Archive {} of {}\r\n", i + 1, name, core::str::from_utf8(when).unwrap_or("")));
        }
        out("\r\n");
    }
    out("   C  A command prompt before the desktop starts\r\n");
    out("   Enter  The current desktop\r\n\r\n");
    let mut left = timeout * 10;
    let mut shown = u32::MAX;
    let choice = loop {
        if timeout != 0 {
            let s = left.div_ceil(10);
            if s != shown {
                shown = s;
                out(&alloc::format!("\rThe current desktop starts in {s:2} seconds. "));
            }
            if left == 0 {
                break None;
            }
            left -= 1;
        }
        match key() {
            Some(k) if k.ch == b'\r' || k.ch == 0x1b => break None,
            Some(k) if k.ch == b'c' || k.ch == b'C' => break Some(0),
            Some(k) if (b'1'..=b'9').contains(&k.ch) && ((k.ch - b'0') as usize) <= list.len().min(9) => break Some((k.ch - b'0') as usize),
            Some(_) => {
                // Any other key stops the countdown.
                if timeout != 0 && left > 0 {
                    left = u32::MAX / 2;
                    out("\r                                              \r");
                }
            }
            None => {}
        }
        unsafe { DosSleep(100) };
    };
    out("\r\n");
    match choice {
        None => Choice::Desktop,
        Some(0) => Choice::CommandPrompt,
        Some(i) => {
            let (dir, when) = &list[i - 1];
            out(&alloc::format!("Restoring the archive of {}...\r\n", core::str::from_utf8(when).unwrap_or("")));
            let config = restore(dir);
            if config {
                out("CONFIG.SYS was restored: the system restarts to use it.\r\n");
                unsafe {
                    DosSleep(2000);
                    DosShutdown(0);
                    Rs2PowerOff(1);
                }
            }
            Choice::Desktop
        }
    }
}

/// Put an archive's files back; true when CONFIG.SYS changed.
fn restore(dir: &[u8]) -> bool {
    let drive = boot_drive() as char;
    let kept = fileops::join(&location(), b"REPLACED");
    if fileops::exists(&kept) {
        fileops::delete_tree(&kept);
        fileops::delete_path(&kept);
    }
    let k = crate::z(&kept);
    unsafe { DosCreateDir(k.as_ptr(), core::ptr::null_mut()) };
    let mut config = false;
    for (name, to) in [(&b"OS2.INI"[..], "OS2\\"), (b"OS2SYS.INI", "OS2\\"), (b"CONFIG.SYS", ""), (b"STARTUP.CMD", "")] {
        let src = fileops::join(dir, name);
        if !fileops::exists(&src) {
            continue;
        }
        let dst = [alloc::format!("{drive}:\\{to}").as_bytes(), name].concat();
        if name == b"CONFIG.SYS" {
            config = read_file(&src) != read_file(&dst);
        }
        let (s, d) = (crate::z(&src), crate::z(&dst));
        if fileops::exists(&dst) {
            let old = crate::z(&fileops::join(&kept, name));
            unsafe { DosCopy(d.as_ptr(), old.as_ptr(), 1) };
        }
        // DCPY_EXISTING
        let rc = unsafe { DosCopy(s.as_ptr(), d.as_ptr(), 1) };
        out(&alloc::format!("  {}: {}\r\n", core::str::from_utf8(name).unwrap_or("?"), if rc == 0 { "restored" } else { "not restored" }));
    }
    let arch_desk = fileops::join(dir, b"DESKTOP");
    if fileops::is_dir(&arch_desk) {
        let desk = alloc::format!("{drive}:\\DESKTOP").into_bytes();
        let mut nothing = |_: &[u8], _: u32| true;
        // The desktop goes aside (moved on one drive, else copied), then
        // the archive's copy takes its place.
        if fileops::exists(&desk) {
            fileops::move_path(&desk, &fileops::join(&kept, b"DESKTOP"), true, &mut nothing);
            if fileops::exists(&desk) {
                fileops::delete_tree(&desk);
                fileops::delete_path(&desk);
            }
        }
        let rc = fileops::copy_tree(&arch_desk, &desk, true, &mut nothing);
        out(&alloc::format!("  The desktop: {}\r\n", if rc == 0 { "restored" } else { "not restored" }));
    }
    config
}
