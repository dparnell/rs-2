//! What rs2's multimedia applets share: the media control interface
//! (MDM's mciSendString), PM windows and controls, the open and save
//! dialogs, the profile and time formatting.

#![no_std]

extern crate alloc;

#[path = "../../appinst/src/pm.rs"]
#[allow(dead_code, unused_imports)]
pub mod pm;

use alloc::string::String;
use alloc::vec::Vec;

pub use pm::*;
pub use utils::args;

pub const WC_BUTTON: &[u8] = b"#3\0";
pub const WC_STATIC: &[u8] = b"#5\0";
pub const WC_ENTRYFIELD: &[u8] = b"#6\0";
pub const WC_SCROLLBAR: &[u8] = b"#8\0";
pub const WC_LISTBOX: &[u8] = b"#7\0";
pub const WC_NOTEBOOK: &[u8] = b"#40\0";
pub const BS_AUTOCHECKBOX: u32 = 2;
pub const BS_AUTORADIOBUTTON: u32 = 4;
pub const BM_QUERYCHECK: u32 = 0x0124;
pub const BM_SETCHECK: u32 = 0x0125;
pub const SBS_HORZ: u32 = 0;
pub const SBS_VERT: u32 = 1;
pub const SBM_QUERYPOS: u32 = 0x01a2;
pub const SB_ENDSCROLL: u32 = 7;
pub const WM_TIMER: u32 = 0x0024;
pub const MM_MCINOTIFY: u32 = 0x0500;
pub const DT_CENTER: u32 = 0x0100;
pub const DT_RIGHT: u32 = 0x0200;
pub const DT_TOP: u32 = 0x0000;
pub const DT_BOTTOM: u32 = 0x0800;
pub const CLR_BLUE: i32 = 1;
pub const CLR_RED: i32 = 2;
pub const CLR_GREEN: i32 = 4;
pub const CLR_CYAN: i32 = 5;
pub const CLR_YELLOW: i32 = 6;
pub const CLR_DARKGREEN: i32 = 12;
pub const CLR_DARKCYAN: i32 = 13;
pub const FCF_MENU: u32 = 0x0004;
pub const FCF_DLGBORDER: u32 = 0x0200;
pub const FCF_ICON: u32 = 0x4000;

unsafe extern "C" {
    #[link_name = "PMWIN.704"]
    pub fn WinCalcFrameRect(h: u32, r: *mut RectL, client: u32) -> u32;
    #[link_name = "PMWIN.884"]
    pub fn WinStartTimer(hab: u32, h: u32, id: u32, ms: u32) -> u32;
    #[link_name = "PMWIN.885"]
    pub fn WinStopTimer(hab: u32, h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.842"]
    pub fn WinQueryWindowTextLength(h: u32) -> i32;
    #[link_name = "PMCTLS.4"]
    fn WinFileDlg(parent: u32, owner: u32, fild: *mut u8) -> u32;
    #[link_name = "MDM.2"]
    fn mciSendString(cmd: *const u8, ret: *mut u8, cb: u32, hwnd: u32, user: u32) -> u32;
    #[link_name = "MDM.3"]
    fn mciGetErrorString(err: u32, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "RS2API.307"]
    pub fn Rs2AudioSetVolume(h: u32, vol: u32) -> u32;
    #[link_name = "RS2API.300"]
    pub fn Rs2AudioQuery(devices: *mut u32) -> u32;
    #[link_name = "RS2API.312"]
    pub fn Rs2AudioSetInput(source: u32, taken: *mut u32) -> u32;
}

/// OS/2 text (one character per byte) NUL-terminated.
pub fn zs(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = os2::text_bytes(s).into_owned();
    v.push(0);
    v
}

pub fn text_of(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

// ---- The media control interface ----

/// A string command; (return code, result text). `hwnd` gets the
/// notifications of a NOTIFY command.
pub fn mci_to(cmd: &str, hwnd: u32) -> (u32, String) {
    let c = zs(cmd);
    let mut ret = [0u8; 256];
    let rc = unsafe { mciSendString(c.as_ptr(), ret.as_mut_ptr(), ret.len() as u32, hwnd, 0) };
    (rc & 0xffff, text_of(&ret))
}

pub fn mci(cmd: &str) -> (u32, String) {
    mci_to(cmd, 0)
}

/// A number the device answers (0 when it does not).
pub fn mci_num(cmd: &str) -> u32 {
    let (rc, t) = mci(cmd);
    if rc != 0 {
        return 0;
    }
    t.trim().parse().unwrap_or(0)
}

pub fn mci_error(rc: u32) -> String {
    let mut b = [0u8; 160];
    unsafe { mciGetErrorString(rc, b.as_mut_ptr(), b.len() as u32) };
    text_of(&b)
}

/// "m:ss.t" for milliseconds.
pub fn fmt_ms(ms: u32) -> String {
    let t = ms / 100;
    alloc::format!("{}:{:02}.{}", t / 600, t / 10 % 60, t % 10)
}

/// "m:ss".
pub fn fmt_s(ms: u32) -> String {
    let s = ms / 1000;
    alloc::format!("{}:{:02}", s / 60, s % 60)
}

/// The file's name without its directory.
pub fn base_name(path: &str) -> &str {
    path.rsplit(['\\', '/', ':']).next().unwrap_or(path)
}

pub fn extension(path: &str) -> String {
    let b = base_name(path);
    match b.rsplit_once('.') {
        Some((_, e)) => e.to_ascii_uppercase(),
        None => String::new(),
    }
}

// ---- The profile ----

/// OS2.INI's `MMPM2` application: rs2's multimedia settings.
pub const APP: &[u8] = b"MMPM2\0";

pub fn ini_get(key: &str) -> Option<String> {
    let k = zs(key);
    let mut b = [0u8; 300];
    let n = unsafe { PrfQueryProfileString(1, APP.as_ptr(), k.as_ptr(), core::ptr::null(), b.as_mut_ptr(), b.len() as u32) };
    (n > 0).then(|| text_of(&b))
}

pub fn ini_set(key: &str, v: &str) {
    let (k, d) = (zs(key), zs(v));
    unsafe { PrfWriteProfileString(1, APP.as_ptr(), k.as_ptr(), d.as_ptr()) };
}

/// The master volume (0-100) and whether the sound is muted, as Volume
/// Control keeps them.
pub fn master_volume() -> (u32, bool) {
    let v = ini_get("MasterVolume").and_then(|v| v.trim().parse().ok()).unwrap_or(100u32).min(100);
    let mute = ini_get("Mute").is_some_and(|m| m.trim() == "1");
    (v, mute)
}

pub fn set_master_volume(v: u32, mute: bool) {
    ini_set("MasterVolume", &alloc::format!("{}", v.min(100)));
    ini_set("Mute", if mute { "1" } else { "0" });
    unsafe { Rs2AudioSetVolume(0, if mute { 0 } else { v.min(100) }) };
}

// ---- PM ----

pub struct Pm {
    pub hab: u32,
    pub hmq: u32,
}

/// PM for this thread (None without PM).
pub fn pm_start() -> Option<Pm> {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    (hmq != 0).then_some(Pm { hab, hmq })
}

impl Pm {
    pub fn run(&self) {
        let mut q = QMsg::default();
        while unsafe { WinGetMsg(self.hab, &mut q, 0, 0, 0) } != 0 {
            unsafe { WinDispatchMsg(self.hab, &q) };
        }
    }
}

impl Drop for Pm {
    fn drop(&mut self) {
        unsafe {
            WinDestroyMsgQueue(self.hmq);
            WinTerminate(self.hab);
        }
    }
}

/// A frame window with a client of class `class` (registered here), its
/// client area `cw` x `ch`, centred; (frame, client).
pub fn main_window(pm: &Pm, class: &[u8], proc_: WndProc, title: &str, cw: i32, ch: i32, sizable: bool) -> (u32, u32) {
    unsafe { WinRegisterClass(pm.hab, class.as_ptr(), proc_, WS_CLIPCHILDREN | 4, 4) };
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_TASKLIST | FCF_ICON | if sizable { FCF_SIZEBORDER | FCF_MAXBUTTON } else { FCF_DLGBORDER };
    let mut client = 0u32;
    let t = zs(title);
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, class.as_ptr(), t.as_ptr(), WS_CLIPCHILDREN, 0, 1, &mut client) };
    let mut r = RectL { l: 0, b: 0, r: cw, t: ch };
    unsafe { WinCalcFrameRect(frame, &mut r, 0) };
    let (fw, fh) = (r.r - r.l, r.t - r.b);
    let (sw, sh) = (sysval(SV_CXSCREEN), sysval(SV_CYSCREEN));
    unsafe { WinSetWindowPos(frame, HWND_TOP, (sw - fw) / 2, (sh - fh) / 2, fw, fh, SWP_SIZE | SWP_MOVE) };
    (frame, client)
}

pub fn show(frame: u32) {
    unsafe { WinSetWindowPos(frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
}

pub fn control(parent: u32, class: &[u8], text: &str, style: u32, id: u32, x: i32, y: i32, cx: i32, cy: i32) -> u32 {
    let t = zs(text);
    unsafe { WinCreateWindow(parent, class.as_ptr(), t.as_ptr(), style | WS_VISIBLE, x, y, cx, cy, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

pub fn button(parent: u32, text: &str, id: u32, x: i32, y: i32, cx: i32, cy: i32) -> u32 {
    control(parent, WC_BUTTON, text, BS_PUSHBUTTON | WS_TABSTOP, id, x, y, cx, cy)
}

pub fn label(parent: u32, text: &str, id: u32, x: i32, y: i32, cx: i32, cy: i32, align: u32) -> u32 {
    control(parent, WC_STATIC, text, SS_TEXT | align | DT_VCENTER, id, x, y, cx, cy)
}

pub fn scrollbar(parent: u32, vertical: bool, id: u32, x: i32, y: i32, cx: i32, cy: i32) -> u32 {
    control(parent, WC_SCROLLBAR, "", if vertical { SBS_VERT } else { SBS_HORZ }, id, x, y, cx, cy)
}

/// Range and position of a scroll bar (and the thumb's share).
pub fn set_scroll(h: u32, pos: u32, max: u32, visible: u32) {
    unsafe {
        WinSendMsg(h, SBM_SETSCROLLBAR, pos, max << 16);
        WinSendMsg(h, SBM_SETTHUMBSIZE, visible | max << 16, 0);
    }
}

pub fn item(parent: u32, id: u32) -> u32 {
    unsafe { WinWindowFromID(parent, id) }
}

pub fn set_text(h: u32, s: &str) {
    let t = zs(&s.replace('\n', "\r\n"));
    unsafe { WinSetWindowText(h, t.as_ptr()) };
}

pub fn get_text(h: u32) -> String {
    let n = unsafe { WinQueryWindowTextLength(h) }.max(0) as usize;
    let mut b = alloc::vec![0u8; n + 1];
    let got = unsafe { WinQueryWindowText(h, b.len() as i32, b.as_mut_ptr()) }.max(0) as usize;
    b[..got.min(n)].iter().map(|&c| c as char).collect()
}

pub fn checked(h: u32) -> bool {
    unsafe { WinSendMsg(h, BM_QUERYCHECK, 0, 0) != 0 }
}

pub fn set_check(h: u32, on: bool) {
    unsafe { WinSendMsg(h, BM_SETCHECK, on as u32, 0) };
}

pub fn enable(h: u32, on: bool) {
    unsafe { WinEnableWindow(h, on as u32) };
}

pub fn message_box(owner: u32, title: &str, text: &str, error: bool) {
    let (t, x) = (zs(title), zs(text));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, x.as_ptr(), t.as_ptr(), 0, MB_OK | MB_MOVEABLE | if error { MB_ERROR } else { 0 }) };
}

pub fn fill(hps: u32, r: &RectL, c: i32) {
    unsafe { WinFillRect(hps, r, c) };
}

/// Text at a point (the baseline), in colour `c`.
pub fn text_at(hps: u32, x: i32, y: i32, s: &str, c: i32) {
    let t = os2::text_bytes(s);
    unsafe {
        GpiSetColor(hps, c);
        GpiCharStringAt(hps, &PointL { x, y }, t.len() as i32, t.as_ptr());
    }
}

pub fn line(hps: u32, x0: i32, y0: i32, x1: i32, y1: i32, c: i32) {
    unsafe {
        GpiSetColor(hps, c);
        GpiMove(hps, &PointL { x: x0, y: y0 });
        GpiLine(hps, &PointL { x: x1, y: y1 });
    }
}

/// A filled box (DRO_FILL, 1) or its outline (DRO_OUTLINE, 2).
pub fn rect(hps: u32, l: i32, b: i32, r: i32, t: i32, c: i32, filled: bool) {
    unsafe {
        GpiSetColor(hps, c);
        GpiMove(hps, &PointL { x: l, y: b });
        GpiBox(hps, if filled { 1 } else { 2 }, &PointL { x: r, y: t }, 0, 0);
    }
}

/// Paint a whole client in one colour (WM_PAINT).
pub fn paint_plain(h: u32, c: i32) {
    let mut r = RectL::default();
    let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
    fill(hps, &r, c);
    unsafe { WinEndPaint(hps) };
}

/// The open or save-as dialog: the full name of the file chosen.
pub fn file_dialog(owner: u32, title: &str, pattern: &str, save: bool) -> Option<String> {
    let mut fd = alloc::vec![0u8; 328];
    let t = zs(title);
    fd[0..4].copy_from_slice(&328u32.to_le_bytes());
    // FDS_CENTER, FDS_OPEN_DIALOG or FDS_SAVEAS_DIALOG.
    let fl: u32 = 1 | if save { 0x200 } else { 0x100 };
    fd[4..8].copy_from_slice(&fl.to_le_bytes());
    fd[20..24].copy_from_slice(&(t.as_ptr() as u32).to_le_bytes());
    let p = os2::text_bytes(pattern);
    fd[52..52 + p.len().min(259)].copy_from_slice(&p[..p.len().min(259)]);
    unsafe { WinFileDlg(HWND_DESKTOP, owner, fd.as_mut_ptr()) };
    let ret = u32::from_le_bytes(fd[12..16].try_into().unwrap());
    if ret != 1 {
        return None;
    }
    let name = text_of(&fd[52..312]);
    (!name.is_empty()).then_some(name)
}

/// A bitmap drawn from a DIB in memory (bottom-up rows, `bits` 1/4/8/24
/// with `palette` as B, G, R, 0) into rectangle `dst` (stretched).
pub fn draw_dib(hps: u32, width: u32, height: u32, bits: u16, palette: &[[u8; 4]], data: &[u8], dst: RectL) {
    let mut bmi = alloc::vec![0u8; 16 + palette.len() * 4];
    bmi[0..4].copy_from_slice(&16u32.to_le_bytes());
    bmi[4..8].copy_from_slice(&width.to_le_bytes());
    bmi[8..12].copy_from_slice(&height.to_le_bytes());
    bmi[12..14].copy_from_slice(&1u16.to_le_bytes());
    bmi[14..16].copy_from_slice(&bits.to_le_bytes());
    for (i, c) in palette.iter().enumerate() {
        bmi[16 + i * 4..20 + i * 4].copy_from_slice(c);
    }
    let pts = [PointL { x: dst.l, y: dst.b }, PointL { x: dst.r - 1, y: dst.t - 1 }, PointL { x: 0, y: 0 }, PointL { x: width as i32, y: height as i32 }];
    // ROP_SRCCOPY, BBO_IGNORE.
    unsafe { GpiDrawBits(hps, data.as_ptr(), bmi.as_ptr(), 4, pts.as_ptr(), 0xcc, 2) };
}

/// A rectangle of `w` x `h` fitted (aspect kept, not enlarged beyond
/// `max_zoom`) and centred in `r`.
pub fn fit(r: RectL, w: i32, h: i32, max_zoom: i32) -> RectL {
    let (aw, ah) = ((r.r - r.l).max(1), (r.t - r.b).max(1));
    let (w, h) = (w.max(1), h.max(1));
    // Scale = min(aw/w, ah/h, max_zoom), in 1/1024ths.
    let s = ((aw as i64 * 1024 / w as i64).min(ah as i64 * 1024 / h as i64)).min(max_zoom as i64 * 1024);
    let (fw, fh) = ((w as i64 * s / 1024) as i32, (h as i64 * s / 1024) as i32);
    let (x, y) = (r.l + (aw - fw) / 2, r.b + (ah - fh) / 2);
    RectL { l: x, b: y, r: x + fw, t: y + fh }
}

// ---- Files ----

/// The whole file (None if it cannot be read).
pub fn read_file(name: &str) -> Option<Vec<u8>> {
    let z = zs(name);
    let (mut h, mut act) = (0u32, 0u32);
    // Open if it exists, deny write, read only.
    if unsafe { os2::DosOpen(z.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 32768];
    loop {
        let mut n = 0;
        if unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(h) };
    Some(out)
}

/// Write a whole file (created or replaced).
pub fn write_file(name: &str, data: &[u8]) -> bool {
    let z = zs(name);
    let (mut h, mut act) = (0u32, 0u32);
    if unsafe { os2::DosOpen(z.as_ptr(), &mut h, &mut act, data.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut ok = true;
    for piece in data.chunks(32768) {
        let mut n = 0;
        if unsafe { os2::DosWrite(h, piece.as_ptr(), piece.len() as u32, &mut n) } != 0 || n as usize != piece.len() {
            ok = false;
            break;
        }
    }
    unsafe { os2::DosClose(h) };
    ok
}
