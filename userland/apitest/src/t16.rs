//! 16-bit threads: DosCreateThread (DOSCALLS.145) from 16-bit code starts
//! the thread with the creator's registers but CS:IP and SS:SP (Borland's
//! and Microsoft C's _beginthread pass the thread function in CX:DX and
//! its argument in SI:DI), and the local information segment's
//! tidCurrent is the running thread's (Microsoft C's multithread _chkstk
//! looks the thread's stack limit up by it).

use core::sync::atomic::{AtomicU32, Ordering};

use thunk16::{far, flat, PascalCall};

use crate::check;

unsafe extern "C" {
    #[link_name = "DOSCALLS.8"]
    fn D16GetInfoSeg();
    #[link_name = "DOSCALLS.145"]
    fn D16CreateThread();
    #[link_name = "DOSCALLS.34"]
    fn D16AllocSeg();
    #[link_name = "DOSCALLS.39"]
    fn D16FreeSeg();
    #[link_name = "DOSCALLS.146"]
    fn D16SubSet();
    #[link_name = "DOSCALLS.147"]
    fn D16SubAlloc();
    #[link_name = "DOSCALLS.148"]
    fn D16SubFree();
    #[link_name = "DOSCALLS.184"]
    fn D16FindFirst2();
    #[link_name = "DOSCALLS.63"]
    fn D16FindClose();
    // The 16-bit DosQueryABIOSSupport: a gate of IBM's kernel itself
    // (SYSINST2.EXE).
    #[link_name = "DOSCALLS.443"]
    fn D16QueryABIOSSupport();
    #[link_name = "DOSCALLS.371"]
    fn DosQueryABIOSSupport(reserved: u32) -> u32;
    // OS/2 1.x named pipes: NAMPIPES (a forwarder module, as Warp's) to
    // the kernel's 16-bit DOSCALLS 399-412 (Open Watcom's NMPBIND.EXE).
    #[link_name = "NAMPIPES.1"]
    fn N16MakeNmPipe();
    #[link_name = "NAMPIPES.2"]
    fn N16QNmPipeInfo();
    #[link_name = "NAMPIPES.3"]
    fn N16ConnectNmPipe();
    #[link_name = "NAMPIPES.4"]
    fn N16DisConnectNmPipe();
    #[link_name = "NAMPIPES.5"]
    fn N16QNmPHandState();
    #[link_name = "NAMPIPES.7"]
    fn N16PeekNmPipe();
    static at16_create: u8;
    static at16_thread: u8;
}

// at16_create(pfnCreateThread, pfnThread, pbStack, ptid, pbuf) (_pascal
// far; after `push bp`: pbuf at bp+6, ptid bp+10, pbStack bp+14,
// pfnThread bp+18, pfnCreateThread bp+22): DosCreateThread(pfnThread,
// ptid, pbStack) with ES:BX = pbuf, CX 1111h, DX 2222h, SI 3333h, DI
// 4444h. at16_thread stores CX, DX, SI, DI at ES:BX, the tidCurrent of
// the local information segment whose selector is at ES:[BX+12] at +8,
// and 1 at +10, then returns (to the thread exit the system put on its
// stack).
core::arch::global_asm!(
    ".pushsection .text.at16,\"ax\"",
    ".p2align 4",
    ".code16",
    ".globl at16_create",
    "at16_create:",
    "push bp",
    "mov bp, sp",
    "push si",
    "push di",
    "push bx",
    "push es",
    "push word ptr [bp + 20]",
    "push word ptr [bp + 18]",
    "push word ptr [bp + 12]",
    "push word ptr [bp + 10]",
    "push word ptr [bp + 16]",
    "push word ptr [bp + 14]",
    "les bx, dword ptr [bp + 6]",
    "mov cx, 0x1111",
    "mov dx, 0x2222",
    "mov si, 0x3333",
    "mov di, 0x4444",
    // call far [bp + 22]
    ".byte 0xff, 0x5e, 22",
    "pop es",
    "pop bx",
    "pop di",
    "pop si",
    "pop bp",
    "xor dx, dx",
    ".byte 0xca, 20, 0",
    ".globl at16_thread",
    "at16_thread:",
    "mov word ptr es:[bx], cx",
    "mov word ptr es:[bx + 2], dx",
    "mov word ptr es:[bx + 4], si",
    "mov word ptr es:[bx + 6], di",
    "push ds",
    "mov ds, word ptr es:[bx + 12]",
    "mov ax, word ptr ds:[6]",
    "pop ds",
    "mov word ptr es:[bx + 8], ax",
    "mov word ptr es:[bx + 10], 1",
    ".byte 0xcb",
    ".code32",
    ".popsection",
);

/// 16-bit suballocation (DosSubSet, DosSubAlloc, DosSubFree): IBM's
/// 16-bit layout, an 8-byte header, 4-byte granularity, areas from 12
/// bytes (IPFC sets up 22-byte ones).
fn suballoc() {
    let gate = |f: unsafe extern "C" fn()| far(f as *const u8);
    let mut sel = 0u16;
    // DosAllocSeg(usSize, pSel, fAlloc)
    let r = PascalCall::new().w(0x100).l(far(&raw mut sel as *const u8)).w(0).call(gate(D16AllocSeg)) & 0xffff;
    check("16-bit DosAllocSeg", r == 0 && sel != 0);
    let seg = flat((sel as u32) << 16) as *const u16;
    let set = |flags: u16, size: u16| PascalCall::new().w(sel).w(flags).w(size).call(gate(D16SubSet)) & 0xffff;
    let alloc = |size: u16| {
        let mut off = 0u16;
        let r = PascalCall::new().w(sel).l(far(&raw mut off as *const u8)).w(size).call(gate(D16SubAlloc)) & 0xffff;
        (r, off)
    };
    let free = |off: u16, size: u16| PascalCall::new().w(sel).w(off).w(size).call(gate(D16SubFree)) & 0xffff;
    check("DosSubSet below 12 bytes: ERROR_DOSSUB_BADSIZE", set(1, 8) == 313);
    check("DosSubSet of 22 bytes", set(1, 0x16) == 0);
    let w = |i: usize| unsafe { seg.add(i).read_volatile() };
    check("its header: first free block at 8, 16 bytes", w(0) == 8 && w(1) == 16 && w(4) == 0 && w(5) == 16);
    let (r1, a) = alloc(5);
    let (r2, b) = alloc(4);
    check("DosSubAlloc rounds to 4 bytes, first fit", r1 == 0 && a == 8 && r2 == 0 && b == 16);
    let (r3, big) = alloc(12);
    check("too large: ERROR_DOSSUB_NOMEM with the largest free size", r3 == 311 && big == 4);
    check("DosSubFree of a block not given out: ERROR_DOSSUB_OVERLAP", free(22, 4) == 312);
    check("DosSubFree merges the blocks back", free(a, 5) == 0 && free(b, 4) == 0 && w(0) == 8 && w(4) == 0 && w(5) == 16);
    check("DosSubSet grows the area", set(0, 0x40) == 0 && w(1) == 56 && w(5) == 56);
    check("but does not shrink it", set(0, 0x20) == 310);
    PascalCall::new().w(sel).call(gate(D16FreeSeg));
}

/// 16-bit DosFindFirst2 at level 12 (as IBM's 16-bit CMD calls it): the
/// 32-bit FILEFINDBUF4L layout, but the entries packed, each right after
/// the previous name's zero (CMD steps through them that way and deleted
/// "C:\" when they were padded to 4 bytes).
fn find16() {
    let gate = |f: unsafe extern "C" fn()| far(f as *const u8);
    let names: [&[u8]; 2] = [b"C:\\F16A.TMP\0", b"C:\\F16BB.TMP\0"];
    for n in names {
        let (mut hf, mut action) = (0, 0);
        unsafe {
            os2::DosOpen(n.as_ptr(), &mut hf, &mut action, 0, os2::FILE_NORMAL, os2::OPEN_ACTION_CREATE_IF_NEW | os2::OPEN_ACTION_REPLACE_IF_EXISTS, os2::OPEN_ACCESS_READWRITE | os2::OPEN_SHARE_DENYWRITE, core::ptr::null_mut());
            os2::DosClose(hf);
        }
    }
    let spec = b"C:\\F16*.TMP\0";
    let mut buf = [0u8; 600];
    let (mut hdir, mut count) = (0xffffu16, 2u16);
    // DosFindFirst2(pszSpec, phdir, usAttr, pBuf, cbBuf, pcSearch, usInfoLevel, ulReserved)
    let r = PascalCall::new()
        .l(far(spec.as_ptr()))
        .l(far(&raw mut hdir as *const u8))
        .w(0)
        .l(far(buf.as_mut_ptr()))
        .w(buf.len() as u16)
        .l(far(&raw mut count as *const u8))
        .w(12)
        .l(0)
        .call(gate(D16FindFirst2))
        & 0xffff;
    // 40 bytes before cchName: oNextEntryOffset, the dates, 64-bit sizes,
    // the attribute and cbList.
    let name = |at: usize| &buf[at + 41..at + 41 + buf[at + 40] as usize];
    let first = name(0);
    let next = 40 + 1 + first.len() + 1;
    check("16-bit DosFindFirst2 level 12: two entries", r == 0 && count == 2);
    check("the second right after the first name's zero", next < buf.len() - 48 && [first, name(next)].iter().all(|n| *n == b"F16A.TMP" || *n == b"F16BB.TMP") && first != name(next));
    PascalCall::new().w(hdir).call(gate(D16FindClose));
    for n in names {
        unsafe { os2::DosDelete(n.as_ptr()) };
    }
}

/// The 16-bit named pipe calls through NAMPIPES.DLL.
fn nampipes() {
    let gate = |f: unsafe extern "C" fn()| far(f as *const u8);
    let name = b"\\PIPE\\T16NMP\0";
    let mut hp = 0u16;
    // DosMakeNmPipe(pszName, php, fsOpenMode, fsPipeMode, cbOut, cbIn, ulTimeOut):
    // duplex; no wait, message pipe read as messages, one instance.
    let r = PascalCall::new()
        .l(far(name.as_ptr()))
        .l(far(&raw mut hp as *const u8))
        .w(2)
        .w(0x8000 | 0x0400 | 0x0100 | 1)
        .w(512)
        .w(512)
        .l(0)
        .call(gate(N16MakeNmPipe))
        & 0xffff;
    check("NAMPIPES DosMakeNmPipe", r == 0 && hp != 0);
    if r != 0 {
        return;
    }
    let connect = || PascalCall::new().w(hp).call(gate(N16ConnectNmPipe)) & 0xffff;
    // No client yet: ERROR_PIPE_NOT_CONNECTED (no-wait pipe).
    check("DosConnectNmPipe without a client: ERROR_PIPE_NOT_CONNECTED", connect() == 233);
    let (mut hc, mut action) = (0, 0);
    let ro = unsafe { os2::DosOpen(name.as_ptr(), &mut hc, &mut action, 0, 0, os2::OPEN_ACTION_OPEN_IF_EXISTS, os2::OPEN_ACCESS_READWRITE | os2::OPEN_SHARE_DENYNONE, core::ptr::null_mut()) };
    check("a client opens it (DosOpen)", ro == 0);
    check("DosConnectNmPipe with the client", connect() == 0);
    let mut state = 0u16;
    let r = PascalCall::new().w(hp).l(far(&raw mut state as *const u8)).call(gate(N16QNmPHandState)) & 0xffff;
    check("DosQNmPHandState: a server end of a message pipe read as messages", r == 0 && state & 0x4000 != 0 && state & 0x0400 != 0 && state & 0x0100 != 0 && state & 0xff == 1);
    let mut n = 0;
    unsafe { os2::DosWrite(hc, b"ping".as_ptr(), 4, &mut n) };
    let mut buf = [0u8; 16];
    let (mut cb, mut avail, mut pst) = (0u16, [0u16; 2], 0u16);
    let r = PascalCall::new()
        .w(hp)
        .l(far(buf.as_mut_ptr()))
        .w(16)
        .l(far(&raw mut cb as *const u8))
        .l(far(avail.as_mut_ptr() as *const u8))
        .l(far(&raw mut pst as *const u8))
        .call(gate(N16PeekNmPipe))
        & 0xffff;
    check("DosPeekNmPipe: the client's message, USHORT count and state", r == 0 && cb == 4 && &buf[..4] == b"ping" && avail == [4, 4] && pst == 3);
    let mut info = [0u8; 64];
    let r = PascalCall::new().w(hp).w(1).l(far(info.as_mut_ptr())).w(64).call(gate(N16QNmPipeInfo)) & 0xffff;
    check("DosQNmPipeInfo level 1: sizes and name", r == 0 && info[0..2] == [0, 2] && info[2..4] == [0, 2] && &info[7..7 + info[6] as usize] == b"\\PIPE\\T16NMP");
    check("DosDisConnectNmPipe", PascalCall::new().w(hp).call(gate(N16DisConnectNmPipe)) & 0xffff == 0);
    unsafe {
        os2::DosClose(hc);
        os2::DosClose(hp as u32);
    }
}

pub fn run() {
    suballoc();
    find16();
    nampipes();
    let gate = |f: unsafe extern "C" fn()| far(f as *const u8);
    let flags = PascalCall::new().l(0).call(gate(D16QueryABIOSSupport)) & 0xffff;
    check("DOSCALLS.443 (16-bit DosQueryABIOSSupport) answers as the 32-bit one", flags == unsafe { DosQueryABIOSSupport(0) } && flags & 0x10 != 0);
    let (mut gsel, mut lsel) = (0u16, 0u16);
    PascalCall::new().l(far(&raw mut gsel as *const u8)).l(far(&raw mut lsel as *const u8)).call(gate(D16GetInfoSeg));
    let lis = flat((lsel as u32) << 16) as *const u16;
    // (A thread of this process still ending on another CPU can hold
    // tidCurrent for a moment: look again after a yield.)
    let main_tid = !lis.is_null()
        && (0..20).any(|_| {
            let t = unsafe { lis.add(3).read_volatile() };
            if t != 1 {
                unsafe { os2::DosSleep(1) };
            }
            t == 1
        });
    check("16-bit DosGetInfoSeg: tidCurrent of the main thread is 1", main_tid);
    // A 16-bit stack (in a tiled object) and the result buffer.
    let mut stack: *mut u8 = core::ptr::null_mut();
    const OBJ_TILE: u32 = 0x40;
    if unsafe { os2::DosAllocMem(&mut stack, 0x10000, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | OBJ_TILE) } != 0 {
        check("16-bit thread stack", false);
        return;
    }
    let buf = unsafe { stack.add(0x100) } as *mut u16;
    unsafe { buf.add(6).write_volatile(lsel) };
    let mut tid = 0u16;
    let r = PascalCall::new()
        .l(gate(D16CreateThread))
        .l(far(&raw const at16_thread))
        .l(far(unsafe { stack.add(0xfff0) }))
        .l(far(&raw mut tid as *const u8))
        .l(far(buf as *const u8))
        .call(far(&raw const at16_create));
    check("16-bit DosCreateThread", r & 0xffff == 0 && tid > 1);
    let mut done = false;
    for _ in 0..200 {
        if unsafe { buf.add(5).read_volatile() } == 1 {
            done = true;
            break;
        }
        unsafe { os2::DosSleep(10) };
    }
    let rd = |i: usize| unsafe { buf.add(i).read_volatile() };
    check("the 16-bit thread ran", done);
    check("it started with the creator's CX, DX, SI, DI", rd(0) == 0x1111 && rd(1) == 0x2222 && rd(2) == 0x3333 && rd(3) == 0x4444);
    check("its local information segment's tidCurrent is its own", rd(4) == tid);
    // (Once the thread has ended: on another CPU it may still be running.)
    unsafe { os2::DosSleep(100) };
    check("and back on the main thread, the main thread's", unsafe { lis.add(3).read_volatile() } == 1);
    // With threads running on several CPUs at once each still sees its own
    // ID there (Open Watcom's 16-bit runtime owns its locks by it: NMPBIND
    // hung in _beginthread).
    LIS16.store(lsel as u32, Ordering::Relaxed);
    let mut tids = [0u32; 4];
    for t in tids.iter_mut() {
        unsafe { os2::DosCreateThread(t, lis16_reader, 0, 0, 16384) };
    }
    for t in tids.iter_mut() {
        unsafe { os2::DosWaitThread(t, os2::DCWW_WAIT) };
    }
    let bad = LIS16_BAD.load(Ordering::Relaxed);
    check(&alloc::format!("16-bit DosGetInfoSeg: four threads at once each read their own tidCurrent ({bad} wrong)"), bad == 0 && LIS16_SEEN.load(Ordering::Relaxed) == 4);
}

static LIS16: AtomicU32 = AtomicU32::new(0);
static LIS16_BAD: AtomicU32 = AtomicU32::new(0);
static LIS16_SEEN: AtomicU32 = AtomicU32::new(0);

/// Compares tidCurrent in the segment 16-bit DosGetInfoSeg named with the
/// thread's ID from its TIB for a while.
unsafe extern "C" fn lis16_reader(_: u32) {
    unsafe {
        let lis = flat(LIS16.load(Ordering::Relaxed) << 16) as *const u16;
        let (mut tib, mut pib): (*mut os2::Tib, *mut os2::Pib) = (core::ptr::null_mut(), core::ptr::null_mut());
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        let me = (*(*tib).ptib2).tid as u16;
        for i in 0..20000u32 {
            if lis.add(3).read_volatile() != me {
                LIS16_BAD.fetch_add(1, Ordering::Relaxed);
            }
            if i % 1000 == 0 {
                os2::DosSleep(0);
            }
        }
        LIS16_SEEN.fetch_add(1, Ordering::Relaxed);
    }
}
