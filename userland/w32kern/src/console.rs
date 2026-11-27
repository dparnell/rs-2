//! The console on the session's VIO and KBD: Windows' screen buffer calls
//! address the session's screen (one buffer), its colours are VIO's
//! attributes (the same bits), keys come from KbdCharIn.
//!
//! Text written while a colour other than the session's is set goes out
//! through VioWrtCharStrAtt, so that it has that colour; otherwise through
//! DosWrite like any text (and the session's ANSI support).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::vio::{call16, far16, ptr_words};

use crate::handles;
use crate::text::{from_wide, to_wide};
use crate::{err, fail, FALSE, TRUE};

unsafe extern "C" {
    #[link_name = "VIOCALLS.9"]
    fn VIOGETCURPOS();
    #[link_name = "VIOCALLS.15"]
    fn VIOSETCURPOS();
    #[link_name = "VIOCALLS.19"]
    fn VIOWRTTTY();
    #[link_name = "VIOCALLS.48"]
    fn VIOWRTCHARSTRATT();
    #[link_name = "VIOCALLS.13"]
    fn VIOWRTCHARSTR();
    #[link_name = "VIOCALLS.53"]
    fn VIOWRTNCHAR();
    #[link_name = "VIOCALLS.26"]
    fn VIOWRTNATTR();
    #[link_name = "VIOCALLS.24"]
    fn VIOREADCELLSTR();
    #[link_name = "VIOCALLS.10"]
    fn VIOWRTCELLSTR();
    #[link_name = "VIOCALLS.7"]
    fn VIOSCROLLUP();
    #[link_name = "VIOCALLS.47"]
    fn VIOSCROLLDN();
    #[link_name = "VIOCALLS.27"]
    fn VIOGETCURTYPE();
    #[link_name = "VIOCALLS.32"]
    fn VIOSETCURTYPE();
    #[link_name = "KBDCALLS.4"]
    fn KBDCHARIN();
    #[link_name = "KBDCALLS.22"]
    fn KBDPEEK();
    #[link_name = "KBDCALLS.13"]
    fn KBDFLUSHBUFFER();
}

fn entry(f: unsafe extern "C" fn()) -> usize {
    f as *const () as usize
}

/// A buffer for 16-bit calls that never crosses a 64 KB tile.
#[repr(C, align(256))]
struct Buf<const N: usize>([u8; N]);

const DEFAULT_ATTR: u32 = 0x07;
static ATTR: AtomicU32 = AtomicU32::new(DEFAULT_ATTR);
static MODE_IN: AtomicU32 = AtomicU32::new(0x7);
static MODE_OUT: AtomicU32 = AtomicU32::new(0x3);

pub fn init() {}

/// Is OS/2 handle `hf` the console (a character device)?
pub fn is_console(hf: u32) -> bool {
    crate::file::handle_type(hf) == crate::file::FILE_TYPE_CHAR
}

fn size() -> (u16, u16) {
    match os2::vio::vio_get_mode() {
        Ok(m) => (m.col, m.row),
        Err(_) => (80, 25),
    }
}

fn cursor() -> (u16, u16) {
    let mut b = Buf([0u8; 8]);
    let row = ptr_words(b.0.as_mut_ptr());
    let col = ptr_words(unsafe { b.0.as_mut_ptr().add(2) });
    unsafe { call16(entry(VIOGETCURPOS), &[row[0], row[1], col[0], col[1], 0]) };
    let b = unsafe { core::ptr::read_volatile(&b.0) };
    (u16::from_le_bytes([b[2], b[3]]), u16::from_le_bytes([b[0], b[1]]))
}

fn set_cursor(x: u16, y: u16) -> bool {
    unsafe { call16(entry(VIOSETCURPOS), &[y, x, 0]) == 0 }
}

/// Write to an OS/2 handle: a console in a colour of its own through
/// VIO, everything else with DosWrite.
pub unsafe fn write(hf: u32, buf: *const u8, n: u32, done: &mut u32) -> u32 {
    let attr = ATTR.load(Ordering::Relaxed);
    if attr == DEFAULT_ATTR || !is_console(hf) {
        return unsafe { os2::DosWrite(hf, buf, n, done) };
    }
    let s = unsafe { core::slice::from_raw_parts(buf, n as usize) };
    write_attr(s, attr as u8);
    *done = n;
    0
}

/// Text at the cursor in attribute `attr`: runs of printable characters
/// with VioWrtCharStrAtt, control characters through VioWrtTTY.
fn write_attr(s: &[u8], attr: u8) {
    let mut i = 0;
    while i < s.len() {
        let j = s[i..].iter().position(|&c| c < 0x20).map_or(s.len(), |k| i + k);
        let mut rest = &s[i..j];
        while !rest.is_empty() {
            let (cols, rows) = size();
            let (x, y) = cursor();
            let room = (cols - x.min(cols - 1)) as usize;
            let k = rest.len().min(room);
            let mut b = Buf([0u8; 256]);
            let k = k.min(256);
            b.0[..k].copy_from_slice(&rest[..k]);
            let mut a = Buf([attr; 4]);
            let pb = ptr_words(b.0.as_ptr());
            let pa = ptr_words(a.0.as_mut_ptr());
            unsafe { call16(entry(VIOWRTCHARSTRATT), &[pb[0], pb[1], k as u16, y, x, pa[0], pa[1], 0]) };
            let nx = x + k as u16;
            if nx >= cols {
                if y + 1 >= rows {
                    scroll_up(1);
                    set_cursor(0, rows - 1);
                } else {
                    set_cursor(0, y + 1);
                }
            } else {
                set_cursor(nx, y);
            }
            rest = &rest[k..];
        }
        if j < s.len() {
            let mut b = Buf([s[j]; 4]);
            let pb = ptr_words(b.0.as_mut_ptr());
            unsafe { call16(entry(VIOWRTTTY), &[pb[0], pb[1], 1, 0]) };
        }
        i = j + 1;
    }
}

fn scroll_up(lines: u16) {
    let (cols, rows) = size();
    let mut cell = Buf([b' ', DEFAULT_ATTR as u8, 0, 0]);
    let pc = ptr_words(cell.0.as_mut_ptr());
    unsafe { call16(entry(VIOSCROLLUP), &[0, 0, rows - 1, cols - 1, lines, pc[0], pc[1], 0]) };
}

/// ReadFile on the console's input: a line (DosRead's line editing).
pub unsafe fn read_line(buf: *mut u8, n: u32, read: *mut u32) -> i32 {
    let mut got = 0u32;
    let rc = unsafe { os2::DosRead(0, buf, n, &mut got) };
    if !read.is_null() {
        unsafe { *read = got };
    }
    if rc != 0 { fail(crate::win_error(rc), FALSE) } else { TRUE }
}

fn console_handle(h: u32) -> Option<u32> {
    handles::file(h).filter(|&hf| is_console(hf))
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WriteConsoleA(h: u32, buf: *const u8, n: u32, written: *mut u32, _r: u32) -> i32 {
    let Some(hf) = console_handle(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let mut done = 0;
    unsafe { write(hf, buf, n, &mut done) };
    if !written.is_null() {
        unsafe { *written = done };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WriteConsoleW(h: u32, buf: *const u16, n: u32, written: *mut u32, _r: u32) -> i32 {
    let Some(hf) = console_handle(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    let b = from_wide(unsafe { core::slice::from_raw_parts(buf, n as usize) });
    let mut done = 0;
    unsafe { write(hf, b.as_ptr(), b.len() as u32, &mut done) };
    if !written.is_null() {
        unsafe { *written = n };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ReadConsoleA(h: u32, buf: *mut u8, n: u32, read: *mut u32, _c: u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    unsafe { read_line(buf, n, read) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ReadConsoleW(h: u32, buf: *mut u16, n: u32, read: *mut u32, c: u32) -> i32 {
    let mut a: Vec<u8> = alloc::vec![0u8; n as usize];
    let mut got = 0u32;
    if unsafe { ReadConsoleA(h, a.as_mut_ptr(), n, &mut got, c) } == FALSE {
        return FALSE;
    }
    let w = to_wide(&a[..got as usize]);
    unsafe { core::ptr::copy_nonoverlapping(w.as_ptr(), buf, w.len()) };
    if !read.is_null() {
        unsafe { *read = w.len() as u32 };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetConsoleMode(h: u32, mode: *mut u32) -> i32 {
    let Some(hf) = console_handle(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    unsafe { *mode = if hf == 0 { MODE_IN.load(Ordering::Relaxed) } else { MODE_OUT.load(Ordering::Relaxed) } };
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleMode(h: u32, mode: u32) -> i32 {
    let Some(hf) = console_handle(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    if hf == 0 { MODE_IN.store(mode, Ordering::Relaxed) } else { MODE_OUT.store(mode, Ordering::Relaxed) }
    TRUE
}

/// CONSOLE_SCREEN_BUFFER_INFO: dwSize, dwCursorPosition, wAttributes,
/// srWindow, dwMaximumWindowSize.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetConsoleScreenBufferInfo(h: u32, info: *mut u16) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let (cols, rows) = size();
    let (x, y) = cursor();
    let v = [cols, rows, x, y, ATTR.load(Ordering::Relaxed) as u16, 0, 0, cols - 1, rows - 1, cols, rows];
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), info, v.len()) };
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetLargestConsoleWindowSize(_h: u32) -> u32 {
    let (c, r) = size();
    c as u32 | (r as u32) << 16
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleTextAttribute(h: u32, attr: u16) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    ATTR.store(attr as u32 & 0xff, Ordering::Relaxed);
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleCursorPosition(h: u32, pos: u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let (cols, rows) = size();
    let (x, y) = (pos as u16, (pos >> 16) as u16);
    if x >= cols || y >= rows {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    set_cursor(x, y) as i32
}

/// CONSOLE_CURSOR_INFO: dwSize (percent), bVisible.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetConsoleCursorInfo(h: u32, info: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let mut b = Buf([0u8; 8]);
    let p = ptr_words(b.0.as_mut_ptr());
    unsafe { call16(entry(VIOGETCURTYPE), &[p[0], p[1], 0]) };
    let b = unsafe { core::ptr::read_volatile(&b.0) };
    let attr = u16::from_le_bytes([b[6], b[7]]);
    unsafe {
        *info = 25;
        *info.add(1) = (attr != 0xffff) as u32;
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetConsoleCursorInfo(h: u32, info: *const u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let mut b = Buf([0u8; 8]);
    let p = ptr_words(b.0.as_mut_ptr());
    unsafe { call16(entry(VIOGETCURTYPE), &[p[0], p[1], 0]) };
    let mut v = unsafe { core::ptr::read_volatile(&b.0) };
    let visible = unsafe { *info.add(1) } != 0;
    v[6..8].copy_from_slice(&(if visible { 0u16 } else { 0xffff }).to_le_bytes());
    unsafe { core::ptr::write_volatile(&mut b.0, v) };
    unsafe { call16(entry(VIOSETCURTYPE), &[p[0], p[1], 0]) };
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FillConsoleOutputCharacterA(h: u32, c: u8, n: u32, pos: u32, written: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let (cols, rows) = size();
    let (x, y) = (pos as u16, (pos >> 16) as u16);
    let max = (cols as u32 * rows as u32).saturating_sub(y as u32 * cols as u32 + x as u32);
    let n = n.min(max);
    let mut b = Buf([c; 4]);
    let pb = ptr_words(b.0.as_mut_ptr());
    unsafe { call16(entry(VIOWRTNCHAR), &[pb[0], pb[1], n as u16, y, x, 0]) };
    if !written.is_null() {
        unsafe { *written = n };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FillConsoleOutputCharacterW(h: u32, c: u16, n: u32, pos: u32, written: *mut u32) -> i32 {
    let b = crate::text::wide_to_byte(0, c).unwrap_or(b'?');
    unsafe { FillConsoleOutputCharacterA(h, b, n, pos, written) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FillConsoleOutputAttribute(h: u32, attr: u16, n: u32, pos: u32, written: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let (cols, rows) = size();
    let (x, y) = (pos as u16, (pos >> 16) as u16);
    let max = (cols as u32 * rows as u32).saturating_sub(y as u32 * cols as u32 + x as u32);
    let n = n.min(max);
    let mut b = Buf([attr as u8; 4]);
    let pb = ptr_words(b.0.as_mut_ptr());
    unsafe { call16(entry(VIOWRTNATTR), &[pb[0], pb[1], n as u16, y, x, 0]) };
    if !written.is_null() {
        unsafe { *written = n };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WriteConsoleOutputCharacterA(h: u32, s: *const u8, n: u32, pos: u32, written: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let (x, y) = (pos as u16, (pos >> 16) as u16);
    let src = unsafe { core::slice::from_raw_parts(s, n as usize) };
    let mut done = 0;
    for chunk in src.chunks(256) {
        let mut b = Buf([0u8; 256]);
        b.0[..chunk.len()].copy_from_slice(chunk);
        let pb = ptr_words(b.0.as_mut_ptr());
        let (cols, _) = size();
        let at = y as u32 * cols as u32 + x as u32 + done as u32;
        unsafe { call16(entry(VIOWRTCHARSTR), &[pb[0], pb[1], chunk.len() as u16, (at / cols as u32) as u16, (at % cols as u32) as u16, 0]) };
        done += chunk.len();
    }
    if !written.is_null() {
        unsafe { *written = n };
    }
    TRUE
}

/// CHAR_INFO is (char u16, attributes u16); a SMALL_RECT region of a
/// buffer of `bsize` at `bpos`.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WriteConsoleOutputA(h: u32, buf: *const u16, bsize: u32, bpos: u32, region: *mut i16) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let (bw, _bh) = (bsize as u16 as usize, (bsize >> 16) as u16 as usize);
    let (bx, by) = (bpos as u16 as usize, (bpos >> 16) as u16 as usize);
    let r = unsafe { core::slice::from_raw_parts_mut(region, 4) };
    let (cols, rows) = size();
    let right = (r[2] as i32).min(cols as i32 - 1);
    let bottom = (r[3] as i32).min(rows as i32 - 1);
    for (dy, y) in (r[1] as i32..=bottom).enumerate() {
        let mut cells = Buf([0u8; 512]);
        let mut n = 0;
        for (dx, _x) in (r[0] as i32..=right).enumerate() {
            let ci = unsafe { buf.add(((by + dy) * bw + bx + dx) * 2) };
            cells.0[n * 2] = unsafe { *ci } as u8;
            cells.0[n * 2 + 1] = unsafe { *ci.add(1) } as u8;
            n += 1;
            if n == 256 {
                break;
            }
        }
        let pc = ptr_words(cells.0.as_mut_ptr());
        unsafe { call16(entry(VIOWRTCELLSTR), &[pc[0], pc[1], (n * 2) as u16, y as u16, r[0] as u16, 0]) };
    }
    r[2] = right as i16;
    r[3] = bottom as i16;
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn WriteConsoleOutputW(h: u32, buf: *const u16, bsize: u32, bpos: u32, region: *mut i16) -> i32 {
    // The characters are converted in a copy.
    let (bw, bh) = (bsize as u16 as usize, (bsize >> 16) as u16 as usize);
    let mut copy: Vec<u16> = unsafe { core::slice::from_raw_parts(buf, bw * bh * 2) }.to_vec();
    for i in 0..bw * bh {
        copy[i * 2] = crate::text::wide_to_byte(0, copy[i * 2]).unwrap_or(b'?') as u16;
    }
    unsafe { WriteConsoleOutputA(h, copy.as_ptr(), bsize, bpos, region) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ReadConsoleOutputA(h: u32, buf: *mut u16, bsize: u32, bpos: u32, region: *mut i16) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let bw = bsize as u16 as usize;
    let (bx, by) = (bpos as u16 as usize, (bpos >> 16) as u16 as usize);
    let r = unsafe { core::slice::from_raw_parts_mut(region, 4) };
    for (dy, y) in (r[1]..=r[3]).enumerate() {
        let n = ((r[2] - r[0] + 1) as usize).min(256);
        let mut cells = Buf([0u8; 512]);
        let mut cb = Buf([0u8; 4]);
        cb.0[..2].copy_from_slice(&((n * 2) as u16).to_le_bytes());
        let pc = ptr_words(cells.0.as_mut_ptr());
        let pn = ptr_words(cb.0.as_mut_ptr());
        unsafe { call16(entry(VIOREADCELLSTR), &[pc[0], pc[1], pn[0], pn[1], y as u16, r[0] as u16, 0]) };
        let cells = unsafe { core::ptr::read_volatile(&cells.0) };
        for dx in 0..n {
            let ci = unsafe { buf.add(((by + dy) * bw + bx + dx) * 2) };
            unsafe {
                *ci = cells[dx * 2] as u16;
                *ci.add(1) = cells[dx * 2 + 1] as u16;
            }
        }
    }
    TRUE
}

/// ScrollConsoleScreenBuffer: the rectangle moves to `dest`; what it
/// leaves is filled with `fill` (CHAR_INFO). Moves up or down only.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ScrollConsoleScreenBufferA(h: u32, rect: *const i16, _clip: *const i16, dest: u32, fill: *const u16) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    let r = unsafe { core::slice::from_raw_parts(rect, 4) };
    let dy = (dest >> 16) as i16 as i32 - r[1] as i32;
    let mut cell = Buf([unsafe { *fill } as u8, unsafe { *fill.add(1) } as u8, 0, 0]);
    let pc = ptr_words(cell.0.as_mut_ptr());
    let (top, left, bottom, right) = (r[1] as u16, r[0] as u16, r[3] as u16, r[2] as u16);
    if dy < 0 {
        unsafe { call16(entry(VIOSCROLLUP), &[top.saturating_sub((-dy) as u16), left, bottom, right, (-dy) as u16, pc[0], pc[1], 0]) };
    } else if dy > 0 {
        unsafe { call16(entry(VIOSCROLLDN), &[top, left, bottom + dy as u16, right, dy as u16, pc[0], pc[1], 0]) };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleTitleA(_t: *const u8) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleTitleW(_t: *const u16) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetConsoleTitleA(buf: *mut u8, size: u32) -> u32 {
    unsafe { crate::put_str(b"Win32", buf, size) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleCtrlHandler(_f: u32, _add: i32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AllocConsole() -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FreeConsole() -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleActiveScreenBuffer(_h: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetConsoleScreenBufferSize(_h: u32, _size: u32) -> i32 {
    TRUE
}

// ---- Input ----

/// KBDKEYINFO: chChar, chScan, fbStatus, bNlsShift, fsState, time.
fn key_in(wait: bool) -> Option<[u8; 10]> {
    let mut b = Buf([0u8; 16]);
    let p = ptr_words(b.0.as_mut_ptr());
    let rc = if wait {
        unsafe { call16(entry(KBDCHARIN), &[p[0], p[1], 0, 0]) }
    } else {
        unsafe { call16(entry(KBDPEEK), &[p[0], p[1], 0]) }
    };
    let b = unsafe { core::ptr::read_volatile(&b.0) };
    // fbStatus bit 6: a character is there.
    (rc == 0 && b[2] & 0x40 != 0).then(|| b[..10].try_into().unwrap())
}

/// The virtual key code of a key.
fn vk(ch: u8, scan: u8) -> u16 {
    if ch != 0 && ch != 0xe0 {
        return match ch {
            b'a'..=b'z' => (ch - 32) as u16,
            b'A'..=b'Z' | b'0'..=b'9' | b' ' => ch as u16,
            8 => 0x08,
            9 => 0x09,
            13 | 10 => 0x0d,
            27 => 0x1b,
            1..=26 => (ch + b'A' - 1) as u16,
            _ => 0,
        };
    }
    match scan {
        0x48 => 0x26,
        0x50 => 0x28,
        0x4b => 0x25,
        0x4d => 0x27,
        0x47 => 0x24,
        0x4f => 0x23,
        0x49 => 0x21,
        0x51 => 0x22,
        0x52 => 0x2d,
        0x53 => 0x2e,
        0x3b..=0x44 => 0x70 + (scan - 0x3b) as u16,
        0x85 => 0x7a,
        0x86 => 0x7b,
        _ => 0,
    }
}

/// An INPUT_RECORD (KEY_EVENT) of a key.
unsafe fn key_record(k: &[u8; 10], rec: *mut u8) {
    let (ch, scan) = (k[0], k[1]);
    let state = u16::from_le_bytes([k[4], k[5]]);
    let mut ctrl = 0u32;
    if state & 3 != 0 {
        ctrl |= 0x10; // SHIFT_PRESSED
    }
    if state & 4 != 0 {
        ctrl |= 0x08; // LEFT_CTRL_PRESSED
    }
    if state & 8 != 0 {
        ctrl |= 0x02; // LEFT_ALT_PRESSED
    }
    if ch == 0 || ch == 0xe0 {
        ctrl |= 0x100; // ENHANCED_KEY
    }
    unsafe {
        core::ptr::write_bytes(rec, 0, 20);
        (rec as *mut u16).write(1); // KEY_EVENT
        (rec.add(4) as *mut u32).write(1); // bKeyDown
        (rec.add(8) as *mut u16).write(1); // wRepeatCount
        (rec.add(10) as *mut u16).write(vk(ch, scan));
        (rec.add(12) as *mut u16).write(scan as u16);
        (rec.add(14) as *mut u16).write(if ch == 0xe0 { 0 } else { ch as u16 });
        (rec.add(16) as *mut u32).write(ctrl);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ReadConsoleInputA(h: u32, rec: *mut u8, n: u32, read: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    if n == 0 {
        unsafe { *read = 0 };
        return TRUE;
    }
    let Some(k) = key_in(true) else { return fail(err::INVALID_HANDLE, FALSE) };
    unsafe {
        key_record(&k, rec);
        *read = 1;
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn ReadConsoleInputW(h: u32, rec: *mut u8, n: u32, read: *mut u32) -> i32 {
    let r = unsafe { ReadConsoleInputA(h, rec, n, read) };
    if r == TRUE && unsafe { *read } == 1 {
        let c = unsafe { *(rec.add(14) as *const u16) } as u8;
        unsafe { (rec.add(14) as *mut u16).write(crate::text::byte_to_wide(0, c)) };
    }
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn PeekConsoleInputA(h: u32, rec: *mut u8, n: u32, read: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    match key_in(false) {
        Some(k) if n > 0 => unsafe {
            key_record(&k, rec);
            *read = 1;
        },
        _ => unsafe { *read = 0 },
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn PeekConsoleInputW(h: u32, rec: *mut u8, n: u32, read: *mut u32) -> i32 {
    unsafe { PeekConsoleInputA(h, rec, n, read) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetNumberOfConsoleInputEvents(h: u32, n: *mut u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    unsafe { *n = key_in(false).is_some() as u32 };
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FlushConsoleInputBuffer(h: u32) -> i32 {
    if console_handle(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    unsafe { call16(entry(KBDFLUSHBUFFER), &[0]) };
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetNumberOfConsoleMouseButtons(n: *mut u32) -> i32 {
    unsafe { *n = 2 };
    TRUE
}

#[allow(dead_code)]
fn unused() {
    let _ = far16::<u8>;
}
