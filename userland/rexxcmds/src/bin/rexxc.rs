//! REXXC: Object REXX's tokenizer.
//!
//! ```text
//! REXXC program [output] [/S]
//! ```
//!
//! Checks the program's syntax and, given an output file, writes the
//! program in a form REXX runs but people do not read (rs2's own form; see
//! REXX.DLL's RexxTranslateProgram). Errors in the program are reported as
//! REXX reports them, the return code being the error number. Parameter
//! errors are REX.MSG's messages 127-130 and return code -1. It needs
//! Object REXX: REXX.DLL as SWITCHRX.CMD installs it, else OREXX.DLL.

#![no_std]
#![no_main]

extern crate alloc;

use rexxcmds::*;

os2::entry!(main);

type Translate = extern "C" fn(*const u8, *const u8, *const u8) -> i32;

fn fail(num: u32) -> u32 {
    message(num, b"REX.MSG\0", None);
    0xffff_ffff
}

/// RexxTranslateProgram of the Object REXX interpreter.
fn translator() -> Option<Translate> {
    for m in [&b"REXX\0"[..], b"OREXX\0"] {
        let mut fail = [0u8; 64];
        let mut h = 0u32;
        if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, m.as_ptr(), &mut h) } != 0 {
            continue;
        }
        let mut p = 0usize;
        if unsafe { os2::DosQueryProcAddr(h, 0, b"RexxTranslateProgram\0".as_ptr(), &mut p) } == 0 && p != 0 {
            return Some(unsafe { core::mem::transmute::<usize, Translate>(p) });
        }
    }
    None
}

fn main(s: &os2::Startup) -> u32 {
    let argv = args(s);
    let files: alloc::vec::Vec<&alloc::vec::Vec<u8>> = argv.iter().filter(|a| !(a.len() == 3 && a[0] == b'/' && a[1].eq_ignore_ascii_case(&b's'))).collect();
    if files.is_empty() || files.len() > 2 || argv.iter().any(|a| a.first() == Some(&b'/') && !(a.len() == 3 && a[1].eq_ignore_ascii_case(&b's'))) {
        message(127, b"REX.MSG\0", None);
        return fail(129);
    }
    if files.len() == 2 && files[0].eq_ignore_ascii_case(files[1]) {
        return fail(128);
    }
    let Some(t) = translator() else { return fail(132) };
    let out = files.get(1).map_or(core::ptr::null(), |f| f.as_ptr());
    t(files[0].as_ptr(), out, core::ptr::null()) as u32
}
