//! Versions, system information, errors, debugging and the rest.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::text::to_wide;
use crate::{cstr, err, fail, put_str, put_wstr, set_error, wstr, FALSE, TRUE};

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetLastError() -> u32 {
    crate::last_error()
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetLastError(e: u32) {
    set_error(e);
}

/// Windows XP: 5.1, build 2600.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetVersion() -> u32 {
    2600 << 16 | 1 << 8 | 5
}

/// OSVERSIONINFOA (148 bytes) or OSVERSIONINFOEXA (156).
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetVersionExA(info: *mut u32) -> i32 {
    let size = unsafe { *info };
    if size != 148 && size != 156 {
        return fail(err::INSUFFICIENT_BUFFER, FALSE);
    }
    unsafe {
        core::ptr::write_bytes(info.add(1) as *mut u8, 0, size as usize - 4);
        *info.add(1) = 5;
        *info.add(2) = 1;
        *info.add(3) = 2600;
        *info.add(4) = 2; // VER_PLATFORM_WIN32_NT
        if size == 156 {
            // wProductType: VER_NT_WORKSTATION.
            *(info as *mut u8).add(154) = 1;
        }
    }
    TRUE
}

/// OSVERSIONINFOW (276 bytes) or OSVERSIONINFOEXW (284).
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetVersionExW(info: *mut u32) -> i32 {
    let size = unsafe { *info };
    if size != 276 && size != 284 {
        return fail(err::INSUFFICIENT_BUFFER, FALSE);
    }
    unsafe {
        core::ptr::write_bytes(info.add(1) as *mut u8, 0, size as usize - 4);
        *info.add(1) = 5;
        *info.add(2) = 1;
        *info.add(3) = 2600;
        *info.add(4) = 2;
        if size == 284 {
            *(info as *mut u8).add(282) = 1;
        }
    }
    TRUE
}

/// SYSTEM_INFO.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetSystemInfo(si: *mut u32) {
    let mut cpus = 1u32;
    // QSV_NUMPROCESSORS
    unsafe { os2::DosQuerySysInfo(26, 26, &mut cpus, 4) };
    let cpus = cpus.max(1);
    unsafe {
        *si = 0; // PROCESSOR_ARCHITECTURE_INTEL
        *si.add(1) = 4096;
        *si.add(2) = 0x1_0000;
        *si.add(3) = 0x7ffe_ffff;
        *si.add(4) = if cpus >= 32 { u32::MAX } else { (1 << cpus) - 1 };
        *si.add(5) = cpus;
        *si.add(6) = 586; // PROCESSOR_INTEL_PENTIUM
        *si.add(7) = 0x1_0000;
        *si.add(8) = 6; // wProcessorLevel 6, wProcessorRevision 0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetNativeSystemInfo(si: *mut u32) {
    unsafe { GetSystemInfo(si) };
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsProcessorFeaturePresent(f: u32) -> i32 {
    // PF_FLOATING_POINT_PRECISION_ERRATA 0 no; MMX 3, XMMI 6, RDTSC 8,
    // XMMI64 10 (SSE2) yes.
    matches!(f, 3 | 6 | 8 | 10) as i32
}

/// The Windows directory: \WIN32 on the boot drive (made when first
/// asked for). Programs keep INI files there.
pub fn windows_dir() -> Vec<u8> {
    let d = crate::file::boot_drive();
    let mut v = alloc::vec![b'A' + d as u8 - 1];
    v.extend_from_slice(b":\\WIN32");
    static MADE: AtomicU32 = AtomicU32::new(0);
    if MADE.swap(1, Ordering::Relaxed) == 0 {
        let p = crate::z(&v);
        unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
    }
    v
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetWindowsDirectoryA(buf: *mut u8, size: u32) -> u32 {
    unsafe { put_str(&windows_dir(), buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetWindowsDirectoryW(buf: *mut u16, size: u32) -> u32 {
    unsafe { put_wstr(&to_wide(&windows_dir()), buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetSystemWindowsDirectoryA(buf: *mut u8, size: u32) -> u32 {
    unsafe { GetWindowsDirectoryA(buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetSystemDirectoryA(buf: *mut u8, size: u32) -> u32 {
    unsafe { put_str(&windows_dir(), buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetSystemDirectoryW(buf: *mut u16, size: u32) -> u32 {
    unsafe { put_wstr(&to_wide(&windows_dir()), buf, size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetComputerNameA(buf: *mut u8, size: *mut u32) -> i32 {
    let n = crate::process::env_get(b"HOSTNAME").unwrap_or_else(|| b"RS2".to_vec());
    let n: Vec<u8> = n.iter().take(15).map(|c| c.to_ascii_uppercase()).collect();
    let r = unsafe { put_str(&n, buf, *size) };
    if r as usize != n.len() {
        unsafe { *size = r };
        return fail(err::INSUFFICIENT_BUFFER, FALSE);
    }
    unsafe { *size = r };
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetComputerNameW(buf: *mut u16, size: *mut u32) -> i32 {
    let mut a = [0u8; 32];
    let mut n = 32u32;
    unsafe { GetComputerNameA(a.as_mut_ptr(), &mut n) };
    let w = to_wide(&a[..n as usize]);
    let r = unsafe { put_wstr(&w, buf, *size) };
    unsafe { *size = r };
    (r as usize == w.len()) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsDebuggerPresent() -> i32 {
    FALSE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn OutputDebugStringA(s: *const u8) {
    crate::trace(unsafe { cstr(s) });
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn OutputDebugStringW(s: *const u16) {
    crate::trace(&crate::text::from_wide(unsafe { wstr(s) }));
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DebugBreak() {
    unsafe { core::arch::asm!("int3") };
}

static ERROR_MODE: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetErrorMode(m: u32) -> u32 {
    ERROR_MODE.swap(m, Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetErrorMode() -> u32 {
    ERROR_MODE.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetUnhandledExceptionFilter(f: u32) -> u32 {
    unsafe { crate::Rs2W32SetUnhandledFilter(f) }
}

/// UnhandledExceptionFilter(EXCEPTION_POINTERS): the program's filter if
/// it set one; else EXCEPTION_CONTINUE_SEARCH, so that the exception
/// reaches the kernel's default action (the process ends with POPUPLOG's
/// report).
#[unsafe(no_mangle)]
pub extern "stdcall" fn UnhandledExceptionFilter(ep: u32) -> i32 {
    let f = unsafe { crate::Rs2W32QueryUnhandledFilter() };
    if f != 0 && f != UnhandledExceptionFilter as *const () as u32 {
        return crate::call(f, ep, 0, 0, 0) as i32;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Beep(_freq: u32, _ms: u32) -> i32 {
    TRUE
}

/// FormatMessageA: FORMAT_MESSAGE_FROM_SYSTEM gives "Error n."; from a
/// string, its text with %1..%9 from the arguments (strings).
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FormatMessageA(flags: u32, src: *const u8, id: u32, _lang: u32, buf: *mut u8, size: u32, args: *const u32) -> u32 {
    const ALLOCATE: u32 = 0x100;
    const IGNORE_INSERTS: u32 = 0x200;
    const FROM_STRING: u32 = 0x400;
    const ARGUMENT_ARRAY: u32 = 0x2000;
    let text: Vec<u8> = if flags & FROM_STRING != 0 {
        let s = unsafe { cstr(src) };
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.len() {
            if s[i] == b'%' && i + 1 < s.len() && flags & IGNORE_INSERTS == 0 {
                let c = s[i + 1];
                if (b'1'..=b'9').contains(&c) && !args.is_null() {
                    let k = (c - b'1') as usize;
                    // Without ARGUMENT_ARRAY args is a va_list, which on
                    // i386 is the same array of words.
                    let _ = ARGUMENT_ARRAY;
                    let p = unsafe { *args.add(k) } as *const u8;
                    out.extend_from_slice(unsafe { cstr(p) });
                    i += 2;
                    // A "!s!" format after the number.
                    if s.get(i) == Some(&b'!') {
                        if let Some(j) = s[i + 1..].iter().position(|&x| x == b'!') {
                            i += j + 2;
                        }
                    }
                    continue;
                }
                match c {
                    b'n' => out.extend_from_slice(b"\r\n"),
                    b'%' => out.push(b'%'),
                    _ => out.push(c),
                }
                i += 2;
                continue;
            }
            out.push(s[i]);
            i += 1;
        }
        out
    } else {
        alloc::format!("Error {id}.\r\n").into_bytes()
    };
    if flags & ALLOCATE != 0 {
        let p = crate::mem::LocalAlloc(0x40, text.len() as u32 + 1) as *mut u8;
        if p.is_null() {
            return fail(err::NOT_ENOUGH_MEMORY, 0);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len());
            *(buf as *mut *mut u8) = p;
        }
        return text.len() as u32;
    }
    if (size as usize) < text.len() + 1 {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    unsafe { put_str(&text, buf, size) }
}

/// The 8.3 name programs ask about, as given.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetProcessVersion(_pid: u32) -> u32 {
    5 << 16 | 1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPriorityClass(_h: u32) -> u32 {
    0x20 // NORMAL_PRIORITY_CLASS
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPriorityClass(_h: u32, _c: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetProcessTimes(_h: u32, created: *mut u32, exited: *mut u32, kernel: *mut u32, user: *mut u32) -> i32 {
    unsafe {
        crate::time::put_ft(created, crate::time::now_ft());
        crate::time::put_ft(exited, 0);
        crate::time::put_ft(kernel, 0);
        crate::time::put_ft(user, crate::time::GetTickCount() as u64 * 10_000);
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetThreadTimes(h: u32, c: *mut u32, e: *mut u32, k: *mut u32, u: *mut u32) -> i32 {
    unsafe { GetProcessTimes(h, c, e, k, u) }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetProcessAffinityMask(_h: u32, process: *mut u32, system: *mut u32) -> i32 {
    let mut cpus = 1u32;
    unsafe { os2::DosQuerySysInfo(26, 26, &mut cpus, 4) };
    let m = if cpus >= 32 { u32::MAX } else { (1 << cpus.max(1)) - 1 };
    unsafe {
        *process = m;
        *system = m;
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetProcessAffinityMask(_h: u32, _m: u32) -> i32 {
    TRUE
}

/// Another thread's registers: not available (rs2 has no call for it
/// yet).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetThreadContext(_h: u32, _ctx: u32) -> i32 {
    fail(err::CALL_NOT_IMPLEMENTED, FALSE)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetThreadContext(_h: u32, _ctx: u32) -> i32 {
    fail(err::CALL_NOT_IMPLEMENTED, FALSE)
}
