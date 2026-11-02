//! SOMU.DLL: SOM 2.1's utility classes as OS/2 Warp 4.52 has them
//! (studied in IBM's SOMU.DLL with Ghidra), binary compatible with it:
//! IBM's ordinals, the class data structures (method tokens in IBM's
//! order, which is not always the IDL's), each class's parents,
//! metaclass, version and instance data size, and the functions and
//! variables IBM's exports (UUIDs, the thread procedures, the heap
//! watcher, the externalization helpers).
//!
//! | Class | What it is |
//! |-------|------------|
//! | SOMMBeforeAfter | Metaclass: sommBeforeMethod/sommAfterMethod around every method call on its classes' instances |
//! | SOMMTraced | A before/after metaclass that traces the calls (`SET SOMM_TRACED=` class names, or empty for all) |
//! | SOMMSingleInstance | Metaclass: its classes have one instance (somNew returns it again) |
//! | SOMMMetaCooperative, SOMMCooperative, SOMMCooperativeSistered, SOMMCooperativeRedispatched, SOMMBeforeAfterDispatcher | IBM's cooperative metaclass framework the above are built on |
//! | SOMUTId, SOMUTStringId | Id classes (IBM's methods only report "not implemented") |
//! | SOMBuf | A gap buffer of fixed-size items |
//! | somutMsgManager, M_somutMsgManager | Messages from a message file |
//! | SOMDesc, SOMArgList, SOMCommand | Method descriptions and argument lists from the interface repository |
//!
//! How the before/after metaclasses work here: when a class whose
//! metaclass descends from SOMMCooperativeRedispatched is ready, each of
//! its methods (the dispatch methods and rs2's legacy initializers
//! aside) is replaced by a stub that, for an instance whose class's
//! metaclass descends from SOMMBeforeAfter, calls the metaclass's
//! sommBeforeMethod implementations (parents left to right), the method
//! if all of them returned TRUE, and the sommAfterMethod implementations
//! of those that ran, in reverse (IBM's dispatcher's order). IBM's
//! framework gets there through redispatch stubs, somDispatch and
//! "sister" classes made with SOM's private class methods; the stubs
//! here call the original implementation directly. See docs/WPS.md.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer, static_mut_refs)]

extern crate alloc;
extern crate os2;

pub mod buf;
pub mod coop;
pub mod desc;
pub mod misc;
pub mod msg;

use somsup::{ClassSpec, aligned, build_class};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, term: u32) -> u32 {
    if term == 0 {
        misc::init();
    }
    1
}

// ---- Class data (IBM's sizes: the class object, then its entries) ----

macro_rules! class_data {
    ($($cd:ident, $ccd:ident, $n:literal;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; $n] = [0; $n];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

class_data! {
    somutMsgManagerClassData, somutMsgManagerCClassData, 3;
    M_somutMsgManagerClassData, M_somutMsgManagerCClassData, 2;
    SOMMMetaCooperativeClassData, SOMMMetaCooperativeCClassData, 3;
    SOMMCooperativeClassData, SOMMCooperativeCClassData, 7;
    SOMMCooperativeSisteredClassData, SOMMCooperativeSisteredCClassData, 2;
    SOMMCooperativeRedispatchedClassData, SOMMCooperativeRedispatchedCClassData, 1;
    SOMMSingleInstanceClassData, SOMMSingleInstanceCClassData, 3;
    SOMArgListClassData, SOMArgListCClassData, 31;
    SOMMBeforeAfterClassData, SOMMBeforeAfterCClassData, 3;
    SOMBufClassData, SOMBufCClassData, 13;
    SOMCommandClassData, SOMCommandCClassData, 6;
    SOMDescClassData, SOMDescCClassData, 17;
    SOMUTIdClassData, SOMUTIdCClassData, 5;
    SOMUTStringIdClassData, SOMUTStringIdCClassData, 6;
    SOMMTracedClassData, SOMMTracedCClassData, 3;
    SOMMBeforeAfterDispatcherClassData, SOMMBeforeAfterDispatcherCClassData, 5;
}

// The private classes behind SOMMSingleInstance.
pub static mut HiddenClassMetaClassData: [usize; 1] = [0; 1];
pub static mut HiddenClassMetaCClassData: [usize; 8] = [0; 8];
pub static mut ProxyMetaClassData: [usize; 1] = [0; 1];
pub static mut ProxyMetaCClassData: [usize; 8] = [0; 8];

pub fn cd<const N: usize>(a: &'static mut [usize; N]) -> *mut usize {
    a.as_mut_ptr()
}

macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}
pub(crate) use p;

// ---- <Class>NewClass ----

/// The SOM.DLL classes are made first (IBM's NewClass functions call
/// SOMObjectNewClass or SOMClassNewClass).
fn base() {
    unsafe {
        somsup::SOMObjectNewClass(1, 4);
        somsup::SOMClassNewClass(1, 4);
    }
}

pub extern "C" fn M_somutMsgManagerNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "M_somutMsgManager",
        parents: &["SOMClass"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[("somutCreateMsgManager", msg::__m_create as usize)],
        overrides: &[],
        cd: p!(M_somutMsgManagerClassData),
        ccd: p!(M_somutMsgManagerCClassData),
    })
}

pub extern "C" fn somutMsgManagerNewClass(major: i32, minor: i32) -> usize {
    base();
    M_somutMsgManagerNewClass(2, 1);
    build_class(&ClassSpec {
        name: "somutMsgManager",
        parents: &["SOMObject"],
        meta: Some("M_somutMsgManager"),
        major,
        minor,
        data: 12,
        align: 4,
        methods: &[("somutMsgGetMessage", msg::__m_get_message as usize), ("somutMsgCatOpen", msg::__m_cat_open as usize)],
        overrides: &[("somInit", msg::__m_init as usize), ("somUninit", msg::__m_uninit as usize)],
        cd: p!(somutMsgManagerClassData),
        ccd: p!(somutMsgManagerCClassData),
    })
}

pub extern "C" fn SOMMMetaCooperativeNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "SOMMMetaCooperative",
        parents: &["SOMClass"],
        meta: None,
        major,
        minor,
        data: 24,
        align: 4,
        methods: &[("somAddReqOverrides", coop::__m_add_req_overrides as usize), ("somAddCoopOverride", coop::__m_add_coop_override as usize)],
        overrides: &[],
        cd: p!(SOMMMetaCooperativeClassData),
        ccd: p!(SOMMMetaCooperativeCClassData),
    })
}

pub extern "C" fn SOMMCooperativeNewClass(major: i32, minor: i32) -> usize {
    base();
    SOMMMetaCooperativeNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMCooperative",
        parents: &["SOMClass"],
        meta: Some("SOMMMetaCooperative"),
        major,
        minor,
        data: 84,
        align: 4,
        methods: &[
            ("sommRequestFinalClassMethodCall", coop::__m_request as usize),
            ("sommAddCooperativeClassMethod", coop::__m_add_coop as usize),
            ("sommAddCooperativeInstanceMethod", coop::__m_add_coop as usize),
            ("sommRequestFirstCooperativeInstanceMethodCall", coop::__m_request as usize),
            ("sommSatisfyRequests", coop::__m_satisfy as usize),
            ("sommRequestFirstCooperativeClassMethodCall", coop::__m_request as usize),
        ],
        overrides: &[],
        cd: p!(SOMMCooperativeClassData),
        ccd: p!(SOMMCooperativeCClassData),
    })
}

pub extern "C" fn SOMMCooperativeSisteredNewClass(major: i32, minor: i32) -> usize {
    SOMMCooperativeNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMCooperativeSistered",
        parents: &["SOMMCooperative"],
        meta: None,
        major,
        minor,
        data: 20,
        align: 4,
        methods: &[("_get_somSisterClass", coop::__m_sister as usize)],
        overrides: &[],
        cd: p!(SOMMCooperativeSisteredClassData),
        ccd: p!(SOMMCooperativeSisteredCClassData),
    })
}

pub extern "C" fn SOMMCooperativeRedispatchedNewClass(major: i32, minor: i32) -> usize {
    SOMMCooperativeSisteredNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMCooperativeRedispatched",
        parents: &["SOMMCooperativeSistered"],
        meta: None,
        major,
        minor,
        data: 8,
        align: 4,
        methods: &[],
        overrides: &[("somClassReady", coop::__m_redispatched_ready as usize)],
        cd: p!(SOMMCooperativeRedispatchedClassData),
        ccd: p!(SOMMCooperativeRedispatchedCClassData),
    })
}

pub extern "C" fn SOMMBeforeAfterDispatcherNewClass(major: i32, minor: i32) -> usize {
    SOMMMetaCooperativeNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMBeforeAfterDispatcher",
        parents: &["SOMMMetaCooperative"],
        meta: None,
        major,
        minor,
        data: 40,
        align: 4,
        methods: &[
            ("", 0),
            ("", 0),
            ("sommBAMethodCount", coop::__m_ba_method_count as usize),
            ("sommSetBAMethodSeq", coop::__m_set_ba_method_seq as usize),
        ],
        overrides: &[],
        cd: p!(SOMMBeforeAfterDispatcherClassData),
        ccd: p!(SOMMBeforeAfterDispatcherCClassData),
    })
}

pub extern "C" fn SOMMBeforeAfterNewClass(major: i32, minor: i32) -> usize {
    SOMMCooperativeRedispatchedNewClass(2, 1);
    SOMMBeforeAfterDispatcherNewClass(2, 2);
    let c = build_class(&ClassSpec {
        name: "SOMMBeforeAfter",
        parents: &["SOMMCooperativeRedispatched"],
        meta: Some("SOMMBeforeAfterDispatcher"),
        major,
        minor,
        data: 4,
        align: 4,
        methods: &[("sommBeforeMethod", coop::__m_before as usize), ("sommAfterMethod", coop::__m_after as usize)],
        overrides: &[],
        cd: p!(SOMMBeforeAfterClassData),
        ccd: p!(SOMMBeforeAfterCClassData),
    });
    // The varargs stubs (somva_SOMMBeforeAfter_sommBeforeMethod, ...).
    unsafe {
        SOMMBeforeAfterCClassData[2] = coop::__v_before as usize;
        SOMMBeforeAfterCClassData[3] = coop::__v_after as usize;
    }
    c
}

pub extern "C" fn SOMMTracedNewClass(major: i32, minor: i32) -> usize {
    SOMMBeforeAfterNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMTraced",
        parents: &["SOMMBeforeAfter"],
        meta: None,
        major,
        minor,
        data: 1,
        align: 1,
        methods: &[("_set_sommTraceIsOn", coop::__m_set_trace as usize), ("_get_sommTraceIsOn", coop::__m_get_trace as usize)],
        overrides: &[
            ("somInitMIClass", coop::__m_traced_init_mi as usize),
            ("sommBeforeMethod", coop::__m_traced_before as usize),
            ("sommAfterMethod", coop::__m_traced_after as usize),
        ],
        cd: p!(SOMMTracedClassData),
        ccd: p!(SOMMTracedCClassData),
    })
}

/// SOMMProxy::HiddenClassMeta: what makes SOMMSingleInstance's classes
/// keep one instance.
pub fn hidden_class_meta() -> usize {
    SOMMCooperativeNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMProxy::HiddenClassMeta",
        parents: &["SOMMCooperative"],
        meta: None,
        major: 0,
        minor: 0,
        data: 44,
        align: 4,
        methods: &[],
        overrides: &[
            ("somNew", coop::__m_single_new as usize),
            ("somNewNoInit", coop::__m_single_new_no_init as usize),
            ("somClassReady", coop::__m_single_ready as usize),
        ],
        cd: p!(HiddenClassMetaClassData),
        ccd: p!(HiddenClassMetaCClassData),
    })
}

pub fn proxy_meta() -> usize {
    SOMMCooperativeRedispatchedNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMMProxy::ProxyMeta",
        parents: &["SOMMCooperativeRedispatched"],
        meta: None,
        major: 0,
        minor: 0,
        data: 12,
        align: 4,
        methods: &[],
        overrides: &[],
        cd: p!(ProxyMetaClassData),
        ccd: p!(ProxyMetaCClassData),
    })
}

pub extern "C" fn SOMMSingleInstanceNewClass(major: i32, minor: i32) -> usize {
    hidden_class_meta();
    build_class(&ClassSpec {
        name: "SOMMSingleInstance",
        parents: &["SOMMProxy::HiddenClassMeta"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[
            ("sommGetSingleInstance", coop::__m_get_single as usize),
            ("sommFreeSingleInstance", coop::__m_free_single as usize),
        ],
        overrides: &[],
        cd: p!(SOMMSingleInstanceClassData),
        ccd: p!(SOMMSingleInstanceCClassData),
    })
}

pub extern "C" fn SOMUTIdNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "SOMUTId",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[
            ("somutSetIdId", misc::__m_id_set_id as usize),
            ("somutEqualsId", misc::__m_id_equals as usize),
            ("somutCompareId", misc::__m_id_compare as usize),
            ("somutHashId", misc::__m_id_hash as usize),
        ],
        overrides: &[],
        cd: p!(SOMUTIdClassData),
        ccd: p!(SOMUTIdCClassData),
    })
}

pub extern "C" fn SOMUTStringIdNewClass(major: i32, minor: i32) -> usize {
    SOMUTIdNewClass(2, 1);
    build_class(&ClassSpec {
        name: "SOMUTStringId",
        parents: &["SOMUTId"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[
            ("somutSetIdString", misc::__m_sid_set as usize),
            ("somutGetIdString", misc::__m_sid_get as usize),
            ("somutGetIdStringLen", misc::__m_sid_len as usize),
            ("somutEqualsString", misc::__m_sid_equals as usize),
            ("somutCompareString", misc::__m_sid_compare as usize),
        ],
        overrides: &[
            ("somutSetIdId", misc::__m_sid_set_id as usize),
            ("somutEqualsId", misc::__m_sid_equals_id as usize),
            ("somutCompareId", misc::__m_sid_compare_id as usize),
            ("somutHashId", misc::__m_sid_hash_id as usize),
        ],
        cd: p!(SOMUTStringIdClassData),
        ccd: p!(SOMUTStringIdCClassData),
    })
}

pub extern "C" fn SOMBufNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "SOMBuf",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 20,
        align: 4,
        methods: &[
            ("sombfCompare", buf::__m_compare as usize),
            ("sombfCopy", buf::__m_copy as usize),
            ("sombfInit", buf::__m_init_buf as usize),
            ("sombfCount", buf::__m_count as usize),
            ("sombfGetItemSize", buf::__m_item_size as usize),
            ("sombfGet", buf::__m_get as usize),
            ("sombfGetLast", buf::__m_get_last as usize),
            ("sombfOverwrite", buf::__m_overwrite as usize),
            ("sombfInsert", buf::__m_insert as usize),
            ("sombfDelete", buf::__m_delete as usize),
            ("sombfAppend", buf::__m_append as usize),
            ("sombfDeleteLast", buf::__m_delete_last as usize),
        ],
        overrides: &[
            ("somInit", buf::__m_init as usize),
            ("somUninit", buf::__m_uninit as usize),
            ("somPrintSelf", buf::__m_print_self as usize),
            ("somDumpSelfInt", buf::__m_dump_self_int as usize),
        ],
        cd: p!(SOMBufClassData),
        ccd: p!(SOMBufCClassData),
    })
}

pub extern "C" fn SOMDescNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "SOMDesc",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 28,
        align: 4,
        methods: &desc::desc_methods(),
        overrides: &[("somInit", desc::__m_desc_init as usize), ("somUninit", desc::__m_desc_uninit as usize)],
        cd: p!(SOMDescClassData),
        ccd: p!(SOMDescCClassData),
    })
}

pub extern "C" fn SOMArgListNewClass(major: i32, minor: i32) -> usize {
    base();
    let c = build_class(&ClassSpec {
        name: "SOMArgList",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 36,
        align: 4,
        methods: &desc::arglist_methods(),
        overrides: &[("somInit", desc::__m_args_init as usize), ("somUninit", desc::__m_args_uninit as usize)],
        cd: p!(SOMArgListClassData),
        ccd: p!(SOMArgListCClassData),
    });
    unsafe { SOMArgListCClassData[2] = desc::__v_set_args as usize };
    c
}

pub extern "C" fn SOMCommandNewClass(major: i32, minor: i32) -> usize {
    base();
    let c = build_class(&ClassSpec {
        name: "SOMCommand",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 20,
        align: 4,
        methods: &desc::command_methods(),
        overrides: &[("somInit", desc::__m_cmd_init as usize), ("somUninit", desc::__m_cmd_uninit as usize)],
        cd: p!(SOMCommandClassData),
        ccd: p!(SOMCommandCClassData),
    });
    unsafe { SOMCommandCClassData[2] = desc::__v_set_command as usize };
    c
}

aligned! {
    __a_M_somutMsgManagerNewClass => M_somutMsgManagerNewClass, 2;
    __a_somutMsgManagerNewClass => somutMsgManagerNewClass, 2;
    __a_SOMMMetaCooperativeNewClass => SOMMMetaCooperativeNewClass, 2;
    __a_SOMMCooperativeNewClass => SOMMCooperativeNewClass, 2;
    __a_SOMMCooperativeSisteredNewClass => SOMMCooperativeSisteredNewClass, 2;
    __a_SOMMCooperativeRedispatchedNewClass => SOMMCooperativeRedispatchedNewClass, 2;
    __a_SOMMBeforeAfterDispatcherNewClass => SOMMBeforeAfterDispatcherNewClass, 2;
    __a_SOMMBeforeAfterNewClass => SOMMBeforeAfterNewClass, 2;
    __a_SOMMTracedNewClass => SOMMTracedNewClass, 2;
    __a_SOMMSingleInstanceNewClass => SOMMSingleInstanceNewClass, 2;
    __a_SOMUTIdNewClass => SOMUTIdNewClass, 2;
    __a_SOMUTStringIdNewClass => SOMUTStringIdNewClass, 2;
    __a_SOMBufNewClass => SOMBufNewClass, 2;
    __a_SOMDescNewClass => SOMDescNewClass, 2;
    __a_SOMArgListNewClass => SOMArgListNewClass, 2;
    __a_SOMCommandNewClass => SOMCommandNewClass, 2;
}
