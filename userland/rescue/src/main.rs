//! OS2\RESCUE.EXE: rs2's way out of a hang, in the spirit of OS/2's
//! WatchCat. Started detached from CONFIG.SYS (RUN=), it waits on the
//! event semaphore `\SEM32\RS2\RESCUE`, which the kernel posts when
//! Ctrl+Alt+W is pressed, whatever the foreground session is doing (also
//! while PM is stuck: the keyboard is the kernel's). It then shows a
//! text pop-up (VioPopUp) with the running processes, marking the PM
//! programs and the one keeping PM's lock (held all through half a
//! second: everything in PM then waits for it), where:
//!
//! - Up and Down choose a process, E ends it (after Y to confirm);
//!   ending PMSHELL.EXE restarts the desktop;
//! - S saves a report (the processes and every thread's state, as
//!   Ctrl+Alt+F12 writes on the serial port) to RS2HANG.TXT on the boot
//!   drive;
//! - F5 refreshes, Esc goes back to what was on the screen.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use os2::vio::{call16, ptr_words};
use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "VIOCALLS.1"]
    fn VIOENDPOPUP();
    #[link_name = "VIOCALLS.7"]
    fn VIOSCROLLUP();
    #[link_name = "VIOCALLS.11"]
    fn VIOPOPUP();
    #[link_name = "VIOCALLS.13"]
    fn VIOWRTCHARSTR();
    #[link_name = "VIOCALLS.32"]
    fn VIOSETCURTYPE();
    #[link_name = "VIOCALLS.52"]
    fn VIOWRTNCELL();
    #[link_name = "KBDCALLS.4"]
    fn KBDCHARIN();
    #[link_name = "RS2API.24"]
    fn Rs2QueryProcesses(buf: *mut u8, cb: u32) -> u32;
    #[link_name = "RS2API.25"]
    fn Rs2QueryThreadReport(buf: *mut u8, cb: u32) -> u32;
    #[link_name = "RS2API.26"]
    fn Rs2KillProcess(pid: u32) -> u32;
}

const SEM: &[u8] = b"\\SEM32\\RS2\\RESCUE\0";
const ROWS: usize = 25;
const COLS: usize = 80;
/// Process rows on the screen.
const LIST_TOP: usize = 4;
const LIST_ROWS: usize = ROWS - LIST_TOP - 4;

const NORMAL: u8 = 0x1f;
const HEAD: u8 = 0x70;
const SELECTED: u8 = 0x2f;
const ALERT: u8 = 0x4f;

/// What the 16-bit calls read and write: one page, so nothing crosses a
/// 64 KB tile.
#[repr(C, align(4096))]
struct Page([u8; 4096]);
static mut PAGE: Page = Page([0; 4096]);

fn page() -> *mut u8 {
    unsafe { (*core::ptr::addr_of_mut!(PAGE)).0.as_mut_ptr() }
}

fn call(e: unsafe extern "C" fn(), words: &[u16]) -> u16 {
    unsafe { call16(e as *const () as usize, words) }
}

/// Text at (row, col) in the attribute `attr`, the rest of the row too
/// when `fill`.
fn put(row: usize, col: usize, text: &[u8], attr: u8, fill: bool) {
    let p = page();
    let n = text.len().min(COLS - col);
    let cells = if fill { COLS - col } else { n };
    unsafe {
        *p = b' ';
        *p.add(1) = attr;
    }
    let c = ptr_words(p);
    call(VIOWRTNCELL, &[c[0], c[1], cells as u16, row as u16, col as u16, 0]);
    if n == 0 {
        return;
    }
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), p.add(16), n) };
    let t = ptr_words(unsafe { p.add(16) });
    call(VIOWRTCHARSTR, &[t[0], t[1], n as u16, row as u16, col as u16, 0]);
}

/// A key: (character, scan code).
fn key() -> (u8, u8) {
    let p = page();
    let k = ptr_words(p);
    // IO_WAIT
    if call(KBDCHARIN, &[k[0], k[1], 0, 0]) != 0 {
        return (0x1b, 0x01);
    }
    unsafe { (*p, *p.add(1)) }
}

struct Proc {
    pid: u32,
    ppid: u32,
    threads: u32,
    pm: bool,
    lock: bool,
    name: String,
}

/// The processes, with the PM lock's holder only if it held the lock
/// all through half a second (PM takes it briefly all the time).
fn processes() -> Vec<Proc> {
    let mut v = sample();
    for _ in 0..5 {
        unsafe { DosSleep(100) };
        let again = sample();
        for p in v.iter_mut() {
            p.lock &= again.iter().any(|q| q.pid == p.pid && q.lock);
        }
    }
    v
}

fn sample() -> Vec<Proc> {
    let mut buf = alloc::vec![0u8; 16384];
    let n = unsafe { Rs2QueryProcesses(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    buf.truncate(n);
    let text = String::from_utf8_lossy(&buf).into_owned();
    text.lines()
        .filter_map(|l| {
            let mut w = l.split(' ');
            let pid = w.next()?.parse().ok()?;
            let ppid = w.next()?.parse().ok()?;
            let threads = w.next()?.parse().ok()?;
            let flags = w.next()?;
            let name = w.collect::<Vec<_>>().join(" ");
            Some(Proc { pid, ppid, threads, pm: flags.contains('P'), lock: flags.contains('L'), name })
        })
        .collect()
}

fn boot_drive() -> u8 {
    let mut v = [0u32; 1];
    // QSV_BOOT_DRIVE
    unsafe { DosQuerySysInfo(5, 5, v.as_mut_ptr(), 4) };
    if (1..=26).contains(&v[0]) { b'A' + v[0] as u8 - 1 } else { b'C' }
}

/// Append a report to RS2HANG.TXT on the boot drive; its name.
fn save_report(procs: &[Proc]) -> Option<String> {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let mut r = alloc::format!(
        "==== rs2 rescue report {:04}-{:02}-{:02} {:02}:{:02}:{:02} ====\r\nPID PARENT THREADS FLAGS NAME (P: PM program, L: holds PM's lock)\r\n",
        dt.year, dt.month, dt.day, dt.hours, dt.minutes, dt.seconds
    );
    for p in procs {
        r.push_str(&alloc::format!("{} {} {} {}{} {}\r\n", p.pid, p.ppid, p.threads, if p.pm { "P" } else { "-" }, if p.lock { "L" } else { "-" }, p.name));
    }
    let mut buf = alloc::vec![0u8; 256 * 1024];
    let n = unsafe { Rs2QueryThreadReport(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    for line in String::from_utf8_lossy(&buf[..n]).lines() {
        r.push_str(line);
        r.push_str("\r\n");
    }
    r.push_str("\r\n");
    let name = alloc::format!("{}:\\RS2HANG.TXT", boot_drive() as char);
    let mut z = name.clone().into_bytes();
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS | CREATE_IF_NEW; DENYWRITE | WRITEONLY.
    if unsafe { DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x11, 0x21, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut at = 0u32;
    let mut done = 0u32;
    let ok = unsafe { DosSetFilePtr(hf, 0, 2, &mut at) } == 0 && unsafe { DosWrite(hf, r.as_ptr(), r.len() as u32, &mut done) } == 0 && done as usize == r.len();
    unsafe { DosClose(hf) };
    ok.then_some(name)
}

fn draw(procs: &[Proc], sel: usize, top: usize, status: &str, status_attr: u8) {
    put(0, 0, b" rs2 Rescue (Ctrl+Alt+W)", HEAD, true);
    put(1, 0, b"", NORMAL, true);
    let lock = procs.iter().find(|p| p.lock);
    let line = match lock {
        Some(p) => alloc::format!(" {} (process {}) keeps PM's lock: PM waits for it. Ending it frees PM.", p.name, p.pid),
        None => String::from(" PM is not blocked: no process keeps its lock."),
    };
    put(2, 0, line.as_bytes(), if lock.is_some() { ALERT } else { NORMAL }, true);
    put(3, 0, b"   PID  PARENT  THREADS  NAME          NOTE", NORMAL, true);
    for r in 0..LIST_ROWS {
        let i = top + r;
        let Some(p) = procs.get(i) else {
            put(LIST_TOP + r, 0, b"", NORMAL, true);
            continue;
        };
        let note = if p.lock {
            "holds PM's lock"
        } else if p.name.eq_ignore_ascii_case("PMSHELL") {
            "the desktop (restarts when ended)"
        } else if p.name.eq_ignore_ascii_case("RESCUE") {
            "this screen"
        } else if p.pm {
            "PM program"
        } else {
            ""
        };
        let text = alloc::format!(" {:5}  {:6}  {:7}  {:12}  {}", p.pid, p.ppid, p.threads, p.name, note);
        put(LIST_TOP + r, 0, text.as_bytes(), if i == sel { SELECTED } else { NORMAL }, true);
    }
    put(ROWS - 4, 0, b"", NORMAL, true);
    put(ROWS - 3, 0, status.as_bytes(), status_attr, true);
    put(ROWS - 2, 0, b"", NORMAL, true);
    put(ROWS - 1, 0, b" Up/Down choose   E end the process   S save a report   F5 refresh   Esc back", HEAD, true);
}

/// The pop-up, until Esc.
fn rescue() {
    let p = page();
    // VP_WAIT, opaque.
    unsafe { *(p as *mut u16) = 1 };
    let w = ptr_words(p);
    if call(VIOPOPUP, &[w[0], w[1], 0]) != 0 {
        return;
    }
    // Hide the cursor: VIOCURSORINFO with attr -1.
    unsafe {
        core::ptr::write_bytes(p, 0, 8);
        *(p.add(6) as *mut u16) = 0xffff;
    }
    let c = ptr_words(p);
    call(VIOSETCURTYPE, &[c[0], c[1], 0]);
    let mut procs = processes();
    let mut sel = procs.iter().position(|p| p.lock).unwrap_or(0);
    let mut top = 0;
    let mut status = String::from(" Choose the process that does not respond and press E to end it.");
    let mut status_attr = NORMAL;
    loop {
        sel = sel.min(procs.len().saturating_sub(1));
        if sel < top {
            top = sel;
        } else if sel >= top + LIST_ROWS {
            top = sel + 1 - LIST_ROWS;
        }
        draw(&procs, sel, top, &status, status_attr);
        status_attr = NORMAL;
        match key() {
            (0x1b, _) => break,
            (0 | 0xe0, 0x48) => sel = sel.saturating_sub(1),
            (0 | 0xe0, 0x50) => sel += 1,
            (0 | 0xe0, 0x3f) => {
                procs = processes();
                status = String::from(" Refreshed.");
            }
            (b'e' | b'E', _) | (0 | 0xe0, 0x53) => {
                let Some(p) = procs.get(sel) else { continue };
                if p.name.eq_ignore_ascii_case("RESCUE") {
                    status = String::from(" That is this screen.");
                    continue;
                }
                let (pid, name) = (p.pid, p.name.clone());
                draw(&procs, sel, top, &alloc::format!(" End {name} (process {pid})? Y ends it, any other key does not."), ALERT);
                if !matches!(key().0, b'y' | b'Y') {
                    status = String::from(" Not ended.");
                    continue;
                }
                let rc = unsafe { Rs2KillProcess(pid) };
                status = if rc == 0 { alloc::format!(" {name} (process {pid}) ended.") } else { alloc::format!(" {name} could not be ended (error {rc}).") };
                status_attr = if rc == 0 { NORMAL } else { ALERT };
                unsafe { DosSleep(300) };
                procs = processes();
            }
            (b's' | b'S', _) => {
                status = match save_report(&procs) {
                    Some(n) => alloc::format!(" Report saved to {n}."),
                    None => {
                        status_attr = ALERT;
                        String::from(" The report could not be saved.")
                    }
                };
            }
            _ => {}
        }
    }
    call(VIOENDPOPUP, &[0]);
}

fn main(_s: &os2::Startup) -> u32 {
    let mut hev = 0u32;
    // One copy: a second finds the semaphore there.
    if unsafe { DosCreateEventSem(SEM.as_ptr(), &mut hev, 0, 0) } != 0 {
        return 1;
    }
    loop {
        if unsafe { DosWaitEventSem(hev, 0xffff_ffff) } != 0 {
            unsafe { DosSleep(1000) };
            continue;
        }
        let mut n = 0;
        unsafe { DosResetEventSem(hev, &mut n) };
        rescue();
    }
}
