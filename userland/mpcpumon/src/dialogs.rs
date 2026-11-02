//! The CPU Monitor's dialogs (OS/2's ids): processor status, sample rate
//! and a processor's graph colour.

use alloc::vec::Vec;

use pmkit::*;

use crate::{app, trace, COLORS, DLG_COLOR, DLG_RATE, DLG_STATUS, TITLE};

pub const ID_CPULIST: u32 = 302;
pub const ID_RATE: u32 = 304;
pub const ID_COLORLIST: u32 = 308;
const ID_HELP: u32 = 0xbfe;

// ---- Processor status ----

/// The status chosen in the dialog, by processor (true: online).
static mut WANT: Vec<bool> = Vec::new();

fn status_line(i: usize, online: bool) -> Vec<u8> {
    alloc::format!("CPU {:<3}                      {}", i + 1, if online { "Online" } else { "Offline" }).into_bytes()
}

extern "C" fn status_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let want = unsafe { &mut *core::ptr::addr_of_mut!(WANT) };
    match m {
        WM_INITDLG => {
            for (i, &on) in want.iter().enumerate() {
                lb_add(h, ID_CPULIST, &status_line(i, on));
            }
            dlg_msg(h, ID_CPULIST, LM_SELECTITEM, 0, 1);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_CPULIST)) };
            1
        }
        WM_CONTROL if lo(mp1) == ID_CPULIST && hi(mp1) == LN_ENTER => {
            // Enter turns the selected processor's status around.
            let sel = dlg_msg(h, ID_CPULIST, LM_QUERYSELECTION, LIT_FIRST, 0) as usize;
            if sel < want.len() {
                want[sel] = !want[sel];
                let t = z(&status_line(sel, want[sel]));
                dlg_msg(h, ID_CPULIST, LM_SETITEMTEXT, sel as u32, t.as_ptr() as u32);
            }
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == DID_OK || id == DID_CANCEL {
                unsafe { WinDismissDlg(h, id) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Status: list the processors with their status; Enter changes the
/// selected one's, OK asks the system for the changes.
pub fn status() {
    let a = app();
    let Some(p) = a.perf.as_mut() else { return };
    p.refresh_status();
    unsafe { WANT = p.online.clone() };
    let tpl = template(
        dialog_frame(b"Processor Status", DLG_STATUS, 230, 130),
        alloc::vec![
            label(b"Select a processor and press Enter to change", (12, 114, 210, 8), DT_LEFT),
            label(b"its status between online and offline, then", (12, 106, 210, 8), DT_LEFT),
            label(b"choose OK to apply the changes.", (12, 98, 210, 8), DT_LEFT),
            label(b"Processor", (14, 86, 60, 8), DT_LEFT),
            label(b"Status", (84, 86, 60, 8), DT_LEFT),
            list_box((12, 30, 206, 56), ID_CPULIST, 0),
            button(b"OK", (12, 8, 50), DID_OK, BS_DEFAULT),
            button(b"Cancel", (68, 8, 50), DID_CANCEL, 0),
            help_button((124, 8, 50), ID_HELP),
        ],
    );
    if run_dialog(a.frame, status_proc, &tpl, 0) != DID_OK {
        return;
    }
    let want = unsafe { core::mem::take(&mut *core::ptr::addr_of_mut!(WANT)) };
    let Some(p) = a.perf.as_mut() else { return };
    for (i, &on) in want.iter().enumerate() {
        if on == p.online[i] {
            continue;
        }
        let rc = unsafe { DosSetProcessorStatus(i as u32 + 1, on as u32) };
        trace(alloc::format!("DosSetProcessorStatus({}, {}) = {rc}", i + 1, on as u32).as_bytes());
        if rc == 0 {
            p.online[i] = on;
        } else {
            let t = alloc::format!("The status of CPU {} could not be changed (error {rc}).", i + 1);
            message(a.frame, t.as_bytes(), b"Error", MB_OK | MB_ERROR);
        }
    }
    p.refresh_status();
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.448"]
    fn DosSetProcessorStatus(id: u32, status: u32) -> u32;
}

// ---- Sample rate ----

extern "C" fn rate_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            dlg_msg(h, ID_RATE, EM_SETTEXTLIMIT, 5, 0);
            set_item_text(h, ID_RATE, &num(app().set.rate as i64));
            dlg_msg(h, ID_RATE, EM_SETSEL, mp(0, 5), 0);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_RATE)) };
            1
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == DID_OK {
                match parse_num(&item_text(h, ID_RATE)) {
                    Some(v) if (50..=30000).contains(&v) => app().set.rate = v as u32,
                    _ => {
                        message(h, b"Type the time between samples in milliseconds, from 50 to 30000.", TITLE, MB_OK | MB_WARNING);
                        return 0;
                    }
                }
            }
            if id == DID_OK || id == DID_CANCEL {
                unsafe { WinDismissDlg(h, id) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Sample Rate: the time between samples in milliseconds; true when it
/// was changed.
pub fn sample_rate() -> bool {
    let a = app();
    let tpl = template(
        dialog_frame(b"Sample Rate", DLG_RATE, 176, 50),
        alloc::vec![
            label(b"Sampling rate (milliseconds):", (6, 30, 116, 8), DT_LEFT),
            entry((126, 30, 40, 8), ID_RATE, 0),
            button(b"OK", (6, 6, 46), DID_OK, BS_DEFAULT),
            button(b"Cancel", (58, 6, 46), DID_CANCEL, 0),
            help_button((110, 6, 46), ID_HELP),
        ],
    );
    run_dialog(a.frame, rate_proc, &tpl, 0) == DID_OK
}

// ---- A processor's colour ----

static mut COLOR_CPU: usize = 0;

extern "C" fn color_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let cpu = unsafe { COLOR_CPU };
    match m {
        WM_INITDLG => {
            for c in COLORS.iter() {
                lb_add(h, ID_COLORLIST, c.0);
            }
            let cur = app().set.colors[cpu.min(16)] as u32;
            dlg_msg(h, ID_COLORLIST, LM_SELECTITEM, cur, 1);
            dlg_msg(h, ID_COLORLIST, LM_SETTOPINDEX, cur.saturating_sub(2), 0);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_COLORLIST)) };
            1
        }
        WM_CONTROL if lo(mp1) == ID_COLORLIST && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == DID_OK {
                let sel = dlg_msg(h, ID_COLORLIST, LM_QUERYSELECTION, LIT_FIRST, 0);
                if (sel as usize) < COLORS.len() {
                    app().set.colors[cpu.min(16)] = sel as u8;
                }
            }
            if id == DID_OK || id == DID_CANCEL {
                unsafe { WinDismissDlg(h, id) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Processor Graph Color > CPU n: choose the colour of its graph.
pub fn cpu_color(cpu: usize) {
    unsafe { COLOR_CPU = cpu };
    let title = alloc::format!("CPU {} Color", cpu + 1).into_bytes();
    let tpl = template(
        dialog_frame(&title, DLG_COLOR, 160, 90),
        alloc::vec![
            list_box((14, 28, 132, 54), ID_COLORLIST, 0),
            button(b"OK", (8, 6, 44), DID_OK, BS_DEFAULT),
            button(b"Cancel", (58, 6, 44), DID_CANCEL, 0),
            help_button((108, 6, 44), ID_HELP),
        ],
    );
    run_dialog(app().frame, color_proc, &tpl, 0);
}
