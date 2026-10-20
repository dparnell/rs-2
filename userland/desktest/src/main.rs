//! Self-checks of what the desktop shell (RS2SHELL.EXE) relies on: its
//! default objects in OS2.INI (made by `RS2SHELL /MAKEINI` if missing),
//! program objects created, changed and removed through the program list
//! API as the shell does it, and PMWIN's desktop window pinning
//! (Rs2SetDesktopWindow), WinWindowFromPoint, WinQueryWindowProcess and
//! presentation parameter colours. Each check prints PASS or FAIL, then
//! "desktest done: N failure(s)"; the exit code is N.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(dead_code)]

extern crate alloc;

#[path = "../../rs2shell/src/objects.rs"]
mod objects;
#[path = "../../rs2shell/src/pm.rs"]
mod pm;

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
    unsafe {
        WinDestroyWindow(a);
        WinDestroyWindow(b);
    }
}

fn main(_s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    check("WinInitialize/WinCreateMsgQueue", hab != 0 && hmq != 0);
    defaults();
    program_objects();
    pmwin(hab);
    let f = FAILS.load(Ordering::Relaxed);
    os2::println!("desktest done: {f} failure(s)");
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    f
}
