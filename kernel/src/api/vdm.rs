//! RS2API entries for DOS sessions (crate::vdm, docs/DOS.md): `VDM.EXE`
//! shows its machine's screen and reads raw input through them, PMVIOWIN
//! reads a windowed DOS session's graphics picture. All 32-bit
//! `_System`.
//!
//! | Ordinal | Call |
//! |---------|------|
//! | 400 | `Rs2VdmRegister(ULONG fl)` |
//! | 401 | `Rs2VdmReadInput(PVDMEVENT aEvents, ULONG cMax, PULONG pcRead, ULONG ulTimeout)` |
//! | 402 | `Rs2VdmSetText(PVDMTEXT pText, PUSHORT pCells)` |
//! | 403 | `Rs2VdmSetPixels(ULONG cx, ULONG cy, ULONG y0, ULONG cRows, PBYTE pRows, PULONG pPalette)` |
//! | 404 | `Rs2VioReadPixels(ULONG sid, PULONG pInfo, PBYTE pPixels, ULONG cb, PULONG pPalette)` |
//! | 405 | `Rs2VdmMouse(ULONG sid, ULONG x, ULONG y, ULONG fsButtons)` |
//! | 406 | `Rs2VdmSurface(ULONG cmd, ULONG a, ULONG b)` |
//!
//! Rs2VdmRegister makes the caller's session a DOS session: from then on
//! its keys and mouse are not translated into KBD and MOU events but
//! queued raw for Rs2VdmReadInput. VDMEVENT (12 bytes): USHORT kind (1
//! key, 2 mouse motion, 3 mouse position), USHORT flags (key: the
//! character the keyboard layout gives a press, else 0; mouse: buttons,
//! bit 0 left, 1 right, 2 middle), LONG a (key: the set-1 scan code byte,
//! E0 prefixes as events of their own, bit 7 = release; mouse: dx in
//! mickeys or x 0-65535 across the screen), LONG b (dy, or y 0-65535 down).
//! Rs2VdmReadInput waits up to ulTimeout ms (0 none, -1 forever) for the
//! first event.
//!
//! Rs2VdmSetText shows a text screen: VDMTEXT is USHORT cols, rows,
//! cursor row, cursor column; BYTE first and last cursor scan line, flags
//! (1 cursor shown, 2 attribute bit 7 blinks), the mode's cell height;
//! ULONG colours[16] (0x00RRGGBB of the 16 attribute colours). pCells
//! holds cols x rows cells (char | attr << 8). It ends a graphics picture.
//!
//! Rs2VdmSetPixels shows a graphics picture of cx x cy 8-bit pixels: cRows
//! rows from row y0 (pRows, cx bytes each) and the 256-entry palette
//! (0x00RRGGBB). The kernel scales it to the largest 4:3 area of the
//! screen while the session is in the foreground; a windowed session's
//! host reads it with Rs2VioReadPixels: pInfo receives cx, cy and a
//! generation (bumped by every change), pPixels cx*cy bytes (as far as cb
//! holds), pPalette 256 entries; ERROR_INVALID_FUNCTION (1) when the
//! session shows text. Rs2VdmMouse gives a windowed DOS session the
//! pointer's position over its window (0-65535 each way).
//!
//! Rs2VdmSurface tells the kernel about a display surface the session's
//! VDM keeps in shared memory (`\SHAREMEM\RS2VDM\<pid>.DSP`, its
//! layout in userland/vdm; Win-OS/2's RS2DISP.DRV draws it): cmd 1 the
//! session shows an a x b surface, 0 no longer; 2 it changed (the PM host
//! redraws; returns 1 when the session is on the full screen, where the
//! VDM shows it with Rs2VdmSetPixels, else 0); 3 the screen's size into
//! the ULONGs at a and b.

use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::{err, Ctx};

const ERROR_TIMEOUT: u32 = 640;
const ERROR_SMG_SESSION_NOT_FOUND: u32 = 371;

fn sid(c: &Ctx) -> u32 {
    c.process.session.load(Ordering::Relaxed)
}

pub fn register(c: &mut Ctx) -> u32 {
    let s = sid(c);
    if s == 1 || s == crate::session::pm_session() {
        return err::INVALID_PARAMETER;
    }
    crate::vdm::register(s);
    0
}

pub fn read_input(c: &mut Ctx) -> u32 {
    let (p, max, pcount, ms) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let deadline = match ms {
        0 => Some(0),
        u32::MAX => None,
        ms => Some(crate::irq::ticks() + (ms as u64 * crate::irq::HZ).div_ceil(1000)),
    };
    let s = sid(c);
    let Some(evs) = crate::vdm::read(s, max.min(256) as usize, deadline) else { return err::INVALID_PARAMETER };
    let mut b = Vec::with_capacity(evs.len() * 12);
    for e in &evs {
        b.extend_from_slice(&e.kind.to_le_bytes());
        b.extend_from_slice(&e.flags.to_le_bytes());
        b.extend_from_slice(&e.a.to_le_bytes());
        b.extend_from_slice(&e.b.to_le_bytes());
    }
    if !b.is_empty() && !c.write(p, &b) {
        return err::INVALID_ADDRESS;
    }
    c.write_u32(pcount, evs.len() as u32);
    if evs.is_empty() {
        if c.thread.exit_requested() {
            return err::INTERRUPT;
        }
        if ms != 0 {
            return ERROR_TIMEOUT;
        }
    }
    0
}

pub fn set_text(c: &mut Ctx) -> u32 {
    let (pt, pcells) = (c.arg(0), c.arg(1));
    let Some(t) = c.read_vec(pt, 76) else { return err::INVALID_ADDRESS };
    let u16_at = |o: usize| u16::from_le_bytes([t[o], t[o + 1]]) as usize;
    let (cols, rows) = (u16_at(0).clamp(1, crate::screen::MAX_COLS), u16_at(2).clamp(1, crate::screen::MAX_ROWS));
    let (crow, ccol) = (u16_at(4), u16_at(6));
    let (cs, ce, flags, ch) = (t[8], t[9], t[10], t[11].max(1));
    let mut colours = [0u32; 16];
    for (i, v) in colours.iter_mut().enumerate() {
        *v = u32::from_le_bytes([t[12 + i * 4], t[13 + i * 4], t[14 + i * 4], t[15 + i * 4]]);
    }
    let Some(raw) = c.read_vec(pcells, (cols * rows * 2) as u32) else { return err::INVALID_ADDRESS };
    let s = sid(c);
    crate::screen::with_session(s, |sc| {
        sc.clear_pixels();
        if (sc.cols, sc.rows) != (cols, rows) && sc.mode_fits(cols, rows) {
            sc.set_mode(cols, rows);
        }
        if sc.palette_rgb() != colours {
            for (i, v) in colours.iter().enumerate() {
                sc.palregs[i] = i as u8;
                sc.dac[i] = (((v >> 16) & 0xff) as u8 >> 2, ((v >> 8) & 0xff) as u8 >> 2, (v & 0xff) as u8 >> 2);
            }
            sc.update_palette();
            sc.redraw();
        }
        let blink = flags & 2 != 0;
        if sc.blink != blink {
            sc.set_blink(blink);
        }
        let (rs, cl) = (sc.rows.min(rows), sc.cols.min(cols));
        for r in 0..rs {
            for col in 0..cl {
                let i = (r * cols + col) * 2;
                let v = u16::from_le_bytes([raw[i], raw[i + 1]]);
                if sc.cell(r, col) != v {
                    sc.put(r, col, v);
                }
            }
        }
        // Cursor lines are of the DOS cell height; scale to ours.
        let h = sc.cell_h as u32;
        let scale = |l: u8| ((l as u32 * h) / ch as u32).min(h - 1) as u16;
        let shown = flags & 1 != 0 && crow < sc.rows && ccol < sc.cols;
        let ty = [scale(cs), scale(ce), 1, if shown { 0 } else { 0xffff }];
        if sc.cursor_type != ty {
            sc.set_cursor_type(ty);
        }
        let pos = shown.then_some((crow, ccol));
        if sc.cursor() != pos {
            sc.set_cursor(pos);
        }
    });
    0
}

pub fn set_pixels(c: &mut Ctx) -> u32 {
    let (w, h, y0, n, prows, ppal) = (c.arg(0) as usize, c.arg(1) as usize, c.arg(2) as usize, c.arg(3) as usize, c.arg(4), c.arg(5));
    if w == 0 || h == 0 || w > 1600 || h > 1200 || y0 >= h {
        return err::INVALID_PARAMETER;
    }
    let n = n.min(h - y0);
    let Some(rows) = c.read_vec(prows, (w * n) as u32) else { return err::INVALID_ADDRESS };
    let Some(pb) = c.read_vec(ppal, 1024) else { return err::INVALID_ADDRESS };
    let mut pal = [0u32; 256];
    for (i, v) in pal.iter_mut().enumerate() {
        *v = u32::from_le_bytes([pb[i * 4], pb[i * 4 + 1], pb[i * 4 + 2], pb[i * 4 + 3]]);
    }
    let s = sid(c);
    crate::screen::with_session(s, |sc| sc.set_pixels(w, h, y0, &rows, &pal));
    0
}

pub fn read_pixels(c: &mut Ctx) -> u32 {
    let (s, pinfo, ppix, cb, ppal) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3) as usize, c.arg(4));
    if !crate::session::exists(s) {
        return ERROR_SMG_SESSION_NOT_FOUND;
    }
    let r = crate::screen::with_session(s, |sc| {
        sc.pixmap.as_ref().map(|p| {
            let n = p.data.len().min(cb);
            (p.w, p.h, p.generation, p.data[..n].to_vec(), p.pal)
        })
    });
    let Some(Some((w, h, generation, data, pal))) = r else { return err::INVALID_FUNCTION };
    let mut info = Vec::with_capacity(16);
    for v in [w as u32, h as u32, generation, 0] {
        info.extend_from_slice(&v.to_le_bytes());
    }
    if !c.write(pinfo, &info) {
        return err::INVALID_ADDRESS;
    }
    if ppix != 0 && !c.write(ppix, &data) {
        return err::INVALID_ADDRESS;
    }
    if ppal != 0 {
        let mut b = Vec::with_capacity(1024);
        for v in pal {
            b.extend_from_slice(&v.to_le_bytes());
        }
        if !c.write(ppal, &b) {
            return err::INVALID_ADDRESS;
        }
    }
    0
}

pub fn surface(c: &mut Ctx) -> u32 {
    let (cmd, a, b) = (c.arg(0), c.arg(1), c.arg(2));
    let s = sid(c);
    match cmd {
        0 | 1 => {
            let v = (cmd == 1 && a > 0 && b > 0 && a <= 4096 && b <= 4096).then_some((a as usize, b as usize));
            crate::screen::with_session(s, |sc| sc.set_surface(v));
            0
        }
        2 => crate::screen::with_session(s, |sc| {
            sc.surface_changed();
            sc.is_visible() as u32
        })
        .unwrap_or(0),
        3 => {
            let Some(fb) = crate::screen::framebuffer() else { return err::INVALID_FUNCTION };
            c.write_u32(a, fb.width);
            c.write_u32(b, fb.height);
            0
        }
        _ => err::INVALID_PARAMETER,
    }
}

pub fn mouse(c: &mut Ctx) -> u32 {
    let (s, x, y, b) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if crate::vdm::mouse_abs(s, b as u8, x, y) { 0 } else { ERROR_SMG_SESSION_NOT_FOUND }
}
