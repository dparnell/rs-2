//! rs2's own icons for the Workplace Shell's objects (pmwin::shellicons,
//! the ones RS2SHELL shows): the Drives folder, drives (hard disk or
//! CD-ROM), the desktop, the Command Prompts, Assistance Center and
//! Information folders and command prompts.
//! IBM's shell takes its class icons from PMWP.DLL's resources and sets
//! the Command Prompts folder's with INI.RC's ICONRESOURCE; rs2's PMWP has
//! no resources, so the icons are drawn here and the folders that INI.RC
//! would give an icon are known by their object IDs.

use pmwin::shellicons as si;

pub use si::{ASSISTANCE, CD_ROM, CLOCK, COLOR_PALETTE, CONNECTIONS, DESKTOP, FONT_PALETTE, DRIVES, FIND, INDEX, INFO_FOLDER, INTERNET, KEYBOARD, LAUNCHPAD, MOUSE, NETWORK, PRINTER, PRINTERS, SCHEME_PALETTE, SHREDDER, SOUND, SYSTEM, HARD_DISK, MINIMIZED, PROMPTS, PROMPT_FULL_SCREEN, PROMPT_WINDOW, SHUTDOWN, WINDOW_LIST};

static mut CACHE: [u32; si::LAST as usize + 1] = [0; si::LAST as usize + 1];

/// One of the shell icons (made once, kept for the process).
pub fn shell(id: u32) -> u32 {
    if id == 0 || id > si::LAST {
        return 0;
    }
    let c = unsafe { &mut *(&raw mut CACHE) };
    if c[id as usize] == 0 {
        c[id as usize] = si::icon(id).map_or(0, |(big, mini)| crate::pm::make_icon(&big, &mini));
    }
    c[id as usize]
}

/// The icon a folder with this object ID has without one of its own.
pub fn of_object_id(id: &[u8]) -> u32 {
    match id {
        b"<WP_PROMPTS>" => shell(PROMPTS),
        b"<WP_DRIVES>" => shell(DRIVES),
        b"<WP_VIEWER>" => shell(MINIMIZED),
        b"<WP_ASSISTANCE>" => shell(ASSISTANCE),
        b"<WP_INFO>" => shell(INFO_FOLDER),
        b"<WP_CONNECTIONSFOLDER>" => shell(CONNECTIONS),
        b"<WP_PRINTERSFOLDER>" => shell(PRINTERS),
        _ => 0,
    }
}

/// Whether drive `d` (1 = A:) holds a CD file system.
pub fn is_cd(d: u32) -> bool {
    if d == 0 || d > 26 {
        return false;
    }
    let dev = [b'A' + d as u8 - 1, b':', 0];
    let mut buf = [0u8; 64];
    let mut cb = buf.len() as u32;
    if unsafe { os2::DosQueryFSAttach(dev.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } != 0 {
        return false;
    }
    // FSQBUFFER2: type, cbName, cbFSDName, cbFSAData, the name and its
    // NUL, then the file system's name.
    let cb_name = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    let at = 8 + cb_name + 1;
    let fs = buf.get(at..).map_or(&[][..], |b| &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
    [&b"CDFS"[..], b"ISO9660", b"UDF"].iter().any(|n| fs.eq_ignore_ascii_case(n))
}
