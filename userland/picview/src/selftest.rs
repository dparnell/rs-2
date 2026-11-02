//! `PICVIEW /SELFTEST`: checks of the page splitting, the fitting, the
//! metafiles made for pages, the clipboard and saving, printed as
//! PASS/FAIL lines and "picview selftest done: N failure(s)".

use alloc::vec::Vec;

use ::gpi::Point;
use ::gpi::goca::{self, Order};
use ::gpi::metafile::{self, Metafile};
use pmkit::*;

use crate::{doc, view};

struct T {
    fails: u32,
}

impl T {
    fn check(&mut self, name: &str, ok: bool) {
        if !ok {
            self.fails += 1;
        }
        crate::trace(alloc::format!("{} {name}", if ok { "PASS" } else { "FAIL" }).as_bytes());
    }
}

fn newframe() -> Order {
    let mut d = 16300i32.to_le_bytes().to_vec();
    d.extend_from_slice(&[0, 0, 0, 0]);
    Order::Escape { ident: 0x80, data: d }
}

fn line(x: i32, y: i32) -> Order {
    Order::Line { at: Some(Point { x: 0, y: 0 }), pts: alloc::vec![Point { x, y }] }
}

/// A metafile of `n` pages, each a line to (100k, 50k).
fn pages_metafile(n: usize) -> Metafile {
    let mut orders: Vec<Order> = alloc::vec![Order::BeginSegment { id: 1, flags: 0, len: 0 }];
    for k in 0..n {
        orders.push(line(100 * (k as i32 + 1), 50 * (k as i32 + 1)));
        // As a printer's metafile: a NEWFRAME after every page.
        orders.push(newframe());
    }
    Metafile { units: PU_PELS, page: (400, 300), long: true, data: goca::encode_all(&orders), ..Metafile::default() }
}

pub fn run(hab: u32) -> u32 {
    let mut t = T { fails: 0 };

    // Splitting at NEWFRAME; the empty page after the last NEWFRAME goes.
    let m = pages_metafile(3);
    let pages = doc::split_pages(&m);
    t.check("three pages", pages.len() == 3);
    t.check("later pages start a segment", pages.iter().skip(1).all(|p| matches!(p.first(), Some(Order::BeginSegment { .. }))));
    t.check("one page without NEWFRAME", doc::split_pages(&pages_metafile(1)).len() == 1);

    // Each page becomes a metafile handle with its own orders.
    match doc::from_metafile(hab, b"", m.clone(), None) {
        Ok(d) => {
            t.check("page handles", d.pages.len() == 3 && d.pages.iter().all(|&h| h != 0));
            let n = unsafe { GpiQueryMetaFileLength(d.pages[1]) };
            let mut b = alloc::vec![0u8; n.max(0) as usize];
            unsafe { GpiQueryMetaFileBits(d.pages[1], 0, n, b.as_mut_ptr()) };
            let p = metafile::parse(&b);
            let has = p.as_ref().is_some_and(|p| goca::decode(&p.data, p.long).iter().any(|o| matches!(o, Order::Line { pts, .. } if pts.first() == Some(&Point { x: 200, y: 100 }))));
            t.check("page 2's metafile has page 2's line", has);
            t.check("page size kept", p.as_ref().is_some_and(|p| p.page == (400, 300)));
            // Its picture's bounds, found by playing it.
            let mut d = d;
            d.page = 1;
            let hps = info_ps(hab);
            let b = view::bounds(hps, &mut d);
            crate::trace(alloc::format!("bounds {:?}", b).as_bytes());
            t.check("bounds of page 2", b.is_some_and(|r| r.l <= 0 && r.b <= 0 && r.r >= 200 && r.t >= 100 && r.r <= 202 && r.t <= 102));
            unsafe { GpiDestroyPS(hps) };
        }
        Err(_) => t.check("page handles", false),
    }

    // A metafile with nothing drawn is refused.
    let empty = Metafile { data: goca::encode_all(&[Order::BeginSegment { id: 1, flags: 0, len: 0 }]), ..Metafile::default() };
    t.check("empty metafile refused", matches!(doc::from_metafile(hab, b"", empty, None), Err(doc::Error::Empty)));

    // Fitting keeps the proportions and centres.
    let d = doc::from_metafile(hab, b"", pages_metafile(1), None).ok();
    if let Some(d) = &d {
        let vp = view::fit(d, RectL::new(0, 0, 400, 300), RectL::new(0, 0, 800, 800));
        t.check("fit page: width", vp == RectL::new(0, 100, 800, 700));
        let vp = view::fit(d, RectL::new(100, 100, 200, 150), RectL::new(0, 0, 400, 400));
        // The picture (100x50) scaled 4 times, centred vertically.
        t.check("fit picture", vp == RectL::new(-400, -300, 1200, 900));
    }

    // The CD's picture, when the image has it.
    let aaaaa = b"C:\\OS2\\BITMAP\\AAAAA.MET";
    if read_file(aaaaa).is_some() {
        match doc::open(hab, aaaaa) {
            Ok(mut d) => {
                t.check("AAAAA.MET: one page", d.pages.len() == 1);
                let hps = info_ps(hab);
                let b = view::bounds(hps, &mut d);
                crate::trace(alloc::format!("AAAAA.MET size {:?} units {} view {:?} bounds {:?}", d.size, d.units, d.view, b).as_bytes());
                t.check("AAAAA.MET: bounds on the page", b.is_some_and(|b| b.l >= -50 && b.b >= -50 && b.r <= d.size.0 + 50 && b.t <= d.size.1 + 50));
                unsafe { GpiDestroyPS(hps) };
            }
            Err(_) => t.check("AAAAA.MET opens", false),
        }
    }

    // Not a metafile.
    let _ = write_file(b"C:\\PVTEST.TXT", b"hello");
    t.check("text file refused", matches!(doc::open(hab, b"C:\\PVTEST.TXT"), Err(doc::Error::NotMetafile)));
    t.check("missing file", matches!(doc::open(hab, b"C:\\NOFILE.MET"), Err(doc::Error::Open)));
    unsafe { os2::DosDelete(b"C:\\PVTEST.TXT\0".as_ptr()) };

    // Save a page, open it again.
    if let Some(d) = &d {
        let copy = unsafe { GpiCopyMetaFile(d.hmf()) };
        let saved = copy != 0 && unsafe { GpiSaveMetaFile(copy, b"C:\\PVTEST.MET\0".as_ptr()) } != 0;
        t.check("save", saved);
        let back = doc::open(hab, b"C:\\PVTEST.MET");
        t.check("open saved", back.as_ref().is_ok_and(|b| b.pages.len() == 1 && b.size == (400, 300)));
        unsafe { os2::DosDelete(b"C:\\PVTEST.MET\0".as_ptr()) };

        // The clipboard: Copy puts a CF_METAFILE there, Paste takes it.
        let c = unsafe { GpiCopyMetaFile(d.hmf()) };
        let ok = unsafe { WinOpenClipbrd(hab) } != 0;
        if ok {
            unsafe {
                WinEmptyClipbrd(hab);
                WinSetClipbrdData(hab, c, CF_METAFILE, CFI_HANDLE);
                WinCloseClipbrd(hab);
            }
        }
        let mut pasted = None;
        if unsafe { WinOpenClipbrd(hab) } != 0 {
            let data = unsafe { WinQueryClipbrdData(hab, CF_METAFILE) };
            let h = if data != 0 { unsafe { GpiCopyMetaFile(data) } } else { 0 };
            unsafe { WinCloseClipbrd(hab) };
            if h != 0 {
                pasted = doc::from_handle(hab, h).ok();
            }
        }
        t.check("clipboard round trip", pasted.is_some_and(|p| p.pages.len() == 1 && p.size == (400, 300)));
    }

    // A spool file of three pages for trying the Page menu (kept).
    let spl = metafile::build(&demo_pages());
    t.check("spool file written", write_file(b"C:\\PVPAGES.SPL", &spl).is_ok());
    t.check("spool file has three pages", doc::open(hab, b"C:\\PVPAGES.SPL").is_ok_and(|d| d.pages.len() == 3));

    crate::trace(alloc::format!("picview selftest done: {} failure(s)", t.fails).as_bytes());
    t.fails
}

/// A normal PS on an information DC of the display (nothing is drawn).
fn info_ps(hab: u32) -> u32 {
    let dop: [u32; 9] = [0; 9];
    let hdc = unsafe { DevOpenDC(hab, OD_MEMORY, b"*\0".as_ptr(), 0, dop.as_ptr(), 0) };
    unsafe { GpiCreatePS(hab, hdc, &SizeL { cx: 0, cy: 0 }, PU_PELS | GPIF_DEFAULT | GPIT_NORMAL | GPIA_ASSOC) }
}

/// Three pages, each a frame, a filled box and its number.
fn demo_pages() -> Metafile {
    let mut orders: Vec<Order> = alloc::vec![Order::BeginSegment { id: 1, flags: 0, len: 0 }];
    let p = |x, y| Point { x, y };
    for k in 0..3i32 {
        orders.push(Order::Set(goca::Attr::Color(1 + k)));
        orders.push(Order::Box { at: Some(p(20, 20)), flags: 0x20, corner: p(380, 280), h: 0, v: 0 });
        orders.push(Order::Box { at: Some(p(60 + 60 * k, 80)), flags: 0x40, corner: p(160 + 60 * k, 200), h: 0, v: 0 });
        orders.push(Order::CharString { at: Some(p(40, 240)), moving: false, text: alloc::format!("Page {}", k + 1).into_bytes() });
        orders.push(newframe());
    }
    Metafile { units: PU_PELS, page: (400, 300), long: true, data: goca::encode_all(&orders), ..Metafile::default() }
}
