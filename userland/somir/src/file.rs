//! The interface repository files (SOMIR in the environment: a list
//! separated by semicolons, else SOM.IR), read whole when the repository
//! is first made.

use alloc::vec::Vec;

use crate::buf::{get_int, get_str};

pub struct IrFile {
    /// The file name, NUL-terminated.
    pub name: Vec<u8>,
    pub data: Vec<u8>,
}

pub const MAGIC: i32 = 0x0517_9979;

impl IrFile {
    pub fn int(&self, at: u32) -> i32 {
        if at as usize + 4 > self.data.len() {
            return 0;
        }
        get_int(unsafe { self.data.as_ptr().add(at as usize) })
    }
    pub fn ptr(&self, at: u32) -> *const u8 {
        unsafe { self.data.as_ptr().add((at as usize).min(self.data.len())) }
    }
    /// A string at `at`: its address in the file image and the offset
    /// after it.
    pub fn str_at(&self, at: u32) -> (*const u8, u32) {
        let (s, q) = get_str(self.ptr(at));
        (s, (q as usize - self.data.as_ptr() as usize) as u32)
    }
    pub fn off(&self, p: *const u8) -> u32 {
        (p as usize - self.data.as_ptr() as usize) as u32
    }
    /// The root container's contents record.
    pub fn root(&self) -> u32 {
        self.int(8) as u32
    }
    /// Is `at` a record of `kind` (8 contents, 9 reference list, 11
    /// variable data)?
    pub fn is(&self, at: u32, kind: i32) -> bool {
        at != 0 && (at as usize) + 8 <= self.data.len() && self.int(at + 4) == kind
    }
}

pub static mut FILES: Vec<IrFile> = Vec::new();
static mut LOADED: bool = false;
static mut NAMES: Vec<u8> = Vec::new();

#[allow(static_mut_refs)]
pub fn files() -> &'static Vec<IrFile> {
    unsafe { &FILES }
}

/// irGetFileNames: SOMIR's value, or "som.ir".
#[allow(static_mut_refs)]
pub fn file_names() -> *const u8 {
    unsafe {
        if NAMES.is_empty() {
            let mut v: *const u8 = core::ptr::null();
            if os2::DosScanEnv(b"SOMIR\0".as_ptr(), &mut v) == 0 && !v.is_null() && *v != 0 {
                NAMES = somsup::cstr(v).to_vec();
            } else {
                NAMES = b"som.ir".to_vec();
            }
            NAMES.push(0);
        }
        NAMES.as_ptr()
    }
}

fn read_file(name: &[u8]) -> Option<Vec<u8>> {
    let z = somsup::z(name);
    let mut h = 0u32;
    let mut action = 0u32;
    if unsafe { os2::DosOpen(z.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0u32;
        if unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(h) };
    Some(data)
}

/// Read the repository's files (once).
#[allow(static_mut_refs)]
pub fn load() {
    unsafe {
        if LOADED {
            return;
        }
        LOADED = true;
        let names = somsup::cstr(file_names()).to_vec();
        for n in names.split(|&c| c == b';').filter(|n| !n.is_empty()) {
            if let Some(data) = read_file(n) {
                if data.len() >= 0x20 && get_int(data.as_ptr()) == MAGIC {
                    FILES.push(IrFile { name: somsup::z(n), data });
                }
            }
        }
    }
}
