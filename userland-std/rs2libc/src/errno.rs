//! errno (per thread, in the TCB), its texts, and the translations of OS/2
//! error codes and rs2's BSD socket errors into Linux errno values (the
//! ones std and the `libc` crate expect).

use core::ffi::{c_char, c_int};
use libc::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __errno_location() -> *mut c_int {
    unsafe { &mut (*crate::tls::tcb()).errno }
}

pub fn set_errno(e: c_int) {
    unsafe { *__errno_location() = e };
}

pub fn errno() -> c_int {
    unsafe { *__errno_location() }
}

/// Set errno and return -1 (the usual failure return).
pub fn fail(e: c_int) -> c_int {
    set_errno(e);
    -1
}

/// errno for an OS/2 return code.
pub fn from_os2(rc: u32) -> c_int {
    match rc {
        2 | 3 | 15 | 110 | 161 => ENOENT,
        4 => EMFILE,
        5 | 65 => EACCES,
        6 => EBADF,
        8 => ENOMEM,
        16 => EBUSY,
        17 => EXDEV,
        19 => EROFS,
        32 | 33 => EBUSY,
        80 | 183 => EEXIST,
        87 => EINVAL,
        95 => EINTR,
        109 | 230 | 232 => EPIPE,
        112 | 39 => ENOSPC,
        123 => ENOENT,
        145 => ENOTEMPTY,
        206 => ENAMETOOLONG,
        231 => EAGAIN,
        640 => ETIMEDOUT,
        _ => EIO,
    }
}

/// Set errno from an OS/2 return code and return -1.
pub fn fail_os2(rc: u32) -> c_int {
    fail(from_os2(rc))
}

/// Linux errno for a BSD (OS/2 TCP/IP, rs2 kernel) socket error.
pub fn from_bsd(e: u32) -> c_int {
    match e {
        35 => EAGAIN,
        36 => EINPROGRESS,
        37 => EALREADY,
        38 => ENOTSOCK,
        39 => EDESTADDRREQ,
        40 => EMSGSIZE,
        41 => EPROTOTYPE,
        42 => ENOPROTOOPT,
        43 => EPROTONOSUPPORT,
        44 => ESOCKTNOSUPPORT,
        45 => EOPNOTSUPP,
        47 => EAFNOSUPPORT,
        48 => EADDRINUSE,
        49 => EADDRNOTAVAIL,
        50 => ENETDOWN,
        51 => ENETUNREACH,
        52 => ENETRESET,
        53 => ECONNABORTED,
        54 => ECONNRESET,
        55 => ENOBUFS,
        56 => EISCONN,
        57 => ENOTCONN,
        58 => ESHUTDOWN,
        60 => ETIMEDOUT,
        61 => ECONNREFUSED,
        64 => EHOSTDOWN,
        65 => EHOSTUNREACH,
        n if n < 35 => n as c_int,
        _ => EIO,
    }
}

pub fn fail_bsd(e: u32) -> c_int {
    fail(from_bsd(e))
}

fn text(e: c_int) -> &'static str {
    match e {
        0 => "Success",
        EPERM => "Operation not permitted",
        ENOENT => "No such file or directory",
        EINTR => "Interrupted system call",
        EIO => "Input/output error",
        EBADF => "Bad file descriptor",
        EAGAIN => "Resource temporarily unavailable",
        ENOMEM => "Cannot allocate memory",
        EACCES => "Permission denied",
        EBUSY => "Device or resource busy",
        EEXIST => "File exists",
        EXDEV => "Invalid cross-device link",
        ENOTDIR => "Not a directory",
        EISDIR => "Is a directory",
        EINVAL => "Invalid argument",
        EMFILE => "Too many open files",
        ENOSPC => "No space left on device",
        ESPIPE => "Illegal seek",
        EROFS => "Read-only file system",
        EPIPE => "Broken pipe",
        ENAMETOOLONG => "File name too long",
        ENOSYS => "Function not implemented",
        ENOTEMPTY => "Directory not empty",
        ENOTSOCK => "Socket operation on non-socket",
        EADDRINUSE => "Address already in use",
        EADDRNOTAVAIL => "Cannot assign requested address",
        ENETUNREACH => "Network is unreachable",
        ECONNABORTED => "Software caused connection abort",
        ECONNRESET => "Connection reset by peer",
        ENOTCONN => "Transport endpoint is not connected",
        ETIMEDOUT => "Connection timed out",
        ECONNREFUSED => "Connection refused",
        EHOSTUNREACH => "No route to host",
        EINPROGRESS => "Operation now in progress",
        EALREADY => "Operation already in progress",
        ENOPROTOOPT => "Protocol not available",
        EAFNOSUPPORT => "Address family not supported by protocol",
        EOPNOTSUPP => "Operation not supported",
        _ => "Unknown error",
    }
}

unsafe fn copy_out(s: &str, buf: *mut c_char, len: size_t) -> c_int {
    if len == 0 {
        return ERANGE;
    }
    let n = s.len().min(len - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf as *mut u8, n);
        *buf.add(n) = 0;
    }
    if n < s.len() { ERANGE } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __xpg_strerror_r(e: c_int, buf: *mut c_char, len: size_t) -> c_int {
    unsafe { copy_out(text(e), buf, len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strerror(e: c_int) -> *const c_char {
    // Static texts: append the NUL from a table of C strings.
    static mut BUF: [u8; 64] = [0; 64];
    unsafe {
        let b = &raw mut BUF;
        copy_out(text(e), b as *mut c_char, 64);
        b as *const c_char
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gai_strerror(e: c_int) -> *const c_char {
    match e {
        EAI_NONAME => c"Name or service not known".as_ptr(),
        EAI_AGAIN => c"Temporary failure in name resolution".as_ptr(),
        EAI_FAIL => c"Non-recoverable failure in name resolution".as_ptr(),
        EAI_FAMILY => c"ai_family not supported".as_ptr(),
        EAI_MEMORY => c"Memory allocation failure".as_ptr(),
        EAI_SERVICE => c"Servname not supported for ai_socktype".as_ptr(),
        EAI_SYSTEM => c"System error".as_ptr(),
        _ => c"Unknown error".as_ptr(),
    }
}
