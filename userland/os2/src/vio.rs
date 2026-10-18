//! Calls to 16-bit APIs (VIOCALLS, KBDCALLS) from 32-bit code.
//!
//! OS/2 has no 32-bit video API; 32-bit programs reach the 16-bit
//! `_Far16 _Pascal` entry points through a thunk. [`call16`] pushes the
//! arguments as words (left to right, as Pascal does), a 16:16 return
//! address into a two-byte `o32 retf` that lives in the code object's
//! 16-bit code alias (elf2lx marks the object for a 16:16 alias), and jumps
//! to the entry. The stack stays the flat one; it is only kept from
//! straddling a 64 KB boundary, since the callee pops with a 16-bit SP.

core::arch::global_asm!(
    ".section .text.rs2_call16,\"ax\"",
    ".globl rs2_call16",
    "rs2_call16:",
    "    push ebp",
    "    mov ebp, esp",
    "    push ebx",
    "    push esi",
    "    push edi",
    "    mov eax, esp",
    "    cmp ax, 0xfe00",
    "    jb 2f",
    "    mov ax, 0xfe00",
    "    mov esp, eax",
    "2:",
    "    push cs",
    "    mov eax, offset rs2_call16_ret",
    "    push eax",
    "    mov esi, [ebp + 12]",
    "    mov ecx, [ebp + 16]",
    "    xor edx, edx",
    "3:",
    "    cmp edx, ecx",
    "    jae 5f",
    "    mov ax, [esi + edx * 2]",
    "    push ax",
    "    inc edx",
    "    jmp 3b",
    "5:",
    "    mov eax, offset rs2_ret16",
    "    mov ebx, eax",
    "    shr ebx, 16",
    "    shl ebx, 3",
    "    or ebx, 7",
    "    shl ebx, 16",
    "    and eax, 0xffff",
    "    or eax, ebx",
    "    push eax",
    "    jmp dword ptr [ebp + 8]",
    "rs2_call16_ret:",
    "    movzx eax, ax",
    "    lea esp, [ebp - 12]",
    "    pop edi",
    "    pop esi",
    "    pop ebx",
    "    pop ebp",
    "    ret",
    "rs2_ret16:",
    "    .byte 0x66, 0xcb",
);

unsafe extern "C" {
    fn rs2_call16(entry: usize, words: *const u16, n: u32) -> u32;
}

/// Call the 16-bit entry `entry` with `words` pushed left to right; returns AX.
///
/// # Safety
/// The words must match the API's argument list; pointers among them
/// (see [`far16`]) must stay valid during the call.
pub unsafe fn call16(entry: usize, words: &[u16]) -> u16 {
    unsafe { rs2_call16(entry, words.as_ptr(), words.len() as u32) as u16 }
}

/// The tiled 16:16 alias of a flat address in the first 512 MB.
pub fn far16<T>(p: *const T) -> u32 {
    let a = p as u32;
    (((a >> 16) << 3 | 7) << 16) | (a & 0xffff)
}

/// Words for a far pointer argument (selector pushed first).
pub fn ptr_words<T>(p: *const T) -> [u16; 2] {
    let f = far16(p);
    [(f >> 16) as u16, f as u16]
}

/// VIOMODEINFO. Aligned so it never crosses a 64 KB tile.
#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
pub struct VioModeInfo {
    pub cb: u16,
    pub fb_type: u8,
    pub color: u8,
    pub col: u16,
    pub row: u16,
    pub hres: u16,
    pub vres: u16,
    pub fmt_id: u8,
    pub attrib: u8,
    pub buf_addr: u32,
    pub buf_length: u32,
    pub full_length: u32,
    pub partial_length: u32,
    pub ext_data_addr: u32,
}

#[repr(C, align(64))]
#[derive(Default)]
struct Aligned<T>(T);

unsafe extern "C" {
    #[link_name = "VIOCALLS.21"]
    fn VIO16GETMODE();
    #[link_name = "VIOCALLS.22"]
    fn VIO16SETMODE();
}

pub fn vio_get_mode() -> Result<VioModeInfo, u16> {
    let mut m = Aligned(VioModeInfo { cb: core::mem::size_of::<VioModeInfo>() as u16, ..Default::default() });
    let p = ptr_words(&mut m.0 as *mut VioModeInfo);
    let rc = unsafe { call16(VIO16GETMODE as *const () as usize, &[p[0], p[1], 0]) };
    // Written by the API behind the compiler's back.
    let m = unsafe { core::ptr::read_volatile(&m.0) };
    if rc == 0 { Ok(m) } else { Err(rc) }
}

pub fn vio_set_mode(m: &VioModeInfo) -> Result<(), u16> {
    let a = Aligned(*m);
    let p = ptr_words(&a.0);
    let rc = unsafe { call16(VIO16SETMODE as *const () as usize, &[p[0], p[1], 0]) };
    if rc == 0 { Ok(()) } else { Err(rc) }
}
