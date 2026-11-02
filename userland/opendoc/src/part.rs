//! ODPart (OpenDoc's default behaviour for every part method), its
//! metaclass M_ODPart, and part handler registration (ordinals 901/906):
//! OS2.INI's `OpenDoc:PartHandlers` (class -> DLL) and
//! `OpenDoc:PartKinds` (kind -> class).

use alloc::vec::Vec;

use crate::obj::acquire;
use crate::rt::*;
use crate::*;

/// ODPart's release order (from MMBasePart's overrides, docs/OPENDOC.md).
pub mod ix {
    pub const INIT_PART: usize = 0;
    pub const INIT_PART_FROM_STORAGE: usize = 1;
    pub const DISPLAY_FRAME_ADDED: usize = 2;
    pub const DISPLAY_FRAME_REMOVED: usize = 3;
    pub const DISPLAY_FRAME_CONNECTED: usize = 4;
    pub const DISPLAY_FRAME_CLOSED: usize = 5;
    pub const FRAME_SHAPE_CHANGED: usize = 7;
    pub const VIEW_TYPE_CHANGED: usize = 8;
    pub const PRESENTATION_CHANGED: usize = 9;
    pub const OPEN: usize = 16;
    pub const DRAW: usize = 17;
    pub const FACET_ADDED: usize = 18;
    pub const FACET_REMOVED: usize = 19;
    pub const GEOMETRY_CHANGED: usize = 20;
    pub const HANDLE_EVENT: usize = 32;
    pub const REQUEST_EMBEDDED_FRAME: usize = 56;
}

methods! {
    fn InitPart(this: usize, ev: usize, su: usize, wrapper: usize) -> usize {
        call(cd!(ODPersistentObjectClassData), crate::obj::ix::INIT_PERSISTENT_OBJECT, &[this, ev, su]);
        0
    }
    fn InitPartFromStorage(this: usize, ev: usize, su: usize, wrapper: usize) -> usize {
        call(cd!(ODPersistentObjectClassData), crate::obj::ix::INIT_PERSISTENT_OBJECT_FROM_STORAGE, &[this, ev, su]);
        0
    }
    fn Frame1(this: usize, ev: usize, frame: usize) -> usize { 0 }
    fn Frame2(this: usize, ev: usize, a: usize, b: usize) -> usize { 0 }
    fn Frame3(this: usize, ev: usize, a: usize, b: usize, c: usize) -> usize { 0 }
    fn Frame4(this: usize, ev: usize, a: usize, b: usize, c: usize, d: usize) -> usize { 0 }
    fn Frame5(this: usize, ev: usize, a: usize, b: usize, c: usize, d: usize, e: usize) -> usize { 0 }
    fn NoArgs(this: usize, ev: usize) -> usize { 0 }
    fn True1(this: usize, ev: usize, a: usize) -> usize { 1 }
    fn True2(this: usize, ev: usize, a: usize, b: usize) -> usize { 1 }
    fn ReadPartInfo(this: usize, ev: usize, frame: usize, view: usize) -> usize { 0 }
    fn ClonePartInfo(this: usize, ev: usize, key: usize, info: usize, view: usize, scope: usize) -> usize { 0 }
    fn Open(this: usize, ev: usize, frame: usize) -> usize { 0 }
    fn HandleEvent(this: usize, ev: usize, event: usize, frame: usize, facet: usize, info: usize) -> usize { 0 }
    fn IsRealPart(this: usize, ev: usize) -> usize { 1 }
    fn GetRealPart(this: usize, ev: usize) -> usize { acquire(this, ev) }
    fn PartRelease(this: usize, ev: usize) -> usize {
        let c: &mut i32 = somsup::data(cd!(ODRefCntObjectCClassData), this);
        if *c > 0 {
            *c -= 1;
        }
        if *c == 0 {
            let su = crate::obj::su_of(this);
            let dr = if su == 0 { 0 } else { crate::storage::sus(su).draft };
            if dr != 0 {
                call(cd!(ODDraftClassData), crate::storage::draft_ix::RELEASE_PART, &[dr, ev, this]);
            }
        }
        0
    }
}

pub static PART_M: &[(&str, M)] = &[
    ("InitPart", InitPart::t),
    ("InitPartFromStorage", InitPartFromStorage::t),
    ("DisplayFrameAdded", Frame1::t),
    ("DisplayFrameRemoved", Frame1::t),
    ("DisplayFrameConnected", Frame1::t),
    ("DisplayFrameClosed", Frame1::t),
    ("AttachSourceFrame", Frame2::t),
    ("FrameShapeChanged", Frame1::t),
    ("ViewTypeChanged", Frame1::t),
    ("PresentationChanged", Frame1::t),
    ("SequenceChanged", Frame1::t),
    ("LinkStatusChanged", Frame1::t),
    ("ContainingPartPropertiesUpdated", Frame2::t),
    ("ReadPartInfo", ReadPartInfo::t),
    ("WritePartInfo", Frame2::t),
    ("ClonePartInfo", ClonePartInfo::t),
    ("Open", Open::t),
    ("Draw", Frame2::t),
    ("FacetAdded", Frame1::t),
    ("FacetRemoved", Frame1::t),
    ("GeometryChanged", Frame3::t),
    ("HighlightChanged", Frame1::t),
    ("CanvasChanged", Frame1::t),
    ("CanvasUpdated", Frame1::t),
    ("GetPrintResolution", Frame1::t),
    ("BeginRelinquishFocus", True2::t),
    ("CommitRelinquishFocus", Frame3::t),
    ("AbortRelinquishFocus", Frame3::t),
    ("FocusAcquired", Frame2::t),
    ("FocusLost", Frame2::t),
    ("ExternalizeKinds", Frame1::t),
    ("ChangeKind", Frame1::t),
    ("HandleEvent", HandleEvent::t),
    ("AdjustMenus", Frame1::t),
    ("CreateRootMenuBar", Frame1::t),
    ("UndoAction", Frame1::t),
    ("RedoAction", Frame1::t),
    ("DisposeActionState", Frame2::t),
    ("WriteActionState", Frame2::t),
    ("ReadActionState", Frame2::t),
    ("FulfillPromise", Frame1::t),
    ("DropCompleted", Frame2::t),
    ("DragEnter", Frame3::t),
    ("DragWithin", Frame3::t),
    ("DragLeave", Frame2::t),
    ("Drop", Frame3::t),
    ("CreateLink", Frame1::t),
    ("RevealLink", Frame1::t),
    ("LinkUpdated", Frame2::t),
    ("ODPart_reserved49", ret0),
    ("ODPart_reserved50", ret0),
    ("ODPart_reserved51", ret0),
    ("ODPart_reserved52", ret0),
    ("ODPart_reserved53", ret0),
    ("EmbeddedFrameUpdated", Frame2::t),
    ("EditInLinkAttempted", Frame1::t),
    ("RequestEmbeddedFrame", Frame5::t),
    ("RemoveEmbeddedFrame", Frame1::t),
    ("RequestFrameShape", Frame2::t),
    ("UsedShapeChanged", Frame1::t),
    ("AdjustBorderShape", Frame2::t),
    ("AcquireContainingPartProperties", Frame1::t),
    ("RevealFrame", Frame2::t),
    ("EmbeddedFrameSpec", Frame2::t),
    ("CreateEmbeddedFramesIterator", Frame1::t),
    ("IsRealPart", IsRealPart::t),
    ("GetRealPart", GetRealPart::t),
    ("ReleaseRealPart", NoArgs::t),
];
pub static PART_O: &[(&str, M)] = &[("ODRefCntObject::Release", PartRelease::t)];

// ---- M_ODPart ----

fn class_name_copy(cls: usize) -> usize {
    dup(cstr(somsup::class_name(cls) as usize))
}

/// _IDL_SEQUENCE_PartKindInfo.
#[repr(C)]
pub struct KindSeq {
    pub maximum: u32,
    pub length: u32,
    pub buffer: *mut [usize; 7],
}

methods! {
    fn clsGetODPartHandlerName(this: usize, ev: usize) -> usize { class_name_copy(this) }
    fn clsGetODPartHandlerDisplayName(this: usize, ev: usize) -> usize { class_name_copy(this) }
    fn clsGetODPartKinds(ret: usize, this: usize, ev: usize) -> usize {
        // One kind named after the class.
        let info = somsup::calloc(28) as *mut [usize; 7];
        if !info.is_null() {
            let n = class_name_copy(this);
            unsafe {
                (*info)[0] = n;
                (*info)[1] = class_name_copy(this);
                for i in 2..7 {
                    (*info)[i] = dup(b"");
                }
            }
        }
        write(ret, KindSeq { maximum: 1, length: 1, buffer: info });
        ret
    }
    fn clsGetOLE2ClassId(this: usize, ev: usize) -> usize { dup(b"") }
    fn clsGetWindowsIconFileName(this: usize, ev: usize) -> usize { dup(b"") }
}

pub static M_PART_M: &[(&str, M)] = &[
    ("clsGetODPartHandlerName", clsGetODPartHandlerName::t),
    ("clsGetODPartHandlerDisplayName", clsGetODPartHandlerDisplayName::t),
    ("*clsGetODPartKinds", clsGetODPartKinds::t),
    ("clsGetOLE2ClassId", clsGetOLE2ClassId::t),
    ("clsGetWindowsIconFileName", clsGetWindowsIconFileName::t),
];

// ---- Part handler registration ----

unsafe extern "C" {
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, val: *const u8) -> u32;
}

const HINI_USER: u32 = 0xffff_ffff;
const APP_HANDLERS: &[u8] = b"OpenDoc:PartHandlers\0";
const APP_KINDS: &[u8] = b"OpenDoc:PartKinds\0";

fn prf_get(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let k = z(key);
    let mut buf = [0u8; 260];
    let n = unsafe { PrfQueryProfileString(HINI_USER, app.as_ptr(), k.as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let s: Vec<u8> = buf[..n.min(buf.len())].iter().copied().take_while(|&b| b != 0).collect();
    if s.is_empty() { None } else { Some(s) }
}

fn prf_set(app: &[u8], key: &[u8], val: Option<&[u8]>) {
    let k = z(key);
    let v = val.map(z);
    unsafe { PrfWriteProfileString(HINI_USER, app.as_ptr(), k.as_ptr(), v.as_ref().map_or(core::ptr::null(), |v| v.as_ptr())) };
}

/// The DLL registered for a part handler class.
pub fn handler_dll(class: &[u8]) -> Option<Vec<u8>> {
    prf_get(APP_HANDLERS, class)
}

/// The part handler class registered for a part kind.
pub fn handler_for_kind(kind: &[u8]) -> Option<Vec<u8>> {
    if kind.is_empty() {
        return None;
    }
    prf_get(APP_KINDS, kind)
}

/// ODRegisterPartHandlerClass(className, dllName, flags, reserved)
/// (ordinal 906): 0 when registered. The class is loaded and its
/// metaclass asked for the kinds it edits.
pub extern "C" fn register_part_handler(class: usize, dll: usize, flags: usize, reserved: usize) -> usize {
    let (c, d) = (cstr(class), cstr(dll));
    if c.is_empty() || d.is_empty() {
        return 1;
    }
    prf_set(APP_HANDLERS, c, Some(d));
    let mgr = unsafe { somsup::SOMClassMgrObject };
    let f = z(d);
    let cls = call(somsup::scm(), somsup::mm::SOM_FIND_CLS_IN_FILE, &[mgr, somsup::id(c) as usize, 0, 0, f.as_ptr() as usize]);
    if cls == 0 {
        return 0;
    }
    let mut ev = somsup::Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };
    let mut seq = KindSeq { maximum: 0, length: 0, buffer: core::ptr::null_mut() };
    if somsup::is_a(cls, unsafe { *cd!(M_ODPartClassData) }) {
        call(cd!(M_ODPartClassData), 2, &[&raw mut seq as usize, cls, &raw mut ev as usize]);
    }
    for i in 0..seq.length as usize {
        let info = unsafe { &*seq.buffer.add(i) };
        let kind = cstr(info[0]);
        if !kind.is_empty() {
            prf_set(APP_KINDS, kind, Some(c));
        }
    }
    0
}

/// ODDeregisterPartHandlerClass(className) (ordinal 901).
pub extern "C" fn deregister_part_handler(class: usize) -> usize {
    let c = cstr(class);
    if c.is_empty() {
        return 1;
    }
    prf_set(APP_HANDLERS, c, None);
    0
}
