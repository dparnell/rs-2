//! Drawing a page of the picture fitted into a rectangle of a device: the
//! PS is given the page's size in arbitrary units (PU_ARBITRARY) and a
//! page viewport that maps the page, or the part of it the picture
//! covers, onto the rectangle with its proportions kept, then the
//! metafile is played without resetting the PS.

use pmkit::*;

use crate::doc::Doc;

unsafe extern "C" {
    #[link_name = "PMGPI.521"]
    fn GpiSetDrawControl(hps: u32, ctl: i32, value: i32) -> u32;
    #[link_name = "PMGPI.497"]
    fn GpiResetBoundaryData(hps: u32) -> u32;
    #[link_name = "PMGPI.428"]
    fn GpiQueryBoundaryData(hps: u32, r: *mut RectL) -> u32;
}

const DCTL_DISPLAY: i32 = 2;
const DCTL_BOUNDARY: i32 = 3;
const DCTL_OFF: i32 = 0;
const DCTL_ON: i32 = 1;

/// The page's size for drawing: the metafile's, else a square that holds
/// most pictures.
pub fn page_size(doc: &Doc) -> (i32, i32) {
    let (w, h) = doc.size;
    if w > 0 && h > 0 { (w, h) } else { (32000, 32000) }
}

/// Play the page's metafile on `hps` (set up already).
pub fn play(hps: u32, hmf: u32, suppress: bool) -> i32 {
    let mut opts = [0i32; 10];
    opts[PMF_LOADTYPE] = LT_NOMODIFY;
    opts[PMF_LCIDS] = LC_LOADDISC;
    opts[PMF_RESET] = RES_NORESET;
    opts[PMF_SUPPRESS] = if suppress { SUP_SUPPRESS } else { SUP_NOSUPPRESS };
    opts[PMF_COLORTABLES] = CTAB_REPLACE;
    opts[PMF_DEFAULTS] = DDEF_LOADDISC;
    let mut segs = 0;
    unsafe { GpiPlayMetaFile(hps, hmf, 10, opts.as_ptr(), &mut segs, 0, core::ptr::null_mut()) }
}

/// Give the PS the page's arbitrary units and a viewport.
fn set_page(hps: u32, doc: &Doc, vp: &RectL) {
    let (w, h) = page_size(doc);
    unsafe {
        GpiSetPS(hps, &SizeL { cx: w, cy: h }, PU_ARBITRARY | GPIF_DEFAULT);
        GpiSetPageViewport(hps, vp);
    }
    // The metafile's default viewing transform (GpiPlayMetaFile applies
    // it only when it resets the PS, which would undo the page set up).
    let m = doc.view.unwrap_or([0x10000, 0, 0, 0x10000, 0, 0]);
    let lf = [m[0], m[1], 0, m[2], m[3], 0, m[4], m[5], 1];
    unsafe { GpiSetDefaultViewMatrix(hps, 9, lf.as_ptr(), 0) };
}

/// The part of the page the picture covers (page units), found by
/// playing it once with boundary data collected and nothing drawn.
pub fn bounds(hps: u32, doc: &mut Doc) -> Option<RectL> {
    if let Some(b) = doc.bounds.get(doc.page).copied().flatten() {
        return Some(b);
    }
    let (w, h) = page_size(doc);
    set_page(hps, doc, &RectL::new(0, 0, w, h));
    let mut r = RectL::default();
    let ok = unsafe {
        GpiSetDrawControl(hps, DCTL_DISPLAY, DCTL_OFF);
        GpiSetDrawControl(hps, DCTL_BOUNDARY, DCTL_ON);
        GpiResetBoundaryData(hps);
        play(hps, doc.hmf(), false);
        let ok = GpiQueryBoundaryData(hps, &mut r) != 0;
        GpiSetDrawControl(hps, DCTL_BOUNDARY, DCTL_OFF);
        GpiSetDrawControl(hps, DCTL_DISPLAY, DCTL_ON);
        ok
    };
    // Boundary data is in model space: the default viewing transform
    // moves it onto the page.
    let b = (ok && r.r > r.l && r.t > r.b).then(|| transform(doc.view, RectL::new(r.l, r.b, r.r + 1, r.t + 1)));
    if let Some(slot) = doc.bounds.get_mut(doc.page) {
        *slot = b;
    }
    b
}

/// The page viewport that shows `area` (page units) as large as fits in
/// `into` (device units), centred, its proportions kept.
pub fn fit(doc: &Doc, area: RectL, into: RectL) -> RectL {
    let (w, h) = page_size(doc);
    let (aw, ah) = (area.width().max(1) as i64, area.height().max(1) as i64);
    let (iw, ih) = (into.width().max(1) as i64, into.height().max(1) as i64);
    // Scale = min(iw/aw, ih/ah), kept as a fraction num/den.
    let (num, den) = if iw * ah <= ih * aw { (iw, aw) } else { (ih, ah) };
    let sx = |v: i64| (v * num / den) as i32;
    let (dw, dh) = (sx(aw), sx(ah));
    let x0 = into.l + (into.width() - dw) / 2;
    let y0 = into.b + (into.height() - dh) / 2;
    // The whole page's rectangle on the device.
    let l = x0 - sx(area.l as i64);
    let b = y0 - sx(area.b as i64);
    RectL::new(l, b, l + sx(w as i64), b + sx(h as i64))
}

/// Draw the page fitted into `into`: the picture (`fit_picture`) or the
/// whole page.
pub fn draw(hps: u32, doc: &mut Doc, into: RectL, fit_picture: bool) {
    let (w, h) = page_size(doc);
    let page = RectL::new(0, 0, w, h);
    let area = if fit_picture || doc.size.0 <= 0 || doc.size.1 <= 0 { bounds(hps, doc).unwrap_or(page) } else { page };
    let vp = fit(doc, area, into);
    set_page(hps, doc, &vp);
    play(hps, doc.hmf(), false);
}

/// A rectangle through a transform (the box around its corners).
fn transform(m: Option<[i32; 6]>, r: RectL) -> RectL {
    let Some(m) = m else { return r };
    let f = |x: i32, y: i32| {
        let px = ((x as i64 * m[0] as i64 + y as i64 * m[2] as i64) >> 16) + m[4] as i64;
        let py = ((x as i64 * m[1] as i64 + y as i64 * m[3] as i64) >> 16) + m[5] as i64;
        (px as i32, py as i32)
    };
    let c = [f(r.l, r.b), f(r.r, r.b), f(r.l, r.t), f(r.r, r.t)];
    RectL::new(c.iter().map(|p| p.0).min().unwrap(), c.iter().map(|p| p.1).min().unwrap(), c.iter().map(|p| p.0).max().unwrap(), c.iter().map(|p| p.1).max().unwrap())
}
