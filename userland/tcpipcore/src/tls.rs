//! Per-thread state: sock_errno, h_errno and the static result buffers
//! (inet_ntoa, hostent, ...), one block per thread id.

use alloc::boxed::Box;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::netdb::{EntBuf, Hostent, Netent, Protoent, Servent};

const SLOTS: usize = 256;

pub struct ThreadData {
    pub errno: i32,
    pub h_errno: i32,
    pub ntoa: [u8; 20],
    pub host: EntBuf<Hostent>,
    pub serv: EntBuf<Servent>,
    pub proto: EntBuf<Protoent>,
    pub net: EntBuf<Netent>,
    /// Scratch text (hstrerror, p_class, ...).
    pub text: [u8; 64],
}

static SLOT: [AtomicPtr<ThreadData>; SLOTS] = [const { AtomicPtr::new(null_mut()) }; SLOTS];

/// The calling thread's id (TIB2 through FS).
pub fn tid() -> u32 {
    let t: u32;
    unsafe {
        core::arch::asm!("mov {0}, dword ptr fs:[0x0c]", "mov {0}, dword ptr [{0}]", out(reg) t, options(nostack, readonly));
    }
    t
}

/// The calling thread's block.
pub fn get() -> &'static mut ThreadData {
    let slot = &SLOT[tid() as usize % SLOTS];
    let mut p = slot.load(Ordering::Acquire);
    if p.is_null() {
        let b = Box::into_raw(Box::new(ThreadData {
            errno: 0,
            h_errno: 0,
            ntoa: [0; 20],
            host: EntBuf::new(),
            serv: EntBuf::new(),
            proto: EntBuf::new(),
            net: EntBuf::new(),
            text: [0; 64],
        }));
        match slot.compare_exchange(null_mut(), b, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => p = b,
            Err(other) => {
                drop(unsafe { Box::from_raw(b) });
                p = other;
            }
        }
    }
    unsafe { &mut *p }
}
