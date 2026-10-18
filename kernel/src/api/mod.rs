//! The OS/2 API gate.
//!
//! Imports of kernel-implemented modules point at `int 0x99` stubs in the
//! gate object (see [`crate::loader::gate`]). This module identifies the
//! API from the stub address, gives the implementation access to its
//! arguments according to the calling convention, and emulates the return.
//!
//! * 32-bit `_System`: arguments at `ESP+4`, caller pops; result in EAX.
//! * 16-bit far `_Pascal`: arguments pushed left to right above the far
//!   return address, callee pops them; result in AX.

pub mod doscalls;
pub mod exceptions;
pub mod file;
pub mod kbdapi;
pub mod msg;
pub mod nls;
pub mod proc;
pub mod vio;
pub mod names;
mod names_gen;
mod sigs;
mod sigs_retf;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::sync::{SpinGuard, SpinLock};
use crate::task::thread::Thread;
use crate::task::{sched, Process};
use crate::vm::{self, AddressSpace};

/// Common OS/2 error codes.
pub mod err {
    pub const NO_ERROR: u32 = 0;
    pub const INVALID_FUNCTION: u32 = 1;
    pub const FILE_NOT_FOUND: u32 = 2;
    pub const PATH_NOT_FOUND: u32 = 3;
    pub const TOO_MANY_OPEN_FILES: u32 = 4;
    pub const ACCESS_DENIED: u32 = 5;
    pub const INVALID_HANDLE: u32 = 6;
    pub const NOT_ENOUGH_MEMORY: u32 = 8;
    pub const INVALID_ACCESS: u32 = 12;
    pub const INVALID_DATA: u32 = 13;
    pub const INVALID_DRIVE: u32 = 15;
    pub const CURRENT_DIRECTORY: u32 = 16;
    pub const NOT_SAME_DEVICE: u32 = 17;
    pub const NO_MORE_FILES: u32 = 18;
    pub const WRITE_PROTECT: u32 = 19;
    pub const NOT_READY: u32 = 21;
    pub const SHARING_VIOLATION: u32 = 32;
    pub const HANDLE_EOF: u32 = 38;
    pub const NOT_SUPPORTED: u32 = 50;
    pub const FILE_EXISTS: u32 = 80;
    pub const CANNOT_MAKE: u32 = 82;
    pub const INVALID_PARAMETER: u32 = 87;
    pub const INTERRUPT: u32 = 95;
    pub const SEM_IS_SET: u32 = 102;
    pub const TOO_MANY_SEM_REQUESTS: u32 = 103;
    pub const SEM_OWNER_DIED: u32 = 105;
    pub const DRIVE_LOCKED: u32 = 108;
    pub const BROKEN_PIPE: u32 = 109;
    pub const OPEN_FAILED: u32 = 110;
    pub const BUFFER_OVERFLOW: u32 = 111;
    pub const DISK_FULL: u32 = 112;
    pub const NO_MORE_SEARCH_HANDLES: u32 = 113;
    pub const INVALID_TARGET_HANDLE: u32 = 114;
    pub const INVALID_CATEGORY: u32 = 117;
    pub const INVALID_VERIFY_SWITCH: u32 = 118;
    pub const CALL_NOT_IMPLEMENTED: u32 = 120;
    pub const SEM_TIMEOUT: u32 = 121;
    pub const INSUFFICIENT_BUFFER: u32 = 122;
    pub const INVALID_NAME: u32 = 123;
    pub const INVALID_LEVEL: u32 = 124;
    pub const NO_VOLUME_LABEL: u32 = 125;
    pub const MOD_NOT_FOUND: u32 = 126;
    pub const PROC_NOT_FOUND: u32 = 127;
    pub const WAIT_NO_CHILDREN: u32 = 128;
    pub const CHILD_NOT_COMPLETE: u32 = 129;
    pub const NEGATIVE_SEEK: u32 = 131;
    pub const SEEK_ON_DEVICE: u32 = 132;
    pub const INVALID_SEGMENT_NUMBER: u32 = 180;
    pub const INVALID_ORDINAL: u32 = 182;
    pub const INVALID_FLAG_NUMBER: u32 = 186;
    pub const SEM_NOT_FOUND: u32 = 187;
    pub const INVALID_STARTING_CODESEG: u32 = 188;
    pub const INVALID_STACKSEG: u32 = 189;
    pub const ENVVAR_NOT_FOUND: u32 = 203;
    pub const NO_SIGNAL_SENT: u32 = 205;
    pub const FILENAME_EXCED_RANGE: u32 = 206;
    pub const META_EXPANSION_TOO_LONG: u32 = 208;
    pub const INVALID_SIGNAL_NUMBER: u32 = 209;
    pub const THREAD_1_INACTIVE: u32 = 210;
    pub const INVALID_EA_NAME: u32 = 254;
    pub const EA_LIST_INCONSISTENT: u32 = 255;
    pub const NO_MORE_ITEMS: u32 = 259;
    pub const SEARCH_STRUC_REUSED: u32 = 260;
    pub const EAS_DIDNT_FIT: u32 = 275;
    pub const EA_FILE_CORRUPT: u32 = 276;
    pub const EA_TABLE_FULL: u32 = 277;
    pub const INVALID_EA_HANDLE: u32 = 278;
    pub const EAS_NOT_SUPPORTED: u32 = 282;
    pub const NOT_OWNER: u32 = 288;
    pub const TOO_MANY_POSTS: u32 = 298;
    pub const ALREADY_POSTED: u32 = 299;
    pub const ALREADY_RESET: u32 = 300;
    pub const SEM_BUSY: u32 = 301;
    pub const INVALID_PROCID: u32 = 303;
    pub const INVALID_PDELTA: u32 = 304;
    pub const NOT_DESCENDANT: u32 = 305;
    pub const NOT_SESSION_MANAGER: u32 = 306;
    pub const INVALID_PCLASS: u32 = 307;
    pub const INVALID_SCOPE: u32 = 308;
    pub const INVALID_THREADID: u32 = 309;
    pub const DOSSUB_SHRINK: u32 = 310;
    pub const DOSSUB_NOMEM: u32 = 311;
    pub const DOSSUB_OVERLAP: u32 = 312;
    pub const DOSSUB_BADSIZE: u32 = 313;
    pub const DOSSUB_BADFLAG: u32 = 314;
    pub const DOSSUB_BADSELECTOR: u32 = 315;
    pub const MR_MSG_TOO_LONG: u32 = 316;
    pub const MR_MID_NOT_FOUND: u32 = 317;
    pub const MR_UN_ACC_MSGF: u32 = 318;
    pub const MR_INV_MSGF_FORMAT: u32 = 319;
    pub const MR_INV_IVCOUNT: u32 = 320;
    pub const MR_UN_PERFORM: u32 = 321;
    pub const TS_WAKEUP: u32 = 322;
    pub const TS_SEMHANDLE: u32 = 323;
    pub const TS_NOTIMER: u32 = 324;
    pub const TS_HANDLE: u32 = 326;
    pub const TS_DATETIME: u32 = 327;
    pub const SYS_INTERNAL: u32 = 328;
    pub const QUE_CURRENT_NAME: u32 = 329;
    pub const QUE_PROC_NOT_OWNED: u32 = 330;
    pub const QUE_PROC_OWNED: u32 = 331;
    pub const QUE_DUPLICATE: u32 = 332;
    pub const QUE_ELEMENT_NOT_EXIST: u32 = 333;
    pub const QUE_NO_MEMORY: u32 = 334;
    pub const QUE_INVALID_NAME: u32 = 335;
    pub const QUE_INVALID_PRIORITY: u32 = 336;
    pub const QUE_INVALID_HANDLE: u32 = 337;
    pub const QUE_LINK_NOT_FOUND: u32 = 338;
    pub const QUE_MEMORY_ERROR: u32 = 339;
    pub const QUE_PREV_AT_END: u32 = 340;
    pub const QUE_PROC_NO_ACCESS: u32 = 341;
    pub const QUE_EMPTY: u32 = 342;
    pub const QUE_NAME_NOT_EXIST: u32 = 343;
    pub const QUE_NOT_INITIALIZED: u32 = 344;
    pub const QUE_UNABLE_TO_ACCESS: u32 = 345;
    pub const QUE_UNABLE_TO_ADD: u32 = 346;
    pub const QUE_UNABLE_TO_INIT: u32 = 347;
    pub const VIO_INVALID_MASK: u32 = 349;
    pub const VIO_PTR: u32 = 350;
    pub const SMG_INVALID_SESSION_ID: u32 = 371;
    pub const SMG_START_IN_BACKGROUND: u32 = 457;
    pub const VIO_INVALID_HANDLE: u32 = 436;
    pub const VIO_INVALID_LENGTH: u32 = 438;
    pub const VIO_INVALID_PARMS: u32 = 421;
    pub const VIO_ROW: u32 = 358;
    pub const VIO_COL: u32 = 359;
    pub const KBD_INVALID_HANDLE: u32 = 447;
    pub const INVALID_ADDRESS: u32 = 487;
    pub const TIMEOUT: u32 = 640;
    pub const DUPLICATE_NAME: u32 = 285;
    pub const TOO_MANY_HANDLES: u32 = 290;
    pub const INVALID_HANDLE_SEM: u32 = 6;
    pub const SEM_OWNER: u32 = 105;
    pub const INTERRUPT_SEM: u32 = 95;
    pub const NOT_OWNER_SEM: u32 = 288;
}

/// The per-call context handed to API implementations.
pub struct Ctx<'a> {
    pub f: &'a mut TrapFrame,
    pub thread: Arc<Thread>,
    pub process: Arc<Process>,
    /// 16-bit `_Pascal` convention.
    pub is16: bool,
    /// Argument sizes, left to right (16-bit only).
    sig: &'static [u8],
    /// Linear address of the first argument byte above the return address.
    args_at: u32,
    /// Set by implementations that already arranged the return (e.g. they
    /// switched the thread to a new context).
    pub no_return: bool,
    /// Override for the value placed in DX (16-bit APIs returning ULONG).
    pub dx: Option<u16>,
}

pub type Handler = fn(&mut Ctx) -> u32;

impl Ctx<'_> {
    pub fn space(&self) -> SpinGuard<'_, AddressSpace> {
        self.process.space.lock()
    }

    /// Raw argument `i` (0 = leftmost).
    pub fn arg(&self, i: usize) -> u32 {
        if self.is16 {
            let Some(&size) = self.sig.get(i) else { return 0 };
            let off: u32 = self.sig[i + 1..].iter().map(|&s| sigs::size(s)).sum();
            let at = self.args_at + off;
            if sigs::size(size) == 2 { self.read_u16(at).unwrap_or(0) as u32 } else { self.read_u32(at).unwrap_or(0) }
        } else {
            self.read_u32(self.args_at + 4 * i as u32).unwrap_or(0)
        }
    }

    /// Argument `i` as a pointer, converted to a flat address (16:16 for
    /// 16-bit APIs). 0 stays 0.
    pub fn ptr(&self, i: usize) -> u32 {
        let v = self.arg(i);
        if self.is16 { self.far16_to_flat(v) } else { v }
    }

    /// Convert a 16:16 pointer to a flat address.
    pub fn far16_to_flat(&self, v: u32) -> u32 {
        if v == 0 {
            return 0;
        }
        let sel = (v >> 16) as u16;
        let off = v & 0xffff;
        self.space().linear(sel, off).unwrap_or(0xffff_ffff)
    }

    pub fn read(&self, addr: u32, out: &mut [u8]) -> bool {
        addr != 0 && self.space().read_bytes(addr, out)
    }

    pub fn write(&self, addr: u32, data: &[u8]) -> bool {
        addr != 0 && self.space().write_bytes(addr, data)
    }

    pub fn read_u8(&self, addr: u32) -> Option<u8> {
        let mut b = [0u8; 1];
        self.read(addr, &mut b).then_some(b[0])
    }

    pub fn read_u16(&self, addr: u32) -> Option<u16> {
        let mut b = [0u8; 2];
        self.read(addr, &mut b).then(|| u16::from_le_bytes(b))
    }

    pub fn read_u32(&self, addr: u32) -> Option<u32> {
        let mut b = [0u8; 4];
        self.read(addr, &mut b).then(|| u32::from_le_bytes(b))
    }

    pub fn write_u8(&self, addr: u32, v: u8) -> bool {
        self.write(addr, &[v])
    }

    pub fn write_u16(&self, addr: u32, v: u16) -> bool {
        self.write(addr, &v.to_le_bytes())
    }

    pub fn write_u32(&self, addr: u32, v: u32) -> bool {
        self.write(addr, &v.to_le_bytes())
    }

    pub fn read_vec(&self, addr: u32, len: u32) -> Option<Vec<u8>> {
        let mut v = alloc::vec![0u8; len as usize];
        self.read(addr, &mut v).then_some(v)
    }

    /// Read a NUL-terminated string (at most `max` bytes).
    pub fn read_str(&self, addr: u32, max: usize) -> Option<String> {
        if addr == 0 {
            return None;
        }
        let mut s = Vec::new();
        let mut a = addr;
        while s.len() < max {
            let b = self.read_u8(a)?;
            if b == 0 {
                break;
            }
            s.push(b);
            a += 1;
        }
        Some(s.iter().map(|&b| b as char).collect())
    }

    /// Write a NUL-terminated string.
    pub fn write_str(&self, addr: u32, s: &str) -> bool {
        let mut v: Vec<u8> = s.bytes().collect();
        v.push(0);
        self.write(addr, &v)
    }
}

/// True if a gate entry uses the 16-bit convention.
pub fn is_16bit(module: u32, ordinal: u32) -> bool {
    names::is16(module as u8, ordinal as u16)
}

/// Log every API call to the serial port (`apitrace` on the command line).
pub static TRACE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

static WARNED: SpinLock<Vec<(u32, u32)>> = SpinLock::new(Vec::new());

/// Linear address of `cs:rip`.
fn code_linear(space: &AddressSpace, cs: u16, rip: u64) -> u32 {
    space.linear(cs, rip as u32).unwrap_or(rip as u32)
}

pub fn dispatch(f: &mut TrapFrame) {
    let thread = sched::current();
    let Some(process) = thread.process() else { return };
    // The trapping instruction is the 2-byte `int 0x99` before RIP.
    let (linear, ss_big, stack_linear) = {
        let space = process.space.lock();
        let lin = code_linear(&space, f.cs as u16, f.rip.wrapping_sub(2));
        let dss = space.descriptor(f.ss as u16);
        let big = vm::desc_big(dss);
        let sp = if big { f.rsp as u32 } else { f.rsp as u32 & 0xffff };
        (lin, big, vm::desc_base(dss).wrapping_add(sp))
    };
    let Some((module, ordinal)) = gate::decode(linear) else {
        // int 0x99 executed outside the gate: treat as a protection fault.
        f.vector = 13;
        f.error = 0x99 * 8 + 2;
        exceptions::deliver(f, &process, &thread);
        return;
    };

    if module == 0 && ordinal == 0 {
        // A 32-bit program returned from its entry point.
        let code = f.rax as u32;
        process.exit(0, code);
        return;
    }
    if module == 0 && ordinal == proc::THREAD_EXIT_ORDINAL {
        // A 32-bit thread function returned.
        let mut ctx = Ctx {
            f,
            thread: thread.clone(),
            process: process.clone(),
            is16: false,
            sig: &[],
            args_at: stack_linear + 4,
            no_return: true,
            dx: None,
        };
        proc::thread_returned(&mut ctx);
        return;
    }
    let (raw_module, raw_ordinal) = (module, ordinal);
    let (m8, o16) = names::canonical(module as u8, ordinal as u16);
    let (module, ordinal) = (m8 as u32, o16 as u32);

    let is16 = names::is16(module as u8, ordinal as u16);
    const WORDS: &[u8] = b"wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww";
    let sig: &'static [u8] = if is16 {
        match sigs::lookup(module as u8, ordinal as u16) {
            Some(s) => s,
            // Untyped fallback: right size, so the return is correct.
            None => match (raw_module, sigs::retf_bytes(raw_ordinal as u16)) {
                (0, Some(n)) => &WORDS[..(n as usize / 2).min(WORDS.len())],
                _ => &[],
            },
        }
    } else {
        &[]
    };
    let measured = raw_module == 0 && sigs::retf_bytes(raw_ordinal as u16).is_some();
    if is16 && sigs::lookup(module as u8, ordinal as u16).is_none() && !measured {
        let name = names::display(module as u8, ordinal as u16);
        crate::println!(
            "SYS3175: {} called 16-bit {}.{} ({}) which has no known signature",
            process.exe.name,
            gate::MODULES[module as usize],
            ordinal,
            name
        );
        process.exit(2, 0xffff);
        return;
    }

    let mut ctx = Ctx {
        f,
        thread: thread.clone(),
        process: process.clone(),
        is16,
        sig,
        args_at: stack_linear + 4,
        no_return: false,
        dx: None,
    };
    let trace = TRACE.load(core::sync::atomic::Ordering::Relaxed);
    if trace {
        crate::trace!(
            "[{}:{}] {}.{} {}",
            process.pid,
            thread.tid,
            gate::MODULES[module as usize],
            ordinal,
            names::display(module as u8, ordinal as u16)
        );
    }
    let result = match doscalls::handler(module as u8, ordinal as u16) {
        Some(h) => h(&mut ctx),
        None => {
            let mut w = WARNED.lock();
            if !w.contains(&(module, ordinal)) {
                w.push((module, ordinal));
                crate::println!(
                    "[unimplemented API {}.{} {} called by {}]",
                    gate::MODULES[module as usize],
                    ordinal,
                    names::display(module as u8, ordinal as u16),
                    process.exe.name
                );
            }
            err::CALL_NOT_IMPLEMENTED
        }
    };
    if trace {
        crate::trace!("    -> {}", result);
    }
    if ctx.no_return || thread.exit_requested() {
        return;
    }
    let dx = ctx.dx;
    let f = ctx.f;
    // Emulate the return.
    if is16 {
        let ret = {
            let mut b = [0u8; 4];
            process.space.lock().read_bytes(stack_linear, &mut b);
            u32::from_le_bytes(b)
        };
        let argbytes: u32 = sig.iter().map(|&s| sigs::size(s)).sum();
        f.rip = (ret & 0xffff) as u64;
        f.cs = (ret >> 16) as u64;
        let sp = (f.rsp as u32 & 0xffff).wrapping_add(4 + argbytes) & 0xffff;
        f.rsp = (f.rsp & !0xffff) | sp as u64;
        f.rax = (f.rax & !0xffff) | (result & 0xffff) as u64;
        if let Some(dx) = dx {
            f.rdx = (f.rdx & !0xffff) | dx as u64;
        }
        let _ = ss_big;
    } else {
        let ret = {
            let mut b = [0u8; 4];
            process.space.lock().read_bytes(stack_linear, &mut b);
            u32::from_le_bytes(b)
        };
        f.rip = ret as u64;
        f.rsp = (f.rsp as u32).wrapping_add(4) as u64;
        f.rax = result as u64;
    }
}
