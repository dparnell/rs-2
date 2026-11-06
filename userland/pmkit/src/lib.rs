//! pmkit: what rs2's own PM applets (Picture Viewer, Icon Editor,
//! Clipboard Viewer, Seek and Scan, Pulse, Check Disk, Format, Locale and
//! the games) share: the PM entry points and constants they use
//! (`sys`), menus, accelerator tables and dialog templates built in
//! memory (`tpl`), help instances made from a table of panel numbers
//! (`help`), OS2.INI settings (`ini`) and small helpers.
//!
//! The applets carry no resources: OS/2's equivalents load theirs from
//! their EXE or a resource DLL, rs2's make them in memory as E.EXE does.

#![no_std]
#![allow(non_snake_case)]
#![allow(dead_code)]

extern crate alloc;

pub mod consts;
pub mod gpi;
pub mod help;
pub mod ini;
pub mod progress;
pub mod sys;
pub mod tpl;

pub use consts::*;
pub use sys::*;
pub use tpl::*;

use alloc::vec::Vec;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn lo(v: u32) -> u32 {
    v & 0xffff
}

pub fn hi(v: u32) -> u32 {
    v >> 16
}

/// The signed x and y of a mouse message's mp1.
pub fn mouse_xy(mp1: u32) -> (i32, i32) {
    ((mp1 & 0xffff) as i16 as i32, (mp1 >> 16) as i16 as i32)
}

pub fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

pub fn post(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinPostMsg(h, m, a, b) }
}

/// A NUL-terminated copy.
pub fn z(t: &[u8]) -> Vec<u8> {
    let mut v = t.to_vec();
    v.push(0);
    v
}

/// Bytes up to a NUL.
pub fn until_nul(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// A NUL-terminated C string's bytes.
///
/// # Safety
/// `p` points at a NUL-terminated string (or is null).
pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

pub fn window_text(h: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 512];
    let n = unsafe { WinQueryWindowText(h, buf.len() as i32, buf.as_mut_ptr()) }.max(0) as usize;
    buf.truncate(n.min(512));
    buf
}

pub fn set_text(h: u32, t: &[u8]) {
    let s = z(t);
    unsafe { WinSetWindowText(h, s.as_ptr()) };
}

pub fn item_text(dlg: u32, id: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 1024];
    let n = unsafe { WinQueryDlgItemText(dlg, id, buf.len() as i32, buf.as_mut_ptr()) } as usize;
    buf.truncate(n.min(1024));
    buf
}

pub fn set_item_text(dlg: u32, id: u32, t: &[u8]) {
    let s = z(t);
    unsafe { WinSetDlgItemText(dlg, id, s.as_ptr()) };
}

pub fn checked(dlg: u32, id: u32) -> bool {
    unsafe { WinSendDlgItemMsg(dlg, id, BM_QUERYCHECK, 0, 0) != 0 }
}

pub fn set_check(dlg: u32, id: u32, on: bool) {
    unsafe { WinSendDlgItemMsg(dlg, id, BM_SETCHECK, on as u32, 0) };
}

pub fn enable(dlg: u32, id: u32, on: bool) {
    unsafe { WinEnableWindow(WinWindowFromID(dlg, id), on as u32) };
}

pub fn item(dlg: u32, id: u32) -> u32 {
    unsafe { WinWindowFromID(dlg, id) }
}

pub fn dlg_msg(dlg: u32, id: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendDlgItemMsg(dlg, id, m, a, b) }
}

/// Add a line to a list box; returns its index.
pub fn lb_add(dlg: u32, id: u32, text: &[u8]) -> u32 {
    let t = z(text);
    dlg_msg(dlg, id, LM_INSERTITEM, LIT_END, t.as_ptr() as u32)
}

pub fn lb_text(dlg: u32, id: u32, i: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 512];
    let n = dlg_msg(dlg, id, LM_QUERYITEMTEXT, mp(i, 512), buf.as_mut_ptr() as u32) as usize;
    buf.truncate(n.min(512));
    buf
}

/// A message box; returns the MBID_* of the button pressed.
pub fn message(owner: u32, text: &[u8], title: &[u8], style: u32) -> u32 {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 0, style | MB_MOVEABLE) }
}

/// A message box with a help id (MB_HELP sends WM_HELP with it).
pub fn message_id(owner: u32, text: &[u8], title: &[u8], id: u32, style: u32) -> u32 {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), id, style | MB_MOVEABLE) }
}

pub fn screen() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) }
}

pub fn window_pos(h: u32) -> Swp {
    let mut s = Swp::default();
    unsafe { WinQueryWindowPos(h, &mut s) };
    s
}

pub fn window_rect(h: u32) -> RectL {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(h, &mut r) };
    r
}

pub fn invalidate(h: u32) {
    unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
}

/// Put a dialog over its owner (centred on it, kept on the screen).
pub fn place_over(dlg: u32, owner: u32) {
    let (d, o) = (window_pos(dlg), window_pos(owner));
    let (sw, sh) = screen();
    let x = (o.x + (o.cx - d.cx) / 2).clamp(0, (sw - d.cx).max(0));
    let y = (o.y + (o.cy - d.cy) * 2 / 3).clamp(0, (sh - d.cy).max(0));
    unsafe { WinSetWindowPos(dlg, 0, x, y, 0, 0, SWP_MOVE) };
}

/// Centre a window on the screen.
pub fn center(h: u32) {
    let d = window_pos(h);
    let (sw, sh) = screen();
    unsafe { WinSetWindowPos(h, 0, ((sw - d.cx) / 2).max(0), ((sh - d.cy) / 2).max(0), 0, 0, SWP_MOVE) };
}

/// A menu item's attribute on or off (searching submenus).
pub fn menu_attr(menu: u32, id: u16, attr: u16, on: bool) {
    send(menu, MM_SETITEMATTR, mp(id as u32, 1), mp(attr as u32, if on { attr as u32 } else { 0 }));
}

pub fn menu_check(menu: u32, id: u16, on: bool) {
    menu_attr(menu, id, MIA_CHECKED, on);
}

pub fn menu_enable(menu: u32, id: u16, on: bool) {
    menu_attr(menu, id, MIA_DISABLED, !on);
}

pub fn menu_text(menu: u32, id: u16, text: &[u8]) {
    let t = z(text);
    send(menu, MM_SETITEMTEXT, id as u32, t.as_ptr() as u32);
}

/// A line on the serial port, `[prefix] text` (test programs read them).
pub fn trace(prefix: &str, s: &[u8]) {
    let mut t = alloc::vec![b'['];
    t.extend_from_slice(prefix.as_bytes());
    t.extend_from_slice(b"] ");
    t.extend_from_slice(s);
    t.push(0);
    unsafe { Rs2DebugTrace(t.as_ptr()) };
}

/// The boot drive's letter.
pub fn boot_drive() -> u8 {
    let mut boot = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(5, 5, boot.as_mut_ptr(), 4) };
    b'@' + boot[0] as u8
}

/// The program's arguments with the blanks around them (and quotes)
/// taken off.
pub fn arg(s: &os2::Startup) -> &'static [u8] {
    let args = s.args();
    let start = args.iter().position(|&c| c != b' ').unwrap_or(args.len());
    let end = args.iter().rposition(|&c| c != b' ').map_or(start, |i| i + 1);
    let a = &args[start..end.max(start)];
    a.strip_prefix(b"\"").and_then(|a| a.strip_suffix(b"\"")).unwrap_or(a)
}

/// The program's arguments split at blanks (quoted parts kept together).
pub fn words(s: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut quoted = false;
    let mut any = false;
    for &c in s {
        match c {
            b'"' => {
                quoted = !quoted;
                any = true;
            }
            b' ' | b'\t' if !quoted => {
                if any {
                    out.push(core::mem::take(&mut cur));
                    any = false;
                }
            }
            _ => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// A number in decimal.
pub fn num(n: i64) -> Vec<u8> {
    alloc::format!("{n}").into_bytes()
}

/// A number with thousands separators (`1,234,567`).
pub fn num_sep(n: u64, sep: u8) -> Vec<u8> {
    let d = alloc::format!("{n}").into_bytes();
    let mut out = Vec::new();
    for (i, &c) in d.iter().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// Parse a decimal number (leading blanks allowed).
pub fn parse_num(b: &[u8]) -> Option<i64> {
    let s = b.iter().skip_while(|&&c| c == b' ').copied().collect::<Vec<u8>>();
    let (neg, s) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        _ => (false, &s[..]),
    };
    let s: Vec<u8> = s.iter().copied().take_while(|c| c.is_ascii_digit()).collect();
    if s.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for c in s {
        v = v.checked_mul(10)?.checked_add((c - b'0') as i64)?;
    }
    Some(if neg { -v } else { v })
}

/// Run the message loop until WM_QUIT.
pub fn message_loop(hab: u32) {
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
}

/// A file's whole content.
pub fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut h, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 32768];
    loop {
        let mut got = 0;
        if unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 {
            unsafe { os2::DosClose(h) };
            return None;
        }
        if got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got as usize]);
    }
    unsafe { os2::DosClose(h) };
    Some(out)
}

/// Write a file (replacing it); the OS/2 error code on failure.
pub fn write_file(path: &[u8], data: &[u8]) -> Result<(), u32> {
    let p = z(path);
    let (mut h, mut action) = (0u32, 0u32);
    // OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
    // OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE.
    let rc = unsafe { os2::DosOpen(p.as_ptr(), &mut h, &mut action, 0, 0, 0x12, 0x21, core::ptr::null_mut()) };
    if rc != 0 {
        return Err(rc);
    }
    let mut done = 0;
    let rc = unsafe { os2::DosWrite(h, data.as_ptr(), data.len() as u32, &mut done) };
    unsafe { os2::DosClose(h) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

/// The last component of a path.
pub fn file_name(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&c| c == b'\\' || c == b'/' || c == b':') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// A path's extension, upper case, without the dot.
pub fn extension(path: &[u8]) -> Vec<u8> {
    let n = file_name(path);
    match n.iter().rposition(|&c| c == b'.') {
        Some(i) => n[i + 1..].to_ascii_uppercase(),
        None => Vec::new(),
    }
}

/// Read a little-endian u16/u32.
pub fn rd16(b: &[u8], o: usize) -> u16 {
    if o + 2 > b.len() {
        return 0;
    }
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub fn rd32(b: &[u8], o: usize) -> u32 {
    if o + 4 > b.len() {
        return 0;
    }
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn wr16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn wr32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// A simple pseudo-random generator (xorshift), seeded from the clock.
pub struct Rng(u32);

impl Rng {
    pub fn new() -> Rng {
        let mut t: u64 = 0;
        unsafe { os2::DosTmrQueryTime(&mut t) };
        let mut ms = [0u32; 1];
        unsafe { os2::DosQuerySysInfo(14, 14, ms.as_mut_ptr(), 4) };
        Rng((t as u32 ^ (t >> 32) as u32 ^ ms[0].rotate_left(13)) | 1)
    }
    pub fn seeded(seed: u32) -> Rng {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// 0..n
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            v.swap(i, j);
        }
    }
}

impl Default for Rng {
    fn default() -> Self {
        Rng::new()
    }
}

/// Create a standard frame window with a client of `class` (registered
/// here with `proc_`), the given frame flags and title; returns (frame,
/// client).
pub fn std_window(hab: u32, class: &[u8], proc_: WndProc, class_style: u32, words: u32, fcf: u32, title: &[u8], id: u32) -> (u32, u32) {
    let c = z(class);
    let t = z(title);
    unsafe { WinRegisterClass(hab, c.as_ptr(), proc_, class_style, words) };
    let mut fl = fcf;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fl, c.as_ptr(), t.as_ptr(), WS_CLIPCHILDREN, 0, id, &mut client) };
    // The program's icon (RT_POINTER 1, put there by xtask) on the frame.
    let icon = unsafe { WinLoadPointer(HWND_DESKTOP, 0, 1) };
    if icon != 0 {
        send(frame, WM_SETICON, icon, 0);
    }
    (frame, client)
}

/// Give a frame a menu bar built in memory; returns the menu.
pub fn set_menu(frame: u32, items: &[Menu]) -> u32 {
    let tpl = menu_template(items);
    let m = unsafe { WinCreateMenu(frame, tpl.as_ptr()) };
    send(frame, WM_UPDATEFRAME, FCF_MENU, 0);
    m
}

/// Give a frame an accelerator table: (fs, key, cmd).
pub fn set_accels(hab: u32, frame: u32, accels: &[(u16, u16, u16)]) {
    let mut acc = Vec::new();
    acc.extend_from_slice(&(accels.len() as u16).to_le_bytes());
    acc.extend_from_slice(&850u16.to_le_bytes());
    for &(fs, key, cmd) in accels {
        acc.extend_from_slice(&fs.to_le_bytes());
        acc.extend_from_slice(&key.to_le_bytes());
        acc.extend_from_slice(&cmd.to_le_bytes());
    }
    unsafe {
        let haccel = WinCreateAccelTable(hab, acc.as_ptr());
        WinSetAccelTable(hab, haccel, frame);
    }
}

/// Show a frame where it was last time (OS2.INI `app`/`key`, an SWP),
/// else at the size given (centred) or where the shell puts programs.
pub fn show_at_saved(hab: u32, frame: u32, app: &[u8], key: &[u8], size: Option<(i32, i32)>) {
    let mut swp = Swp::default();
    let saved = ini::read_struct(app, key, &mut swp) && swp.cx > 0 && swp.cy > 0;
    let (sw, sh) = screen();
    if !saved {
        match size {
            Some((cx, cy)) => {
                swp.cx = cx.min(sw);
                swp.cy = cy.min(sh);
                swp.x = (sw - swp.cx) / 2;
                swp.y = (sh - swp.cy) / 2;
            }
            None => unsafe {
                WinQueryTaskSizePos(hab, 0, &mut swp);
            },
        }
    }
    // Keep it on the screen.
    swp.x = swp.x.clamp(-swp.cx / 2, (sw - 32).max(0));
    swp.y = swp.y.clamp(0, (sh - 32).max(0));
    unsafe { WinSetWindowPos(frame, HWND_TOP, swp.x, swp.y, swp.cx, swp.cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE) };
}

/// Save a frame's normal position (not when minimized or maximized).
pub fn save_position(frame: u32, app: &[u8], key: &[u8]) {
    let swp = window_pos(frame);
    if swp.fl & (SWP_MINIMIZE | SWP_MAXIMIZE) != 0 || swp.cx <= 0 || swp.cy <= 0 {
        return;
    }
    ini::write_struct(app, key, &swp);
}

/// The text of a WinQuerySysValue-sized font, as a PP_FONTNAMESIZE
/// presentation parameter block.
pub fn font_pres(font: &[u8]) -> Vec<u8> {
    // PRESPARAMS: cb, then PARAM (id, cb, value).
    let mut v = Vec::new();
    let len = font.len() as u32 + 1;
    v.extend_from_slice(&(8 + len).to_le_bytes());
    v.extend_from_slice(&PP_FONTNAMESIZE.to_le_bytes());
    v.extend_from_slice(&len.to_le_bytes());
    v.extend_from_slice(font);
    v.push(0);
    v
}

/// Set a window's font (`"10.Helv"`).
pub fn set_font(h: u32, font: &[u8]) {
    let f = z(font);
    unsafe { WinSetPresParam(h, PP_FONTNAMESIZE, f.len() as u32, f.as_ptr()) };
}

/// Make a program object in a folder (through the Workplace Shell); true
/// when it was made.
pub fn create_object(class: &[u8], title: &[u8], setup: &[u8], location: &[u8], fl: u32) -> bool {
    let (c, t, s, l) = (z(class), z(title), z(setup), z(location));
    unsafe { WinCreateObject(c.as_ptr(), t.as_ptr(), s.as_ptr(), l.as_ptr(), fl) != 0 }
}

/// Run a program, wait for it and return its exit code (or the error of
/// starting it).
pub fn run_program(path: &[u8], args: &[u8]) -> Result<u32, u32> {
    let mut a = Vec::new();
    a.extend_from_slice(file_name(path));
    a.push(0);
    a.push(b' ');
    a.extend_from_slice(args);
    a.extend_from_slice(&[0, 0]);
    let p = z(path);
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), 64, 0, a.as_ptr(), core::ptr::null(), &mut res, p.as_ptr()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(res.code_result)
}
