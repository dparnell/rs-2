//! The OS/2 and rs2 API entries rs2libc is built on. `MODULE.ordinal`
//! link names become LX import fixups (tools/elf2lx).

#![allow(non_snake_case, dead_code)]

pub type APIRET = u32;

pub const PAG_READ: u32 = 0x0001;
pub const PAG_WRITE: u32 = 0x0002;
pub const PAG_EXECUTE: u32 = 0x0004;
pub const PAG_COMMIT: u32 = 0x0010;
pub const PAG_DECOMMIT: u32 = 0x0020;
/// Anywhere, also above 512 MB ("high memory", OS/2 4.5 kernels and rs2).
pub const OBJ_ANY: u32 = 0x0400;

/// DosAllocMem of committed read/write memory, above 512 MB when the
/// system has room there; kernels without high memory (before OS/2 4.5)
/// refuse OBJ_ANY with ERROR_INVALID_PARAMETER: then below.
pub unsafe fn alloc_any(ppb: *mut *mut u8, cb: u32) -> APIRET {
    let rc = unsafe { DosAllocMem(ppb, cb, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_ANY) };
    if rc != 87 {
        return rc;
    }
    unsafe { DosAllocMem(ppb, cb, PAG_READ | PAG_WRITE | PAG_COMMIT) }
}
pub const OBJ_TILE: u32 = 0x0040;

pub const SEM_INDEFINITE_WAIT: u32 = 0xffff_ffff;
pub const ERROR_TIMEOUT: u32 = 640;
pub const ERROR_INTERRUPT: u32 = 95;

#[repr(C)]
pub struct Tib {
    pub pexchain: u32,
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
#[derive(Default, Clone, Copy)]
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

/// FILESTATUS3 (level 1 of DosQueryPathInfo / DosQueryFileInfo).
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct FileStatus3 {
    pub cdate: u16,
    pub ctime: u16,
    pub adate: u16,
    pub atime: u16,
    pub wdate: u16,
    pub wtime: u16,
    pub cb_file: u32,
    pub cb_alloc: u32,
    pub attr: u32,
}

/// FILEFINDBUF3 (level 1 of DosFindFirst).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileFindBuf3 {
    pub next: u32,
    pub st: FileStatus3,
    pub cch_name: u8,
    pub name: [u8; 257],
}

/// `struct sockaddr_in` as rs2's socket interface takes it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SockAddrIn {
    pub family: u16,
    pub port: u16,
    pub addr: [u8; 4],
    pub zero: [u8; 8],
}

pub type ThreadFn = unsafe extern "C" fn(u32);

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
        size: u32,
        attr: u32,
        open_flags: u32,
        open_mode: u32,
        eaop: *mut u8,
    ) -> APIRET;
    #[link_name = "DOSCALLS.257"]
    pub fn DosClose(hf: u32) -> APIRET;
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
    #[link_name = "DOSCALLS.334"]
    pub fn DosRequestMutexSem(hmtx: u32, timeout: u32) -> APIRET;
    #[link_name = "DOSCALLS.335"]
    pub fn DosReleaseMutexSem(hmtx: u32) -> APIRET;
    #[link_name = "DOSCALLS.227"]
    pub fn DosScanEnv(name: *const u8, value: *mut *const u8) -> APIRET;
    #[link_name = "DOSCALLS.239"]
    pub fn DosCreatePipe(hread: *mut u32, hwrite: *mut u32, size: u32) -> APIRET;
    #[link_name = "DOSCALLS.260"]
    pub fn DosDupHandle(old: u32, new: *mut u32) -> APIRET;
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
    #[link_name = "DOSCALLS.279"]
    pub fn DosQueryFileInfo(hf: u32, level: u32, buf: *mut u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.218"]
    pub fn DosSetFileInfo(hf: u32, level: u32, buf: *const u8, cb: u32) -> APIRET;
    #[link_name = "DOSCALLS.219"]
    pub fn DosSetPathInfo(path: *const u8, level: u32, buf: *const u8, cb: u32, opts: u32) -> APIRET;
    #[link_name = "DOSCALLS.271"]
    pub fn DosMove(from: *const u8, to: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.224"]
    pub fn DosQueryHType(hf: u32, ptype: *mut u32, pattr: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.272"]
    pub fn DosSetFileSize(hf: u32, size: u32) -> APIRET;
    #[link_name = "DOSCALLS.221"]
    pub fn DosSetFHState(hf: u32, state: u32) -> APIRET;
    #[link_name = "DOSCALLS.276"]
    pub fn DosQueryFHState(hf: u32, state: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.320"]
    pub fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> APIRET;
    #[link_name = "DOSCALLS.362"]
    pub fn DosTmrQueryFreq(freq: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.363"]
    pub fn DosTmrQueryTime(t: *mut u64) -> APIRET;
    #[link_name = "DOSCALLS.235"]
    pub fn DosKillProcess(action: u32, pid: u32) -> APIRET;
    #[link_name = "DOSCALLS.283"]
    pub fn DosExecPgm(fail: *mut u8, cbfail: u32, flags: u32, args: *const u8, env: *const u8, res: *mut [u32; 2], name: *const u8) -> APIRET;
    #[link_name = "DOSCALLS.280"]
    pub fn DosWaitChild(action: u32, option: u32, res: *mut [u32; 2], ppid: *mut u32, pid: u32) -> APIRET;
    #[link_name = "DOSCALLS.232"]
    pub fn DosEnterCritSec() -> APIRET;
    #[link_name = "DOSCALLS.233"]
    pub fn DosExitCritSec() -> APIRET;

    /// RS2API 421: LDT alias selectors (kernel/src/api/win16.rs): op 0 a
    /// new data selector (sel 0), 2 set its base, 3 its limit, 4 free it.
    #[link_name = "RS2API.421"]
    pub fn Rs2Win16Selector(op: u32, sel: u32, value: u32) -> u32;

    #[link_name = "RS2API.201"]
    pub fn Rs2SockCreate(domain: u32, typ: u32, proto: u32, ps: *mut u32) -> u32;
    #[link_name = "RS2API.202"]
    pub fn Rs2SockBind(s: u32, addr: *const SockAddrIn, cb: u32) -> u32;
    #[link_name = "RS2API.203"]
    pub fn Rs2SockListen(s: u32, backlog: u32) -> u32;
    #[link_name = "RS2API.204"]
    pub fn Rs2SockAccept(s: u32, peer: *mut SockAddrIn, ps: *mut u32) -> u32;
    #[link_name = "RS2API.205"]
    pub fn Rs2SockConnect(s: u32, addr: *const SockAddrIn, cb: u32) -> u32;
    #[link_name = "RS2API.206"]
    pub fn Rs2SockSend(s: u32, buf: *const u8, cb: u32, flags: u32, sent: *mut u32) -> u32;
    #[link_name = "RS2API.207"]
    pub fn Rs2SockRecv(s: u32, buf: *mut u8, cb: u32, flags: u32, got: *mut u32) -> u32;
    #[link_name = "RS2API.208"]
    pub fn Rs2SockSendTo(s: u32, buf: *const u8, cb: u32, flags: u32, to: *const SockAddrIn, sent: *mut u32) -> u32;
    #[link_name = "RS2API.209"]
    pub fn Rs2SockRecvFrom(s: u32, buf: *mut u8, cb: u32, flags: u32, from: *mut SockAddrIn, got: *mut u32) -> u32;
    #[link_name = "RS2API.210"]
    pub fn Rs2SockShutdown(s: u32, how: u32) -> u32;
    #[link_name = "RS2API.211"]
    pub fn Rs2SockClose(s: u32) -> u32;
    #[link_name = "RS2API.212"]
    pub fn Rs2SockSelect(socks: *mut u32, nread: u32, nwrite: u32, nexcept: u32, ms: i32, nready: *mut u32) -> u32;
    #[link_name = "RS2API.213"]
    pub fn Rs2SockIoctl(s: u32, cmd: u32, arg: *mut u32) -> u32;
    #[link_name = "RS2API.214"]
    pub fn Rs2SockSetOpt(s: u32, level: u32, opt: u32, val: *const u8, cb: u32) -> u32;
    #[link_name = "RS2API.215"]
    pub fn Rs2SockGetOpt(s: u32, level: u32, opt: u32, val: *mut u8, cb: *mut u32) -> u32;
    #[link_name = "RS2API.216"]
    pub fn Rs2SockGetName(s: u32, peer: u32, addr: *mut SockAddrIn) -> u32;
    #[link_name = "RS2API.217"]
    pub fn Rs2NetResolve(name: *const u8, addrs: *mut [u8; 4], max: u32, count: *mut u32) -> u32;
}

/// The current thread's TIB and the process's PIB.
pub fn info_blocks() -> (&'static Tib, &'static Pib) {
    let mut t = core::ptr::null_mut();
    let mut p = core::ptr::null_mut();
    unsafe {
        DosGetInfoBlocks(&mut t, &mut p);
        (&*t, &*p)
    }
}
