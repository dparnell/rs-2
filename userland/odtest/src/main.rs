//! Self-checks of OPENDOC.DLL, rs2's OpenDoc runtime (docs/OPENDOC.md).
//!
//! `ODTEST` makes a session, a document holding a root part with an
//! embedded part (frames, facets, shapes, transforms; the parts are this
//! program's own SOM class, an ODPart subclass built as the SOM compiler
//! would), saves it to `C:\ODTEST.OD`, closes everything, reopens the file
//! and checks what comes back. `ODTEST MMODPTS` loads IBM's multimedia
//! OpenDoc parts (MMODPTS.DLL) on rs2's OPENDOC.DLL, registers them,
//! creates an audio part in a document, saves and reopens it. Each check
//! prints PASS or FAIL; the last line is "odtest done: N failure(s)" and
//! the exit code N.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs, function_casts_as_integer, unused_unsafe, unused_variables, dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use os2::*;
use somsup::{ClassSpec, Environment};

os2::entry!(main);

// ---- OPENDOC.DLL's exports (IBM's ordinals) ----

unsafe extern "C" {
    #[link_name = "OPENDOC.451"]
    fn ODSessionNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.412"]
    fn ODPartNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.457"]
    fn ODShapeNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.487"]
    fn ODTransformNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.328"]
    fn ODFacetNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.298"]
    fn ODDispatchModuleNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "OPENDOC.300"]
    static ODDispatcherClassData: [usize; 11];
    #[link_name = "OPENDOC.504"]
    static ODWindowClassData: [usize; 26];
    #[link_name = "OPENDOC.375"]
    static ODMenuBarClassData: [usize; 24];
    #[link_name = "OPENDOC.810"]
    fn StorageUnitGetValue(su: usize, ev: usize, size: usize, buf: *mut u8) -> usize;
    #[link_name = "OPENDOC.906"]
    fn ODRegisterPartHandlerClass(class: *const u8, dll: *const u8, a: u32, b: u32) -> u32;
    #[link_name = "OPENDOC.901"]
    fn ODDeregisterPartHandlerClass(class: *const u8) -> u32;
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, cb: u32) -> u32;

    #[link_name = "OPENDOC.393"]
    static ODObjectClassData: [usize; 8];
    #[link_name = "OPENDOC.210"]
    static M_ODPartClassData: [usize; 6];
    #[link_name = "OPENDOC.444"]
    static ODRefCntObjectClassData: [usize; 5];
    #[link_name = "OPENDOC.423"]
    static ODPersistentObjectClassData: [usize; 8];
    #[link_name = "OPENDOC.411"]
    static ODPartClassData: [usize; 69];
    #[link_name = "OPENDOC.410"]
    static ODPartCClassData: [usize; 4];
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
    #[link_name = "OPENDOC.477"]
    static ODStorageUnitViewClassData: [usize; 36];
    #[link_name = "OPENDOC.345"]
    static ODFrameClassData: [usize; 59];
    #[link_name = "OPENDOC.327"]
    static ODFacetClassData: [usize; 52];
    #[link_name = "OPENDOC.348"]
    static ODFrameFacetIteratorClassData: [usize; 4];
    #[link_name = "OPENDOC.267"]
    static ODBaseShapeClassData: [usize; 24];
    #[link_name = "OPENDOC.456"]
    static ODShapeClassData: [usize; 5];
    #[link_name = "OPENDOC.273"]
    static ODBaseTransformClassData: [usize; 24];
    #[link_name = "OPENDOC.486"]
    static ODTransformClassData: [usize; 4];
    #[link_name = "OPENDOC.510"]
    static ODWindowStateClassData: [usize; 31];
    #[link_name = "OPENDOC.219"]
    static ODArbitratorClassData: [usize; 15];
    #[link_name = "OPENDOC.339"]
    static ODFocusSetClassData: [usize; 4];
}

macro_rules! cd {
    ($a:ident) => {
        unsafe { (&raw const $a) as *const usize }
    };
}

/// Call method `i` of a class through its token.
fn m(cd: *const usize, i: usize, args: &[usize]) -> usize {
    somsup::call(cd, i, args)
}

// Release order indices used here (docs/OPENDOC.md).
const REF_ACQUIRE: usize = 1;
const REF_RELEASE: usize = 2;
const REF_GET_REF_COUNT: usize = 3;
const PO_GET_STORAGE_UNIT: usize = 4;
const PO_GET_ID: usize = 5;
const SESS_INIT: usize = 0;
const BS_GET_ARBITRATOR: usize = 0;
const BS_GET_WINDOW_STATE: usize = 13;
const BS_GET_STORAGE_SYSTEM: usize = 14;
const BS_TOKENIZE: usize = 30;
const BS_GET_TYPE: usize = 32;
const SYS_ACQUIRE_CONTAINER: usize = 1;
const SYS_CREATE_CONTAINER: usize = 2;
const CONT_ACQUIRE_DOCUMENT: usize = 4;
const DOC_ACQUIRE_BASE_DRAFT: usize = 7;
const DR_ACQUIRE_DRAFT_PROPERTIES: usize = 2;
const DR_EXTERNALIZE: usize = 4;
const DR_CREATE_STORAGE_UNIT: usize = 6;
const DR_ACQUIRE_FRAME: usize = 21;
const DR_CREATE_FRAME: usize = 20;
const DR_CREATE_PART: usize = 23;
const DR_ACQUIRE_PART: usize = 24;
const SU_EXISTS: usize = 3;
const SU_COUNT_PROPERTIES: usize = 5;
const SU_COUNT_VALUES: usize = 6;
const SU_FOCUS: usize = 7;
const SU_GET_ID: usize = 11;
const SU_ADD_PROPERTY: usize = 14;
const SU_ADD_VALUE: usize = 15;
const SU_REMOVE: usize = 16;
const SU_CREATE_VIEW: usize = 18;
const SU_GET_CURRENT_VALUE_TYPE: usize = 22;
const SU_SET_OFFSET: usize = 24;
const SU_GET_VALUE: usize = 26;
const SU_SET_VALUE: usize = 27;
const SU_INSERT_VALUE: usize = 28;
const SU_GET_SIZE: usize = 30;
const SU_GET_STRONG_REF: usize = 32;
const SU_GET_ID_FROM_REF: usize = 37;
const SUV_GET_PROPERTY: usize = 12;
const SUV_GET_TYPE: usize = 13;
const SUV_GET_SIZE: usize = 21;
const FR_ACQUIRE_CONTAINING_FRAME: usize = 0;
const FR_IS_ROOT: usize = 8;
const FR_ACQUIRE_PART: usize = 18;
const FR_GET_VIEW_TYPE: usize = 22;
const FR_FACET_ADDED: usize = 28;
const FR_CREATE_FACET_ITERATOR: usize = 30;
const FR_ACQUIRE_FRAME_SHAPE: usize = 32;
const FC_GET_FRAME: usize = 0;
const FC_CREATE_EMBEDDED_FACET: usize = 1;
const FC_ACQUIRE_FRAME_TRANSFORM: usize = 16;
const FC_ACQUIRE_WINDOW_FRAME_TRANSFORM: usize = 18;
const FC_ACQUIRE_WINDOW_CONTENT_TRANSFORM: usize = 19;
const FC_CONTAINS_POINT: usize = 40;
const FC_ACQUIRE_WINDOW_AGGREGATE_CLIP_SHAPE: usize = 48;
const FC_INIT_FACET: usize = 49;
const IT_FIRST: usize = 0;
const SH_GET_BOUNDING_BOX: usize = 2;
const SH_SET_RECTANGLE: usize = 3;
const SH_IS_EMPTY: usize = 10;
const SH_CONTAINS_POINT: usize = 11;
const SH_COPY: usize = 15;
const SH_TRANSFORM: usize = 17;
const SH_INTERSECT: usize = 20;
const SH_UNION: usize = 21;
const XF_GET_TYPE: usize = 0;
const XF_SET_OFFSET: usize = 11;
const XF_SCALE_BY: usize = 13;
const XF_TRANSFORM_POINT: usize = 18;
const XF_INVERT_POINT: usize = 19;
const XF_GET_MATRIXLF: usize = 1;
const ARB_REQUEST_FOCUS: usize = 1;
const ARB_ACQUIRE_FOCUS_OWNER: usize = 6;
const ARB_CREATE_FOCUS_SET: usize = 13;
const FS_CONTAINS: usize = 2;
const FS_ADD: usize = 0;
const PART_INIT_PART: usize = 0;
const PART_INIT_PART_FROM_STORAGE: usize = 1;

const ONE: i32 = 0x10000;
const PROP_ROOT: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:Draft:RootPart\0";
const PROP_TEXT: &[u8] = b"rs2:TestPart:Text\0";
const PROP_CHILD: &[u8] = b"rs2:TestPart:EmbeddedFrame\0";
const TYPE_TEXT: &[u8] = b"rs2:Type:Text\0";
const TYPE_REF: &[u8] = b"rs2:Type:StorageUnitRef\0";
const KIND: &[u8] = b"rs2:Kind:TestPart\0";
const FRAME_TYPE: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Object:Frame\0";
const VIEW_FRAME: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:Frame\0";
const PRES_NORMAL: &[u8] = b"rs2:Presentation:Normal\0";
const FILE: &[u8] = b"C:\\ODTEST.OD\0";

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct ByteArray {
    maximum: u32,
    length: u32,
    buffer: *mut u8,
}

fn ba(data: &[u8]) -> ByteArray {
    ByteArray { maximum: data.len() as u32, length: data.len() as u32, buffer: data.as_ptr() as *mut u8 }
}

fn rect(l: i32, b: i32, r: i32, t: i32) -> Rect {
    Rect { left: l * ONE, bottom: b * ONE, right: r * ONE, top: t * ONE }
}

// ---- Checks ----

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {}", name);
    } else {
        println!("FAIL {}", name);
        unsafe { FAILS += 1 };
    }
}

fn check_eq(name: &str, got: i64, want: i64) {
    if got == want {
        println!("PASS {} = {}", name, got);
    } else {
        println!("FAIL {}: {} (expected {})", name, got, want);
        unsafe { FAILS += 1 };
    }
}

static mut EV: Environment = Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };

fn ev() -> usize {
    unsafe { &raw mut EV as usize }
}

/// Check (and clear) the environment after a step.
fn ok_ev(name: &str) {
    let e = unsafe { &mut EV };
    if e.major != 0 {
        let id = unsafe { somsup::cstr(e.exception_name) };
        let code = if e.params.is_null() { 0 } else { unsafe { *(e.params as *const i32) } };
        println!("FAIL {}: exception {} ({})", name, core::str::from_utf8(id).unwrap_or("?"), code);
        unsafe { FAILS += 1 };
        unsafe { somsup::somExceptionFree(e) };
        e.major = 0;
    }
}

fn s(p: usize) -> &'static [u8] {
    unsafe { somsup::cstr(p as *const u8) }
}

// ---- A part class of this program's own: RS2TestPart ----

#[unsafe(no_mangle)]
pub static mut RS2TestPartClassData: [usize; 4] = [0; 4];
#[unsafe(no_mangle)]
pub static mut RS2TestPartCClassData: [usize; 8] = [0; 8];

#[derive(Default)]
struct TestPart {
    text: Vec<u8>,
    /// The embedded frame's ID (its storage unit), 0 for none.
    child: usize,
    facets: u32,
    frames: u32,
    draws: u32,
    events: u32,
    last_msg: u32,
    last_frame: usize,
    last_facet: usize,
    foci: u32,
}

fn tp(this: usize) -> &'static mut TestPart {
    let slot: &mut usize = somsup::data(unsafe { (&raw const RS2TestPartCClassData) as *const usize }, this);
    if *slot == 0 {
        *slot = Box::into_raw(Box::new(TestPart::default())) as usize;
    }
    unsafe { &mut *(*slot as *mut TestPart) }
}

fn ccd() -> *const usize {
    unsafe { (&raw const RS2TestPartCClassData) as *const usize }
}

fn su_text(su: usize, prop: &[u8], ty: &[u8]) -> Option<Vec<u8>> {
    let e = ev();
    if m(cd!(ODStorageUnitClassData), SU_EXISTS, &[su, e, prop.as_ptr() as usize, ty.as_ptr() as usize, 0]) & 0xff == 0 {
        return None;
    }
    m(cd!(ODStorageUnitClassData), SU_FOCUS, &[su, e, prop.as_ptr() as usize, 0, ty.as_ptr() as usize, 0, 0]);
    let n = m(cd!(ODStorageUnitClassData), SU_GET_SIZE, &[su, e]);
    let mut buf = alloc::vec![0u8; n];
    let got = unsafe { StorageUnitGetValue(su, e, n, buf.as_mut_ptr()) };
    buf.truncate(got);
    Some(buf)
}

fn su_set(su: usize, prop: &[u8], ty: &[u8], data: &[u8]) {
    let e = ev();
    let c = cd!(ODStorageUnitClassData);
    if m(c, SU_EXISTS, &[su, e, prop.as_ptr() as usize, 0, 0]) & 0xff != 0 {
        m(c, SU_FOCUS, &[su, e, prop.as_ptr() as usize, 0, 0, 0, 0]);
        m(c, SU_REMOVE, &[su, e]);
    }
    m(c, SU_ADD_PROPERTY, &[su, e, prop.as_ptr() as usize]);
    m(c, SU_ADD_VALUE, &[su, e, ty.as_ptr() as usize]);
    let b = ba(data);
    m(c, SU_SET_VALUE, &[su, e, &b as *const ByteArray as usize]);
}

fn read_part(this: usize, su: usize) {
    let e = ev();
    if let Some(t) = su_text(su, PROP_TEXT, TYPE_TEXT) {
        tp(this).text = t;
    }
    if let Some(r) = su_text(su, PROP_CHILD, TYPE_REF) {
        if r.len() == 4 {
            tp(this).child = m(cd!(ODStorageUnitClassData), SU_GET_ID_FROM_REF, &[su, e, r.as_ptr() as usize]);
        }
    }
}

extern "C" fn part_init(this: usize, ev: usize, su: usize, wrapper: usize) -> usize {
    somsup::call_parent(ccd(), cd!(ODPartClassData), PART_INIT_PART, &[this, ev, su, wrapper]);
    tp(this).text = b"new".to_vec();
    0
}

extern "C" fn part_init_from_storage(this: usize, ev: usize, su: usize, wrapper: usize) -> usize {
    somsup::call_parent(ccd(), cd!(ODPartClassData), PART_INIT_PART_FROM_STORAGE, &[this, ev, su, wrapper]);
    read_part(this, su);
    0
}

extern "C" fn part_externalize(this: usize, ev: usize) -> usize {
    let su = m(cd!(ODPersistentObjectClassData), PO_GET_STORAGE_UNIT, &[this, ev]);
    if su == 0 {
        return 0;
    }
    let t = tp(this).text.clone();
    su_set(su, PROP_TEXT, TYPE_TEXT, &t);
    let child = tp(this).child;
    if child != 0 {
        let mut r = [0u8; 4];
        m(cd!(ODStorageUnitClassData), SU_GET_STRONG_REF, &[su, ev, child, r.as_mut_ptr() as usize]);
        su_set(su, PROP_CHILD, TYPE_REF, &r);
    }
    0
}

extern "C" fn part_frame_added(this: usize, ev: usize, frame: usize) -> usize {
    tp(this).frames += 1;
    0
}

extern "C" fn part_facet_added(this: usize, ev: usize, facet: usize) -> usize {
    tp(this).facets += 1;
    0
}

extern "C" fn part_draw(this: usize, ev: usize, facet: usize, shape: usize) -> usize {
    tp(this).draws += 1;
    0
}

/// HandleEvent: every event but a menu command is the part's.
extern "C" fn part_handle_event(this: usize, ev: usize, event: usize, frame: usize, facet: usize, info: usize) -> usize {
    let q = unsafe { *(event as *const [u32; 8]) };
    let t = tp(this);
    t.events += 1;
    t.last_msg = q[1];
    t.last_frame = frame;
    t.last_facet = facet;
    (q[1] != 0x20) as usize
}

extern "C" fn part_focus_acquired(this: usize, ev: usize, focus: usize, frame: usize) -> usize {
    tp(this).foci += 1;
    0
}

static mut MODULE_EVENTS: u32 = 0;
static mut MODULE_LAST: [u32; 8] = [0; 8];

/// RS2TestModule's Dispatch: the menu commands no part took.
extern "C" fn module_dispatch(this: usize, ev: usize, event: usize, info: usize) -> usize {
    unsafe {
        MODULE_EVENTS += 1;
        MODULE_LAST = *(event as *const [u32; 8]);
    }
    1
}

#[unsafe(no_mangle)]
pub static mut RS2TestModuleClassData: [usize; 4] = [0; 4];
#[unsafe(no_mangle)]
pub static mut RS2TestModuleCClassData: [usize; 8] = [0; 8];

fn test_module() -> usize {
    unsafe { ODDispatchModuleNewClass(1, 0) };
    let cls = somsup::build_class(&ClassSpec {
        name: "RS2TestModule",
        parents: &["ODDispatchModule"],
        meta: None,
        major: 1,
        minor: 0,
        data: 0,
        align: 0,
        methods: &[],
        overrides: &[("ODDispatchModule::Dispatch", t_module as usize)],
        cd: unsafe { (&raw mut RS2TestModuleClassData) as *mut usize },
        ccd: unsafe { (&raw mut RS2TestModuleCClassData) as *mut usize },
    });
    somsup::new_object(cls)
}

/// Dispatch a PM message (an OS/2 ODEvent) with no window handle.
fn dispatch(d: usize, msg: u32, mp1: u32, mp2: u32) -> bool {
    let q = [0u32, msg, mp1, mp2, 0, 0, 0, 0];
    m(cd!(ODDispatcherClassData), 6, &[d, ev(), q.as_ptr() as usize]) & 0xff != 0
}

fn pt(x: i32, y: i32) -> u32 {
    (x as u16 as u32) | ((y as u16 as u32) << 16)
}

fn events_and_menus(session: usize, rf: usize, cf: usize, root: usize, child: usize) {
    let e = ev();
    let ws = m(cd!(ODBaseSessionClassData), BS_GET_WINDOW_STATE, &[session, e]);
    // A window with no PM window: its root facet is made by Open.
    let w = m(cd!(ODWindowStateClassData), 1, &[ws, e, 0, rf, 1, 1, 0, 1, 1, 0]);
    m(cd!(ODWindowClassData), 6, &[w, e]);
    let rfc = m(cd!(ODWindowClassData), 2, &[w, e]);
    check("RegisterWindowForFrame and Open make the root facet", w != 0 && rfc != 0);
    let ext = somsup::new_object(unsafe { ODTransformNewClass(1, 0) });
    let off = Point { x: 10 * ONE, y: 20 * ONE };
    m(cd!(ODBaseTransformClassData), XF_SET_OFFSET, &[ext, e, &off as *const Point as usize]);
    let cfc = m(cd!(ODFacetClassData), FC_CREATE_EMBEDDED_FACET, &[rfc, e, cf, 0, ext, 0, 0, 0, 0]);
    release(ext);
    let d = m(cd!(ODBaseSessionClassData), 6, &[session, e]);
    check("the session's dispatcher", d != 0);
    let module = test_module();
    m(cd!(ODDispatcherClassData), 1, &[d, e, 0x20, module]);
    let arb = m(cd!(ODBaseSessionClassData), BS_GET_ARBITRATOR, &[session, e]);
    let sel = tok(session, b"Selection\0");
    let key = tok(session, b"Key\0");
    let menu = tok(session, b"Menu\0");
    let owner = |f: usize| {
        let o = m(cd!(ODArbitratorClassData), ARB_ACQUIRE_FOCUS_OWNER, &[arb, e, f]);
        release(o);
        o
    };
    // A click in the root part only.
    let handled = dispatch(d, 0x71, pt(150, 80), 0);
    check("a click outside the embedded facet goes to the root part", handled && tp(root).last_msg == 0x71 && tp(root).last_frame == rf && tp(root).last_facet == rfc);
    check("and activates the root frame", owner(sel) == rf && owner(key) == rf);
    // A click in the embedded part's facet (10..60, 20..60 in the window).
    tp(child).foci = 0;
    let handled = dispatch(d, 0x71, pt(30, 40), 0);
    check("mouse down goes to the part under the pointer", handled && tp(child).last_msg == 0x71 && tp(child).last_frame == cf && tp(child).last_facet == cfc);
    check("the click activated the embedded frame (selection, menu, key foci)", owner(sel) == cf && owner(menu) == cf && owner(key) == cf && tp(child).foci == 3);
    let n = tp(child).events;
    dispatch(d, 0x7a, 0x41, 0);
    check("WM_CHAR goes to the frame with the key focus", tp(child).events == n + 1 && tp(child).last_msg == 0x7a);
    let n = tp(child).events;
    let handled = dispatch(d, 0x20, 0x5100, 0);
    check("a menu command goes to the menu focus's part, then (not taken) to the dispatch module", handled && tp(child).events == n + 1 && tp(child).last_msg == 0x20 && unsafe { MODULE_EVENTS } == 1 && unsafe { MODULE_LAST[2] } == 0x5100);
    check("an event nobody handles is not handled", !dispatch(d, 0x1234, 0, 0));
    let moved = dispatch(d, 0x70, pt(500, 500), 0);
    check("mouse outside every facet is not dispatched", !moved);

    // Menus.
    let mb = m(cd!(ODWindowStateClassData), 18, &[ws, e, 0]);
    let c = cd!(ODMenuBarClassData);
    m(c, 2, &[mb, e, 0x0e, b"~Document\0".as_ptr() as usize, 0]);
    m(c, 2, &[mb, e, 0x11, b"~Edit\0".as_ptr() as usize, 0]);
    let mi = [0xffffu16, 1, 0, 0x5100, 0, 0, 0, 0];
    m(c, 14, &[mb, e, 0x0e, 0x5100, mi.as_ptr() as usize]);
    m(c, 7, &[mb, e, 0x0e, 0x5100, b"~Save\0".as_ptr() as usize]);
    let mi2 = [0xffffu16, 1, 0, 0x5016, 0, 0, 0, 0];
    m(c, 14, &[mb, e, 0x11, 0x5016, mi2.as_ptr() as usize]);
    check("menu items exist", m(c, 16, &[mb, e, 0x0e, 0x5100]) & 0xff != 0 && m(c, 16, &[mb, e, 0, 0x11]) & 0xff != 0 && m(c, 16, &[mb, e, 0x0e, 0x5016]) & 0xff == 0);
    check("GetMenuItemText", s(m(c, 8, &[mb, e, 0x0e, 0x5100])) == b"~Save");
    m(c, 12, &[mb, e, 0x11, 0x5016, 0]);
    m(c, 11, &[mb, e, 0x0e, 0x5100, 1]);
    check("EnableMenuItem / CheckMenuItem", m(c, 17, &[mb, e, 0x11, 0x5016]) & 0xff == 0 && m(c, 18, &[mb, e, 0x0e, 0x5100]) & 0xff != 0);
    let cp = m(c, 1, &[mb, e]);
    check("a copied menu bar keeps its items", m(c, 16, &[cp, e, 0x11, 0x5016]) & 0xff != 0 && m(c, 17, &[cp, e, 0x11, 0x5016]) & 0xff == 0);
    m(c, 21, &[cp, e, 0x11, 0x5016]);
    check("RemoveMenuItem (on the copy only)", m(c, 16, &[cp, e, 0x11, 0x5016]) & 0xff == 0 && m(c, 16, &[mb, e, 0x11, 0x5016]) & 0xff != 0);
    m(cd!(ODWindowStateClassData), 15, &[ws, e, mb]);
    let base = m(cd!(ODWindowStateClassData), 16, &[ws, e]);
    check("SetBaseMenuBar / CopyBaseMenuBar", base != 0 && base != mb && m(c, 16, &[base, e, 0x0e, 0x5100]) & 0xff != 0);
    release(base);
    release(cp);
    ok_ev("events and menus");
}

extern "C" fn part_uninit(this: usize) -> usize {
    let slot: &mut usize = somsup::data(ccd(), this);
    if *slot != 0 {
        drop(unsafe { Box::from_raw(*slot as *mut TestPart) });
        *slot = 0;
    }
    somsup::call_parent(ccd(), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
    0
}

somsup::methods! {
    t_init => part_init, 4;
    t_init_storage => part_init_from_storage, 4;
    t_externalize => part_externalize, 2;
    t_frame_added => part_frame_added, 3;
    t_facet_added => part_facet_added, 3;
    t_draw => part_draw, 4;
    t_uninit => part_uninit, 1;
    t_handle => part_handle_event, 6;
    t_focus => part_focus_acquired, 4;
    t_module => module_dispatch, 4;
}

fn test_part_class() -> usize {
    unsafe { ODPartNewClass(1, 0) };
    somsup::build_class(&ClassSpec {
        name: "RS2TestPart",
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
            ("ODPart::DisplayFrameAdded", t_frame_added as usize),
            ("ODPart::DisplayFrameConnected", t_frame_added as usize),
            ("ODPart::FacetAdded", t_facet_added as usize),
            ("ODPart::Draw", t_draw as usize),
            ("SOMObject::somUninit", t_uninit as usize),
            ("ODPart::HandleEvent", t_handle as usize),
            ("ODPart::FocusAcquired", t_focus as usize),
        ],
        cd: unsafe { (&raw mut RS2TestPartClassData) as *mut usize },
        ccd: unsafe { (&raw mut RS2TestPartCClassData) as *mut usize },
    })
}

// ---- The checks ----

fn new_session() -> usize {
    let cls = unsafe { ODSessionNewClass(1, 0) };
    let s = somsup::new_object(cls);
    m(cd!(ODSessionClassData), SESS_INIT, &[s, ev()]);
    s
}

fn tok(session: usize, t: &[u8]) -> usize {
    m(cd!(ODBaseSessionClassData), BS_TOKENIZE, &[session, ev(), t.as_ptr() as usize])
}

fn release(o: usize) {
    if o != 0 {
        m(cd!(ODRefCntObjectClassData), REF_RELEASE, &[o, ev()]);
    }
}

fn shapes_and_transforms() {
    let e = ev();
    let sc = unsafe { ODShapeNewClass(1, 0) };
    let a = somsup::new_object(sc);
    m(cd!(ODShapeClassData), 0, &[a, e]);
    let r = rect(0, 0, 100, 50);
    m(cd!(ODBaseShapeClassData), SH_SET_RECTANGLE, &[a, e, &r as *const Rect as usize]);
    let b = m(cd!(ODBaseShapeClassData), SH_COPY, &[a, e]);
    let r2 = rect(50, 25, 150, 75);
    m(cd!(ODBaseShapeClassData), SH_SET_RECTANGLE, &[b, e, &r2 as *const Rect as usize]);
    let i = m(cd!(ODBaseShapeClassData), SH_COPY, &[a, e]);
    m(cd!(ODBaseShapeClassData), SH_INTERSECT, &[i, e, b]);
    let mut bb = Rect::default();
    m(cd!(ODBaseShapeClassData), SH_GET_BOUNDING_BOX, &[i, e, &mut bb as *mut Rect as usize]);
    check("shape Intersect", bb == rect(50, 25, 100, 50));
    m(cd!(ODBaseShapeClassData), SH_UNION, &[i, e, a]);
    m(cd!(ODBaseShapeClassData), SH_UNION, &[i, e, b]);
    m(cd!(ODBaseShapeClassData), SH_GET_BOUNDING_BOX, &[i, e, &mut bb as *mut Rect as usize]);
    check("shape Union", bb == rect(0, 0, 150, 75));
    let p = Point { x: 10 * ONE, y: 10 * ONE };
    check("shape ContainsPoint", m(cd!(ODBaseShapeClassData), SH_CONTAINS_POINT, &[a, e, &p as *const Point as usize]) & 0xff != 0);
    check("shape not IsEmpty", m(cd!(ODBaseShapeClassData), SH_IS_EMPTY, &[a, e]) & 0xff == 0);

    let tc = unsafe { ODTransformNewClass(1, 0) };
    let t = somsup::new_object(tc);
    m(cd!(ODTransformClassData), 0, &[t, e]);
    check_eq("identity GetType", m(cd!(ODBaseTransformClassData), XF_GET_TYPE, &[t, e]) as i64, 0);
    let off = Point { x: 20 * ONE, y: 30 * ONE };
    m(cd!(ODBaseTransformClassData), XF_SET_OFFSET, &[t, e, &off as *const Point as usize]);
    let sc2 = Point { x: 2 * ONE, y: 2 * ONE };
    m(cd!(ODBaseTransformClassData), XF_SCALE_BY, &[t, e, &sc2 as *const Point as usize]);
    check_eq("scale+offset GetType", m(cd!(ODBaseTransformClassData), XF_GET_TYPE, &[t, e]) as i64, 3);
    let mut q = Point { x: 5 * ONE, y: 5 * ONE };
    m(cd!(ODBaseTransformClassData), XF_TRANSFORM_POINT, &[t, e, &mut q as *mut Point as usize]);
    check("TransformPoint (5,5) -> (50,70)", q == Point { x: 50 * ONE, y: 70 * ONE });
    m(cd!(ODBaseTransformClassData), XF_INVERT_POINT, &[t, e, &mut q as *mut Point as usize]);
    check("InvertPoint back to (5,5)", q == Point { x: 5 * ONE, y: 5 * ONE });
    let mut mx = [0i32; 9];
    m(cd!(ODTransformClassData), XF_GET_MATRIXLF, &[t, e, mx.as_mut_ptr() as usize]);
    check("GetMATRIXLF", mx[0] == 2 * ONE && mx[4] == 2 * ONE && mx[6] == 40 && mx[7] == 60 && mx[8] == 1);
    m(cd!(ODBaseShapeClassData), SH_TRANSFORM, &[a, e, t]);
    m(cd!(ODBaseShapeClassData), SH_GET_BOUNDING_BOX, &[a, e, &mut bb as *mut Rect as usize]);
    check("shape Transform", bb == rect(40, 60, 240, 160));
    for o in [a, b, i, t] {
        release(o);
    }
    ok_ev("shapes and transforms");
}

fn storage_unit_api(draft: usize) {
    let e = ev();
    let c = cd!(ODStorageUnitClassData);
    let su = m(cd!(ODDraftClassData), DR_CREATE_STORAGE_UNIT, &[draft, e]);
    check("CreateStorageUnit", su != 0);
    su_set(su, b"rs2:P1\0", b"rs2:T1\0", b"hello");
    m(c, SU_ADD_VALUE, &[su, e, b"rs2:T2\0".as_ptr() as usize]);
    let b2 = ba(b"world");
    m(c, SU_SET_VALUE, &[su, e, &b2 as *const ByteArray as usize]);
    su_set(su, b"rs2:P2\0", b"rs2:T1\0", b"x");
    check_eq("CountProperties", m(c, SU_COUNT_PROPERTIES, &[su, e]) as i64, 2);
    m(c, SU_FOCUS, &[su, e, b"rs2:P1\0".as_ptr() as usize, 0, 0, 0, 0]);
    check_eq("CountValues", m(c, SU_COUNT_VALUES, &[su, e]) as i64, 2);
    m(c, SU_FOCUS, &[su, e, b"rs2:P1\0".as_ptr() as usize, 0, 0, 2, 0]);
    let t = m(c, SU_GET_CURRENT_VALUE_TYPE, &[su, e]);
    check("GetCurrentValueType", s(t) == b"rs2:T2");
    somsup::free(t as *mut u8);
    m(c, SU_FOCUS, &[su, e, b"rs2:P1\0".as_ptr() as usize, 0, b"rs2:T1\0".as_ptr() as usize, 0, 0]);
    m(c, SU_SET_OFFSET, &[su, e, 5]);
    let b3 = ba(b", there");
    m(c, SU_INSERT_VALUE, &[su, e, &b3 as *const ByteArray as usize]);
    check("InsertValue", su_text(su, b"rs2:P1\0", b"rs2:T1\0").as_deref() == Some(b"hello, there"));
    m(c, SU_FOCUS, &[su, e, b"rs2:P1\0".as_ptr() as usize, 0, b"rs2:T1\0".as_ptr() as usize, 0, 0]);
    m(c, SU_SET_OFFSET, &[su, e, 7]);
    let mut out = ByteArray { maximum: 0, length: 0, buffer: core::ptr::null_mut() };
    let n = m(c, SU_GET_VALUE, &[su, e, 3, &mut out as *mut ByteArray as usize]);
    check("GetValue at an offset", n == 3 && !out.buffer.is_null() && unsafe { core::slice::from_raw_parts(out.buffer, 3) } == b"the");
    somsup::free(out.buffer);
    let v = m(c, SU_CREATE_VIEW, &[su, e]);
    check("CreateView keeps the focus", s(m(cd!(ODStorageUnitViewClassData), SUV_GET_PROPERTY, &[v, e])) == b"rs2:P1"
        && s(m(cd!(ODStorageUnitViewClassData), SUV_GET_TYPE, &[v, e])) == b"rs2:T1"
        && m(cd!(ODStorageUnitViewClassData), SUV_GET_SIZE, &[v, e]) == 12);
    somsup::free_object(v);
    m(c, SU_FOCUS, &[su, e, b"rs2:P2\0".as_ptr() as usize, 0, 0, 0, 0]);
    m(c, SU_REMOVE, &[su, e]);
    check_eq("Remove property", m(c, SU_COUNT_PROPERTIES, &[su, e]) as i64, 1);
    m(c, SU_FOCUS, &[su, e, b"rs2:nothing\0".as_ptr() as usize, 0, 0, 0, 0]);
    let failed = unsafe { EV.major } != 0;
    check("Focus on a missing property raises ODException", failed && s(unsafe { EV.exception_name } as usize) == b"ODException");
    unsafe { somsup::somExceptionFree(&raw mut EV) };
    unsafe { EV.major = 0 };
    let id = m(c, SU_GET_ID, &[su, e]);
    let mut r = [0u8; 4];
    m(c, SU_GET_STRONG_REF, &[su, e, id, r.as_mut_ptr() as usize]);
    check_eq("storage unit ref round trip", m(c, SU_GET_ID_FROM_REF, &[su, e, r.as_ptr() as usize]) as i64, id as i64);
    release(su);
    ok_ev("storage unit API");
}

fn create_document(session: usize, part_class: &str) -> Option<(usize, usize, usize)> {
    let e = ev();
    let sys = m(cd!(ODBaseSessionClassData), BS_GET_STORAGE_SYSTEM, &[session, e]);
    let id = ba(FILE);
    let cont = m(cd!(ODStorageSystemClassData), SYS_CREATE_CONTAINER, &[sys, e, b"rs2:Container\0".as_ptr() as usize, &id as *const ByteArray as usize]);
    check("CreateContainer", cont != 0);
    let doc = m(cd!(ODContainerClassData), CONT_ACQUIRE_DOCUMENT, &[cont, e, 1]);
    let draft = m(cd!(ODDocumentClassData), DOC_ACQUIRE_BASE_DRAFT, &[doc, e, 4]);
    check("document and base draft", doc != 0 && draft != 0);
    ok_ev("create document");
    if draft == 0 {
        return None;
    }
    let _ = part_class;
    Some((cont, doc, draft))
}

fn main(st: &Startup) -> u32 {
    let args = st.args();
    let args: Vec<u8> = args.iter().copied().skip_while(|&b| b == b' ').collect();
    if args.len() >= 7 && args[..7].eq_ignore_ascii_case(b"MMODPTS") {
        mmodpts();
    } else {
        basic();
    }
    let n = unsafe { FAILS };
    println!("odtest done: {} failure(s)", n);
    n
}

fn basic() {
    let e = ev();
    let session = new_session();
    check("session", session != 0);
    let t1 = tok(session, VIEW_FRAME);
    let t2 = tok(session, PRES_NORMAL);
    check("Tokenize gives the same token again", t1 != 0 && t1 != t2 && tok(session, VIEW_FRAME) == t1);
    let mut name = 0usize;
    let got = m(cd!(ODBaseSessionClassData), BS_GET_TYPE, &[session, e, t2, &mut name as *mut usize as usize]);
    check("GetType", got & 0xff != 0 && s(name) == &PRES_NORMAL[..PRES_NORMAL.len() - 1]);
    let arb = m(cd!(ODBaseSessionClassData), BS_GET_ARBITRATOR, &[session, e]);
    let ws = m(cd!(ODBaseSessionClassData), BS_GET_WINDOW_STATE, &[session, e]);
    check("arbitrator and window state", arb != 0 && ws != 0);
    shapes_and_transforms();

    let cls = test_part_class();
    check("RS2TestPart built on ODPart", cls != 0 && somsup::is_a(somsup::new_object(cls), unsafe { ODPartClassData[0] }));

    // A document: root part, embedded part, frames and facets.
    let Some((cont, doc, draft)) = create_document(session, "RS2TestPart") else { return };
    storage_unit_api(draft);
    let root = m(cd!(ODDraftClassData), DR_CREATE_PART, &[draft, e, KIND.as_ptr() as usize, b"RS2TestPart\0".as_ptr() as usize]);
    ok_ev("CreatePart root");
    let child = m(cd!(ODDraftClassData), DR_CREATE_PART, &[draft, e, KIND.as_ptr() as usize, b"RS2TestPart\0".as_ptr() as usize]);
    ok_ev("CreatePart child");
    check("CreatePart made RS2TestParts", root != 0 && child != 0 && tp(root).text == b"new");
    if root == 0 || child == 0 {
        return;
    }
    tp(root).text = b"the root part".to_vec();
    tp(child).text = b"an embedded part".to_vec();
    let sc = unsafe { ODShapeNewClass(1, 0) };
    let shp = somsup::new_object(sc);
    let r = rect(0, 0, 200, 100);
    m(cd!(ODBaseShapeClassData), SH_SET_RECTANGLE, &[shp, e, &r as *const Rect as usize]);
    let view = tok(session, VIEW_FRAME);
    let pres = tok(session, PRES_NORMAL);
    let rf = m(cd!(ODDraftClassData), DR_CREATE_FRAME, &[draft, e, FRAME_TYPE.as_ptr() as usize, 0, shp, 0, root, view, pres, 0, 0]);
    let r2 = rect(0, 0, 50, 40);
    m(cd!(ODBaseShapeClassData), SH_SET_RECTANGLE, &[shp, e, &r2 as *const Rect as usize]);
    let cf = m(cd!(ODDraftClassData), DR_CREATE_FRAME, &[draft, e, FRAME_TYPE.as_ptr() as usize, rf, shp, 0, child, view, pres, 0, 0]);
    ok_ev("CreateFrame");
    check("CreateFrame told the parts (DisplayFrameAdded)", rf != 0 && cf != 0 && tp(root).frames == 1 && tp(child).frames == 1);
    check("root frame IsRoot, embedded one is not", m(cd!(ODFrameClassData), FR_IS_ROOT, &[rf, e]) & 0xff != 0 && m(cd!(ODFrameClassData), FR_IS_ROOT, &[cf, e]) & 0xff == 0);
    tp(root).child = m(cd!(ODPersistentObjectClassData), PO_GET_ID, &[cf, e]);

    // Facets: the root facet at (100,100) in its window, the embedded one at (10,20) in the root's content.
    let fcls = unsafe { ODFacetNewClass(1, 0) };
    let rfc = somsup::new_object(fcls);
    let tcls = unsafe { ODTransformNewClass(1, 0) };
    let ext = somsup::new_object(tcls);
    let off = Point { x: 100 * ONE, y: 100 * ONE };
    m(cd!(ODBaseTransformClassData), XF_SET_OFFSET, &[ext, e, &off as *const Point as usize]);
    m(cd!(ODFacetClassData), FC_INIT_FACET, &[rfc, e, rf, 0, ext, 0, 0]);
    m(cd!(ODFrameClassData), FR_FACET_ADDED, &[rf, e, rfc]);
    let ext2 = somsup::new_object(tcls);
    let off2 = Point { x: 10 * ONE, y: 20 * ONE };
    m(cd!(ODBaseTransformClassData), XF_SET_OFFSET, &[ext2, e, &off2 as *const Point as usize]);
    let cfc = m(cd!(ODFacetClassData), FC_CREATE_EMBEDDED_FACET, &[rfc, e, cf, 0, ext2, 0, 0, 0, 0]);
    ok_ev("facets");
    check("FacetAdded reached both parts", tp(root).facets == 1 && tp(child).facets == 1);
    check("facet GetFrame", m(cd!(ODFacetClassData), FC_GET_FRAME, &[cfc, e]) == cf);
    let wf = m(cd!(ODFacetClassData), FC_ACQUIRE_WINDOW_FRAME_TRANSFORM, &[cfc, e, 0]);
    let mut p = Point { x: 5 * ONE, y: 5 * ONE };
    m(cd!(ODBaseTransformClassData), XF_TRANSFORM_POINT, &[wf, e, &mut p as *mut Point as usize]);
    check("embedded facet's window frame transform (5,5) -> (115,125)", p == Point { x: 115 * ONE, y: 125 * ONE });
    let ft = m(cd!(ODFacetClassData), FC_ACQUIRE_FRAME_TRANSFORM, &[cfc, e, 0]);
    let mut p2 = Point { x: 0, y: 0 };
    m(cd!(ODBaseTransformClassData), XF_TRANSFORM_POINT, &[ft, e, &mut p2 as *mut Point as usize]);
    check("embedded facet's frame transform", p2 == off2);
    let wc = m(cd!(ODFacetClassData), FC_ACQUIRE_WINDOW_CONTENT_TRANSFORM, &[rfc, e, 0]);
    let mut p3 = Point { x: 1 * ONE, y: 2 * ONE };
    m(cd!(ODBaseTransformClassData), XF_TRANSFORM_POINT, &[wc, e, &mut p3 as *mut Point as usize]);
    check("root facet's window content transform", p3 == Point { x: 101 * ONE, y: 102 * ONE });
    let inside = Point { x: 120 * ONE, y: 130 * ONE };
    let outside = Point { x: 170 * ONE, y: 130 * ONE };
    check("ContainsPoint (window coordinates)", m(cd!(ODFacetClassData), FC_CONTAINS_POINT, &[cfc, e, &inside as *const Point as usize, 0]) & 0xff != 0
        && m(cd!(ODFacetClassData), FC_CONTAINS_POINT, &[cfc, e, &outside as *const Point as usize, 0]) & 0xff == 0);
    let clip = m(cd!(ODFacetClassData), FC_ACQUIRE_WINDOW_AGGREGATE_CLIP_SHAPE, &[cfc, e, 0]);
    let mut bb = Rect::default();
    m(cd!(ODBaseShapeClassData), SH_GET_BOUNDING_BOX, &[clip, e, &mut bb as *mut Rect as usize]);
    check("window aggregate clip shape", bb == rect(110, 120, 160, 160));
    let it = m(cd!(ODFrameClassData), FR_CREATE_FACET_ITERATOR, &[cf, e]);
    check("frame facet iterator", m(cd!(ODFrameFacetIteratorClassData), IT_FIRST, &[it, e]) == cfc);
    somsup::free_object(it);
    for o in [wf, ft, wc, clip, ext, ext2, shp] {
        release(o);
    }

    // The arbitrator.
    let sel = tok(session, b"Selection\0");
    check("RequestFocus", m(cd!(ODArbitratorClassData), ARB_REQUEST_FOCUS, &[arb, e, sel, cf]) & 0xff != 0);
    let owner = m(cd!(ODArbitratorClassData), ARB_ACQUIRE_FOCUS_OWNER, &[arb, e, sel]);
    check("AcquireFocusOwner", owner == cf);
    release(owner);
    let set = m(cd!(ODArbitratorClassData), ARB_CREATE_FOCUS_SET, &[arb, e]);
    m(cd!(ODFocusSetClassData), FS_ADD, &[set, e, sel]);
    check("focus set Contains", m(cd!(ODFocusSetClassData), FS_CONTAINS, &[set, e, sel]) & 0xff != 0);
    somsup::free_object(set);
    events_and_menus(session, rf, cf, root, child);

    // The root part's storage unit goes in the draft's properties; save.
    let props = m(cd!(ODDraftClassData), DR_ACQUIRE_DRAFT_PROPERTIES, &[draft, e]);
    let rsu = m(cd!(ODPersistentObjectClassData), PO_GET_STORAGE_UNIT, &[root, e]);
    let rid = m(cd!(ODStorageUnitClassData), SU_GET_ID, &[rsu, e]);
    let mut r = [0u8; 4];
    m(cd!(ODStorageUnitClassData), SU_GET_STRONG_REF, &[props, e, rid, r.as_mut_ptr() as usize]);
    su_set(props, PROP_ROOT, TYPE_REF, &r);
    release(props);
    m(cd!(ODDraftClassData), DR_EXTERNALIZE, &[draft, e]);
    ok_ev("draft Externalize");
    let size = file_size(FILE);
    check("the document file was written", size > 0);
    println!("     ({} bytes)", size);

    // Close everything.
    release(root);
    release(child);
    release(rf);
    release(cf);
    release(draft);
    release(doc);
    release(cont);
    ok_ev("close");

    // Reopen.
    let sys = m(cd!(ODBaseSessionClassData), BS_GET_STORAGE_SYSTEM, &[session, e]);
    let id = ba(FILE);
    let cont = m(cd!(ODStorageSystemClassData), SYS_ACQUIRE_CONTAINER, &[sys, e, b"rs2:Container\0".as_ptr() as usize, &id as *const ByteArray as usize]);
    ok_ev("AcquireContainer");
    check("AcquireContainer read the file", cont != 0);
    if cont == 0 {
        return;
    }
    let doc = m(cd!(ODContainerClassData), CONT_ACQUIRE_DOCUMENT, &[cont, e, 1]);
    let draft = m(cd!(ODDocumentClassData), DOC_ACQUIRE_BASE_DRAFT, &[doc, e, 4]);
    let props = m(cd!(ODDraftClassData), DR_ACQUIRE_DRAFT_PROPERTIES, &[draft, e]);
    let rr = su_text(props, PROP_ROOT, TYPE_REF).unwrap_or_default();
    let rid2 = if rr.len() == 4 { m(cd!(ODStorageUnitClassData), SU_GET_ID_FROM_REF, &[props, e, rr.as_ptr() as usize]) } else { 0 };
    check("the root part reference came back", rid2 == rid);
    let root = m(cd!(ODDraftClassData), DR_ACQUIRE_PART, &[draft, e, rid2]);
    ok_ev("AcquirePart");
    check("root part read back (InitPartFromStorage)", root != 0 && tp(root).text == b"the root part");
    let cfid = if root != 0 { tp(root).child } else { 0 };
    let cf = m(cd!(ODDraftClassData), DR_ACQUIRE_FRAME, &[draft, e, cfid]);
    ok_ev("AcquireFrame");
    check("embedded frame read back", cf != 0);
    if cf != 0 {
        let child = m(cd!(ODFrameClassData), FR_ACQUIRE_PART, &[cf, e]);
        check("embedded part read back through its frame", child != 0 && tp(child).text == b"an embedded part" && tp(child).frames == 1);
        let fs = m(cd!(ODFrameClassData), FR_ACQUIRE_FRAME_SHAPE, &[cf, e, 0]);
        m(cd!(ODBaseShapeClassData), SH_GET_BOUNDING_BOX, &[fs, e, &mut bb as *mut Rect as usize]);
        check("embedded frame shape", bb == rect(0, 0, 50, 40));
        release(fs);
        let vt = m(cd!(ODFrameClassData), FR_GET_VIEW_TYPE, &[cf, e]);
        check("embedded frame view type", vt == tok(session, VIEW_FRAME));
        let rf = m(cd!(ODFrameClassData), FR_ACQUIRE_CONTAINING_FRAME, &[cf, e]);
        check("containing frame read back", rf != 0 && m(cd!(ODFrameClassData), FR_IS_ROOT, &[rf, e]) & 0xff != 0);
        let rp = if rf != 0 { m(cd!(ODFrameClassData), FR_ACQUIRE_PART, &[rf, e]) } else { 0 };
        check("containing frame's part is the root part", rp == root);
        release(rp);
        release(rf);
        release(child);
    }
    release(cf);
    release(root);
    release(props);
    release(draft);
    release(doc);
    release(cont);
    ok_ev("reopen");
    let _ = (REF_ACQUIRE, REF_GET_REF_COUNT, PART_INIT_PART);
}

fn file_size(name: &[u8]) -> u32 {
    let (mut hf, mut action) = (0u32, 0u32);
    if unsafe { DosOpen(name.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return 0;
    }
    let mut size = 0u32;
    unsafe { DosSetFilePtr(hf, 0, 2, &mut size) };
    unsafe { DosClose(hf) };
    size
}

// ---- IBM's multimedia parts (MMODPTS.DLL) on rs2's OpenDoc ----

fn mmodpts() {
    let e = ev();
    let session = new_session();
    check("session", session != 0);
    let mut fail = [0u8; 64];
    let mut hmod = 0u32;
    let rc = unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, b"MMODPTS\0".as_ptr(), &mut hmod) };
    if rc != 0 {
        let n = fail.iter().position(|&b| b == 0).unwrap_or(0);
        println!("FAIL DosLoadModule MMODPTS: rc {} ({})", rc, core::str::from_utf8(&fail[..n]).unwrap_or("?"));
        unsafe { FAILS += 1 };
        return;
    }
    check("MMODPTS.DLL loaded", true);
    let rc = unsafe { ODRegisterPartHandlerClass(b"MMAudioPart\0".as_ptr(), b"MMODPTS\0".as_ptr(), 1, 0) };
    check_eq("ODRegisterPartHandlerClass MMAudioPart", rc as i64, 0);
    let cls = somsup::find_class(b"MMAudioPart", 0, 0);
    check("MMAudioPart class", cls != 0);
    if cls == 0 {
        return;
    }
    let part_cls = unsafe { ODPartClassData[0] };
    check("MMAudioPart descends from rs2's ODPart", somsup::sc_call(somsup::cm::SOM_DESCENDED_FROM, &[cls, part_cls]) & 0xff != 0);
    let meta = somsup::class_of(cls);
    println!("     (metaclass {})", core::str::from_utf8(s(somsup::class_name(meta) as usize)).unwrap_or("?"));
    // M_ODPart's methods, as MMODPTS's metaclass overrides them.
    let mpart = unsafe { (&raw const M_ODPartClassData) as *const usize };
    let hn = m(mpart, 0, &[cls, e]);
    println!("     (clsGetODPartHandlerName: {})", core::str::from_utf8(s(hn)).unwrap_or("?"));
    check("clsGetODPartHandlerName (M_MMAudioPart)", s(hn) == b"MMAudioPart");
    // The kinds the part handler registered (OS2.INI OpenDoc:PartKinds).
    let mut keys = [0u8; 1024];
    let n = unsafe { PrfQueryProfileString(0xffff_ffff, b"OpenDoc:PartKinds\0".as_ptr(), core::ptr::null(), b"\0".as_ptr(), keys.as_mut_ptr(), keys.len() as u32) } as usize;
    let mut kind: Vec<u8> = Vec::new();
    for k in keys[..n.min(keys.len())].split(|&b| b == 0).filter(|k| !k.is_empty()) {
        let mut v = [0u8; 64];
        let kz = [k, &b"\0"[..]].concat();
        let vn = unsafe { PrfQueryProfileString(0xffff_ffff, b"OpenDoc:PartKinds\0".as_ptr(), kz.as_ptr(), b"\0".as_ptr(), v.as_mut_ptr(), 64) } as usize;
        if v[..vn.min(64)].starts_with(b"MMAudioPart") {
            println!("     (kind {} -> MMAudioPart)", core::str::from_utf8(k).unwrap_or("?"));
            kind = k.to_vec();
        }
    }
    check("the part kinds were registered (clsGetODPartKinds)", !kind.is_empty());
    kind.push(0);

    let id = ba(b"C:\\ODMM.OD\0");
    let sys = m(cd!(ODBaseSessionClassData), BS_GET_STORAGE_SYSTEM, &[session, e]);
    let cont = m(cd!(ODStorageSystemClassData), SYS_CREATE_CONTAINER, &[sys, e, b"rs2:Container\0".as_ptr() as usize, &id as *const ByteArray as usize]);
    let doc = m(cd!(ODContainerClassData), CONT_ACQUIRE_DOCUMENT, &[cont, e, 1]);
    let draft = m(cd!(ODDocumentClassData), DOC_ACQUIRE_BASE_DRAFT, &[doc, e, 4]);
    let part = m(cd!(ODDraftClassData), DR_CREATE_PART, &[draft, e, kind.as_ptr() as usize, 0]);
    ok_ev("CreatePart MMAudioPart");
    check("an MMAudioPart made by its kind and initialised (InitPart)", part != 0 && somsup::is_a(part, cls));
    if part == 0 {
        return;
    }
    let pid = m(cd!(ODPersistentObjectClassData), PO_GET_ID, &[part, e]);
    let props = m(cd!(ODDraftClassData), DR_ACQUIRE_DRAFT_PROPERTIES, &[draft, e]);
    let mut r = [0u8; 4];
    m(cd!(ODStorageUnitClassData), SU_GET_STRONG_REF, &[props, e, pid, r.as_mut_ptr() as usize]);
    su_set(props, PROP_ROOT, TYPE_REF, &r);
    release(props);
    m(cd!(ODDraftClassData), DR_EXTERNALIZE, &[draft, e]);
    ok_ev("draft Externalize with MMAudioPart");
    let psu = m(cd!(ODPersistentObjectClassData), PO_GET_STORAGE_UNIT, &[part, e]);
    let np = m(cd!(ODStorageUnitClassData), SU_COUNT_PROPERTIES, &[psu, e]);
    println!("     (the part's storage unit has {} properties)", np);
    check("MMAudioPart wrote its properties", np > 3);
    check("MMAudioPart document written", file_size(b"C:\\ODMM.OD\0") > 0);
    release(part);
    release(draft);
    release(doc);
    release(cont);
    ok_ev("close");
    let cont = m(cd!(ODStorageSystemClassData), SYS_ACQUIRE_CONTAINER, &[sys, e, b"rs2:Container\0".as_ptr() as usize, &id as *const ByteArray as usize]);
    let doc = if cont != 0 { m(cd!(ODContainerClassData), CONT_ACQUIRE_DOCUMENT, &[cont, e, 1]) } else { 0 };
    let draft = if doc != 0 { m(cd!(ODDocumentClassData), DOC_ACQUIRE_BASE_DRAFT, &[doc, e, 4]) } else { 0 };
    let part = if draft != 0 { m(cd!(ODDraftClassData), DR_ACQUIRE_PART, &[draft, e, pid]) } else { 0 };
    ok_ev("AcquirePart MMAudioPart");
    check("MMAudioPart read back (InitPartFromStorage)", part != 0 && somsup::is_a(part, cls));
    release(part);
    release(draft);
    release(doc);
    release(cont);
    ok_ev("close again");
    let rc = unsafe { ODDeregisterPartHandlerClass(b"MMAudioPart\0".as_ptr()) };
    check_eq("ODDeregisterPartHandlerClass", rc as i64, 0);
}
