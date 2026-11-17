//! Host file access for tests and the host `rc` example: OS/2 paths
//! (backslashes, any case) on a Unix file system, relative to a directory.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;
use std::path::{Path, PathBuf};

pub struct HostFiles {
    pub dir: PathBuf,
}

/// `p` with each component matched case-insensitively (Toolkit files are
/// lower case; scripts name them in any case).
pub fn resolve(base: &Path, p: &str) -> Option<PathBuf> {
    let p = p.replace('\\', "/");
    let mut cur = if p.starts_with('/') { PathBuf::from("/") } else { base.to_path_buf() };
    for comp in p.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            cur.pop();
            continue;
        }
        let exact = cur.join(comp);
        if exact.exists() {
            cur = exact;
            continue;
        }
        let found = std::fs::read_dir(&cur).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(comp))?;
        cur = found.path();
    }
    Some(cur)
}

impl crate::Files for HostFiles {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let p = resolve(&self.dir, path)?;
        if p.is_file() { std::fs::read(p).ok() } else { None }
    }
}

pub fn path_string(p: &Path) -> String {
    String::from(p.to_string_lossy())
}
