//! MOUCALLS: the mouse subsystem (16-bit `_Pascal` APIs; the last argument
//! is always the mouse handle). Events and the pointer are kept per
//! session in crate::mouse.

use super::{err, Ctx};
use crate::mouse;

const ERROR_MOUSE_NO_DEVICE: u32 = 385;
const ERROR_MOUSE_INV_HANDLE: u32 = 389;
const MOU_WAIT: u16 = 1;
const HMOU: u16 = 1;

fn sid(c: &Ctx) -> u32 {
    c.process.session.load(core::sync::atomic::Ordering::Relaxed)
}

/// Check the handle (the last argument, at `i`).
fn handle(c: &Ctx, i: usize) -> Result<(), u32> {
    if c.arg(i) as u16 != HMOU || !mouse::with(sid(c), |m| m.open) {
        return Err(ERROR_MOUSE_INV_HANDLE);
    }
    Ok(())
}

fn status(r: Result<u32, u32>) -> u32 {
    r.unwrap_or_else(|e| e)
}

pub fn ok(_c: &mut Ctx) -> u32 {
    0
}

/// MouOpen(pszDvrName, phmou)
pub fn mou_open(c: &mut Ctx) -> u32 {
    if !mouse::present() {
        return ERROR_MOUSE_NO_DEVICE;
    }
    mouse::with(sid(c), |m| m.open = true);
    c.write_u16(c.ptr(1), HMOU);
    0
}

pub fn mou_close(c: &mut Ctx) -> u32 {
    status(handle(c, 0).map(|_| {
        let s = sid(c);
        mouse::show(s, false);
        mouse::with(s, |m| {
            m.open = false;
            m.queue.clear();
        });
        0
    }))
}

/// MouReadEventQue(pEvent, pfWait, hmou): MOUEVENTINFO is fs, time, row,
/// col. With no event and no wait the record is all zero.
pub fn mou_read_event_que(c: &mut Ctx) -> u32 {
    status(handle(c, 2).map(|_| {
        let wait = c.read_u16(c.ptr(1)).unwrap_or(0) == MOU_WAIT;
        let e = mouse::read(sid(c), wait).unwrap_or_default();
        let at = c.ptr(0);
        c.write_u16(at, e.fs);
        c.write_u32(at + 2, e.time);
        c.write_u16(at + 6, e.row as u16);
        c.write_u16(at + 8, e.col as u16);
        0
    }))
}

/// MouGetNumQueEl(pQueInfo, hmou): events queued, queue size.
pub fn mou_get_num_que_el(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let n = mouse::with(sid(c), |m| m.queue.len());
        c.write_u16(c.ptr(0), n as u16);
        c.write_u16(c.ptr(0) + 2, 64);
        0
    }))
}

pub fn mou_flush_que(c: &mut Ctx) -> u32 {
    status(handle(c, 0).map(|_| {
        mouse::with(sid(c), |m| m.queue.clear());
        0
    }))
}

pub fn mou_get_num_buttons(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        c.write_u16(c.ptr(0), 3);
        0
    }))
}

/// Mickeys per centimetre.
pub fn mou_get_num_mickeys(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        c.write_u16(c.ptr(0), 50);
        0
    }))
}

pub fn mou_get_dev_status(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let v = mouse::with(sid(c), |m| m.dev_status);
        c.write_u16(c.ptr(0), v);
        0
    }))
}

pub fn mou_set_dev_status(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let v = c.read_u16(c.ptr(0)).unwrap_or(0);
        mouse::with(sid(c), |m| m.dev_status = (m.dev_status & 0xff) | (v & 0x0300));
        0
    }))
}

pub fn mou_get_event_mask(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let v = mouse::with(sid(c), |m| m.event_mask);
        c.write_u16(c.ptr(0), v);
        0
    }))
}

pub fn mou_set_event_mask(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let v = c.read_u16(c.ptr(0)).unwrap_or(0x7f);
        mouse::with(sid(c), |m| m.event_mask = v & 0x7f);
        0
    }))
}

/// PTRLOC: row, col.
pub fn mou_get_ptr_pos(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let (r, col) = mouse::with(sid(c), |m| (m.row, m.col));
        c.write_u16(c.ptr(0), r as u16);
        c.write_u16(c.ptr(0) + 2, col as u16);
        0
    }))
}

pub fn mou_set_ptr_pos(c: &mut Ctx) -> u32 {
    status(handle(c, 1).and_then(|_| {
        let r = c.read_u16(c.ptr(0)).unwrap_or(0) as i32;
        let col = c.read_u16(c.ptr(0) + 2).unwrap_or(0) as i32;
        let (rows, cols) = crate::screen::with_session(sid(c), |s| (s.rows as i32, s.cols as i32)).unwrap_or((25, 80));
        if r >= rows || col >= cols {
            return Err(err::INVALID_PARAMETER);
        }
        mouse::set_pos(sid(c), r, col);
        Ok(0)
    }))
}

pub fn mou_draw_ptr(c: &mut Ctx) -> u32 {
    status(handle(c, 0).map(|_| {
        mouse::show(sid(c), true);
        0
    }))
}

/// MouRemovePtr(pMouRect, hmou): hide the pointer (the whole screen is
/// treated as the exclusion area).
pub fn mou_remove_ptr(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        mouse::show(sid(c), false);
        0
    }))
}

/// SCALEFACT: rowScale, colScale.
pub fn mou_get_scale_fact(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        let (r, col) = mouse::with(sid(c), |m| m.scale);
        c.write_u16(c.ptr(0), r);
        c.write_u16(c.ptr(0) + 2, col);
        0
    }))
}

pub fn mou_set_scale_fact(c: &mut Ctx) -> u32 {
    status(handle(c, 1).and_then(|_| {
        let r = c.read_u16(c.ptr(0)).unwrap_or(0);
        let col = c.read_u16(c.ptr(0) + 2).unwrap_or(0);
        if r == 0 || col == 0 {
            return Err(err::INVALID_PARAMETER);
        }
        mouse::with(sid(c), |m| m.scale = (r, col));
        Ok(0)
    }))
}

/// MouGetPtrShape(pBuf, pPtrShape, hmou): the text pointer is a 1x1
/// inverting AND/XOR mask.
pub fn mou_get_ptr_shape(c: &mut Ctx) -> u32 {
    status(handle(c, 2).map(|_| {
        let ps = c.ptr(1);
        let cb = c.read_u16(ps).unwrap_or(0);
        // PTRSHAPE: cb, col, row, colHot, rowHot.
        for (i, v) in [4u16, 1, 1, 0, 0].into_iter().enumerate() {
            c.write_u16(ps + i as u32 * 2, v);
        }
        if cb < 4 {
            return err::BUFFER_OVERFLOW;
        }
        c.write_u16(c.ptr(0), 0xffff);
        c.write_u16(c.ptr(0) + 2, 0x7700);
        0
    }))
}

pub fn mou_set_ptr_shape(c: &mut Ctx) -> u32 {
    status(handle(c, 2).map(|_| 0))
}

/// THRESHOLD: length, level1, lev1Mult, level2, lev2Mult.
pub fn mou_get_threshold(c: &mut Ctx) -> u32 {
    status(handle(c, 1).map(|_| {
        for (i, v) in [10u16, 4, 2, 12, 4].into_iter().enumerate() {
            c.write_u16(c.ptr(0) + i as u32 * 2, v);
        }
        0
    }))
}
