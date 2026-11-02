//! ODSHELL.EXE: a simple OpenDoc document shell on rs2's OPENDOC.DLL.
//!
//! `ODSHELL FILE [KIND [DLL CLASS]]` opens the OpenDoc document FILE (rs2's
//! container format, docs/OPENDOC.md) in a PM window: the root part's
//! frame is the window's root frame, its facet the window's root facet,
//! and the parts draw into the window's presentation space. A FILE that
//! does not exist is made: its root part is ODSHELL's own RS2TextPart,
//! holding an embedded part of KIND (another RS2TextPart if none; DLL and
//! CLASS register a part handler first, as IBM's `RxInstallParts` does).
//! Closing the window saves the document.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs, function_casts_as_integer, unused_variables, dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use os2::*;
use somsup::{ClassSpec, Environment};

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "OPENDOC.451"]
    fn ODSessionNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.412"]
    fn ODPartNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.457"]
    fn ODShapeNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.487"]
    fn ODTransformNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.810"]
    fn StorageUnitGetValue(su: usize, ev: usize, size: usize, buf: *mut u8) -> usize;
    #[link_name = "OPENDOC.906"]
    fn ODRegisterPartHandlerClass(class: *const u8, dll: *const u8, a: u32, b: u32) -> u32;

    #[link_name = "OPENDOC.444"]
    static ODRefCntObjectClassData: [usize; 5];
    #[link_name = "OPENDOC.423"]
    static ODPersistentObjectClassData: [usize; 8];
    #[link_name = "OPENDOC.411"]
    static ODPartClassData: [usize; 69];
    #[link_name = "OPENDOC.450"]
    static ODSessionClassData: [usize; 7];
    #[link_name = "OPENDOC.264"]
    static ODBaseSessionClassData: [usize; 34];
    #[link_name = "OPENDOC.465"]
    static ODStorageSystemClassData: [usize; 7];
    #[link_name = "OPENDOC.294"]
    static ODContainerClassData: [usize; 8];
    #[link_name = "OPENDOC.303"]
    static ODDocumentClassData: [usize; 14];
    #[link_name = "OPENDOC.312"]
    static ODDraftClassData: [usize; 40];
    #[link_name = "OPENDOC.468"]
    static ODStorageUnitClassData: [usize; 50];
    #[link_name = "OPENDOC.345"]
    static ODFrameClassData: [usize; 59];
    #[link_name = "OPENDOC.327"]
    static ODFacetClassData: [usize; 52];
    #[link_name = "OPENDOC.222"]
    static ODBaseCanvasClassData: [usize; 17];
    #[link_name = "OPENDOC.426"]
    static ODPlatformCanvasClassData: [usize; 5];
    #[link_name = "OPENDOC.267"]
    static ODBaseShapeClassData: [usize; 24];
    #[link_name = "OPENDOC.273"]
    static ODBaseTransformClassData: [usize; 24];
    #[link_name = "OPENDOC.510"]
    static ODWindowStateClassData: [usize; 31];
    #[link_name = "OPENDOC.504"]
    static ODWindowClassData: [usize; 26];

    #[link_name = "PMWIN.763"]
    fn WinInitialize(opts: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, cmsg: i32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, qmsg: *mut [u32; 8], hwnd: u32, first: u32, last: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, qmsg: *const [u32; 8]) -> usize;
    #[link_name = "PMWIN.877"]
    fn WinSetWindowText(hwnd: u32, text: *const u8) -> u32;
    #[link_name = "PMGPI.592"]
    fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: i32, start: i32, n: i32, table: *const i32) -> u32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, color: i32) -> u32;
    #[link_name = "PMGPI.404"]
    fn GpiMove(hps: u32, pt: *const [i32; 2]) -> u32;
    #[link_name = "PMGPI.356"]
    fn GpiBox(hps: u32, ctl: i32, pt: *const [i32; 2], h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.359"]
    fn GpiCharStringAt(hps: u32, pt: *const [i32; 2], n: i32, s: *const u8) -> i32;
}

macro_rules! cd {
    ($a:ident) => {
        unsafe { (&raw const $a) as *const usize }
    };
}

fn m(cd: *const usize, i: usize, args: &[usize]) -> usize {
    somsup::call(cd, i, args)
}

static mut EV: Environment = Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };

fn ev() -> usize {
    unsafe { &raw mut EV as usize }
}

fn release(o: usize) {
    if o != 0 {
        m(cd!(ODRefCntObjectClassData), 2, &[o, ev()]);
    }
}

const ONE: i32 = 0x10000;
const PROP_ROOT: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:Draft:RootPart\0";
const PROP_TEXT: &[u8] = b"rs2:TextPart:Text\0";
const PROP_CHILD: &[u8] = b"rs2:TextPart:EmbeddedFrame\0";
const TYPE_TEXT: &[u8] = b"rs2:Type:Text\0";
const TYPE_REF: &[u8] = b"rs2:Type:StorageUnitRef\0";
const FRAME_TYPE: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Object:Frame\0";
const VIEW_FRAME: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:Frame\0";
const PRES_NORMAL: &[u8] = b"rs2:Presentation:Normal\0";

#[repr(C)]
struct ByteArray {
    maximum: u32,
    length: u32,
    buffer: *mut u8,
}

fn ba(d: &[u8]) -> ByteArray {
    ByteArray { maximum: d.len() as u32, length: d.len() as u32, buffer: d.as_ptr() as *mut u8 }
}

fn su_get(su: usize, prop: &[u8], ty: &[u8]) -> Option<Vec<u8>> {
    let e = ev();
    let c = cd!(ODStorageUnitClassData);
    if m(c, 3, &[su, e, prop.as_ptr() as usize, ty.as_ptr() as usize, 0]) & 0xff == 0 {
        return None;
    }
    m(c, 7, &[su, e, prop.as_ptr() as usize, 0, ty.as_ptr() as usize, 0, 0]);
    let n = m(c, 30, &[su, e]);
    let mut b = alloc::vec![0u8; n];
    let got = unsafe { StorageUnitGetValue(su, e, n, b.as_mut_ptr()) };
    b.truncate(got);
    Some(b)
}

fn su_set(su: usize, prop: &[u8], ty: &[u8], data: &[u8]) {
    let e = ev();
    let c = cd!(ODStorageUnitClassData);
    if m(c, 3, &[su, e, prop.as_ptr() as usize, 0, 0]) & 0xff != 0 {
        m(c, 7, &[su, e, prop.as_ptr() as usize, 0, 0, 0, 0]);
        m(c, 16, &[su, e]);
    }
    m(c, 14, &[su, e, prop.as_ptr() as usize]);
    m(c, 15, &[su, e, ty.as_ptr() as usize]);
    let b = ba(data);
    m(c, 27, &[su, e, &b as *const ByteArray as usize]);
}

// ---- RS2TextPart: a part that shows a line of text in a box ----

#[unsafe(no_mangle)]
pub static mut RS2TextPartClassData: [usize; 4] = [0; 4];
#[unsafe(no_mangle)]
pub static mut RS2TextPartCClassData: [usize; 8] = [0; 8];

#[derive(Default)]
struct TextPart {
    text: Vec<u8>,
    /// The embedded frame's ID (0: none).
    child: usize,
    color: i32,
}

fn ccd() -> *const usize {
    unsafe { (&raw const RS2TextPartCClassData) as *const usize }
}

fn tp(this: usize) -> &'static mut TextPart {
    let slot: &mut usize = somsup::data(ccd(), this);
    if *slot == 0 {
        *slot = Box::into_raw(Box::new(TextPart::default())) as usize;
    }
    unsafe { &mut *(*slot as *mut TextPart) }
}

extern "C" fn part_init(this: usize, ev: usize, su: usize, w: usize) -> usize {
    somsup::call_parent(ccd(), cd!(ODPartClassData), 0, &[this, ev, su, w]);
    tp(this).text = b"An rs2 OpenDoc text part".to_vec();
    tp(this).color = 0xd0d0d0;
    0
}

extern "C" fn part_init_storage(this: usize, ev: usize, su: usize, w: usize) -> usize {
    somsup::call_parent(ccd(), cd!(ODPartClassData), 1, &[this, ev, su, w]);
    tp(this).color = 0xd0d0d0;
    if let Some(t) = su_get(su, PROP_TEXT, TYPE_TEXT) {
        tp(this).text = t;
    }
    if let Some(r) = su_get(su, PROP_CHILD, TYPE_REF) {
        if r.len() == 4 {
            tp(this).child = m(cd!(ODStorageUnitClassData), 37, &[su, ev, r.as_ptr() as usize]);
        }
    }
    0
}

extern "C" fn part_externalize(this: usize, ev: usize) -> usize {
    let su = m(cd!(ODPersistentObjectClassData), 4, &[this, ev]);
    if su == 0 {
        return 0;
    }
    let t = tp(this).text.clone();
    su_set(su, PROP_TEXT, TYPE_TEXT, &t);
    if tp(this).child != 0 {
        let mut r = [0u8; 4];
        m(cd!(ODStorageUnitClassData), 32, &[su, ev, tp(this).child, r.as_mut_ptr() as usize]);
        su_set(su, PROP_CHILD, TYPE_REF, &r);
    }
    0
}

/// A facet of one of the part's frames was added: the embedded frame gets
/// a facet inside it, 40 units in from the bottom left.
extern "C" fn part_facet_added(this: usize, ev: usize, facet: usize) -> usize {
    let child = tp(this).child;
    let frame = m(cd!(ODFacetClassData), 0, &[facet, ev]);
    if child == 0 || frame == 0 {
        return 0;
    }
    let su = m(cd!(ODPersistentObjectClassData), 4, &[this, ev]);
    let draft = m(cd!(ODStorageUnitClassData), 1, &[su, ev]);
    let cf = m(cd!(ODDraftClassData), 21, &[draft, ev, child]);
    if cf == 0 {
        return 0;
    }
    let t = somsup::new_object(unsafe { ODTransformNewClass(1, 0) });
    let off = [40 * ONE, 40 * ONE];
    m(cd!(ODBaseTransformClassData), 11, &[t, ev, off.as_ptr() as usize]);
    m(cd!(ODFacetClassData), 1, &[facet, ev, cf, 0, t, 0, 0, 0, 0]);
    release(t);
    0
}

extern "C" fn part_draw(this: usize, ev: usize, facet: usize, shape: usize) -> usize {
    let canvas = m(cd!(ODFacetClassData), 22, &[facet, ev]);
    let pc = if canvas != 0 { m(cd!(ODBaseCanvasClassData), 1, &[canvas, ev, 3]) } else { 0 };
    let hps = if pc != 0 { m(cd!(ODPlatformCanvasClassData), 1, &[pc, ev, facet]) as u32 } else { 0 };
    if hps == 0 {
        return 0;
    }
    let frame = m(cd!(ODFacetClassData), 0, &[facet, ev]);
    let fs = m(cd!(ODFrameClassData), 32, &[frame, ev, 0]);
    let mut r = [0i32; 4];
    m(cd!(ODBaseShapeClassData), 2, &[fs, ev, r.as_mut_ptr() as usize]);
    release(fs);
    let xf = m(cd!(ODFacetClassData), 18, &[facet, ev, 0]);
    let mut a = [r[0], r[3]];
    let mut b = [r[2], r[1]];
    m(cd!(ODBaseTransformClassData), 18, &[xf, ev, a.as_mut_ptr() as usize]);
    m(cd!(ODBaseTransformClassData), 18, &[xf, ev, b.as_mut_ptr() as usize]);
    release(xf);
    let (l, bt, rt, tp_) = (a[0] >> 16, a[1] >> 16, (b[0] >> 16) - 1, (b[1] >> 16) - 1);
    let color = tp(this).color;
    // RGB colours.
    unsafe {
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null()); // LCOLF_RGB
        GpiSetColor(hps, color);
        GpiMove(hps, &[l, bt]);
        GpiBox(hps, 1, &[rt, tp_], 0, 0); // DRO_FILL
        GpiSetColor(hps, 0);
        GpiMove(hps, &[l, bt]);
        GpiBox(hps, 2, &[rt, tp_], 0, 0); // DRO_OUTLINE
        let t = &tp(this).text;
        GpiCharStringAt(hps, &[l + 6, tp_ - 18], t.len() as i32, t.as_ptr());
    }
    0
}

extern "C" fn part_uninit(this: usize) -> usize {
    let slot: &mut usize = somsup::data(ccd(), this);
    if *slot != 0 {
        drop(unsafe { Box::from_raw(*slot as *mut TextPart) });
        *slot = 0;
    }
    somsup::call_parent(ccd(), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
    0
}

somsup::methods! {
    t_init => part_init, 4;
    t_init_storage => part_init_storage, 4;
    t_externalize => part_externalize, 2;
    t_facet_added => part_facet_added, 3;
    t_draw => part_draw, 4;
    t_uninit => part_uninit, 1;
}

fn text_part_class() -> usize {
    unsafe { ODPartNewClass(1, 0) };
    somsup::build_class(&ClassSpec {
        name: "RS2TextPart",
        parents: &["ODPart"],
        meta: None,
        major: 1,
        minor: 0,
        data: 4,
        align: 4,
        methods: &[],
        overrides: &[
            ("ODPart::InitPart", t_init as usize),
            ("ODPart::InitPartFromStorage", t_init_storage as usize),
            ("ODPersistentObject::Externalize", t_externalize as usize),
            ("ODPart::FacetAdded", t_facet_added as usize),
            ("ODPart::Draw", t_draw as usize),
            ("SOMObject::somUninit", t_uninit as usize),
        ],
        cd: unsafe { (&raw mut RS2TextPartClassData) as *mut usize },
        ccd: unsafe { (&raw mut RS2TextPartCClassData) as *mut usize },
    })
}

fn words(a: &[u8]) -> Vec<Vec<u8>> {
    a.split(|&b| b == b' ').filter(|w| !w.is_empty()).map(|w| w.to_vec()).collect()
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn main(st: &Startup) -> u32 {
    let args = words(st.args());
    let file = z(args.first().map_or(&b"C:\\ODSHELL.OD"[..], |v| &v[..]));
    let kind = args.get(1).map(|k| z(k));
    if let (Some(dll), Some(cls)) = (args.get(2), args.get(3)) {
        let (d, c) = (z(dll), z(cls));
        unsafe { ODRegisterPartHandlerClass(c.as_ptr(), d.as_ptr(), 1, 0) };
    }
    let hab = unsafe { WinInitialize(0) };
    let _hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let e = ev();
    let session = somsup::new_object(unsafe { ODSessionNewClass(1, 0) });
    m(cd!(ODSessionClassData), 0, &[session, e]);
    m(cd!(ODSessionClassData), 2, &[session, e, hab as usize]);
    text_part_class();

    let sys = m(cd!(ODBaseSessionClassData), 14, &[session, e]);
    let id = ba(&file);
    let ty = b"rs2:Container\0";
    let mut cont = m(cd!(ODStorageSystemClassData), 1, &[sys, e, ty.as_ptr() as usize, &id as *const ByteArray as usize]);
    let new = cont == 0;
    unsafe {
        if EV.major != 0 {
            somsup::somExceptionFree(&raw mut EV);
            EV.major = 0;
        }
    }
    if new {
        cont = m(cd!(ODStorageSystemClassData), 2, &[sys, e, ty.as_ptr() as usize, &id as *const ByteArray as usize]);
    }
    let doc = m(cd!(ODContainerClassData), 4, &[cont, e, 1]);
    let draft = m(cd!(ODDocumentClassData), 7, &[doc, e, 4]);
    let view = m(cd!(ODBaseSessionClassData), 30, &[session, e, VIEW_FRAME.as_ptr() as usize]);
    let pres = m(cd!(ODBaseSessionClassData), 30, &[session, e, PRES_NORMAL.as_ptr() as usize]);
    let root;
    if new {
        root = m(cd!(ODDraftClassData), 23, &[draft, e, 0, b"RS2TextPart\0".as_ptr() as usize]);
        let props = m(cd!(ODDraftClassData), 2, &[draft, e]);
        let rid = m(cd!(ODPersistentObjectClassData), 5, &[root, e]);
        let mut r = [0u8; 4];
        m(cd!(ODStorageUnitClassData), 32, &[props, e, rid, r.as_mut_ptr() as usize]);
        su_set(props, PROP_ROOT, TYPE_REF, &r);
        release(props);
    } else {
        let props = m(cd!(ODDraftClassData), 2, &[draft, e]);
        let r = su_get(props, PROP_ROOT, TYPE_REF).unwrap_or_default();
        let rid = if r.len() == 4 { m(cd!(ODStorageUnitClassData), 37, &[props, e, r.as_ptr() as usize]) } else { 0 };
        release(props);
        root = m(cd!(ODDraftClassData), 24, &[draft, e, rid]);
    }
    if root == 0 {
        println!("odshell: no root part");
        return 1;
    }

    // The window: its root frame shows the root part.
    let ws = m(cd!(ODBaseSessionClassData), 13, &[session, e]);
    let hwnd = m(cd!(ODSessionClassData), 3, &[session, e, 0x783f]);
    // RegisterWindow takes 13 words (more than somsup::call passes).
    let reg: extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize =
        unsafe { core::mem::transmute(ODWindowStateClassData[1]) };
    let win = reg(ws, e, hwnd, FRAME_TYPE.as_ptr() as usize, 1, 1, 0, 1, 1, root, view, pres, 0);
    let rf = m(cd!(ODWindowClassData), 1, &[win, e]);
    if new {
        // An embedded part in a frame of its own inside the root frame.
        let child = m(cd!(ODDraftClassData), 23, &[draft, e, kind.as_ref().map_or(0, |k| k.as_ptr() as usize),
            if kind.is_some() { 0 } else { b"RS2TextPart\0".as_ptr() as usize }]);
        if child != 0 {
            if somsup::is_a(child, unsafe { RS2TextPartClassData[0] }) {
                tp(child).text = b"An embedded part".to_vec();
                tp(child).color = 0xffff90;
            }
            let shp = somsup::new_object(unsafe { ODShapeNewClass(1, 0) });
            let r = [0, 140 * ONE, 260 * ONE, 0];
            m(cd!(ODBaseShapeClassData), 3, &[shp, e, r.as_ptr() as usize]);
            let cf = m(cd!(ODDraftClassData), 20, &[draft, e, FRAME_TYPE.as_ptr() as usize, rf, shp, 0, child, view, pres, 0, 0]);
            tp(root).child = m(cd!(ODPersistentObjectClassData), 5, &[cf, e]);
            release(shp);
            release(cf);
            release(child);
        }
    }
    let mut title = Vec::from(&b"OpenDoc - "[..]);
    title.extend_from_slice(&file[..file.len() - 1]);
    title.push(0);
    unsafe { WinSetWindowText(hwnd as u32, title.as_ptr()) };
    m(cd!(ODWindowClassData), 6, &[win, e]); // Open: the root facet
    m(cd!(ODWindowClassData), 9, &[win, e]); // Show
    m(cd!(ODWindowClassData), 22, &[win, e]); // Select
    m(cd!(ODWindowClassData), 23, &[win, e]); // Update: draw it all

    let mut q = [0u32; 8];
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    m(cd!(ODDraftClassData), 4, &[draft, e]); // save
    release(rf);
    release(root);
    0
}
