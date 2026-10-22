//! DISPLAY.EXE: the display settings. Lists the screen resolutions the
//! display driver can switch to (VMware SVGA II, virtio-gpu; a plain
//! firmware framebuffer has only its own) and switches while PM runs
//! (RS2API `Rs2SetDisplayMode`; PM lays the desktop out again). After
//! Apply the new resolution stays only if Keep is chosen within 15
//! seconds, otherwise the old one comes back, as on other systems. A
//! kept resolution is saved as `VIDEO=WxH` in CONFIG.SYS, which the
//! kernel applies at the next start.
//!
//! Usage: `DISPLAY` (the window), `DISPLAY WxH` (switch and save without
//! asking, e.g. from a script).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../appinst/src/pm.rs"]
#[allow(dead_code)]
mod pm;

use alloc::string::String;
use alloc::vec::Vec;

use pm::*;

os2::entry!(main);

const WC_BUTTON: &[u8] = b"#3\0";
const WC_STATIC: &[u8] = b"#5\0";
const WC_LISTBOX: &[u8] = b"#7\0";
const LN_SELECT: u32 = 1;
const WM_TIMER: u32 = 0x0024;

const ID_LABEL: u32 = 10;
const ID_MODES: u32 = 11;
const ID_STATUS: u32 = 12;
/// Apply, or Keep while asking.
const ID_APPLY: u32 = 13;
/// Close, or Revert while asking.
const ID_CLOSE: u32 = 14;
const TID_COUNT: u32 = 1;

/// Seconds to keep a new resolution without an answer.
const CONFIRM_SECONDS: u32 = 15;

static CLASS: &[u8] = b"Rs2Display\0";

unsafe extern "C" {
    #[link_name = "RS2API.22"]
    fn Rs2QueryDisplayModes(pul: *mut u32, max: u32, pc: *mut u32) -> u32;
    #[link_name = "RS2API.23"]
    fn Rs2SetDisplayMode(cx: u32, cy: u32) -> u32;
    #[link_name = "PMWIN.704"]
    fn WinCalcFrameRect(h: u32, r: *mut RectL, client: u32) -> u32;
    #[link_name = "PMWIN.884"]
    fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
}

struct App {
    hab: u32,
    frame: u32,
    client: u32,
    modes: Vec<(u32, u32)>,
    /// Asking whether to keep a new mode: the old one, the new one,
    /// seconds left.
    asking: Option<((u32, u32), (u32, u32), u32)>,
    /// The screen size the window was last centred on.
    centred: (i32, i32),
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().collect();
    v.push(0);
    v
}

fn item(id: u32) -> u32 {
    unsafe { WinWindowFromID(app().client, id) }
}

fn set_text(id: u32, s: &str) {
    let t = z(s);
    unsafe { WinSetWindowText(item(id), t.as_ptr()) };
}

fn screen() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

fn current() -> (u32, u32) {
    let (w, h) = screen();
    (w as u32, h as u32)
}

fn modes() -> Vec<(u32, u32)> {
    let mut v = [0u32; 64];
    let mut n = 0;
    if unsafe { Rs2QueryDisplayModes(v.as_mut_ptr(), 32, &mut n) } != 0 {
        return Vec::new();
    }
    (0..n.min(32) as usize).map(|i| (v[i * 2], v[i * 2 + 1])).collect()
}

/// "WxH" (also "W x H").
fn parse_mode(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// The boot drive's CONFIG.SYS.
fn config_path() -> Vec<u8> {
    let mut d = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(5, 5, d.as_mut_ptr(), 4) };
    let letter = if (1..=26).contains(&d[0]) { b'A' + d[0] as u8 - 1 } else { b'C' };
    let mut p = alloc::vec![letter];
    p.extend_from_slice(b":\\CONFIG.SYS\0");
    p
}

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; OPEN_SHARE_DENYNONE | OPEN_ACCESS_READONLY.
    if unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut got = 0;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(out)
}

fn write_file(path: &[u8], data: &[u8]) -> bool {
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_REPLACE_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW;
    // OPEN_SHARE_DENYWRITE | OPEN_ACCESS_WRITEONLY.
    if unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, data.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut done = 0;
    let rc = unsafe { os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut done) };
    unsafe { os2::DosClose(hf) };
    rc == 0 && done as usize == data.len()
}

/// CONFIG.SYS with its VIDEO= statement set to `w` x `h` (replacing
/// any there).
fn with_video(text: &[u8], w: u32, h: u32) -> Vec<u8> {
    let text = match text.iter().position(|&b| b == 0x1a) {
        Some(i) => &text[..i],
        None => text,
    };
    let mut out = Vec::new();
    for line in text.split_inclusive(|&b| b == b'\n') {
        let t: Vec<u8> = line.iter().copied().skip_while(|&b| b == b' ' || b == b'\t').collect();
        if t.len() >= 6 && t[..6].eq_ignore_ascii_case(b"VIDEO=") {
            continue;
        }
        out.extend_from_slice(line);
    }
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(alloc::format!("VIDEO={w}x{h}\r\n").as_bytes());
    out
}

/// Save the resolution for the next start.
fn save(w: u32, h: u32) -> bool {
    let path = config_path();
    let old = read_file(&path).unwrap_or_default();
    write_file(&path, &with_video(&old, w, h))
}

fn main(s: &os2::Startup) -> u32 {
    let args = String::from_utf8_lossy(s.args()).trim().to_ascii_uppercase();
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if hmq == 0 {
        os2::println!("DISPLAY: Presentation Manager is not available");
        return 1;
    }
    if let Some((w, h)) = parse_mode(&args) {
        let rc = unsafe { Rs2SetDisplayMode(w, h) };
        if rc == 0 {
            save(w, h);
        }
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return rc;
    }
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App { hab, frame: 0, client: 0, modes: modes(), asking: None, centred: (0, 0) });
        WinRegisterClass(hab, CLASS.as_ptr(), client_proc, WS_CLIPCHILDREN, 0);
    }
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_TASKLIST | 0x0200; // FCF_DLGBORDER
    let mut client = 0u32;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), b"Display\0".as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client) };
    app().frame = frame;
    app().client = client;
    build(client);
    centre(true);
    unsafe { WinSetFocus(HWND_DESKTOP, item(ID_MODES)) };
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

/// Put the window in the middle of the screen (again after the screen
/// changed size).
fn centre(show: bool) {
    let a = app();
    let (cx, cy) = (340, 260);
    let mut r = RectL { l: 0, b: 0, r: cx, t: cy };
    unsafe { WinCalcFrameRect(a.frame, &mut r, 0) };
    let (fw, fh) = (r.r - r.l, r.t - r.b);
    let (sw, sh) = screen();
    a.centred = (sw, sh);
    let fl = SWP_SIZE | SWP_MOVE | if show { SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER } else { 0 };
    unsafe { WinSetWindowPos(a.frame, HWND_TOP, (sw - fw) / 2, (sh - fh) / 2, fw, fh, fl) };
}

fn control(parent: u32, class: &[u8], text: &str, style: u32, id: u32, x: i32, y: i32, cx: i32, cy: i32) -> u32 {
    let t = z(text);
    unsafe { WinCreateWindow(parent, class.as_ptr(), t.as_ptr(), style | WS_VISIBLE, x, y, cx, cy, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

/// The controls (client coordinates, from the bottom left).
fn build(h: u32) {
    let label = SS_TEXT | DT_LEFT | DT_VCENTER;
    control(h, WC_STATIC, "Screen resolution:", label, ID_LABEL, 12, 228, 316, 20);
    control(h, WC_LISTBOX, "", WS_TABSTOP | WS_GROUP, ID_MODES, 12, 92, 316, 132);
    control(h, WC_STATIC, "", SS_TEXT | DT_LEFT | DT_WORDBREAK, ID_STATUS, 12, 48, 316, 40);
    control(h, WC_BUTTON, "~Apply", BS_PUSHBUTTON | WS_TABSTOP | WS_GROUP, ID_APPLY, 120, 10, 100, 30);
    control(h, WC_BUTTON, "Close", BS_PUSHBUTTON | WS_TABSTOP, ID_CLOSE, 228, 10, 100, 30);
    fill_list();
}

fn fill_list() {
    let cur = current();
    unsafe { WinSendMsg(item(ID_MODES), LM_DELETEALL, 0, 0) };
    let mut sel = 0;
    for (i, &(w, h)) in app().modes.iter().enumerate() {
        let mark = if (w, h) == cur { "  (current)" } else { "" };
        if (w, h) == cur {
            sel = i;
        }
        let t = z(&alloc::format!("{w} x {h}{mark}"));
        unsafe { WinSendMsg(item(ID_MODES), LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
    if app().modes.is_empty() {
        set_text(ID_STATUS, &alloc::format!("This display stays at {} x {}: it has no driver that can change the resolution.", cur.0, cur.1));
        unsafe { WinEnableWindow(item(ID_APPLY), 0) };
    } else {
        unsafe { WinSendMsg(item(ID_MODES), LM_SELECTITEM, sel as u32, 1) };
        set_text(ID_STATUS, &alloc::format!("The screen is at {} x {}.", cur.0, cur.1));
    }
}

fn selected() -> Option<(u32, u32)> {
    let i = unsafe { WinSendMsg(item(ID_MODES), LM_QUERYSELECTION, LIT_FIRST, 0) } as i32;
    app().modes.get(i as usize).copied().filter(|_| i >= 0)
}

/// Switch to the selected resolution and ask whether to keep it.
fn apply() {
    let Some((w, h)) = selected() else { return };
    let old = current();
    if (w, h) == old {
        return;
    }
    if unsafe { Rs2SetDisplayMode(w, h) } != 0 {
        set_text(ID_STATUS, &alloc::format!("The display cannot switch to {w} x {h}."));
        return;
    }
    app().asking = Some((old, (w, h), CONFIRM_SECONDS));
    set_text(ID_APPLY, "~Keep");
    set_text(ID_CLOSE, "~Revert");
    unsafe {
        WinEnableWindow(item(ID_MODES), 0);
        WinStartTimer(app().hab, app().client, TID_COUNT, 1000);
    }
    ask_text();
}

fn ask_text() {
    if let Some((_, (w, h), left)) = app().asking {
        set_text(ID_STATUS, &alloc::format!("Keep {w} x {h}? The old resolution comes back in {left} seconds."));
    }
}

/// The answer: keep the new resolution (and save it), or go back.
fn answer(keep: bool) {
    let Some((old, new, _)) = app().asking.take() else { return };
    unsafe { WinStopTimer(app().hab, app().client, TID_COUNT) };
    if !keep {
        unsafe { Rs2SetDisplayMode(old.0, old.1) };
    }
    set_text(ID_APPLY, "~Apply");
    set_text(ID_CLOSE, "Close");
    unsafe { WinEnableWindow(item(ID_MODES), 1) };
    fill_list();
    let (w, h) = if keep { new } else { old };
    if keep && !save(w, h) {
        set_text(ID_STATUS, &alloc::format!("The screen is at {w} x {h}. It could not be saved in CONFIG.SYS."));
    } else if keep {
        set_text(ID_STATUS, &alloc::format!("The screen is at {w} x {h} (saved for the next start)."));
    }
    unsafe { WinSetFocus(HWND_DESKTOP, item(ID_MODES)) };
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe {
                WinFillRect(hps, &r, CLR_PALEGRAY);
                WinEndPaint(hps);
            }
            0
        }
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            if id == ID_MODES && code == LN_ENTER {
                apply();
            } else if id == ID_MODES && code == LN_SELECT && app().asking.is_none() {
                let cur = current();
                if let Some((w, h)) = selected().filter(|&m| m != cur) {
                    set_text(ID_STATUS, &alloc::format!("The screen is at {} x {}. Apply switches to {w} x {h}.", cur.0, cur.1));
                }
            }
            0
        }
        WM_COMMAND => {
            match (mp1 & 0xffff, app().asking.is_some()) {
                (ID_APPLY, false) => apply(),
                (ID_APPLY, true) => answer(true),
                (ID_CLOSE, true) => answer(false),
                (ID_CLOSE, false) => unsafe {
                    WinPostMsg(h, WM_QUIT, 0, 0);
                },
                _ => {}
            }
            0
        }
        WM_TIMER if mp1 & 0xffff == TID_COUNT => {
            if app().centred != screen() {
                centre(false);
            }
            if let Some((_, _, left)) = app().asking.as_mut() {
                *left = left.saturating_sub(1);
                if *left == 0 {
                    answer(false);
                } else {
                    ask_text();
                }
            }
            0
        }
        // Closing while asking goes back to the old resolution.
        WM_CLOSE if app().asking.is_some() => {
            answer(false);
            unsafe { WinPostMsg(h, WM_QUIT, 0, 0) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}
