//! The unwind tables of the modules this rs2libc serves: the program and
//! the DLLs that share its std. (64-bit.)

use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, Ordering};

use unwinding::custom_eh_frame_finder::{EhFrameFinder, FrameInfo, FrameInfoKind, set_custom_eh_frame_finder};

use crate::tls::Module;

const MAX: usize = 256;
static MODULES: [AtomicPtr<Module>; MAX] = [const { AtomicPtr::new(null_mut()) }; MAX];

struct Finder;

unsafe impl EhFrameFinder for Finder {
    fn find(&self, pc: usize) -> Option<FrameInfo> {
        MODULES.iter().find_map(|m| {
            let m = unsafe { m.load(Ordering::Acquire).as_ref()? };
            let text = m.text();
            text.contains(&pc).then(|| FrameInfo { text_base: Some(text.start), kind: FrameInfoKind::EhFrameHdr(m.eh_frame_hdr()) })
        })
    }
}

pub fn add(m: *mut Module) {
    let _ = set_custom_eh_frame_finder(&Finder);
    for s in &MODULES {
        if s.compare_exchange(null_mut(), m, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
            return;
        }
    }
}

pub fn remove(m: *mut Module) {
    for s in &MODULES {
        let _ = s.compare_exchange(m, null_mut(), Ordering::AcqRel, Ordering::Relaxed);
    }
}
