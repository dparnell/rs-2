//! Starting programs for program objects and data files (as RS2SHELL
//! starts them: a session through DosStartSession).

use alloc::vec::Vec;

use crate::util::*;

unsafe extern "C" {
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
}

const HWND_DESKTOP: u32 = 1;

pub const PROG_DEFAULT: u32 = 0;
pub const PROG_FULLSCREEN: u32 = 1;
pub const PROG_WINDOWABLEVIO: u32 = 2;
pub const PROG_PM: u32 = 3;
pub const PROG_VDM: u32 = 4;
pub const PROG_WINDOWEDVDM: u32 = 7;

/// Start `exe` with `params` in `dir` (the command processor for "*" or
/// nothing; .CMD and .BAT files in one). Returns the error, 0 if it
/// started; a program that did not start is reported in a message box,
/// as the Workplace Shell does. A DOS session (PROG_VDM,
/// PROG_WINDOWEDVDM) runs its own COMMAND.COM for "*" and .BAT files.
pub fn start(exe: &[u8], params: &[u8], dir: &[u8], title: &[u8], progc: u32) -> u32 {
    let rc = start_session(exe, params, dir, title, progc);
    if rc != 0 {
        report(exe, rc);
    }
    rc
}

/// "Cannot start the program": the program and OSO001.MSG's text for the
/// error (SYSnnnn without the file).
fn report(exe: &[u8], rc: u32) {
    let mut t = b"Cannot start the program ".to_vec();
    t.extend_from_slice(if exe.is_empty() || exe == b"*" { b"*" } else { exe });
    t.extend_from_slice(b".\n\n");
    let mut buf = [0u8; 512];
    let mut n = 0;
    let file = b"OSO001.MSG\0";
    if unsafe { os2::DosGetMessage(core::ptr::null(), 0, buf.as_mut_ptr(), buf.len() as u32, rc, file.as_ptr(), &mut n) } == 0 {
        t.extend_from_slice(buf[..n as usize].trim_ascii_end());
    } else {
        t.extend_from_slice(alloc::format!("SYS{rc:04}").as_bytes());
    }
    let t = z(&t);
    // MB_OK | MB_ERROR | MB_MOVEABLE
    unsafe { WinMessageBox(HWND_DESKTOP, HWND_DESKTOP, t.as_ptr(), b"Workplace Shell\0".as_ptr(), 0, 0x0000 | 0x0040 | 0x4000) };
}

fn start_session(exe: &[u8], params: &[u8], dir: &[u8], title: &[u8], progc: u32) -> u32 {
    let up = exe.to_ascii_uppercase();
    let dos = progc == PROG_VDM || progc == PROG_WINDOWEDVDM;
    let batch = !dos && (up.ends_with(b".CMD") || up.ends_with(b".BAT"));
    let shell = exe.is_empty() || exe == b"*";
    let (name, inputs): (Option<Vec<u8>>, Vec<u8>) = if batch {
        let mut i = b"/K ".to_vec();
        i.extend_from_slice(exe);
        if !params.is_empty() {
            i.push(b' ');
            i.extend_from_slice(params);
        }
        (None, i)
    } else if shell {
        (None, params.to_vec())
    } else {
        (Some(z(exe)), params.to_vec())
    };
    let session_type = match progc {
        PROG_FULLSCREEN => 1,
        PROG_WINDOWABLEVIO => 2,
        PROG_PM => 3,
        PROG_VDM => 4,
        PROG_WINDOWEDVDM => 7,
        // Win-OS/2: the kernel starts WINOS2.COM with the program.
        13..=20 => progc as u16,
        _ if batch => 2,
        _ => 0,
    };
    let wd = if !dir.is_empty() {
        dir.to_vec()
    } else if exe.contains(&b'\\') {
        split_path(exe).0
    } else {
        Vec::new()
    };
    if !wd.is_empty() {
        if wd.len() >= 2 && wd[1] == b':' && wd[0].is_ascii_alphabetic() {
            unsafe { os2::DosSetDefaultDisk((wd[0].to_ascii_uppercase() - b'A' + 1) as u32) };
        }
        let c = z(&wd);
        unsafe { os2::DosSetCurrentDir(c.as_ptr()) };
    }
    let title = if title.is_empty() { split_path(exe).1 } else { title.to_vec() };
    let (t, i) = (z(&title), z(&inputs));
    let sd = os2::StartData {
        length: core::mem::size_of::<os2::StartData>() as u16,
        related: 0,
        fgbg: 0,
        trace: 0,
        title: t.as_ptr(),
        name: name.as_ref().map_or(core::ptr::null(), |n| n.as_ptr()),
        inputs: i.as_ptr(),
        termq: core::ptr::null(),
        env: core::ptr::null(),
        inherit: 0,
        session_type,
    };
    let (mut sid, mut pid) = (0, 0);
    let rc = unsafe { os2::DosStartSession(&sd, &mut sid, &mut pid) };
    // ERROR_SMG_START_IN_BACKGROUND is not an error.
    if rc == 457 { 0 } else { rc }
}
