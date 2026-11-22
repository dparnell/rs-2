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

/// Compile the `.RC` script `rc` for a 64-bit module (a build script's
/// job, docs/API64.md): its resource table ([`crate::bind::table64`]), or
/// the diagnostics if there were errors.
pub fn compile_table64(rc: &Path) -> Result<Vec<u8>, String> {
    let dir = rc.parent().unwrap_or(Path::new(".")).to_path_buf();
    let name = rc.file_name().map(|n| String::from(n.to_string_lossy())).unwrap_or_default();
    let mut files = HostFiles { dir };
    let out = crate::compile(&mut files, &name, &crate::Options::default());
    let bad: Vec<String> = out.diags.iter().filter(|d| d.severity() >= crate::Severity::Error).map(|d| d.format()).collect();
    if !bad.is_empty() {
        return Err(bad.join("\n"));
    }
    Ok(crate::bind::table64(&out.resources))
}
