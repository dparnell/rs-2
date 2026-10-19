//! PMWP.DLL: the Workplace Shell's entry points that programs use
//! without the Workplace Shell itself. So far the file icon calls,
//! which PMWIN implements (rs2-private ordinals 9004-9006); these entries
//! jump there.

#![no_std]
#![no_main]

extern crate os2;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

macro_rules! forward {
    ($($name:ident = $ord:literal;)*) => {$(
        #[allow(non_snake_case)]
        mod $name {
            unsafe extern "C" {
                #[link_name = concat!("PMWIN.", stringify!($ord))]
                pub fn target();
            }
        }
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!("jmp {}", sym $name::target);
        }
    )*};
}

forward! {
    WinLoadFileIcon = 9004;
    WinFreeFileIcon = 9005;
    WinSetFileIcon = 9006;
}
