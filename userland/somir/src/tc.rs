//! SOMTC.DLL's calls and TypeCode constants (by IBM's ordinals).

use somsup::Environment;

pub const TK_OBJREF: u32 = 15;
pub const TK_STRUCT: u32 = 16;
pub const TK_UNION: u32 = 17;
pub const TK_ENUM: u32 = 18;
pub const TK_STRING: u32 = 19;
pub const TK_SEQUENCE: u32 = 20;
pub const TK_ARRAY: u32 = 21;
pub const TK_POINTER: u32 = 101;
pub const TK_SELF: u32 = 102;
pub const TK_FOREIGN: u32 = 103;

unsafe extern "C" {
    #[link_name = "SOMTC.33"]
    pub fn tcNewVL(kind: u32, ap: *const usize) -> usize;
    #[link_name = "SOMTC.15"]
    pub fn tcSetAlignment(t: usize, ev: *mut Environment, a: i32);
    #[link_name = "SOMTC.25"]
    pub fn tcFree(t: usize, ev: *mut Environment);
    #[link_name = "SOMTC.28"]
    pub fn tcCopy(t: usize, ev: *mut Environment) -> usize;
    #[link_name = "SOMTC.13"]
    fn tcBufferLength();
    #[link_name = "SOMTC.14"]
    fn tcCopyToBuffer();
    #[link_name = "SOMTC.2"]
    pub static TC__string: u32;
    #[link_name = "SOMTC.42"]
    pub static TC__null: u32;
    #[link_name = "SOMTC.21"]
    pub static TC__AttributeDescription: u32;
    #[link_name = "SOMTC.3"]
    pub static TC__ConstDescription: u32;
    #[link_name = "SOMTC.19"]
    pub static TC__ExceptionDescription: u32;
    #[link_name = "SOMTC.17"]
    pub static TC__InterfaceDescription: u32;
    #[link_name = "SOMTC.5"]
    pub static TC__ModuleDescription: u32;
    #[link_name = "SOMTC.18"]
    pub static TC__OperationDescription: u32;
    #[link_name = "SOMTC.20"]
    pub static TC__ParameterDescription: u32;
    #[link_name = "SOMTC.4"]
    pub static TC__TypeDescription: u32;
    #[link_name = "SOMTC.34"]
    pub static TC__FullInterfaceDescription: u32;
}

/// tcBufferLength (IBM's _Optlink: the TypeCode in EAX).
pub fn buffer_length(t: usize) -> usize {
    let r: usize;
    unsafe {
        core::arch::asm!(
            "sub esp, 4", "call {f}", "add esp, 4",
            f = sym tcBufferLength,
            inout("eax") t => r, out("ecx") _, out("edx") _,
        );
    }
    r
}

/// tcCopyToBuffer(t, buffer, allocated) (_Optlink): the end of the copy.
pub fn copy_to_buffer(t: usize, buf: usize) -> usize {
    let r: usize;
    unsafe {
        core::arch::asm!(
            "sub esp, 12", "call {f}", "add esp, 12",
            f = sym tcCopyToBuffer,
            inout("eax") t => r, inout("edx") buf => _, inout("ecx") 0usize => _,
        );
    }
    r
}

pub fn tc_ptr(r: &'static u32) -> usize {
    r as *const u32 as usize
}
