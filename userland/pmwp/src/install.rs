//! Making the desktop's objects, as IBM's shell does on its first start:
//! MAKEINI puts INI.RC's `PM_InstallClass` (class and DLL) and
//! `PM_InstallObject` entries in OS2.INI, and the shell makes them and
//! removes the entries. A `PM_InstallObject` key is
//! `title;class;location;flags`, its value the setup string; `?:` is
//! the boot drive; the flags say what to do when an object with the
//! entry's OBJECTID exists: PRESERVEOLD (leave it, the default), UPDATE
//! (set it up again), REPLACE (make it anew), UPDATEONLY (only set up an
//! existing one), DELETE (remove it).
//!
//! rs2 has no INI.RC from IBM: the first start writes rs2's own entries
//! (the desktop, drives, the Startup, Programs and Command Prompts
//! folders, the programs rs2 has) unless OS2.INI says the desktop was
//! made (`PM_Workplace` `Rs2Desktop`).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::util::*;
use crate::{clsmgr, obj, wps};

const APP_OBJECTS: &[u8] = b"PM_InstallObject";
const APP_CLASSES: &[u8] = b"PM_InstallClass";

pub fn boot_drive() -> u8 {
    let mut d = 3u32;
    unsafe { os2::DosQuerySysInfo(5, 5, &mut d, 4) };
    b'A' + d as u8 - 1
}

/// `?:` is the boot drive.
fn boot(s: &[u8]) -> Vec<u8> {
    let d = boot_drive();
    let mut out = Vec::with_capacity(s.len());
    for (i, &c) in s.iter().enumerate() {
        if c == b'?' && s.get(i + 1) == Some(&b':') {
            out.push(d);
        } else {
            out.push(c);
        }
    }
    out
}

/// rs2's desktop (its INI.RC): (title;class;location;flags, setup);
/// programs only where their files are.
fn defaults() -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut v: Vec<(&[u8], &[u8])> = alloc::vec![
        (b"Desktop;WPDesktop;?:\\;PRESERVEOLD", b"OBJECTID=<WP_DESKTOP>"),
        (b"Drives;WPDrives;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_DRIVES>"),
        (b"Command Prompts;WPFolder;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_PROMPTS>"),
        (b"Programs;WPFolder;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_PROGRAMSFOLDER>"),
        (b"Startup;WPStartup;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_START>"),
        (b"OS/2 Window;WPProgram;<WP_PROMPTS>;PRESERVEOLD", b"EXENAME=*;PROGTYPE=WINDOWABLEVIO;OBJECTID=<WP_OS2WIN>"),
        (b"OS/2 Full Screen;WPProgram;<WP_PROMPTS>;PRESERVEOLD", b"EXENAME=*;PROGTYPE=FULLSCREEN;OBJECTID=<WP_OS2FS>"),
    ];
    let programs: [(&[u8], &[u8], &[u8], &[u8]); 4] = [
        (b"OS2\\E.EXE", b"System Editor", b"<WP_SYSED>", b"ASSOCFILTER=*.TXT,*.DOC,*.LOG,*.ME,*.CFG,*.SYS,*.INI,*.CMD;"),
        (b"OS2\\APPINST.EXE", b"Install OS/2 Applications", b"<RS2_APPINST>", b""),
        (b"OS2\\DISPLAY.EXE", b"Display", b"<RS2_DISPLAY>", b""),
        (b"OS2\\PMINSTAL.EXE", b"Install rs2", b"<RS2_INSTALL>", b""),
    ];
    let mut out: Vec<(Vec<u8>, Vec<u8>)> = v.drain(..).map(|(k, s)| (k.to_vec(), s.to_vec())).collect();
    for (file, title, id, extra) in programs {
        let path = [&b"?:\\"[..], file].concat();
        if !file_exists(&boot(&path)) {
            continue;
        }
        let mut k = title.to_vec();
        k.extend_from_slice(b";WPProgram;<WP_PROGRAMSFOLDER>;PRESERVEOLD");
        let mut s = b"EXENAME=".to_vec();
        s.extend_from_slice(&path);
        s.extend_from_slice(b";PROGTYPE=PM;");
        s.extend_from_slice(extra);
        s.extend_from_slice(b"OBJECTID=");
        s.extend_from_slice(id);
        out.push((k, s));
    }
    out
}

/// One entry: done (true), or its location is not there yet (false).
fn install_object(key: &[u8], setup: &[u8]) -> bool {
    let parts: Vec<&[u8]> = key.split(|&c| c == b';').collect();
    let (title, class, location) = (parts.first().copied().unwrap_or(b""), parts.get(1).copied().unwrap_or(b""), parts.get(2).copied().unwrap_or(b""));
    let flags = parts.get(3).map(|f| f.to_ascii_uppercase()).unwrap_or_default();
    let setup = boot(setup);
    let id = obj::setup_pairs(&setup).into_iter().find(|(k, _)| k == b"OBJECTID").map(|(_, v)| v);
    let existing = id.as_ref().and_then(|i| wps::handle_of_id(i)).map(wps::object_from_handle).filter(|&o| o != 0);
    match flags.as_slice() {
        b"DELETE" => {
            if let Some(o) = existing {
                obj::call(m::WPObject::wpFree, &[o]);
            }
            return true;
        }
        b"UPDATEONLY" | b"UPDATE" if existing.is_some() => {
            let o = existing.unwrap();
            let s = z(&setup);
            obj::call(m::WPObject::wpSetup, &[o, s.as_ptr() as usize]);
            obj::call(m::WPObject::wpSaveImmediate, &[o]);
            return true;
        }
        b"UPDATEONLY" => return true,
        b"REPLACE" if existing.is_some() => {
            obj::call(m::WPObject::wpFree, &[existing.unwrap()]);
        }
        _ if existing.is_some() => return true,
        _ => {}
    }
    let loc = boot(location);
    let folder = wps::object_from_location(&loc);
    if folder == 0 {
        return false;
    }
    crate::api::create_object(class, &boot(title), &setup, &loc, 1) != 0 || {
        debug(&alloc::format!("install: {:?} was not made", core::str::from_utf8(key)));
        true
    }
}

/// Make what OS2.INI asks for (and, the first time, rs2's desktop).
pub fn run() {
    for k in prf_keys(HINI_USER, APP_CLASSES) {
        if let Some(v) = prf_read(HINI_USER, APP_CLASSES, &k) {
            let dll = v.strip_suffix(&[0]).unwrap_or(&v).to_vec();
            let mut t = clsmgr::table();
            if !t.iter().any(|(c, _)| eq_nocase(c, &k)) {
                t.push((k.clone(), dll));
                clsmgr::set_table(&t);
            }
        }
        prf_write(HINI_USER, APP_CLASSES, &k, None);
    }
    let mut pending: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if prf_read(HINI_USER, b"PM_Workplace", b"Rs2Desktop").is_none() {
        pending.extend(defaults());
        prf_write(HINI_USER, b"PM_Workplace", b"Rs2Desktop", Some(b"1\0"));
    }
    for k in prf_keys(HINI_USER, APP_OBJECTS) {
        let v = prf_read(HINI_USER, APP_OBJECTS, &k).unwrap_or_default();
        pending.push((k.clone(), v.strip_suffix(&[0]).unwrap_or(&v).to_vec()));
        prf_write(HINI_USER, APP_OBJECTS, &k, None);
    }
    // Objects whose folders come later wait for them.
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|(k, s)| !install_object(k, s));
        if pending.len() == before {
            for (k, _) in &pending {
                debug(&alloc::format!("install: no location for {:?}", core::str::from_utf8(k)));
            }
            break;
        }
    }
}
