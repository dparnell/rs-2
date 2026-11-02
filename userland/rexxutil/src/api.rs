//! The OS/2 and REXX entry points REXXUTIL imports (beyond the os2
//! crate's), and the 16-bit VIO/KBD calls.

#![allow(clippy::missing_safety_doc)]

use os2::vio::{call16, ptr_words};
use os2::APIRET;

use crate::rx::{RxString, ShvBlock};

unsafe extern "C" {
    #[link_name = "REXX.2"]
    pub fn RexxVariablePool(p: *mut ShvBlock) -> u32;
    #[link_name = "REXXAPI.17"]
    pub fn RexxRegisterFunctionDll(name: *const u8, dll: *const u8, entry: *const u8) -> u32;
    #[link_name = "REXXAPI.19"]
    pub fn RexxDeregisterFunction(name: *const u8) -> u32;
    #[link_name = "REXXAPI.22"]
    pub fn RexxAddMacro(name: *const u8, file: *const u8, pos: u32) -> u32;
    #[link_name = "REXXAPI.23"]
    pub fn RexxDropMacro(name: *const u8) -> u32;
    #[link_name = "REXXAPI.24"]
    pub fn RexxClearMacroSpace() -> u32;
    #[link_name = "REXXAPI.25"]
    pub fn RexxLoadMacroSpace(n: u32, names: *const *const u8, file: *const u8) -> u32;
    #[link_name = "REXXAPI.26"]
    pub fn RexxQueryMacro(name: *const u8, pos: *mut u16) -> u32;
    #[link_name = "REXXAPI.27"]
    pub fn RexxReorderMacro(name: *const u8, pos: u32) -> u32;
    #[link_name = "REXXAPI.28"]
    pub fn RexxSaveMacroSpace(n: u32, names: *const *const u8, file: *const u8) -> u32;

    #[link_name = "DOSCALLS.209"]
    pub fn DosSetMaxFH(n: u32) -> APIRET;
    #[link_name = "DOSCALLS.228"]
    pub fn DosSearchPath(flags: u32, path: *const u8, file: *const u8, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.236"]
    pub fn DosSetPriority(scope: u32, class: u32, delta: i32, id: u32) -> APIRET;
    #[link_name = "DOSCALLS.325"]
    pub fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.332"]
    pub fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.382"]
    pub fn DosSetRelMaxFH(delta: *mut i32, cur: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.415"]
    pub fn DosShutdown(reserved: u32) -> APIRET;
    #[link_name = "DOSCALLS.586"]
    pub fn DosQueryProcType(hmod: u32, ord: u32, name: *const u8, ptype: *mut u32) -> APIRET;
}

/// A REXX external function (SysLoadLibrary calls a package's loader).
pub type RexxFunction = unsafe extern "C" fn(*const u8, u32, *const RxString, *const u8, *mut RxString) -> u32;

// ---- 16-bit VIO and KBD calls (through the os2 crate's thunk) ----

unsafe extern "C" {
    #[link_name = "VIOCALLS.9"]
    fn VIO16GETCURPOS();
    #[link_name = "VIOCALLS.15"]
    fn VIO16SETCURPOS();
    #[link_name = "VIOCALLS.19"]
    fn VIO16WRTTTY();
    #[link_name = "VIOCALLS.27"]
    fn VIO16GETCURTYPE();
    #[link_name = "VIOCALLS.30"]
    fn VIO16READCHARSTR();
    #[link_name = "VIOCALLS.32"]
    fn VIO16SETCURTYPE();
    #[link_name = "VIOCALLS.47"]
    fn VIO16SCROLLDN();
    #[link_name = "KBDCALLS.4"]
    fn KBD16CHARIN();
}

/// A small buffer that never crosses a 64 KB tile (16-bit pointers).
#[repr(C, align(64))]
pub struct Tile<T>(pub T);

fn entry(f: unsafe extern "C" fn()) -> usize {
    f as *const () as usize
}

pub fn vio_get_cur_pos() -> (u16, u16) {
    let mut v = Tile([0u16; 2]);
    let r = ptr_words(&mut v.0[0] as *mut u16);
    let c = ptr_words(&mut v.0[1] as *mut u16);
    unsafe { call16(entry(VIO16GETCURPOS), &[r[0], r[1], c[0], c[1], 0]) };
    let v = unsafe { core::ptr::read_volatile(&v.0) };
    (v[0], v[1])
}

pub fn vio_set_cur_pos(row: u16, col: u16) -> u16 {
    unsafe { call16(entry(VIO16SETCURPOS), &[row, col, 0]) }
}

/// VIOCURSORINFO: start line, end line, width, attribute.
pub fn vio_get_cur_type() -> [u16; 4] {
    let mut v = Tile([0u16; 4]);
    let p = ptr_words(&mut v.0 as *mut [u16; 4]);
    unsafe { call16(entry(VIO16GETCURTYPE), &[p[0], p[1], 0]) };
    unsafe { core::ptr::read_volatile(&v.0) }
}

pub fn vio_set_cur_type(ci: [u16; 4]) -> u16 {
    let v = Tile(ci);
    let p = ptr_words(&v.0 as *const [u16; 4]);
    unsafe { call16(entry(VIO16SETCURTYPE), &[p[0], p[1], 0]) }
}

/// Scroll the whole screen down by its height with blanks of attribute 7.
pub fn vio_clear() {
    let cell = Tile([0x20u8, 0x07]);
    let p = ptr_words(&cell.0 as *const [u8; 2]);
    unsafe { call16(entry(VIO16SCROLLDN), &[0, 0, 0xffff, 0xffff, 0xffff, p[0], p[1], 0]) };
}

pub fn vio_wrt_tty(b: &[u8]) {
    let mut t = Tile([0u8; 32]);
    let n = b.len().min(32);
    t.0[..n].copy_from_slice(&b[..n]);
    let p = ptr_words(&t.0 as *const [u8; 32]);
    unsafe { call16(entry(VIO16WRTTTY), &[p[0], p[1], n as u16, 0]) };
}

/// VIOMODEINFO's rows and columns.
pub fn vio_get_mode() -> (u16, u16) {
    match os2::vio::vio_get_mode() {
        Ok(m) => (m.row, m.col),
        Err(_) => (0, 0),
    }
}

/// VioReadCharStr into `buf` (which must not cross a 64 KB boundary):
/// the number of characters read.
pub fn vio_read_char_str(buf: *mut u8, len: u16, row: u16, col: u16) -> u16 {
    let mut n = Tile([len]);
    let b = ptr_words(buf);
    let pn = ptr_words(&mut n.0 as *mut [u16; 1]);
    unsafe { call16(entry(VIO16READCHARSTR), &[b[0], b[1], pn[0], pn[1], row, col, 0]) };
    let v: [u16; 1] = unsafe { core::ptr::read_volatile(&n.0) };
    v[0]
}

/// KbdCharIn with wait: (rc, character, scan code, status).
pub fn kbd_char_in() -> (u16, u8, u8, u8) {
    let mut k = Tile([0u8; 10]);
    let p = ptr_words(&mut k.0 as *mut [u8; 10]);
    let rc = unsafe { call16(entry(KBD16CHARIN), &[p[0], p[1], 0, 0]) };
    let k = unsafe { core::ptr::read_volatile(&k.0) };
    (rc, k[0], k[1], k[2])
}
