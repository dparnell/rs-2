//! Kernel APIs called from 64-bit code (docs/API64.md).
//!
//! 64-bit programs call the same gate stubs as 32-bit ones, from long mode,
//! with the System V convention: arguments in RDI, RSI, RDX, RCX, R8, R9,
//! then on the stack above the return address; the result in RAX.
//!
//! In the 64-bit API ULONG stays 32 bits and pointers are 64 bits, so a
//! call whose arguments and buffers hold no pointers runs the 32-bit
//! implementation unchanged (`Ctx` reads the argument registers). Calls
//! that return a pointer through an argument (`DosAllocMem`'s PPVOID) run
//! it too and get the pointer's upper half written as 0 (phase 1: all of a
//! 64-bit program's memory is below 4 GB). Calls with pointers inside their
//! structures have their own versions here, or are not available yet:
//! they return ERROR_CALL_NOT_IMPLEMENTED.

use super::{err, Ctx, Handler};
use crate::arch::gdt::{TIB_SEL, USER_CS64, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::task::process::{self, info64_base, TIB64_0};
use crate::vm::{self, pag, SegKind};

/// DOSCALLS entries whose 32-bit implementation is right for 64-bit
/// callers as it is.
const PLAIN: &[u16] = &[
    // Files and directories.
    110, 218, 219, 220, 221, 223, 224, 225, 226, 228, 239, 254, 255, 256, 257, 258, 259, 260, 261, 263, 264, 265, 269,
    270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 281, 282, 284, 285, 287, 372, 382, 209, 210, 222, 428, 639, 981,
    986, 988, 989,
    // Named pipes.
    240, 241, 242, 243, 244, 245, 248, 249, 250, 251, 252, 253,
    // Memory.
    304, 305, 306, 302, 303, 344, 346, 347, 455,
    // Semaphores (event, mutex).
    324, 325, 326, 327, 328, 329, 330, 331, 332, 333, 334, 335, 336,
    // Processes and threads.
    111, 113, 229, 232, 233, 234, 235, 236, 237, 238, 280, 283, 349, 379, 380, 381, 415, 460,
    // Modules.
    318, 319, 320, 322, 323, 353, 572, 586, 873, 874,
    // Time, information, national language support.
    212, 230, 286, 289, 290, 291, 292, 348, 350, 351, 362, 363, 395, 396, 397, 398,
    // Queues.
    661, 662, 663, 664, 665, 666, 667, 668,
];

/// Entries that return a pointer through argument `n`: the 32-bit
/// implementation writes its low half, the high half is set to 0.
const PTR_OUT: &[(u16, usize)] = &[
    (227, 1), // DosScanEnv(psz, ppszValue)
    (299, 0), // DosAllocMem(ppb, cb, fl)
    (300, 0), // DosAllocSharedMem(ppb, psz, cb, fl)
    (301, 0), // DosGetNamedSharedMem(ppb, psz, fl)
    (321, 3), // DosQueryProcAddr(hmod, ord, psz, ppfn)
    (345, 1), // DosSubAllocMem(pbBase, ppb, cb)
    (352, 3), // DosGetResource(hmod, type, name, ppb)
    (454, 1), // DosAllocThreadLocalMemory(cb, ppul)
    (661, 3), // DosReadQueue(hq, preq, pcb, ppbuf, ...)
    (665, 3), // DosPeekQueue(hq, preq, pcb, ppbuf, ...)
];

/// The handler for a 64-bit call of DOSCALLS `ordinal` (other modules:
/// none yet), or None if 64-bit programs cannot call it.
pub fn handler(module: u8, ordinal: u16) -> Option<Handler> {
    if module != 0 {
        return None;
    }
    match ordinal {
        311 => return Some(dos_create_thread64),
        318 => return Some(dos_load_module64),
        354 => return Some(super::exceptions64::dos_set_exception_handler),
        355 => return Some(super::exceptions64::dos_unset_exception_handler),
        356 => return Some(super::exceptions64::dos_raise_exception),
        357 => return Some(super::exceptions64::dos_unwind_exception),
        LOAD_MODULE32 => return Some(super::module::dos_load_module),
        312 => return Some(dos_get_info_blocks64),
        _ => {}
    }
    if PLAIN.contains(&ordinal) || PTR_OUT.iter().any(|p| p.0 == ordinal) {
        return super::doscalls::handler(module, ordinal);
    }
    None
}

/// Arguments that would need a 64-bit structure the 32-bit code does not
/// know (extended attribute lists hold pointers): refused for now.
pub fn refused(c: &Ctx, module: u8, ordinal: u16) -> Option<u32> {
    if module != 0 {
        return None;
    }
    let bad = match ordinal {
        // DosOpen / DosOpenL: peaop2.
        273 | 981 => c.arg64(7) != 0,
        // DosCreateDir: peaop2.
        270 => c.arg64(1) != 0,
        // DosQueryPathInfo / DosQueryFileInfo / DosFindFirst level 3
        // (FIL_QUERYEASFROMLIST), DosSetPathInfo / DosSetFileInfo level 2.
        223 | 279 => c.arg(1) == 3,
        264 => c.arg(5) == 3,
        219 | 218 => c.arg(1) == 2,
        _ => false,
    };
    bad.then_some(err::INVALID_PARAMETER)
}

/// rs2's private DOSCALLS entry `Dos64LoadModule32` (same arguments as
/// DosLoadModule): load a 32-bit DLL into a 64-bit process, for the 64-bit
/// DLLs that call 32-bit ones (the 64-bit PMWIN).
pub const LOAD_MODULE32: u16 = 2040;

/// DosLoadModule from 64-bit code: a 64-bit DLL of that name, else a
/// 32-bit one (whose procedures 64-bit code calls through thunks).
fn dos_load_module64(c: &mut Ctx) -> u32 {
    let (fail, cbfail, pname, phmod) = (c.ptr(0), c.arg(1), c.ptr(2), c.ptr(3));
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    let loading = crate::loader::lock();
    let m = match crate::loader::load_global64(&name) {
        Ok(m) if m.format == crate::loader::Format::Elf64 => m,
        _ => {
            drop(loading);
            return super::module::dos_load_module(c);
        }
    };
    if let Err(e) = crate::loader::attach(&m, &mut c.space()) {
        if fail != 0 && cbfail > 0 {
            let n = (cbfail as usize - 1).min(m.name.len());
            c.write_str(fail, &m.name[..n]);
        }
        return e;
    }
    {
        let mut l = c.process.loaded.lock();
        match l.iter_mut().find(|(x, _)| alloc::sync::Arc::ptr_eq(x, &m)) {
            Some(e) => e.1 += 1,
            None => l.push((m.clone(), 1)),
        }
    }
    drop(loading);
    if !c.write_u32(phmod, m.hmte as u32) {
        return err::INVALID_ADDRESS;
    }
    0
}

/// After a successful call: widen pointers returned through arguments.
pub fn after(c: &Ctx, module: u8, ordinal: u16) {
    if module != 0 {
        return;
    }
    if let Some(&(_, n)) = PTR_OUT.iter().find(|p| p.0 == ordinal) {
        let at = c.ptr(n);
        if at != 0 {
            c.write_u32(at + 4, 0);
        }
    }
}

/// DosCreateThread(ptid, pfn, param, flag, cbStack): pfn(param) in long
/// mode, param 64 bits wide.
fn dos_create_thread64(c: &mut Ctx) -> u32 {
    let (ptid, pfn, param, flag, cb) = (c.ptr(0), c.arg64(1), c.arg64(2), c.arg(3), c.arg(4));
    if pfn == 0 || pfn > u32::MAX as u64 {
        return err::INVALID_ADDRESS;
    }
    // At least 64 KB: 64-bit code has bigger frames.
    let size = cb.max(0x1_0000).next_multiple_of(vm::PAGE);
    let base = match c.space().alloc(size, pag::READ | pag::WRITE, SegKind::Data, None) {
        Ok(b) => b,
        Err(e) => return e.code(),
    };
    {
        let mut sp = c.space();
        if flag & 2 != 0 {
            let _ = sp.set_mem(base, size, pag::COMMIT | pag::READ | pag::WRITE);
        } else {
            let _ = sp.set_mem(base + size - vm::PAGE, vm::PAGE, pag::COMMIT | pag::READ | pag::WRITE);
            let _ = sp.set_mem(base + size - 2 * vm::PAGE, vm::PAGE, pag::COMMIT | pag::GUARD | pag::READ | pag::WRITE);
        }
    }
    let top = base + size;
    // Entered as if called from the thread exit stub.
    let sp = ((top - 64) & !15) - 8;
    let exit_stub = crate::loader::gate::slot_address(0, super::proc::THREAD_EXIT_ORDINAL) as u64;
    c.space().poke_bytes(sp, &exit_stub.to_le_bytes());
    let frame = TrapFrame {
        rflags: 0x202,
        cs: USER_CS64 as u64,
        rip: pfn,
        ss: USER_DS as u64,
        rsp: sp as u64,
        ds: USER_DS as u64,
        es: USER_DS as u64,
        fs: TIB_SEL as u64,
        rdi: param,
        ..Default::default()
    };
    let r = super::proc::start_thread(c, frame, base, top, ptid, flag & 1 != 0, false, base);
    if r != 0 {
        let _ = c.space().free(base);
    }
    r
}

/// DosGetInfoBlocks(pptib, pppib): the 64-bit TIB and PIB.
fn dos_get_info_blocks64(c: &mut Ctx) -> u32 {
    let (ptib, ppib) = (c.ptr(0), c.ptr(1));
    let base = info64_base();
    if ptib != 0 {
        let tib = (base + TIB64_0 + c.thread.tid * process::info::TIB_STRIDE) as u64;
        c.write(ptib, &tib.to_le_bytes());
    }
    if ppib != 0 {
        c.write(ppib, &(base as u64).to_le_bytes());
    }
    0
}
