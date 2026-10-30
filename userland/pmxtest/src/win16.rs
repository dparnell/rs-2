//! A Windows program started as the Workplace Shell starts one (its own
//! session through DosStartSession) that dies of a General Protection
//! Fault: WIN16 shows Windows' "Application Error" for it instead of the
//! program vanishing (`HELLO16 GPF` writes to its code segment).

use alloc::vec::Vec;

use os2::*;

use crate::check;
use crate::pm::*;

const WM_COMMAND: u32 = 0x20;

fn text(h: u32) -> Vec<u8> {
    let mut b = [0u8; 160];
    let n = unsafe { WinQueryWindowText(h, 160, b.as_mut_ptr()) };
    b[..n.max(0) as usize].to_vec()
}

fn contains(s: &[u8], pat: &[u8]) -> bool {
    s.windows(pat.len()).any(|w| w == pat)
}

/// The desktop's windows titled `title`, and the text of their children.
fn find(title: &[u8]) -> Option<(u32, Vec<u8>)> {
    let e = unsafe { WinBeginEnumWindows(HWND_DESKTOP) };
    let mut found = None;
    loop {
        let h = unsafe { WinGetNextWindow(e) };
        if h == 0 {
            break;
        }
        if text(h) != title {
            continue;
        }
        let mut all = Vec::new();
        let c = unsafe { WinBeginEnumWindows(h) };
        loop {
            let k = unsafe { WinGetNextWindow(c) };
            if k == 0 {
                break;
            }
            all.extend_from_slice(&text(k));
            all.push(b'|');
        }
        unsafe { WinEndEnumWindows(c) };
        found = Some((h, all));
        break;
    }
    unsafe { WinEndEnumWindows(e) };
    found
}

/// The box's text lines (`find`'s `|`-separated control texts, in either
/// order: the message box breaks long lines) joined by blanks contain
/// `what`.
fn joined_contains(body: &[u8], what: &[u8]) -> bool {
    let mut parts: Vec<&[u8]> = body.split(|&c| c == b'|').filter(|p| !p.is_empty()).collect();
    let join = |ps: &[&[u8]]| ps.join(&b' ');
    if contains(&join(&parts), what) {
        return true;
    }
    parts.reverse();
    contains(&join(&parts), what)
}

pub fn win16_checks(hab: u32) {
    let mut info = [0u8; 64];
    if unsafe { DosQueryPathInfo(b"C:\\RS2\\HELLO16.EXE\0".as_ptr(), 1, info.as_mut_ptr(), 64) } != 0 {
        println!("SKIP Win16 Application Error (no C:\\RS2\\HELLO16.EXE)");
        return;
    }
    let sd = StartData {
        length: core::mem::size_of::<StartData>() as u16,
        related: 0,
        fgbg: 0,
        trace: 0,
        title: core::ptr::null(),
        name: b"C:\\RS2\\HELLO16.EXE\0".as_ptr(),
        inputs: b"GPF\0".as_ptr(),
        termq: core::ptr::null(),
        env: core::ptr::null(),
        inherit: 0,
        session_type: 0,
    };
    let (mut sid, mut pid) = (0, 0);
    let rc = unsafe { DosStartSession(&sd, &mut sid, &mut pid) };
    check("DosStartSession of a Windows program (HELLO16 GPF), as the shell starts it", rc == 0 || rc == 457);
    let mut boxed = None;
    for _ in 0..1000 {
        crate::pump(hab);
        boxed = find(b"Application Error");
        if boxed.is_some() {
            break;
        }
        unsafe { DosSleep(10) };
    }
    let (h, body) = boxed.unwrap_or_default();
    check(
        "WIN16's Application Error for a General Protection Fault",
        h != 0 && joined_contains(&body, b"HELLO16 caused a General Protection Fault in module HELLO16.EXE at 0001:"),
    );
    if h == 0 {
        return;
    }
    // OK ends the program: the box goes, and its window never appears.
    unsafe { WinPostMsg(h, WM_COMMAND, 1, 1) };
    let mut gone = false;
    for _ in 0..500 {
        crate::pump(hab);
        if find(b"Application Error").is_none() {
            gone = true;
            break;
        }
        unsafe { DosSleep(10) };
    }
    crate::pump_for(hab, 300);
    check("OK closes it and the program ends", gone && find(b"Hello Win16").is_none());
}
