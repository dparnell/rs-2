//! Self-checks of what the desktop shell (RS2SHELL.EXE) relies on: its
//! default objects in OS2.INI (made by `RS2SHELL /MAKEINI` if missing),
//! program objects created, changed and removed through the program list
//! API as the shell does it, PMWIN's desktop window pinning
//! (Rs2SetDesktopWindow), WinWindowFromPoint, WinQueryWindowProcess and
//! presentation parameter colours; the Startup folder (`RS2SHELL
//! /STARTUP` starts its objects once per session); shadows in OS2.INI
//! following their objects; copying and moving trees with their EAs (the
//! shell's fileops.rs); and direct manipulation through PMDRAG: string
//! handles, DRAGINFOs, RMF and type checks, and a drag with DrgDrag onto
//! a container in a second process (`DESKTEST DROPTARGET`, which answers
//! DM_DRAGOVER, takes the drop, asks for one item to be rendered with
//! DM_RENDER and ends each item's conversation). Each check prints PASS
//! or FAIL, then "desktest done: N failure(s)"; the exit code is N.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(dead_code)]

extern crate alloc;

#[path = "../../rs2shell/src/fileops.rs"]
mod fileops;
#[path = "../../rs2shell/src/objects.rs"]
mod objects;
#[path = "../../rs2shell/src/pm.rs"]
mod pm;
#[path = "../../rs2shell/src/shadows.rs"]
mod shadows;
mod drag;
mod files;

use core::sync::atomic::{AtomicU32, Ordering};

use objects::Program;
use pm::*;

os2::entry!(main);

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        os2::println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        os2::println!("FAIL {name}");
    }
}

extern "C" fn proc_(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinDefWindowProc(h, m, a, b) }
}

/// RS2SHELL /MAKEINI makes the default objects if they are missing.
fn make_ini() -> u32 {
    let prog = objects::sys_path(b"OS2\\RS2SHELL.EXE");
    let p = z(&prog);
    let mut args = prog.clone();
    args.push(0);
    args.extend_from_slice(b"/MAKEINI\0\0");
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), core::ptr::null(), &mut res, p.as_ptr()) };
    if rc != 0 { rc } else { res.code_result }
}

fn defaults() {
    check("RS2SHELL /MAKEINI", make_ini() == 0);
    check("RS2_Desktop Version", objects::query(objects::APP_DESKTOP, b"Version").is_some_and(|v| v == objects::VERSION));
    let g = |k: &[u8]| objects::query_u32(objects::APP_DESKTOP, k).filter(|&h| objects::group_exists(h));
    let (desk, prompts, programs) = (g(b"DesktopGroup"), g(b"PromptsGroup"), g(b"ProgramsGroup"));
    check("the shell's groups exist in the program list", desk.is_some() && prompts.is_some() && programs.is_some());
    let groups = objects::titles(SGH_ROOT);
    check("the Desktop group is hidden", groups.iter().any(|x| Some(x.0) == desk && x.2 & SHE_INVISIBLE != 0 && x.3 == b"Desktop"));
    check("Command Prompts and Programs are visible folders", groups.iter().any(|x| Some(x.0) == prompts && x.2 == SHE_VISIBLE && x.3 == b"Command Prompts") && groups.iter().any(|x| Some(x.0) == programs && x.3 == b"Programs"));
    let p = prompts.map(objects::titles).unwrap_or_default();
    let defs: alloc::vec::Vec<Program> = p.iter().filter_map(|x| objects::definition(x.0)).collect();
    check("OS/2 Window: the command processor in a window", defs.iter().any(|d| d.title == b"OS/2 Window" && d.exe == b"*" && d.progc == PROG_WINDOWABLEVIO));
    check("OS/2 Full Screen", defs.iter().any(|d| d.title == b"OS/2 Full Screen" && d.exe == b"*" && d.progc == PROG_FULLSCREEN));
    let editor = objects::sys_path(b"OS2\\E.EXE");
    if objects::exists(&editor) {
        let e = programs.map(objects::titles).unwrap_or_default();
        check("Programs: the System Editor", e.iter().any(|x| objects::definition(x.0).is_some_and(|d| d.exe == editor && d.progc == PROG_PM)));
        check("TXT files open in the System Editor", objects::association(b"txt").is_some_and(|a| a == editor));
    } else {
        os2::println!("(no E.EXE: System Editor checks skipped)");
    }
    let display = objects::sys_path(b"OS2\\DISPLAY.EXE");
    if objects::exists(&display) {
        let e = programs.map(objects::titles).unwrap_or_default();
        check("Programs: Display (the screen resolution)", e.iter().any(|x| x.3 == b"Display" && objects::definition(x.0).is_some_and(|d| d.exe == display && d.progc == PROG_PM)));
    }
    check("desktop background colour", objects::query(objects::APP_DESKTOP, b"Background").and_then(|t| objects::parse_hex(&t)).is_some());
}

fn program_objects() {
    let g = objects::create_group(b"DeskTest Folder", SHE_VISIBLE);
    check("PrfCreateGroup", g != 0);
    check("PrfCreateGroup again: the same group", objects::create_group(b"DeskTest Folder", SHE_VISIBLE) == g);
    let p = Program { progc: PROG_PM, title: b"Test Program".to_vec(), exe: b"C:\\HELLO.EXE".to_vec(), params: b"/X".to_vec(), dir: b"C:\\".to_vec(), ..Program::default() };
    let h = objects::add_program(g, &p);
    check("PrfAddProgram (as the settings notebook does)", h != 0);
    let d = objects::definition(h);
    check("PrfQueryDefinition gives it back", d.as_ref().is_some_and(|d| d.title == p.title && d.exe == p.exe && d.params == p.params && d.dir == p.dir && d.progc == PROG_PM));
    check("PrfQueryProgramTitles lists it", objects::titles(g).iter().any(|x| x.0 == h && x.3 == b"Test Program"));
    let mut q = p.clone();
    q.title = b"Renamed".to_vec();
    q.progc = PROG_WINDOWABLEVIO;
    q.params.clear();
    check("PrfChangeProgram", objects::change_program(h, &q));
    check("the change is there", objects::definition(h).is_some_and(|d| d.title == b"Renamed" && d.progc == PROG_WINDOWABLEVIO && d.params.is_empty()));
    // A DOS program object with DOS settings in its environment.
    let mut dosp = q.clone();
    dosp.progc = PROG_WINDOWEDVDM;
    dosp.env = b"DOS_HIGH=0\0EMS_MEMORY_LIMIT=0\0".to_vec();
    check("a DOS object's settings (pszEnvironment) are kept", objects::change_program(h, &dosp) && objects::definition(h).is_some_and(|d| d.progc == PROG_WINDOWEDVDM && d.env == dosp.env));
    objects::set_icon_pos(b"DESKTOP", b"PTEST", Some((123, -4)));
    check("icon positions in RS2_IconPos", objects::icon_pos(b"DESKTOP", b"PTEST") == Some((123, -4)));
    objects::set_icon_pos(b"DESKTOP", b"PTEST", None);
    check("icon position removed", objects::icon_pos(b"DESKTOP", b"PTEST").is_none());
    check("PrfRemoveProgram", objects::remove_program(h));
    check("it is gone", objects::definition(h).is_none() && objects::titles(g).is_empty());
    check("PrfDestroyGroup", objects::destroy_group(g) && !objects::group_exists(g));
}

fn pmwin(hab: u32) {
    unsafe { WinRegisterClass(hab, b"DeskTest\0".as_ptr(), proc_, 0, 0) };
    let mk = |t: &[u8]| {
        let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER;
        let mut client = 0;
        let tt = z(t);
        let f = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"DeskTest\0".as_ptr(), tt.as_ptr(), 0, 0, 1, &mut client) };
        (f, client)
    };
    let (a, _) = mk(b"Desk A");
    let (b, cb) = mk(b"Desk B");
    unsafe {
        WinSetWindowPos(a, HWND_TOP, 100, 100, 300, 200, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER);
        WinSetWindowPos(b, HWND_TOP, 200, 150, 300, 200, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
    }
    let old = unsafe { Rs2SetDesktopWindow(a) };
    check("Rs2SetDesktopWindow", unsafe { WinQueryWindow(HWND_DESKTOP, QW_BOTTOM) } == a);
    unsafe { WinSetWindowPos(a, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER | SWP_ACTIVATE) };
    check("the desktop window stays at the bottom when activated", unsafe { WinQueryWindow(HWND_DESKTOP, QW_BOTTOM) } == a);
    unsafe { WinSetWindowPos(b, HWND_BOTTOM, 0, 0, 0, 0, SWP_ZORDER) };
    check("a window sent to the bottom goes above it", unsafe { WinQueryWindow(HWND_DESKTOP, QW_BOTTOM) } == a && unsafe { WinQueryWindow(a, QW_PREV) } == b);
    check("Rs2SetDesktopWindow(previous) returns this one", unsafe { Rs2SetDesktopWindow(old) } == a);
    // WinWindowFromPoint: B's client at a point inside it.
    unsafe { WinSetWindowPos(b, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
    let s = window_pos(b);
    let p = PointL { x: s.x + s.cx / 2, y: s.y + s.cy / 2 };
    check("WinWindowFromPoint (top level)", unsafe { WinWindowFromPoint(HWND_DESKTOP, &p, 0) } == b);
    check("WinWindowFromPoint (descendant)", unsafe { WinWindowFromPoint(HWND_DESKTOP, &p, 1) } == cb);
    let far = PointL { x: -10, y: -10 };
    check("WinWindowFromPoint outside", unsafe { WinWindowFromPoint(HWND_DESKTOP, &far, 1) } == 0);
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    check("WinQueryWindowProcess", window_pid(b) == unsafe { (*pib).pid });
    // Presentation parameter colours.
    let c = 0x2e6e8eu32;
    let ok = unsafe { WinSetPresParam(cb, 3, 4, &c) } != 0;
    let mut got = 0u32;
    let mut id = 0u32;
    let n = unsafe { WinQueryPresParam(cb, 3, 0, &mut id, 4, &mut got, 0) };
    check("PP_BACKGROUNDCOLOR set and queried", ok && n == 4 && got == c && id == 3);
    check("PP_FOREGROUNDCOLOR not set", unsafe { WinQueryPresParam(cb, 1, 0, &mut id, 4, &mut got, 0) } == 0);
    // PP_FONTNAMESIZE: set on the frame, inherited by its client.
    let ok = unsafe { WinSetPresParam(b, 15, 8, b"12.Helv\0".as_ptr() as *const u32) } != 0;
    let mut name = [0u32; 8];
    let n = unsafe { WinQueryPresParam(cb, 15, 0, &mut id, 32, name.as_mut_ptr(), 0) };
    let text = unsafe { core::slice::from_raw_parts(name.as_ptr() as *const u8, 8) };
    check("PP_FONTNAMESIZE set and inherited", ok && n == 8 && &text[..8] == b"12.Helv\0" && id == 15);
    // QPF_NOINHERIT
    check("PP_FONTNAMESIZE not the client's own", unsafe { WinQueryPresParam(cb, 15, 0, &mut id, 32, name.as_mut_ptr(), 1) } == 0);
    check("PP_FONTNAMESIZE of an unknown face refused", unsafe { WinSetPresParam(b, 15, 13, b"10.NoSuchFace\0".as_ptr() as *const u32) } == 0);
    // A frame made with WinCreateWindow and FCF_SHELLPOSITION in its
    // FRAMECDATA (as the Toolkit's WPSTutorial makes its view) gets the
    // shell's position when it is first shown.
    let mut fcd = [0u8; 12];
    fcd[0..2].copy_from_slice(&12u16.to_le_bytes());
    fcd[2..6].copy_from_slice(&(FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_SHELLPOSITION).to_le_bytes());
    // WC_FRAME as a class atom: 0xffff0001.
    let f = unsafe { WinCreateWindow(HWND_DESKTOP, 0xffff_0001, b"Shell Position\0".as_ptr(), 0, 0, 0, 0, 0, 0, HWND_TOP, 9, fcd.as_ptr(), core::ptr::null()) };
    let before = window_pos(f);
    unsafe { WinShowWindow(f, 1) };
    let after = window_pos(f);
    check("WinCreateWindow frame with FCF_SHELLPOSITION: sized when shown", f != 0 && before.cx == 0 && after.cx > 0 && after.cy > 0);
    unsafe {
        WinDestroyWindow(f);
        WinDestroyWindow(a);
        WinDestroyWindow(b);
    }
}

/// The Startup folder: its group, and RS2SHELL /STARTUP starting its
/// program object once (the second time in a session it does not).
fn startup() {
    let g = objects::query_u32(objects::APP_DESKTOP, b"StartupGroup").filter(|&h| objects::group_exists(h));
    check("the Startup folder exists (RS2_Desktop StartupGroup)", g.is_some());
    let Some(g) = g else { return };
    check("the Startup folder is a visible group named Startup", objects::titles(SGH_ROOT).iter().any(|x| x.0 == g && x.2 == SHE_VISIBLE && x.3 == b"Startup"));
    let marker = b"C:\\DTSTART.TMP";
    fileops::delete_file(marker);
    let me = objects::sys_path(b"RS2\\DESKTEST.EXE");
    let p = Program { progc: PROG_DEFAULT, title: b"DeskTest Startup".to_vec(), exe: me, params: [&b"MARK "[..], marker].concat(), ..Program::default() };
    let h = objects::add_program(g, &p);
    check("a program object in the Startup folder", h != 0);
    let run = || {
        let prog = objects::sys_path(b"OS2\\RS2SHELL.EXE");
        let mut args = prog.clone();
        args.push(0);
        args.extend_from_slice(b"/STARTUP\0\0");
        let pz = z(&prog);
        let mut fail = [0u8; 64];
        let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
        unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), core::ptr::null(), &mut res, pz.as_ptr()) }
    };
    let wait = |ms: u32| {
        for _ in 0..ms / 100 {
            if fileops::exists(marker) {
                return true;
            }
            unsafe { os2::DosSleep(100) };
        }
        fileops::exists(marker)
    };
    check("RS2SHELL /STARTUP", run() == 0);
    check("the Startup folder's object started", wait(15000));
    fileops::delete_file(marker);
    check("RS2SHELL /STARTUP again", run() == 0);
    check("... starts nothing the second time in a session", !wait(3000));
    objects::remove_program(h);
}

/// Shadows: made, listed by folder, following a rename and a move of
/// their object (and of the directory they are kept in), broken when the
/// object is gone, removed with it.
fn shadow_checks() {
    use shadows::Target;
    let dir = b"C:\\DTSHADOW";
    fileops::delete_path(dir);
    let dz = z(dir);
    unsafe { os2::DosCreateDir(dz.as_ptr(), core::ptr::null_mut()) };
    let file = b"C:\\DTSHADOW\\ORIGINAL.TXT";
    files::write(file, b"the original");
    let before = shadows::all().len();
    let a = shadows::create(b"DESKTOP", Target::Path(file.to_vec()));
    let b = shadows::create(b"DIR:C:\\DTSHADOW\\", Target::Program(0x1234));
    check("shadows made", a != 0 && b != 0 && a != b && shadows::all().len() == before + 2);
    check("listed in their folder", shadows::in_folder(b"DESKTOP").iter().any(|s| s.id == a) && shadows::in_folder(b"DIR:C:\\DTSHADOW\\").iter().any(|s| s.id == b));
    check("a shadow of a file shows the file's name", shadows::get(a).is_some_and(|s| s.target.title() == b"ORIGINAL.TXT" && s.target.exists()));
    check("a shadow of a missing program object is broken", shadows::get(b).is_some_and(|s| !s.target.exists()));
    let renamed = b"C:\\DTSHADOW\\RENAMED.TXT";
    let (o, n) = (z(file), z(renamed));
    unsafe { os2::DosMove(o.as_ptr(), n.as_ptr()) };
    check("a rename: the shadow follows", shadows::path_moved(file, renamed) == 1 && shadows::get(a).is_some_and(|s| s.target == Target::Path(renamed.to_vec())));
    let (o, n) = (z(dir), z(b"C:\\DTSHADW2"));
    unsafe { os2::DosMove(o.as_ptr(), n.as_ptr()) };
    let moved = shadows::path_moved(dir, b"C:\\DTSHADW2");
    check("a directory moved: shadows of what is in it and kept in it follow", moved == 2 && shadows::get(a).is_some_and(|s| s.target == Target::Path(b"C:\\DTSHADW2\\RENAMED.TXT".to_vec())) && shadows::get(b).is_some_and(|s| s.folder == b"DIR:C:\\DTSHADW2\\"));
    check("Target ids parse back", Target::parse(&Target::Program(0xabc).id()) == Target::Program(0xabc) && Target::parse(b"DRIVE:D") == Target::Drive(b'D') && Target::parse(b"G0000ABCD") == Target::Group(0xabcd));
    fileops::delete_path(b"C:\\DTSHADW2");
    check("the object gone: the shadow is broken", shadows::get(a).is_some_and(|s| !s.target.exists()));
    check("deleted through the shell: its shadows go", shadows::target_deleted(&Target::Path(b"C:\\DTSHADW2".to_vec())) == 2 && shadows::get(a).is_none() && shadows::get(b).is_none());
    check("shadows cleaned up", shadows::all().len() == before);
}

/// `DESKTEST DOS W|F`: DOS program objects (GFX.COM and the DPMI test
/// under COMMAND.COM /K, from C:\\DOS) in the Startup folder, in DOS
/// windows (W) or full screen (F), started by the shell as it starts
/// objects; removed again once started.
fn dos_objects(windowed: bool) -> u32 {
    make_ini();
    let Some(g) = objects::query_u32(objects::APP_DESKTOP, b"StartupGroup").filter(|&h| objects::group_exists(h)) else {
        os2::println!("no Startup folder");
        return 1;
    };
    let progc = if windowed { PROG_WINDOWEDVDM } else { PROG_VDM };
    let gfx = Program { progc, title: b"GFX test".to_vec(), exe: b"C:\\DOS\\GFX.COM".to_vec(), ..Program::default() };
    let dpmi = Program { progc, title: b"DPMI test".to_vec(), exe: b"C:\\OS2\\MDOS\\COMMAND.COM".to_vec(), params: b"/K C:\\DOS\\DPMITEST".to_vec(), ..Program::default() };
    let (a, b) = (objects::add_program(g, &gfx), objects::add_program(g, &dpmi));
    check("DOS program objects in the Startup folder", a != 0 && b != 0);
    let prog = objects::sys_path(b"OS2\\RS2SHELL.EXE");
    let mut args = prog.clone();
    args.push(0);
    args.extend_from_slice(b"/STARTUP\0\0");
    let pz = z(&prog);
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), core::ptr::null(), &mut res, pz.as_ptr()) };
    check("RS2SHELL /STARTUP starts them", rc == 0);
    unsafe { os2::DosSleep(2000) };
    objects::remove_program(a);
    objects::remove_program(b);
    FAILS.load(Ordering::Relaxed)
}

/// `DESKTEST WIN`: Win-OS/2 program objects (Clock and Notepad, of type
/// PROG_31_ENHSEAMLESSCOMMON as APPINST makes them) in the Startup
/// folder, started by the shell: one seamless Win-OS/2 session, the
/// second program started in the first one's (docs/WINOS2.md).
fn winos2_objects() -> u32 {
    make_ini();
    let Some(g) = objects::query_u32(objects::APP_DESKTOP, b"StartupGroup").filter(|&h| objects::group_exists(h)) else {
        os2::println!("no Startup folder");
        return 1;
    };
    const PROG_31_ENHSEAMLESSCOMMON: u32 = 18;
    let dir = b"C:\\OS2\\MDOS\\WINOS2".to_vec();
    let clock = Program { progc: PROG_31_ENHSEAMLESSCOMMON, title: b"Clock".to_vec(), exe: b"C:\\OS2\\MDOS\\WINOS2\\CLOCK.EXE".to_vec(), dir: dir.clone(), ..Program::default() };
    let notepad = Program { progc: PROG_31_ENHSEAMLESSCOMMON, title: b"Notepad".to_vec(), exe: b"C:\\OS2\\MDOS\\WINOS2\\NOTEPAD.EXE".to_vec(), dir, ..Program::default() };
    let a = objects::add_program(g, &clock);
    check("a seamless Win-OS/2 object in the Startup folder", a != 0 && objects::definition(a).is_some_and(|d| d.progc == PROG_31_ENHSEAMLESSCOMMON));
    let prog = objects::sys_path(b"OS2\\RS2SHELL.EXE");
    let mut args = prog.clone();
    args.push(0);
    args.extend_from_slice(b"/STARTUP\0\0");
    let pz = z(&prog);
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), core::ptr::null(), &mut res, pz.as_ptr()) };
    check("RS2SHELL /STARTUP starts it", rc == 0);
    objects::remove_program(a);
    // The second one (as the shell starts it: DosStartSession with the
    // object's type) goes into the first one's session.
    unsafe { os2::DosSleep(3000) };
    let (t, n) = (z(&notepad.title), z(&notepad.exe));
    let empty = [0u8; 1];
    let sd = os2::StartData {
        length: core::mem::size_of::<os2::StartData>() as u16,
        related: 0,
        fgbg: 0,
        trace: 0,
        title: t.as_ptr(),
        name: n.as_ptr(),
        inputs: empty.as_ptr(),
        termq: core::ptr::null(),
        env: core::ptr::null(),
        inherit: 0,
        session_type: PROG_31_ENHSEAMLESSCOMMON as u16,
    };
    let (mut sid, mut pid) = (0, 0);
    let rc = unsafe { os2::DosStartSession(&sd, &mut sid, &mut pid) };
    check("a second one goes into the same session", (rc == 0 || rc == 457) && sid != 0);
    FAILS.load(Ordering::Relaxed)
}

fn main(s: &os2::Startup) -> u32 {
    let args: alloc::vec::Vec<u8> = s.args().iter().copied().skip_while(|&c| c == b' ').collect();
    if let Some(m) = args.strip_prefix(b"DOS ") {
        return dos_objects(m.first() == Some(&b'W'));
    }
    if args.starts_with(b"WIN") {
        return winos2_objects();
    }
    // DESKTEST MARK FILE: write FILE (a Startup folder object).
    if let Some(f) = args.strip_prefix(b"MARK ") {
        files::write(f, b"started");
        return 0;
    }
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if args.starts_with(b"DROPTARGET") {
        return drag::target(hab);
    }
    check("WinInitialize/WinCreateMsgQueue", hab != 0 && hmq != 0);
    defaults();
    program_objects();
    pmwin(hab);
    startup();
    shadow_checks();
    files::checks();
    drag::checks(hab);
    let f = FAILS.load(Ordering::Relaxed);
    os2::println!("desktest done: {f} failure(s)");
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    f
}
