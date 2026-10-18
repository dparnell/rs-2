//! Static controls (WC_STATIC): text with DT_* layout flags, group boxes,
//! rectangles and frames, icons and bitmaps.

use gpi::Rect;

use crate::ctl::{self, *};
use crate::draw;
use crate::lock::wndproc;
use crate::msg::WinDefWindowProc;
use crate::win::WM_PAINT;

const SS_TEXT: u32 = 0x01;
const SS_GROUPBOX: u32 = 0x02;
const SS_ICON: u32 = 0x03;
const SS_BITMAP: u32 = 0x04;
const SS_FGNDRECT: u32 = 0x05;
const SS_HALFTONERECT: u32 = 0x06;
const SS_BKGNDRECT: u32 = 0x07;
const SS_FGNDFRAME: u32 = 0x08;
const SS_HALFTONEFRAME: u32 = 0x09;
const SS_BKGNDFRAME: u32 = 0x0a;
const SS_SYSICON: u32 = 0x0b;
const SS_AUTOSIZE: u32 = 0x40;

const WM_CREATE: u32 = 0x0001;
const SM_SETHANDLE: u32 = 0x0100;
const SM_QUERYHANDLE: u32 = 0x0101;

wndproc! {
    static_entry => static_proc;
}

fn paint(h: u32) {
    let style = ctl::style(h);
    let kind = style & 0x7f;
    let text = ctl::text(h);
    let enabled = ctl::enabled(h);
    let bmp = if kind == SS_BITMAP { crate::bitmap::for_static(h) } else { None };
    draw::paint(h, |c| {
        let r = c.rect();
        let bg = draw::dialog_bg();
        match kind {
            SS_TEXT => {
                c.fill(r, bg);
                let fl = style & 0xff00;
                let fg = if !enabled || fl & draw::DT_HALFTONE != 0 { draw::GRAY_TEXT } else { draw::TEXT };
                c.text_in(r, &text, fg, fl);
            }
            SS_GROUPBOX => {
                c.fill(r, bg);
                let f = draw::font();
                let top = r.top - f.height / 2;
                let b = Rect::new(r.left, r.bottom, r.right, top);
                c.frame(Rect::new(b.left + 1, b.bottom, b.right, b.top - 1), draw::LIGHT);
                c.frame(Rect::new(b.left, b.bottom + 1, b.right - 1, b.top), draw::SHADOW);
                if !text.is_empty() {
                    let w = draw::text_width(&draw::strip_mnemonic(&text).0);
                    let t = Rect::new(r.left + 6, r.top - f.height, (r.left + 10 + w).min(r.right), r.top);
                    c.fill(t, bg);
                    c.text_in(Rect::new(t.left + 2, t.bottom, t.right, t.top), &text, if enabled { draw::TEXT } else { draw::GRAY_TEXT }, draw::DT_MNEMONIC);
                }
            }
            SS_FGNDRECT => c.fill(r, draw::TEXT),
            SS_HALFTONERECT => c.fill(r, draw::SHADOW),
            SS_BKGNDRECT => c.fill(r, bg),
            SS_FGNDFRAME => c.frame(r, draw::TEXT),
            SS_HALFTONEFRAME => c.frame(r, draw::SHADOW),
            SS_BKGNDFRAME => c.frame(r, bg),
            SS_ICON | SS_SYSICON | SS_BITMAP => {
                c.fill(r, bg);
                if let Some(b) = &bmp {
                    // Stretched to the control (SS_AUTOSIZE sized the
                    // control to the bitmap).
                    crate::bitmap::blit(c, b, r.width(), r.height());
                }
            }
            _ => c.fill(r, bg),
        }
    });
}

pub extern "C" fn static_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint(h);
            0
        }
        WM_CREATE => {
            // SS_AUTOSIZE: a bitmap static takes the bitmap's size,
            // keeping its top edge and horizontal centre.
            if ctl::style(h) & 0x7f == SS_BITMAP && ctl::style(h) & SS_AUTOSIZE != 0 {
                if let Some(b) = crate::bitmap::for_static(h) {
                    if let Some(w) = crate::state::st().desk.get(h) {
                        let (x, y) = (w.x + w.cx / 2 - b.w / 2, w.y + w.cy - b.h);
                        crate::state::st().desk.set_pos(h, x, y, b.w, b.h);
                    }
                }
            }
            0
        }
        WM_QUERYDLGCODE => DLGC_STATIC,
        WM_MATCHMNEMONIC => {
            let s = ctl::style(h) & 0x7f;
            (matches!(s, SS_TEXT | SS_GROUPBOX) && draw::mnemonic(&ctl::text(h)) == Some((mp1 as u8).to_ascii_lowercase())) as u32
        }
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        SM_SETHANDLE => {
            ctl::set_data::<u32>(h, mp1);
            ctl::invalidate(h);
            1
        }
        SM_QUERYHANDLE => ctl::data::<u32>(h).map_or(0, |v| *v),
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
