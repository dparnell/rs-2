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
//! folders, the Minimized Window Viewer, Window List, System Setup with
//! System, System Clock and Display, Shut Down, the Shredder, Connections
//! with Network and Printers, the programs rs2 has) unless OS2.INI says the desktop was
//! made (`PM_Workplace` `Rs2Desktop`).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::util::*;
use crate::{clsmgr, obj, wps};

const APP_OBJECTS: &[u8] = b"PM_InstallObject";
const APP_CLASSES: &[u8] = b"PM_InstallClass";
/// `PM_Workplace` `Rs2Desktop`: the version of rs2's desktop made.
const DESKTOP_VERSION: &[u8] = b"9";

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
        (b"Minimized^Window Viewer;WPMinWinViewer;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_VIEWER>"),
        (b"Window List;RS2WindowList;<WP_DESKTOP>;PRESERVEOLD", b"NOCOPY=YES;OBJECTID=<RS2_WINLIST>"),
        (b"System Setup;WPFolder;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_CONFIG>"),
        // rs2's earlier System object gives way to IBM's class.
        (b"System;RS2System;<WP_CONFIG>;DELETE", b"OBJECTID=<RS2_SYSTEM>"),
        (b"System;WPSystem;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_SYSTEM>"),
        (b"System Clock;WPClock;<WP_CONFIG>;PRESERVEOLD", b"OBJECTID=<WP_CLOCK>"),
        (b"Keyboard;WPKeyboard;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_KEYB>"),
        (b"Mouse;WPMouse;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_MOUSE>"),
        (b"Sound;WPSound;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_SOUND>"),
        (b"Color Palette;WPColorPalette;<WP_CONFIG>;PRESERVEOLD", b"OBJECTID=<WP_HIRESCLRPAL>"),
        (b"Scheme Palette;WPSchemePalette;<WP_CONFIG>;PRESERVEOLD", b"OBJECTID=<WP_SCHPAL96>"),
        (b"Font Palette;WPFontPalette;<WP_CONFIG>;PRESERVEOLD", b"OBJECTID=<WP_FNTPAL>"),
        (b"Shut Down;RS2Shutdown;<WP_DESKTOP>;PRESERVEOLD", b"NOCOPY=YES;OBJECTID=<RS2_SHUTDOWN>"),
        (b"Shredder;WPShredder;<WP_DESKTOP>;PRESERVEOLD", b"NOCOPY=YES;OBJECTID=<WP_SHRED>;LOCKEDINPLACE=YES"),
        // Connections, as IBM's INI.RC has it (Drives stays on the
        // desktop): the Network folder and the Printers folder with a
        // printer template.
        (b"Connections;WPFolder;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_CONNECTIONSFOLDER>"),
        (b"Network;WPNetwork;<WP_CONNECTIONSFOLDER>;PRESERVEOLD", b"NODELETE=YES;DEFAULTVIEW=ICON;OBJECTID=<WP_NETWORK>"),
        (b"Printers;WPFolder;<WP_CONNECTIONSFOLDER>;PRESERVEOLD", b"DEFAULTVIEW=ICON;OBJECTID=<WP_PRINTERSFOLDER>"),
        (b"Printer;WPPrinter;<WP_PRINTERSFOLDER>;PRESERVEOLD", b"TEMPLATE=YES;OBJECTID=<WP_NEWPRINTER>"),
        (b"Templates;WPTemplates;<WP_DESKTOP>;PRESERVEOLD", b"NODELETE=YES;OBJECTID=<WP_TEMPS>"),
        (b"Data File;WPDataFile;<WP_TEMPS>;PRESERVEOLD", b"TEMPLATE=YES;OBJECTID=<RS2_TEMPDATAFILE>"),
        (b"Folder;WPFolder;<WP_TEMPS>;PRESERVEOLD", b"TEMPLATE=YES;OBJECTID=<RS2_TEMPFOLDER>"),
        (b"Program;WPProgram;<WP_TEMPS>;PRESERVEOLD", b"TEMPLATE=YES;OBJECTID=<RS2_TEMPPROGRAM>"),
        // The Assistance Center, as IBM's INI.RC lays it out (the books
        // APPINST installs go in its folders).
        (b"Assistance Center;WPFolder;<WP_DESKTOP>;PRESERVEOLD", b"OBJECTID=<WP_ASSISTANCE>"),
        (b"Information;WPFolder;<WP_ASSISTANCE>;PRESERVEOLD", b"DEFAULTVIEW=TREE;OBJECTID=<WP_INFO>;SHOWALLINTREEVIEW=YES"),
        (b"Reference and Commands;WPFolder;<WP_INFO>;PRESERVEOLD", b"DEFAULTVIEW=ICON;OBJECTID=<WP_REFCMDFOLDER>;SHOWALLINTREEVIEW=YES"),
        (b"Tasks;WPFolder;<WP_INFO>;PRESERVEOLD", b"DEFAULTVIEW=ICON;OBJECTID=<WP_TASKSINFO>;SHOWALLINTREEVIEW=YES"),
        (b"Read Me;WPFolder;<WP_INFO>;PRESERVEOLD", b"DEFAULTVIEW=ICON;OBJECTID=<WP_READMEFOLDER>;SHOWALLINTREEVIEW=YES"),
        (b"ReadMe;WPShadow;<WP_READMEFOLDER>;PRESERVEOLD", b"SHADOWID=?:\\README.TXT;OBJECTID=<WP_RDME>"),
        (b"Troubleshooting;WPFolder;<WP_ASSISTANCE>;PRESERVEOLD", b"DEFAULTVIEW=ICON;OBJECTID=<WP_TROUBLEINFO>"),
        (b"Help Index;Mindex;<WP_INFO>;PRESERVEOLD", b"INDEX=HELP;NODELETE=YES;OBJECTID=<WP_MINDEX>"),
        (b"Online Information Overview;WPFolder;<WP_INFO>;PRESERVEOLD", b"DEFAULTVIEW=ICON;OBJECTID=<WP_INFOOVERFOLDER>;SHOWALLINTREEVIEW=YES"),
        (b"Glossary;Mindex;<WP_INFOOVERFOLDER>;PRESERVEOLD", b"INDEX=GLOSSARY;NODELETE=YES;OBJECTID=<WP_GLOSS>"),
        (b"rs2 on^the Internet;WPUrl;<WP_ASSISTANCE>;PRESERVEOLD", b"URL=https://github.com/dparnell/rs-2;OBJECTID=<RS2_URL>"),
        (b"OS/2 Window;WPProgram;<WP_PROMPTS>;PRESERVEOLD", b"EXENAME=*;PROGTYPE=WINDOWABLEVIO;OBJECTID=<WP_OS2WIN>"),
        (b"OS/2 Full Screen;WPProgram;<WP_PROMPTS>;PRESERVEOLD", b"EXENAME=*;PROGTYPE=FULLSCREEN;OBJECTID=<WP_OS2FS>"),
    ];
    // (file, title, object ID, folder, more setup)
    let programs: [(&[u8], &[u8], &[u8], &[u8], &[u8]); 4] = [
        (b"OS2\\E.EXE", b"System Editor", b"<WP_SYSED>", b"<WP_PROGRAMSFOLDER>", b"ASSOCFILTER=*.TXT,*.DOC,*.LOG,*.ME,*.CFG,*.SYS,*.INI,*.CMD;"),
        (b"OS2\\APPINST.EXE", b"Install OS/2 Applications", b"<RS2_APPINST>", b"<WP_PROGRAMSFOLDER>", b""),
        (b"OS2\\DISPLAY.EXE", b"Display", b"<RS2_DISPLAY>", b"<WP_CONFIG>", b""),
        (b"OS2\\PMINSTAL.EXE", b"Install rs2", b"<RS2_INSTALL>", b"<WP_PROGRAMSFOLDER>", b""),
    ];
    let mut out: Vec<(Vec<u8>, Vec<u8>)> = v.drain(..).map(|(k, s)| (k.to_vec(), s.to_vec())).collect();
    for (file, title, id, folder, extra) in programs {
        let path = [&b"?:\\"[..], file].concat();
        if !file_exists(&boot(&path)) {
            continue;
        }
        let mut k = title.to_vec();
        k.extend_from_slice(b";WPProgram;");
        k.extend_from_slice(folder);
        k.extend_from_slice(b";PRESERVEOLD");
        let mut s = b"EXENAME=".to_vec();
        s.extend_from_slice(&path);
        s.extend_from_slice(b";PROGTYPE=PM;");
        s.extend_from_slice(extra);
        s.extend_from_slice(b"OBJECTID=");
        s.extend_from_slice(id);
        out.push((k, s));
    }
    // The LaunchPad, after the objects it shows (open at each start: its
    // shadow is in Startup).
    out.push((b"LaunchPad;WPLaunchPad;<WP_DESKTOP>;PRESERVEOLD".to_vec(), b"FPOBJECTS=<WP_DRIVES>,<WP_OS2WIN>,<WP_SYSED>,<WP_ASSISTANCE>;DRAWEROBJECTS=2,<WP_OS2FS>;OBJECTID=<WP_LAUNCHPAD>".to_vec()));
    out.push((b"LaunchPad;WPShadow;<WP_START>;PRESERVEOLD".to_vec(), b"SHADOWID=<WP_LAUNCHPAD>;OBJECTID=<RS2_LAUNCHPADSTART>".to_vec()));
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
            // Even an object made non-deleteable goes.
            if let Some(o) = existing {
                let s = obj::call(m::WPObject::wpQueryStyle, &[o]) & !(obj::OBJSTYLE_NODELETE as usize);
                obj::call(m::WPObject::wpSetStyle, &[o, s]);
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
    // INI.RC's titles break lines with ^.
    let title: Vec<u8> = boot(title).into_iter().map(|c| if c == b'^' { b'\n' } else { c }).collect();
    crate::api::create_object(class, &title, &setup, &loc, 1) != 0 || {
        debug(&alloc::format!("install: {:?} was not made", core::str::from_utf8(key)));
        true
    }
}

/// Make what OS2.INI asks for (and, the first time, rs2's desktop).
pub fn run() {
    // The installer belongs to the live media: an installed system (which
    // has no PMINSTAL.EXE) drops the object its OS2.INI came with.
    if !file_exists(&boot(b"?:\\OS2\\PMINSTAL.EXE")) {
        if let Some(o) = wps::handle_of_id(b"<RS2_INSTALL>").map(wps::object_from_handle).filter(|&o| o != 0) {
            obj::call(m::WPObject::wpFree, &[o]);
        }
    }
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
    // Version 2 added the Minimized Window Viewer, Window List, System
    // Setup (System, Display) and Shut Down, version 3 Templates,
    // version 4 the Assistance Center, version 5 its Help Index,
    // Glossary and Internet object, version 6 the LaunchPad, version 9
    // the Shredder, Connections (Network, Printers), the System Clock and
    // IBM's System class in place of RS2System;
    // PRESERVEOLD leaves what an earlier version made.
    if prf_read(HINI_USER, b"PM_Workplace", b"Rs2Desktop").is_none_or(|v| v.strip_suffix(&[0]).unwrap_or(&v) != DESKTOP_VERSION) {
        pending.extend(defaults());
        let mut v = DESKTOP_VERSION.to_vec();
        v.push(0);
        prf_write(HINI_USER, b"PM_Workplace", b"Rs2Desktop", Some(&v));
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
