//! What the shell keeps in OS2.INI: the program list (groups and program
//! objects, through PMSHAPI's Prf* program list calls so other programs
//! see them), and its own applications:
//!
//! - `RS2_Desktop`: `Version` (written when the default objects have been
//!   made), `DesktopGroup` (handle of the hidden group whose programs sit
//!   on the desktop), `Background` (desktop colour, `RRGGBB` hex),
//!   `PromptsGroup`, `ProgramsGroup`, `StartupGroup` (the Startup folder,
//!   whose objects start once after each boot), `NextShadow`.
//! - `RS2_Shadows`: shadows (see shadows.rs).
//! - `RS2_IconPos`: `FOLDER|OBJECT` -> `x y` (icon positions).
//! - `RS2_Windows`: `FOLDER` -> `x y cx cy view` (folder windows).
//! - `RS2_Associations`: extension (upper case, no dot) -> program.

use alloc::vec::Vec;

use crate::pm::*;

pub const APP_DESKTOP: &[u8] = b"RS2_Desktop\0";
pub const APP_POS: &[u8] = b"RS2_IconPos\0";
pub const APP_WINDOWS: &[u8] = b"RS2_Windows\0";
pub const APP_ASSOC: &[u8] = b"RS2_Associations\0";

/// Current layout of the default objects.
pub const VERSION: &[u8] = b"5";

pub fn query(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let k = z(key);
    let mut size = 0u32;
    if unsafe { PrfQueryProfileSize(HINI_USERPROFILE, app.as_ptr(), k.as_ptr(), &mut size) } == 0 {
        return None;
    }
    let mut buf = alloc::vec![0u8; size as usize + 1];
    let mut n = size;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, app.as_ptr(), k.as_ptr(), buf.as_mut_ptr(), &mut n) } == 0 {
        return None;
    }
    buf.truncate(n as usize);
    // Strings are stored with their NUL.
    while buf.last() == Some(&0) {
        buf.pop();
    }
    Some(buf)
}

/// The keys of an application.
pub fn keys(app: &[u8]) -> Vec<Vec<u8>> {
    let mut size = 0u32;
    if unsafe { PrfQueryProfileSize(HINI_USERPROFILE, app.as_ptr(), core::ptr::null(), &mut size) } == 0 || size == 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; size as usize + 1];
    let mut n = size;
    if unsafe { PrfQueryProfileData(HINI_USERPROFILE, app.as_ptr(), core::ptr::null(), buf.as_mut_ptr(), &mut n) } == 0 {
        return Vec::new();
    }
    buf.truncate(n as usize);
    buf.split(|&c| c == 0).filter(|k| !k.is_empty()).map(|k| k.to_vec()).collect()
}

pub fn write(app: &[u8], key: &[u8], value: Option<&[u8]>) -> bool {
    let k = z(key);
    match value {
        Some(v) => {
            let v = z(v);
            unsafe { PrfWriteProfileData(HINI_USERPROFILE, app.as_ptr(), k.as_ptr(), v.as_ptr(), v.len() as u32) != 0 }
        }
        None => unsafe { PrfWriteProfileData(HINI_USERPROFILE, app.as_ptr(), k.as_ptr(), core::ptr::null(), 0) != 0 },
    }
}

pub fn query_u32(app: &[u8], key: &[u8]) -> Option<u32> {
    parse_hex(&query(app, key)?)
}

pub fn parse_hex(t: &[u8]) -> Option<u32> {
    if t.is_empty() || t.len() > 8 {
        return None;
    }
    let mut v = 0u32;
    for &c in t {
        v = v << 4 | (c as char).to_digit(16)?;
    }
    Some(v)
}

pub fn hex(v: u32) -> Vec<u8> {
    alloc::format!("{v:08X}").into_bytes()
}

/// Two decimal numbers (possibly negative) and more: "x y" or "x y cx cy v".
pub fn numbers(t: &[u8]) -> Vec<i32> {
    t.split(|&c| c == b' ').filter(|s| !s.is_empty()).filter_map(|s| core::str::from_utf8(s).ok()?.parse().ok()).collect()
}

// ---- Icon positions ----

pub fn icon_pos(folder: &[u8], obj: &[u8]) -> Option<(i32, i32)> {
    let mut k = folder.to_vec();
    k.push(b'|');
    k.extend_from_slice(obj);
    let v = numbers(&query(APP_POS, &k)?);
    (v.len() >= 2).then(|| (v[0], v[1]))
}

pub fn set_icon_pos(folder: &[u8], obj: &[u8], p: Option<(i32, i32)>) {
    let mut k = folder.to_vec();
    k.push(b'|');
    k.extend_from_slice(obj);
    match p {
        Some((x, y)) => write(APP_POS, &k, Some(alloc::format!("{x} {y}").as_bytes())),
        None => write(APP_POS, &k, None),
    };
}

// ---- The program list ----

/// A program object (PROGDETAILS without the SWP).
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Program {
    pub progc: u32,
    pub visible: u32,
    pub title: Vec<u8>,
    pub exe: Vec<u8>,
    pub params: Vec<u8>,
    pub dir: Vec<u8>,
    pub icon: Vec<u8>,
    /// Environment strings (`NAME=value`; for DOS programs their DOS
    /// settings, as OS/2 keeps them), each NUL-terminated.
    pub env: Vec<u8>,
}

/// A NUL-separated string list ending with an empty string.
fn cstr_list(p: *const u8) -> Vec<u8> {
    let mut v = Vec::new();
    if p.is_null() {
        return v;
    }
    let mut q = p;
    unsafe {
        loop {
            if *q == 0 {
                break;
            }
            while *q != 0 {
                v.push(*q);
                q = q.add(1);
            }
            v.push(0);
            q = q.add(1);
        }
    }
    v
}

/// The entries of a group (SGH_ROOT: the groups) as (handle, progc,
/// visibility, title).
pub fn titles(group: u32) -> Vec<(u32, u32, u32, Vec<u8>)> {
    let mut count = 0u32;
    let need = unsafe { PrfQueryProgramTitles(HINI_USERPROFILE, group, core::ptr::null_mut(), 0, &mut count) };
    if need == 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; need as usize + 16];
    let got = unsafe { PrfQueryProgramTitles(HINI_USERPROFILE, group, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    if got == 0 {
        return Vec::new();
    }
    let p = buf.as_ptr() as u32;
    (0..count as usize)
        .map(|i| {
            let at = i * 16;
            (rd(p, at), rd(p, at + 4), rd(p, at + 8), cstr(rd(p, at + 12) as *const u8))
        })
        .collect()
}

pub fn definition(h: u32) -> Option<Program> {
    let need = unsafe { PrfQueryDefinition(HINI_USERPROFILE, h, core::ptr::null_mut(), 0) };
    if need < 72 {
        return None;
    }
    let mut buf = alloc::vec![0u8; need as usize + 16];
    if unsafe { PrfQueryDefinition(HINI_USERPROFILE, h, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        return None;
    }
    let p = buf.as_ptr() as u32;
    let s = |off: usize| cstr(rd(p, off) as *const u8);
    let env = cstr_list(rd(p, 32) as *const u8);
    Some(Program { progc: rd(p, 4), visible: rd(p, 8), title: s(12), exe: s(16), params: s(20), dir: s(24), icon: s(28), env })
}

/// PROGDETAILS for a program (the strings must outlive the call).
fn details(p: &Program, strs: &[Vec<u8>; 6]) -> [u32; 18] {
    let mut d = [0u32; 18];
    d[0] = 72;
    d[1] = p.progc;
    d[2] = p.visible;
    for (i, s) in strs.iter().enumerate() {
        d[3 + i] = if s.len() > 1 { s.as_ptr() as u32 } else { 0 };
    }
    // Title must be there even when empty.
    d[3] = strs[0].as_ptr() as u32;
    d
}

fn strings(p: &Program) -> [Vec<u8>; 6] {
    let mut env = p.env.clone();
    env.extend_from_slice(&[0, 0]);
    [z(&p.title), z(&p.exe), z(&p.params), z(&p.dir), z(&p.icon), env]
}

pub fn add_program(group: u32, p: &Program) -> u32 {
    let s = strings(p);
    let d = details(p, &s);
    unsafe { PrfAddProgram(HINI_USERPROFILE, d.as_ptr() as *const u8, group) }
}

pub fn change_program(h: u32, p: &Program) -> bool {
    let s = strings(p);
    let d = details(p, &s);
    unsafe { PrfChangeProgram(HINI_USERPROFILE, h, d.as_ptr() as *const u8) != 0 }
}

pub fn remove_program(h: u32) -> bool {
    unsafe { PrfRemoveProgram(HINI_USERPROFILE, h) != 0 }
}

pub fn create_group(title: &[u8], visibility: u32) -> u32 {
    let t = z(title);
    unsafe { PrfCreateGroup(HINI_USERPROFILE, t.as_ptr(), visibility) }
}

pub fn destroy_group(h: u32) -> bool {
    unsafe { PrfDestroyGroup(HINI_USERPROFILE, h) != 0 }
}

/// Whether a group handle is still a group.
pub fn group_exists(h: u32) -> bool {
    titles(SGH_ROOT).iter().any(|g| g.0 == h)
}

// ---- Associations ----

/// The program that opens files with this extension.
pub fn association(ext: &[u8]) -> Option<Vec<u8>> {
    if ext.is_empty() {
        return None;
    }
    let e: Vec<u8> = ext.iter().map(|c| c.to_ascii_uppercase()).collect();
    query(APP_ASSOC, &e).filter(|v| !v.is_empty())
}

// ---- Files ----

pub fn exists(path: &[u8]) -> bool {
    let p = z(path);
    let mut st = os2::FileStatus3::default();
    unsafe { os2::DosQueryPathInfo(p.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) == 0 }
}

/// The boot drive letter (from the shell's own path), C by default.
pub fn boot_drive() -> u8 {
    let mut v = [0u32; 1];
    // QSV_BOOT_DRIVE
    if unsafe { os2::DosQuerySysInfo(5, 5, v.as_mut_ptr(), 4) } == 0 && (1..=26).contains(&v[0]) {
        return b'A' + v[0] as u8 - 1;
    }
    b'C'
}

pub fn sys_path(rest: &[u8]) -> Vec<u8> {
    let mut p = alloc::vec![boot_drive(), b':', b'\\'];
    p.extend_from_slice(rest);
    p
}

// ---- The default objects (first start) ----

/// Handles of the shell's groups, made on first start.
pub struct Groups {
    pub desktop: u32,
    pub prompts: u32,
    pub programs: u32,
    pub startup: u32,
}

/// The groups, creating the default objects if this profile has none
/// yet (the shell's MAKEINI step: rs2's own content).
pub fn ensure_defaults() -> Groups {
    let made = query(APP_DESKTOP, b"Version").is_some();
    let get = |k: &[u8]| query_u32(APP_DESKTOP, k).filter(|&h| group_exists(h));
    if made {
        if let (Some(desktop), Some(prompts), Some(programs)) = (get(b"DesktopGroup"), get(b"PromptsGroup"), get(b"ProgramsGroup")) {
            // A profile made by an older shell (version 1: before rs2 had
            // its own System Editor; 2: before the application installer)
            // gets the editor's object and associations and the
            // installer's object once.
            // ... and 3, before the Startup folder: the folder.
            let startup = match get(b"StartupGroup") {
                Some(s) => s,
                None => make_startup(),
            };
            drop_missing_installer(programs);
            if query(APP_DESKTOP, b"Version").as_deref() != Some(VERSION) {
                add_editor(programs);
                add_app_installer(programs);
                add_display(programs);
                write(APP_DESKTOP, b"Version", Some(VERSION));
            }
            return Groups { desktop, prompts, programs, startup };
        }
    }
    make_defaults()
}

/// The Startup folder (a visible program group).
fn make_startup() -> u32 {
    let g = create_group(b"Startup", SHE_VISIBLE);
    write(APP_DESKTOP, b"StartupGroup", Some(&hex(g)));
    g
}

/// The System Editor's program object (unless there is one) and its
/// associations (where a type has none), when E.EXE is installed.
fn add_editor(programs: u32) {
    let editor = sys_path(b"OS2\\E.EXE");
    if !exists(&editor) {
        return;
    }
    if !titles(programs).iter().any(|t| t.3 == b"System Editor") {
        let p = Program { progc: PROG_PM, title: b"System Editor".to_vec(), exe: editor.clone(), ..Program::default() };
        add_program(programs, &p);
    }
    for ext in [&b"TXT"[..], b"DOC", b"LOG", b"ME", b"1ST", b"SYS", b"CFG", b"NFO", b"H", b"C", b"RS", b"MD"] {
        if query(APP_ASSOC, ext).is_none() {
            write(APP_ASSOC, ext, Some(&editor));
        }
    }
}

/// The object of the installer of OS/2's applications (APPINST.EXE).
fn add_app_installer(programs: u32) {
    let exe = sys_path(b"OS2\\APPINST.EXE");
    if exists(&exe) && !titles(programs).iter().any(|t| t.3 == b"Install OS/2 Applications") {
        let p = Program { progc: PROG_PM, title: b"Install OS/2 Applications".to_vec(), exe, ..Program::default() };
        add_program(programs, &p);
    }
}

/// The Display settings' object (DISPLAY.EXE: the screen resolution);
/// version 4 profiles get it once.
fn add_display(programs: u32) {
    let exe = sys_path(b"OS2\\DISPLAY.EXE");
    if exists(&exe) && !titles(programs).iter().any(|t| t.3 == b"Display") {
        let p = Program { progc: PROG_PM, title: b"Display".to_vec(), exe, ..Program::default() };
        add_program(programs, &p);
    }
}

/// The installer belongs to the live media (an installed system has
/// none): its object goes where its program is not.
fn drop_missing_installer(programs: u32) {
    if exists(&sys_path(b"OS2\\PMINSTAL.EXE")) || exists(&sys_path(b"OS2\\INSTALL.EXE")) {
        return;
    }
    if let Some(t) = titles(programs).into_iter().find(|t| t.3 == b"Install rs2") {
        remove_program(t.0);
    }
}

/// Make (or complete) the default objects.
pub fn make_defaults() -> Groups {
    let desktop = create_group(b"Desktop", SHE_INVISIBLE);
    let prompts = create_group(b"Command Prompts", SHE_VISIBLE);
    let programs = create_group(b"Programs", SHE_VISIBLE);
    write(APP_DESKTOP, b"DesktopGroup", Some(&hex(desktop)));
    write(APP_DESKTOP, b"PromptsGroup", Some(&hex(prompts)));
    write(APP_DESKTOP, b"ProgramsGroup", Some(&hex(programs)));
    if titles(prompts).is_empty() {
        for (title, progc) in [(&b"OS/2 Window"[..], PROG_WINDOWABLEVIO), (&b"OS/2 Full Screen"[..], PROG_FULLSCREEN)] {
            let p = Program { progc, title: title.to_vec(), exe: b"*".to_vec(), ..Program::default() };
            add_program(prompts, &p);
        }
    }
    // DOS prompts (added to groups made before DOS sessions existed).
    if exists(&sys_path(b"OS2\\MDOS\\VDM.EXE")) {
        let have = titles(prompts);
        for (title, progc) in [(&b"DOS Window"[..], PROG_WINDOWEDVDM), (&b"DOS Full Screen"[..], PROG_VDM)] {
            if !have.iter().any(|t| t.3 == title) {
                let p = Program { progc, title: title.to_vec(), exe: b"*".to_vec(), ..Program::default() };
                add_program(prompts, &p);
            }
        }
    }
    add_editor(programs);
    add_app_installer(programs);
    add_display(programs);
    // The installer, on the live system and after: PMINSTAL.EXE (an
    // object of an earlier version, for the text-mode INSTALL.EXE, is
    // changed over), else INSTALL.EXE.
    let pm_installer = sys_path(b"OS2\\PMINSTAL.EXE");
    let installer = sys_path(b"OS2\\INSTALL.EXE");
    let (exe, progc) = if exists(&pm_installer) { (pm_installer, PROG_PM) } else { (installer, PROG_FULLSCREEN) };
    if exists(&exe) {
        let p = Program { progc, title: b"Install rs2".to_vec(), exe, ..Program::default() };
        match titles(programs).iter().find(|t| t.3 == b"Install rs2") {
            Some(t) => {
                if definition(t.0).is_some_and(|d| d.exe != p.exe) {
                    change_program(t.0, &p);
                }
            }
            None => {
                add_program(programs, &p);
            }
        }
    }
    if query(APP_DESKTOP, b"Background").is_none() {
        write(APP_DESKTOP, b"Background", Some(b"2E6E8E"));
    }
    let startup = query_u32(APP_DESKTOP, b"StartupGroup").filter(|&h| group_exists(h)).unwrap_or_else(make_startup);
    write(APP_DESKTOP, b"Version", Some(VERSION));
    Groups { desktop, prompts, programs, startup }
}
