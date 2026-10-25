//! The mouse driver (INT 33h), with its pointer drawn into video memory
//! as a DOS mouse driver does, and the user event handler called from
//! IRQ 12.

use x86emu::{Seg, CS, ES, ESP, SS};

use crate::{Host, Machine};

#[derive(Clone, Copy, Default)]
struct Saved {
    regs: [u32; 8],
    segs: [u16; 6],
    eip: u32,
    flags: u32,
}

pub struct Mouse {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    /// Visible when 0 (show increments, hide decrements).
    pub shown: i32,
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    /// Mickeys per 8 pixels.
    pub mickey_x: i32,
    pub mickey_y: i32,
    pub mickeys: (i32, i32),
    /// Fractional mickeys.
    acc: (i32, i32),
    pub presses: [(u16, i32, i32); 3],
    pub releases: [(u16, i32, i32); 3],
    pub handler: Option<(u16, u16, u16)>,
    /// The handler is a DPMI client's protected-mode procedure (set
    /// from protected mode, as Win-OS/2's MOUSE.DRV does).
    handler_pm: bool,
    /// Events not yet delivered to the handler.
    pending: u16,
    in_handler: bool,
    saved: Saved,
    /// Text cursor: screen and cursor masks; what is under it.
    pub text_masks: (u16, u16),
    under: Option<(u32, u16)>,
    /// Graphics cursor: hot spot and the 32 mask words.
    pub hot: (i32, i32),
    pub shape: [u16; 32],
    gfx_under: Option<(i32, i32, [[u8; 16]; 16])>,
    pub reset_done: bool,
    pub owner: u16,
    /// Win-OS/2's MOUSE.DRV asked VMOUSE$ for its scale factors: its
    /// protected-mode handler gets absolute positions (AX bit 15, CX
    /// and DX 0-65535 across the screen), as OS/2's VMOUSE gives them.
    pub absolute: bool,
    /// The position as a fraction of the screen (0-65535 each way).
    pub abs: (u32, u32),
    /// The screen's size when a display driver of the machine's own
    /// (crate::disp) draws it, instead of the video mode's.
    pub screen: Option<(i32, i32)>,
}

impl Default for Mouse {
    fn default() -> Self {
        let mut shape = [0u16; 32];
        // The default arrow: screen mask, then cursor mask.
        let arrow: [u16; 16] = [0x0000, 0x4000, 0x6000, 0x7000, 0x7800, 0x7c00, 0x7e00, 0x7f00, 0x7f80, 0x7c00, 0x6c00, 0x4600, 0x0600, 0x0300, 0x0300, 0x0000];
        for i in 0..16 {
            shape[i] = !(arrow[i] | arrow[i] >> 1 | arrow[i] << 1 | if i > 0 { arrow[i - 1] } else { 0 } | if i < 15 { arrow[i + 1] } else { 0 });
            shape[16 + i] = arrow[i];
        }
        Mouse {
            x: 320,
            y: 100,
            buttons: 0,
            shown: -1,
            min_x: 0,
            max_x: 639,
            min_y: 0,
            max_y: 199,
            mickey_x: 8,
            mickey_y: 16,
            mickeys: (0, 0),
            acc: (0, 0),
            presses: [(0, 0, 0); 3],
            releases: [(0, 0, 0); 3],
            handler: None,
            handler_pm: false,
            pending: 0,
            in_handler: false,
            saved: Saved::default(),
            text_masks: (0x77ff, 0x7700),
            under: None,
            hot: (0, 0),
            shape,
            gfx_under: None,
            reset_done: false,
            owner: 0,
            absolute: false,
            abs: (0x8000, 0x8000),
            screen: None,
        }
    }
}

impl Machine {
    /// The virtual screen the driver uses for the current mode.
    fn mouse_extent(&mut self) -> (i32, i32) {
        if let Some(s) = self.mouse.screen {
            return s;
        }
        let m = self.bda_mode();
        match m {
            0x0f | 0x10 => (640, 350),
            0x11 | 0x12 => (640, 480),
            _ => {
                let rows = self.bda_rows() as i32;
                (640, if m <= 3 || m == 7 { rows * 8 } else { 200 })
            }
        }
    }

    /// Screen pixel of the virtual mouse position (graphics modes).
    fn mouse_pixel(&mut self) -> (i32, i32) {
        let m = self.bda_mode();
        let x = if matches!(m, 4 | 5 | 0x0d | 0x13) { self.mouse.x / 2 } else { self.mouse.x };
        (x, self.mouse.y)
    }

    pub(crate) fn mouse_motion(&mut self, dx: i32, dy: i32, buttons: u8) {
        let ms = &mut self.mouse;
        ms.mickeys.0 = ms.mickeys.0.wrapping_add(dx);
        ms.mickeys.1 = ms.mickeys.1.wrapping_add(dy);
        ms.acc.0 += dx * 8;
        ms.acc.1 += dy * 8;
        let px = ms.acc.0 / ms.mickey_x.max(1);
        let py = ms.acc.1 / ms.mickey_y.max(1);
        ms.acc.0 -= px * ms.mickey_x.max(1);
        ms.acc.1 -= py * ms.mickey_y.max(1);
        let (x, y) = (ms.x + px, ms.y + py);
        self.mouse_move_to(x, y, buttons, dx != 0 || dy != 0);
        let (w, h) = self.mouse_extent();
        let ms = &mut self.mouse;
        ms.abs = (((ms.x.max(0) as u32) << 16) / w.max(1) as u32, ((ms.y.max(0) as u32) << 16) / h.max(1) as u32);
    }

    pub(crate) fn mouse_abs(&mut self, x: u32, y: u32, buttons: u8) {
        let (w, h) = self.mouse_extent();
        let nx = (x as i64 * w as i64 / 65536) as i32;
        let ny = (y as i64 * h as i64 / 65536) as i32;
        let (dx, dy) = (nx - self.mouse.x, ny - self.mouse.y);
        let moved = (x.min(65535), y.min(65535)) != self.mouse.abs;
        self.mouse.abs = (x.min(65535), y.min(65535));
        // At least a mickey for a move, which MOUSE.DRV checks.
        let mk = |d: i32, m: i32, a: bool| if d == 0 && a { 1 } else { d * m / 8 };
        self.mouse.mickeys.0 += mk(dx, self.mouse.mickey_x, moved);
        self.mouse.mickeys.1 += mk(dy, self.mouse.mickey_y, false);
        self.mouse_move_to(nx, ny, buttons, moved || dx != 0 || dy != 0);
    }

    fn mouse_move_to(&mut self, x: i32, y: i32, buttons: u8, moved: bool) {
        self.mouse_hide_cursor();
        let ms = &mut self.mouse;
        ms.x = x.clamp(ms.min_x, ms.max_x);
        ms.y = y.clamp(ms.min_y, ms.max_y);
        let mut ev = if moved { 1u16 } else { 0 };
        for b in 0..3 {
            let was = ms.buttons & (1 << b) != 0;
            let now = buttons & (1 << b) != 0;
            if now && !was {
                ms.presses[b].0 = ms.presses[b].0.wrapping_add(1);
                ms.presses[b].1 = ms.x;
                ms.presses[b].2 = ms.y;
                ev |= [2, 8, 32][b];
            } else if was && !now {
                ms.releases[b].0 = ms.releases[b].0.wrapping_add(1);
                ms.releases[b].1 = ms.x;
                ms.releases[b].2 = ms.y;
                ev |= [4, 16, 64][b];
            }
        }
        ms.buttons = buttons;
        if let Some((mask, _, _)) = ms.handler {
            if ev & mask != 0 {
                ms.pending |= ev;
            }
        }
        self.mouse_show_cursor();
    }

    /// Raise IRQ 12 when the handler has events to see.
    pub(crate) fn mouse_irq(&mut self) {
        if (self.mouse.pending != 0 || self.vwin_call_ready()) && !self.mouse.in_handler {
            self.hw.pic.raise(12);
        }
    }

    /// INT 74h (IRQ 12): call the program's event handler.
    pub(crate) fn mouse_service(&mut self) {
        self.eoi(12);
        // VWIN's user routine (WINSHELD's) is called from here too: the
        // same far call into protected mode from real mode.
        if !self.mouse.in_handler && !self.cpu.pm() && self.vwin_call_ready() {
            let mut segs = [0u16; 6];
            for (i, s) in segs.iter_mut().enumerate() {
                *s = self.cpu.seg[i].sel;
            }
            self.mouse.saved = Saved { regs: self.cpu.regs, segs, eip: self.cpu.eip, flags: self.cpu.flags };
            self.mouse.in_handler = true;
            self.vwin_call_routine();
            return;
        }
        let ev = core::mem::take(&mut self.mouse.pending);
        let Some((mask, seg, off)) = self.mouse.handler else { return };
        if ev & mask == 0 || self.mouse.in_handler || self.cpu.pm() {
            return;
        }
        let mut segs = [0u16; 6];
        for (i, s) in segs.iter_mut().enumerate() {
            *s = self.cpu.seg[i].sel;
        }
        self.mouse.saved = Saved { regs: self.cpu.regs, segs, eip: self.cpu.eip, flags: self.cpu.flags };
        self.mouse.in_handler = true;
        // A far call to the handler that returns to the callback.
        let _ = self.cpu.push(&mut self.hw, crate::bios::ROM as u32, 2);
        let _ = self.cpu.push(&mut self.hw, crate::bios::MOUSE_RET_CODE as u32, 2);
        let (b, x, y) = (self.mouse.buttons, self.mouse.x, self.mouse.y);
        let abs = self.mouse.absolute && self.mouse.handler_pm;
        self.cpu.set_ax(ev & mask | if abs { 0x8000 } else { 0 });
        self.cpu.set_bx(b as u16);
        if abs {
            let (ax, ay) = self.mouse.abs;
            self.cpu.set_cx(ax as u16);
            self.cpu.set_dx(ay as u16);
        } else {
            self.cpu.set_cx(x as u16);
            self.cpu.set_dx(y as u16);
        }
        let (mx, my) = self.mouse.mickeys;
        self.cpu.set_si(mx as u16);
        self.cpu.set_di(my as u16);
        if self.mouse.handler_pm {
            // In protected mode; mouse_handler_done follows its return.
            let _ = self.cpu.pop(&mut self.hw, 2);
            let _ = self.cpu.pop(&mut self.hw, 2);
            self.dpmi_call_pm_args(seg, off as u32, &[]);
            return;
        }
        self.cpu.seg[CS] = Seg::real_code(seg);
        self.cpu.eip = off as u32;
    }

    pub(crate) fn mouse_handler_done(&mut self) {
        let s = self.mouse.saved;
        self.cpu.regs = s.regs;
        for (i, v) in s.segs.iter().enumerate() {
            self.cpu.seg[i] = if i == CS { Seg::real_code(*v) } else { Seg::real(*v) };
        }
        self.cpu.eip = s.eip;
        self.cpu.flags = s.flags;
        self.mouse.in_handler = false;
        self.vwin.in_call = false;
    }

    fn mouse_visible(&self) -> bool {
        self.mouse.shown >= 0
    }

    fn mouse_hide_cursor(&mut self) {
        if let Some((a, cell)) = self.mouse.under.take() {
            self.hw.wr16(a, cell);
        }
        if let Some((x, y, under)) = self.mouse.gfx_under.take() {
            for (r, row) in under.iter().enumerate() {
                for (c, &v) in row.iter().enumerate() {
                    let (px, py) = (x + c as i32, y + r as i32);
                    if px >= 0 && py >= 0 {
                        self.put_pixel(px as u32, py as u32, v);
                    }
                }
            }
        }
    }

    fn mouse_show_cursor(&mut self) {
        if !self.mouse_visible() {
            return;
        }
        let m = self.bda_mode();
        if m <= 3 || m == 7 {
            let cols = self.bda_cols() as i32;
            let col = (self.mouse.x / 8).min(cols - 1);
            let row = self.mouse.y / 8;
            let base = if m == 7 { 0xb0000 } else { 0xb8000 };
            let page = self.hw.rd16(0x44e) as u32;
            let a = base + page + ((row * cols + col) * 2) as u32;
            let cell = self.hw.rd16(a);
            self.mouse.under = Some((a, cell));
            let (sm, cm) = self.mouse.text_masks;
            self.hw.wr16(a, (cell & sm) ^ cm);
        } else {
            let (px, py) = self.mouse_pixel();
            let (x, y) = (px - self.mouse.hot.0, py - self.mouse.hot.1);
            let mut under = [[0u8; 16]; 16];
            let (w, h) = match self.hw.vga.kind() {
                crate::video::Kind::Graphics { width, height } => (width as i32, height as i32),
                _ => (0, 0),
            };
            let white = if m == 6 || m == 0x11 { 1 } else if m == 4 || m == 5 { 3 } else { 15 };
            for r in 0..16 {
                for c in 0..16 {
                    let (qx, qy) = (x + c, y + r);
                    if qx < 0 || qy < 0 || qx >= w || qy >= h {
                        continue;
                    }
                    let v = self.get_pixel(qx as u32, qy as u32);
                    under[r as usize][c as usize] = v;
                    let bit = 0x8000u16 >> c;
                    let sm = self.mouse.shape[r as usize] & bit != 0;
                    let cm = self.mouse.shape[16 + r as usize] & bit != 0;
                    let nv = match (sm, cm) {
                        (true, false) => v,
                        (true, true) => v ^ white,
                        (false, false) => 0,
                        (false, true) => white,
                    };
                    self.put_pixel(qx as u32, qy as u32, nv);
                }
            }
            self.mouse.gfx_under = Some((x, y, under));
        }
    }

    pub(crate) fn mouse_process_ended(&mut self, psp: u16) {
        if self.mouse.owner == psp {
            self.mouse_hide_cursor();
            self.mouse.handler = None;
            self.mouse.shown = -1;
        }
    }

    fn mouse_reset(&mut self) {
        self.mouse_hide_cursor();
        let keep = (self.mouse.x, self.mouse.y, self.mouse.buttons);
        let (absolute, abs, screen) = (self.mouse.absolute, self.mouse.abs, self.mouse.screen);
        self.mouse = Mouse::default();
        (self.mouse.absolute, self.mouse.abs, self.mouse.screen) = (absolute, abs, screen);
        let (w, h) = self.mouse_extent();
        self.mouse.max_x = w - 1;
        self.mouse.max_y = h - 1;
        self.mouse.x = w / 2;
        self.mouse.y = h / 2;
        self.mouse.buttons = keep.2;
        self.mouse.reset_done = true;
        self.mouse.owner = self.dos.psp;
    }

    pub(crate) fn int33<H: Host>(&mut self, host: &mut H) {
        let _ = host;
        let ax = self.cpu.ax();
        match ax {
            0x00 | 0x21 => {
                self.mouse_reset();
                self.cpu.set_ax(0xffff);
                self.cpu.set_bx(2);
            }
            0x01 => {
                if self.mouse.shown < 0 {
                    self.mouse.shown += 1;
                    if self.mouse.shown == 0 {
                        self.mouse_show_cursor();
                    }
                }
            }
            0x02 => {
                if self.mouse.shown == 0 {
                    self.mouse_hide_cursor();
                }
                self.mouse.shown -= 1;
            }
            0x03 => {
                let (b, x, y) = (self.mouse.buttons, self.mouse.x, self.mouse.y);
                self.cpu.set_bx(b as u16);
                self.cpu.set_cx(x as u16);
                self.cpu.set_dx(y as u16);
            }
            0x04 => {
                let (x, y) = (self.cpu.cx() as i16 as i32, self.cpu.dx() as i16 as i32);
                let b = self.mouse.buttons;
                self.mouse_move_to(x, y, b, false);
            }
            0x05 | 0x06 => {
                let b = (self.cpu.bx() as usize).min(2);
                let e = if ax == 5 { &mut self.mouse.presses[b] } else { &mut self.mouse.releases[b] };
                let (n, x, y) = *e;
                e.0 = 0;
                let st = self.mouse.buttons;
                self.cpu.set_ax(st as u16);
                self.cpu.set_bx(n);
                self.cpu.set_cx(x as u16);
                self.cpu.set_dx(y as u16);
            }
            0x07 | 0x08 => {
                let (a, b) = (self.cpu.cx() as i16 as i32, self.cpu.dx() as i16 as i32);
                let (lo, hi) = (a.min(b), a.max(b));
                if ax == 7 {
                    self.mouse.min_x = lo;
                    self.mouse.max_x = hi;
                } else {
                    self.mouse.min_y = lo;
                    self.mouse.max_y = hi;
                }
                let b = self.mouse.buttons;
                let (x, y) = (self.mouse.x, self.mouse.y);
                self.mouse_move_to(x, y, b, false);
            }
            0x09 => {
                self.mouse_hide_cursor();
                self.mouse.hot = (self.cpu.bx() as i16 as i32, self.cpu.cx() as i16 as i32);
                let a = self.seg_lin(ES, self.cpu.dx());
                for i in 0..32 {
                    self.mouse.shape[i] = self.hw.rd16(a + i as u32 * 2);
                }
                self.mouse_show_cursor();
            }
            0x0a => {
                self.mouse_hide_cursor();
                self.mouse.text_masks = (self.cpu.cx(), self.cpu.dx());
                self.mouse_show_cursor();
            }
            0x0b => {
                let (x, y) = core::mem::take(&mut self.mouse.mickeys);
                self.cpu.set_cx(x as u16);
                self.cpu.set_dx(y as u16);
            }
            0x0c | 0x14 => {
                let (old, old_pm) = (self.mouse.handler, self.mouse.handler_pm);
                let (cx, es, dx) = (self.cpu.cx(), self.cpu.seg[ES].sel, self.cpu.dx());
                self.mouse.handler = if cx == 0 { None } else { Some((cx, es, dx)) };
                self.mouse.handler_pm = self.cpu.pm();
                self.mouse.owner = self.dos.psp;
                if ax == 0x14 {
                    let (m, s, o) = old.unwrap_or((0, 0, 0));
                    self.cpu.set_cx(m);
                    if self.cpu.pm() {
                        let s = if old_pm { s } else { 0 };
                        let _ = self.cpu.load_seg(&mut self.hw, ES, s);
                    } else {
                        self.cpu.seg[ES] = Seg::real(s);
                    }
                    self.cpu.set_dx(o);
                }
            }
            0x0f => {
                self.mouse.mickey_x = (self.cpu.cx() as i32).max(1);
                self.mouse.mickey_y = (self.cpu.dx() as i32).max(1);
            }
            0x10 | 0x13 | 0x1a | 0x1d => {}
            0x15 => self.cpu.set_bx(64),
            0x16 | 0x17 => {}
            0x1b => {
                self.cpu.set_bx(50);
                self.cpu.set_cx(50);
                self.cpu.set_dx(50);
            }
            0x1e => self.cpu.set_bx(0),
            0x24 => {
                self.cpu.set_bx(0x0626);
                self.cpu.set_cx(0x0400);
            }
            0x26 => {
                self.cpu.set_bx(0);
                let (mx, my) = (self.mouse.max_x, self.mouse.max_y);
                self.cpu.set_cx(mx as u16);
                self.cpu.set_dx(my as u16);
            }
            _ => {}
        }
        let _ = (SS, ESP);
    }
}
