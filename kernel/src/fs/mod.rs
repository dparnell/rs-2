//! File systems.
//!
//! Bring-up version: the files the boot loader placed in `\RS2\MODULES`
//! appear in the root of drive C:.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::sync::SpinLock;

/// An open file handle.
#[derive(Clone)]
pub enum Handle {
    /// The console device (keyboard in, screen out).
    Con,
    /// The NUL device.
    Nul,
    File(Arc<SpinLock<OpenFile>>),
}

pub struct OpenFile {
    pub path: String,
    pub data: Arc<[u8]>,
    pub pos: u64,
    /// Open mode flags (OPEN_ACCESS_*, OPEN_SHARE_*, ...).
    pub mode: u32,
}

pub struct BootFile {
    pub name: String,
    pub data: Arc<[u8]>,
}

static BOOTFS: SpinLock<Vec<BootFile>> = SpinLock::new(Vec::new());

pub fn init(info: &bootinfo::BootInfo) {
    let mut v = BOOTFS.lock();
    for m in info.modules() {
        let bytes = unsafe {
            core::slice::from_raw_parts(crate::mm::phys::phys_to_virt(m.base), m.len as usize)
        };
        v.push(BootFile { name: m.name().to_ascii_uppercase(), data: Arc::from(bytes) });
    }
}

/// Base name of a path, upper case.
fn base_name(path: &str) -> String {
    path.rsplit(['\\', '/', ':']).next().unwrap_or(path).to_ascii_uppercase()
}

pub fn read_file(path: &str) -> Option<Arc<[u8]>> {
    let want = base_name(path);
    BOOTFS.lock().iter().find(|f| f.name == want).map(|f| f.data.clone())
}

pub fn exists(path: &str) -> bool {
    read_file(path).is_some()
}

/// Locate a DLL for the loader: an explicit path, or `NAME.DLL` along the
/// LIBPATH. Returns the full path and contents.
pub fn find_module(name: &str) -> Option<(String, Arc<[u8]>)> {
    let has_ext = base_name(name).contains('.');
    let file = if has_ext { base_name(name) } else { alloc::format!("{}.DLL", base_name(name)) };
    read_file(&file).map(|d| (alloc::format!("C:\\{file}"), d))
}

pub fn list() -> Vec<(String, usize)> {
    BOOTFS.lock().iter().map(|f| (f.name.clone(), f.data.len())).collect()
}
