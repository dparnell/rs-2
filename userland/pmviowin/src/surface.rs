//! DOS sessions with a display surface (Win-OS/2 with RS2DISP.DRV): the
//! VDM shares its screen (crate `win16drv::surface`); the window shows
//! it 1:1, redrawing the rectangles the VDM reports, and takes the
//! pointer shape Windows set.

use alloc::vec;
use alloc::vec::Vec;

use win16drv::surface::{self as sf, Surface};

use crate::{PointL, RectL};

unsafe extern "C" {
    #[link_name = "PMWIN.757"]
    fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.866"]
    fn WinSetPointer(desktop: u32, hptr: u32) -> u32;
    #[link_name = "PMWIN.717"]
    fn WinCreatePointer(desktop: u32, hbm: u32, pointer: u32, x: i32, y: i32) -> u32;
    #[link_name = "PMWIN.727"]
    fn WinDestroyPointer(hptr: u32) -> u32;
    #[link_name = "PMWIN.828"]
    fn WinQuerySysPointer(desktop: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, bmp: *const u8, fl: u32, bits: *const u8, bmi: *const u8) -> u32;
    #[link_name = "PMGPI.371"]
    fn GpiDeleteBitmap(hbm: u32) -> u32;
}

/// A session's surface as the window sees it.
pub struct View {
    pub s: Surface,
    pub w: usize,
    pub h: usize,
    /// Damage entries seen.
    seq: u32,
    ptr_gen: u32,
    /// The PM pointer made from Windows' (0: none made; hidden pointers
    /// get a blank one).
    pub hptr: u32,
    pal_gen: u32,
    pub pal: [u32; 256],
}

impl View {
    /// Map process `pid`'s surface.
    pub fn open(pid: u32) -> Option<View> {
        let name = sf::name(pid);
        let mut base: *mut u8 = core::ptr::null_mut();
        if unsafe { os2::DosGetNamedSharedMem(&mut base, name.as_ptr(), 0x3) } != 0 || base.is_null() {
            return None;
        }
        let s = Surface(base);
        if unsafe { s.get(0) } != sf::MAGIC {
            return None;
        }
        let (w, h) = unsafe { (s.get(sf::W) as usize, s.get(sf::H) as usize) };
        Some(View { s, w, h, seq: unsafe { s.get(sf::SEQ) }, ptr_gen: 0, hptr: 0, pal_gen: u32::MAX, pal: [0; 256] })
    }

    /// The rectangles (surface coordinates) damaged since the last call;
    /// None: all of it.
    pub fn damage(&mut self) -> Option<Vec<(i32, i32, i32, i32)>> {
        let (w, h) = unsafe { (self.s.get(sf::W) as usize, self.s.get(sf::H) as usize) };
        let pg = unsafe { self.s.get(sf::PAL_GEN) };
        let seq = unsafe { self.s.get(sf::SEQ) };
        let behind = seq.wrapping_sub(self.seq);
        let all = (w, h) != (self.w, self.h) || pg != self.pal_gen || behind > sf::RING_LEN;
        if pg != self.pal_gen {
            for (i, v) in self.pal.iter_mut().enumerate() {
                *v = unsafe { self.s.get(sf::PALETTE + i * 4) };
            }
            self.pal_gen = pg;
        }
        self.w = w;
        self.h = h;
        let old = self.seq;
        self.seq = seq;
        if all {
            return None;
        }
        let mut v = Vec::new();
        for k in 0..behind {
            let e = sf::RING + (old.wrapping_add(k) % sf::RING_LEN) as usize * 16;
            let r = unsafe { (self.s.get(e) as i32, self.s.get(e + 4) as i32, self.s.get(e + 8) as i32, self.s.get(e + 12) as i32) };
            v.push(r);
        }
        Some(v)
    }

    /// Draw the part of the surface under client rectangle `r` (client
    /// height `ch`, the surface's top left at the client's top left).
    pub fn paint(&self, hps: u32, r: &RectL, ch: i32) {
        let x0 = r.l.max(0) as usize;
        let x1 = (r.r.max(0) as usize).min(self.w);
        let y0 = (ch - r.t).max(0) as usize;
        let y1 = ((ch - r.b).max(0) as usize).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let (bw, bh) = (x1 - x0, y1 - y0);
        let stride = viocell::bgr24_stride(bw);
        let mut buf = vec![0u8; stride * bh];
        let px = unsafe { self.s.bytes(sf::PIXELS, self.w * self.h) };
        for y in 0..bh {
            let row = &px[(y0 + y) * self.w + x0..(y0 + y) * self.w + x1];
            let out = &mut buf[(bh - 1 - y) * stride..];
            for (x, &c) in row.iter().enumerate() {
                let v = self.pal[c as usize];
                out[x * 3] = v as u8;
                out[x * 3 + 1] = (v >> 8) as u8;
                out[x * 3 + 2] = (v >> 16) as u8;
            }
        }
        let mut bmi = [0u8; 12];
        bmi[0] = 12;
        bmi[4..6].copy_from_slice(&(bw as u16).to_le_bytes());
        bmi[6..8].copy_from_slice(&(bh as u16).to_le_bytes());
        bmi[8] = 1;
        bmi[10] = 24;
        let (l, b) = (x0 as i32, ch - y1 as i32);
        let pts = [PointL { x: l, y: b }, PointL { x: l + bw as i32 - 1, y: b + bh as i32 - 1 }, PointL { x: 0, y: 0 }];
        unsafe { crate::GpiDrawBits(hps, buf.as_ptr(), bmi.as_ptr(), 3, pts.as_ptr(), 0xcc, 0) };
    }

    /// Windows' pointer as a PM pointer (made again when it changed).
    pub fn pointer(&mut self, client: u32) -> u32 {
        let g = unsafe { self.s.get(sf::PTR_GEN) };
        if g == self.ptr_gen && self.hptr != 0 {
            return self.hptr;
        }
        self.ptr_gen = g;
        if self.hptr != 0 {
            unsafe { WinDestroyPointer(self.hptr) };
            self.hptr = 0;
        }
        let s = self.s;
        let (shown, w, h, wb) = unsafe { (s.get(sf::PTR_SHOWN) != 0, s.get(sf::PTR_W) as usize, s.get(sf::PTR_H) as usize, s.get(sf::PTR_WB) as usize) };
        let (hx, hy) = unsafe { (s.get(sf::PTR_HOTX) as i32, s.get(sf::PTR_HOTY) as i32) };
        let (w, h) = if shown && w > 0 && h > 0 && wb * h <= sf::PTR_MASK { (w, h) } else { (32, 32) };
        // A 1 bpp bitmap twice the pointer's height: the XOR mask below
        // the AND mask (rows bottom up); hidden: all transparent.
        let stride = w.div_ceil(32) * 4;
        let mut bits = vec![0u8; stride * h * 2];
        let (and, xor) = unsafe { (s.bytes(sf::PTR_AND, sf::PTR_MASK), s.bytes(sf::PTR_XOR, sf::PTR_MASK)) };
        for r in 0..h {
            let src = h - 1 - r;
            for c in 0..w.div_ceil(8) {
                let (a, x) = if shown { (and[src * wb + c], xor[src * wb + c]) } else { (0xff, 0) };
                bits[r * stride + c] = x;
                bits[(h + r) * stride + c] = a;
            }
        }
        let mut hdr = [0u8; 12];
        hdr[0] = 12;
        hdr[4..6].copy_from_slice(&(w as u16).to_le_bytes());
        hdr[6..8].copy_from_slice(&((h * 2) as u16).to_le_bytes());
        hdr[8] = 1;
        hdr[10] = 1;
        let mut bmi = [0u8; 18];
        bmi[..12].copy_from_slice(&hdr);
        bmi[15..18].copy_from_slice(&[0xff, 0xff, 0xff]);
        unsafe {
            let hps = WinGetPS(client);
            let hbm = GpiCreateBitmap(hps, hdr.as_ptr(), 4, bits.as_ptr(), bmi.as_ptr());
            WinReleasePS(hps);
            if hbm != 0 {
                self.hptr = WinCreatePointer(crate::HWND_DESKTOP, hbm, 1, hx, h as i32 - 1 - hy);
                GpiDeleteBitmap(hbm);
            }
            if self.hptr == 0 {
                return WinQuerySysPointer(crate::HWND_DESKTOP, 1, 0);
            }
        }
        self.hptr
    }

    /// Set the pointer while it is over the window.
    pub fn set_pointer(&mut self, client: u32) {
        let p = self.pointer(client);
        unsafe { WinSetPointer(crate::HWND_DESKTOP, p) };
    }
}

impl Drop for View {
    fn drop(&mut self) {
        if self.hptr != 0 {
            unsafe { WinDestroyPointer(self.hptr) };
        }
        unsafe { os2::DosFreeMem(self.s.0) };
    }
}
