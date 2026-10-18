//! THK16_UNITHUNK (DOSCALLS.576): the generic 16-to-32 thunk of DOSCALL1.
//!
//! Register based: EAX is the flat address of a 32-bit `_System` function,
//! EDX describes the 16-bit `_Pascal` arguments on the caller's stack: the
//! low 4 bits are their count, then two bits per argument starting with
//! the one nearest the return address (00 = dword, 01 = 16:16 pointer
//! converted to flat, 10 = word zero-extended, 11 = word sign-extended).
//! The function runs on a flat view of the caller's stack and returns
//! through gate slot (0, 19); its EAX comes back in DX:AX and the
//! arguments are popped as by `RETF n`.

use alloc::vec::Vec;

use super::Ctx;
use crate::arch::gdt::{TIB_SEL, USER_CS, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::sync::SpinLock;

pub const UNITHUNK_RETURN_ORDINAL: u32 = 19;

struct Saved {
    thread: u32,
    frame: TrapFrame,
    /// Far return address and argument bytes of the 16-bit call.
    ret: u32,
    argbytes: u32,
}

static SAVED: SpinLock<Vec<Saved>> = SpinLock::new(Vec::new());

pub fn thk16_unithunk(c: &mut Ctx) -> u32 {
    let target = c.f.rax as u32;
    let desc = c.f.rdx as u32;
    if super::tracing() {
        crate::trace!("    unithunk target {target:x} desc {desc:x}");
    }
    let count = desc & 0xf;
    if count == 0xf {
        // Variable argument form: not seen yet.
        return super::err::INVALID_PARAMETER;
    }
    let base = c.args_base();
    let ret = c.read_u32(base - 4).unwrap_or(0);
    let mut args = Vec::new();
    let mut at = base;
    let mut kinds = desc >> 4;
    for _ in 0..count {
        let v = match kinds & 3 {
            0 => {
                let v = c.read_u32(at).unwrap_or(0);
                at += 4;
                v
            }
            1 => {
                let v = c.read_u32(at).unwrap_or(0);
                at += 4;
                c.far16_to_flat(v)
            }
            2 => {
                let v = c.read_u16(at).unwrap_or(0) as u32;
                at += 2;
                v
            }
            _ => {
                let v = c.read_u16(at).unwrap_or(0) as i16 as i32 as u32;
                at += 2;
                v
            }
        };
        args.push(v);
        kinds >>= 2;
    }
    let argbytes = at - base;
    // Flat stack below the 16-bit frame: arguments as read (the nearest
    // first ends up deepest, i.e. last in _System order), then the return
    // into the gate.
    let mut esp = (base - 4 - 64) & !3;
    for v in args.iter() {
        esp -= 4;
        c.write_u32(esp, *v);
    }
    esp -= 4;
    c.write_u32(esp, gate::slot_address(0, UNITHUNK_RETURN_ORDINAL));
    SAVED.lock().push(Saved { thread: c.thread.id, frame: *c.f, ret, argbytes });
    let f = &mut *c.f;
    f.cs = USER_CS as u64;
    f.rip = target as u64;
    f.ss = USER_DS as u64;
    f.rsp = esp as u64;
    f.ds = USER_DS as u64;
    f.es = USER_DS as u64;
    f.fs = TIB_SEL as u64;
    c.no_return = true;
    0
}

/// The 32-bit function returned into gate slot (0, 19).
pub fn returned(f: &mut TrapFrame, thread_id: u32) {
    let saved = {
        let mut s = SAVED.lock();
        match s.iter().rposition(|x| x.thread == thread_id) {
            Some(i) => s.remove(i),
            None => return,
        }
    };
    let result = f.rax as u32;
    *f = saved.frame;
    f.rip = (saved.ret & 0xffff) as u64;
    f.cs = (saved.ret >> 16) as u64;
    let sp = (f.rsp as u32 & 0xffff).wrapping_add(4 + saved.argbytes) & 0xffff;
    f.rsp = (f.rsp & !0xffff) | sp as u64;
    f.rax = result as u64;
    f.rdx = (f.rdx & !0xffff) | (result >> 16) as u64;
}
