//! RS2API services for pseudo terminal sessions (crate::pty). All 32-bit
//! `_System`.
//!
//! | Ordinal | Call |
//! |---------|------|
//! | 18 | `Rs2PtyStart(PSZ pszProgram, PSZ pszArgs, PSZ pszEnv, ULONG ulSize, PULONG psid, PULONG ppid)` |
//! | 19 | `Rs2PtyWait(ULONG sid, PULONG pulChanges, ULONG ulTimeout)` |
//!
//! The session's screen is read with Rs2VioReadScreen (12), keys go in
//! with Rs2VioKey (14) and Rs2VioControl(sid, 3, 0) (stop) ends it.

use alloc::string::ToString;
use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::session;
use crate::task::process;

const ERROR_TIMEOUT: u32 = 640;
const ERROR_SMG_SESSION_NOT_FOUND: u32 = 371;

/// Rs2PtyStart(pszProgram, pszArgs, pszEnv, ulSize, psid, ppid): start a
/// program (found as DosStartSession finds it) in a new pseudo terminal
/// session of LOWORD(ulSize) columns and HIWORD(ulSize) rows (0: 80x25)
/// with the console as its standard handles. pszArgs is the argument
/// string (may be NULL); pszEnv an environment block (NULL: the
/// caller's). The session is unrelated and stays in the background.
pub fn start(c: &mut Ctx) -> u32 {
    let (pprog, pargs, penv, size, psid, ppid) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5));
    let Some(name) = c.read_str(pprog, 260) else { return err::INVALID_ADDRESS };
    let args = if pargs == 0 { alloc::string::String::new() } else { c.read_str(pargs, 4096).unwrap_or_default() };
    let env = if penv == 0 { super::proc::parent_env(c) } else { super::proc::read_block(c, penv, 32768).unwrap_or_default() };
    let Some(path) = super::sesmgr::find_program(c, &name, &env) else { return err::FILE_NOT_FOUND };
    let program = path.rsplit('\\').next().unwrap_or(&path).to_string();
    let (cols, rows) = match (size & 0xffff, size >> 16) {
        (0, _) | (_, 0) => (80, 25),
        (w, h) => (w as usize, h as usize),
    };
    let caller = c.process.session.load(Ordering::Relaxed);
    let sid = session::create(program.clone(), caller, None, None);
    session::make_pty(sid, cols, rows);
    let r = process::exec(process::ExecParams {
        path: &path,
        program: &program,
        args: &args,
        env,
        parent: Some(c.process.clone()),
        handles: super::file::console_handles(),
        detached: true,
        session: sid,
        traced: false,
    });
    let child = match r {
        Ok(p) => p,
        Err(e) => {
            session::discard(sid);
            return e;
        }
    };
    session::set_leader(sid, child.pid);
    if psid != 0 {
        c.write_u32(psid, sid);
    }
    if ppid != 0 {
        c.write_u32(ppid, child.pid);
    }
    0
}

/// Rs2PtyWait(sid, pulChanges, ulTimeout): wait until the screen of pty
/// session `sid` changed since its change count *pulChanges (ms, -1 =
/// forever); stores the new count. ERROR_SMG_SESSION_NOT_FOUND once the
/// session ended.
pub fn wait(c: &mut Ctx) -> u32 {
    let (sid, p, ms) = (c.arg(0), c.arg(1), c.arg(2));
    let Some(seen) = c.read_u32(p) else { return err::INVALID_ADDRESS };
    let now = || if session::is_pty(sid) { crate::screen::with_session(sid, |s| s.changes) } else { None };
    let Some(cur) = now() else { return ERROR_SMG_SESSION_NOT_FOUND };
    if cur != seen {
        c.write_u32(p, cur);
        return 0;
    }
    if ms == 0 {
        return ERROR_TIMEOUT;
    }
    let deadline = (ms != u32::MAX).then(|| crate::irq::ticks() + ms as u64 * crate::irq::HZ / 1000);
    loop {
        let g = crate::pty::generation();
        if now() != Some(seen) || !crate::pty::wait(deadline, g) {
            break;
        }
    }
    match now() {
        None => ERROR_SMG_SESSION_NOT_FOUND,
        Some(g) if g != seen => {
            c.write_u32(p, g);
            0
        }
        _ if c.thread.exit_requested() => err::INTERRUPT,
        _ => ERROR_TIMEOUT,
    }
}
