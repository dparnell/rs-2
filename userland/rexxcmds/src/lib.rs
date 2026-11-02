//! Shared code of RXQUEUE and RXSUBCOM: the REXXAPI entry points they
//! use, the command line as the C runtime splits it, and REX.MSG/OSO001.MSG
//! messages written to standard error.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// RXSTRING
#[repr(C)]
pub struct RxString {
    pub len: u32,
    pub ptr: *mut u8,
}

unsafe extern "C" {
    #[link_name = "REXXAPI.3"]
    pub fn RexxAddQueue(name: *const u8, data: *const RxString, flag: u32) -> u32;
    #[link_name = "REXXAPI.4"]
    pub fn RexxPullQueue(name: *const u8, data: *mut RxString, dt: *mut u8, wait: u32) -> u32;
    #[link_name = "REXXAPI.5"]
    pub fn RexxQueryQueue(name: *const u8, count: *mut u32) -> u32;
    #[link_name = "REXXAPI.6"]
    pub fn RexxRegisterSubcomDll(env: *const u8, dll: *const u8, proc_: *const u8, user: *const u8, drop: u32) -> u32;
    #[link_name = "REXXAPI.8"]
    pub fn RexxQuerySubcom(env: *const u8, dll: *const u8, flag: *mut u16, user: *mut u8) -> u32;
    #[link_name = "REXXAPI.9"]
    pub fn RexxDeregisterSubcom(env: *const u8, dll: *const u8) -> u32;
    #[link_name = "REXXAPI.10"]
    pub fn RexxLoadSubcom(env: *const u8, dll: *const u8) -> u32;
}

/// The arguments after the program name, each zero-terminated, split as
/// IBM's C runtime does: blanks separate them, double quotes group blanks
/// into one, `\"` is a quote.
pub fn args(s: &os2::Startup) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut cur = Vec::new();
    let mut any = false;
    let mut q = false;
    let a = s.args();
    let mut i = 0;
    while i < a.len() {
        let c = a[i];
        if c == b'\\' && a.get(i + 1) == Some(&b'"') {
            cur.push(b'"');
            any = true;
            i += 2;
            continue;
        }
        match c {
            b'"' => {
                q = !q;
                any = true;
            }
            b' ' | b'\t' if !q => {
                if any {
                    cur.push(0);
                    v.push(core::mem::take(&mut cur));
                    any = false;
                }
            }
            _ => {
                cur.push(c);
                any = true;
            }
        }
        i += 1;
    }
    if any {
        cur.push(0);
        v.push(cur);
    }
    v
}

/// Compare a zero-terminated argument with a keyword, ignoring case.
pub fn is(arg: &[u8], kw: &str) -> bool {
    let a = &arg[..arg.len() - 1];
    a.len() == kw.len() && a.iter().zip(kw.bytes()).all(|(x, y)| x.to_ascii_uppercase() == y)
}

/// Write message `num` of `file` (zero-terminated) to standard error, with
/// one optional insertion. False when the message could not be read.
pub fn message(num: u32, file: &[u8], ins: Option<*const u8>) -> bool {
    let mut buf = [0u8; 256];
    let mut n = 0;
    let tbl = [ins.unwrap_or(core::ptr::null())];
    let count = ins.is_some() as u32;
    let rc = unsafe { os2::DosGetMessage(tbl.as_ptr(), count, buf.as_mut_ptr(), buf.len() as u32, num, file.as_ptr(), &mut n) };
    if rc != 0 {
        return false;
    }
    unsafe { os2::DosPutMessage(2, n, buf.as_ptr()) };
    true
}
