//! OPENDOC.DLL: rs2's OpenDoc for OS/2 runtime.
//!
//! IBM's OpenDoc for OS/2 is not on the Warp 4.52 CDs, but programs on
//! them import it (the multimedia parts MMODPTS.DLL, Object REXX's
//! scripting component OREXXSC.DLL). This DLL has the exports they import,
//! with IBM's ordinals (alphabetical: three per class, `XCClassData`,
//! `XClassData`, `XNewClass`), and every class is a SOM class with
//! OpenDoc's parents and release order, so IBM's parts subclass them and
//! call them through their method tokens. How the ordinals and release
//! orders were found, and what is real and what is a stub: docs/OPENDOC.md.
//!
//! | Module | Classes |
//! |--------|---------|
//! | obj | ODObject, ODRefCntObject, ODPersistentObject |
//! | storage | ODStorageSystem, ODContainer (rs2's file format), ODDocument, ODDraft, ODStorageUnit, ODStorageUnitView, ODStorageUnitCursor |
//! | layout | ODFrame, ODFacet, ODBaseShape/ODShape, ODBaseTransform/ODTransform, ODBaseCanvas/ODCanvas, ODPlatformCanvas, facet iterators |
//! | part | ODPart, M_ODPart, part handler registration |
//! | services | ODBaseSession/ODSession, ODArbitrator, ODFocusSet, ODWindowState, ODWindow, ODMenuBar, clipboard, drag and drop, help, extensions, dispatcher; stubs |

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer, static_mut_refs, unused_variables, clippy::all)]

extern crate alloc;
extern crate os2;

#[macro_use]
pub mod rt;
pub mod layout;
pub mod obj;
pub mod part;
pub mod services;
pub mod storage;

use rt::Def;
use somsup::{ClassSpec, build_class};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, term: u32) -> u32 {
    if term == 0 {
        for (n, _) in CLASSES {
            new_class(n);
        }
    }
    1
}

/// A pointer to an exported class data structure.
#[macro_export]
macro_rules! cd {
    ($a:ident) => {
        (&raw mut $crate::$a) as *mut usize as *const usize
    };
}

// ---- Class data (exported) ----

class_data! {
    M_ODFacetWindowClassData, M_ODFacetWindowCClassData, 4;
    M_ODPartClassData, M_ODPartCClassData, 8;
    ODArbitratorClassData, ODArbitratorCClassData, 24;
    ODBaseCanvasClassData, ODBaseCanvasCClassData, 24;
    ODBaseDragAndDropClassData, ODBaseDragAndDropCClassData, 12;
    ODBaseDragItemIteratorClassData, ODBaseDragItemIteratorCClassData, 8;
    ODBaseSessionClassData, ODBaseSessionCClassData, 40;
    ODBaseShapeClassData, ODBaseShapeCClassData, 32;
    ODBaseTransformClassData, ODBaseTransformCClassData, 32;
    ODCanvasClassData, ODCanvasCClassData, 4;
    ODClipboardClassData, ODClipboardCClassData, 16;
    ODContainerClassData, ODContainerCClassData, 12;
    ODDispatcherClassData, ODDispatcherCClassData, 12;
    ODDocumentClassData, ODDocumentCClassData, 16;
    ODDraftClassData, ODDraftCClassData, 44;
    ODDragAndDropClassData, ODDragAndDropCClassData, 12;
    ODExtensionClassData, ODExtensionCClassData, 8;
    ODFacetClassData, ODFacetCClassData, 56;
    ODFacetIteratorClassData, ODFacetIteratorCClassData, 8;
    ODFocusSetClassData, ODFocusSetCClassData, 8;
    ODFrameClassData, ODFrameCClassData, 64;
    ODFrameFacetIteratorClassData, ODFrameFacetIteratorCClassData, 8;
    ODHelpClassData, ODHelpCClassData, 8;
    ODMenuBarClassData, ODMenuBarCClassData, 28;
    ODNameSpaceManagerClassData, ODNameSpaceManagerCClassData, 8;
    ODObjectClassData, ODObjectCClassData, 12;
    ODPartClassData, ODPartCClassData, 72;
    ODPersistentObjectClassData, ODPersistentObjectCClassData, 12;
    ODPlatformCanvasClassData, ODPlatformCanvasCClassData, 8;
    ODRefCntObjectClassData, ODRefCntObjectCClassData, 8;
    ODSessionClassData, ODSessionCClassData, 8;
    ODSettingsExtensionClassData, ODSettingsExtensionCClassData, 28;
    ODShapeClassData, ODShapeCClassData, 8;
    ODStatusLineExtensionClassData, ODStatusLineExtensionCClassData, 8;
    ODStorageSystemClassData, ODStorageSystemCClassData, 8;
    ODStorageUnitClassData, ODStorageUnitCClassData, 52;
    ODStorageUnitCursorClassData, ODStorageUnitCursorCClassData, 8;
    ODStorageUnitViewClassData, ODStorageUnitViewCClassData, 40;
    ODTransformClassData, ODTransformCClassData, 8;
    ODTypeListClassData, ODTypeListCClassData, 8;
    ODWindowClassData, ODWindowCClassData, 28;
    ODWindowIteratorClassData, ODWindowIteratorCClassData, 8;
    ODWindowStateClassData, ODWindowStateCClassData, 32;
    OSAScriptingComponentClassData, OSAScriptingComponentCClassData, 44;
}

/// The Apple-event terminology classes (ordinals 1000-1046) are not
/// built: their class data only exists so OREXXSC's imports resolve.
#[unsafe(no_mangle)]
pub static mut ODAETEClassDataStub: [usize; 64] = [0; 64];

/// Every class: name, and its description.
pub static CLASSES: &[(&str, fn() -> Def)] = &[
    ("ODObject", || Def { name: "ODObject", parents: &["SOMObject"], meta: None, data: 0, methods: obj::OBJECT_M, overrides: &[], cd: p!(ODObjectClassData), ccd: p!(ODObjectCClassData) }),
    ("ODRefCntObject", || Def { name: "ODRefCntObject", parents: &["ODObject"], meta: None, data: 4, methods: obj::REFCNT_M, overrides: obj::REFCNT_O, cd: p!(ODRefCntObjectClassData), ccd: p!(ODRefCntObjectCClassData) }),
    ("ODPersistentObject", || Def { name: "ODPersistentObject", parents: &["ODRefCntObject"], meta: None, data: 4, methods: obj::PERSISTENT_M, overrides: obj::PERSISTENT_O, cd: p!(ODPersistentObjectClassData), ccd: p!(ODPersistentObjectCClassData) }),
    ("M_ODPart", || Def { name: "M_ODPart", parents: &["SOMClass"], meta: None, data: 0, methods: part::M_PART_M, overrides: &[], cd: p!(M_ODPartClassData), ccd: p!(M_ODPartCClassData) }),
    ("ODPart", || Def { name: "ODPart", parents: &["ODPersistentObject"], meta: Some("M_ODPart"), data: 0, methods: part::PART_M, overrides: part::PART_O, cd: p!(ODPartClassData), ccd: p!(ODPartCClassData) }),
    ("M_ODFacetWindow", || Def { name: "M_ODFacetWindow", parents: &["SOMClass"], meta: None, data: 0, methods: services::FACET_WINDOW_META_M, overrides: &[], cd: p!(M_ODFacetWindowClassData), ccd: p!(M_ODFacetWindowCClassData) }),
    ("ODStorageSystem", || Def { name: "ODStorageSystem", parents: &["ODObject"], meta: None, data: 4, methods: storage::SYSTEM_M, overrides: storage::SYSTEM_O, cd: p!(ODStorageSystemClassData), ccd: p!(ODStorageSystemCClassData) }),
    ("ODContainer", || Def { name: "ODContainer", parents: &["ODRefCntObject"], meta: None, data: 4, methods: storage::CONTAINER_M, overrides: storage::CONTAINER_O, cd: p!(ODContainerClassData), ccd: p!(ODContainerCClassData) }),
    ("ODDocument", || Def { name: "ODDocument", parents: &["ODRefCntObject"], meta: None, data: 4, methods: storage::DOCUMENT_M, overrides: storage::DOCUMENT_O, cd: p!(ODDocumentClassData), ccd: p!(ODDocumentCClassData) }),
    ("ODDraft", || Def { name: "ODDraft", parents: &["ODRefCntObject"], meta: None, data: 4, methods: storage::DRAFT_M, overrides: storage::DRAFT_O, cd: p!(ODDraftClassData), ccd: p!(ODDraftCClassData) }),
    ("ODStorageUnit", || Def { name: "ODStorageUnit", parents: &["ODRefCntObject"], meta: None, data: 4, methods: storage::SU_M, overrides: storage::SU_O, cd: p!(ODStorageUnitClassData), ccd: p!(ODStorageUnitCClassData) }),
    ("ODStorageUnitView", || Def { name: "ODStorageUnitView", parents: &["ODObject"], meta: None, data: 4, methods: storage::VIEW_M, overrides: storage::VIEW_O, cd: p!(ODStorageUnitViewClassData), ccd: p!(ODStorageUnitViewCClassData) }),
    ("ODStorageUnitCursor", || Def { name: "ODStorageUnitCursor", parents: &["ODObject"], meta: None, data: 4, methods: storage::CURSOR_M, overrides: storage::CURSOR_O, cd: p!(ODStorageUnitCursorClassData), ccd: p!(ODStorageUnitCursorCClassData) }),
    ("ODBaseTransform", || Def { name: "ODBaseTransform", parents: &["ODRefCntObject"], meta: None, data: 4, methods: layout::BASE_TRANSFORM_M, overrides: layout::BASE_TRANSFORM_O, cd: p!(ODBaseTransformClassData), ccd: p!(ODBaseTransformCClassData) }),
    ("ODTransform", || Def { name: "ODTransform", parents: &["ODBaseTransform"], meta: None, data: 0, methods: layout::TRANSFORM_M, overrides: &[], cd: p!(ODTransformClassData), ccd: p!(ODTransformCClassData) }),
    ("ODBaseShape", || Def { name: "ODBaseShape", parents: &["ODRefCntObject"], meta: None, data: 4, methods: layout::BASE_SHAPE_M, overrides: layout::BASE_SHAPE_O, cd: p!(ODBaseShapeClassData), ccd: p!(ODBaseShapeCClassData) }),
    ("ODShape", || Def { name: "ODShape", parents: &["ODBaseShape"], meta: None, data: 0, methods: layout::SHAPE_M, overrides: &[], cd: p!(ODShapeClassData), ccd: p!(ODShapeCClassData) }),
    ("ODBaseCanvas", || Def { name: "ODBaseCanvas", parents: &["ODObject"], meta: None, data: 4, methods: layout::BASE_CANVAS_M, overrides: layout::BASE_CANVAS_O, cd: p!(ODBaseCanvasClassData), ccd: p!(ODBaseCanvasCClassData) }),
    ("ODCanvas", || Def { name: "ODCanvas", parents: &["ODBaseCanvas"], meta: None, data: 0, methods: layout::CANVAS_M, overrides: &[], cd: p!(ODCanvasClassData), ccd: p!(ODCanvasCClassData) }),
    ("ODPlatformCanvas", || Def { name: "ODPlatformCanvas", parents: &["ODObject"], meta: None, data: 4, methods: layout::PLATFORM_CANVAS_M, overrides: layout::PLATFORM_CANVAS_O, cd: p!(ODPlatformCanvasClassData), ccd: p!(ODPlatformCanvasCClassData) }),
    ("ODFrame", || Def { name: "ODFrame", parents: &["ODPersistentObject"], meta: None, data: 4, methods: layout::FRAME_M, overrides: layout::FRAME_O, cd: p!(ODFrameClassData), ccd: p!(ODFrameCClassData) }),
    ("ODFacet", || Def { name: "ODFacet", parents: &["ODObject"], meta: None, data: 4, methods: layout::FACET_M, overrides: layout::FACET_O, cd: p!(ODFacetClassData), ccd: p!(ODFacetCClassData) }),
    ("ODFrameFacetIterator", || Def { name: "ODFrameFacetIterator", parents: &["ODObject"], meta: None, data: 4, methods: layout::FRAME_FACET_ITER_M, overrides: layout::FRAME_FACET_ITER_O, cd: p!(ODFrameFacetIteratorClassData), ccd: p!(ODFrameFacetIteratorCClassData) }),
    ("ODFacetIterator", || Def { name: "ODFacetIterator", parents: &["ODObject"], meta: None, data: 4, methods: layout::FACET_ITER_M, overrides: layout::FACET_ITER_O, cd: p!(ODFacetIteratorClassData), ccd: p!(ODFacetIteratorCClassData) }),
    ("ODBaseSession", || Def { name: "ODBaseSession", parents: &["ODObject"], meta: None, data: 4, methods: services::BASE_SESSION_M, overrides: services::BASE_SESSION_O, cd: p!(ODBaseSessionClassData), ccd: p!(ODBaseSessionCClassData) }),
    ("ODSession", || Def { name: "ODSession", parents: &["ODBaseSession"], meta: None, data: 0, methods: services::SESSION_M, overrides: &[], cd: p!(ODSessionClassData), ccd: p!(ODSessionCClassData) }),
    ("ODArbitrator", || Def { name: "ODArbitrator", parents: &["ODObject"], meta: None, data: 4, methods: services::ARBITRATOR_M, overrides: services::ARBITRATOR_O, cd: p!(ODArbitratorClassData), ccd: p!(ODArbitratorCClassData) }),
    ("ODFocusSet", || Def { name: "ODFocusSet", parents: &["ODObject"], meta: None, data: 4, methods: services::FOCUS_SET_M, overrides: services::FOCUS_SET_O, cd: p!(ODFocusSetClassData), ccd: p!(ODFocusSetCClassData) }),
    ("ODWindowState", || Def { name: "ODWindowState", parents: &["ODObject"], meta: None, data: 4, methods: services::WINDOW_STATE_M, overrides: services::WINDOW_STATE_O, cd: p!(ODWindowStateClassData), ccd: p!(ODWindowStateCClassData) }),
    ("ODWindow", || Def { name: "ODWindow", parents: &["ODRefCntObject"], meta: None, data: 4, methods: services::WINDOW_M, overrides: services::WINDOW_O, cd: p!(ODWindowClassData), ccd: p!(ODWindowCClassData) }),
    ("ODWindowIterator", || Def { name: "ODWindowIterator", parents: &["ODObject"], meta: None, data: 4, methods: services::WINDOW_ITER_M, overrides: services::WINDOW_ITER_O, cd: p!(ODWindowIteratorClassData), ccd: p!(ODWindowIteratorCClassData) }),
    ("ODMenuBar", || Def { name: "ODMenuBar", parents: &["ODRefCntObject"], meta: None, data: 4, methods: services::MENU_BAR_M, overrides: services::MENU_BAR_O, cd: p!(ODMenuBarClassData), ccd: p!(ODMenuBarCClassData) }),
    ("ODClipboard", || Def { name: "ODClipboard", parents: &["ODObject"], meta: None, data: 4, methods: services::CLIPBOARD_M, overrides: &[], cd: p!(ODClipboardClassData), ccd: p!(ODClipboardCClassData) }),
    ("ODBaseDragAndDrop", || Def { name: "ODBaseDragAndDrop", parents: &["ODObject"], meta: None, data: 4, methods: services::BASE_DND_M, overrides: &[], cd: p!(ODBaseDragAndDropClassData), ccd: p!(ODBaseDragAndDropCClassData) }),
    ("ODDragAndDrop", || Def { name: "ODDragAndDrop", parents: &["ODBaseDragAndDrop"], meta: None, data: 0, methods: services::DND_M, overrides: &[], cd: p!(ODDragAndDropClassData), ccd: p!(ODDragAndDropCClassData) }),
    ("ODBaseDragItemIterator", || Def { name: "ODBaseDragItemIterator", parents: &["ODObject"], meta: None, data: 0, methods: services::DRAG_ITEM_ITER_M, overrides: &[], cd: p!(ODBaseDragItemIteratorClassData), ccd: p!(ODBaseDragItemIteratorCClassData) }),
    ("ODHelp", || Def { name: "ODHelp", parents: &["ODObject"], meta: None, data: 0, methods: services::HELP_M, overrides: &[], cd: p!(ODHelpClassData), ccd: p!(ODHelpCClassData) }),
    ("ODExtension", || Def { name: "ODExtension", parents: &["ODRefCntObject"], meta: None, data: 4, methods: services::EXTENSION_M, overrides: &[], cd: p!(ODExtensionClassData), ccd: p!(ODExtensionCClassData) }),
    ("ODSettingsExtension", || Def { name: "ODSettingsExtension", parents: &["ODExtension"], meta: None, data: 0, methods: services::SETTINGS_EXTENSION_M, overrides: &[], cd: p!(ODSettingsExtensionClassData), ccd: p!(ODSettingsExtensionCClassData) }),
    ("ODStatusLineExtension", || Def { name: "ODStatusLineExtension", parents: &["ODExtension"], meta: None, data: 0, methods: services::STATUS_LINE_M, overrides: &[], cd: p!(ODStatusLineExtensionClassData), ccd: p!(ODStatusLineExtensionCClassData) }),
    ("ODDispatcher", || Def { name: "ODDispatcher", parents: &["ODObject"], meta: None, data: 0, methods: services::DISPATCHER_M, overrides: &[], cd: p!(ODDispatcherClassData), ccd: p!(ODDispatcherCClassData) }),
    ("ODNameSpaceManager", || Def { name: "ODNameSpaceManager", parents: &["ODObject"], meta: None, data: 0, methods: services::NAME_SPACE_MANAGER_M, overrides: &[], cd: p!(ODNameSpaceManagerClassData), ccd: p!(ODNameSpaceManagerCClassData) }),
    ("ODTypeList", || Def { name: "ODTypeList", parents: &["ODObject"], meta: None, data: 0, methods: services::TYPE_LIST_M, overrides: &[], cd: p!(ODTypeListClassData), ccd: p!(ODTypeListCClassData) }),
    ("OSAScriptingComponent", || Def { name: "OSAScriptingComponent", parents: &["SOMObject"], meta: None, data: 0, methods: services::OSA_M, overrides: &[], cd: p!(OSAScriptingComponentClassData), ccd: p!(OSAScriptingComponentCClassData) }),
];

/// Build a class of this DLL (its parents and metaclass first).
pub fn new_class(name: &str) -> usize {
    match name {
        "SOMObject" | "SOMClass" => unsafe {
            somsup::SOMObjectNewClass(1, 4);
            return somsup::SOMClassNewClass(1, 4);
        },
        _ => {}
    }
    let Some(&(_, f)) = CLASSES.iter().find(|(n, _)| *n == name) else { return 0 };
    let d = f();
    unsafe {
        if *d.cd != 0 {
            return *d.cd;
        }
    }
    for p in d.parents {
        new_class(p);
    }
    if let Some(m) = d.meta {
        new_class(m);
    }
    let methods: alloc::vec::Vec<(&str, usize)> = d.methods.iter().map(|&(n, m)| (n, m as usize)).collect();
    let overrides: alloc::vec::Vec<(&str, usize)> = d.overrides.iter().map(|&(n, m)| (n, m as usize)).collect();
    build_class(&ClassSpec {
        name: d.name,
        parents: d.parents,
        meta: d.meta,
        major: 1,
        minor: 0,
        data: d.data,
        align: if d.data != 0 { 4 } else { 0 },
        methods: &methods,
        overrides: &overrides,
        cd: d.cd,
        ccd: d.ccd,
    })
}

// ---- Exported functions ----

macro_rules! new_class_fns {
    ($($f:ident, $t:ident => $n:literal;)*) => {$(
        pub extern "C" fn $f(_major: i32, _minor: i32) -> usize {
            new_class($n)
        }
        somsup::aligned! { $t => $f, 2; }
    )*};
}

new_class_fns! {
    M_ODFacetWindowNewClass, __a_M_ODFacetWindowNewClass => "M_ODFacetWindow";
    M_ODPartNewClass, __a_M_ODPartNewClass => "M_ODPart";
    ODArbitratorNewClass, __a_ODArbitratorNewClass => "ODArbitrator";
    ODBaseCanvasNewClass, __a_ODBaseCanvasNewClass => "ODBaseCanvas";
    ODBaseDragAndDropNewClass, __a_ODBaseDragAndDropNewClass => "ODBaseDragAndDrop";
    ODBaseDragItemIteratorNewClass, __a_ODBaseDragItemIteratorNewClass => "ODBaseDragItemIterator";
    ODBaseSessionNewClass, __a_ODBaseSessionNewClass => "ODBaseSession";
    ODBaseShapeNewClass, __a_ODBaseShapeNewClass => "ODBaseShape";
    ODBaseTransformNewClass, __a_ODBaseTransformNewClass => "ODBaseTransform";
    ODCanvasNewClass, __a_ODCanvasNewClass => "ODCanvas";
    ODClipboardNewClass, __a_ODClipboardNewClass => "ODClipboard";
    ODContainerNewClass, __a_ODContainerNewClass => "ODContainer";
    ODDispatcherNewClass, __a_ODDispatcherNewClass => "ODDispatcher";
    ODDocumentNewClass, __a_ODDocumentNewClass => "ODDocument";
    ODDraftNewClass, __a_ODDraftNewClass => "ODDraft";
    ODDragAndDropNewClass, __a_ODDragAndDropNewClass => "ODDragAndDrop";
    ODExtensionNewClass, __a_ODExtensionNewClass => "ODExtension";
    ODFacetNewClass, __a_ODFacetNewClass => "ODFacet";
    ODFacetIteratorNewClass, __a_ODFacetIteratorNewClass => "ODFacetIterator";
    ODFocusSetNewClass, __a_ODFocusSetNewClass => "ODFocusSet";
    ODFrameNewClass, __a_ODFrameNewClass => "ODFrame";
    ODFrameFacetIteratorNewClass, __a_ODFrameFacetIteratorNewClass => "ODFrameFacetIterator";
    ODHelpNewClass, __a_ODHelpNewClass => "ODHelp";
    ODMenuBarNewClass, __a_ODMenuBarNewClass => "ODMenuBar";
    ODNameSpaceManagerNewClass, __a_ODNameSpaceManagerNewClass => "ODNameSpaceManager";
    ODObjectNewClass, __a_ODObjectNewClass => "ODObject";
    ODPartNewClass, __a_ODPartNewClass => "ODPart";
    ODPersistentObjectNewClass, __a_ODPersistentObjectNewClass => "ODPersistentObject";
    ODPlatformCanvasNewClass, __a_ODPlatformCanvasNewClass => "ODPlatformCanvas";
    ODRefCntObjectNewClass, __a_ODRefCntObjectNewClass => "ODRefCntObject";
    ODSessionNewClass, __a_ODSessionNewClass => "ODSession";
    ODSettingsExtensionNewClass, __a_ODSettingsExtensionNewClass => "ODSettingsExtension";
    ODShapeNewClass, __a_ODShapeNewClass => "ODShape";
    ODStatusLineExtensionNewClass, __a_ODStatusLineExtensionNewClass => "ODStatusLineExtension";
    ODStorageSystemNewClass, __a_ODStorageSystemNewClass => "ODStorageSystem";
    ODStorageUnitNewClass, __a_ODStorageUnitNewClass => "ODStorageUnit";
    ODStorageUnitCursorNewClass, __a_ODStorageUnitCursorNewClass => "ODStorageUnitCursor";
    ODStorageUnitViewNewClass, __a_ODStorageUnitViewNewClass => "ODStorageUnitView";
    ODTransformNewClass, __a_ODTransformNewClass => "ODTransform";
    ODTypeListNewClass, __a_ODTypeListNewClass => "ODTypeList";
    ODWindowNewClass, __a_ODWindowNewClass => "ODWindow";
    ODWindowIteratorNewClass, __a_ODWindowIteratorNewClass => "ODWindowIterator";
    ODWindowStateNewClass, __a_ODWindowStateNewClass => "ODWindowState";
    OSAScriptingComponentNewClass, __a_OSAScriptingComponentNewClass => "OSAScriptingComponent";
}

somsup::aligned! {
    __a_StorageUnitGetValue => storage::storage_unit_get_value, 4;
    __a_ODRegisterPartHandlerClass => part::register_part_handler, 4;
    __a_ODDeregisterPartHandlerClass => part::deregister_part_handler, 1;
}

/// The Apple Event Manager calls (ordinals 1-71): no Apple events.
#[unsafe(no_mangle)]
pub extern "C" fn __AENotHandled() -> usize {
    services::ae_not_handled()
}

#[unsafe(no_mangle)]
pub extern "C" fn __AEDisposeDesc(desc: usize) -> usize {
    services::ae_dispose_desc(desc)
}

#[unsafe(no_mangle)]
pub extern "C" fn __AETENoClass(major: i32, minor: i32) -> usize {
    services::no_class(major, minor)
}

// ---- PM helpers ----

unsafe extern "C" {
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(hwnd: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(hwnd: u32, id: u32) -> u32;
}

pub fn pmparent(hwnd: u32) -> u32 {
    unsafe { WinQueryWindow(hwnd, 5) }
}

pub fn pmchild(hwnd: u32, id: u32) -> u32 {
    unsafe { WinWindowFromID(hwnd, id) }
}
