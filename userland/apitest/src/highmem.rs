//! High memory (above 512 MB): DosAllocMem / DosAllocSharedMem with
//! OBJ_ANY, the DosQuerySysInfo values, DosFlatToSel, a DLL whose objects
//! are marked for high memory (TESTDLLH, OBJHIGHMEM). With
//! VIRTUALADDRESSLIMIT=512 (no high memory) OBJ_ANY gives low memory.

use core::ptr::{null, null_mut};

use os2::*;

use crate::check;

const HIGH: u32 = 0x2000_0000;
const QSV_MAXPRMEM: u32 = 20;
const QSV_VIRTUALADDRESSLIMIT: u32 = 30;
const MAGIC: u32 = 0x4849_4748;
const NAME: &[u8] = b"\\SHAREMEM\\APIHIGH\0";

unsafe extern "C" {
    #[link_name = "DOSCALLS.425"]
    fn DosFlatToSel();
}

/// DosFlatToSel: register-based (EAX in, EAX out).
fn flat_to_sel(a: u32) -> u32 {
    let r: u32;
    unsafe {
        core::arch::asm!("call {f}", f = in(reg) DosFlatToSel as *const () as usize, inout("eax") a => r, clobber_abi("C"));
    }
    r
}

/// QSV_MAXPRMEM .. QSV_VIRTUALADDRESSLIMIT.
fn sys_info() -> [u32; 11] {
    let mut v = [0u32; 11];
    unsafe { DosQuerySysInfo(QSV_MAXPRMEM, QSV_VIRTUALADDRESSLIMIT, v.as_mut_ptr(), 44) };
    v
}

fn hex(mut v: u32, out: &mut [u8]) {
    for i in (0..8).rev() {
        out[i] = b"0123456789ABCDEF"[(v & 15) as usize];
        v >>= 4;
    }
}

fn unhex(s: &[u8]) -> u32 {
    s.iter().take(8).fold(0, |a, &c| a << 4 | (c as char).to_digit(16).unwrap_or(0))
}

pub fn run() {
    let v = sys_info();
    let (prmem, shmem, hprmem, hshmem, limit_mb) = (v[0], v[1], v[7], v[8], v[10]);
    check("QSV_VIRTUALADDRESSLIMIT from 512, in 64 MB steps", limit_mb >= 512 && limit_mb % 64 == 0);
    check("QSV_MAXPRMEM / QSV_MAXSHMEM", prmem > 0x0100_0000 && shmem > 0x0100_0000 && prmem % 0x10000 == 0);
    let high = limit_mb > 512;
    let limit = limit_mb << 20;
    if high {
        check("QSV_MAXHPRMEM / QSV_MAXHSHMEM with high memory", hprmem >= 0x0400_0000 && hshmem >= 0x0400_0000 && hprmem <= limit - HIGH && hshmem <= limit - HIGH);
    } else {
        check("QSV_MAXHPRMEM / QSV_MAXHSHMEM 0 without high memory", hprmem == 0 && hshmem == 0);
    }
    let in_arena = |p: u32| if high { (HIGH..limit).contains(&p) } else { p < HIGH };
    unsafe {
        // A private object: high when there is high memory, else low.
        let mut p = null_mut();
        let rc = DosAllocMem(&mut p, 0x10_0000, PAG_READ | PAG_WRITE | OBJ_ANY);
        let a = p as u32;
        check("DosAllocMem OBJ_ANY in the high arena (or low without one)", rc == 0 && in_arena(a) && a % 0x10000 == 0);
        if rc != 0 {
            return;
        }
        check("DosAllocMem OBJ_ANY | OBJ_TILE is invalid", DosAllocMem(&mut null_mut(), 0x1000, PAG_READ | OBJ_ANY | OBJ_TILE) == 87);
        if high {
            let after = sys_info()[7];
            check("QSV_MAXHPRMEM less by the object", after + 0x10_0000 <= hprmem);
        }
        let (mut cb, mut fl) = (0x10_0000u32, 0u32);
        let rc = DosQueryMem(p, &mut cb, &mut fl);
        check("DosQueryMem high object reserved", rc == 0 && fl & PAG_COMMIT == 0 && fl & PAG_BASE != 0 && cb == 0x10_0000);
        let q = p.add(0x8_0000);
        let rc = DosSetMem(q, 0x2000, PAG_COMMIT | PAG_READ | PAG_WRITE);
        (q as *mut u32).write_volatile(MAGIC);
        (q.add(0x1ffc) as *mut u32).write_volatile(!MAGIC);
        check("DosSetMem commit + touch high pages", rc == 0 && (q as *const u32).read_volatile() == MAGIC);
        let (mut cb, mut fl) = (0x10_0000u32, 0u32);
        DosQueryMem(q, &mut cb, &mut fl);
        check("DosQueryMem committed high run", fl & PAG_COMMIT != 0 && fl & PAG_WRITE != 0 && cb == 0x2000);
        let rc = DosSetMem(q, 0x2000, PAG_READ);
        let (mut cb, mut fl) = (0x2000u32, 0u32);
        DosQueryMem(q, &mut cb, &mut fl);
        check("DosSetMem read-only on high pages", rc == 0 && fl & PAG_WRITE == 0 && fl & PAG_READ != 0);
        let rc = DosSetMem(q, 0x2000, PAG_DECOMMIT);
        let (mut cb, mut fl) = (0x2000u32, 0u32);
        DosQueryMem(q, &mut cb, &mut fl);
        check("DosSetMem decommit high pages", rc == 0 && fl & PAG_COMMIT == 0);
        // 16:16 pointers cannot reach high memory.
        if high {
            check("DosFlatToSel of a high address is 0", flat_to_sel(a) == 0);
        }
        let mut low = null_mut();
        DosAllocMem(&mut low, 0x1000, PAG_READ | PAG_WRITE | PAG_COMMIT);
        let l = low as u32;
        check("DosFlatToSel of a low address is its tile", flat_to_sel(l) == ((l >> 13) | 7) << 16 | (l & 0xffff));
        DosFreeMem(low);
        check("DosFreeMem high object", DosFreeMem(p) == 0);
        let (mut cb, mut fl) = (0x1000u32, 0u32);
        DosQueryMem(p, &mut cb, &mut fl);
        check("DosQueryMem freed high memory is free", fl & PAG_FREE != 0);

        // More than the low arena holds, when the high one is big enough.
        if high && hprmem >= 0x2000_0000 {
            let mut big = null_mut();
            let rc = DosAllocMem(&mut big, 0x1e00_0000, PAG_READ | PAG_WRITE | OBJ_ANY);
            check("DosAllocMem OBJ_ANY 480 MB", rc == 0 && big as u32 >= HIGH);
            if rc == 0 {
                DosFreeMem(big);
            }
        }

        // Shared memory: the same high address in another process.
        let mut s = null_mut();
        let rc = DosAllocSharedMem(&mut s, NAME.as_ptr(), 0x1_0000, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_ANY);
        check("DosAllocSharedMem OBJ_ANY in the high arena (or low)", rc == 0 && in_arena(s as u32));
        let mut g = null_mut();
        let rc2 = DosAllocSharedMem(&mut g, null(), 0x2000, PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GETTABLE | OBJ_ANY);
        check("DosAllocSharedMem gettable OBJ_ANY", rc2 == 0 && in_arena(g as u32));
        if rc == 0 && rc2 == 0 {
            (s as *mut u32).write_volatile(MAGIC);
            (g as *mut u32).write_volatile(MAGIC + 1);
            let (mut cb, mut fl) = (0x1_0000u32, 0u32);
            DosQueryMem(s, &mut cb, &mut fl);
            check("DosQueryMem high shared object", fl & PAG_SHARED != 0 && fl & PAG_COMMIT != 0 && cb == 0x1_0000);
            let code = run_child(s as u32, g as u32);
            check("another process sees the high shared objects at the same addresses", code == 0);
            check("and its writes", (s as *const u32).read_volatile() == !MAGIC && (g as *const u32).read_volatile() == !(MAGIC + 1));
            DosFreeMem(g);
            check("DosFreeMem high shared", DosFreeMem(s) == 0);
        }

        // A DLL with objects marked OBJHIGHMEM.
        let mut h = 0;
        let rc = DosLoadModule(null_mut(), 0, b"TESTDLLH\0".as_ptr(), &mut h);
        let (mut pa, mut pc, mut pg) = (0usize, 0usize, 0usize);
        let ok = rc == 0
            && DosQueryProcAddr(h, 1, null(), &mut pa) == 0
            && DosQueryProcAddr(h, 2, null(), &mut pc) == 0
            && DosQueryProcAddr(h, 3, null(), &mut pg) == 0;
        check("DosLoadModule a DLL marked for high memory", ok);
        if ok {
            let add: extern "C" fn(u32, u32) -> u32 = core::mem::transmute(pa);
            let count: extern "C" fn() -> u32 = core::mem::transmute(pc);
            let greet: extern "C" fn() -> *const u8 = core::mem::transmute(pg);
            check("its code and data in the high arena (or low)", in_arena(pa as u32) && in_arena(greet() as u32));
            check("and it runs", add(40, 2) == 42 && count() == 1 && *greet() == b'h');
            DosFreeModule(h);
        }
    }
}

/// `APITEST highchild SSSSSSSS GGGGGGGG`: the parent's named and gettable
/// high shared objects. Exit code 0 when both are found at those addresses.
pub fn child(args: &[u8]) -> u32 {
    let mut it = args.split(|&c| c == b' ').filter(|w| !w.is_empty()).skip(1);
    let s = unhex(it.next().unwrap_or(b""));
    let g = unhex(it.next().unwrap_or(b""));
    unsafe {
        let mut p = null_mut();
        if DosGetNamedSharedMem(&mut p, NAME.as_ptr(), PAG_READ | PAG_WRITE) != 0 || p as u32 != s {
            return 1;
        }
        if (p as *const u32).read_volatile() != MAGIC {
            return 2;
        }
        if DosGetSharedMem(g as *mut u8, PAG_READ | PAG_WRITE) != 0 || (g as *const u32).read_volatile() != MAGIC + 1 {
            return 3;
        }
        (p as *mut u32).write_volatile(!MAGIC);
        (g as *mut u32).write_volatile(!(MAGIC + 1));
    }
    0
}

fn run_child(s: u32, g: u32) -> u32 {
    let mut pib = null_mut();
    let mut tib = null_mut();
    let mut path = [0u8; 260];
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        DosQueryModuleName((*pib).hmte, 260, path.as_mut_ptr());
    }
    let mut args = [0u8; 300];
    let n = path.iter().position(|&c| c == 0).unwrap_or(0);
    args[..n].copy_from_slice(&path[..n]);
    let tail = b"highchild SSSSSSSS GGGGGGGG";
    args[n + 1..n + 1 + tail.len()].copy_from_slice(tail);
    hex(s, &mut args[n + 11..n + 19]);
    hex(g, &mut args[n + 20..n + 28]);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), 64, EXEC_SYNC, args.as_ptr(), null(), &mut res, path.as_ptr()) };
    if rc != 0 { 100 + rc } else { res.code_result }
}
