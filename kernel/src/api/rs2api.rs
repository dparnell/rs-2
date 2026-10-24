//! RS2API: rs2-private kernel services for rs2's own DLLs (the
//! Presentation Manager). Applications never import this module.

use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::{display, session};

/// Rs2MapDisplay(PDISPLAYINFO): join the PM session, map the framebuffer
/// and describe it: address, width, height, stride (pixels), flags (bit 0:
/// red in the low byte).
pub fn rs2_map_display(c: &mut Ctx) -> u32 {
    let Some(d) = display::display() else { return err::NOT_SUPPORTED };
    display::add_pm(c.process.pid);
    {
        let mut sp = c.space();
        if !sp.regions.contains_key(&d.obj.base) {
            sp.map_shared(d.obj.clone(), crate::vm::pag::READ | crate::vm::pag::WRITE, crate::vm::SegKind::Data);
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
    write_info(c, &d);
    0
}

fn write_info(c: &mut Ctx, d: &display::Info) {
    let at = c.arg(0);
    for (i, v) in [d.obj.base + d.offset, d.width, d.height, d.stride, d.red_low as u32].into_iter().enumerate() {
        c.write_u32(at + i as u32 * 4, v);
    }
}

/// Rs2QueryDisplay(PDISPLAYINFO) (21): the display as Rs2MapDisplay
/// describes it, after a mode change (EV_MODE), and a sixth ULONG: the
/// number of mode changes so far. The caller has mapped it.
pub fn rs2_query_display(c: &mut Ctx) -> u32 {
    let Some(d) = display::query() else { return err::NOT_SUPPORTED };
    write_info(c, &d);
    c.write_u32(c.arg(0) + 20, display::mode_generation());
    0
}

/// Rs2QueryDisplayModes(pul, cMax, pcModes) (22): the resolutions
/// Rs2SetDisplayMode can switch to, as (width, height) pairs, at most
/// `cMax`; *pcModes = how many there are.
pub fn rs2_query_display_modes(c: &mut Ctx) -> u32 {
    let (p, max, pc) = (c.arg(0), c.arg(1), c.arg(2));
    let modes = display::modes();
    for (i, &(w, h)) in modes.iter().take(max as usize).enumerate() {
        c.write_u32(p + i as u32 * 8, w);
        c.write_u32(p + i as u32 * 8 + 4, h);
    }
    c.write_u32(pc, modes.len() as u32);
    0
}

/// Rs2SetDisplayMode(cx, cy) (23): switch the display to `cx` x `cy`
/// (0, 0: the display's preferred size). PM gets EV_MODE.
/// ERROR_NOT_SUPPORTED if the driver cannot.
pub fn rs2_set_display_mode(c: &mut Ctx) -> u32 {
    let (w, h) = (c.arg(0), c.arg(1));
    if (w, h) != (0, 0) && !display::modes().contains(&(w, h)) {
        return err::INVALID_PARAMETER;
    }
    if display::set_mode(w, h) { 0 } else { err::NOT_SUPPORTED }
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

/// Rs2UpdateDisplay(pul, c) (20): PM drew on the screen in `c`
/// rectangles at `pul` (four ULONGs each: left, top, right, bottom in
/// framebuffer pixels, rows from the top; right and bottom exclusive).
/// They go from PM's back buffer to the framebuffer now (the caller
/// holds PM's lock, so they are as PM finished them), and display drivers
/// that copy changes to the display copy those areas.
pub fn rs2_update_display(c: &mut Ctx) -> u32 {
    let (p, n) = (c.arg(0), c.arg(1));
    if n > 64 {
        return err::INVALID_PARAMETER;
    }
    let mut rects = alloc::vec::Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut v = [0u32; 4];
        for (k, x) in v.iter_mut().enumerate() {
            let Some(r) = c.read_u32(p + (i * 4 + k as u32) * 4) else { return err::INVALID_ADDRESS };
            *x = r;
        }
        rects.push((v[0], v[1], v[2], v[3]));
    }
    display::present(&rects);
    0
}

/// Rs2QueryProcesses(pBuf, cbBuf) (24): the processes as text, a line
/// each: "pid ppid threads flags name", flags P (a PM program) and L
/// (holds PM's lock); the bytes written (the whole list if it fits,
/// else as many whole lines as fit).
pub fn rs2_query_processes(c: &mut Ctx) -> u32 {
    let (buf, cb) = (c.arg(0), c.arg(1) as usize);
    let lock = super::sem::mutex_owner("\\SEM32\\PMWIN\\LOCK").map(|(p, _)| p);
    let mut out = alloc::string::String::new();
    for p in crate::task::process::all() {
        let n = p.threads.lock().len();
        let flags = alloc::format!("{}{}", if crate::display::is_pm(p.pid) { "P" } else { "-" }, if lock == Some(p.pid) { "L" } else { "-" });
        let line = alloc::format!("{} {} {} {} {}\n", p.pid, p.ppid, n, flags, p.exe.name);
        if out.len() + line.len() > cb {
            break;
        }
        out.push_str(&line);
    }
    if !c.write(buf, out.as_bytes()) {
        return 0;
    }
    out.len() as u32
}

/// Rs2KillProcess(pid) (26): end any process (not only a descendant,
/// as DosKillProcess): the rescue screen's way out of a hang.
pub fn rs2_kill_process(c: &mut Ctx) -> u32 {
    let Some(p) = crate::task::process::by_pid(c.arg(0)) else { return err::INVALID_PROCID };
    if p.exit_info().is_some() || p.pid == c.process.pid {
        return err::INVALID_PROCID;
    }
    crate::trace!("rescue: process {} ({}) ended by process {}", p.pid, p.exe.name, c.process.pid);
    p.exit(3, 0); // TC_KILLPROCESS
    0
}

/// Rs2QueryThreadReport(pBuf, cbBuf) (25): what Ctrl+Alt+F12 writes on
/// the serial port, as text (cut at `cbBuf`); the bytes written.
pub fn rs2_query_thread_report(c: &mut Ctx) -> u32 {
    let (buf, cb) = (c.arg(0), c.arg(1) as usize);
    let r = crate::debug::thread_report();
    let n = r.len().min(cb);
    if !c.write(buf, &r.as_bytes()[..n]) {
        return 0;
    }
    n as u32
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
