//! 16-bit entry points for rs2's Rust DLLs, so 16-bit programs (NE, the
//! `_cdecl far` calling convention of Microsoft C 6) can call them.
//!
//! `entries16!` emits, in one 4 KB-aligned piece of the code object, a
//! 16-bit stub per entry (`mov ax, N; jmp common`) and the common thunk.
//! A 16-bit caller reaches a stub through its 16:16 alias: elf2lx gives
//! code objects 16-bit code aliases (the tiled LDT), and the loader
//! resolves a 16:16 import of a 32-bit export to that alias. The common
//! code saves BP, DS, ES, SI, DI, BX, FS, jumps to flat 32-bit code (FS
//! the TIB selector), takes
//! the flat alias of the 16-bit stack (SS tile base + SP) and switches
//! to a per-thread 64 KB stack, calls `dispatch(N, args)` (`args` is
//! the flat address of the caller's arguments, after the far return
//! address), puts the 32-bit result in DX:AX, returns to the 16-bit
//! stack and code (through the alias of the epilogue) and `retf`s; the
//! caller removes its arguments.

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

/// Flat address of a 16:16 pointer (tiled: selector >> 3 is the 64 KB
/// tile), null for 0.
pub fn flat(far: u32) -> *mut u8 {
    if far == 0 { core::ptr::null_mut() } else { ((far >> 19) << 16 | far & 0xffff) as *mut u8 }
}

/// The 16:16 alias of a flat address below 512 MB (0 for null).
pub fn far(p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let f = p as u32;
    ((f >> 16) << 3 | 7) << 16 | f & 0xffff
}

/// The arguments of a 16-bit call: `w`/`i` 16-bit words, `l` 32-bit
/// values, `p` far pointers (as flat), at byte offsets.
#[derive(Clone, Copy)]
pub struct Args(pub *const u8);

impl Args {
    pub fn w(&self, off: usize) -> u16 {
        unsafe { (self.0.add(off) as *const u16).read_unaligned() }
    }
    pub fn i(&self, off: usize) -> i32 {
        self.w(off) as i16 as i32
    }
    pub fn l(&self, off: usize) -> u32 {
        unsafe { (self.0.add(off) as *const u32).read_unaligned() }
    }
    pub fn p(&self, off: usize) -> *mut u8 {
        flat(self.l(off))
    }
}

const SLOTS: usize = 256;
const STACK: u32 = 64 * 1024;
static STACKS: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];

/// The top of the calling thread's 32-bit stack (allocated on first use).
pub extern "C" fn thunk16_stack() -> u32 {
    let tid: u32;
    unsafe {
        core::arch::asm!("mov {0}, dword ptr fs:[0x0c]", "mov {0}, dword ptr [{0}]", out(reg) tid, options(nostack, readonly));
    }
    let slot = &STACKS[tid as usize % SLOTS];
    let mut base = slot.load(Ordering::Acquire);
    if base == 0 {
        let mut p: *mut u8 = core::ptr::null_mut();
        if unsafe { os2::DosAllocMem(&mut p, STACK, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT) } != 0 {
            return 0;
        }
        base = p as u32;
        slot.store(base, Ordering::Release);
    }
    base + STACK - 16
}

/// `entries16! { dispatch = path; stub = N; ... }`: 16-bit entry points
/// named `stub` that call `path(N, Args)` (`extern "C" fn(u32, *const
/// u8) -> u32`). Export the stubs under their 16-bit names in
/// exports.def.
#[macro_export]
macro_rules! entries16 {
    (dispatch = $dispatch:path; $($name:ident = $idx:literal;)*) => {
        core::arch::global_asm!(
            ".pushsection .text.thunk16,\"ax\"",
            ".p2align 12",
            ".code16",
            $(
                concat!(".globl ", stringify!($name)),
                concat!(stringify!($name), ":"),
                concat!("mov ax, ", stringify!($idx)),
                "jmp .Lt16_common",
            )*
            ".Lt16_common:",
            "push bp",
            "mov bp, sp",
            "push ds",
            "push es",
            "push si",
            "push di",
            "push bx",
            "push fs",
            // jmp far 0x5b:.Lt16_flat (flat code selector)
            ".byte 0x66, 0xea",
            ".long .Lt16_flat",
            ".word 0x5b",
            ".code32",
            ".Lt16_flat:",
            "cld",
            "movzx ebp, bp",
            "movzx eax, ax",
            "xor ecx, ecx",
            "mov cx, ss",
            "mov edi, ecx",
            "movzx esi, sp",
            "shr ecx, 3",
            "shl ecx, 16",
            "lea ebx, [ecx + ebp + 6]",
            "add ecx, esi",
            "mov dx, 0x53",
            "mov ds, dx",
            "mov es, dx",
            "mov ss, dx",
            "mov esp, ecx",
            // FS: the thread information block (16-bit programs may
            // have it 0).
            "mov dx, 0x150b",
            "mov fs, dx",
            "push edi",
            "push esi",
            "push eax",
            "push ebx",
            "mov ebp, esp",
            "and esp, -16",
            "call {stack}",
            "mov esp, eax",
            "push dword ptr [ebp + 12]",
            "push dword ptr [ebp + 8]",
            "sub esp, 16",
            "push dword ptr [ebp]",
            "push dword ptr [ebp + 4]",
            "call {dispatch}",
            "add esp, 24",
            "pop esi",
            "pop edi",
            "mov edx, eax",
            "shr edx, 16",
            "mov ss, di",
            "mov esp, esi",
            "mov ebx, offset .Lt16_back",
            "mov ecx, ebx",
            "shr ecx, 16",
            "shl ecx, 3",
            "or ecx, 7",
            "and ebx, 0xffff",
            "push ecx",
            "push ebx",
            "retf",
            ".code16",
            ".Lt16_back:",
            "pop fs",
            "pop bx",
            "pop di",
            "pop si",
            "pop es",
            "pop ds",
            "pop bp",
            // retf (16-bit; the assembler would add an operand-size prefix)
            ".byte 0xcb",
            ".code32",
            ".popsection",
            stack = sym $crate::thunk16_stack,
            dispatch = sym $dispatch,
        );
    };
}
