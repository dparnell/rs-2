//! RS2SHELL.EXE: rs2's desktop shell. Not the Workplace Shell (no SOM):
//! a PM program that shows
//!
//! - the desktop: a container in icon view covering the screen, kept
//!   below every other window (PMWIN's `Rs2SetDesktopWindow`), with the
//!   program groups as folders, Drives, the Minimized Window Viewer, the
//!   Window List, Shutdown and the program objects of the hidden
//!   "Desktop" group;
//! - folder windows: a group's program objects, the drives, a directory
//!   (files and folders with their icons, a details view with size and
//!   date), the minimized windows;
//! - pop-up menus (button 2), a settings notebook for program objects,
//!   direct editing of names (Alt+click, F2), deleting with
//!   confirmation, dragging icons with button 2 (positions are kept in
//!   OS2.INI; a file dropped on another folder moves there);
//! - the shutdown sequence (WM_SAVEAPPLICATION and WM_CLOSE to every
//!   program, DosShutdown, ACPI power off).
//!
//! Program objects are the program list in OS2.INI (PrfAddProgram and
//! friends), so the API sees what the desktop shows; see objects.rs for
//! the shell's own profile entries. The first start makes the default
//! objects (rs2's own MAKEINI step).
//!
//! Usage: `RS2SHELL [/MAKEINI]`: /MAKEINI only makes the default objects
//! (if missing) and ends.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

mod dialogs;
mod launch;
mod objects;
mod pm;
mod shutdown;
mod view;

use alloc::vec::Vec;

use pm::*;

os2::entry!(main);

/// Everything the shell knows (one thread: the window thread).
pub struct Shell {
    pub hab: u32,
    pub pid: u32,
    pub groups: objects::Groups,
    pub views: Vec<view::View>,
    /// The shell's icons by pmwin::shellicons id (index 0 unused).
    pub icons: Vec<u32>,
    /// Frames hidden by minimizing (the minimized window viewer's).
    pub minimized: Vec<u32>,
    pub drag: Option<view::Drag>,
    pub next_key: u32,
    /// The pop-up menu on show, and what it was opened on: (client of
    /// the view, item key; 0 = the folder itself).
    pub popup: u32,
    pub popup_on: (u32, u32),
    /// Direct editing: the buffer the container copies the new name to.
    pub edit: Vec<u8>,
    pub shutdown: Option<shutdown::Shutdown>,
}

static mut SHELL: Option<Shell> = None;

pub fn sh() -> &'static mut Shell {
    unsafe { (*core::ptr::addr_of_mut!(SHELL)).as_mut().unwrap() }
}

pub const CLASS: &[u8] = b"RS2Folder\0";

/// One of the shell's own icons (pmwin::shellicons ids).
pub fn icon(id: u32) -> u32 {
    sh().icons.get(id as usize).copied().unwrap_or(0)
}

pub fn sys_icon(id: i32) -> u32 {
    unsafe { WinQuerySysPointer(HWND_DESKTOP, id, 0) }
}

/// Minimized windows go to the Minimized Window Viewer (the default) or
/// become icons at the bottom of the desktop (System Setup;
/// RS2_Desktop MinimizeTo = DESKTOP).
pub fn apply_minimize_setting() {
    let viewer = objects::query(objects::APP_DESKTOP, b"MinimizeTo").is_none_or(|v| v != b"DESKTOP");
    let client = sh().views.first().map_or(0, |d| d.client);
    unsafe { Rs2SetMinimizeViewer(if viewer { client } else { 0 }) };
}

/// Run %OS2_SHELL% (else C:\OS2\CMD.EXE) and wait for it.
fn text_shell() -> u32 {
    let mut p: *const u8 = core::ptr::null();
    let name = if unsafe { os2::DosScanEnv(b"OS2_SHELL\0".as_ptr(), &mut p) } == 0 && !p.is_null() { cstr(p) } else { objects::sys_path(b"OS2\\CMD.EXE") };
    let prog = z(&name);
    let mut args = name.clone();
    args.extend_from_slice(b"\0\0\0");
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), core::ptr::null(), &mut res, prog.as_ptr()) };
    res.code_result
}

fn main(s: &os2::Startup) -> u32 {
    let args: Vec<u8> = s.args().iter().copied().skip_while(|&c| c == b' ').map(|c| c.to_ascii_uppercase()).collect();
    let hab = unsafe { WinInitialize(0) };
    let hmq = if hab != 0 { unsafe { WinCreateMsgQueue(hab, 0) } } else { 0 };
    if hmq == 0 {
        // No PM (no graphics display): the text-mode command prompt.
        os2::println!("RS2SHELL: Presentation Manager is not available; starting the command prompt");
        return text_shell();
    }
    let groups = objects::ensure_defaults();
    if args.starts_with(b"/MAKEINI") {
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return 0;
    }
    // Only one desktop.
    let mut sem = 0u32;
    if unsafe { os2::DosCreateMutexSem(b"\\SEM32\\RS2SHELL\\DESKTOP\0".as_ptr(), &mut sem, 0, 1) } != 0 {
        os2::println!("RS2SHELL: the desktop is already running");
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(hab);
        }
        return 1;
    }
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    let pid = unsafe { (*pib).pid };
    let mut icons = alloc::vec![0u32];
    for id in 1..=pmwin::shellicons::LAST {
        let h = pmwin::shellicons::icon(id).map_or(0, |(a, b)| make_icon(&a, &b));
        icons.push(h);
    }
    unsafe {
        SHELL = Some(Shell {
            hab,
            pid,
            groups,
            views: Vec::new(),
            icons,
            minimized: Vec::new(),
            drag: None,
            next_key: 1,
            popup: 0,
            popup_on: (0, 0),
            edit: Vec::new(),
            shutdown: None,
        });
        WinRegisterClass(hab, CLASS.as_ptr(), view::client_proc, 0, 4);
    }
    dialogs::register(hab);
    view::open(view::Kind::Desktop);
    apply_minimize_setting();
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        Rs2SetMinimizeViewer(0);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
