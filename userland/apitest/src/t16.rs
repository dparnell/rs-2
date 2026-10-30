//! 16-bit threads: DosCreateThread (DOSCALLS.145) from 16-bit code starts
//! the thread with the creator's registers but CS:IP and SS:SP (Borland's
//! and Microsoft C's _beginthread pass the thread function in CX:DX and
//! its argument in SI:DI), and the local information segment's
//! tidCurrent is the running thread's (Microsoft C's multithread _chkstk
//! looks the thread's stack limit up by it).

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

pub fn run() {
    suballoc();
    let gate = |f: unsafe extern "C" fn()| far(f as *const u8);
    let (mut gsel, mut lsel) = (0u16, 0u16);
    PascalCall::new().l(far(&raw mut gsel as *const u8)).l(far(&raw mut lsel as *const u8)).call(gate(D16GetInfoSeg));
    let lis = flat((lsel as u32) << 16) as *const u16;
    check("16-bit DosGetInfoSeg: tidCurrent of the main thread is 1", !lis.is_null() && unsafe { lis.add(3).read_volatile() } == 1);
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
}
