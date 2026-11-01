//! The shell process's part (PMSHELL calls ShlStartWorkplace, as IBM's
//! PMSHELL calls PMWP's): the classes, the first start's objects
//! (install.rs), the server for other processes' object calls (ipc.rs),
//! the desktop, and the Startup folder's objects, once per PM session
//! (the system atom `WPS_STARTUP` marks it done; `SET
//! RESTARTOBJECTS=NO` turns it off, as on OS/2).

use crate::classes_gen::m;
use crate::obj::{self, OPEN_DEFAULT};
use crate::pm::*;
use crate::{install, ipc, wps};

unsafe extern "C" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
    #[link_name = "DOSCALLS.330"]
    fn DosQueryEventSem(hev: u32, count: *mut u32) -> u32;
}

const STARTED: &[u8] = b"WPS_STARTUP\0";

fn startup_wanted() -> bool {
    let mut p: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"RESTARTOBJECTS\0".as_ptr(), &mut p) } == 0 && !p.is_null() && cstr(p).eq_ignore_ascii_case(b"NO") {
        return false;
    }
    let t = unsafe { WinQuerySystemAtomTable() };
    if unsafe { WinFindAtom(t, STARTED.as_ptr()) } != 0 {
        return false;
    }
    unsafe { WinAddAtom(t, STARTED.as_ptr()) };
    true
}

/// The Startup folder's objects open (programs start).
fn run_startup() -> u32 {
    let f = wps::object_from_location(b"<WP_START>");
    if f == 0 {
        return 0;
    }
    let mut n = 0;
    let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, 0, 0]);
    while o != 0 && n < 200 {
        obj::call(m::WPObject::wpViewObject, &[o, 0, OPEN_DEFAULT as usize, 0]);
        n += 1;
        o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, o, 1]);
    }
    n
}

// ---- WinWaitForShell ----
//
// The shell posts an event semaphore for each stage of its start: the
// desktop created (WWFS_DESKTOPCREATED 1), opened (2) and populated (3);
// WinWaitForShell waits for one, or with WWFS_QUERY (0x80000000) only
// asks whether it is posted.

fn stage_sem(n: u32) -> [u8; 24] {
    let mut s = *b"\\SEM32\\RS2WPS\\DESKTOP0\0\0";
    s[21] = b'0' + n as u8;
    s
}

fn stages_create() {
    for n in 1..=3 {
        let mut h = 0;
        unsafe { os2::DosCreateEventSem(stage_sem(n).as_ptr(), &mut h, 0, 0) };
    }
}

fn stage_reached(n: u32) {
    let mut h = 0;
    if unsafe { DosOpenEventSem(stage_sem(n).as_ptr(), &mut h) } == 0 {
        unsafe {
            os2::DosPostEventSem(h);
            os2::DosCloseEventSem(h);
        }
    }
}

/// WinWaitForShell(ulEvent) (PMWP 512): TRUE once the shell's desktop
/// is created, opened or populated (waiting for it, unless WWFS_QUERY);
/// FALSE for other events or without a shell.
#[unsafe(no_mangle)]
pub extern "C" fn WinWaitForShell(event: u32) -> u32 {
    const WWFS_QUERY: u32 = 0x8000_0000;
    let n = event & !WWFS_QUERY;
    if !(1..=3).contains(&n) {
        return 0;
    }
    let mut h = 0;
    if unsafe { DosOpenEventSem(stage_sem(n).as_ptr(), &mut h) } != 0 {
        return 0;
    }
    let ok = if event & WWFS_QUERY != 0 {
        let mut count = 0;
        (unsafe { DosQueryEventSem(h, &mut count) }) == 0 && count != 0
    } else {
        (unsafe { os2::DosWaitEventSem(h, u32::MAX) }) == 0
    };
    unsafe { os2::DosCloseEventSem(h) };
    ok as u32
}

/// The registered classes (OS2SYS.INI's class table) are loaded when the
/// shell starts, as on OS/2: their DLLs set themselves up (MMPARTS's
/// SOMInitModule) and their instance types and filters choose the
/// classes of files from then on. A class whose DLL is missing or does
/// not load stays out (its objects wake as their parent classes').
fn load_registered_classes() {
    use crate::util::{prf_keys, prf_read, prf_write, HINI_USER};
    // INI.RC's and MINSTALL's `PM_InstallClassReplacement` (key: the
    // class replaced, value: the one replacing it) go to the replace
    // list once, as IBM's ShlInitClassInfo takes them.
    const INSTALL: &[u8] = b"PM_InstallClassReplacement";
    for k in prf_keys(HINI_USER, INSTALL) {
        if let Some(v) = prf_read(HINI_USER, INSTALL, &k) {
            let new = v.split(|&c| c == 0).next().unwrap_or(b"").to_vec();
            if !new.is_empty() {
                crate::api::update_replace_list(&k, &new, true);
            }
        }
        prf_write(HINI_USER, INSTALL, &k, None);
    }
    for (c, d) in crate::clsmgr::table() {
        if crate::som::find_class(&c) == 0 {
            crate::util::debug(&alloc::format!(
                "PMWP: class {} of {} not loaded",
                core::str::from_utf8(&c).unwrap_or("?"),
                core::str::from_utf8(&d).unwrap_or("?")
            ));
        }
    }
    // The replacements in effect (the last one for each class), where
    // the replacing class loads.
    for old in prf_keys(HINI_USER, crate::api::REPLACE_LIST) {
        let v = prf_read(HINI_USER, crate::api::REPLACE_LIST, &old).unwrap_or_default();
        if let Some(new) = v.split(|&c| c == 0).filter(|s| !s.is_empty()).last() {
            crate::api::replace_class(&old, new);
        }
    }
}

/// ShlStartWorkplace(hab, hmq): start the Workplace Shell in this process
/// (its PM thread). FALSE if another process runs it.
#[unsafe(no_mangle)]
pub extern "C" fn ShlStartWorkplace(hab: u32, _hmq: u32) -> u32 {
    if ipc::shell_running() {
        return 0;
    }
    if !wps::start() {
        return 0;
    }
    stages_create();
    install::run();
    load_registered_classes();
    crate::printer::sync_printers();
    crate::setup::apply_saved();
    crate::system::apply_saved();
    ipc::start_server(hab);
    let desk = wps::object_from_location(b"<WP_DESKTOP>");
    if desk != 0 {
        stage_reached(1);
        obj::call(m::WPObject::wpViewObject, &[desk, 0, OPEN_DEFAULT as usize, 0]);
        // The desktop's view is filled as it opens.
        stage_reached(2);
        stage_reached(3);
    }
    crate::minwin::apply();
    crate::desktop::archive_at_startup();
    if startup_wanted() {
        run_startup();
    }
    crate::desktop::started();
    1
}

// ---- The shell's exception guard (IBM's ShlSaveEnv and friends) ----
//
// PMSHELL and the WPS class DLLs (MMPARTS) guard their calls as IBM's
// PMWP does: `if (ShlSaveEnv(&rec, ShlExceptionHandler)) { ...;
// DosUnsetExceptionHandler(&rec); } else { a trap happened }`. The
// record is an EXCEPTIONREGISTRATIONRECORD (prev, handler) followed by
// the caller's EBP, EBX, EDI, ESI, ESP and return address.

/// The shell's window for the object calls (IBM's: its object thread's
/// window); MMPARTS waits for it before it configures itself.
#[unsafe(no_mangle)]
pub static mut vhwndWorkplaceWindow: u32 = 0;

/// The shell's help instance (IBM's: made at start; rs2's objects show
/// their help through their own instances, so none).
#[unsafe(no_mangle)]
pub static mut vhwndHelpInstance: u32 = 0;

/// ShlSaveEnv(prec, pfnHandler) (PMWP 213): saves the caller's
/// registers and return address in the record and puts the record at the
/// head of the thread's exception handler chain; returns 1. When the
/// handler takes a trap, ShlSaveEnv returns again, with 0.
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub extern "C" fn ShlSaveEnv() {
    core::arch::naked_asm!(
        "mov ecx, [esp + 4]",
        "lea eax, [esp + 4]",
        "mov [ecx + 0x18], eax",
        "mov [ecx + 0x08], ebp",
        "mov eax, [esp]",
        "mov [ecx + 0x1c], eax",
        "mov [ecx + 0x0c], ebx",
        "mov [ecx + 0x10], edi",
        "mov [ecx + 0x14], esi",
        "mov eax, fs:[0]",
        "mov [ecx], eax",
        "mov eax, [esp + 8]",
        "mov [ecx + 0x04], eax",
        "mov fs:[0], ecx",
        "mov eax, 1",
        "ret",
    );
}

/// ShlRestoreEnv(prec, rc): back to where ShlSaveEnv returned, with
/// `rc`; the record is taken off the chain.
#[unsafe(naked)]
extern "C" fn shl_restore_env(_rec: u32, _rc: u32) -> ! {
    core::arch::naked_asm!(
        "mov ecx, [esp + 4]",
        "mov eax, [esp + 8]",
        "mov edx, [ecx]",
        "mov fs:[0], edx",
        "mov ebp, [ecx + 0x08]",
        "mov ebx, [ecx + 0x0c]",
        "mov edi, [ecx + 0x10]",
        "mov esi, [ecx + 0x14]",
        "mov esp, [ecx + 0x18]",
        "jmp dword ptr [ecx + 0x1c]",
    );
}

/// ShlExceptionHandler(report, rec, context, dispatcher) (PMWP 212), as
/// IBM's: an access violation goes back to ShlSaveEnv's caller (unless
/// SHELLEXCEPTIONHANDLER says OFF), logged; a kill signal to the shell's
/// process saves what the lazy writer holds first. Everything else goes
/// on to the next handler.
#[unsafe(no_mangle)]
pub extern "C" fn ShlExceptionHandler(report: *const u32, rec: u32, context: *const u32, _disp: u32) -> u32 {
    const XCPT_ACCESS_VIOLATION: u32 = 0xC000_0005;
    const XCPT_SIGNAL: u32 = 0xC001_0003;
    const XCPT_SIGNAL_KILLPROC: u32 = 3;
    const EH_UNWINDING: u32 = 0x02;
    const EH_EXIT_UNWIND: u32 = 0x04;
    if report.is_null() {
        return 0;
    }
    let (code, flags) = unsafe { (*report, *report.add(1)) };
    if flags & (EH_UNWINDING | EH_EXIT_UNWIND) != 0 {
        return 0;
    }
    match code {
        XCPT_ACCESS_VIOLATION => {
            let mut v: *const u8 = core::ptr::null();
            let off = unsafe { os2::DosScanEnv(b"SHELLEXCEPTIONHANDLER\0".as_ptr(), &mut v) } == 0
                && !v.is_null()
                && crate::util::cstr(v).windows(3).any(|w| w.eq_ignore_ascii_case(b"OFF"));
            if off {
                return 0;
            }
            if !context.is_null() {
                // CONTEXTRECORD (bsexcpt.h): EBP at 0x98, EIP 0x9c, ESP 0xa8.
                let (ebp, eip, esp) = unsafe { (*context.add(0x98 / 4), *context.add(0x9c / 4), *context.add(0xa8 / 4)) };
                let mut s = alloc::format!("PMWP: trap at {eip:#x} caught by ShlExceptionHandler; called from");
                let mut f = ebp;
                for _ in 0..12 {
                    if f < esp || f >= esp.wrapping_add(0x10000) || f & 3 != 0 {
                        break;
                    }
                    let (next, ret) = unsafe { (*(f as *const u32), *((f + 4) as *const u32)) };
                    s.push_str(&alloc::format!(" {ret:#x}"));
                    if next <= f {
                        break;
                    }
                    f = next;
                }
                crate::util::debug(&s);
                // Registers (EDI at 0x80 .. EDX at 0x94).
                let r = |o: usize| unsafe { *context.add(o / 4) };
                crate::util::debug(&alloc::format!(
                    "PMWP: eax {:x} ebx {:x} ecx {:x} edx {:x} esi {:x} edi {:x} ebp {ebp:x} esp {esp:x}",
                    r(0x88), r(0x8c), r(0x90), r(0x94), r(0x84), r(0x80)
                ));
            }
            shl_restore_env(rec, 0);
        }
        XCPT_SIGNAL => {
            let sig = unsafe { *report.add(5) };
            let mut v: *const u8 = core::ptr::null();
            if sig == XCPT_SIGNAL_KILLPROC && unsafe { os2::DosScanEnv(b"WORKPLACE_PROCESS\0".as_ptr(), &mut v) } == 0 {
                save_all();
            }
        }
        _ => {}
    }
    0
}

/// WinShutdownSystem(hab, hmq): close the programs and turn off.
#[unsafe(no_mangle)]
pub extern "C" fn WinShutdownSystem(_hab: u32, _hmq: u32) -> u32 {
    crate::shutdown::begin(false);
    1
}

/// Before turning off: what the lazy writer has not saved yet.
pub fn save_all() {
    crate::obj::flush_dirty();
}
