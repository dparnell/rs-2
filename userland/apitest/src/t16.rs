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

pub fn run() {
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
