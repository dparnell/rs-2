//! The Startup folder: when the desktop starts for the first time after
//! the system started, its program objects start and its shadows open
//! their objects, as the Workplace Shell's Startup folder does. A shell
//! started again in the same session does not (the system atom
//! `RS2SHELL_STARTUP` marks that it was done; PM's atom table lives as
//! long as the PM session). `SET RESTARTOBJECTS=NO` in CONFIG.SYS turns
//! it off, as on OS/2.

use crate::objects;
use crate::pm::*;
use crate::shadows;

const DONE: &[u8] = b"RS2SHELL_STARTUP\0";

/// Whether the Startup folder was processed in this session already
/// (and mark it processed).
fn first_time() -> bool {
    let t = unsafe { WinQuerySystemAtomTable() };
    if unsafe { WinFindAtom(t, DONE.as_ptr()) } != 0 {
        return false;
    }
    unsafe { WinAddAtom(t, DONE.as_ptr()) };
    true
}

fn disabled() -> bool {
    let mut p: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"RESTARTOBJECTS\0".as_ptr(), &mut p) } != 0 || p.is_null() {
        return false;
    }
    cstr(p).eq_ignore_ascii_case(b"NO")
}

/// Start the Startup folder's objects, once per session. Returns how
/// many were started (or opened).
pub fn run(group: u32, open: impl Fn(&shadows::Target)) -> u32 {
    if disabled() || !first_time() {
        return 0;
    }
    let mut n = 0;
    for (h, ..) in objects::titles(group) {
        if let Some(p) = objects::definition(h) {
            if crate::launch::start_program(&p) == 0 {
                n += 1;
            }
        }
    }
    let folder = [&b"G"[..], &objects::hex(group)].concat();
    for s in shadows::in_folder(&folder) {
        if s.target.exists() {
            open(&s.target);
            n += 1;
        }
    }
    n
}
