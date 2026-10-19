//! Session manager APIs: DosStartSession, DosSelectSession, DosSetSession,
//! DosStopSession (sessions themselves are in crate::session).

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::session;
use crate::task::process;

const ERROR_SMG_INVALID_SESSION_ID: u32 = 369;
const ERROR_SMG_SESSION_NOT_FOUND: u32 = 371;
const ERROR_SMG_SESSION_NOT_PARENT: u32 = 452;
const ERROR_SMG_INVALID_RELATED_OPT: u32 = 454;
const ERROR_SMG_INVALID_SELECT_OPT: u32 = 456;
const ERROR_SMG_START_IN_BACKGROUND: u32 = 457;
const ERROR_SMG_INVALID_STOP_OPTION: u32 = 458;
const ERROR_SMG_INVALID_DATA_LENGTH: u32 = 461;
const ERROR_SMG_INVALID_PROGRAM_TYPE: u32 = 491;

const SSF_TYPE_DEFAULT: u16 = 0;
const SSF_TYPE_FULLSCREEN: u16 = 1;
const SSF_TYPE_WINDOWABLEVIO: u16 = 2;
const SSF_TYPE_PM: u16 = 3;
const SSF_TYPE_VDM: u16 = 4;
const SSF_TYPE_WINDOWEDVDM: u16 = 7;

/// The fields of STARTDATA rs2 uses.
struct StartData {
    related: bool,
    background: bool,
    title: Option<String>,
    name: Option<String>,
    inputs: String,
    termq: Option<String>,
    env: Option<Vec<String>>,
    /// SessionType (SSF_TYPE_*).
    session_type: u16,
}

/// Read STARTDATA (pointers are 16:16 for the 16-bit API).
fn read_start_data(c: &Ctx, at: u32) -> Result<StartData, u32> {
    let len = c.read_u16(at).ok_or(err::INVALID_ADDRESS)? as u32;
    if len < 24 {
        return Err(ERROR_SMG_INVALID_DATA_LENGTH);
    }
    let u16_at = |o: u32| if o + 2 <= len { c.read_u16(at + o).unwrap_or(0) } else { 0 };
    let ptr_at = |o: u32| {
        if o + 4 > len {
            return 0;
        }
        let v = c.read_u32(at + o).unwrap_or(0);
        if c.is16 { c.far16_to_flat(v) } else { v }
    };
    let str_at = |o: u32| match ptr_at(o) {
        0 => None,
        p => c.read_str(p, 1024),
    };
    let related = match u16_at(2) {
        0 => false,
        1 => true,
        _ => return Err(ERROR_SMG_INVALID_RELATED_OPT),
    };
    let ty = u16_at(30);
    if ty == SSF_TYPE_VDM || ty == SSF_TYPE_WINDOWEDVDM {
        // DOS sessions need V86 mode, which long mode does not have.
        return Err(ERROR_SMG_INVALID_PROGRAM_TYPE);
    }
    if ty > SSF_TYPE_WINDOWEDVDM {
        return Err(ERROR_SMG_INVALID_PROGRAM_TYPE);
    }
    let env = match ptr_at(24) {
        0 => None,
        p => Some(super::proc::read_block(c, p, 4096).unwrap_or_default()),
    };
    Ok(StartData {
        related,
        background: u16_at(4) & 1 != 0,
        title: str_at(8).filter(|s| !s.is_empty()),
        name: str_at(12).filter(|s| !s.is_empty()),
        inputs: str_at(16).unwrap_or_default(),
        termq: if related { str_at(20).filter(|s| !s.is_empty()) } else { None },
        env,
        session_type: ty,
    })
}

/// Whether a session started by a process of `caller` with `ty` for a
/// program of FAPPTYP `app` runs in a PM window. As on OS/2: type 2
/// (windowable VIO) does, unless the program is not window compatible;
/// type 0 (default) does for a window-compatible program started from
/// PM or from a windowed session; full-screen (1) and PM (3) programs
/// never do.
fn wants_window(ty: u16, app: u32, caller: u32) -> bool {
    let app = app & 7;
    let from_pm = (caller != 0 && caller == session::pm_session()) || session::is_windowed(caller);
    match ty {
        SSF_TYPE_WINDOWABLEVIO => app != 1 && app != 3,
        SSF_TYPE_DEFAULT => app == 2 && from_pm,
        SSF_TYPE_FULLSCREEN | SSF_TYPE_PM => false,
        _ => false,
    }
}

/// The caller's session counts as the foreground one: it is, or it is a
/// windowed session (or PM) while PM is in front.
fn caller_in_front(caller: u32) -> bool {
    let fg = session::foreground();
    fg == caller || (fg != 0 && fg == session::pm_session() && (caller == fg || session::is_windowed(caller)))
}

fn env_value(env: &[String], key: &str) -> Option<String> {
    env.iter().find_map(|e| {
        let (k, v) = e.split_once('=')?;
        k.eq_ignore_ascii_case(key).then(|| v.to_string())
    })
}

/// Find a program: as given, with .EXE added, then along PATH.
fn find_program(c: &Ctx, name: &str, env: &[String]) -> Option<String> {
    let exists = |p: &str| super::file::resolve(c, p).ok().filter(|f| crate::fs::stat(f).is_ok()).map(|f| f.display());
    let has_ext = name.rsplit(['\\', '/']).next().is_some_and(|b| b.contains('.'));
    let candidates = |base: &str| {
        let mut v = alloc::vec![base.to_string()];
        if !has_ext {
            v.push(alloc::format!("{base}.EXE"));
        }
        v
    };
    for cand in candidates(name) {
        if let Some(p) = exists(&cand) {
            return Some(p);
        }
    }
    if name.contains(['\\', '/', ':']) {
        return None;
    }
    let path = env_value(env, "PATH").unwrap_or_default();
    for dir in path.split(';').filter(|d| !d.is_empty()) {
        for cand in candidates(&alloc::format!("{}\\{}", dir.trim_end_matches('\\'), name)) {
            if let Some(p) = exists(&cand) {
                return Some(p);
            }
        }
    }
    None
}

/// Start a session; returns (session id, pid, return code).
fn start(c: &Ctx, sd: StartData) -> Result<(u32, u32, u32), u32> {
    let env = sd.env.clone().unwrap_or_else(|| super::proc::parent_env(c));
    let comspec = env_value(&env, "COMSPEC").unwrap_or_else(|| String::from("C:\\OS2\\CMD.EXE"));
    // No program: a command processor. Batch files run under one too.
    let (name, inputs) = match &sd.name {
        None => (comspec.clone(), sd.inputs.clone()),
        Some(n) => {
            let upper = n.to_ascii_uppercase();
            if upper.ends_with(".CMD") || upper.ends_with(".BAT") {
                (comspec.clone(), alloc::format!("/C {} {}", n, sd.inputs))
            } else {
                (n.clone(), sd.inputs.clone())
            }
        }
    };
    let path = find_program(c, &name, &env).ok_or(err::FILE_NOT_FOUND)?;
    let program = path.rsplit('\\').next().unwrap_or(&path).to_string();
    let title = sd.title.clone().unwrap_or_else(|| program.clone());
    let caller_session = c.process.session.load(Ordering::Relaxed);
    let parent_pid = sd.related.then_some(c.process.pid);
    let app = crate::fs::read_file(&path).map_or(0, |d| super::module::app_type(&d));
    let mut windowed = wants_window(sd.session_type, app, caller_session);
    // The window needs the PM host; without it the session runs full
    // screen.
    if windowed && !crate::viowin::ensure_host(env.clone()) {
        windowed = false;
    }
    let sid = session::create(title, caller_session, parent_pid, sd.termq.clone());
    if windowed {
        session::make_windowed(sid);
    }
    let r = process::exec(process::ExecParams {
        path: &path,
        program: &program,
        args: &inputs,
        env,
        parent: Some(c.process.clone()),
        handles: super::file::console_handles(),
        detached: true,
        session: sid,
    });
    let child = match r {
        Ok(p) => p,
        Err(e) => {
            session::discard(sid);
            return Err(e);
        }
    };
    session::set_leader(sid, child.pid);
    let mut rc = 0;
    if windowed {
        crate::viowin::changed();
    }
    if !sd.background {
        if caller_in_front(caller_session) {
            session::select(sid);
        } else {
            rc = ERROR_SMG_START_IN_BACKGROUND;
        }
    }
    Ok((sid, child.pid, rc))
}

/// DosStartSession(pStartData, pulSessID, ppid)
pub fn dos_start_session(c: &mut Ctx) -> u32 {
    let (psd, psid, ppid) = if c.is16 { (c.ptr(0), c.ptr(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let sd = match read_start_data(c, psd) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let related = sd.related;
    match start(c, sd) {
        Ok((sid, pid, rc)) => {
            // The session ID is only meaningful for related sessions.
            let sid = if related { sid } else { 0 };
            if c.is16 {
                if psid != 0 {
                    c.write_u16(psid, sid as u16);
                }
                if ppid != 0 {
                    c.write_u16(ppid, pid as u16);
                }
            } else {
                if psid != 0 {
                    c.write_u32(psid, sid);
                }
                if ppid != 0 {
                    c.write_u32(ppid, pid);
                }
            }
            rc
        }
        Err(e) => e,
    }
}

/// A child session of the caller, by ID.
fn own_child(c: &Ctx, sid: u32) -> Result<u32, u32> {
    if sid == 0 {
        return Err(ERROR_SMG_INVALID_SESSION_ID);
    }
    if !session::exists(sid) {
        return Err(ERROR_SMG_SESSION_NOT_FOUND);
    }
    if session::parent_pid(sid) != Some(c.process.pid) {
        return Err(ERROR_SMG_SESSION_NOT_PARENT);
    }
    Ok(sid)
}

/// DosSelectSession(idSession): 0 = the caller's own session.
pub fn dos_select_session(c: &mut Ctx) -> u32 {
    let sid = c.arg(0) & if c.is16 { 0xffff } else { u32::MAX };
    let target = if sid == 0 {
        c.process.session.load(Ordering::Relaxed)
    } else {
        match own_child(c, sid) {
            Ok(s) => s,
            Err(e) => return e,
        }
    };
    // Only the foreground session may move the focus.
    let mine = c.process.session.load(Ordering::Relaxed);
    let fg = session::foreground();
    if fg != mine && fg != target && session::parent_pid(fg) != Some(c.process.pid) {
        return ERROR_SMG_INVALID_SELECT_OPT;
    }
    session::select(target);
    0
}

/// DosSetSession(idSession, pStatusData): selectability and bonding are
/// accepted but have no effect yet.
pub fn dos_set_session(c: &mut Ctx) -> u32 {
    let sid = c.arg(0) & if c.is16 { 0xffff } else { u32::MAX };
    match own_child(c, sid) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// DosStopSession(scope, idSession)
pub fn dos_stop_session(c: &mut Ctx) -> u32 {
    let (scope, sid) = (c.arg(0) & 0xffff, c.arg(1) & if c.is16 { 0xffff } else { u32::MAX });
    let targets = match scope {
        0 => match own_child(c, sid) {
            Ok(s) => alloc::vec![s],
            Err(e) => return e,
        },
        1 => session::children_of(c.process.pid),
        _ => return ERROR_SMG_INVALID_STOP_OPTION,
    };
    for sid in targets {
        for p in process::all() {
            if p.session.load(Ordering::Relaxed) == sid && p.exit_info().is_none() {
                p.exit(3, 0); // TC_KILLPROCESS
            }
        }
    }
    0
}
