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
    #[link_name = "PMWIN.884"]
    fn WinStartTimer(hab: u32, hwnd: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    fn WinStopTimer(hab: u32, hwnd: u32, id: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.735"]
    fn WinEnableWindow(hwnd: u32, enable: u32) -> u32;
}

const CLASS: &[u8] = b"rs2PlayFilePalette\0";
const WC_BUTTON: usize = 0xffff_0003;
const WS_VISIBLE: u32 = 0x8000_0000;
const WC_SLIDER: usize = 0xffff_0026;
const SLM_SETSLIDERINFO: u32 = 0x371;
const SLM_QUERYSLIDERINFO: u32 = 0x36c;
const SMA_SLIDERARMPOSITION: usize = 3;
const SMA_INCREMENTVALUE: usize = 1;
const CX: i32 = 200;
const CY: i32 = 70;

struct Palette {
    frame: u32,
    client: u32,
    old_proc: usize,
    file: Vec<u8>,
    title: Vec<u8>,
    alias: u32,
    open: bool,
    playing: bool,
    /// The state observer (IBM's: observer(state, error, user); state 0
    /// playing, 1 stopped) and its user data.
    observer: usize,
    user: usize,
    slider: u32,
    stop_button: u32,
    /// The position (0-100) the slider shows.
    position: u32,
}

/// Call the state observer: the arguments both on the stack and in
/// EAX/EDX/ECX, for _System and _Optlink callers alike.
fn notify(p: &Palette, state: u32, err: u32) {
    if p.observer == 0 {
        return;
    }
    let (f, user) = (p.observer, p.user);
    unsafe {
        core::arch::asm!(
            "push ecx", "push edx", "push eax", "call {f}", "add esp, 12",
            f = in(reg) f,
            inout("eax") state => _, inout("edx") err => _, inout("ecx") user => _,
        );
    }
}

fn set_state(p: &mut Palette, playing: bool, err: u32) {
    p.playing = playing;
    if p.stop_button != 0 {
        unsafe { WinEnableWindow(p.stop_button, playing as u32) };
    }
    unsafe {
        if playing {
            WinStartTimer(0, p.client, 1, 300);
        } else {
            WinStopTimer(0, p.client, 1);
        }
    }
    notify(p, if playing { 0 } else { 1 }, err);
}

fn set_slider(p: &mut Palette, pos: u32) {
    p.position = pos.min(100);
    if p.slider != 0 {
        unsafe { WinSendMsg(p.slider, SLM_SETSLIDERINFO, SMA_SLIDERARMPOSITION | (SMA_INCREMENTVALUE << 16), p.position as usize) };
    }
}

fn status(p: &Palette, item: &str) -> alloc::string::String {
    let c = z(&alloc::format!("status pfpal{} {item} wait", p.alias));
    let mut buf = [0u8; 64];
    unsafe { mciSendString(c.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, p.client, 0) };
    buf.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()
}

/// Every 300 ms while playing: the slider follows the position, and the
/// end of the file stops the palette.
fn tick(p: &mut Palette) {
    if !p.playing || !p.open {
        return;
    }
    let len: u64 = status(p, "length").trim().parse().unwrap_or(0);
    let pos: u64 = status(p, "position").trim().parse().unwrap_or(0);
    if len > 0 {
        set_slider(p, (pos * 100 / len) as u32);
    }
    if status(p, "mode").trim() != "playing" {
        set_slider(p, if len > 0 && pos >= len { 100 } else { p.position });
        set_state(p, false, 0);
    }
}

fn seek_percent(p: &mut Palette, pct: u32) {
    set_slider(p, pct);
    if !p.open {
        return;
    }
    let len: u64 = status(p, "length").trim().parse().unwrap_or(0);
    let to = len * p.position as u64 / 100;
    let was = p.playing;
    send(&alloc::format!("seek pfpal{} to {to} wait", p.alias), p.client);
    if was {
        send(&alloc::format!("play pfpal{}", p.alias), p.client);
    }
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
    let rc = start(p);
    set_state(p, rc == 0, rc);
    rc
}

fn start(p: &mut Palette) -> u32 {
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
    } else if p.position >= 100 || p.position == 0 {
        send(&alloc::format!("seek pfpal{} to start wait", p.alias), p.client);
    }
    send(&alloc::format!("play pfpal{}", p.alias), p.client)
}

fn stop(p: &mut Palette) -> u32 {
    if !p.open {
        return 0;
    }
    let rc = send(&alloc::format!("stop pfpal{} wait", p.alias), p.client);
    if p.playing {
        set_state(p, false, 0);
    }
    rc
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
        // Position in percent of the file (IBM's slider is 0-100).
        0x100a => {
            seek_percent(p, (mp1 & 0xffff) as u32);
            1
        }
        0x100b => p.position as usize,
        0x1006..=0x1009 => 0,
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
        if msg == 0x24 {
            // WM_TIMER
            tick(p);
            return 0;
        }
        if msg == 0x30 && (mp1 & 0xffff) == 3 && matches!(mp1 >> 16, 1 | 2) {
            // WM_CONTROL from the slider (SLN_CHANGE, SLN_SLIDERTRACK).
            let pos = unsafe { WinSendMsg(p.slider, SLM_QUERYSLIDERINFO, SMA_SLIDERARMPOSITION | (SMA_INCREMENTVALUE << 16), 0) } as u32 & 0xffff;
            if mp1 >> 16 == 1 && pos != p.position {
                seek_percent(p, pos);
            }
            return 0;
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
    observer: usize, user: usize, _a: u32, _b: u32, title: *const u8, _c: u32,
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
        PALETTES.push(Palette {
            frame, client, old_proc: 0, file: Vec::new(), title: t[..t.len() - 1].to_vec(), alias, open: false,
            playing: false, observer, user, slider: 0, stop_button: 0, position: 0,
        });
        let old = WinSubclassWindow(frame, crate::exports::__pfpal_frame_proc as usize);
        if let Some(p) = find(frame) {
            p.old_proc = old;
        }
        let mut stop_button = 0;
        for (id, text, bx) in [(1u32, &b"Play\0"[..], 4), (2, b"Stop\0", 70)] {
            let b = WinCreateWindow(client, WC_BUTTON, text.as_ptr(), WS_VISIBLE, bx, 4, 60, 30, client, 3, id, 0, 0);
            if id == 2 {
                stop_button = b;
            }
        }
        // SLDCDATA: 101 positions (0-100).
        let sld: [u16; 6] = [12, 0, 101, 0, 0, 0];
        let slider = WinCreateWindow(client, WC_SLIDER, b"\0".as_ptr(), WS_VISIBLE, 4, 38, CX - 12, 24, client, 3, 3, sld.as_ptr() as usize, 0);
        if let Some(p) = find(frame) {
            p.slider = slider;
            p.stop_button = stop_button;
            set_state(p, false, 0);
        }
        let (w, h) = if cx < 0 || cy < 0 { (CX, CY) } else { (cx, cy) };
        WinSetWindowPos(frame, behind, x, y, w, h, 0x47);
        let _ = WinQueryWindow(frame, 5);
        frame
    }
}
