//! Replacement subsystems: VioRegister, KbdRegister, MouRegister (and
//! VioGlobalReg) route chosen VIO/KBD/MOU functions of a session to an
//! entry point in a DLL.
//!
//! The entry is called far, on the caller's stack, which holds (from SP
//! up): the far return address into the router, the function index (the
//! bit number in the registration masks), the caller's DS, the caller's
//! far return address and the original parameters. It returns with RETF
//! (popping nothing) with AX = -1 to have the base subsystem do the call,
//! or any other AX as the call's result. Calls made while a thread is inside a replacement go
//! to the base subsystem.

use alloc::string::String;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::sync::SpinLock;

/// Gate ordinal the replacement returns to (in the module's own slots).
pub const RETURN_ORDINAL: u32 = 2000;

const ERROR_VIO_REGISTER: u32 = 426;
const ERROR_KBD_REGISTER: u32 = 410;
const ERROR_KBD_DEREGISTER: u32 = 411;
const ERROR_VIO_DEREGISTER: u32 = 404;
const ERROR_MOUSE_REGISTER: u32 = 415;
const ERROR_MOU_DEREGISTER: u32 = 416;

struct Reg {
    module: u8,
    /// Session, or 0 for VioGlobalReg.
    sid: u32,
    pid: u32,
    entry: u32,
    masks: [u32; 2],
    _name: String,
}

struct Active {
    thread: u32,
    frame: TrapFrame,
}

static REGS: SpinLock<Vec<Reg>> = SpinLock::new(Vec::new());
static ACTIVE: SpinLock<Vec<Active>> = SpinLock::new(Vec::new());

/// VIO functions in VR_* bit order (mask 1, then mask 2).
const VIO_ORDER: [u16; 41] = [
    9, 27, 21, 31, 2, 15, 32, 22, 43, 30, 24, 53, 26, 52, 19, 13, 48, 10, 7, 47, 44, 12, 5, 3, 8, 23, 18, 25, 28, 11, 1, 50, // mask 1
    37, 35, 29, 46, 42, 40, 33, 49, 51, // mask 2
];
/// KBD functions in KR_* bit order.
const KBD_ORDER: [u16; 14] = [4, 22, 13, 10, 11, 9, 23, 17, 12, 18, 3, 5, 14, 1];
/// MOU functions in MR_* bit order (bits 9 and 10 are the hot key calls).
const MOU_ORDER: [u16; 21] = [8, 3, 22, 13, 20, 6, 15, 11, 16, 4, 10, 17, 9, 1, 2, 26, 18, 19, 21, 27, 25];

fn index(module: u8, ordinal: u16) -> Option<u32> {
    let t: &[u16] = match module {
        1 => &VIO_ORDER,
        2 => &KBD_ORDER,
        3 => &MOU_ORDER,
        _ => return None,
    };
    t.iter().position(|&o| o == ordinal).map(|i| i as u32)
}

fn in_replacement(thread: u32) -> bool {
    ACTIVE.lock().iter().any(|a| a.thread == thread)
}

/// Route a VIO/KBD/MOU call to a registered replacement. Returns true if
/// the thread now runs the replacement.
pub fn route(c: &mut Ctx, module: u8, ordinal: u16) -> bool {
    let Some(idx) = index(module, ordinal) else { return false };
    if REGS.lock().is_empty() || in_replacement(c.thread.id) {
        return false;
    }
    let sid = crate::session::current();
    let entry = {
        let regs = REGS.lock();
        let bit = |r: &Reg| r.masks[(idx / 32) as usize] & 1 << (idx % 32) != 0;
        regs.iter().find(|r| r.module == module && r.sid == sid && bit(r)).or_else(|| regs.iter().find(|r| r.module == module && r.sid == 0 && bit(r))).map(|r| r.entry)
    };
    let Some(entry) = entry else { return false };
    let saved = *c.f;
    let base = c.args_base() - 4;
    let big = {
        let sp = c.process.space.lock();
        crate::vm::desc_big(sp.descriptor(saved.ss as u16))
    };
    // Below the caller's far return address: DS, index, our return.
    let ret = gate::slot_address(module as u32, RETURN_ORDINAL);
    let ret16 = (crate::vm::tile_selector(ret) as u32) << 16 | (ret & 0xffff);
    let mut b = Vec::new();
    b.extend_from_slice(&ret16.to_le_bytes());
    b.extend_from_slice(&(idx as u16).to_le_bytes());
    b.extend_from_slice(&(saved.ds as u16).to_le_bytes());
    if !c.process.space.lock().poke_bytes(base - 8, &b) {
        return false;
    }
    let f = &mut *c.f;
    if big {
        f.rsp = (f.rsp as u32).wrapping_sub(8) as u64;
    } else {
        let sp = (f.rsp as u32 & 0xffff).wrapping_sub(8) & 0xffff;
        f.rsp = (f.rsp & !0xffff) | sp as u64;
    }
    f.cs = (entry >> 16) as u64;
    f.rip = (entry & 0xffff) as u64;
    ACTIVE.lock().push(Active { thread: c.thread.id, frame: saved });
    c.no_return = true;
    true
}

/// The replacement returned into the router (gate slot RETURN_ORDINAL).
pub fn returned(f: &mut TrapFrame, thread: u32) {
    let saved = {
        let mut a = ACTIVE.lock();
        match a.iter().rposition(|x| x.thread == thread) {
            Some(i) => a.remove(i),
            None => return,
        }
    };
    let ax = f.rax as u16;
    *f = saved.frame;
    // The original call again: through the base subsystem (AX = -1), or
    // completed with the replacement's AX.
    super::dispatch_with(f, false, if ax == 0xffff { None } else { Some(ax as u32) });
}

fn register(c: &mut Ctx, module: u8, name: u32, entry_name: u32, masks: [u32; 2], global: bool) -> u32 {
    let fail = match module {
        1 => ERROR_VIO_REGISTER,
        2 => ERROR_KBD_REGISTER,
        _ => ERROR_MOUSE_REGISTER,
    };
    let (Some(m), Some(e)) = (c.read_str(name, 260), c.read_str(entry_name, 256)) else { return err::INVALID_ADDRESS };
    let sid = if global { 0 } else { crate::session::current() };
    if REGS.lock().iter().any(|r| r.module == module && r.sid == sid) {
        return fail;
    }
    let r = {
        let mut sp = c.space();
        crate::loader::load_dll(&m, &mut sp)
    };
    let Ok(module_obj) = r else { return fail };
    let Ok(t) = module_obj.resolve_name(&e, 0) else { return fail };
    // A 32-bit export is entered through the 16:16 alias of its object.
    let entry = if t.is16 { (t.sel as u32) << 16 | t.off as u32 } else { (crate::vm::tile_selector(t.flat) as u32) << 16 | (t.flat & 0xffff) };
    REGS.lock().push(Reg { module, sid, pid: c.process.pid, entry, masks, _name: m });
    0
}

fn deregister(c: &Ctx, module: u8) -> u32 {
    let sid = crate::session::current();
    let mut regs = REGS.lock();
    let n = regs.len();
    regs.retain(|r| !(r.module == module && r.sid == sid && r.pid == c.process.pid));
    if regs.len() == n {
        return match module {
            1 => ERROR_VIO_DEREGISTER,
            2 => ERROR_KBD_DEREGISTER,
            _ => ERROR_MOU_DEREGISTER,
        };
    }
    0
}

/// VioRegister(pszModName, pszEntryName, flFun1, flFun2).
pub fn vio_register(c: &mut Ctx) -> u32 {
    let (m, e, f1, f2) = (c.ptr(0), c.ptr(1), c.arg(2), c.arg(3));
    register(c, 1, m, e, [f1, f2], false)
}

/// VioGlobalReg(pszModName, pszEntryName, flFun1, flFun2, usReturn): for
/// every session.
pub fn vio_global_reg(c: &mut Ctx) -> u32 {
    let (m, e, f1, f2) = (c.ptr(0), c.ptr(1), c.arg(2), c.arg(3));
    register(c, 1, m, e, [f1, f2], true)
}

pub fn vio_deregister(c: &mut Ctx) -> u32 {
    deregister(c, 1)
}

/// KbdRegister(pszModName, pszEntryPt, FunMask).
pub fn kbd_register(c: &mut Ctx) -> u32 {
    let (m, e, f) = (c.ptr(0), c.ptr(1), c.arg(2));
    register(c, 2, m, e, [f, 0], false)
}

pub fn kbd_deregister(c: &mut Ctx) -> u32 {
    deregister(c, 2)
}

/// MouRegister(pszModName, pszEntryName, flFuns).
pub fn mou_register(c: &mut Ctx) -> u32 {
    let (m, e, f) = (c.ptr(0), c.ptr(1), c.arg(2));
    register(c, 3, m, e, [f, 0], false)
}

pub fn mou_deregister(c: &mut Ctx) -> u32 {
    deregister(c, 3)
}

/// A process ended: its registrations go.
pub fn process_ended(pid: u32) {
    REGS.lock().retain(|r| r.pid != pid);
}
