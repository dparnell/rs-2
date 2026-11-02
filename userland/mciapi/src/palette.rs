//! mciCreatePlayFilePalette (IBM's export 25): the small "play file"
//! palette window MMPM/2's players and IBM's OpenDoc multimedia parts
//! (MMODPTS) use to play a file. As IBM's (studied in IBM's MCIAPI.DLL,
//! PFPalProc), the returned frame window takes these messages:
//!
//! | Message | |
//! |---|---|
//! | 0x1000 | play the file |
//! | 0x1001 | stop |
//! | 0x1002 | mp1: the file (any MMIO name, compound ones too) |
//! | 0x1003 | mp1/mp2: buffer and size for the file name |
//! | 0x1004/0x1005 | the title (kept) |
//! | 0x100c | the optimal size (cx in the low word, cy in the high) |
//! | 0x100d | enable (TRUE) |
//!
//! rs2's palette is its own: a border frame with Play and Stop buttons;
//! the file plays on an MCI device opened for it (alias `pfpalN`).

use alloc::vec::Vec;

use crate::{cstr, mciSendString, z};

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(hwnd: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, class: *const u8, proc_: usize, style: u32, cb: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, flags: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: usize, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32,
                       owner: u32, behind: u32, id: u32, ctl: usize, pres: usize) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.929"]
    fn WinSubclassWindow(hwnd: u32, proc_: usize) -> usize;
    #[link_name = "PMWIN.864"]
    fn WinSetOwner(hwnd: u32, owner: u32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(hwnd: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(hwnd: u32, cmd: i32) -> u32;
}

const CLASS: &[u8] = b"rs2PlayFilePalette\0";
const WC_BUTTON: usize = 0xffff_0003;
const WS_VISIBLE: u32 = 0x8000_0000;
const CX: i32 = 136;
const CY: i32 = 40;

struct Palette {
    frame: u32,
    client: u32,
    old_proc: usize,
    file: Vec<u8>,
    title: Vec<u8>,
    alias: u32,
    open: bool,
}

static mut PALETTES: Vec<Palette> = Vec::new();
static mut NEXT_ALIAS: u32 = 1;
static mut REGISTERED: bool = false;

#[allow(static_mut_refs)]
fn find(hwnd: u32) -> Option<&'static mut Palette> {
    unsafe { PALETTES.iter_mut().find(|p| p.frame == hwnd || p.client == hwnd) }
}

fn send(cmd: &str, owner: u32) -> u32 {
    let c = z(cmd);
    let mut buf = [0u8; 64];
    unsafe { mciSendString(c.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, owner, 0) }
}

fn play(p: &mut Palette) -> u32 {
    if p.file.is_empty() {
        return 5017; // MCIERR_MISSING_PARAMETER
    }
    let f: alloc::string::String = p.file.iter().map(|&b| b as char).collect();
    if !p.open {
        let rc = send(&alloc::format!("open \"{f}\" alias pfpal{} shareable wait", p.alias), p.client);
        if rc != 0 {
            return rc;
        }
        p.open = true;
    } else {
        send(&alloc::format!("seek pfpal{} to start wait", p.alias), p.client);
    }
    send(&alloc::format!("play pfpal{}", p.alias), p.client)
}

fn stop(p: &mut Palette) -> u32 {
    if !p.open {
        return 0;
    }
    send(&alloc::format!("stop pfpal{} wait", p.alias), p.client)
}

fn close(p: &mut Palette) {
    if p.open {
        send(&alloc::format!("close pfpal{} wait", p.alias), p.client);
        p.open = false;
    }
}

/// The palette's own messages, sent to its frame (or client).
fn palette_message(p: &mut Palette, msg: u32, mp1: usize, mp2: usize) -> Option<usize> {
    Some(match msg {
        0x1000 => (play(p) == 0) as usize,
        0x1001 => (stop(p) == 0) as usize,
        0x1002 => {
            if mp1 == 0 {
                return Some(0);
            }
            close(p);
            p.file = cstr(mp1 as *const u8).bytes().collect();
            1
        }
        0x1003 => {
            if mp1 != 0 && mp2 > 0 {
                let n = p.file.len().min(mp2 - 1);
                unsafe {
                    core::ptr::copy_nonoverlapping(p.file.as_ptr(), mp1 as *mut u8, n);
                    *(mp1 as *mut u8).add(n) = 0;
                }
            }
            p.file.len()
        }
        0x1004 => {
            p.title = if mp1 == 0 { Vec::new() } else { cstr(mp1 as *const u8).bytes().collect() };
            1
        }
        0x1005 => p.title.as_ptr() as usize,
        0x100c => (CX as usize) | ((CY as usize) << 16),
        0x100d => 1,
        0x1006..=0x100b => 0,
        _ => return None,
    })
}

pub(crate) extern "C" fn frame_proc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize {
    let Some(p) = find(hwnd) else { return unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) } };
    if let Some(r) = palette_message(p, msg, mp1, mp2) {
        return r;
    }
    let old = p.old_proc;
    if msg == 2 {
        // WM_DESTROY
        close(p);
    }
    let f: extern "C" fn(u32, u32, usize, usize) -> usize = unsafe { core::mem::transmute(old) };
    f(hwnd, msg, mp1, mp2)
}

pub(crate) extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize {
    if let Some(p) = find(hwnd) {
        if let Some(r) = palette_message(p, msg, mp1, mp2) {
            return r;
        }
        if msg == 0x20 {
            // WM_COMMAND from the buttons.
            match mp1 & 0xffff {
                1 => {
                    play(p);
                }
                2 => {
                    stop(p);
                }
                _ => {}
            }
            return 0;
        }
    }
    unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
}


/// mciCreatePlayFilePalette(parent, style, x, y, cx, cy, owner, behind,
/// flags, observer, user, ..., title, ...): the palette's frame window
/// (not shown: its owner shows it), sized to its optimal size for a
/// negative cx or cy.
#[unsafe(no_mangle)]
#[allow(static_mut_refs)]
pub extern "C" fn mciCreatePlayFilePalette(
    parent: u32, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, _flags: u32,
    _observer: usize, _user: usize, _a: u32, _b: u32, title: *const u8, _c: u32,
) -> u32 {
    unsafe {
        let hab = WinQueryAnchorBlock(1);
        if !REGISTERED {
            REGISTERED = WinRegisterClass(hab, CLASS.as_ptr(), crate::exports::__pfpal_client_proc as usize, 4, 4) != 0;
        }
        let mut fl = 0x200u32; // FCF_BORDER
        let mut client = 0u32;
        let t = if title.is_null() { z("") } else { z(&cstr(title)) };
        let frame = WinCreateStdWindow(if parent == 0 { 1 } else { parent }, style & !WS_VISIBLE, &mut fl, CLASS.as_ptr(), t.as_ptr(), WS_VISIBLE, 0, 0x8008, &mut client);
        if frame == 0 {
            return 0;
        }
        WinSetOwner(frame, owner);
        let alias = NEXT_ALIAS;
        NEXT_ALIAS += 1;
        PALETTES.push(Palette { frame, client, old_proc: 0, file: Vec::new(), title: t[..t.len() - 1].to_vec(), alias, open: false });
        let old = WinSubclassWindow(frame, crate::exports::__pfpal_frame_proc as usize);
        if let Some(p) = find(frame) {
            p.old_proc = old;
        }
        for (id, text, bx) in [(1u32, &b"Play\0"[..], 4), (2, b"Stop\0", 70)] {
            WinCreateWindow(client, WC_BUTTON, text.as_ptr(), WS_VISIBLE, bx, 4, 60, 30, client, 3, id, 0, 0);
        }
        let (w, h) = if cx < 0 || cy < 0 { (CX, CY) } else { (cx, cy) };
        WinSetWindowPos(frame, behind, x, y, w, h, 0x47);
        let _ = WinQueryWindow(frame, 5);
        frame
    }
}
