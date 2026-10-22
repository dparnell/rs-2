//! Shutting down (the desktop's "Shut down...", WinShutdownSystem), as
//! RS2SHELL does it: after a confirmation, the text sessions end, every
//! PM program gets WM_SAVEAPPLICATION and WM_CLOSE (a program may still
//! ask about unsaved work), the shell waits for their windows to go
//! (asking after a while whether to end the rest), the objects are
//! saved, then DosShutdown flushes the file systems and Rs2PowerOff turns
//! the machine off (ACPI S5) or restarts it. If the machine cannot be
//! turned off, a screen says it is safe to do so.

use alloc::vec::Vec;

use crate::pm::*;
use crate::view;

pub const TID_SHUTDOWN: u32 = 0x5701;

struct State {
    restart: bool,
    ticks: u32,
    status: u32,
    timer_window: u32,
}

static mut STATE: Option<State> = None;

#[allow(static_mut_refs)]
fn state() -> Option<&'static mut State> {
    unsafe { STATE.as_mut() }
}

const ID_OFF: u16 = 501;
const ID_RESTART: u16 = 502;

extern "C" fn ask_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            unsafe { WinSendDlgItemMsg(h, ID_OFF as u32, BM_SETCHECK, 1, 0) };
            center(h);
            1
        }
        WM_COMMAND => {
            let r = if a & 0xffff == DID_OK {
                if unsafe { WinSendDlgItemMsg(h, ID_RESTART as u32, BM_QUERYCHECK, 0, 0) } != 0 { 2 } else { 1 }
            } else {
                0
            };
            unsafe { WinDismissDlg(h, r) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// The desktop's window (the shutdown's owner and timer window).
fn desktop_client() -> (u32, u32) {
    view::wins().iter().find(|w| w.desktop).map_or((HWND_DESKTOP, 0), |w| (w.frame, w.client))
}

/// Ask, then shut down or restart.
pub fn ask() {
    if state().is_some() {
        return;
    }
    let radio = |t: &[u8], y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (40, y, 150, 10), id);
    let tpl = template(dialog_frame(b"Shutdown", 200, 92), alloc::vec![
        item(WC_STATIC, b"#12", WS_VISIBLE | 0x000b, (8, 56, 20, 20), 0xffff),
        item(WC_STATIC, b"Close all programs and", WS_VISIBLE | 0x0001, (40, 72, 150, 8), 0xffff),
        radio(b"~Shut down the computer", 56, ID_OFF, true),
        radio(b"~Restart the computer", 42, ID_RESTART, false),
        item(WC_BUTTON, b"OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 5, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 5, 50, 14), DID_CANCEL as u16),
    ]);
    let owner = desktop_client().0;
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, ask_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    if r == 1 || r == 2 {
        begin(r == 2);
    }
}

fn my_pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe {
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pid
    }
}

/// Top-level windows of other processes that look like programs' frames.
fn program_frames() -> Vec<u32> {
    let me = my_pid();
    let mut out = Vec::new();
    let mut h = unsafe { WinQueryWindow(HWND_DESKTOP, QW_TOP) };
    let mut n = 0;
    while h != 0 && n < 1000 {
        n += 1;
        let pid = window_pid(h);
        let frame = unsafe { WinWindowFromID(h, FID_TITLEBAR) != 0 || WinWindowFromID(h, FID_CLIENT) != 0 };
        if pid != 0 && pid != me && frame {
            out.push(h);
        }
        h = unsafe { WinQueryWindow(h, QW_NEXT) };
    }
    out
}

fn status(text: &[u8]) {
    if let Some(s) = state() {
        if s.status != 0 {
            let t = z(text);
            unsafe { WinSetDlgItemText(s.status, 600, t.as_ptr()) };
        }
    }
}

extern "C" fn status_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            center(h);
            1
        }
        WM_COMMAND | WM_CLOSE => 0,
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// Close the programs (asked or not), then turn off or restart.
pub fn begin(restart: bool) {
    if state().is_some() {
        return;
    }
    let tpl = template(dialog_frame(b"Shutdown", 220, 40), alloc::vec![item(WC_STATIC, b"Closing programs...", WS_VISIBLE | 0x0001 | 0x0100 | 0x0400, (8, 8, 204, 24), 600)]);
    let st = unsafe { WinCreateDlg(HWND_DESKTOP, 0, status_proc as *const () as u32, tpl.as_ptr(), 0) };
    if st != 0 {
        unsafe { WinSetWindowPos(st, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER) };
    }
    let client = desktop_client().1;
    unsafe { STATE = Some(State { restart, ticks: 0, status: st, timer_window: client }) };
    let mut list = [VioSession { sid: 0, flags: 0, changes: 0, leader: 0, title: [0; 64] }; 32];
    let mut found = 0;
    unsafe { Rs2VioQuerySessions(list.as_mut_ptr(), 32, &mut found, 1) };
    for s in list.iter().take(found.min(32) as usize) {
        if s.flags & VS_PM == 0 {
            unsafe { Rs2VioControl(s.sid, 3, 0) };
        }
    }
    for f in program_frames() {
        post(f, WM_SAVEAPPLICATION, 0, 0);
        post(f, WM_CLOSE, 0, 0);
    }
    if client != 0 {
        unsafe { WinStartTimer(WinQueryAnchorBlock(HWND_DESKTOP), client, TID_SHUTDOWN, 500) };
    } else {
        finish();
    }
}

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

/// Every half second (the desktop's timer): done when no program window
/// is left.
pub fn tick() {
    let Some(s) = state() else { return };
    s.ticks += 1;
    let ticks = s.ticks;
    let left = program_frames();
    if left.is_empty() {
        finish();
        return;
    }
    status(&alloc::format!("Closing programs... ({} left)", left.len()).into_bytes());
    if ticks == 40 {
        let (client, st) = (s.timer_window, s.status);
        unsafe { WinStopTimer(WinQueryAnchorBlock(HWND_DESKTOP), client, TID_SHUTDOWN) };
        let names: Vec<u8> = left
            .iter()
            .take(5)
            .flat_map(|&f| {
                let mut t = frame_title(f);
                t.insert(0, b'\n');
                t
            })
            .collect();
        let mut text = b"These programs have not ended:".to_vec();
        text.extend_from_slice(&names);
        text.extend_from_slice(b"\n\nEnd them and shut down anyway?");
        if message(st, &text, b"Shutdown", MB_YESNO | MB_WARNING) == MBID_YES {
            let mut pids: Vec<u32> = left.iter().map(|&f| window_pid(f)).filter(|&p| p != 0).collect();
            pids.sort();
            pids.dedup();
            for p in pids {
                unsafe { os2::DosKillProcess(1, p) };
            }
            unsafe { os2::DosSleep(500) };
            finish();
        } else {
            if st != 0 {
                unsafe { WinDestroyWindow(st) };
            }
            unsafe { STATE = None };
        }
    }
}

fn finish() {
    let (client, restart, st) = state().map_or((0, false, 0), |s| (s.timer_window, s.restart, s.status));
    if client != 0 {
        unsafe { WinStopTimer(WinQueryAnchorBlock(HWND_DESKTOP), client, TID_SHUTDOWN) };
    }
    status(b"Saving the desktop...");
    crate::shell::save_all();
    status(b"Writing the disks...");
    unsafe {
        DosShutdown(0);
        Rs2PowerOff(restart as u32);
    }
    if st != 0 {
        unsafe { WinDestroyWindow(st) };
    }
    safe_screen();
}

extern "C" fn safe_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_PAINT => {
            let mut r = RectL::default();
            unsafe {
                let hps = WinBeginPaint(h, 0, &mut r);
                let mut all = RectL::default();
                WinQueryWindowRect(h, &mut all);
                WinFillRect(hps, &all, -1);
                let t = b"Shutdown has completed.\n\nIt is now safe to turn off your computer.";
                WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut all, -2, -1, 0x0100 | 0x0400 | 0x4000);
                WinEndPaint(hps);
            }
            0
        }
        WM_CHAR | WM_BUTTON1DOWN | WM_BUTTON2DOWN | WM_CLOSE => 1,
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}

fn safe_screen() {
    unsafe {
        let hab = WinQueryAnchorBlock(HWND_DESKTOP);
        WinRegisterClass(hab, b"WPS_ShutdownDone\0".as_ptr(), safe_proc, 0, 0);
        let (w, h) = screen();
        let mut fcf = 0u32;
        let mut client = 0;
        let f = WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"WPS_ShutdownDone\0".as_ptr(), b"\0".as_ptr(), 0, 0, 2, &mut client);
        WinSetWindowPos(f, HWND_TOP, 0, 0, w, h, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetCapture(HWND_DESKTOP, client);
    }
}
