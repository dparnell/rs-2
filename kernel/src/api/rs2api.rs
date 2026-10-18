//! RS2API: rs2-private kernel services for rs2's own DLLs (the
//! Presentation Manager). Applications never import this module.

use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::{display, session};

/// Rs2MapDisplay(PDISPLAYINFO): join the PM session, map the framebuffer
/// and describe it: address, width, height, stride (pixels), flags (bit 0:
/// red in the low byte).
pub fn rs2_map_display(c: &mut Ctx) -> u32 {
    let Some((obj, w, h, stride, red_low)) = display::display() else { return err::NOT_SUPPORTED };
    let offset = crate::screen::framebuffer().map_or(0, |f| (f.base & 0xfff) as u32);
    {
        let mut sp = c.space();
        if !sp.regions.contains_key(&obj.base) {
            sp.map_shared(obj.clone(), crate::vm::pag::READ | crate::vm::pag::WRITE, crate::vm::SegKind::Data);
        }
    }
    // The process now belongs to the PM session, which comes to the front.
    let pm = session::pm_session_or_create();
    c.process.session.store(pm, Ordering::Relaxed);
    session::select(pm);
    let at = c.arg(0);
    for (i, v) in [obj.base + offset, w, h, stride, red_low as u32].into_iter().enumerate() {
        c.write_u32(at + i as u32 * 4, v);
    }
    0
}

/// Rs2ReadInput(PINPUTEVENT, ulTimeout): ERROR_TIMEOUT (640) when nothing
/// arrived in time.
pub fn rs2_read_input(c: &mut Ctx) -> u32 {
    let Some(e) = display::read(c.arg(1)) else {
        return if c.thread.exit_requested() { err::INTERRUPT } else { 640 };
    };
    let at = c.arg(0);
    c.write_u16(at, e.kind);
    c.write_u16(at + 2, e.flags);
    c.write_u32(at + 4, e.a);
    c.write_u32(at + 8, e.b);
    c.write_u32(at + 12, e.time);
    0
}

/// Rs2QueryPointer(PPOINTL): pointer position in pixels.
pub fn rs2_query_pointer(c: &mut Ctx) -> u32 {
    let (x, y) = display::pointer();
    c.write_u32(c.arg(0), x as u32);
    c.write_u32(c.arg(0) + 4, y as u32);
    0
}
