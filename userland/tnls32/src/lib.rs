//! TNLS32.DLL: the national-language helper of IBM's TCP/IP applications
//! (FTP, TELNET, FINGER, TALK, REXEC, RSH, ... import it). It translates
//! between the PC code page and network character sets (ISO 8859, the
//! JIS/EUC/ISO 2022 forms for DBCS countries) and keeps the DBCS tables.
//!
//! IBM ships no import library or headers for it and imports are by
//! ordinal only, so the entries here are what the applications'
//! disassembly shows them using (`_Optlink`: arguments in EAX, EDX, ECX,
//! the rest on the stack after 12 reserved bytes; the caller cleans up).
//! rs2 has no translation tables: initialisation reports "no
//! translation", converters copy their input, the DBCS lead-byte table
//! is empty.
//!
//! | Ord | Use |
//! |-----|-----|
//! | 6 | data: DBCS lead-byte vector (256 bytes) |
//! | 7, 34 | data: the code page used to open converters |
//! | 16 | convert to characters(handle, USHORT *dst, src, len): characters out |
//! | 17 | convert(handle, dst, src, len): bytes out |
//! | 19, 20, 49 | data: translation state flags the applications reset |
//! | 24 | init(app name, translation name): flags (bit 0 name unknown, 1 tables, 2 DBCS) |
//! | 38 | close(handle) |
//! | 39 | query tables(&a, &b) |
//! | 40 | re-initialise(name): flags as 24 |
//! | 43 | open converter(from, to): handle |
//! | 48 | per-process set-up, no arguments |
//! | 50 | release(&name) |
//!
//! 16-bit programs (TFTP, LPD, TELNETDC) call three entries with 16:16
//! pointers (`_cdecl far`, through thunk16): 9 init (as 24), 37 release
//! (as 50), 51 query tables (as 39).

#![no_std]
#![no_main]

extern crate os2;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// `_Optlink` entry points to `extern "C"` functions of 0-4 arguments,
/// realigning the stack to 16 bytes.
macro_rules! optlink {
    ($($tramp:ident => $target:ident;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
                "mov [esp], eax", "mov [esp + 4], edx", "mov [esp + 8], ecx",
                "mov ecx, [ebp + 20]", "mov [esp + 12], ecx",
                "call {f}",
                "mov esp, ebp", "pop ebp", "ret",
                f = sym $target,
            );
        }
    )*};
}

optlink! {
    tnls_cv16 => widen;
    tnls_cv17 => convert;
    tnls_init24 => init;
    tnls_close38 => close;
    tnls_tables39 => tables;
    tnls_init40 => reinit;
    tnls_open43 => open;
    tnls_setup48 => setup;
    tnls_release50 => release;
}

extern "C" fn convert(_h: u32, dst: *mut u8, src: *const u8, len: i32) -> i32 {
    if len <= 0 || dst.is_null() || src.is_null() {
        return 0;
    }
    if dst as *const u8 != src {
        unsafe { core::ptr::copy(src, dst, len as usize) };
    }
    len
}

/// Bytes to 16-bit characters (TELNET's terminal emulators take DBCS
/// characters as USHORTs, single-byte ones with a zero high byte).
extern "C" fn widen(_h: u32, dst: *mut u16, src: *const u8, len: i32) -> i32 {
    if len <= 0 || dst.is_null() || src.is_null() {
        return 0;
    }
    // Back to front, so a conversion in place does not overwrite input.
    for i in (0..len as usize).rev() {
        unsafe { dst.add(i).write_unaligned(*src.add(i) as u16) };
    }
    len
}

extern "C" fn init(_app: *const u8, _name: *const u8) -> u32 {
    0
}

extern "C" fn reinit(_name: *const u8) -> u32 {
    0
}

extern "C" fn close(_h: u32) -> u32 {
    0
}

extern "C" fn tables(a: *mut u32, b: *mut u32) -> u32 {
    unsafe {
        if !a.is_null() {
            *a = 0;
        }
        if !b.is_null() {
            *b = 0;
        }
    }
    0
}

extern "C" fn open(_from: u32, _to: u32) -> u32 {
    1
}

extern "C" fn setup() -> u32 {
    0
}

extern "C" fn release(_p: *mut u32) -> u32 {
    0
}

thunk16::entries16! {
    dispatch = dispatch16;
    tnls16_init9 = 9;
    tnls16_release37 = 37;
    tnls16_tables51 = 51;
}

extern "C" fn dispatch16(ord: u32, args: *const u8) -> u32 {
    let a = thunk16::Args(args);
    if ord == 51 {
        for off in [0, 4] {
            let p = a.p(off) as *mut u32;
            if !p.is_null() {
                unsafe { p.write_unaligned(0) };
            }
        }
    }
    0
}

#[unsafe(no_mangle)]
pub static mut tnls_dbcs_vector: [u8; 256] = [0; 256];
#[unsafe(no_mangle)]
pub static mut tnls_codepage: u32 = 850;
#[unsafe(no_mangle)]
pub static mut tnls_codepage2: u32 = 850;
#[unsafe(no_mangle)]
pub static mut tnls_flag19: u32 = 0;
#[unsafe(no_mangle)]
pub static mut tnls_flag20: u32 = 0;
#[unsafe(no_mangle)]
pub static mut tnls_flag49: u32 = 0;
