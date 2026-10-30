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
    /// DosError(FERR_*): bit 0 enables hard-error pop-ups, bit 1 disables
    /// exception pop-ups.
    #[link_name = "DOSCALLS.212"]
    pub fn DosError(fl: u32) -> APIRET;
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
    #[link_name = "DOSCALLS.243"]
    pub fn DosCreateNPipe(name: *const u8, ph: *mut u32, open_mode: u32, pipe_mode: u32, cb_out: u32, cb_in: u32, ms: u32) -> APIRET;
    #[link_name = "DOSCALLS.241"]
    pub fn DosConnectNPipe(h: u32) -> APIRET;
    #[link_name = "DOSCALLS.242"]
    pub fn DosDisConnectNPipe(h: u32) -> APIRET;
    #[link_name = "DOSCALLS.244"]
    pub fn DosPeekNPipe(h: u32, buf: *mut u8, cb: u32, actual: *mut u32, avail: *mut u16, state: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.245"]
    pub fn DosQueryNPHState(h: u32, state: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.250"]
    pub fn DosSetNPHState(h: u32, state: u32) -> APIRET;
    #[link_name = "DOSCALLS.248"]
    pub fn DosQueryNPipeInfo(h: u32, level: u32, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.252"]
    pub fn DosTransactNPipe(h: u32, out: *const u8, cb_out: u32, inb: *mut u8, cb_in: u32, read: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.253"]
    pub fn DosWaitNPipe(name: *const u8, ms: u32) -> APIRET;
    #[link_name = "DOSCALLS.240"]
    pub fn DosCallNPipe(name: *const u8, inb: *const u8, cb_in: u32, out: *mut u8, cb_out: u32, actual: *mut u32, ms: u32) -> APIRET;
}

/// STARTDATA (DosStartSession), up to SessionType and the icon fields.
#[repr(C, packed)]
pub struct StartData {
    pub length: u16,
    pub related: u16,
    pub fgbg: u16,
    pub trace: u16,
    pub title: *const u8,
    pub name: *const u8,
    pub inputs: *const u8,
    pub termq: *const u8,
    pub env: *const u8,
    pub inherit: u16,
    pub session_type: u16,
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.323"]
    pub fn DosQueryAppType(name: *const u8, flags: *mut u32) -> APIRET;
    #[link_name = "SESMGR.37"]
    pub fn DosStartSession(sd: *const StartData, sid: *mut u32, pid: *mut u32) -> APIRET;
    #[link_name = "SESMGR.38"]
    pub fn DosSelectSession(sid: u32) -> APIRET;
    #[link_name = "SESMGR.40"]
    pub fn DosStopSession(scope: u32, sid: u32) -> APIRET;
}

pub const EXEC_BACKGROUND: u32 = 4;

pub const NP_ACCESS_DUPLEX: u32 = 2;
pub const NP_NOWAIT: u32 = 0x8000;
pub const NP_TYPE_MESSAGE: u32 = 0x0400;
pub const NP_READMODE_MESSAGE: u32 = 0x0100;

unsafe extern "C" {
    #[link_name = "DOSCALLS.318"]
    pub fn DosLoadModule(fail: *mut u8, cbfail: u32, name: *const u8, phmod: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.321"]
    pub fn DosQueryProcAddr(hmod: u32, ordinal: u32, name: *const u8, ppfn: *mut usize) -> APIRET;
    #[link_name = "DOSCALLS.322"]
    pub fn DosFreeModule(hmod: u32) -> APIRET;
    #[link_name = "DOSCALLS.319"]
    pub fn DosQueryModuleHandle(name: *const u8, phmod: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.320"]
    pub fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> APIRET;
    #[link_name = "DOSCALLS.317"]
    pub fn DosDebug(pdbgbuf: *mut UDb) -> APIRET;
}

/// DosDebug's buffer (Toolkit bsedos.h uDB_t).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UDb {
    pub pid: u32,
    pub tid: u32,
    pub cmd: i32,
    pub value: u32,
    pub addr: u32,
    pub buffer: u32,
    pub len: u32,
    pub index: u32,
    pub mte: u32,
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esp: u32,
    pub ebp: u32,
    pub esi: u32,
    pub edi: u32,
    pub eflags: u32,
    pub eip: u32,
    /// CS, DS, ES, FS, GS, SS: limit, base, access, attributes, selector.
    pub segs: [DbgSeg; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DbgSeg {
    pub lim: u32,
    pub base: u32,
    pub acc: u8,
    pub atr: u8,
    pub sel: u16,
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
    #[link_name = "DOSCALLS.364"]
    pub fn DosAliasPerfCtrs(flags: u32, handle: u32, ppb: *mut *mut u8, pcb: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.365"]
    pub fn DosConfigurePerf(flags: u32, cmd: u32, a: u32, b: u32, c: *const u8, d: u32) -> APIRET;
    #[link_name = "DOSCALLS.447"]
    pub fn DosGetProcessorStatus(id: u32, status: *mut u32) -> APIRET;
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

unsafe extern "C" {
    #[link_name = "DOSCALLS.221"]
    pub fn DosSetFHState(hf: u32, state: u32) -> APIRET;
    #[link_name = "DOSCALLS.276"]
    pub fn DosQueryFHState(hf: u32, state: *mut u32) -> APIRET;
}

pub const OPEN_FLAGS_DASD: u32 = 0x8000;
pub const OPEN_FLAGS_FAIL_ON_ERROR: u32 = 0x2000;
pub const OPEN_SHARE_DENYREADWRITE: u32 = 0x0010;
pub const IOCTL_DISK: u32 = 8;
pub const DSK_LOCKDRIVE: u32 = 0x00;
pub const DSK_UNLOCKDRIVE: u32 = 0x01;
pub const DSK_REDETERMINEMEDIA: u32 = 0x02;
pub const DSK_WRITETRACK: u32 = 0x44;
pub const DSK_GETDEVICEPARAMS: u32 = 0x63;
pub const DSK_READTRACK: u32 = 0x64;

/// BIOSPARAMETERBLOCK (DSK_GETDEVICEPARAMS).
#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
pub struct BiosParameterBlock {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fats: u8,
    pub root_entries: u16,
    pub sectors: u16,
    pub media: u8,
    pub sectors_per_fat: u16,
    pub sectors_per_track: u16,
    pub heads: u16,
    pub hidden_sectors: u32,
    pub large_sectors: u32,
    pub reserved: [u8; 6],
    pub cylinders: u16,
    pub device_type: u8,
    pub device_attr: u16,
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.284"]
    pub fn DosDevIOCtl(
        h: u32,
        category: u32,
        function: u32,
        params: *mut u8,
        cb_params: u32,
        pcb_params: *mut u32,
        data: *mut u8,
        cb_data: u32,
        pcb_data: *mut u32,
    ) -> APIRET;
    #[link_name = "DOSCALLS.222"]
    pub fn DosSetFSInfo(disk: u32, level: u32, buf: *const u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.277"]
    pub fn DosQueryFSAttach(dev: *const u8, ordinal: u32, level: u32, buf: *mut u8, pcb: *mut u32) -> APIRET;
}

// Memory suballocation.
pub const DOSSUB_INIT: u32 = 0x01;
pub const DOSSUB_GROW: u32 = 0x02;
pub const DOSSUB_SPARSE_OBJ: u32 = 0x04;
pub const DOSSUB_SERIALIZE: u32 = 0x08;

unsafe extern "C" {
    #[link_name = "DOSCALLS.344"]
    pub fn DosSubSetMem(base: *mut u8, flags: u32, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.345"]
    pub fn DosSubAllocMem(base: *mut u8, ppb: *mut *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.346"]
    pub fn DosSubFreeMem(base: *mut u8, pb: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.347"]
    pub fn DosSubUnsetMem(base: *mut u8) -> APIRET;
}

pub const DCPY_EXISTING: u32 = 1;
pub const DCPY_APPEND: u32 = 2;
pub const DCPY_FAILEAS: u32 = 4;

// Extended attributes (bsedos.h).
pub const FIL_QUERYEASIZE: u32 = 2;
pub const FIL_QUERYEASFROMLIST: u32 = 3;
pub const FIL_QUERYALLEAS: u32 = 4;
pub const ENUMEA_LEVEL_NO_VALUE: u32 = 1;
pub const ENUMEA_REFTYPE_FHANDLE: u32 = 0;
pub const ENUMEA_REFTYPE_PATH: u32 = 1;
pub const FEA_NEEDEA: u8 = 0x80;
pub const EAT_BINARY: u16 = 0xfffe;
pub const EAT_ASCII: u16 = 0xfffd;
pub const EAT_MVMT: u16 = 0xffdf;

/// EAOP2: the GEA2 list to query, the FEA2 list to fill or set, and the
/// offset of a bad entry.
#[repr(C)]
pub struct EaOp2 {
    pub gea2: *mut u8,
    pub fea2: *mut u8,
    pub error: u32,
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.218"]
    pub fn DosSetFileInfo(hf: u32, level: u32, buf: *const u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.372"]
    pub fn DosEnumAttribute(reftype: u32, file: *const u8, entry: u32, buf: *mut u8, cb: u32, count: *mut u32, level: u32) -> APIRET;
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.258"]
    pub fn DosCopy(source: *const u8, target: *const u8, options: u32) -> APIRET;
    #[link_name = "DOSCALLS.110"]
    pub fn DosForceDelete(name: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.261"]
    pub fn DosEditName(level: u32, source: *const u8, edit: *const u8, target: *mut u8, cb: u32) -> APIRET;
}
