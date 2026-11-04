//! PM entry points and constants: the ones rs2's own applets use
//! (userland/pmkit's sys.rs and consts.rs, plain declarations shared by
//! file), plus small helpers.

#![allow(dead_code, non_snake_case, non_upper_case_globals, clippy::all)]

#[path = "../../../userland/pmkit/src/sys.rs"]
mod sys;
#[path = "../../../userland/pmkit/src/consts.rs"]
mod consts;

pub use consts::*;
pub use sys::*;

pub const WC: u32 = 0xffff_0000;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

pub fn lo(v: u32) -> u32 {
    v & 0xffff
}

pub fn hi(v: u32) -> u32 {
    v >> 16
}

pub fn z(s: &str) -> Vec<u8> {
    let mut v = webcore::codepage::encode(s, 850);
    v.push(0);
    v
}

pub fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

pub fn post(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinPostMsg(h, m, a, b) }
}

pub fn set_text(h: u32, t: &str) {
    let s = z(t);
    unsafe { WinSetWindowText(h, s.as_ptr()) };
}

/// A window's text, as Unicode (code page 850 bytes).
pub fn text(h: u32) -> String {
    let mut buf = vec![0u8; 2048];
    let n = unsafe { WinQueryWindowText(h, buf.len() as i32, buf.as_mut_ptr()) } as usize;
    buf.truncate(n.min(2047));
    decode_850(&buf)
}

pub fn decode_850(b: &[u8]) -> String {
    const HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";
    b.iter().map(|&c| if c < 0x80 { c as char } else { HIGH.chars().nth((c - 0x80) as usize).unwrap_or('?') }).collect()
}

pub fn rect(h: u32) -> RectL {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(h, &mut r) };
    r
}

pub fn invalidate(h: u32) {
    unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
}

pub fn move_window(h: u32, x: i32, y: i32, cx: i32, cy: i32) {
    unsafe { WinSetWindowPos(h, 0, x, y, cx.max(0), cy.max(0), SWP_MOVE | SWP_SIZE) };
}

pub fn message(owner: u32, text: &str, title: &str) {
    let (t, c) = (z(text), z(title));
    unsafe { WinMessageBox(HWND_DESKTOP, owner, t.as_ptr(), c.as_ptr(), 0, MB_OK | MB_MOVEABLE) };
}
