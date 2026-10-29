//! A stand-in for Program Manager's DDE: setup programs (Delphi's
//! installer, through DDEML) open a conversation with service and topic
//! PROGMAN and send it commands such as `[CreateGroup("Delphi")]` and
//! `[AddItem("C:\DELPHI\BIN\DELPHI.EXE", "Delphi")]`. When no task or PM
//! program answers a WM_DDE_INITIATE for PROGMAN (dde.rs), a window of
//! this task does: a group becomes a folder on the Workplace Shell's
//! desktop and an item a program object in it (WinCreateObject; without
//! a running shell, a program object on the desktop), as Win-OS/2's
//! Program Manager makes its groups known to OS/2. The groups are
//! listed in PROGMAN.INI's [Groups], as Windows' Program Manager does,
//! and a request for "Groups" or for a group answers with them.

use alloc::vec::Vec;

use crate::dde::*;
use crate::kernel::{global_alloc, global_free, global_size, GMEM_DDESHARE};
use crate::user::{self, post, send};
use crate::Global;

const DDE_FACK: u16 = 0x8000;
const CF_TEXT: u16 = 1;

/// The group commands go to (the last created or shown).
static CURRENT: Global<Vec<u8>> = Global::new(Vec::new());

fn is_progman(atom: u16) -> bool {
    global_atom_name(atom).eq_ignore_ascii_case(b"PROGMAN")
}

/// A WM_DDE_INITIATE nobody answered: answer it for service PROGMAN
/// (topic PROGMAN or any).
pub fn initiate(client: u16, app: u16, topic: u16) {
    if client == 0 || !is_progman(app) || topic != 0 && !is_progman(topic) {
        return;
    }
    let w = create_window();
    if w == 0 {
        return;
    }
    let (a, t) = (add_global_atom(b"PROGMAN"), add_global_atom(b"PROGMAN"));
    send(client, WM_DDE_ACK, w, (t as u32) << 16 | a as u32);
}

fn create_window() -> u16 {
    static CLASS: Global<bool> = Global::new(false);
    let hinst = crate::kernel::TASK.get().hinst;
    if !*CLASS.get() {
        let proc_ = user::native_far(10);
        let ok = crate::mem16::with_str(b"W16Progman", |fname| {
            crate::mem16::with(26, |far, p| {
                let mut b = [0u8; 26];
                b[2..6].copy_from_slice(&proc_.to_le_bytes());
                b[10..12].copy_from_slice(&hinst.max(1).to_le_bytes());
                b[22..26].copy_from_slice(&fname.to_le_bytes());
                unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 26) };
                user::call(57, &mut crate::commdlg::args(&[crate::commdlg::A::L(far)]))
            })
        });
        *CLASS.get() = ok != 0;
    }
    let Some(ci) = user::find_class_name(b"W16Progman") else { return 0 };
    user::create(&user::Create { exstyle: 0, class: ci, title: b"Program Manager", title_far: 0, class_far: 0, style: user::WS_POPUP, pos: [0, 0, 0, 0], parent: 0, menu: 0, hinst, param: 0 })
}

/// The server window's procedure (WIN16 entry 10).
pub fn wnd_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    let (lo, hi) = (lp as u16, (lp >> 16) as u16);
    match m {
        WM_DDE_EXECUTE => {
            let text = block_text(hi);
            let ok = execute(&text);
            if !post(wp, WM_DDE_ACK, h, (hi as u32) << 16 | if ok { DDE_FACK } else { 0 } as u32) {
                global_free(hi);
            }
            0
        }
        WM_DDE_REQUEST => {
            let (fmt, item) = (lo, hi);
            let name = global_atom_name(item);
            let reply = if fmt == CF_TEXT { request(&name) } else { None };
            match reply {
                Some(text) => {
                    // DDEDATA: fResponse | fRelease, the format, the text.
                    let g = global_alloc(0x0002 | GMEM_DDESHARE, text.len() as u32 + 5);
                    if g != 0 {
                        let p = thunk16::flat(((g | 7) as u32) << 16);
                        unsafe {
                            (p as *mut u16).write_unaligned(0x1000 | 0x2000);
                            (p.add(2) as *mut u16).write_unaligned(CF_TEXT);
                            core::ptr::copy_nonoverlapping(text.as_ptr(), p.add(4), text.len());
                            *p.add(4 + text.len()) = 0;
                        }
                    }
                    if g == 0 || !post(wp, WM_DDE_DATA, h, (item as u32) << 16 | g as u32) {
                        global_free(g);
                    }
                }
                None => {
                    post(wp, WM_DDE_ACK, h, (item as u32) << 16);
                }
            }
            0
        }
        WM_DDE_POKE | WM_DDE_ADVISE => {
            global_free(lo);
            post(wp, WM_DDE_ACK, h, (hi as u32) << 16);
            0
        }
        WM_DDE_UNADVISE => {
            post(wp, WM_DDE_ACK, h, (hi as u32) << 16);
            0
        }
        WM_DDE_TERMINATE => {
            post(wp, WM_DDE_TERMINATE, h, 0);
            user::destroy_window(h);
            0
        }
        _ => user::def_window_proc(h, m, wp, lp),
    }
}

fn block_text(g: u16) -> Vec<u8> {
    let n = global_size(g) as usize;
    if n == 0 {
        return Vec::new();
    }
    let b = unsafe { core::slice::from_raw_parts(thunk16::flat(((g | 7) as u32) << 16), n) };
    b[..b.iter().position(|&c| c == 0).unwrap_or(n)].to_vec()
}

/// A request's answer: "Groups" lists the groups (one a line), a group
/// its line `"name",path,items` and an empty item list.
fn request(item: &[u8]) -> Option<Vec<u8>> {
    let groups = groups();
    if item.eq_ignore_ascii_case(b"Groups") || item.eq_ignore_ascii_case(b"PROGMAN") {
        let mut t = Vec::new();
        for g in &groups {
            t.extend_from_slice(g);
            t.extend_from_slice(b"\r\n");
        }
        return Some(t);
    }
    let g = groups.iter().find(|g| g.eq_ignore_ascii_case(item))?;
    Some([b"\"", &g[..], b"\",", &group_file(g)[..], b",0\r\n"].concat())
}

// ---- The command language ----

/// Parse `[Command(arg, "arg", ...)][...]` and run each command; false
/// when one fails.
fn execute(text: &[u8]) -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < text.len() {
        if text[i] != b'[' {
            i += 1;
            continue;
        }
        i += 1;
        let name_start = i;
        while i < text.len() && (text[i].is_ascii_alphanumeric() || text[i] == b'_') {
            i += 1;
        }
        let name = text[name_start..i].to_vec();
        let mut args: Vec<Vec<u8>> = Vec::new();
        while i < text.len() && text[i] == b' ' {
            i += 1;
        }
        if i < text.len() && text[i] == b'(' {
            i += 1;
            let mut cur = Vec::new();
            let mut quoted = false;
            while i < text.len() {
                let c = text[i];
                i += 1;
                match c {
                    b'"' if quoted && text.get(i) == Some(&b'"') => {
                        cur.push(b'"');
                        i += 1;
                    }
                    b'"' => quoted = !quoted,
                    b',' if !quoted => args.push(core::mem::take(&mut cur)),
                    b')' if !quoted => break,
                    _ => cur.push(c),
                }
            }
            args.push(cur);
        }
        while i < text.len() && text[i] != b']' {
            i += 1;
        }
        i += 1;
        let args: Vec<Vec<u8>> = args.into_iter().map(|a| trim(&a).to_vec()).collect();
        if crate::tracing() {
            let mut all = Vec::new();
            for (k, a) in args.iter().enumerate() {
                if k > 0 {
                    all.push(b',');
                }
                all.extend_from_slice(a);
            }
            crate::pm::trace(format_args!("W16 PROGMAN {}({})", core::str::from_utf8(&name).unwrap_or("?"), core::str::from_utf8(&all).unwrap_or("?")));
        }
        ok &= command(&name, &args);
    }
    ok
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ').map_or(a, |i| i + 1);
    &s[a..b]
}

fn command(name: &[u8], args: &[Vec<u8>]) -> bool {
    let arg = |i: usize| args.get(i).map_or(&b""[..], |a| &a[..]);
    let is = |n: &[u8]| name.eq_ignore_ascii_case(n);
    if is(b"CreateGroup") {
        let g = arg(0);
        if g.is_empty() {
            return false;
        }
        add_group(g);
        create_folder(g);
        *CURRENT.get() = g.to_vec();
        true
    } else if is(b"ShowGroup") {
        let g = arg(0);
        if groups().iter().any(|x| x.eq_ignore_ascii_case(g)) {
            *CURRENT.get() = g.to_vec();
        }
        true
    } else if is(b"AddItem") {
        let cmd = arg(0);
        if cmd.is_empty() {
            return false;
        }
        add_item(cmd, arg(1), arg(2), arg(6));
        true
    } else if is(b"DeleteGroup") {
        let g = arg(0).to_vec();
        destroy(&folder_id(&g));
        remove_group(&g);
        true
    } else if is(b"DeleteItem") || is(b"ReplaceItem") {
        let g = CURRENT.get().clone();
        destroy(&item_id(&g, arg(0)));
        true
    } else {
        // Reload, ExitProgman...: nothing to do.
        is(b"Reload") || is(b"ExitProgman")
    }
}

// ---- PROGMAN.INI ----

fn groups() -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    for n in 1..100 {
        let key = alloc::format!("Group{n}");
        if let Some(file) = crate::profile::get(b"PROGMAN.INI", b"Groups", key.as_bytes()) {
            if let Some(name) = crate::profile::get(b"PROGMAN.INI", b"Group Names", &file) {
                v.push(name);
            }
        }
    }
    v
}

/// The group's file, as Program Manager names it (C:\WINDOWS\NAME.GRP).
fn group_file(g: &[u8]) -> Vec<u8> {
    let stem: Vec<u8> = g.iter().filter(|c| c.is_ascii_alphanumeric()).take(8).map(|c| c.to_ascii_uppercase()).collect();
    [crate::profile::WINDOWS_DIR, b"\\", if stem.is_empty() { b"GROUP" } else { &stem[..] }, b".GRP"].concat()
}

fn write(sec: &[u8], key: &[u8], value: Option<&[u8]>) {
    crate::mem16::with_str(sec, |s| {
        crate::mem16::with_str(key, |k| match value {
            Some(v) => crate::mem16::with_str(v, |v| crate::profile::write_string(b"PROGMAN.INI", s, k, v)),
            None => crate::profile::write_string(b"PROGMAN.INI", s, k, 0),
        })
    });
}

fn add_group(g: &[u8]) {
    if groups().iter().any(|x| x.eq_ignore_ascii_case(g)) {
        return;
    }
    let file = group_file(g);
    let n = (1..100).find(|n| crate::profile::get(b"PROGMAN.INI", b"Groups", alloc::format!("Group{n}").as_bytes()).is_none()).unwrap_or(99);
    write(b"Groups", alloc::format!("Group{n}").as_bytes(), Some(&file));
    write(b"Group Names", &file, Some(g));
}

fn remove_group(g: &[u8]) {
    let file = group_file(g);
    for n in 1..100 {
        let key = alloc::format!("Group{n}");
        if crate::profile::get(b"PROGMAN.INI", b"Groups", key.as_bytes()).is_some_and(|f| f.eq_ignore_ascii_case(&file)) {
            write(b"Groups", key.as_bytes(), None);
        }
    }
    write(b"Group Names", &file, None);
}

// ---- The Workplace Shell ----

/// A PMWP entry point (PMWP is loaded when first needed).
fn wp(ord: u32) -> usize {
    static MODULE: Global<u32> = Global::new(0);
    let m = MODULE.get();
    if *m == 0 {
        let mut fail = [0u8; 32];
        if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 32, b"PMWP\0".as_ptr(), m) } != 0 {
            return 0;
        }
    }
    let mut f = 0usize;
    if unsafe { os2::DosQueryProcAddr(*m, ord, core::ptr::null(), &mut f) } != 0 {
        return 0;
    }
    f
}

const CO_REPLACEIFEXISTS: u32 = 1;
const CO_UPDATEIFEXISTS: u32 = 2;

/// WinCreateObject(class, title, setup, location, flags).
fn create_object(class: &[u8], title: &[u8], setup: &[u8], location: &[u8], flags: u32) -> u32 {
    let f = wp(281);
    if f == 0 {
        return 0;
    }
    let f: extern "C" fn(*const u8, *const u8, *const u8, *const u8, u32) -> u32 = unsafe { core::mem::transmute(f) };
    let (c, t, s, l) = (user::z(class), user::z(title), user::z(setup), user::z(location));
    f(c.as_ptr(), t.as_ptr(), s.as_ptr(), l.as_ptr(), flags)
}

/// WinDestroyObject(WinQueryObject(id)).
fn destroy(id: &[u8]) {
    let (q, d) = (wp(252), wp(251));
    if q == 0 || d == 0 {
        return;
    }
    let q: extern "C" fn(*const u8) -> u32 = unsafe { core::mem::transmute(q) };
    let d: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(d) };
    let z = user::z(id);
    let h = q(z.as_ptr());
    if h != 0 {
        d(h);
    }
}

/// Object IDs: <W16_GROUP_name>, <W16_ITEM_group_name>.
fn folder_id(g: &[u8]) -> Vec<u8> {
    let clean = |s: &[u8]| s.iter().map(|&c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { b'_' }).collect::<Vec<u8>>();
    [b"<W16_GROUP_", &clean(g)[..], b">"].concat()
}

fn item_id(g: &[u8], name: &[u8]) -> Vec<u8> {
    let clean = |s: &[u8]| s.iter().map(|&c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { b'_' }).collect::<Vec<u8>>();
    [b"<W16_ITEM_", &clean(g)[..], b"_", &clean(name)[..], b">"].concat()
}

/// The group's folder on the desktop (none without a running shell).
fn create_folder(g: &[u8]) -> bool {
    let setup = [b"OBJECTID=", &folder_id(g)[..], b";"].concat();
    create_object(b"WPFolder", g, &setup, b"<WP_DESKTOP>", CO_UPDATEIFEXISTS) != 0
}

/// AddItem(command line, name, icon path, icon index, x, y, directory):
/// a program object in the current group's folder (or on the desktop).
fn add_item(cmd: &[u8], name: &[u8], icon: &[u8], dir: &[u8]) {
    let (exe, params) = match cmd.iter().position(|&c| c == b' ') {
        Some(i) => (&cmd[..i], trim(&cmd[i + 1..])),
        None => (cmd, &b""[..]),
    };
    let title = if name.is_empty() { crate::module::base_name(exe) } else { name.to_vec() };
    // The program where Windows would find it ("NOTEPAD.EXE README.TXT":
    // the Windows directory, the PATH). Windows' Notepad is not there:
    // the System Editor opens the text instead. A program that is not
    // there at all (WINHELP.EXE) shows the icon of the file it opens.
    let found = crate::compat::find_program(exe)
        .or_else(|| crate::module::base_name(exe).eq_ignore_ascii_case(b"NOTEPAD.EXE").then(|| crate::compat::find_program(b"E.EXE")).flatten());
    let icon = match &found {
        None if icon.is_empty() && crate::compat::find_program(params).is_some() => params,
        _ => icon,
    };
    let exe = found.as_deref().unwrap_or(exe);
    // The program's own directory unless one is given (as Program
    // Manager starts it).
    let dir = if dir.is_empty() { exe[..exe.iter().rposition(|&c| c == b'\\').unwrap_or(0)].to_vec() } else { dir.to_vec() };
    let g = CURRENT.get().clone();
    let mut setup = [b"EXENAME=", exe, b";STARTUPDIR=", &dir[..], b";"].concat();
    if !params.is_empty() {
        setup.extend_from_slice(&[b"PARAMETERS=", params, b";"].concat());
    }
    if !icon.is_empty() && !icon.eq_ignore_ascii_case(exe) {
        setup.extend_from_slice(&[b"ICONFILE=", icon, b";"].concat());
    }
    setup.extend_from_slice(&[b"OBJECTID=", &item_id(&g, &title)[..], b";"].concat());
    if !g.is_empty() && create_object(b"WPProgram", &title, &setup, &folder_id(&g), CO_REPLACEIFEXISTS) != 0 {
        return;
    }
    create_object(b"WPProgram", &title, &setup, b"<WP_DESKTOP>", CO_REPLACEIFEXISTS);
}
