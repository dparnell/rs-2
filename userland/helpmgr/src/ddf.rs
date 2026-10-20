//! Dynamic data formatting: the Ddf* functions an application calls to
//! build the text of a `:ddf.` (asked for with HM_QUERY_DDF_DATA when the
//! panel is laid out), as IBM's (`bhddf.c`): the buffer is cell text with
//! its words written out (see `ipf::text`), which the help manager lays
//! out in the panel and frees when it has read it.

use crate::pm::*;
use alloc::boxed::Box;
use alloc::vec::Vec;

pub const HMERR_DDF_MEMORY: u32 = 0x3001;
pub const HMERR_DDF_ALIGN_TYPE: u32 = 0x3002;
pub const HMERR_DDF_BACKCOLOR: u32 = 0x3003;
pub const HMERR_DDF_FORECOLOR: u32 = 0x3004;
pub const HMERR_DDF_FONTSTYLE: u32 = 0x3005;
pub const HMERR_DDF_REFTYPE: u32 = 0x3006;
pub const HMERR_DDF_LIST_UNCLOSED: u32 = 0x3007;
pub const HMERR_DDF_LIST_UNINITIALIZED: u32 = 0x3008;
pub const HMERR_DDF_LIST_BREAKTYPE: u32 = 0x3009;
pub const HMERR_DDF_LIST_SPACING: u32 = 0x300a;
pub const HMERR_DDF_HINSTANCE: u32 = 0x300b;
pub const HMERR_DDF_EXCEED_MAX_LENGTH: u32 = 0x300c;
pub const HMERR_DDF_EXCEED_MAX_INC: u32 = 0x300d;
pub const HMERR_DDF_INVALID_DDF: u32 = 0x300e;
pub const HMERR_DDF_FORMAT_TYPE: u32 = 0x300f;
pub const HMERR_DDF_INVALID_PARM: u32 = 0x3010;

/// The largest buffer (IBM's limit: one 60 KB segment).
const MAX: usize = 0xf000;

/// A DDF handle's data.
pub struct Ddf {
    pub buf: Vec<u8>,
    /// Formatted text (1) or as it is (0, after DdfSetFormat(FALSE)).
    format: u32,
    /// The alignment of text as it is: 1 left, 2 right, 4 centre.
    align: u8,
    /// The left margin a list writes.
    margin: u32,
    /// The list's term width (None: no list), break type, spacing and
    /// whether no item was written yet.
    list: Option<u32>,
    brk: u32,
    spacing: u32,
    first: bool,
}

struct Handles(core::cell::UnsafeCell<Vec<usize>>);
unsafe impl Sync for Handles {}
static HANDLES: Handles = Handles(core::cell::UnsafeCell::new(Vec::new()));

fn handles() -> &'static mut Vec<usize> {
    unsafe { &mut *HANDLES.0.get() }
}

/// The data of handle `h`, if it is one (IBM's IsHandleDdf).
pub fn get(h: u32) -> Option<&'static mut Ddf> {
    if h == 0 || !handles().contains(&(h as usize)) {
        return None;
    }
    Some(unsafe { &mut *(h as *mut Ddf) })
}

/// Free handle `h` (the help manager does once it has the text).
pub fn free(h: u32) {
    if get(h).is_some() {
        handles().retain(|&p| p != h as usize);
        unsafe { drop(Box::from_raw(h as *mut Ddf)) };
    }
}

fn fail(e: u32) -> u32 {
    crate::set_error(e);
    0
}

impl Ddf {
    fn room(&self, n: usize) -> bool {
        self.buf.len() + n <= MAX
    }

    fn put(&mut self, b: &[u8]) -> Result<(), u32> {
        if !self.room(b.len()) {
            return Err(HMERR_DDF_MEMORY);
        }
        self.buf.extend_from_slice(b);
        Ok(())
    }

    fn esc(&mut self, ty: u8, data: &[u8]) -> Result<(), u32> {
        let mut e = alloc::vec![0xff, data.len() as u8 + 2, ty];
        e.extend_from_slice(data);
        self.put(&e)
    }

    /// Text: words of letters and digits (other characters are words of
    /// their own), a blank as one space; a line end breaks the line in
    /// text as it is and is a blank in formatted text (DdfText).
    fn text(&mut self, t: &[u8]) -> Result<(), u32> {
        let start = self.buf.len();
        let mut i = 0;
        let r = (|| {
            while i < t.len() {
                let c = t[i];
                if c == b' ' || (c == b'\n' && self.format != 0) {
                    self.put(&[0xfe])?;
                    i += 1;
                } else if c == b'\n' {
                    self.put(&[0xfd])?;
                    i += 1;
                } else if c == b'\r' {
                    i += 1;
                } else {
                    let word = |c: u8| c.is_ascii_alphanumeric() || c >= 0x80;
                    let mut j = i + 1;
                    if word(c) {
                        while j < t.len() && word(t[j]) && j - i < 250 {
                            j += 1;
                        }
                    }
                    self.esc(0x66, &t[i..j])?;
                    i = j;
                }
            }
            Ok(())
        })();
        if r.is_err() {
            self.buf.truncate(start);
        }
        r
    }

    fn margin(&mut self, ty: u8) -> Result<(), u32> {
        let m = self.margin as u8;
        self.esc(ty, &[m])
    }
}

fn str_arg<'a>(p: *const u8) -> Option<&'a [u8]> {
    if p.is_null() { None } else { Some(unsafe { cstr(p) }) }
}

/// DdfInitialize(hwndHelpInstance, cbBuffer, ulIncrement): a new DDF
/// for the help instance.
#[unsafe(no_mangle)]
pub extern "C" fn DdfInitialize(help: u32, cb: u32, inc: u32) -> u32 {
    if crate::instance(help).is_none() {
        return fail(HMERR_DDF_HINSTANCE);
    }
    if cb > MAX as u32 {
        return fail(HMERR_DDF_EXCEED_MAX_LENGTH);
    }
    if inc > MAX as u32 {
        return fail(HMERR_DDF_EXCEED_MAX_INC);
    }
    let d = Box::new(Ddf {
        buf: Vec::with_capacity((cb as usize).max(256)),
        format: 1,
        align: 1,
        margin: 0,
        list: None,
        brk: 1,
        spacing: 1,
        first: true,
    });
    let h = Box::into_raw(d) as usize;
    handles().push(h);
    h as u32
}

/// Run `f` on the data of `h`; FALSE (and the error) when it fails.
fn with(h: u32, f: impl FnOnce(&mut Ddf) -> Result<(), u32>) -> u32 {
    let Some(d) = get(h) else { return fail(HMERR_DDF_INVALID_DDF) };
    match f(d) {
        Ok(()) => 1,
        Err(e) => fail(e),
    }
}

/// DdfPara: a new paragraph.
#[unsafe(no_mangle)]
pub extern "C" fn DdfPara(h: u32) -> u32 {
    with(h, |d| d.put(&[0xfa]))
}

/// DdfSetFormat(hddf, fFormatType): TRUE formatted text, FALSE text as
/// it is (lines, in the DdfSetTextAlign alignment).
#[unsafe(no_mangle)]
pub extern "C" fn DdfSetFormat(h: u32, f: u32) -> u32 {
    with(h, |d| {
        if f > 1 {
            return Err(HMERR_DDF_FORMAT_TYPE);
        }
        if d.format == f {
            return Ok(());
        }
        if f == 0 {
            let a = d.align;
            d.esc(0x1a, &[a])?;
        } else {
            d.esc(0x1b, &[])?;
        }
        d.format = f;
        Ok(())
    })
}

/// DdfSetTextAlign(hddf, fAlign): TA_LEFT (2), TA_CENTER (3) or TA_RIGHT
/// (4), for text as it is from the next DdfSetFormat(FALSE).
#[unsafe(no_mangle)]
pub extern "C" fn DdfSetTextAlign(h: u32, a: u32) -> u32 {
    with(h, |d| {
        d.align = match a {
            2 => 1,
            4 => 2,
            3 => 4,
            _ => return Err(HMERR_DDF_ALIGN_TYPE),
        };
        Ok(())
    })
}

/// A PM colour as a help file colour index (IBM's DdfSetColor):
/// CLR_DEFAULT the window's, CLR_BLACK 14, CLR_BROWN 7.
fn colour_index(c: i32) -> i32 {
    match c {
        -3 => 0,
        -1 => 14,
        14 => 7,
        c => c,
    }
}

const CLR_UNCHANGED: i32 = -6;

/// DdfSetColor(hddf, fBackColor, fForColor); CLR_UNCHANGED keeps one.
#[unsafe(no_mangle)]
pub extern "C" fn DdfSetColor(h: u32, back: i32, fore: i32) -> u32 {
    with(h, |d| {
        if back == 0 {
            return Err(HMERR_DDF_BACKCOLOR);
        }
        if fore == 0 {
            return Err(HMERR_DDF_FORECOLOR);
        }
        let (b, f) = (colour_index(back), colour_index(fore));
        if b > 15 && b != CLR_UNCHANGED {
            return Err(HMERR_DDF_BACKCOLOR);
        }
        if f > 15 && f != CLR_UNCHANGED {
            return Err(HMERR_DDF_FORECOLOR);
        }
        if b != CLR_UNCHANGED {
            d.esc(0x14, &[b as u8])?;
        }
        if f != CLR_UNCHANGED {
            d.esc(0x13, &[f as u8])?;
        }
        Ok(())
    })
}

/// DdfInform(hddf, pszText, resInformNumber): text that sends HM_INFORM
/// with the number when chosen.
#[unsafe(no_mangle)]
pub extern "C" fn DdfInform(h: u32, text: *const u8, res: u32) -> u32 {
    with(h, |d| {
        let Some(t) = str_arg(text) else { return Err(HMERR_DDF_INVALID_PARM) };
        if res == 0 || res > 64000 {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        let start = d.buf.len();
        let r = d.esc(0x16, &(res as u16).to_le_bytes()).and_then(|_| d.text(t)).and_then(|_| d.esc(0x08, &[]));
        if r.is_err() {
            d.buf.truncate(start);
        }
        r
    })
}

/// DdfSetFontStyle(hddf, fFontStyle): FATTR_SEL_ITALIC (1),
/// FATTR_SEL_UNDERSCORE (2), FATTR_SEL_BOLD (0x20), or 0 for plain.
#[unsafe(no_mangle)]
pub extern "C" fn DdfSetFontStyle(h: u32, st: u32) -> u32 {
    with(h, |d| {
        if st != 0 && st & 0x23 == 0 {
            return Err(HMERR_DDF_FONTSTYLE);
        }
        let mut s = (st & 1) as u8;
        if st & 0x20 != 0 {
            s |= 2;
        }
        if st & 2 != 0 {
            s |= 4;
        }
        d.esc(0x04, &[s])
    })
}

/// DdfHyperText(hddf, pszText, pszReference, fReferenceType): text
/// linking to a panel by name (REFERENCE_BY_ID, 0) or resource number
/// (REFERENCE_BY_RES, 1, the number in decimal).
#[unsafe(no_mangle)]
pub extern "C" fn DdfHyperText(h: u32, text: *const u8, reference: *const u8, ty: u32) -> u32 {
    with(h, |d| {
        let t = str_arg(text).unwrap_or(b"");
        let r = str_arg(reference).unwrap_or(b"");
        let start = d.buf.len();
        let link = match ty {
            0 => d.esc(0x65, r),
            1 => {
                let n = r.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |a, &c| a.wrapping_mul(10).wrapping_add((c - b'0') as u32));
                d.esc(0x1d, &(n as u16).to_le_bytes())
            }
            _ => return Err(HMERR_DDF_REFTYPE),
        };
        let res = link.and_then(|_| d.text(t)).and_then(|_| d.esc(0x08, &[]));
        if res.is_err() {
            d.buf.truncate(start);
        }
        res
    })
}

/// DdfBeginList(hddf, ulWidthDT, fBreakType, fSpacing): a definition
/// list with terms `ulWidthDT` characters wide; HMBT_NONE (1), HMBT_ALL
/// (2) or HMBT_FIT (3); HMLS_SINGLELINE (1) or HMLS_DOUBLELINE (2).
#[unsafe(no_mangle)]
pub extern "C" fn DdfBeginList(h: u32, width: u32, brk: u32, spacing: u32) -> u32 {
    with(h, |d| {
        if d.list.is_some() {
            return Err(HMERR_DDF_LIST_UNCLOSED);
        }
        if width == 0 || width > 64000 {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        if !(1..=3).contains(&brk) {
            return Err(HMERR_DDF_LIST_BREAKTYPE);
        }
        if !(1..=2).contains(&spacing) {
            return Err(HMERR_DDF_LIST_SPACING);
        }
        d.list = Some(width);
        d.first = true;
        d.brk = brk;
        d.spacing = spacing;
        Ok(())
    })
}

/// DdfListItem(hddf, pszTerm, pszDescription): an item of the list.
#[unsafe(no_mangle)]
pub extern "C" fn DdfListItem(h: u32, term: *const u8, desc: *const u8) -> u32 {
    with(h, |d| {
        let Some(width) = d.list else { return Err(HMERR_DDF_LIST_UNINITIALIZED) };
        let (term, desc) = (str_arg(term), str_arg(desc));
        if term.is_none() && desc.is_none() {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        d.margin = 1;
        d.margin(0x02)?;
        if d.first || d.spacing != 1 {
            d.put(&[0xfa])?;
        } else {
            d.put(&[0xfd])?;
        }
        d.first = false;
        if let Some(t) = term {
            d.text(t)?;
        }
        d.margin += width;
        // HMBT_ALL: the description on a line of its own; HMBT_FIT: when
        // the term is wider; HMBT_NONE: after the term.
        let ty = match d.brk {
            2 => 0x11,
            3 => 0x12,
            _ => 0x22,
        };
        d.margin(ty)?;
        if let Some(t) = desc {
            d.text(t)?;
        }
        d.margin -= width;
        Ok(())
    })
}

/// DdfEndList: the end of the list.
#[unsafe(no_mangle)]
pub extern "C" fn DdfEndList(h: u32) -> u32 {
    with(h, |d| {
        if d.list.is_none() {
            return Err(HMERR_DDF_LIST_UNINITIALIZED);
        }
        d.margin = 1;
        d.margin(0x02)?;
        d.put(&[0xfd])?;
        d.list = None;
        Ok(())
    })
}

/// DdfText(hddf, pszText): text.
#[unsafe(no_mangle)]
pub extern "C" fn DdfText(h: u32, text: *const u8) -> u32 {
    with(h, |d| match str_arg(text) {
        Some(t) => d.text(t),
        None => Err(HMERR_DDF_INVALID_PARM),
    })
}

/// DdfMetafile(hddf, hmf, prclRect): a metafile, in the rectangle (NULL:
/// the window).
#[unsafe(no_mangle)]
pub extern "C" fn DdfMetafile(h: u32, hmf: u32, rect: *const RectL) -> u32 {
    with(h, |d| {
        if hmf == 0 {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        let r = if rect.is_null() { RectL::default() } else { unsafe { rect.read_unaligned() } };
        if !d.room(23) {
            return Err(HMERR_DDF_MEMORY);
        }
        // As IBM's: the length byte says 0x14 for 22 bytes.
        d.buf.extend_from_slice(&[0xff, 0x14, 0x69]);
        for v in [r.l, r.b, r.r, r.t] {
            d.buf.extend_from_slice(&v.to_le_bytes());
        }
        d.buf.extend_from_slice(&hmf.to_le_bytes());
        Ok(())
    })
}

/// DdfSetFont(hddf, pszFaceName, ulWidth, ulHeight): a font by face
/// name and point size (NULL or "": the default font).
#[unsafe(no_mangle)]
pub extern "C" fn DdfSetFont(h: u32, face: *const u8, w: u32, ht: u32) -> u32 {
    with(h, |d| {
        if w == 0 || ht == 0 || w > 64000 || ht > 64000 {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        let f = match str_arg(face) {
            None | Some(b"") => &b"default"[..],
            Some(f) => &f[..f.len().min(32)],
        };
        let mut data = Vec::with_capacity(5 + f.len());
        data.extend_from_slice(&(w as u16).to_le_bytes());
        data.extend_from_slice(&(ht as u16).to_le_bytes());
        data.push(f.len() as u8);
        data.extend_from_slice(f);
        d.esc(0x67, &data)
    })
}

/// DdfBitmap(hddf, hbm, fAlign): a bitmap; ART_LEFT (1), ART_RIGHT (2),
/// ART_CENTER (4) or ART_RUNIN (0x10).
#[unsafe(no_mangle)]
pub extern "C" fn DdfBitmap(h: u32, hbm: u32, align: u32) -> u32 {
    with(h, |d| {
        let mut bmp = [0u8; 12];
        bmp[0] = 12;
        if unsafe { GpiQueryBitmapParameters(hbm, bmp.as_mut_ptr()) } == 0 {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        if ![0x10, 1, 2, 4].contains(&align) {
            return Err(HMERR_DDF_INVALID_PARM);
        }
        let mut data = hbm.to_le_bytes().to_vec();
        data.push(align as u8);
        d.esc(0x68, &data)
    })
}
