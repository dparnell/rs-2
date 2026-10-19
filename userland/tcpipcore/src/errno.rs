//! sock_errno() and h_errno.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::tls;

pub const SOCBASEERR: i32 = 10000;

pub const EPERM: u32 = 1;
pub const EINTR: u32 = 4;
pub const EBADF: u32 = 9;
pub const ENOMEM: u32 = 12;
pub const EFAULT: u32 = 14;
pub const EINVAL: u32 = 22;
pub const EWOULDBLOCK: u32 = 35;
pub const ENOTSOCK: u32 = 38;
pub const EMSGSIZE: u32 = 40;
pub const ENOPROTOOPT: u32 = 42;
pub const EOPNOTSUPP: u32 = 45;
pub const EAFNOSUPPORT: u32 = 47;
pub const EADDRNOTAVAIL: u32 = 49;
pub const ENETDOWN: u32 = 50;
pub const ENXIO: u32 = 6;
pub const ETIMEDOUT: u32 = 60;
pub const ECONNREFUSED: u32 = 61;

// h_errno values.
pub const HOST_NOT_FOUND: i32 = 1;
pub const TRY_AGAIN: i32 = 2;
pub const NO_RECOVERY: i32 = 3;
pub const NO_DATA: i32 = 4;

/// The last h_errno of any thread (TCP32DLL exports it as the variable
/// `h_errno`).
#[unsafe(no_mangle)]
pub static tcpipcore_h_errno: AtomicI32 = AtomicI32::new(0);

/// Record a BSD errno (SOCBASEERR is added) for sock_errno().
pub fn set(e: u32) {
    let e = e as i32;
    tls::get().errno = if e == 0 || e >= SOCBASEERR { e } else { e + SOCBASEERR };
}

/// Record a value as given (set_errno()).
pub fn set_raw(e: i32) {
    tls::get().errno = e;
}

pub fn get() -> i32 {
    tls::get().errno
}

/// Fail with `e`: -1.
pub fn fail(e: u32) -> i32 {
    set(e);
    -1
}

pub fn set_h(h: i32) {
    tls::get().h_errno = h;
    tcpipcore_h_errno.store(h, Ordering::Relaxed);
}

pub fn h() -> i32 {
    tls::get().h_errno
}

pub fn h_ptr() -> *mut i32 {
    &mut tls::get().h_errno
}

/// Text for a sock_errno() value (with or without SOCBASEERR).
pub fn strerror(e: i32) -> &'static str {
    let e = if e >= SOCBASEERR { e - SOCBASEERR } else { e };
    match e {
        0 => "No error",
        1 => "Not owner",
        2 => "No such file or directory",
        3 => "No such process",
        4 => "Interrupted system call",
        5 => "Input/output error",
        6 => "No such device or address",
        7 => "Argument list too long",
        9 => "Bad file number",
        12 => "Cannot allocate memory",
        13 => "Permission denied",
        14 => "Bad address",
        16 => "Device busy",
        17 => "File exists",
        22 => "Invalid argument",
        23 => "Too many open files in system",
        24 => "Too many open files",
        25 => "Inappropriate ioctl for device",
        32 => "Broken pipe",
        35 => "Operation would block",
        36 => "Operation now in progress",
        37 => "Operation already in progress",
        38 => "Socket operation on non-socket",
        39 => "Destination address required",
        40 => "Message too long",
        41 => "Protocol wrong type for socket",
        42 => "Protocol not available",
        43 => "Protocol not supported",
        44 => "Socket type not supported",
        45 => "Operation not supported on socket",
        46 => "Protocol family not supported",
        47 => "Address family not supported by protocol family",
        48 => "Address already in use",
        49 => "Can't assign requested address",
        50 => "Network is down",
        51 => "Network is unreachable",
        52 => "Network dropped connection on reset",
        53 => "Software caused connection abort",
        54 => "Connection reset by peer",
        55 => "No buffer space available",
        56 => "Socket is already connected",
        57 => "Socket is not connected",
        58 => "Can't send after socket shutdown",
        59 => "Too many references: can't splice",
        60 => "Connection timed out",
        61 => "Connection refused",
        62 => "Too many levels of symbolic links",
        63 => "File name too long",
        64 => "Host is down",
        65 => "No route to host",
        66 => "Directory not empty",
        78 => "Function not implemented",
        100 => "OS/2 Error",
        _ => "Unknown error",
    }
}

/// Text for an h_errno value.
pub fn hstrerror(h: i32) -> &'static str {
    match h {
        0 => "Resolver Error 0 (no error)",
        HOST_NOT_FOUND => "Unknown host",
        TRY_AGAIN => "Host name lookup failure",
        NO_RECOVERY => "Unknown server error",
        NO_DATA => "No address associated with name",
        _ => "Unknown resolver error",
    }
}
