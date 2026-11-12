//! The printer pages' controls that IBM's WPPRINT has and printobj.rs's
//! first pages did not (studied with Ghidra: InitInputBinComboBox,
//! SetSepFileInputBin, SavePDPRQ8Property, the View page's Network job
//! view):
//!
//! - Print options: the **input bin** the separator page comes from
//!   (PMSPL's PrtQuery/PrtSet, rs2's PostScript printers' bins), and
//!   **print times for large jobs only**: PRQINFO8's flag, with rs2's own
//!   size limit beside it (IBM's page has only the check box; its limits
//!   are set elsewhere). Jobs up to the limit print at any time; larger
//!   ones wait for the start and stop times.
//! - View: the **Network job view**, a network printer's job view showing
//!   all of its jobs or only those of the user logged on
//!   (wpSet/QueryRemoteOptions); local printers have it disabled.

use alloc::vec::Vec;
use core::ptr::null;

use crate::book;
use crate::classes_gen::m;
use crate::pm::*;
use crate::setup::{check, checked, spin, spin_init, spin_value};
use crate::util::{cstr, z};
use crate::wps;

unsafe extern "C" {
    #[link_name = "PMSPL.397"]
    fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.398"]
    fn SplSetQueue(comp: *const u8, q: *const u8, level: u32, buf: *const u8, cb: u32, parm: u32) -> u32;
    #[link_name = "PMSPL.603"]
    fn PrtQuery(comp: *const u8, dev: *const u8, fl: u32, cmd: u32, pin: *const u8, cbin: u32, out: *mut u8, pcb: *mut u32) -> u32;
    #[link_name = "PMSPL.604"]
    fn PrtSet(comp: *const u8, dev: *const u8, fl: u32, cmd: u32, data: *const u8, cb: u32) -> u32;
}

pub const ID_BIN: u16 = 20612;
pub const ID_BINLABEL: u16 = 20613;
pub const ID_LARGEONLY: u16 = 20614;
pub const ID_LIMITKB: u16 = 20615;
pub const ID_ALLJOBS: u16 = 20704;
pub const ID_OWNJOBS: u16 = 20705;
pub const ID_NETVIEW: u16 = 20709;

const BIDI_Q_INPUTBINS: u32 = 0xA017;
const BIDI_SET_INPUTBIN: u32 = 0xA018;
const BIN_RECORD: usize = 96;
const LIMIT_TIMES_ONLY_LARGE: u32 = 0x10;
const CBS_DROPDOWNLIST: u32 = 0x0004;

// ---- Print options ----

/// The Print options page's input bin (in the separator page's box).
pub fn bin_items() -> Vec<Item> {
    alloc::vec![
        item(WC_STATIC, b"Input bin", WS_VISIBLE | 0x0001, (104, 86, 40, 8), ID_BINLABEL),
        item(2, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | CBS_DROPDOWNLIST, (146, 46, 82, 48), ID_BIN),
    ]
}

/// Print times for large jobs only, and the size that makes a job large.
pub fn limit_items() -> Vec<Item> {
    alloc::vec![
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"Print ~times only for jobs over", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (6, 2, 140, 10), ID_LARGEONLY),
        spin((148, 1, 46), ID_LIMITKB),
        book::label(b"KB", (198, 3, 20)),
    ]
}

/// The printer's input bins, and which the separator page uses.
fn bins(q: &[u8]) -> Option<(Vec<Vec<u8>>, usize)> {
    let qz = z(q);
    let mut cb = 0u32;
    unsafe { PrtQuery(null(), qz.as_ptr(), 1, BIDI_Q_INPUTBINS, null(), 0, core::ptr::null_mut(), &mut cb) };
    if cb < 8 {
        return None;
    }
    let mut buf = alloc::vec![0u8; cb as usize];
    if unsafe { PrtQuery(null(), qz.as_ptr(), 1, BIDI_Q_INPUTBINS, null(), 0, buf.as_mut_ptr(), &mut cb) } != 0 {
        return None;
    }
    let n = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let mut names = Vec::new();
    let mut cur = 0;
    for i in 0..n {
        let r = 8 + i * BIN_RECORD;
        let Some(rec) = buf.get(r..r + BIN_RECORD) else { break };
        names.push(cstr(rec[32..].as_ptr()).to_vec());
        if rec[4] & 1 != 0 {
            cur = i;
        }
    }
    Some((names, cur))
}

/// rs2's PRQINFO8: (flags, the most kilobytes); see PMSPL's api.rs.
fn limits(q: &[u8]) -> Option<(u32, u32)> {
    let mut buf = [0u8; 256];
    let mut need = 0;
    if unsafe { SplQueryQueue(null(), z(q).as_ptr(), 8, buf.as_mut_ptr(), buf.len() as u32, &mut need) } != 0 {
        return None;
    }
    let rd = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    Some((rd(8), rd(24)))
}

fn set_limits(q: &[u8], large_only: bool, kb: u32) {
    let mut buf = [0u8; 256];
    let mut need = 0;
    let qz = z(q);
    if unsafe { SplQueryQueue(null(), qz.as_ptr(), 8, buf.as_mut_ptr(), buf.len() as u32, &mut need) } != 0 {
        return;
    }
    let mut d = buf[..60].to_vec();
    let flags = u32::from_le_bytes([d[8], d[9], d[10], d[11]]);
    let flags = if large_only { flags | LIMIT_TIMES_ONLY_LARGE } else { flags & !LIMIT_TIMES_ONLY_LARGE };
    d[8..12].copy_from_slice(&flags.to_le_bytes());
    d[24..28].copy_from_slice(&kb.to_le_bytes());
    // The reserved strings, as IBM's page clears them.
    for o in (28..60).step_by(4) {
        d[o..o + 4].copy_from_slice(&0u32.to_le_bytes());
    }
    unsafe { SplSetQueue(null(), qz.as_ptr(), 8, d.as_ptr(), d.len() as u32, 0) };
}

/// The Print options page's WM_INITDLG: the bins (disabled when the
/// printer has none to choose) and the limits.
pub fn init_print_options(h: u32, q: &[u8]) {
    match bins(q) {
        Some((names, cur)) if !names.is_empty() => {
            for n in &names {
                let t = z(n);
                unsafe { WinSendDlgItemMsg(h, ID_BIN as u32, LM_INSERTITEM, 0xffff, t.as_ptr() as u32) };
            }
            unsafe { WinSendDlgItemMsg(h, ID_BIN as u32, LM_SELECTITEM, cur as u32, 1) };
        }
        _ => unsafe {
            WinEnableWindow(WinWindowFromID(h, ID_BIN as u32), 0);
            WinEnableWindow(WinWindowFromID(h, ID_BINLABEL as u32), 0);
        },
    }
    let (flags, kb) = limits(q).unwrap_or((0, 0));
    check(h, ID_LARGEONLY, flags & LIMIT_TIMES_ONLY_LARGE != 0);
    spin_init(h, ID_LIMITKB, 1, 99999, if kb == 0 { 1024 } else { kb as i32 });
}

/// The Print options page's WM_CONTROL for these controls; true when it
/// was one of them.
pub fn print_options_control(h: u32, q: &[u8], id: u16, code: u32, initializing: bool) -> bool {
    match id {
        // CBN_LBSELECT
        ID_BIN if code == 4 => {
            let i = unsafe { WinSendDlgItemMsg(h, ID_BIN as u32, LM_QUERYSELECTION, 0xffff, 0) } as i32;
            if i >= 0 && !initializing {
                let mut rec = [0u8; BIN_RECORD];
                let n = item_text(h, ID_BIN as u32);
                let k = n.len().min(63);
                rec[32..32 + k].copy_from_slice(&n[..k]);
                unsafe { PrtSet(null(), z(q).as_ptr(), 1, BIDI_SET_INPUTBIN, rec.as_ptr(), rec.len() as u32) };
            }
            true
        }
        ID_LARGEONLY | ID_LIMITKB => {
            let changed = (id == ID_LARGEONLY && code == 1) || (id == ID_LIMITKB && code == SPBN_CHANGE);
            if changed && !initializing {
                let kb = spin_value(h, ID_LIMITKB).filter(|v| (1..=99999).contains(v)).unwrap_or(1024) as u32;
                set_limits(q, checked(h, ID_LARGEONLY), kb);
            }
            true
        }
        _ => false,
    }
}

const SPBN_CHANGE: u32 = 0x20d;

// ---- View: the network job view ----

pub fn network_view_items() -> Vec<Item> {
    alloc::vec![
        // SS_GROUPBOX
        item(WC_STATIC, b"Network job view", WS_VISIBLE | 0x0002, (4, 20, 90, 34), ID_NETVIEW),
        crate::setup::radio(b"Show ~all jobs", (10, 37, 80), ID_ALLJOBS, true),
        crate::setup::radio(b"Show ~own jobs", (10, 26, 80), ID_OWNJOBS, false),
    ]
}

/// The View page's WM_INITDLG: a network printer's choice; disabled for
/// local printers.
pub fn init_view(h: u32, o: usize) {
    let mut all = 1u32;
    let mut refresh = 0u32;
    wps::call("WPPrinter", m::WPPrinter::wpQueryRemoteOptions, &[o, &mut refresh as *mut u32 as usize, &mut all as *mut u32 as usize]);
    check(h, ID_ALLJOBS, all != 0);
    check(h, ID_OWNJOBS, all == 0);
    if !wps::is(o, "WPRPrinter") {
        for id in [ID_NETVIEW, ID_ALLJOBS, ID_OWNJOBS] {
            unsafe { WinEnableWindow(WinWindowFromID(h, id as u32), 0) };
        }
    }
}

/// The View page's WM_CONTROL for the network job view.
pub fn view_control(h: u32, o: usize, id: u16, code: u32, initializing: bool) -> bool {
    if !matches!(id, ID_ALLJOBS | ID_OWNJOBS) {
        return false;
    }
    if code == 1 && !initializing {
        wps::call("WPPrinter", m::WPPrinter::wpSetRemoteOptions, &[o, 0, checked(h, ID_ALLJOBS) as usize]);
    }
    true
}
