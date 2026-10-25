//! The shell process's part (PMSHELL calls ShlStartWorkplace, as IBM's
//! PMSHELL calls PMWP's): the classes, the first start's objects
//! (install.rs), the server for other processes' object calls (ipc.rs),
//! the desktop, and the Startup folder's objects, once per PM session
//! (the system atom `WPS_STARTUP` marks it done; `SET
//! RESTARTOBJECTS=NO` turns it off, as on OS/2).

use crate::classes_gen::m;
use crate::obj::{self, OPEN_DEFAULT};
use crate::pm::*;
use crate::{install, ipc, wps};

const STARTED: &[u8] = b"WPS_STARTUP\0";

fn startup_wanted() -> bool {
    let mut p: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"RESTARTOBJECTS\0".as_ptr(), &mut p) } == 0 && !p.is_null() && cstr(p).eq_ignore_ascii_case(b"NO") {
        return false;
    }
    let t = unsafe { WinQuerySystemAtomTable() };
    if unsafe { WinFindAtom(t, STARTED.as_ptr()) } != 0 {
        return false;
    }
    unsafe { WinAddAtom(t, STARTED.as_ptr()) };
    true
}

/// The Startup folder's objects open (programs start).
fn run_startup() -> u32 {
    let f = wps::object_from_location(b"<WP_START>");
    if f == 0 {
        return 0;
    }
    let mut n = 0;
    let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, 0, 0]);
    while o != 0 && n < 200 {
        obj::call(m::WPObject::wpViewObject, &[o, 0, OPEN_DEFAULT as usize, 0]);
        n += 1;
        o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, o, 1]);
    }
    n
}

/// ShlStartWorkplace(hab, hmq): start the Workplace Shell in this process
/// (its PM thread). FALSE if another process runs it.
#[unsafe(no_mangle)]
pub extern "C" fn ShlStartWorkplace(hab: u32, _hmq: u32) -> u32 {
    if ipc::shell_running() {
        return 0;
    }
    if !wps::start() {
        return 0;
    }
    install::run();
    crate::printer::sync_printers();
    crate::setup::apply_saved();
    ipc::start_server(hab);
    let desk = wps::object_from_location(b"<WP_DESKTOP>");
    if desk != 0 {
        obj::call(m::WPObject::wpViewObject, &[desk, 0, OPEN_DEFAULT as usize, 0]);
    }
    crate::minwin::apply();
    if startup_wanted() {
        run_startup();
    }
    1
}

/// WinShutdownSystem(hab, hmq): close the programs and turn off.
#[unsafe(no_mangle)]
pub extern "C" fn WinShutdownSystem(_hab: u32, _hmq: u32) -> u32 {
    crate::shutdown::begin(false);
    1
}

/// Before turning off: what the lazy writer has not saved yet.
pub fn save_all() {
    crate::obj::flush_dirty();
}
