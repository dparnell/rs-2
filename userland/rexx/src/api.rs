//! The REXXAPI and OS/2 entry points REXX.DLL imports beyond the os2
//! crate's.

#![allow(clippy::missing_safety_doc)]

use os2::APIRET;

use crate::RxString;

unsafe extern "C" {
    #[link_name = "REXXAPI.1"]
    pub fn RexxCreateQueue(buf: *mut u8, cb: u32, name: *const u8, dup: *mut u32) -> u32;
    #[link_name = "REXXAPI.2"]
    pub fn RexxDeleteQueue(name: *const u8) -> u32;
    #[link_name = "REXXAPI.3"]
    pub fn RexxAddQueue(name: *const u8, data: *const RxString, flag: u32) -> u32;
    #[link_name = "REXXAPI.4"]
    pub fn RexxPullQueue(name: *const u8, data: *mut RxString, time: *mut u8, wait: u32) -> u32;
    #[link_name = "REXXAPI.5"]
    pub fn RexxQueryQueue(name: *const u8, count: *mut u32) -> u32;
    #[link_name = "REXXAPI.15"]
    pub fn RexxCallExit(name: *const u8, dll: *const u8, func: i32, sub: i32, parm: *mut u8) -> i32;
    #[link_name = "REXXAPI.16"]
    pub fn RexxCallSubcom(env: *const u8, dll: *const u8, cmd: *const RxString, flags: *mut u16, rc: *mut u16, ret: *mut RxString) -> u32;
    #[link_name = "REXXAPI.17"]
    pub fn RexxRegisterFunctionDll(name: *const u8, dll: *const u8, entry: *const u8) -> u32;
    #[link_name = "REXXAPI.19"]
    pub fn RexxDeregisterFunction(name: *const u8) -> u32;
    #[link_name = "REXXAPI.20"]
    pub fn RexxQueryFunction(name: *const u8) -> u32;
    #[link_name = "REXXAPI.21"]
    pub fn RexxCallFunction(name: *const u8, argc: u32, argv: *const RxString, rc: *mut u16, ret: *mut RxString, queue: *const u8) -> u32;
    #[link_name = "REXXAPI.26"]
    pub fn RexxQueryMacro(name: *const u8, pos: *mut u16) -> u32;
    #[link_name = "REXXAPI.29"]
    pub fn RexxExecuteMacroFunction(name: *const u8, image: *mut RxString) -> u32;

    #[link_name = "DOSCALLS.228"]
    pub fn DosSearchPath(flags: u32, path: *const u8, file: *const u8, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.286"]
    pub fn DosBeep(freq: u32, ms: u32) -> APIRET;
    #[link_name = "DOSCALLS.418"]
    pub fn DosAcknowledgeSignalException(num: u32) -> APIRET;
}
