//! MSG and NLS: messages, country information, code pages.
//!
//! DosGetMessage is not an entry point: OS2386.LIB's stub calls
//! DosTrueGetMessage (MSG.6) with the program's bound message segment as
//! an extra first argument, which MSG.6 pops itself (`RET 4`). The stub
//! here does the same with a caller-supplied segment (null = none).

#![allow(non_snake_case, clippy::too_many_arguments, clippy::missing_safety_doc)]

use crate::APIRET;

core::arch::global_asm!(
    ".section .text.rs2_call_msgseg,\"ax\"",
    ".globl rs2_call_msgseg",
    // rs2_call_msgseg(entry, args, n, msgseg): push the n dword arguments
    // right to left, then msgseg, and call entry (which pops msgseg).
    "rs2_call_msgseg:",
    "    push ebp",
    "    mov ebp, esp",
    "    push esi",
    "    mov esi, [ebp + 12]",
    "    mov ecx, [ebp + 16]",
    "2:",
    "    test ecx, ecx",
    "    jz 3f",
    "    dec ecx",
    "    push dword ptr [esi + ecx * 4]",
    "    jmp 2b",
    "3:",
    "    push dword ptr [ebp + 20]",
    "    call dword ptr [ebp + 8]",
    "    lea esp, [ebp - 4]",
    "    pop esi",
    "    pop ebp",
    "    ret",
);

unsafe extern "C" {
    fn rs2_call_msgseg(entry: usize, args: *const u32, n: u32, msgseg: u32) -> u32;
    #[link_name = "MSG.6"]
    fn DosTrueGetMessage();
}

/// DosGetMessage with a bound message segment (`msgseg` points at an
/// `FF "MSGSEG32"` header, or is null).
///
/// # Safety
/// All pointers must be valid as for DosGetMessage.
pub unsafe fn DosGetMessageSeg(
    table: *const *const u8,
    count: u32,
    buf: *mut u8,
    cb: u32,
    msgnum: u32,
    file: *const u8,
    pcb: *mut u32,
    msgseg: *const u8,
) -> APIRET {
    let args = [table as u32, count, buf as u32, cb, msgnum, file as u32, pcb as u32];
    unsafe { rs2_call_msgseg(DosTrueGetMessage as usize, args.as_ptr(), 7, msgseg as u32) }
}

/// DosGetMessage: a message from a message file.
///
/// # Safety
/// All pointers must be valid as for DosGetMessage.
pub unsafe fn DosGetMessage(
    table: *const *const u8,
    count: u32,
    buf: *mut u8,
    cb: u32,
    msgnum: u32,
    file: *const u8,
    pcb: *mut u32,
) -> APIRET {
    unsafe { DosGetMessageSeg(table, count, buf, cb, msgnum, file, pcb, core::ptr::null()) }
}

unsafe extern "C" {
    #[link_name = "MSG.4"]
    pub fn DosInsertMessage(
        table: *const *const u8,
        count: u32,
        msg: *const u8,
        cbmsg: u32,
        buf: *mut u8,
        cb: u32,
        pcb: *mut u32,
    ) -> APIRET;
    #[link_name = "MSG.5"]
    pub fn DosPutMessage(hf: u32, cb: u32, msg: *const u8) -> APIRET;
    /// DosIQueryMessageCP: the library stub appends the bound message
    /// segment as a fifth argument.
    #[link_name = "MSG.8"]
    pub fn DosIQueryMessageCP(buf: *mut u8, cb: u32, file: *const u8, pcb: *mut u32, msgseg: *const u8) -> APIRET;

    #[link_name = "NLS.5"]
    pub fn DosQueryCtryInfo(cb: u32, pcc: *const CountryCode, pci: *mut CountryInfo, pcb: *mut u32) -> APIRET;
    #[link_name = "NLS.6"]
    pub fn DosQueryDBCSEnv(cb: u32, pcc: *const CountryCode, buf: *mut u8) -> APIRET;
    #[link_name = "NLS.7"]
    pub fn DosMapCase(cb: u32, pcc: *const CountryCode, buf: *mut u8) -> APIRET;
    #[link_name = "NLS.8"]
    pub fn DosQueryCollate(cb: u32, pcc: *const CountryCode, buf: *mut u8, pcb: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.291"]
    pub fn DosQueryCp(cb: u32, cps: *mut u32, pcb: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.289"]
    pub fn DosSetProcessCp(cp: u32) -> APIRET;
    /// DosSetCp: the process code page and the session's display and
    /// keyboard code pages.
    #[link_name = "DOSCALLS.288"]
    pub fn DosSetCp(cp: u32) -> APIRET;
}

/// DosQueryMessageCP: code pages and language of a message file.
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn DosQueryMessageCP(buf: *mut u8, cb: u32, file: *const u8, pcb: *mut u32) -> APIRET {
    unsafe { DosIQueryMessageCP(buf, cb, file, pcb, core::ptr::null()) }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CountryCode {
    pub country: u32,
    pub codepage: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CountryInfo {
    pub country: u32,
    pub codepage: u32,
    pub date_fmt: u32,
    pub currency: [u8; 5],
    pub thousands: [u8; 2],
    pub decimal: [u8; 2],
    pub date_sep: [u8; 2],
    pub time_sep: [u8; 2],
    pub currency_fmt: u8,
    pub decimals: u8,
    pub time_fmt: u8,
    pub reserved1: [u16; 2],
    pub data_sep: [u8; 2],
    pub reserved2: [u16; 5],
}
