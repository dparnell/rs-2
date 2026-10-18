//! DOSCALLS (32-bit).

#![allow(non_snake_case, clippy::missing_safety_doc)]

pub type APIRET = u32;

pub const NO_ERROR: APIRET = 0;
pub const EXIT_THREAD: u32 = 0;
pub const EXIT_PROCESS: u32 = 1;

pub const PAG_READ: u32 = 0x0001;
pub const PAG_WRITE: u32 = 0x0002;
pub const PAG_EXECUTE: u32 = 0x0004;
pub const PAG_GUARD: u32 = 0x0008;
pub const PAG_COMMIT: u32 = 0x0010;
pub const PAG_DECOMMIT: u32 = 0x0020;
pub const OBJ_TILE: u32 = 0x0040;
pub const PAG_FREE: u32 = 0x4000;
pub const PAG_BASE: u32 = 0x10000;

pub const SEM_INDEFINITE_WAIT: u32 = 0xffff_ffff;
pub const SEM_IMMEDIATE_RETURN: u32 = 0;
pub const DC_SEM_SHARED: u32 = 1;
pub const DCMW_WAIT_ANY: u32 = 2;
pub const DCMW_WAIT_ALL: u32 = 4;

pub const EXLST_ADD: u32 = 1;
pub const EXLST_REMOVE: u32 = 2;
pub const EXLST_EXIT: u32 = 3;

pub const DCWW_WAIT: u32 = 0;
pub const DCWW_NOWAIT: u32 = 1;
pub const DCWA_PROCESS: u32 = 0;
pub const EXEC_SYNC: u32 = 0;
pub const EXEC_ASYNCRESULT: u32 = 2;

pub const XCPT_CONTINUE_SEARCH: u32 = 0;
pub const XCPT_CONTINUE_EXECUTION: u32 = 0xffff_ffff;
pub const XCPT_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const XCPT_INTEGER_DIVIDE_BY_ZERO: u32 = 0xC000_009B;
pub const EH_UNWINDING: u32 = 0x02;

pub const FILE_BEGIN: u32 = 0;
pub const FILE_CURRENT: u32 = 1;
pub const FILE_END: u32 = 2;
pub const OPEN_ACTION_FAIL_IF_NEW: u32 = 0x0000;
pub const OPEN_ACTION_CREATE_IF_NEW: u32 = 0x0010;
pub const OPEN_ACTION_OPEN_IF_EXISTS: u32 = 0x0001;
pub const OPEN_ACTION_REPLACE_IF_EXISTS: u32 = 0x0002;
pub const OPEN_ACCESS_READONLY: u32 = 0x0000;
pub const OPEN_ACCESS_READWRITE: u32 = 0x0002;
pub const OPEN_SHARE_DENYNONE: u32 = 0x0040;
pub const OPEN_SHARE_DENYWRITE: u32 = 0x0020;
pub const FILE_NORMAL: u32 = 0;
pub const FILE_DIRECTORY: u32 = 0x10;
pub const HDIR_CREATE: u32 = 0xffff_ffff;
pub const FIL_STANDARD: u32 = 1;

#[repr(C)]
pub struct Tib {
    pub pexchain: *mut ExceptionRegistrationRecord,
    pub pstack: u32,
    pub pstacklimit: u32,
    pub ptib2: *const Tib2,
    pub version: u32,
    pub ordinal: u32,
}

#[repr(C)]
pub struct Tib2 {
    pub tid: u32,
    pub pri: u32,
    pub version: u32,
    pub mc_count: u16,
    pub mc_force: u16,
}

#[repr(C)]
pub struct Pib {
    pub pid: u32,
    pub ppid: u32,
    pub hmte: u32,
    pub pchcmd: *const u8,
    pub pchenv: *const u8,
    pub flstatus: u32,
    pub ultype: u32,
}

#[repr(C)]
pub struct ExceptionReportRecord {
    pub num: u32,
    pub flags: u32,
    pub nested: *mut ExceptionReportRecord,
    pub address: u32,
    pub cparams: u32,
    pub info: [u32; 4],
}

pub type ExceptionHandler = unsafe extern "C" fn(
    *mut ExceptionReportRecord,
    *mut ExceptionRegistrationRecord,
    *mut ContextRecord,
    *mut u8,
) -> u32;

#[repr(C)]
pub struct ExceptionRegistrationRecord {
    pub prev: *mut ExceptionRegistrationRecord,
    pub handler: ExceptionHandler,
}

#[repr(C)]
pub struct ContextRecord {
    pub flags: u32,
    pub env: [u32; 7],
    pub fpstack: [u8; 80],
    pub gs: u32,
    pub fs: u32,
    pub es: u32,
    pub ds: u32,
    pub edi: u32,
    pub esi: u32,
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebp: u32,
    pub eip: u32,
    pub cs: u32,
    pub eflags: u32,
    pub esp: u32,
    pub ss: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct DateTime {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub hundredths: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
    pub timezone: i16,
    pub weekday: u8,
}

#[repr(C)]
pub struct ResultCodes {
    pub code_terminate: u32,
    pub code_result: u32,
}

#[repr(C)]
pub struct FileFindBuf3 {
    pub next: u32,
    pub fdate_creation: u16,
    pub ftime_creation: u16,
    pub fdate_access: u16,
    pub ftime_access: u16,
    pub fdate_write: u16,
    pub ftime_write: u16,
    pub cb_file: u32,
    pub cb_alloc: u32,
    pub attr: u32,
    pub cch_name: u8,
    pub name: [u8; 256],
}

#[repr(C)]
pub struct SemRecord {
    pub hsem: u32,
    pub user: u32,
}

pub type ThreadFn = unsafe extern "C" fn(u32);
pub type ExitListFn = unsafe extern "C" fn(u32);

unsafe extern "C" {
    #[link_name = "DOSCALLS.234"]
    pub fn DosExit(action: u32, result: u32);
    #[link_name = "DOSCALLS.282"]
    pub fn DosWrite(hf: u32, buf: *const u8, cb: u32, actual: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.281"]
    pub fn DosRead(hf: u32, buf: *mut u8, cb: u32, actual: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.273"]
    pub fn DosOpen(
        name: *const u8,
        phf: *mut u32,
        action: *mut u32,
        cb: u32,
        attr: u32,
        flags: u32,
        mode: u32,
        eaop: *mut u8,
    ) -> APIRET;
    #[link_name = "DOSCALLS.257"]
    pub fn DosClose(hf: u32) -> APIRET;
    #[link_name = "DOSCALLS.256"]
    pub fn DosSetFilePtr(hf: u32, off: i32, method: u32, actual: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.259"]
    pub fn DosDelete(name: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.270"]
    pub fn DosCreateDir(name: *const u8, eaop: *mut u8) -> APIRET;
    #[link_name = "DOSCALLS.226"]
    pub fn DosDeleteDir(name: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.264"]
    pub fn DosFindFirst(spec: *const u8, phdir: *mut u32, attr: u32, buf: *mut u8, cb: u32, count: *mut u32, level: u32) -> APIRET;
    #[link_name = "DOSCALLS.265"]
    pub fn DosFindNext(hdir: u32, buf: *mut u8, cb: u32, count: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.263"]
    pub fn DosFindClose(hdir: u32) -> APIRET;
    #[link_name = "DOSCALLS.312"]
    pub fn DosGetInfoBlocks(tib: *mut *mut Tib, pib: *mut *mut Pib) -> APIRET;
    #[link_name = "DOSCALLS.299"]
    pub fn DosAllocMem(ppb: *mut *mut u8, cb: u32, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.304"]
    pub fn DosFreeMem(pb: *mut u8) -> APIRET;
    #[link_name = "DOSCALLS.305"]
    pub fn DosSetMem(pb: *mut u8, cb: u32, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.306"]
    pub fn DosQueryMem(pb: *mut u8, pcb: *mut u32, pflags: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.229"]
    pub fn DosSleep(ms: u32) -> APIRET;
    #[link_name = "DOSCALLS.230"]
    pub fn DosGetDateTime(dt: *mut DateTime) -> APIRET;
    #[link_name = "DOSCALLS.348"]
    pub fn DosQuerySysInfo(first: u32, last: u32, buf: *mut u32, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.311"]
    pub fn DosCreateThread(ptid: *mut u32, pfn: ThreadFn, param: u32, flags: u32, cb_stack: u32) -> APIRET;
    #[link_name = "DOSCALLS.349"]
    pub fn DosWaitThread(ptid: *mut u32, option: u32) -> APIRET;
    #[link_name = "DOSCALLS.232"]
    pub fn DosEnterCritSec() -> APIRET;
    #[link_name = "DOSCALLS.233"]
    pub fn DosExitCritSec() -> APIRET;
    #[link_name = "DOSCALLS.324"]
    pub fn DosCreateEventSem(name: *const u8, phev: *mut u32, attr: u32, posted: u32) -> APIRET;
    #[link_name = "DOSCALLS.326"]
    pub fn DosCloseEventSem(hev: u32) -> APIRET;
    #[link_name = "DOSCALLS.327"]
    pub fn DosResetEventSem(hev: u32, count: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.328"]
    pub fn DosPostEventSem(hev: u32) -> APIRET;
    #[link_name = "DOSCALLS.329"]
    pub fn DosWaitEventSem(hev: u32, timeout: u32) -> APIRET;
    #[link_name = "DOSCALLS.331"]
    pub fn DosCreateMutexSem(name: *const u8, phmtx: *mut u32, attr: u32, owned: u32) -> APIRET;
    #[link_name = "DOSCALLS.333"]
    pub fn DosCloseMutexSem(hmtx: u32) -> APIRET;
    #[link_name = "DOSCALLS.334"]
    pub fn DosRequestMutexSem(hmtx: u32, timeout: u32) -> APIRET;
    #[link_name = "DOSCALLS.335"]
    pub fn DosReleaseMutexSem(hmtx: u32) -> APIRET;
    #[link_name = "DOSCALLS.337"]
    pub fn DosCreateMuxWaitSem(name: *const u8, phmux: *mut u32, n: u32, recs: *const SemRecord, attr: u32) -> APIRET;
    #[link_name = "DOSCALLS.340"]
    pub fn DosWaitMuxWaitSem(hmux: u32, timeout: u32, user: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.296"]
    pub fn DosExitList(code: u32, pfn: ExitListFn) -> APIRET;
    #[link_name = "DOSCALLS.354"]
    pub fn DosSetExceptionHandler(rec: *mut ExceptionRegistrationRecord) -> APIRET;
    #[link_name = "DOSCALLS.355"]
    pub fn DosUnsetExceptionHandler(rec: *mut ExceptionRegistrationRecord) -> APIRET;
    #[link_name = "DOSCALLS.356"]
    pub fn DosRaiseException(report: *mut ExceptionReportRecord) -> APIRET;
    #[link_name = "DOSCALLS.357"]
    pub fn DosUnwindException(target: *mut ExceptionRegistrationRecord, ip: *const u8, report: *mut ExceptionReportRecord) -> APIRET;
    #[link_name = "DOSCALLS.283"]
    pub fn DosExecPgm(fail: *mut u8, cbfail: u32, flags: u32, args: *const u8, env: *const u8, res: *mut ResultCodes, name: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.280"]
    pub fn DosWaitChild(action: u32, option: u32, res: *mut ResultCodes, ppid: *mut u32, pid: u32) -> APIRET;
    #[link_name = "DOSCALLS.227"]
    pub fn DosScanEnv(name: *const u8, value: *mut *const u8) -> APIRET;
    #[link_name = "DOSCALLS.239"]
    pub fn DosCreatePipe(hread: *mut u32, hwrite: *mut u32, size: u32) -> APIRET;
    #[link_name = "DOSCALLS.260"]
    pub fn DosDupHandle(old: u32, new: *mut u32) -> APIRET;
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.318"]
    pub fn DosLoadModule(fail: *mut u8, cbfail: u32, name: *const u8, phmod: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.321"]
    pub fn DosQueryProcAddr(hmod: u32, ordinal: u32, name: *const u8, ppfn: *mut usize) -> APIRET;
    #[link_name = "DOSCALLS.322"]
    pub fn DosFreeModule(hmod: u32) -> APIRET;
    #[link_name = "DOSCALLS.319"]
    pub fn DosQueryModuleHandle(name: *const u8, phmod: *mut u32) -> APIRET;
}

pub const OBJ_GIVEABLE: u32 = 0x0200;
pub const OBJ_GETTABLE: u32 = 0x0100;
pub const QUE_FIFO: u32 = 0;
pub const QUE_PRIORITY: u32 = 2;

#[repr(C)]
#[derive(Default)]
pub struct RequestData {
    pub pid: u32,
    pub data: u32,
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.350"]
    pub fn DosAsyncTimer(ms: u32, hsem: u32, ph: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.351"]
    pub fn DosStartTimer(ms: u32, hsem: u32, ph: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.290"]
    pub fn DosStopTimer(h: u32) -> APIRET;
    #[link_name = "DOSCALLS.362"]
    pub fn DosTmrQueryFreq(freq: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.363"]
    pub fn DosTmrQueryTime(t: *mut u64) -> APIRET;
    #[link_name = "DOSCALLS.300"]
    pub fn DosAllocSharedMem(ppb: *mut *mut u8, name: *const u8, cb: u32, flags: u32) -> APIRET;
    #[link_name = "DOSCALLS.301"]
    pub fn DosGetNamedSharedMem(ppb: *mut *mut u8, name: *const u8, flags: u32) -> APIRET;
    #[link_name = "QUECALLS.16"]
    pub fn DosCreateQueue(phq: *mut u32, order: u32, name: *const u8) -> APIRET;
    #[link_name = "QUECALLS.15"]
    pub fn DosOpenQueue(ppid: *mut u32, phq: *mut u32, name: *const u8) -> APIRET;
    #[link_name = "QUECALLS.14"]
    pub fn DosWriteQueue(hq: u32, req: u32, cb: u32, pb: *const u8, prio: u32) -> APIRET;
    #[link_name = "QUECALLS.9"]
    pub fn DosReadQueue(hq: u32, req: *mut RequestData, pcb: *mut u32, ppb: *mut *const u8, elem: u32, wait: u32, prio: *mut u8, hsem: u32) -> APIRET;
    #[link_name = "QUECALLS.12"]
    pub fn DosQueryQueue(hq: u32, count: *mut u32) -> APIRET;
    #[link_name = "QUECALLS.11"]
    pub fn DosCloseQueue(hq: u32) -> APIRET;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct FileStatus3 {
    pub fdate_creation: u16,
    pub ftime_creation: u16,
    pub fdate_access: u16,
    pub ftime_access: u16,
    pub fdate_write: u16,
    pub ftime_write: u16,
    pub cb_file: u32,
    pub cb_alloc: u32,
    pub attr: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct FsAllocate {
    pub id: u32,
    pub sectors_per_unit: u32,
    pub units: u32,
    pub units_avail: u32,
    pub bytes_per_sector: u16,
}

#[repr(C)]
pub struct FsInfo {
    pub serial: u32,
    pub cch_label: u8,
    pub label: [u8; 12],
}

pub const FIL_QUERYFULLNAME: u32 = 5;
pub const FSIL_ALLOC: u32 = 1;
pub const FSIL_VOLSER: u32 = 2;
pub const FILE_READONLY: u32 = 0x01;
pub const FILE_HIDDEN: u32 = 0x02;
pub const FILE_SYSTEM: u32 = 0x04;
pub const FILE_ARCHIVED: u32 = 0x20;
pub const EXEC_ASYNC: u32 = 1;
pub const OPEN_ACCESS_WRITEONLY: u32 = 0x0001;
pub const OPEN_FLAGS_NOINHERIT: u32 = 0x0080;

unsafe extern "C" {
    #[link_name = "DOSCALLS.275"]
    pub fn DosQueryCurrentDisk(pdisk: *mut u32, plogical: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.220"]
    pub fn DosSetDefaultDisk(disk: u32) -> APIRET;
    #[link_name = "DOSCALLS.274"]
    pub fn DosQueryCurrentDir(disk: u32, buf: *mut u8, pcb: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.255"]
    pub fn DosSetCurrentDir(dir: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.223"]
    pub fn DosQueryPathInfo(path: *const u8, level: u32, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.219"]
    pub fn DosSetPathInfo(path: *const u8, level: u32, buf: *const u8, cb: u32, opts: u32) -> APIRET;
    #[link_name = "DOSCALLS.279"]
    pub fn DosQueryFileInfo(hf: u32, level: u32, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.278"]
    pub fn DosQueryFSInfo(disk: u32, level: u32, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.271"]
    pub fn DosMove(from: *const u8, to: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.292"]
    pub fn DosSetDateTime(dt: *const DateTime) -> APIRET;
    #[link_name = "DOSCALLS.224"]
    pub fn DosQueryHType(hf: u32, ptype: *mut u32, pattr: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.272"]
    pub fn DosSetFileSize(hf: u32, size: u32) -> APIRET;
    #[link_name = "DOSCALLS.235"]
    pub fn DosKillProcess(action: u32, pid: u32) -> APIRET;
}
