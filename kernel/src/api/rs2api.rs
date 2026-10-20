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
    display::add_pm(c.process.pid);
    {
        let mut sp = c.space();
        if !sp.regions.contains_key(&obj.base) {
            sp.map_shared(obj.clone(), crate::vm::pag::READ | crate::vm::pag::WRITE, crate::vm::SegKind::Data);
        }
    }
    // A PM program now belongs to the PM session, which comes to the
    // front. A text-mode program that only initialises PM (REXXUTIL's
    // SysIni calls WinInitialize) stays in its session: moving it took its
    // keyboard away, and CMD.EXE never saw another key.
    if c.process.exe.app_type == 3 {
        let pm = session::pm_session_or_create();
        c.process.session.store(pm, Ordering::Relaxed);
        session::select(pm);
    }
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

/// Rs2SetPointerShape(cx, cy, xHot, yHot, pulPixels): make the mouse
/// pointer a hardware cursor with this image (0xAARRGGBB pixels, rows
/// from the top, at most 64x64; hot spot from the top left). The kernel
/// moves it with the mouse from then on. ERROR_NOT_SUPPORTED without a
/// hardware cursor (virtio-gpu or VMware SVGA II).
pub fn rs2_set_pointer_shape(c: &mut Ctx) -> u32 {
    let (w, h, hx, hy, p) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    if w == 0 || h == 0 || w > 64 || h > 64 || hx >= w || hy >= h {
        return err::INVALID_PARAMETER;
    }
    let mut px = alloc::vec::Vec::with_capacity((w * h) as usize);
    for i in 0..w * h {
        let Some(v) = c.read_u32(p + i * 4) else { return err::INVALID_ADDRESS };
        px.push(v);
    }
    if display::define_cursor(w, h, hx, hy, &px) { 0 } else { err::NOT_SUPPORTED }
}

/// Rs2ShowPointer(fShow): show or hide the hardware cursor.
pub fn rs2_show_pointer(c: &mut Ctx) -> u32 {
    display::show_cursor(c.arg(0) != 0);
    0
}

/// Rs2DebugTrace(psz): a line on the serial port (debugging aid for rs2's
/// own DLLs).
pub fn rs2_debug_trace(c: &mut Ctx) -> u32 {
    let Some(t) = c.read_str(c.arg(0), 512) else { return err::INVALID_ADDRESS };
    crate::trace!("[{}] {}", c.process.pid, t);
    0
}

/// Rs2PowerOff(ulAction) (7): the end of a shutdown. 0 powers the
/// machine off (ACPI S5) and returns ERROR_NOT_SUPPORTED if it is still
/// running then; 1 restarts it. The disks' write caches are committed
/// first; the caller has done DosShutdown.
pub fn rs2_power_off(c: &mut Ctx) -> u32 {
    let action = c.arg(0);
    if action > 1 {
        return err::INVALID_PARAMETER;
    }
    crate::drivers::flush_disks();
    crate::println!("[Rs2PowerOff: {}]", if action == 1 { "restart" } else { "power off" });
    if action == 1 {
        crate::power::reboot();
    }
    crate::power::try_power_off();
    err::NOT_SUPPORTED
}
