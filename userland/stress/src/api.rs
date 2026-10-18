//! Bindings this program needs beyond the os2 crate.

#![allow(non_snake_case, dead_code)]

use os2::APIRET;

unsafe extern "C" {
    #[link_name = "RS2API.100"]
    pub fn Rs2QueryCounters(buf: *mut u32, cb: u32) -> APIRET;

    #[link_name = "DOSCALLS.111"]
    pub fn DosKillThread(tid: u32) -> APIRET;
    #[link_name = "DOSCALLS.238"]
    pub fn DosSuspendThread(tid: u32) -> APIRET;
    #[link_name = "DOSCALLS.237"]
    pub fn DosResumeThread(tid: u32) -> APIRET;
    #[link_name = "DOSCALLS.236"]
    pub fn DosSetPriority(scope: u32, class: u32, delta: i32, id: u32) -> APIRET;
    #[link_name = "DOSCALLS.379"]
    pub fn DosSendSignalException(pid: u32, exception: u32) -> APIRET;
    #[link_name = "DOSCALLS.418"]
    pub fn DosAcknowledgeSignalException(sig: u32) -> APIRET;
    #[link_name = "DOSCALLS.378"]
    pub fn DosSetSignalExceptionFocus(flag: u32, times: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.380"]
    pub fn DosEnterMustComplete(nesting: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.381"]
    pub fn DosExitMustComplete(nesting: *mut u32) -> APIRET;

    #[link_name = "DOSCALLS.209"]
    pub fn DosSetMaxFH(n: u32) -> APIRET;
    #[link_name = "DOSCALLS.382"]
    pub fn DosSetRelMaxFH(req: *mut i32, max: *mut u32) -> APIRET;

    #[link_name = "DOSCALLS.325"]
    pub fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.330"]
    pub fn DosQueryEventSem(hev: u32, count: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.332"]
    pub fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.336"]
    pub fn DosQueryMutexSem(hmtx: u32, pid: *mut u32, tid: *mut u32, count: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.339"]
    pub fn DosCloseMuxWaitSem(hmux: u32) -> APIRET;
    #[link_name = "DOSCALLS.341"]
    pub fn DosAddMuxWaitSem(hmux: u32, rec: *const os2::SemRecord) -> APIRET;
    #[link_name = "DOSCALLS.342"]
    pub fn DosDeleteMuxWaitSem(hmux: u32, hsem: u32) -> APIRET;

    #[link_name = "DOSCALLS.302"]
    pub fn DosGetSharedMem(pb: *mut u8, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.303"]
    pub fn DosGiveSharedMem(pb: *mut u8, pid: u32, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.298"]
    pub fn DosAliasMem(pb: *const u8, cb: u32, ppb: *mut *mut u8, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.307"]
    pub fn DosQueryMemState(pb: *const u8, pcb: *mut u32, pflags: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.454"]
    pub fn DosAllocThreadLocalMemory(count: u32, pp: *mut *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.455"]
    pub fn DosFreeThreadLocalMemory(p: *mut u32) -> APIRET;

    #[link_name = "DOSCALLS.368"]
    pub fn DosQuerySysState(entity: u32, level: u32, pid: u32, tid: u32, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.976"]
    pub fn DosPerfSysCall(cmd: u32, a1: u32, a2: u32, a3: u32) -> APIRET;
    #[link_name = "DOSCALLS.360"]
    pub fn DosQueryModFromEIP(phmod: *mut u32, pobj: *mut u32, cb: u32, name: *mut u8, poff: *mut u32, eip: u32) -> APIRET;
    #[link_name = "DOSCALLS.211"]
    pub fn DosErrClass(code: u32, class: *mut u32, action: *mut u32, locus: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.876"]
    pub fn DosSysCtl(func: u32, p: *mut u8) -> APIRET;
    #[link_name = "DOSCALLS.285"]
    pub fn DosFSCtl(
        data: *mut u8,
        cb_data: u32,
        pcb_data: *mut u32,
        parm: *mut u8,
        cb_parm: u32,
        pcb_parm: *mut u32,
        func: u32,
        route: *const u8,
        hfile: u32,
        method: u32,
    ) -> APIRET;
    #[link_name = "DOSCALLS.428"]
    pub fn DosSetFileLocks(hf: u32, unlock: *const u32, lock: *const u32, timeout: u32, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.112"]
    pub fn DosQueryRASInfo(index: u32, addr: *mut *mut u8) -> APIRET;
    #[link_name = "DOSCALLS.371"]
    pub fn DosQueryABIOSSupport(reserved: u32) -> APIRET;
    #[link_name = "DOSCALLS.572"]
    pub fn DosQueryResourceSize(hmod: u32, typ: u32, id: u32, pcb: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.997"]
    pub fn DosCreateThread2(tc: *mut u8) -> APIRET;
}

/// DosQuerySysState entity flags.
pub const QS_PROCESS: u32 = 0x0001;
pub const QS_SEMAPHORE: u32 = 0x0002;
pub const QS_MTE: u32 = 0x0004;
pub const QS_FILESYS: u32 = 0x0008;
pub const QS_SHMEMORY: u32 = 0x0010;

pub const XCPT_SIGNAL: u32 = 0xC001_0003;
pub const XCPT_SIGNAL_INTR: u32 = 1;
pub const XCPT_SIGNAL_BREAK: u32 = 4;
pub const EH_NESTED_CALL: u32 = 0x10;
pub const XCPT_UNWIND: u32 = 0xC000_0026;
pub const ERROR_INTERRUPT: u32 = 95;
pub const ERROR_TIMEOUT: u32 = 640;
pub const ERROR_SEM_OWNER_DIED: u32 = 105;
pub const ERROR_SEM_BUSY: u32 = 301;
pub const ERROR_INVALID_HANDLE: u32 = 6;
pub const DCWA_PROCESSTREE: u32 = 1;
