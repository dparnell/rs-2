//! Start-up code linked into every 64-bit program and DLL by lld.sh (as an
//! object, whatever crates are in it): it hands the module over to rs2libc,
//! which may be in another module (docs/STD64.md). `--cfg dll` makes a
//! DLL's, else a program's.
#![no_std]
#![crate_type = "lib"]

#[repr(C)]
pub struct Module([usize; 10]);

unsafe extern "C" {
    // link64.ld: this module's.
    static mut RS2_MODULE: Module;
}

#[cfg(not(dll))]
mod exe {
    use super::*;
    unsafe extern "C" {
        fn main(argc: i32, argv: *const *const u8, envp: *const *const u8) -> i32;
        fn __rs2_exe_start(
            m: *mut Module,
            hmod: u32,
            env: *const u8,
            cmdline: *const u8,
            main: unsafe extern "C" fn(i32, *const *const u8, *const *const u8) -> i32,
        ) -> !;
        fn __rs2_thread_start(m: *mut Module, param: usize, f: unsafe extern "C" fn(usize));
    }

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".text.start")]
    pub unsafe extern "C" fn _start(hmod: u32, _reserved: u32, env: *const u8, cmdline: *const u8) -> ! {
        unsafe { __rs2_exe_start(&raw mut RS2_MODULE, hmod, env, cmdline, main) }
    }

    /// Whose rs2libc starts this program: one in a DLL looks for it.
    #[unsafe(no_mangle)]
    pub static RS2_HOST: unsafe extern "C" fn(
        *mut Module,
        u32,
        *const u8,
        *const u8,
        unsafe extern "C" fn(i32, *const *const u8, *const *const u8) -> i32,
    ) -> ! = __rs2_exe_start;

    /// Where the kernel starts the program's threads.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn RS2_THREAD_START(param: usize, f: unsafe extern "C" fn(usize)) {
        unsafe { __rs2_thread_start(&raw mut RS2_MODULE, param, f) }
    }
}

#[cfg(dll)]
mod dll {
    use super::*;
    unsafe extern "C" {
        fn __rs2_dll_init(m: *mut Module, hmod: u32, flag: u32) -> u32;
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn _DLL_InitTerm(hmod: u32, flag: u32) -> u32 {
        unsafe { __rs2_dll_init(&raw mut RS2_MODULE, hmod, flag) }
    }

    /// The ELF entry point (unused).
    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".text.start")]
    pub extern "C" fn _start() {}
}
