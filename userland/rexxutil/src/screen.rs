//! The text screen and the keyboard (16-bit VIO and KBD calls).

use alloc::vec::Vec;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::rx::*;

/// SysCls(): clear the screen and home the cursor.
pub fn sys_cls(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    vio_clear();
    vio_set_cur_pos(0, 0);
    r.set(b"0");
    0
}

/// SysCurPos([row, col]): "row col" of the cursor before any move.
pub fn sys_cur_pos(a: &Args, r: &mut Ret) -> u32 {
    r.set(b"0");
    if a.n() != 0 && a.n() != 2 {
        return INVALID;
    }
    let (row, col) = vio_get_cur_pos();
    let mut v = dec(row as i64);
    v.push(b' ');
    v.extend(dec(col as i64));
    r.set(&v);
    if a.n() == 0 {
        return 0;
    }
    if !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let Some(row) = string2long(a.cstr(0)).filter(|&n| n >= 0) else {
        return INVALID;
    };
    let Some(col) = string2long(a.cstr(1)).filter(|&n| n >= 0) else {
        return INVALID;
    };
    vio_set_cur_pos(row as u16, col as u16);
    0
}

/// SysCurState('ON'|'OFF'): show or hide the cursor.
pub fn sys_cur_state(a: &Args, r: &mut Ret) -> u32 {
    r.set(b"0");
    if a.n() != 1 {
        return INVALID;
    }
    let attr = if eqi(a.cstr(0), b"ON") {
        0
    } else if eqi(a.cstr(0), b"OFF") {
        0xffff
    } else {
        return INVALID;
    };
    let mut ci = vio_get_cur_type();
    ci[3] = attr;
    vio_set_cur_type(ci);
    0
}

/// The scan code of an extended key, returned by the next SysGetKey.
static PENDING: AtomicU32 = AtomicU32::new(0);

/// SysGetKey(['ECHO'|'NOECHO']): one key; an extended key comes as its
/// prefix (0 or E0) and then its scan code on the next call.
pub fn sys_get_key(a: &Args, r: &mut Ret) -> u32 {
    if a.n() > 1 {
        return INVALID;
    }
    let mut echo = true;
    if a.n() == 1 {
        if eqi(a.cstr(0), b"NOECHO") {
            echo = false;
        } else if !eqi(a.cstr(0), b"ECHO") {
            return INVALID;
        }
    }
    let p = PENDING.swap(0, Ordering::Relaxed);
    let ch = if p != 0 {
        (p & 0xff) as u8
    } else {
        let (rc, ch, scan, status) = kbd_char_in();
        if rc != 0 {
            r.set(b"");
            return 0;
        }
        if status & 2 != 0 && (ch == 0 || ch == 0xe0) {
            PENDING.store(0x100 | scan as u32, Ordering::Relaxed);
        }
        ch
    };
    if echo {
        vio_wrt_tty(&[ch]);
    }
    r.set(&[ch]);
    0
}

/// SysTextScreenSize(): "rows cols".
pub fn sys_text_screen_size(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    let (row, col) = vio_get_mode();
    let mut v: Vec<u8> = dec(row as i64);
    v.push(b' ');
    v.extend(dec(col as i64));
    r.set(&v);
    0
}

/// SysTextScreenRead(row, col [, len]): the characters on the screen.
pub fn sys_text_screen_read(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 3 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let Some(row) = string2long(a.cstr(0)).filter(|&n| n >= 0) else {
        return INVALID;
    };
    let Some(col) = string2long(a.cstr(1)).filter(|&n| n >= 0) else {
        return INVALID;
    };
    let mut len: i32 = 0x1fe0;
    if a.n() == 3 {
        if !a.valid(2) {
            return INVALID;
        }
        match string2long(a.cstr(2)).filter(|&n| n >= 0) {
            Some(n) => len = n,
            None => return INVALID,
        }
    }
    let want = len as u32 as u16;
    let mut buf: *mut u8 = null_mut();
    if unsafe { DosAllocMem(&mut buf, 0x10000, PAG_COMMIT | PAG_WRITE | PAG_READ) } != 0 {
        r.set(b"2");
        return 0;
    }
    let got = vio_read_char_str(buf, want, row as u16, col as u16);
    let s = unsafe { core::slice::from_raw_parts(buf, got as usize) };
    if !r.set(s) {
        r.set(b"2");
    }
    unsafe { DosFreeMem(buf) };
    0
}
