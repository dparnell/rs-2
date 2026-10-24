//! Starting programs: program objects, executables and command files,
//! and data files through the associations (by extension).

use alloc::vec::Vec;

use crate::objects;
use crate::pm::*;

/// The System Editor, if installed.
pub fn editor() -> Option<Vec<u8>> {
    let e = objects::sys_path(b"OS2\\E.EXE");
    objects::exists(&e).then_some(e)
}

/// The directory part of a path (with its backslash).
pub fn dir_of(p: &[u8]) -> Vec<u8> {
    match p.iter().rposition(|&c| c == b'\\') {
        Some(i) => p[..=i].to_vec(),
        None => Vec::new(),
    }
}

pub fn file_name(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|&c| c == b'\\') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

fn extension(p: &[u8]) -> Vec<u8> {
    let n = file_name(p);
    match n.iter().rposition(|&c| c == b'.') {
        Some(i) => n[i + 1..].to_ascii_uppercase(),
        None => Vec::new(),
    }
}

/// Change to a directory (drive and path) for the next program.
fn change_dir(dir: &[u8]) {
    let d = dir.strip_suffix(b"\\").filter(|d| d.len() > 2).unwrap_or(dir);
    if d.len() >= 2 && d[1] == b':' {
        let drive = d[0].to_ascii_uppercase();
        if drive.is_ascii_uppercase() {
            unsafe { os2::DosSetDefaultDisk((drive - b'A' + 1) as u32) };
        }
    }
    let mut p = d.to_vec();
    if p.len() == 2 {
        p.push(b'\\');
    }
    let c = z(&p);
    unsafe { os2::DosSetCurrentDir(c.as_ptr()) };
}

/// Start a program in a new session. `exe` "*" (or empty) is the
/// command processor (COMMAND.COM for the DOS types); .CMD files run in
/// one (/K, so the window stays), .BAT files in a DOS session. `progc` is
/// the PROG_* session type (PROG_DEFAULT lets the program's own type
/// decide). Returns the error, 0 if it started.
pub fn start(exe: &[u8], params: &[u8], dir: &[u8], title: &[u8], progc: u32) -> u32 {
    let up = exe.to_ascii_uppercase();
    let dos_batch = up.ends_with(b".BAT");
    let progc = if dos_batch && !matches!(progc, PROG_VDM | PROG_WINDOWEDVDM) { PROG_WINDOWEDVDM } else { progc };
    let batch = up.ends_with(b".CMD");
    let shell = exe.is_empty() || exe == b"*";
    let (name, inputs) = if batch {
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
        _ if batch => 2,
        _ => 0,
    };
    let title = if title.is_empty() {
        if shell {
            (match session_type {
                1 => &b"OS/2 Full Screen"[..],
                4 => b"DOS Full Screen",
                7 => b"DOS Window",
                _ => b"OS/2 Window",
            })
            .to_vec()
        } else {
            file_name(exe).to_vec()
        }
    } else {
        title.to_vec()
    };
    let wd = if !dir.is_empty() {
        dir.to_vec()
    } else if !shell && !batch && exe.contains(&b'\\') {
        dir_of(exe)
    } else if batch && exe.contains(&b'\\') {
        dir_of(exe)
    } else {
        objects::sys_path(b"")
    };
    change_dir(&wd);
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
    let rc = if rc == 457 { 0 } else { rc };
    if rc != 0 {
        let mut text = b"Cannot start ".to_vec();
        text.extend_from_slice(if shell { b"the command processor" } else { exe });
        text.extend_from_slice(alloc::format!(" (error {rc}).").as_bytes());
        crate::view::error(&text);
    }
    rc
}

pub fn start_program(p: &objects::Program) -> u32 {
    start(&p.exe, &p.params, &p.dir, &p.title, p.progc)
}

/// Open a file: run a program, else open it with the program associated
/// with its extension (OS2.INI RS2_Associations), else offer the System
/// Editor.
pub fn open_file(path: &[u8]) {
    let ext = extension(path);
    let dir = dir_of(path);
    match ext.as_slice() {
        b"EXE" | b"COM" | b"CMD" | b"BAT" => {
            start(path, b"", &dir, b"", PROG_DEFAULT);
            return;
        }
        _ => {}
    }
    if let Some(prog) = objects::association(&ext) {
        start(&prog, path, &dir, b"", PROG_DEFAULT);
        return;
    }
    let Some(e) = editor() else {
        let mut t = b"No program is associated with ".to_vec();
        t.extend_from_slice(file_name(path));
        t.push(b'.');
        crate::view::error(&t);
        return;
    };
    let mut t = b"No program is associated with ".to_vec();
    if ext.is_empty() {
        t.extend_from_slice(b"files without an extension");
    } else {
        t.extend_from_slice(b".");
        t.extend_from_slice(&ext);
        t.extend_from_slice(b" files");
    }
    t.extend_from_slice(b". Open it with the System Editor?");
    if message(HWND_DESKTOP, &t, b"Open", MB_YESNO | MB_ICONQUESTION) == MBID_YES {
        start(&e, path, &dir, b"", PROG_PM);
    }
}
