//! GPI test program. Runs self-checks on memory bitmaps (each prints
//! PASS or FAIL; the exit code is the number of failures), then opens a
//! window showing the primitives, areas, paths, regions, bitmaps,
//! transforms and text, for a visual check. Close it to exit.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

extern crate alloc;

os2::entry!(main);

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct PointL {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct RectL {
    l: i32,
    b: i32,
    r: i32,
    t: i32,
}

#[repr(C)]
#[derive(Default)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.829"]
    fn WinQuerySysValue(desk: u32, i: i32) -> i32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.794"]
    fn WinOpenWindowDC(h: u32) -> u32;
    #[link_name = "PMWIN.730"]
    fn WinDrawBitmap(hps: u32, hbm: u32, src: *const RectL, dst: *const PointL, fore: i32, back: i32, fl: u32) -> u32;

    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const PointL, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.504"]
    fn GpiSetBackColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.404"]
    fn GpiMove(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.398"]
    fn GpiLine(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.415"]
    fn GpiPolyLine(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.356"]
    fn GpiBox(hps: u32, ctl: i32, p: *const PointL, h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.393"]
    fn GpiFullArc(hps: u32, ctl: i32, mult: i32) -> i32;
    #[link_name = "PMGPI.612"]
    fn GpiPartialArc(hps: u32, c: *const PointL, mult: i32, start: i32, sweep: i32) -> i32;
    #[link_name = "PMGPI.412"]
    fn GpiPointArc(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.502"]
    fn GpiSetArcParams(hps: u32, p: *const [i32; 4]) -> u32;
    #[link_name = "PMGPI.417"]
    fn GpiPolySpline(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.413"]
    fn GpiPolyFillet(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.530"]
    fn GpiSetLineType(hps: u32, t: i32) -> u32;
    #[link_name = "PMGPI.531"]
    fn GpiSetLineWidth(hps: u32, w: i32) -> u32;
    #[link_name = "PMGPI.532"]
    fn GpiSetLineWidthGeom(hps: u32, w: i32) -> u32;
    #[link_name = "PMGPI.416"]
    fn GpiPolyMarker(hps: u32, n: i32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.533"]
    fn GpiSetMarker(hps: u32, s: i32) -> u32;
    #[link_name = "PMGPI.352"]
    fn GpiBeginArea(hps: u32, fl: u32) -> u32;
    #[link_name = "PMGPI.385"]
    fn GpiEndArea(hps: u32) -> i32;
    #[link_name = "PMGPI.354"]
    fn GpiBeginPath(hps: u32, id: i32) -> u32;
    #[link_name = "PMGPI.387"]
    fn GpiEndPath(hps: u32) -> u32;
    #[link_name = "PMGPI.392"]
    fn GpiFillPath(hps: u32, id: i32, opts: i32) -> i32;
    #[link_name = "PMGPI.554"]
    fn GpiStrokePath(hps: u32, id: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.515"]
    fn GpiSetClipPath(hps: u32, id: i32, opts: i32) -> u32;
    #[link_name = "PMGPI.541"]
    fn GpiSetPattern(hps: u32, p: i32) -> u32;
    #[link_name = "PMGPI.370"]
    fn GpiCreateRegion(hps: u32, n: i32, r: *const RectL) -> u32;
    #[link_name = "PMGPI.611"]
    fn GpiDestroyRegion(hps: u32, h: u32) -> u32;
    #[link_name = "PMGPI.362"]
    fn GpiCombineRegion(hps: u32, d: u32, a: u32, b: u32, mode: i32) -> i32;
    #[link_name = "PMGPI.409"]
    fn GpiPaintRegion(hps: u32, h: u32) -> i32;
    #[link_name = "PMGPI.516"]
    fn GpiSetClipRegion(hps: u32, h: u32, old: *mut u32) -> i32;
    #[link_name = "PMGPI.481"]
    fn GpiQueryRegionBox(hps: u32, h: u32, r: *mut RectL) -> i32;
    #[link_name = "PMGPI.419"]
    fn GpiPtInRegion(hps: u32, h: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.407"]
    fn GpiOffsetRegion(hps: u32, h: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, init: *const u8, bmi: *const u8) -> u32;
    #[link_name = "PMGPI.371"]
    fn GpiDeleteBitmap(h: u32) -> u32;
    #[link_name = "PMGPI.506"]
    fn GpiSetBitmap(hps: u32, h: u32) -> u32;
    #[link_name = "PMGPI.573"]
    fn GpiQueryBitmapParameters(h: u32, p: *mut u8) -> u32;
    #[link_name = "PMGPI.599"]
    fn GpiQueryBitmapBits(hps: u32, start: i32, n: i32, buf: *mut u8, bmi: *mut u8) -> i32;
    #[link_name = "PMGPI.355"]
    fn GpiBitBlt(dst: u32, src: u32, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.557"]
    fn GpiWCBitBlt(dst: u32, hbm: u32, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.603"]
    fn GpiDrawBits(hps: u32, bits: *const u8, bmi: *const u8, n: i32, p: *const PointL, rop: i32, opts: u32) -> i32;
    #[link_name = "PMGPI.592"]
    fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: i32, start: i32, n: i32, t: *const i32) -> u32;
    #[link_name = "PMGPI.479"]
    fn GpiQueryRGBColor(hps: u32, opts: u32, c: i32) -> i32;
    #[link_name = "PMGPI.439"]
    fn GpiQueryColorIndex(hps: u32, opts: u32, c: i32) -> i32;
    #[link_name = "PMGPI.594"]
    fn GpiCreatePalette(hab: u32, opts: u32, fmt: u32, n: u32, t: *const u32) -> u32;
    #[link_name = "PMGPI.578"]
    fn GpiSelectPalette(hps: u32, h: u32) -> u32;
    #[link_name = "PMGPI.453"]
    fn GpiQueryFontMetrics(hps: u32, n: i32, p: *mut u8) -> u32;
    #[link_name = "PMGPI.489"]
    fn GpiQueryTextBox(hps: u32, n: i32, s: *const u8, n2: i32, p: *mut PointL) -> u32;
    #[link_name = "PMGPI.584"]
    fn GpiQueryCharStringPos(hps: u32, opts: u32, n: i32, s: *const u8, dx: *const i32, p: *mut PointL) -> u32;
    #[link_name = "PMGPI.580"]
    fn GpiCharStringPos(hps: u32, r: *const RectL, opts: u32, n: i32, s: *const u8, dx: *const i32) -> i32;
    #[link_name = "PMGPI.359"]
    fn GpiCharStringAt(hps: u32, p: *const PointL, n: i32, s: *const u8) -> i32;
    #[link_name = "PMGPI.510"]
    fn GpiSetCharBox(hps: u32, p: *const [i32; 2]) -> u32;
    #[link_name = "PMGPI.368"]
    fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.513"]
    fn GpiSetCharSet(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.378"]
    fn GpiDeleteSetId(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.492"]
    fn GpiQueryWidthTable(hps: u32, first: i32, n: i32, out: *mut i32) -> u32;
    #[link_name = "PMGPI.586"]
    fn GpiQueryFonts(hps: u32, opts: u32, face: *const u8, req: *mut i32, len: i32, out: *mut u8) -> i32;
    #[link_name = "PMGPI.520"]
    fn GpiSetDefaultViewMatrix(hps: u32, n: i32, m: *const [i32; 9], opt: i32) -> u32;
    #[link_name = "PMGPI.538"]
    fn GpiSetModelTransformMatrix(hps: u32, n: i32, m: *const [i32; 9], opt: i32) -> u32;
    #[link_name = "PMGPI.566"]
    fn GpiRotate(hps: u32, m: *mut [i32; 9], opt: i32, a: i32, c: *const PointL) -> u32;
    #[link_name = "PMGPI.364"]
    fn GpiConvert(hps: u32, from: i32, to: i32, n: i32, p: *mut PointL) -> u32;
    #[link_name = "PMGPI.476"]
    fn GpiQueryPel(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.389"]
    fn GpiErase(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.411"]
    fn GpiPlayMetaFile(hps: u32, hmf: u32, n1: i32, opts: *const i32, segs: *mut i32, n2: i32, desc: *mut u8) -> i32;
    #[link_name = "PMGPI.466"]
    fn GpiQueryMetaFileLength(hmf: u32) -> i32;
    #[link_name = "PMGPI.465"]
    fn GpiQueryMetaFileBits(hmf: u32, off: i32, n: i32, p: *mut u8) -> u32;
    #[link_name = "PMGPI.536"]
    fn GpiSetMetaFileBits(hmf: u32, off: i32, n: i32, p: *const u8) -> u32;
    #[link_name = "PMGPI.365"]
    fn GpiCopyMetaFile(hmf: u32) -> u32;
    #[link_name = "PMGPI.375"]
    fn GpiDeleteMetaFile(hmf: u32) -> u32;
    #[link_name = "PMGPI.500"]
    fn GpiSaveMetaFile(hmf: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.401"]
    fn GpiLoadMetaFile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMGPI.522"]
    fn GpiSetDrawingMode(hps: u32, mode: i32) -> u32;
    #[link_name = "PMGPI.408"]
    fn GpiOpenSegment(hps: u32, id: i32) -> u32;
    #[link_name = "PMGPI.361"]
    fn GpiCloseSegment(hps: u32) -> u32;
    #[link_name = "PMGPI.380"]
    fn GpiDrawChain(hps: u32) -> u32;
    #[link_name = "PMGPI.394"]
    fn GpiGetData(hps: u32, id: i32, off: *mut i32, format: i32, n: i32, buf: *mut u8) -> i32;
    #[link_name = "PMGPI.421"]
    fn GpiPutData(hps: u32, format: i32, n: *mut i32, buf: *const u8) -> i32;
    #[link_name = "PMGPI.483"]
    fn GpiQuerySegmentNames(hps: u32, first: i32, last: i32, max: i32, ids: *mut i32) -> i32;
    #[link_name = "PMGPI.376"]
    fn GpiDeleteSegment(hps: u32, id: i32) -> u32;
    #[link_name = "PMWIN.712"]
    fn WinCpTranslateString(hab: u32, from: u32, src: *const u8, to: u32, max: u32, dst: *mut u8) -> u32;
    #[link_name = "PMGPI.606"]
    fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;
}

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

const CLR_BACKGROUND: i32 = 0;
const CLR_BLUE: i32 = 1;
const CLR_RED: i32 = 2;
const CLR_PINK: i32 = 3;
const CLR_GREEN: i32 = 4;
const CLR_CYAN: i32 = 5;
const CLR_YELLOW: i32 = 6;
const CLR_NEUTRAL: i32 = 7;
const CLR_DARKGRAY: i32 = 8;
const CLR_DARKBLUE: i32 = 9;
const CLR_DARKGREEN: i32 = 12;
const CLR_BROWN: i32 = 14;
const CLR_PALEGRAY: i32 = 15;
const CLR_BLACK: i32 = -1;

const PU_PELS: u32 = 8;
const PU_LOMETRIC: u32 = 0x0c;
const GPIT_MICRO: u32 = 0x1000;
const GPIA_ASSOC: u32 = 0x4000;
const OD_MEMORY: i32 = 8;
const ROP_SRCCOPY: i32 = 0xcc;
const ROP_NOTSRCCOPY: i32 = 0x33;
const ROP_ONE: i32 = 0xff;
const ROP_ZERO: i32 = 0x00;
const DRO_FILL: i32 = 1;
const DRO_OUTLINE: i32 = 2;
const DRO_OUTLINEFILL: i32 = 3;
const BA_BOUNDARY: u32 = 1;
const BA_WINDING: u32 = 2;
const CVTC_WORLD: i32 = 1;
const CVTC_DEVICE: i32 = 5;
const TRANSFORM_REPLACE: i32 = 0;
const TRANSFORM_ADD: i32 = 1;
const CHS_OPAQUE: u32 = 1;
const CHS_CLIP: u32 = 0x10;
const MAKEFIXED: i32 = 0x10000;

fn pt(x: i32, y: i32) -> PointL {
    PointL { x, y }
}

fn rect(l: i32, b: i32, r: i32, t: i32) -> RectL {
    RectL { l, b, r, t }
}

/// A BITMAPINFOHEADER2 (16-byte form) as bytes.
fn header2(cx: u32, cy: u32, bits: u16) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(&16u32.to_le_bytes());
    h[4..8].copy_from_slice(&cx.to_le_bytes());
    h[8..12].copy_from_slice(&cy.to_le_bytes());
    h[12..14].copy_from_slice(&1u16.to_le_bytes());
    h[14..16].copy_from_slice(&bits.to_le_bytes());
    h
}

/// A memory DC, PS and selected 24-bit bitmap.
struct Mem {
    hdc: u32,
    hps: u32,
    hbm: u32,
}

fn mem(hab: u32, w: u32, h: u32, units: u32) -> Mem {
    unsafe {
        let hdc = DevOpenDC(hab, OD_MEMORY, b"*\0".as_ptr(), 0, null(), 0);
        let size = pt(0, 0);
        let hps = GpiCreatePS(hab, hdc, &size, units | GPIT_MICRO | GPIA_ASSOC);
        let hdr = header2(w, h, 24);
        let hbm = GpiCreateBitmap(hps, hdr.as_ptr(), 0, null(), null());
        GpiSetBitmap(hps, hbm);
        Mem { hdc, hps, hbm }
    }
}

fn free(m: Mem) {
    unsafe {
        GpiSetBitmap(m.hps, 0);
        GpiDeleteBitmap(m.hbm);
        GpiDestroyPS(m.hps);
        DevCloseDC(m.hdc);
    }
}

/// RGB of a pel of a 24-bit memory bitmap (via GpiQueryBitmapBits).
fn pel(m: &Mem, x: i32, y: i32) -> u32 {
    let mut hdr = [0u8; 16 + 1024];
    hdr[..16].copy_from_slice(&header2(0, 0, 24));
    let mut row = [0u8; 4096];
    let n = unsafe { GpiQueryBitmapBits(m.hps, y, 1, row.as_mut_ptr(), hdr.as_mut_ptr()) };
    if n != 1 {
        return 0xdead;
    }
    let i = x as usize * 3;
    (row[i + 2] as u32) << 16 | (row[i + 1] as u32) << 8 | row[i] as u32
}

fn count(m: &Mem, w: i32, h: i32, c: u32) -> i32 {
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            if pel(m, x, y) == c {
                n += 1;
            }
        }
    }
    n
}

fn self_checks(hab: u32) {
    unsafe {
        // Drawing into a memory bitmap, read back with GpiQueryPel and
        // GpiQueryBitmapBits.
        let m = mem(hab, 32, 32, PU_PELS);
        check("memory PS and bitmap", m.hdc != 0 && m.hps != 0 && m.hbm != 0);
        let mut bp = [0u8; 12];
        bp[0] = 12;
        GpiQueryBitmapParameters(m.hbm, bp.as_mut_ptr());
        check("GpiQueryBitmapParameters", u16::from_le_bytes([bp[4], bp[5]]) == 32 && u16::from_le_bytes([bp[10], bp[11]]) == 24);
        GpiErase(m.hps);
        GpiSetColor(m.hps, CLR_RED);
        GpiMove(m.hps, &pt(0, 0));
        GpiBox(m.hps, DRO_FILL, &pt(9, 9), 0, 0);
        check("GpiQueryPel = CLR_RED", GpiQueryPel(m.hps, &pt(5, 5)) == CLR_RED);
        check("box fill is inclusive", pel(&m, 9, 9) == 0xff0000 && pel(&m, 10, 10) == 0xffffff);
        // Raster ops between bitmaps.
        let m2 = mem(hab, 32, 32, PU_PELS);
        let p = [pt(0, 0), pt(32, 32), pt(0, 0)];
        GpiBitBlt(m2.hps, m.hps, 3, p.as_ptr(), ROP_NOTSRCCOPY, 0);
        check("GpiBitBlt ROP_NOTSRCCOPY", pel(&m2, 5, 5) == 0x00ffff && pel(&m2, 20, 20) == 0);
        let p = [pt(0, 0), pt(32, 32), pt(0, 0), pt(16, 16)];
        GpiBitBlt(m2.hps, m.hps, 4, p.as_ptr(), ROP_SRCCOPY, 0);
        check("GpiBitBlt stretch", pel(&m2, 19, 19) == 0xff0000 && pel(&m2, 21, 21) == 0xffffff);
        // Clip path: only the path's interior is touched.
        let p = [pt(0, 0), pt(32, 32)];
        GpiBitBlt(m2.hps, 0, 2, p.as_ptr(), ROP_ZERO, 0);
        GpiBeginPath(m2.hps, 1);
        GpiMove(m2.hps, &pt(10, 10));
        GpiBox(m2.hps, DRO_OUTLINE, &pt(19, 19), 0, 0);
        GpiEndPath(m2.hps);
        GpiSetClipPath(m2.hps, 1, 4);
        GpiBitBlt(m2.hps, 0, 2, p.as_ptr(), ROP_ONE, 0);
        GpiSetClipPath(m2.hps, 0, 0);
        check("GpiSetClipPath clips blits", count(&m2, 32, 32, 0xffffff) == 100);
        // Areas: winding fills nested figures, alternate leaves a hole.
        for (fl, hole, name) in [(0, true, "BA_ALTERNATE"), (BA_WINDING, false, "BA_WINDING")] {
            GpiErase(m2.hps);
            GpiSetColor(m2.hps, CLR_BLUE);
            GpiBeginArea(m2.hps, fl);
            GpiMove(m2.hps, &pt(2, 2));
            GpiPolyLine(m2.hps, 3, [pt(29, 2), pt(29, 29), pt(2, 29)].as_ptr());
            GpiMove(m2.hps, &pt(10, 10));
            GpiPolyLine(m2.hps, 3, [pt(20, 10), pt(20, 20), pt(10, 20)].as_ptr());
            GpiEndArea(m2.hps);
            check(name, pel(&m2, 5, 5) == 0x0000ff && (pel(&m2, 15, 15) == 0xffffff) == hole);
        }
        // Clip region: lines only inside it.
        GpiErase(m2.hps);
        GpiSetColor(m2.hps, CLR_NEUTRAL);
        let cr = GpiCreateRegion(m2.hps, 1, &rect(8, 8, 16, 16));
        let mut old = 0;
        GpiSetClipRegion(m2.hps, cr, &mut old);
        GpiMove(m2.hps, &pt(0, 10));
        GpiLine(m2.hps, &pt(31, 10));
        GpiSetClipRegion(m2.hps, 0, &mut old);
        GpiDestroyRegion(m2.hps, cr);
        check("GpiSetClipRegion clips lines", count(&m2, 32, 32, 0) == 8 && old == cr);
        // Full arc area.
        GpiErase(m2.hps);
        GpiMove(m2.hps, &pt(16, 16));
        GpiFullArc(m2.hps, DRO_FILL, 10 * MAKEFIXED);
        check("GpiFullArc fill", pel(&m2, 16, 16) == 0 && pel(&m2, 16, 25) == 0 && pel(&m2, 16, 28) == 0xffffff);
        free(m2);
        free(m);

        // Regions.
        let a = GpiCreateRegion(0, 1, &rect(0, 0, 10, 10));
        let b = GpiCreateRegion(0, 1, &rect(5, 5, 15, 15));
        let c = GpiCreateRegion(0, 0, null());
        check("GpiCombineRegion CRGN_OR", GpiCombineRegion(0, c, a, b, 1) == 3);
        let mut r = RectL::default();
        GpiQueryRegionBox(0, c, &mut r);
        check("GpiQueryRegionBox", r == rect(0, 0, 15, 15));
        check("GpiPtInRegion", GpiPtInRegion(0, c, &pt(12, 2)) == 1 && GpiPtInRegion(0, c, &pt(12, 12)) == 2);
        check("GpiCombineRegion CRGN_AND", GpiCombineRegion(0, c, a, b, 6) == 2);
        GpiOffsetRegion(0, c, &pt(10, 0));
        GpiQueryRegionBox(0, c, &mut r);
        check("GpiOffsetRegion", r == rect(15, 5, 20, 10));
        for h in [a, b, c] {
            GpiDestroyRegion(0, h);
        }

        // Page units and transforms.
        let m = mem(hab, 8, 8, PU_LOMETRIC);
        let mut p = [pt(1000, 500)];
        GpiConvert(m.hps, CVTC_WORLD, CVTC_DEVICE, 1, p.as_mut_ptr());
        check("PU_LOMETRIC convert", p[0] == pt(378, 189));
        free(m);
        let m = mem(hab, 8, 8, PU_PELS);
        let mx = [2 * MAKEFIXED, 0, 0, 0, 3 * MAKEFIXED, 0, 10, 20, 1];
        GpiSetDefaultViewMatrix(m.hps, 9, &mx, TRANSFORM_REPLACE);
        let mut p = [pt(5, 5)];
        GpiConvert(m.hps, CVTC_WORLD, CVTC_DEVICE, 1, p.as_mut_ptr());
        check("default view matrix", p[0] == pt(20, 35));
        GpiConvert(m.hps, CVTC_DEVICE, CVTC_WORLD, 1, p.as_mut_ptr());
        check("convert back", p[0] == pt(5, 5));

        // Colour tables.
        let t = [0x123456];
        GpiCreateLogColorTable(m.hps, 0, 2, 1, 1, t.as_ptr());
        check("LCOLF_CONSECRGB", GpiQueryRGBColor(m.hps, 0, 1) == 0x123456 && GpiQueryColorIndex(m.hps, 0, 0x123456) == 1);
        GpiCreateLogColorTable(m.hps, 0, 3, 0, 0, null());
        check("LCOLF_RGB", GpiQueryRGBColor(m.hps, 0, 0x00abcdef) == 0xabcdef);
        let pal = GpiCreatePalette(hab, 0, 2, 3, [0x010203u32, 0x040506, 0x070809].as_ptr());
        check("GpiCreatePalette/SelectPalette", pal != 0 && GpiSelectPalette(m.hps, pal) == 0);

        free(m);
        let m = mem(hab, 8, 8, PU_PELS);
        // Fonts and text.
        let mut fm = [0u8; 228];
        check("GpiQueryFontMetrics", GpiQueryFontMetrics(m.hps, 228, fm.as_mut_ptr()) != 0);
        let l = |o: usize| i32::from_le_bytes([fm[o], fm[o + 1], fm[o + 2], fm[o + 3]]);
        // The system font, whichever it is: its cell is its ascender and
        // descender, and it has a face name.
        let (asc, desc) = (l(76), l(80));
        check("font metrics values", l(112) == asc + desc && l(100) > 0 && asc > desc && fm[32] != 0);
        free(m);
        let m = mem(hab, 8, 8, PU_PELS);
        let mut tb = [PointL::default(); 5];
        GpiQueryTextBox(m.hps, 3, b"abc".as_ptr(), 5, tb.as_mut_ptr());
        check("GpiQueryTextBox", tb[4].x > 0 && tb[4].y == 0 && tb[0] == pt(0, asc) && tb[1] == pt(0, -desc));
        let mut cp = [PointL::default(); 4];
        GpiMove(m.hps, &pt(10, 0));
        GpiQueryCharStringPos(m.hps, 0, 3, b"abc".as_ptr(), null(), cp.as_mut_ptr());
        check("GpiQueryCharStringPos", cp[0] == pt(10, 0) && cp[3] == pt(10 + tb[4].x, 0) && cp[1].x > 10 && cp[2].x > cp[1].x);
        let mut req = 0;
        check("GpiQueryFonts", GpiQueryFonts(m.hps, 1, null(), &mut req, 228, null_mut()) == 12);
        let mut fms = [0u8; 228 * 2];
        req = 2;
        let left = GpiQueryFonts(m.hps, 1, b"Helvetica Bold\0".as_ptr(), &mut req, 228, fms.as_mut_ptr());
        check("GpiQueryFonts by face", left == 0 && req == 1 && &fms[32..46] == b"Helvetica Bold" && &fms[0..10] == b"Helvetica\0");
        let mut fat = [0u8; 56];
        fat[0] = 56;
        fat[54] = 4; // FATTR_FONTUSE_OUTLINE
        check("GpiCreateLogFont", GpiCreateLogFont(m.hps, null(), 1, fat.as_ptr()) == 2 && GpiSetCharSet(m.hps, 1) != 0);
        GpiSetCharBox(m.hps, &[16 * MAKEFIXED, 16 * MAKEFIXED]);
        GpiQueryTextBox(m.hps, 2, b"ab".as_ptr(), 5, tb.as_mut_ptr());
        let small = (tb[4].x, tb[0].y);
        GpiSetCharBox(m.hps, &[32 * MAKEFIXED, 32 * MAKEFIXED]);
        GpiQueryTextBox(m.hps, 2, b"ab".as_ptr(), 5, tb.as_mut_ptr());
        // Twice the box: about twice the text (small sizes snap their
        // x-height to whole pels), and an ascender within the em.
        let near = |a: i32, b: i32| (a - b).abs() * 100 <= b * 15;
        check("outline font scales to the char box", near(tb[4].x, 2 * small.0) && near(tb[0].y, 2 * small.1) && tb[0].y > 16 && tb[0].y <= 32);
        GpiSetCharSet(m.hps, 0);
        check("GpiDeleteSetId", GpiDeleteSetId(m.hps, 1) != 0);
        // Each character's width is its own advance (fonts loaded with
        // GpiLoadFonts are proportional; EPM places its text with
        // GpiQueryWidthTable).
        let mut fat = [0u8; 56];
        fat[0] = 56;
        fat[8..12].copy_from_slice(b"Helv");
        GpiCreateLogFont(m.hps, null(), 2, fat.as_ptr());
        GpiSetCharSet(m.hps, 2);
        let mut w = [0i32; 3];
        GpiQueryWidthTable(m.hps, b'i' as i32, 1, w.as_mut_ptr());
        GpiQueryWidthTable(m.hps, b'V' as i32, 2, w[1..].as_mut_ptr());
        let mut ti = [PointL::default(); 5];
        GpiQueryTextBox(m.hps, 1, b"i".as_ptr(), 5, ti.as_mut_ptr());
        GpiQueryTextBox(m.hps, 1, b"W".as_ptr(), 5, tb.as_mut_ptr());
        check("GpiQueryWidthTable per character", w[0] > 0 && w[0] == ti[4].x && w[2] == tb[4].x);
        GpiSetCharSet(m.hps, 0);
        GpiDeleteSetId(m.hps, 2);
        free(m);

        let hdc = DevOpenDC(hab, OD_MEMORY, b"*\0".as_ptr(), 0, null(), 0);
        let mut caps = [0i32; 2];
        DevQueryCaps(hdc, 16, 1, caps.as_mut_ptr());
        check("DevQueryCaps CAPS_COLOR_BITCOUNT", caps[0] == 24);
        DevCloseDC(hdc);
    }
}

// ---- Metafiles and retained segments ----

const OD_METAFILE: i32 = 3;
const DM_DRAW: i32 = 1;
const DM_RETAIN: i32 = 2;
const DM_DRAWANDRETAIN: i32 = 3;

/// Draw the metafile test picture: a red filled box (2,2)-(9,9) and a
/// blue line (20,20)-(30,20).
unsafe fn picture(hps: u32) {
    unsafe {
        GpiSetColor(hps, CLR_RED);
        GpiMove(hps, &pt(2, 2));
        GpiBox(hps, DRO_FILL, &pt(9, 9), 0, 0);
        GpiSetColor(hps, CLR_BLUE);
        GpiMove(hps, &pt(20, 20));
        GpiLine(hps, &pt(30, 20));
    }
}

fn picture_ok(m: &Mem) -> bool {
    pel(m, 5, 5) == 0xff0000 && pel(m, 25, 20) == 0x0000ff && pel(m, 15, 15) == 0xffffff
}

/// A metafile DC with an associated PS.
fn meta_dc(hab: u32) -> (u32, u32) {
    unsafe {
        let hdc = DevOpenDC(hab, OD_METAFILE, b"*\0".as_ptr(), 0, null(), 0);
        let size = pt(0, 0);
        (hdc, GpiCreatePS(hab, hdc, &size, PU_PELS | GPIA_ASSOC))
    }
}

/// Close a metafile DC (its PS disassociated first): the metafile.
fn close_meta(hdc: u32, hps: u32) -> u32 {
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc)
    }
}

/// A 32x32 memory bitmap cleared to white.
fn white(hab: u32) -> Mem {
    let m = mem(hab, 32, 32, PU_PELS);
    unsafe { GpiErase(m.hps) };
    m
}

fn meta_checks(hab: u32) {
    unsafe {
        let (hdc, hps) = meta_dc(hab);
        check("DevOpenDC OD_METAFILE", hdc != 0 && hps != 0);
        picture(hps);
        let hmf = close_meta(hdc, hps);
        check("DevCloseDC gives the metafile", hmf > 1);
        let len = GpiQueryMetaFileLength(hmf);
        let mut bits = alloc::vec![0u8; len.max(0) as usize];
        let ok = len > 0 && GpiQueryMetaFileBits(hmf, 0, len, bits.as_mut_ptr()) != 0;
        check("GpiQueryMetaFileBits: a Begin Document field", ok && bits[2..5] == [0xd3, 0xa8, 0xa8]);
        let m = white(hab);
        check("GpiPlayMetaFile", GpiPlayMetaFile(m.hps, hmf, 0, null(), null_mut(), 0, null_mut()) == 1);
        check("played metafile draws the picture", picture_ok(&m));
        free(m);

        // An empty metafile filled with GpiSetMetaFileBits plays the same.
        let empty = DevCloseDC(DevOpenDC(hab, OD_METAFILE, b"*\0".as_ptr(), 0, null(), 0));
        let set = GpiSetMetaFileBits(empty, 0, len, bits.as_ptr()) != 0;
        let m = white(hab);
        GpiPlayMetaFile(m.hps, empty, 0, null(), null_mut(), 0, null_mut());
        check("GpiSetMetaFileBits", set && GpiQueryMetaFileLength(empty) == len && picture_ok(&m));
        free(m);
        GpiDeleteMetaFile(empty);

        // Copy, save (deletes the copy), load.
        let copy = GpiCopyMetaFile(hmf);
        let saved = GpiSaveMetaFile(copy, b"C:\\GPITEST.MET\0".as_ptr()) != 0;
        check("GpiSaveMetaFile deletes the metafile", saved && GpiQueryMetaFileLength(copy) == -1);
        let loaded = GpiLoadMetaFile(hab, b"C:\\GPITEST.MET\0".as_ptr());
        let m = white(hab);
        GpiPlayMetaFile(m.hps, loaded, 0, null(), null_mut(), 0, null_mut());
        check("GpiLoadMetaFile", loaded != 0 && GpiQueryMetaFileLength(loaded) == len && picture_ok(&m));
        free(m);
        GpiDeleteMetaFile(loaded);
        check("GpiDeleteMetaFile", GpiDeleteMetaFile(hmf) != 0 && GpiDeleteMetaFile(hmf) == 0);
        check("GpiLoadMetaFile of a non-metafile fails", GpiLoadMetaFile(hab, b"C:\\CONFIG.SYS\0".as_ptr()) == 0);

        // Retained segments: not drawn in DM_RETAIN, drawn by GpiDrawChain.
        let m = white(hab);
        GpiSetDrawingMode(m.hps, DM_RETAIN);
        let opened = GpiOpenSegment(m.hps, 7) != 0;
        picture(m.hps);
        GpiCloseSegment(m.hps);
        check("DM_RETAIN does not draw", opened && pel(&m, 5, 5) == 0xffffff);
        let mut ids = [0i32; 4];
        check("GpiQuerySegmentNames", GpiQuerySegmentNames(m.hps, 1, 100, 4, ids.as_mut_ptr()) == 1 && ids[0] == 7);
        GpiDrawChain(m.hps);
        check("GpiDrawChain", picture_ok(&m));
        let mut buf = [0u8; 512];
        let mut off = 0;
        let n = GpiGetData(m.hps, 7, &mut off, 0, 512, buf.as_mut_ptr());
        check("GpiGetData", n > 0 && off == n);
        GpiDeleteSegment(m.hps, 7);
        check("GpiDeleteSegment", GpiQuerySegmentNames(m.hps, 1, 100, 4, ids.as_mut_ptr()) == 0);
        free(m);
        let m = white(hab);
        let mut cnt = n;
        let put = GpiPutData(m.hps, 0, &mut cnt, buf.as_ptr());
        check("GpiPutData draws the orders", put == 1 && cnt == n && picture_ok(&m));
        free(m);
        let m = white(hab);
        GpiSetDrawingMode(m.hps, DM_DRAWANDRETAIN);
        GpiOpenSegment(m.hps, 3);
        picture(m.hps);
        GpiCloseSegment(m.hps);
        check("DM_DRAWANDRETAIN draws", picture_ok(&m));
        GpiSetDrawingMode(m.hps, DM_DRAW);
        free(m);

        // Retained segments drawn onto a metafile (as OS/2's PMPIC does).
        let (hdc, hps) = meta_dc(hab);
        GpiSetDrawingMode(hps, DM_RETAIN);
        GpiOpenSegment(hps, 1);
        picture(hps);
        GpiCloseSegment(hps);
        GpiDrawChain(hps);
        let hmf = close_meta(hdc, hps);
        let m = white(hab);
        GpiPlayMetaFile(m.hps, hmf, 0, null(), null_mut(), 0, null_mut());
        check("GpiDrawChain onto a metafile", picture_ok(&m));
        free(m);
        GpiDeleteMetaFile(hmf);

        // Code page 437's cent sign is 0xBD in code page 850.
        let mut out = [0u8; 4];
        let r = WinCpTranslateString(hab, 437, b"a\x9b\0".as_ptr(), 850, 4, out.as_mut_ptr());
        check("WinCpTranslateString", r != 0 && out[..3] == [b'a', 0xbd, 0]);
    }
}

// ---- The window ----

const WM_PAINT: u32 = 0x23;

static HAB: AtomicU32 = AtomicU32::new(0);
/// C:\YOSEMITE.BMP (the Toolkit's jigsaw picture), when present.
static PICTURE: AtomicU32 = AtomicU32::new(0);

/// Load a bitmap file into a GPI bitmap with GpiCreateBitmap(CBM_INIT),
/// as the jigsaw sample does.
fn load_picture(name: &[u8]) -> u32 {
    unsafe {
        let (mut hf, mut action) = (0, 0);
        if DosOpen(name.as_ptr(), &mut hf, &mut action, 0, 0, 1, 0x40, null_mut()) != 0 {
            return 0;
        }
        let mut data = alloc::vec![0u8; 512 * 1024];
        let mut n = 0;
        DosRead(hf, data.as_mut_ptr(), data.len() as u32, &mut n);
        DosClose(hf);
        if n < 30 || &data[0..2] != b"BM" {
            return 0;
        }
        let off = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
        let info = data.as_ptr().add(14);
        GpiCreateBitmap(0, info, 4, data.as_ptr().add(off), info)
    }
}

/// A small 8-bit bitmap: a gradient with a colour table (BITMAPINFO2).
fn gradient() -> ([u8; 16 + 256 * 4], [u8; 16 * 16]) {
    let mut bmi = [0u8; 16 + 256 * 4];
    bmi[..16].copy_from_slice(&header2(16, 16, 8));
    for i in 0..256usize {
        let (r, g, b) = ((i * 16 % 256) as u8, (i / 16 * 16) as u8, 0x80u8);
        bmi[16 + i * 4..16 + i * 4 + 3].copy_from_slice(&[b, g, r]);
    }
    let mut bits = [0u8; 256];
    for (i, v) in bits.iter_mut().enumerate() {
        *v = i as u8;
    }
    (bmi, bits)
}

unsafe fn paint(hwnd: u32) {
    unsafe {
        let hps = WinBeginPaint(hwnd, 0, null_mut());
        let mut r = RectL::default();
        r.r = 4000;
        r.t = 4000;
        WinFillRect(hps, &r, CLR_PALEGRAY);
        let label = |x: i32, y: i32, s: &[u8]| {
            GpiSetColor(hps, CLR_BLACK);
            GpiCharStringAt(hps, &pt(x, y), s.len() as i32, s.as_ptr());
        };

        // Row 1: boxes, arcs, areas, curves.
        let y = 360;
        label(10, y + 150, b"Boxes");
        GpiSetColor(hps, CLR_RED);
        GpiMove(hps, &pt(10, y));
        GpiBox(hps, DRO_OUTLINEFILL, &pt(70, y + 60), 0, 0);
        GpiSetColor(hps, CLR_DARKGREEN);
        GpiSetPattern(hps, 19);
        GpiMove(hps, &pt(85, y));
        GpiBox(hps, DRO_OUTLINEFILL, &pt(165, y + 60), 30, 30);
        GpiSetPattern(hps, 0);
        GpiSetColor(hps, CLR_BLUE);
        GpiMove(hps, &pt(10, y + 75));
        GpiBox(hps, DRO_OUTLINE, &pt(165, y + 135), 60, 60);
        for (i, sym) in [9, 10, 11, 12, 18].iter().enumerate() {
            GpiSetPattern(hps, *sym);
            GpiMove(hps, &pt(30 + i as i32 * 26, y + 90));
            GpiBox(hps, DRO_OUTLINEFILL, &pt(50 + i as i32 * 26, y + 120), 0, 0);
        }
        GpiSetPattern(hps, 0);

        let x = 190;
        label(x, y + 150, b"Arcs");
        GpiSetColor(hps, CLR_CYAN);
        GpiSetArcParams(hps, &[2, 1, 0, 0]);
        GpiMove(hps, &pt(x + 45, y + 105));
        GpiFullArc(hps, DRO_OUTLINEFILL, 20 * MAKEFIXED);
        GpiSetArcParams(hps, &[1, 1, 0, 0]);
        GpiSetColor(hps, CLR_PINK);
        GpiBeginArea(hps, BA_BOUNDARY);
        GpiMove(hps, &pt(x + 45, y + 30));
        GpiPartialArc(hps, &pt(x + 45, y + 30), 30 * MAKEFIXED, 30 * MAKEFIXED, 270 * MAKEFIXED);
        GpiEndArea(hps);
        GpiSetColor(hps, CLR_DARKBLUE);
        GpiMove(hps, &pt(x + 100, y + 20));
        GpiPointArc(hps, [pt(x + 130, y + 70), pt(x + 160, y + 20)].as_ptr());
        GpiMove(hps, &pt(x + 100, y + 90));
        GpiPointArc(hps, [pt(x + 130, y + 130), pt(x + 160, y + 90)].as_ptr());

        let x = 370;
        label(x, y + 150, b"Areas");
        let star = |cx: i32, cy: i32| -> [PointL; 5] {
            // Five-pointed star (every second vertex of a pentagon).
            let v = [(0, 60), (35, -48), (-57, 18), (57, 18), (-35, -48)];
            let mut p = [PointL::default(); 5];
            for (i, (dx, dy)) in v.iter().enumerate() {
                p[i] = pt(cx + dx, cy + dy);
            }
            p
        };
        GpiSetColor(hps, CLR_YELLOW);
        for (cx, fl) in [(x + 40, 0u32), (x + 120, BA_WINDING)] {
            let s = star(cx, y + 75);
            GpiBeginArea(hps, fl | BA_BOUNDARY);
            GpiMove(hps, &s[4]);
            GpiPolyLine(hps, 5, s.as_ptr());
            GpiEndArea(hps);
        }

        let x = 550;
        label(x, y + 150, b"Curves");
        GpiSetColor(hps, CLR_DARKBLUE);
        GpiSetLineWidth(hps, 2 * MAKEFIXED);
        GpiMove(hps, &pt(x, y + 100));
        GpiPolySpline(hps, 6, [pt(x + 30, y + 140), pt(x + 50, y + 60), pt(x + 80, y + 100), pt(x + 110, y + 140), pt(x + 130, y + 60), pt(x + 160, y + 100)].as_ptr());
        GpiSetLineWidth(hps, 0);
        let f = [pt(x + 40, y + 60), pt(x + 80, y), pt(x + 120, y + 60), pt(x + 160, y)];
        GpiSetColor(hps, CLR_DARKGRAY);
        GpiSetLineType(hps, 1);
        GpiMove(hps, &pt(x, y));
        GpiPolyLine(hps, 4, f.as_ptr());
        GpiSetLineType(hps, 0);
        GpiSetColor(hps, CLR_RED);
        GpiMove(hps, &pt(x, y));
        GpiPolyFillet(hps, 4, f.as_ptr());

        // Row 2: line types, markers, regions, paths.
        let y = 185;
        label(10, y + 150, b"Lines");
        GpiSetColor(hps, CLR_NEUTRAL);
        for t in 0..9 {
            GpiSetLineType(hps, t);
            GpiMove(hps, &pt(10, y + 130 - t * 10));
            GpiLine(hps, &pt(165, y + 130 - t * 10));
        }
        GpiSetLineType(hps, 0);
        for w in 1..4 {
            GpiSetLineWidth(hps, w * MAKEFIXED);
            GpiSetColor(hps, CLR_BROWN);
            GpiMove(hps, &pt(10, y + 30 - w * 8));
            GpiLine(hps, &pt(165, y + 30 - w * 8));
        }
        GpiSetLineWidth(hps, 0);

        let x = 190;
        label(x, y + 150, b"Markers");
        GpiSetColor(hps, CLR_DARKGREEN);
        for sym in 1..=10 {
            GpiSetMarker(hps, sym);
            let (cx, cy) = (x + 15 + (sym - 1) % 5 * 32, y + 110 - (sym - 1) / 5 * 40);
            GpiPolyMarker(hps, 1, &pt(cx, cy));
        }
        GpiSetMarker(hps, 0);

        let x = 370;
        label(x, y + 150, b"Regions");
        let a = GpiCreateRegion(hps, 1, &rect(x, y + 20, x + 100, y + 120));
        let b = GpiCreateRegion(hps, 1, &rect(x + 60, y + 50, x + 160, y + 140));
        let c = GpiCreateRegion(hps, 0, null());
        GpiCombineRegion(hps, c, a, b, 4);
        GpiSetColor(hps, CLR_BLUE);
        GpiSetPattern(hps, 4);
        GpiPaintRegion(hps, c);
        GpiSetPattern(hps, 0);
        let mut old = 0;
        GpiSetClipRegion(hps, c, &mut old);
        GpiSetColor(hps, CLR_RED);
        for i in 0..20 {
            GpiMove(hps, &pt(x, y + 10 + i * 7));
            GpiLine(hps, &pt(x + 170, y + 30 + i * 7));
        }
        GpiSetClipRegion(hps, 0, &mut old);
        for h in [a, b, c] {
            GpiDestroyRegion(hps, h);
        }

        let x = 550;
        label(x, y + 150, b"Paths");
        GpiSetColor(hps, CLR_DARKBLUE);
        GpiBeginPath(hps, 1);
        GpiMove(hps, &pt(x + 10, y + 100));
        GpiPolyLine(hps, 3, [pt(x + 50, y + 135), pt(x + 90, y + 100), pt(x + 130, y + 135)].as_ptr());
        GpiEndPath(hps);
        GpiSetLineWidthGeom(hps, 8);
        GpiStrokePath(hps, 1, 0);
        GpiSetColor(hps, CLR_GREEN);
        GpiBeginPath(hps, 1);
        GpiMove(hps, &pt(x + 140, y + 100));
        GpiFullArc(hps, DRO_OUTLINE, 15 * MAKEFIXED);
        GpiEndPath(hps);
        GpiFillPath(hps, 1, 0);
        // A clip path from outline-font text, with lines drawn through it.
        let mut fat = [0u8; 56];
        fat[0] = 56;
        fat[54] = 4;
        GpiCreateLogFont(hps, null(), 1, fat.as_ptr());
        GpiSetCharSet(hps, 1);
        GpiSetCharBox(hps, &[40 * MAKEFIXED, 70 * MAKEFIXED]);
        GpiBeginPath(hps, 1);
        GpiCharStringAt(hps, &pt(x + 5, y + 20), 4, b"GPI!".as_ptr());
        GpiEndPath(hps);
        GpiSetClipPath(hps, 1, 4 | 2);
        for i in 0..40 {
            GpiSetColor(hps, [CLR_RED, CLR_GREEN, CLR_BLUE, CLR_PINK][i as usize % 4]);
            GpiMove(hps, &pt(x + i * 5, y));
            GpiLine(hps, &pt(x + i * 5 + 40, y + 90));
        }
        GpiSetClipPath(hps, 0, 0);
        GpiSetCharSet(hps, 0);
        GpiDeleteSetId(hps, 1);

        // Row 3: bitmaps, image bits, transforms, text.
        let y = 10;
        label(10, y + 150, b"Bitmaps");
        let hab = HAB.load(Ordering::Relaxed);
        let m = mem(hab, 48, 48, PU_PELS);
        GpiErase(m.hps);
        GpiSetColor(m.hps, CLR_GREEN);
        GpiMove(m.hps, &pt(24, 24));
        GpiFullArc(m.hps, DRO_OUTLINEFILL, 20 * MAKEFIXED);
        GpiSetColor(m.hps, CLR_RED);
        GpiMove(m.hps, &pt(4, 4));
        GpiBox(m.hps, DRO_OUTLINE, &pt(43, 43), 0, 0);
        GpiBitBlt(hps, m.hps, 3, [pt(10, y + 80), pt(58, y + 128), pt(0, 0)].as_ptr(), ROP_SRCCOPY, 0);
        GpiBitBlt(hps, m.hps, 4, [pt(70, y + 10), pt(166, y + 106), pt(0, 0), pt(48, 48)].as_ptr(), ROP_SRCCOPY, 0);
        GpiBitBlt(hps, m.hps, 3, [pt(10, y + 20), pt(58, y + 68), pt(0, 0)].as_ptr(), ROP_NOTSRCCOPY, 0);
        WinDrawBitmap(hps, m.hbm, null(), &pt(118, y + 112), 0, 0, 0);
        free(m);

        let x = 190;
        label(x, y + 150, b"GpiDrawBits");
        let (bmi, bits) = gradient();
        GpiDrawBits(hps, bits.as_ptr(), bmi.as_ptr(), 4, [pt(x, y), pt(x + 63, y + 127), pt(0, 0), pt(16, 16)].as_ptr(), ROP_SRCCOPY, 0);
        let pic = PICTURE.load(Ordering::Relaxed);
        if pic != 0 {
            let mut bp = [0u8; 12];
            bp[0] = 12;
            GpiQueryBitmapParameters(pic, bp.as_mut_ptr());
            let (w, h) = (u16::from_le_bytes([bp[4], bp[5]]) as i32, u16::from_le_bytes([bp[6], bp[7]]) as i32);
            GpiWCBitBlt(hps, pic, 4, [pt(x + 68, y), pt(x + 167, y + 127), pt(0, 0), pt(w, h)].as_ptr(), ROP_SRCCOPY, 0);
        }

        let x = 370;
        label(x, y + 150, b"Transforms");
        let mut m = [MAKEFIXED, 0, 0, 0, MAKEFIXED, 0, 0, 0, 1];
        for i in 0..6 {
            GpiRotate(hps, &mut m, TRANSFORM_REPLACE, i * 15 * MAKEFIXED, &pt(x + 80, y + 70));
            GpiSetModelTransformMatrix(hps, 9, &m, TRANSFORM_REPLACE);
            GpiSetColor(hps, [CLR_RED, CLR_BROWN, CLR_YELLOW, CLR_GREEN, CLR_BLUE, CLR_PINK][i as usize]);
            GpiMove(hps, &pt(x + 30, y + 40));
            GpiBox(hps, DRO_OUTLINE, &pt(x + 130, y + 100), 0, 0);
        }
        GpiSetModelTransformMatrix(hps, 0, null(), TRANSFORM_REPLACE);
        let sc = [MAKEFIXED / 2, 0, 0, 0, MAKEFIXED / 2, 0, x / 2, y / 2, 1];
        GpiSetDefaultViewMatrix(hps, 9, &sc, TRANSFORM_ADD);
        GpiSetColor(hps, CLR_DARKBLUE);
        GpiMove(hps, &pt(x + 20, y + 20));
        GpiBox(hps, DRO_FILL, &pt(x + 60, y + 60), 20, 20);
        GpiSetDefaultViewMatrix(hps, 0, null(), TRANSFORM_REPLACE);

        let x = 550;
        label(x, y + 150, b"Text");
        GpiSetColor(hps, CLR_YELLOW);
        GpiSetBackColor(hps, CLR_DARKBLUE);
        let r = rect(x, y + 110, x + 150, y + 130);
        let s = b"Opaque, clipped text box";
        GpiMove(hps, &pt(x + 4, y + 115));
        GpiCharStringPos(hps, &r, CHS_OPAQUE | CHS_CLIP, s.len() as i32, s.as_ptr(), null());
        GpiSetBackColor(hps, CLR_BACKGROUND);
        GpiSetColor(hps, CLR_RED);
        let s = b"Text box";
        let mut tb = [PointL::default(); 5];
        GpiQueryTextBox(hps, s.len() as i32, s.as_ptr(), 5, tb.as_mut_ptr());
        let o = pt(x + 4, y + 85);
        GpiSetColor(hps, CLR_BLACK);
        GpiCharStringAt(hps, &o, s.len() as i32, s.as_ptr());
        GpiSetColor(hps, CLR_RED);
        GpiMove(hps, &pt(o.x + tb[1].x, o.y + tb[1].y));
        GpiBox(hps, DRO_OUTLINE, &pt(o.x + tb[2].x, o.y + tb[2].y), 0, 0);
        GpiCreateLogFont(hps, null(), 2, fat.as_ptr());
        GpiSetCharSet(hps, 2);
        GpiSetCharBox(hps, &[20 * MAKEFIXED, 48 * MAKEFIXED]);
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, null());
        GpiSetColor(hps, 0x008000);
        GpiCharStringAt(hps, &pt(x + 4, y + 20), 6, b"Scaled".as_ptr());
        GpiSetCharSet(hps, 0);
        GpiDeleteSetId(hps, 2);
        WinEndPaint(hps);
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            unsafe { paint(h) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn main(_s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    HAB.store(hab, Ordering::Relaxed);
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    check("WinInitialize/WinCreateMsgQueue", hab != 0 && hmq != 0);
    self_checks(hab);
    meta_checks(hab);
    let pic = load_picture(b"C:\\YOSEMITE.BMP\0");
    if pic != 0 {
        check("GpiCreateBitmap from a bitmap file", true);
    }
    PICTURE.store(pic, Ordering::Relaxed);
    let f = FAILS.load(Ordering::Relaxed);
    println!("gpitest done: {f} failure(s)");
    unsafe {
        WinRegisterClass(hab, b"GpiTest\0".as_ptr(), client_proc, 4, 0);
        let mut fcf = 1 | 2 | 8; // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER
        let mut client = 0;
        let frame = WinCreateStdWindow(1, 0, &mut fcf, b"GpiTest\0".as_ptr(), b"GPI test\0".as_ptr(), 0, 0, 1, &mut client);
        let (sw, sh) = (WinQuerySysValue(1, 20), WinQuerySysValue(1, 21));
        let (w, h) = (740.min(sw), 570.min(sh));
        // SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE
        WinSetWindowPos(frame, 0, (sw - w) / 2, (sh - h) / 2, w, h, 0x1 | 0x2 | 0x8 | 0x80);
        let _ = WinOpenWindowDC(client);
        let _ = GpiAssociate(0, 0);
        let mut q = QMsg::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &q);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    f
}
