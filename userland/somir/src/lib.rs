//! SOMIR.DLL: SOM 2.1's interface repository as OS/2 Warp 4.52 has it
//! (studied in IBM's SOMIR.DLL with Ghidra), binary compatible with it:
//! IBM's ordinals, the classes' parents, versions, instance data sizes
//! and class data order (IBM's private methods included), and it reads
//! IBM's .IR files (format in docs/formats/som-ir.md) from the files the
//! SOMIR environment variable lists (default SOM.IR).
//!
//! The repository is read-only here: the update calls (saveIR, the
//! _set_ methods' persistence, irEnableUpdates) change nothing on disk.
//! Objects are made from the file's records when first looked up and
//! kept; somFree on one counts its uses down.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer, static_mut_refs)]

extern crate alloc;
extern crate os2;

pub mod buf;
pub mod file;
pub mod methods;
pub mod model;
pub mod tc;

use methods::*;
use somsup::{ClassSpec, aligned, build_class, optlink};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, _term: u32) -> u32 {
    1
}

macro_rules! class_data {
    ($($cd:ident, $ccd:ident, $n:literal;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; $n] = [0; $n];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

class_data! {
    ContainedClassData, ContainedCClassData, 36;
    ContainerClassData, ContainerCClassData, 23;
    AttributeDefClassData, AttributeDefCClassData, 6;
    TypeDefClassData, TypeDefCClassData, 4;
    RepositoryClassData, RepositoryCClassData, 7;
    M_RepositoryClassData, M_RepositoryCClassData, 3;
    ConstantDefClassData, ConstantDefCClassData, 6;
    ExceptionDefClassData, ExceptionDefCClassData, 4;
    InterfaceDefClassData, InterfaceDefCClassData, 8;
    ModuleDefClassData, ModuleDefCClassData, 2;
    OperationDefClassData, OperationDefCClassData, 10;
    ParameterDefClassData, ParameterDefCClassData, 6;
}

/// IBM's private class keeping one file.
pub static mut SubRepositoryClassData: [usize; 21] = [0; 21];
pub static mut SubRepositoryCClassData: [usize; 8] = [0; 8];

macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}
pub(crate) use p;

fn base() {
    unsafe {
        somsup::SOMObjectNewClass(1, 4);
        somsup::SOMClassNewClass(1, 4);
    }
}

pub extern "C" fn ContainedNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "Contained",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 64,
        align: 4,
        methods: &[
            ("*within", __m_within as usize),
            ("*describe", __m_describe as usize),
            ("_get_name", __m_get_name as usize),
            ("_set_name", __m_set_name as usize),
            ("_get_id", __m_get_id as usize),
            ("_set_id", __m_set_id as usize),
            ("_get_defined_in", __m_get_defined_in as usize),
            ("_set_defined_in", __m_set_defined_in as usize),
            ("*_get_somModifiers", __m_get_modifiers as usize),
            ("_set_somModifiers", __m_set_modifiers as usize),
            ("_get_repository", __m_get_repository as usize),
            ("_set_repository", __m_set_repository as usize),
            ("_get_reference_list", 0),
            ("_set_reference_list", 0),
            ("_get_iid", __m_get_iid as usize),
            ("_get_defined_in_obj", __m_get_defined_in_obj as usize),
            ("getFixedPersistentSize", 0),
            ("getVariablePersistentSize", 0),
            ("putDataInFixedBuf", 0),
            ("putDataInVariableBuf", 0),
            ("getDataFromFixedBuf", 0),
            ("getDataFromVariableBuf", 0),
            ("saveToIR", 0),
            ("fixedDirty", 0),
            ("variableDirty", 0),
            ("addToReferenceList", 0),
            ("allocDef", 0),
            ("removeFromReferenceList", 0),
            ("incrementUseCount", __m_increment_uses as usize),
            ("deleteFromIR", 0),
            ("forceFree", 0),
            ("freeUnreferenced", 0),
            ("decrementUseCount", __m_decrement_uses as usize),
            ("", 0),
            ("", 0),
        ],
        overrides: &[("somFree", __m_contained_free as usize)],
        cd: p!(ContainedClassData),
        ccd: p!(ContainedCClassData),
    })
}

pub extern "C" fn ContainerNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "Container",
        parents: &["SOMObject"],
        meta: None,
        major,
        minor,
        data: 12,
        align: 4,
        methods: &[
            ("*contents", __m_contents as usize),
            ("*lookup_name", __m_lookup_name as usize),
            ("*describe_contents", __m_describe_contents as usize),
            ("internalNameLookup", __m_internal_lookup as usize),
            ("", 0),
            ("", 0),
            ("addToContents", 0),
            ("removeFromContents", 0),
            ("getContainerFixedPersistentSize", 0),
            ("putContainerDataInFixedBuf", 0),
            ("getContainerDataFromFixedBuf", 0),
            ("_set_iidDirectory", 0),
            ("_get_iidDirectory", 0),
            ("saveContents", 0),
            ("restoreContents", 0),
            ("getContentsSize", 0),
            ("deleteContainerFromIR", 0),
            ("_get_dirdirtybit", 0),
            ("_set_dirdirtybit", 0),
            ("gatherSubclasses", 0),
            ("_get_mycontents", 0),
            ("_set_mycontents", 0),
        ],
        overrides: &[],
        cd: p!(ContainerClassData),
        ccd: p!(ContainerCClassData),
    })
}

/// The definitions: one parent (Contained), or Contained and Container.
fn def(name: &str, container: bool, data: u32, methods: &[(&str, usize)], overrides: &[(&str, usize)], cd: *mut usize, ccd: *mut usize, major: i32, minor: i32) -> usize {
    ContainedNewClass(2, 3);
    if container {
        ContainerNewClass(2, 3);
    }
    build_class(&ClassSpec {
        name,
        parents: if container { &["Contained", "Container"] } else { &["Contained"] },
        meta: None,
        major,
        minor,
        data,
        align: if data == 0 { 0 } else { 4 },
        methods,
        overrides,
        cd,
        ccd,
    })
}

pub extern "C" fn AttributeDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "AttributeDef",
        false,
        8,
        &[
            ("_get_type", __m_attr_get_type as usize),
            ("_set_type", __m_attr_set_type as usize),
            ("_get_mode", __m_attr_get_mode as usize),
            ("_set_mode", __m_attr_set_mode as usize),
            ("", 0),
        ],
        &[("describe", __m_describe_attribute as usize)],
        p!(AttributeDefClassData),
        p!(AttributeDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn TypeDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "TypeDef",
        false,
        4,
        &[("_get_type", __m_typedef_get_type as usize), ("_set_type", __m_typedef_set_type as usize), ("", 0)],
        &[("describe", __m_describe_typedef as usize)],
        p!(TypeDefClassData),
        p!(TypeDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn ConstantDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "ConstantDef",
        false,
        8,
        &[
            ("_get_type", __m_const_get_type as usize),
            ("_set_type", __m_const_set_type as usize),
            ("*_get_value", __m_const_get_value as usize),
            ("_set_value", __m_const_set_value as usize),
            ("", 0),
        ],
        &[("describe", __m_describe_constant as usize)],
        p!(ConstantDefClassData),
        p!(ConstantDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn ExceptionDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "ExceptionDef",
        false,
        4,
        &[("_get_type", __m_except_get_type as usize), ("_set_type", __m_except_set_type as usize), ("", 0)],
        &[("describe", __m_describe_exception as usize)],
        p!(ExceptionDefClassData),
        p!(ExceptionDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn ParameterDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "ParameterDef",
        false,
        8,
        &[
            ("_get_type", __m_param_get_type as usize),
            ("_set_type", __m_param_set_type as usize),
            ("_get_mode", __m_param_get_mode as usize),
            ("_set_mode", __m_param_set_mode as usize),
            ("", 0),
        ],
        &[("describe", __m_describe_parameter as usize)],
        p!(ParameterDefClassData),
        p!(ParameterDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn InterfaceDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "InterfaceDef",
        true,
        16,
        &[
            ("*describe_interface", __m_describe_interface as usize),
            ("*_get_base_interfaces", __m_get_bases as usize),
            ("_set_base_interfaces", __m_set_bases as usize),
            ("_get_instanceData", __m_get_instance_data as usize),
            ("_set_instanceData", __m_set_instance_data as usize),
            ("subclasses", __m_iface_subclasses as usize),
            ("", 0),
        ],
        &[("describe", __m_describe_interface_brief as usize), ("within", __m_iface_within as usize)],
        p!(InterfaceDefClassData),
        p!(InterfaceDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn ModuleDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "ModuleDef",
        true,
        0,
        &[("", 0)],
        &[("describe", __m_describe_module as usize), ("within", __m_iface_within as usize)],
        p!(ModuleDefClassData),
        p!(ModuleDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn OperationDefNewClass(major: i32, minor: i32) -> usize {
    def(
        "OperationDef",
        true,
        32,
        &[
            ("_get_result", __m_op_get_result as usize),
            ("_set_result", __m_op_set_result as usize),
            ("_get_mode", __m_op_get_mode as usize),
            ("_set_mode", __m_op_set_mode as usize),
            ("*_get_contexts", __m_op_get_contexts as usize),
            ("_set_contexts", __m_op_set_contexts as usize),
            ("*_get_raiseSeq", __m_op_get_raises as usize),
            ("_set_raiseSeq", __m_op_set_raises as usize),
            ("", 0),
        ],
        &[("describe", __m_describe_operation as usize)],
        p!(OperationDefClassData),
        p!(OperationDefCClassData),
        major,
        minor,
    )
}

pub extern "C" fn M_RepositoryNewClass(major: i32, minor: i32) -> usize {
    base();
    build_class(&ClassSpec {
        name: "M_Repository",
        parents: &["SOMClass"],
        meta: None,
        major,
        minor,
        data: 8,
        align: 4,
        methods: &[("createRepository", __m_create_repository as usize), ("createRepositoryFromPathname", __m_create_from_pathname as usize)],
        overrides: &[("somNew", __m_repository_new as usize)],
        cd: p!(M_RepositoryClassData),
        ccd: p!(M_RepositoryCClassData),
    })
}

pub extern "C" fn RepositoryNewClass(major: i32, minor: i32) -> usize {
    ContainerNewClass(2, 3);
    M_RepositoryNewClass(2, 3);
    build_class(&ClassSpec {
        name: "Repository",
        parents: &["Container"],
        meta: Some("M_Repository"),
        major,
        minor,
        data: 20,
        align: 4,
        methods: &[
            ("lookup_id", __m_lookup_id as usize),
            ("lookup_modifier", __m_lookup_modifier as usize),
            ("release_cache", __m_release_cache as usize),
            ("saveIR", __m_save_ir as usize),
            ("*_get_subrepositorylist", __m_get_subrepositories as usize),
            ("_set_subrepositorylist", 0),
        ],
        overrides: &[],
        cd: p!(RepositoryClassData),
        ccd: p!(RepositoryCClassData),
    })
}

pub extern "C" fn SubRepositoryNewClass(major: i32, minor: i32) -> usize {
    ContainerNewClass(2, 3);
    build_class(&ClassSpec {
        name: "SubRepository",
        parents: &["Container"],
        meta: None,
        major,
        minor,
        data: 288,
        align: 4,
        methods: &[
            ("_get_filehandle", 0),
            ("_set_filehandle", 0),
            ("_get_uselist", 0),
            ("_set_uselist", 0),
            ("_set_mainrepository", 0),
            ("_get_mainrepository", 0),
            ("_get_filename", 0),
            ("_set_filename", 0),
            ("locallookup_id", 0),
            ("saveIRFile", 0),
            ("addToUseList", 0),
            ("searchUseList", 0),
            ("removeFromUseList", 0),
            ("_get_allowUpdates", 0),
            ("_set_allowUpdates", 0),
            ("_get_mallocInfo", 0),
            ("_set_mallocInfo", 0),
            ("establishUpdateMode", 0),
            ("", 0),
            ("", 0),
        ],
        overrides: &[],
        cd: p!(SubRepositoryClassData),
        ccd: p!(SubRepositoryCClassData),
    })
}

/// SOMInitModule: every class (the class manager calls it when it loads
/// the DLL).
pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: *const u8) {
    RepositoryNewClass(2, 3);
    AttributeDefNewClass(2, 3);
    ConstantDefNewClass(2, 3);
    ExceptionDefNewClass(2, 3);
    InterfaceDefNewClass(2, 3);
    ModuleDefNewClass(2, 3);
    OperationDefNewClass(2, 3);
    ParameterDefNewClass(2, 3);
    TypeDefNewClass(2, 3);
}

// ---- The file functions ----

extern "C" fn ir_get_file_names() -> *const u8 {
    file::file_names()
}

static mut LAST: [u8; 260] = [0; 260];

/// irGetLastFileName: the last of SOMIR's files (where IBM's writes go).
extern "C" fn ir_get_last_file_name() -> *const u8 {
    let names = unsafe { somsup::cstr(file::file_names()) };
    let last = names.split(|&c| c == b';').filter(|n| !n.is_empty()).last().unwrap_or(b"");
    let n = last.len().min(259);
    unsafe {
        LAST[..n].copy_from_slice(&last[..n]);
        LAST[n] = 0;
        LAST.as_ptr()
    }
}

extern "C" fn ir_nothing() {}

unsafe extern "C" {
    #[link_name = "DOSCALLS.256"]
    fn DosSetFilePtr(h: u32, off: i32, method: u32, new: *mut u32) -> u32;
}

/// irRead(file, offset, buffer, size) (_Optlink): a record's size into
/// *size, and the record into the buffer when there is one; TRUE when it
/// could not be read.
pub extern "C" fn irRead(h: u32, off: u32, buf: *mut u8, size: *mut u32) -> u32 {
    if size.is_null() {
        return 1;
    }
    let mut at = 0u32;
    let mut b = [0u8; 4];
    let mut n = 0u32;
    unsafe {
        if DosSetFilePtr(h, off as i32, 0, &mut at) != 0 || os2::DosRead(h, b.as_mut_ptr(), 4, &mut n) != 0 || n != 4 {
            return 1;
        }
        let len = u32::from_be_bytes(b);
        *size = len;
        if buf.is_null() {
            return 0;
        }
        if DosSetFilePtr(h, off as i32, 0, &mut at) != 0 || os2::DosRead(h, buf, len, &mut n) != 0 || n != len {
            return 1;
        }
    }
    0
}

aligned! {
    __a_ContainedNewClass => ContainedNewClass, 2;
    __a_ContainerNewClass => ContainerNewClass, 2;
    __a_AttributeDefNewClass => AttributeDefNewClass, 2;
    __a_TypeDefNewClass => TypeDefNewClass, 2;
    __a_ConstantDefNewClass => ConstantDefNewClass, 2;
    __a_ExceptionDefNewClass => ExceptionDefNewClass, 2;
    __a_ParameterDefNewClass => ParameterDefNewClass, 2;
    __a_InterfaceDefNewClass => InterfaceDefNewClass, 2;
    __a_ModuleDefNewClass => ModuleDefNewClass, 2;
    __a_OperationDefNewClass => OperationDefNewClass, 2;
    __a_M_RepositoryNewClass => M_RepositoryNewClass, 2;
    __a_RepositoryNewClass => RepositoryNewClass, 2;
    __a_SOMInitModule => SOMInitModule, 3;
    __a_irGetFileNames => ir_get_file_names, 0;
    __a_irGetLastFileName => ir_get_last_file_name, 0;
    __a_irEnableUpdates => ir_nothing, 0;
    __a_irDisableUpdates => ir_nothing, 0;
}

optlink! {
    __o_irRead => irRead, 4;
}
