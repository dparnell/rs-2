//! 16-bit entry points for rs2's Rust DLLs, so 16-bit programs can call
//! them, and calls from 32-bit code back into 16-bit code.
//!
//! `entries16!` (`_cdecl far`, the caller removes the arguments, as
//! Microsoft C 6 libraries) and `entries16_pascal!` (`_pascal far`, the
//! callee removes them: the OS/2 API convention) emit, in one 4 KB-aligned
//! piece of the code object, a 16-bit stub per entry (`mov ax, N` and a
//! jump or near call to the common thunk; a `_pascal` stub then does
//! `retf` with its argument size) and the common thunk. A 16-bit caller
//! reaches a stub through its 16:16 alias: elf2lx gives code objects
//! 16-bit code aliases (the tiled LDT), and the loader resolves a 16:16
//! import of a 32-bit export to that alias.
//!
//! The common code saves BP, DS, ES, SI, DI, BX, FS, jumps to flat 32-bit
//! code (FS the TIB selector), takes the flat alias of the 16-bit stack
//! (SS tile base + SP), records the caller's SS:SP and DS in the thread's
//! TIB (see below), switches to a 32-bit stack of the calling thread (one
//! per nesting depth, allocated on first use: a 16-bit procedure called
//! back from 32-bit code may enter again), calls `dispatch(N, args)`
//! (`args` is the flat address of the caller's arguments, after the far
//! return address), puts the 32-bit result in DX:AX, returns to the
//! 16-bit stack and code (through the alias of the epilogue) and returns.
//!
//! Every DLL has its own copy of this code, so what callbacks need is kept
//! in the thread's TIB, which all of them see: rs2's TIBs are 0x40 apart
//! and the TIB and TIB2 use 0x30 bytes. `fs:[0x30]` is the 16-bit SS:SP
//! of the innermost 16-bit caller (below which a callback may put its
//! arguments), `fs:[0x34]` the 32-bit stack pointer of a callback in
//! progress, `fs:[0x38]` the innermost 16-bit caller's DS.

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

/// The arguments of a `_pascal` call, read in the order they are
/// declared: the first argument is at the highest address. `new(args,
/// size)` with the total size of the arguments.
pub struct Pascal {
    base: *const u8,
    off: usize,
}

impl Pascal {
    pub fn new(args: *const u8, size: usize) -> Pascal {
        Pascal { base: args, off: size }
    }
    /// The next 16-bit argument.
    pub fn w(&mut self) -> u16 {
        self.off -= 2;
        unsafe { (self.base.add(self.off) as *const u16).read_unaligned() }
    }
    /// The next 16-bit argument, sign-extended.
    pub fn i(&mut self) -> i32 {
        self.w() as i16 as i32
    }
    /// The next 32-bit argument.
    pub fn l(&mut self) -> u32 {
        self.off -= 4;
        unsafe { (self.base.add(self.off) as *const u32).read_unaligned() }
    }
    /// The next far pointer, as a flat address.
    pub fn p(&mut self) -> *mut u8 {
        flat(self.l())
    }
}

const SLOTS: usize = 256;
const DEPTHS: usize = 12;
const STACK: u32 = 64 * 1024;
static STACKS: [[AtomicU32; DEPTHS]; SLOTS] = [const { [const { AtomicU32::new(0) }; DEPTHS] }; SLOTS];
static DEPTH: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];

fn slot() -> usize {
    let tid: u32;
    unsafe {
        core::arch::asm!("mov {0}, dword ptr fs:[0x0c]", "mov {0}, dword ptr [{0}]", out(reg) tid, options(nostack, readonly));
    }
    tid as usize % SLOTS
}

/// Entering from 16-bit code: the top of the calling thread's 32-bit
/// stack for this nesting depth (allocated on first use).
pub extern "C" fn thunk16_enter() -> u32 {
    let s = slot();
    let d = (DEPTH[s].fetch_add(1, Ordering::Relaxed) as usize).min(DEPTHS - 1);
    let st = &STACKS[s][d];
    let mut base = st.load(Ordering::Acquire);
    if base == 0 {
        let mut p: *mut u8 = core::ptr::null_mut();
        if unsafe { os2::DosAllocMem(&mut p, STACK, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT) } != 0 {
            return 0;
        }
        base = p as u32;
        st.store(base, Ordering::Release);
    }
    base + STACK - 16
}

/// Returning to 16-bit code.
pub extern "C" fn thunk16_leave() {
    let s = slot();
    let _ = DEPTH[s].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| d.checked_sub(1));
}

/// The 16-bit SS:SP below which a callback may use the stack (0: the
/// thread has not come from 16-bit code).
pub fn sp16() -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("mov {0}, dword ptr fs:[0x30]", out(reg) v, options(nostack, readonly)) };
    v
}

pub fn set_sp16(v: u32) {
    unsafe { core::arch::asm!("mov dword ptr fs:[0x30], {0}", in(reg) v, options(nostack)) };
}

/// Room for `n` bytes on the 16-bit stack (below the innermost 16-bit
/// caller's frame, word-aligned): its 16:16 address. Callers put back
/// `sp16()` as it was when the data is no longer needed.
pub fn alloc16(n: usize) -> u32 {
    let v = stack16();
    let sp = ((v & 0xffff) - n as u32) & !1;
    let nv = (v & 0xffff_0000) | sp;
    set_sp16(nv);
    nv
}

/// A 16-bit stack for each thread that never came from 16-bit code.
static FALLBACK: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];

/// The thread's 16-bit stack (one is made for threads that never came
/// from 16-bit code).
fn stack16() -> u32 {
    let v = sp16();
    if v != 0 {
        return v;
    }
    let slot = &FALLBACK[slot()];
    let mut v = slot.load(Ordering::Acquire);
    if v == 0 {
        let mut p: *mut u8 = core::ptr::null_mut();
        const OBJ_TILE: u32 = 0x40;
        if unsafe { os2::DosAllocMem(&mut p, 0x10000, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | OBJ_TILE) } != 0 {
            return 0;
        }
        v = far(p) & 0xffff_0000 | 0xfff0;
        slot.store(v, Ordering::Release);
    }
    set_sp16(v);
    v
}

unsafe extern "C" {
    fn thunk16_call16(target: u32, args: *const u8, len: u32) -> u32;
}

/// Call the 16-bit far procedure `target` with `args` (the bytes of the
/// argument list as the caller pushes them, lowest address first) on the
/// thread's 16-bit stack, with the innermost 16-bit caller's DS: DX:AX.
/// Arguments of `_pascal` procedures are in reverse order of declaration.
pub fn call16(target: u32, args: &[u8]) -> u32 {
    let saved = sp16();
    stack16();
    let r = unsafe { thunk16_call16(target, args.as_ptr(), args.len() as u32) };
    set_sp16(saved);
    r
}

/// Builds the argument bytes of a `_pascal` call: `w`/`l` in the order the
/// parameters are declared.
pub struct PascalCall {
    buf: [u8; 64],
    len: usize,
}

impl PascalCall {
    pub fn new() -> PascalCall {
        PascalCall { buf: [0; 64], len: 0 }
    }
    fn put(&mut self, b: &[u8]) {
        // The first parameter ends up highest: build from the top.
        let n = b.len();
        self.buf.copy_within(0..self.len, n);
        self.buf[..n].copy_from_slice(b);
        self.len += n;
    }
    pub fn w(mut self, v: u16) -> PascalCall {
        self.put(&v.to_le_bytes());
        self
    }
    pub fn l(mut self, v: u32) -> PascalCall {
        self.put(&v.to_le_bytes());
        self
    }
    pub fn call(&self, target: u32) -> u32 {
        call16(target, &self.buf[..self.len])
    }
}

impl Default for PascalCall {
    fn default() -> Self {
        Self::new()
    }
}

// 32-bit code calling 16-bit code: the arguments and the return address
// (the 16:16 alias of `.Lcb16_back`) go below fs:[0x30]; a 16-bit far
// return enters the procedure, whose return comes back through the alias
// to flat code that restores the 32-bit stack from fs:[0x34].
core::arch::global_asm!(
    ".pushsection .text.thunk16cb,\"ax\"",
    ".p2align 8",
    ".code32",
    ".globl thunk16_call16",
    "thunk16_call16:",
    "push ebp",
    "mov ebp, esp",
    "push ebx",
    "push esi",
    "push edi",
    "push dword ptr fs:[0x34]",
    // eax = SS:SP, ebx = SS, edi = flat SP
    "mov eax, dword ptr fs:[0x30]",
    "mov ebx, eax",
    "shr ebx, 16",
    "mov edi, ebx",
    "shr edi, 3",
    "shl edi, 16",
    "movzx eax, ax",
    // new SP: below the arguments, the return address and the target
    "mov ecx, [ebp + 16]",
    "sub eax, ecx",
    "sub eax, 8",
    "and eax, 0xfffe",
    "add edi, eax",
    "mov edx, eax",
    // [sp] target, [sp+4] return address, [sp+8] arguments
    "mov esi, [ebp + 8]",
    "mov [edi], esi",
    "mov esi, offset .Lcb16_back",
    "mov eax, esi",
    "shr eax, 16",
    "shl eax, 3",
    "or eax, 7",
    "mov [edi + 4], si",
    "mov [edi + 6], ax",
    "push edi",
    "lea edi, [edi + 8]",
    "mov esi, [ebp + 12]",
    "cld",
    "rep movsb",
    "pop edi",
    "mov dword ptr fs:[0x34], esp",
    // DS for the procedure: the innermost 16-bit caller's (its SS if none)
    "mov ecx, dword ptr fs:[0x38]",
    "test cx, cx",
    "jnz 2f",
    "mov ecx, ebx",
    "2:",
    "mov ss, bx",
    "movzx esp, dx",
    "mov ds, cx",
    "mov es, cx",
    // 16-bit far return: to the target
    ".byte 0x66, 0xcb",
    ".code16",
    ".Lcb16_back:",
    // jmp far 0x5b:.Lcb32_back
    ".byte 0x66, 0xea",
    ".long .Lcb32_back",
    ".word 0x5b",
    ".code32",
    ".Lcb32_back:",
    "mov cx, 0x150b",
    "mov fs, cx",
    "mov cx, 0x53",
    "mov ds, cx",
    "mov es, cx",
    "mov ss, cx",
    "mov esp, dword ptr fs:[0x34]",
    "cld",
    "pop dword ptr fs:[0x34]",
    "movzx eax, ax",
    "shl edx, 16",
    "or eax, edx",
    "pop edi",
    "pop esi",
    "pop ebx",
    "pop ebp",
    "ret",
    ".popsection",
);

/// The common 16-to-32-bit code of `entries16!` and `entries16_pascal!`
/// (`$args`: offset of the arguments from BP after `push bp`; `$ret`: the
/// 16-bit return instruction).
#[doc(hidden)]
#[macro_export]
macro_rules! common16 {
    ($label:literal, $back:literal, $flat:literal, $args:literal, $ret:literal) => {
        concat!(
            $label, ":\n",
            "push bp\n",
            "mov bp, sp\n",
            "push ds\n",
            "push es\n",
            "push si\n",
            "push di\n",
            "push bx\n",
            "push fs\n",
            // jmp far 0x5b:flat (flat code selector)
            ".byte 0x66, 0xea\n",
            ".long ", $flat, "\n",
            ".word 0x5b\n",
            ".code32\n",
            $flat, ":\n",
            "cld\n",
            "movzx ebp, bp\n",
            "movzx eax, ax\n",
            "xor ecx, ecx\n",
            "mov cx, ss\n",
            "mov edi, ecx\n",
            "movzx esi, sp\n",
            "shr ecx, 3\n",
            "shl ecx, 16\n",
            "lea ebx, [ecx + ebp + ", $args, "]\n",
            "add ecx, esi\n",
            "mov dx, 0x53\n",
            "mov ds, dx\n",
            "mov es, dx\n",
            "mov ss, dx\n",
            "mov esp, ecx\n",
            // FS: the thread information block (16-bit programs may
            // have it 0).
            "mov dx, 0x150b\n",
            "mov fs, dx\n",
            // The caller's SS:SP and DS for callbacks (the previous ones
            // are put back on return).
            "push dword ptr fs:[0x30]\n",
            "push dword ptr fs:[0x38]\n",
            // (below what this code keeps on the 16-bit stack)
            "mov edx, edi\n",
            "shl edx, 16\n",
            "lea edx, [edx + esi - 32]\n",
            "mov dword ptr fs:[0x30], edx\n",
            "movzx edx, word ptr [ecx + 10]\n",
            "mov dword ptr fs:[0x38], edx\n",
            "push edi\n",
            "push esi\n",
            "push eax\n",
            "push ebx\n",
            "mov ebp, esp\n",
            "and esp, -16\n",
            "call {enter}\n",
            "mov esp, eax\n",
            "sub esp, 8\n",
            "push dword ptr [ebp]\n",
            "push dword ptr [ebp + 4]\n",
            "call {dispatch}\n",
            "add esp, 8\n",
            "mov [esp], eax\n",
            "sub esp, 8\n",
            "call {leave}\n",
            "mov eax, [esp + 8]\n",
            "mov esp, ebp\n",
            "mov esi, [ebp + 8]\n",
            "mov edi, [ebp + 12]\n",
            "add esp, 16\n",
            "pop dword ptr fs:[0x38]\n",
            "pop dword ptr fs:[0x30]\n",
            "mov edx, eax\n",
            "shr edx, 16\n",
            "mov ss, di\n",
            "mov esp, esi\n",
            "mov ebx, offset ", $back, "\n",
            "mov ecx, ebx\n",
            "shr ecx, 16\n",
            "shl ecx, 3\n",
            "or ecx, 7\n",
            "and ebx, 0xffff\n",
            "push ecx\n",
            "push ebx\n",
            "retf\n",
            ".code16\n",
            $back, ":\n",
            "pop fs\n",
            "pop bx\n",
            "pop di\n",
            "pop si\n",
            "pop es\n",
            "pop ds\n",
            "pop bp\n",
            ".byte ", $ret, "\n",
        )
    };
}

/// `entries16! { dispatch = path; stub = N; ... }`: `_cdecl far` 16-bit
/// entry points named `stub` that call `path(N, args)` (`extern "C"
/// fn(u32, *const u8) -> u32`; `thunk16::Args(args)` reads them). Export
/// the stubs under their 16-bit names in exports.def.
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
            $crate::common16!(".Lt16_common", ".Lt16_back", ".Lt16_flat", "6", "0xcb"),
            ".code32",
            ".popsection",
            enter = sym $crate::thunk16_enter,
            leave = sym $crate::thunk16_leave,
            dispatch = sym $dispatch,
        );
    };
}

/// `entries16_pascal! { dispatch = path; [pool = NAME, COUNT, BASE,
/// SIZE;] stub = N, SIZE; ... }`: `_pascal far` 16-bit entry points
/// (`SIZE` bytes of arguments, which the stub removes) that call
/// `path(N, args)`; `thunk16::Pascal::new(args, SIZE)` reads them in
/// declaration order. A pool is COUNT stubs from the label NAME on, 9
/// bytes apart, that call `path(BASE + i, args)`. The stubs and the common
/// code must fit in 8 KB (one 16-bit tile; near jumps).
#[macro_export]
macro_rules! entries16_pascal {
    (dispatch = $dispatch:path; pool = $pool:ident, $count:literal, $base:literal, $psize:literal; $($name:ident = $idx:literal, $size:literal;)*) => {
        $crate::entries16_pascal!(@emit $dispatch;
            concat!(".globl ", stringify!($pool)),
            concat!(stringify!($pool), ":"),
            ".set .Lt16p_i, 0",
            concat!(".rept ", stringify!($count)),
            concat!("mov ax, ", stringify!($base), " + .Lt16p_i"),
            ".byte 0xe8",
            ".short .Lt16p_common - . - 2",
            ".byte 0xca",
            concat!(".short ", stringify!($psize)),
            ".set .Lt16p_i, .Lt16p_i + 1",
            ".endr",;
            $($name = $idx, $size;)*);
    };
    (dispatch = $dispatch:path; $($name:ident = $idx:literal, $size:literal;)*) => {
        $crate::entries16_pascal!(@emit $dispatch; ; $($name = $idx, $size;)*);
    };
    (@emit $dispatch:path; $($pre:expr,)*; $($name:ident = $idx:literal, $size:literal;)*) => {
        core::arch::global_asm!(
            ".pushsection .text.thunk16p,\"ax\"",
            ".p2align 13",
            ".code16",
            $($pre,)*
            $(
                concat!(".globl ", stringify!($name)),
                concat!(stringify!($name), ":"),
                concat!("mov ax, ", stringify!($idx)),
                // call near .Lt16p_common
                ".byte 0xe8",
                ".short .Lt16p_common - . - 2",
                // retf SIZE
                ".byte 0xca",
                concat!(".short ", stringify!($size)),
            )*
            $crate::common16!(".Lt16p_common", ".Lt16p_back", ".Lt16p_flat", "8", "0xc3"),
            ".code32",
            ".popsection",
            enter = sym $crate::thunk16_enter,
            leave = sym $crate::thunk16_leave,
            dispatch = sym $dispatch,
        );
    };
}
