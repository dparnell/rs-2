//! REXXUTIL.DLL: the REXX utility function package (SysLoadFuncs,
//! SysFileTree, SysIni, SysStemSort, ...), binary compatible with IBM
//! Warp 4.52's: the same 85 exports with IBM's ordinals and upper-case
//! names, the same argument checks (REXX error 40 for a bad call), result
//! strings and stem variables.
//!
//! Every export is a REXX external function, `ULONG _System f(PUCHAR
//! name, ULONG argc, RXSTRING argv[], PSZ queue, PRXSTRING ret)`. The
//! interpreter passes a 256-byte result buffer; longer results get their
//! own memory (DosAllocMem), which the interpreter frees. Stems are read
//! and written through RexxVariablePool (REXX.2); SysLoadFuncs registers
//! the functions with RexxRegisterFunctionDll (REXXAPI.17).
//!
//! The Workplace Shell, switch list and profile functions reach PMWIN,
//! PMSHAPI and PMWP at run time (DosLoadModule and DosQueryProcAddr by
//! ordinal), as IBM's does, so that REXX programs that use none of them
//! never load PM.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod api;
mod files;
mod ini;
mod misc;
mod rx;
mod screen;
mod stems;
mod wps;

use rx::{Args, Ret, RxString};

/// _DLL_InitTerm(hmod, flag): nothing to set up.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// The exported entry points: each realigns the stack to 16 bytes (the
/// interpreter calls with 4-byte alignment) and calls the function with
/// its arguments wrapped.
macro_rules! rexx_functions {
    ($($tramp:ident => $f:path;)*) => {$(
        mod $tramp {
            use super::*;
            pub extern "C" fn shim(argc: u32, argv: *const RxString, queue: *const u8, ret: *mut RxString) -> u32 {
                $f(&Args::new(argc, argv, queue), &mut Ret::new(ret))
            }
        }
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
                "mov eax, [ebp + 12]", "mov [esp], eax",
                "mov eax, [ebp + 16]", "mov [esp + 4], eax",
                "mov eax, [ebp + 20]", "mov [esp + 8], eax",
                "mov eax, [ebp + 24]", "mov [esp + 12], eax",
                "call {f}",
                "mov esp, ebp", "pop ebp", "ret",
                f = sym $tramp::shim,
            );
        }
    )*};
}

rexx_functions! {
    __a_SysCls => screen::sys_cls;
    __a_SysCurPos => screen::sys_cur_pos;
    __a_SysCurState => screen::sys_cur_state;
    __a_SysDriveInfo => files::sys_drive_info;
    __a_SysDriveMap => files::sys_drive_map;
    __a_SysDropFuncs => sys_drop_funcs;
    __a_SysFileDelete => files::sys_file_delete;
    __a_SysFileSearch => files::sys_file_search;
    __a_SysFileTree => files::sys_file_tree;
    __a_SysGetKey => screen::sys_get_key;
    __a_SysGetMessage => misc::sys_get_message;
    __a_SysIni => ini::sys_ini;
    __a_SysLoadFuncs => sys_load_funcs;
    __a_SysMkDir => files::sys_mkdir;
    __a_SysOS2Ver => misc::sys_os2_ver;
    __a_SysRmDir => files::sys_rmdir;
    __a_SysSearchPath => files::sys_search_path;
    __a_SysSleep => misc::sys_sleep;
    __a_SysTempFileName => files::sys_temp_file_name;
    __a_SysTextScreenRead => screen::sys_text_screen_read;
    __a_SysTextScreenSize => screen::sys_text_screen_size;
    __a_SysGetEA => files::sys_get_ea;
    __a_SysPutEA => files::sys_put_ea;
    __a_SysWaitNamedPipe => misc::sys_wait_named_pipe;
    __a_SysSetIcon => wps::sys_set_icon;
    __a_SysRegisterObjectClass => wps::sys_register_object_class;
    __a_SysDeregisterObjectClass => wps::sys_deregister_object_class;
    __a_SysCreateObject => wps::sys_create_object;
    __a_SysQueryClassList => wps::sys_query_class_list;
    __a_SysDestroyObject => wps::sys_destroy_object;
    __a_SysSetObjectData => wps::sys_set_object_data;
    __a_SysSaveObject => wps::sys_save_object;
    __a_SysOpenObject => wps::sys_open_object;
    __a_SysMoveObject => wps::sys_move_object;
    __a_SysCopyObject => wps::sys_copy_object;
    __a_SysCreateShadow => wps::sys_create_shadow;
    __a_SysBootDrive => misc::sys_boot_drive;
    __a_SysQueryEAList => files::sys_query_ea_list;
    __a_SysWildCard => files::sys_wild_card;
    __a_SysFileSystemType => files::sys_file_system_type;
    __a_SysAddFileHandle => misc::sys_add_file_handle;
    __a_SysSetFileHandle => misc::sys_set_file_handle;
    __a_SysCreateMutexSem => misc::sys_create_mutex_sem;
    __a_SysOpenMutexSem => misc::sys_open_mutex_sem;
    __a_SysCloseMutexSem => misc::sys_close_mutex_sem;
    __a_SysRequestMutexSem => misc::sys_request_mutex_sem;
    __a_SysReleaseMutexSem => misc::sys_release_mutex_sem;
    __a_SysCreateEventSem => misc::sys_create_event_sem;
    __a_SysOpenEventSem => misc::sys_open_event_sem;
    __a_SysCloseEventSem => misc::sys_close_event_sem;
    __a_SysResetEventSem => misc::sys_reset_event_sem;
    __a_SysPostEventSem => misc::sys_post_event_sem;
    __a_SysWaitEventSem => misc::sys_wait_event_sem;
    __a_SysSetPriority => misc::sys_set_priority;
    __a_SysProcessType => misc::sys_process_type;
    __a_SysGetCollate => misc::sys_get_collate;
    __a_SysNationalLanguageCompare => misc::sys_national_language_compare;
    __a_SysMapCase => misc::sys_map_case;
    __a_SysSetProcessCodePage => misc::sys_set_process_code_page;
    __a_SysQueryProcessCodePage => misc::sys_query_process_code_page;
    __a_SysAddRexxMacro => misc::sys_add_rexx_macro;
    __a_SysDropRexxMacro => misc::sys_drop_rexx_macro;
    __a_SysReorderRexxMacro => misc::sys_reorder_rexx_macro;
    __a_SysQueryRexxMacro => misc::sys_query_rexx_macro;
    __a_SysClearRexxMacroSpace => misc::sys_clear_rexx_macro_space;
    __a_SysLoadRexxMacroSpace => misc::sys_load_rexx_macro_space;
    __a_SysSaveRexxMacroSpace => misc::sys_save_rexx_macro_space;
    __a_SysShutDownSystem => wps::sys_shutdown_system;
    __a_SysSwitchSession => wps::sys_switch_session;
    __a_SysLoadLibrary => misc::sys_load_library;
    __a_SysDropLibrary => misc::sys_drop_library;
    __a_SysElapsedTime => misc::sys_elapsed_time;
    __a_SysWaitForShell => wps::sys_wait_for_shell;
    __a_SysQuerySwitchList => wps::sys_query_switch_list;
    __a_SysQueryExtLIBPATH => misc::sys_query_ext_libpath;
    __a_SysSetExtLIBPATH => misc::sys_set_ext_libpath;
    __a_SysDumpVariables => stems::sys_dump_variables;
    __a_SysSetFileDateTime => files::sys_set_file_date_time;
    __a_SysGetFileDateTime => files::sys_get_file_date_time;
    __a_SysStemSort => stems::sys_stem_sort;
    __a_SysStemDelete => stems::sys_stem_delete;
    __a_SysStemInsert => stems::sys_stem_insert;
    __a_SysStemCopy => stems::sys_stem_copy;
    __a_SysVersion => misc::sys_version;
    __a_SysUtilVersion => misc::sys_util_version;
}

/// The names SysLoadFuncs registers and SysDropFuncs drops, in IBM's
/// order (SysGetEA appears twice there too).
const FUNCTIONS: &[&[u8]] = &[
    b"SysBootDrive\0", b"SysElapsedTime\0", b"SysQueryEAList\0", b"SysWildCard\0",
    b"SysFileSystemType\0", b"SysAddFileHandle\0", b"SysSetFileHandle\0",
    b"SysCreateMutexSem\0", b"SysOpenMutexSem\0", b"SysCloseMutexSem\0",
    b"SysRequestMutexSem\0", b"SysReleaseMutexSem\0", b"SysCreateEventSem\0",
    b"SysOpenEventSem\0", b"SysCloseEventSem\0", b"SysResetEventSem\0",
    b"SysPostEventSem\0", b"SysWaitEventSem\0", b"SysSetPriority\0", b"SysProcessType\0",
    b"SysGetCollate\0", b"SysNationalLanguageCompare\0", b"SysMapCase\0",
    b"SysSetProcessCodePage\0", b"SysQueryProcessCodePage\0", b"SysAddRexxMacro\0",
    b"SysDropRexxMacro\0", b"SysReorderRexxMacro\0", b"SysQueryRexxMacro\0",
    b"SysClearRexxMacroSpace\0", b"SysLoadRexxMacroSpace\0", b"SysSaveRexxMacroSpace\0",
    b"SysLoadLibrary\0", b"SysDropLibrary\0", b"SysCls\0", b"SysCurPos\0", b"SysCurState\0",
    b"SysDriveInfo\0", b"SysDriveMap\0", b"SysDropFuncs\0", b"SysFileDelete\0",
    b"SysFileSearch\0", b"SysFileTree\0", b"SysGetEA\0", b"SysGetKey\0", b"SysGetMessage\0",
    b"SysMkDir\0", b"SysOS2Ver\0", b"SysVersion\0", b"SysRmDir\0", b"SysSearchPath\0",
    b"SysSleep\0", b"SysTempFileName\0", b"SysTextScreenRead\0", b"SysTextScreenSize\0",
    b"SysGetEA\0", b"SysPutEA\0", b"SysWaitNamedPipe\0", b"SysIni\0", b"SysShutDownSystem\0",
    b"SysSwitchSession\0", b"SysSetIcon\0", b"SysRegisterObjectClass\0",
    b"SysDeregisterObjectClass\0", b"SysCreateObject\0", b"SysQueryClassList\0",
    b"SysDestroyObject\0", b"SysSetObjectData\0", b"SysSaveObject\0", b"SysOpenObject\0",
    b"SysMoveObject\0", b"SysCopyObject\0", b"SysCreateShadow\0", b"SysWaitForShell\0",
    b"SysQuerySwitchList\0", b"SysQueryExtLIBPATH\0", b"SysDumpVariables\0",
    b"SysSetFileDateTime\0", b"SysGetFileDateTime\0", b"SysStemSort\0", b"SysStemDelete\0",
    b"SysStemInsert\0", b"SysStemCopy\0", b"SysUtilVersion\0", b"SysSetExtLIBPATH\0",
    b"SysLoadFuncs\0",
];

/// SysLoadFuncs(): register every function; returns "".
fn sys_load_funcs(a: &Args, r: &mut Ret) -> u32 {
    r.set(b"");
    if a.n() > 0 {
        return rx::INVALID;
    }
    for f in FUNCTIONS {
        unsafe { api::RexxRegisterFunctionDll(f.as_ptr(), b"REXXUTIL\0".as_ptr(), f.as_ptr()) };
    }
    0
}

/// SysDropFuncs(): drop every function; returns "".
fn sys_drop_funcs(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return rx::INVALID;
    }
    r.set(b"");
    for f in FUNCTIONS {
        unsafe { api::RexxDeregisterFunction(f.as_ptr()) };
    }
    0
}
