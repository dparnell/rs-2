//! PMWIN.DLL: rs2's Presentation Manager window manager. It also
//! implements the GPI (PMGPI.DLL forwards to it), since presentation
//! spaces belong to windows. See docs/PM.md.

#![no_std]
#![no_main]

extern crate alloc;
extern crate os2;

mod accel;
mod bitmap;
mod button;
mod clip;
mod combo;
mod color;
mod ctl;
mod dlg;
mod draw;
mod entry;
mod exports;
mod fileicon;
mod frame;
mod gpiapi;
mod gpiobj;
mod gpitext;
mod input;
mod listbox;
mod lock;
mod menu;
mod misc;
mod mle;
mod msg;
mod pointer;
mod rect;
mod res;
mod scrollbar;
mod server;
mod shared;
mod slider;
mod spin;
mod state;
mod statics;
mod valueset;
mod viowin;
mod win;

/// A line on the serial port (RS2API.4), for debugging.
#[allow(dead_code)]
pub fn trace(args: core::fmt::Arguments) {
    unsafe extern "C" {
        #[link_name = "RS2API.4"]
        fn Rs2DebugTrace(psz: *const u8) -> u32;
    }
    let mut s = alloc::format!("{}", args).into_bytes();
    s.push(0);
    unsafe { Rs2DebugTrace(s.as_ptr()) };
}

/// _DLL_InitTerm(hmod, flag): at the end of a process, what it had in
/// PM goes (the PM server does the same when the kernel reports the
/// process gone, also after a crash).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(hmod: u32, flag: u32) -> u32 {
    server::HMOD.store(hmod, core::sync::atomic::Ordering::Relaxed);
    shared::use_shared_heap();
    if flag == 1 {
        term();
    }
    1
}

#[unsafe(naked)]
extern "C" fn term() {
    // Called on the OS/2 stack: align it for Rust.
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16",
        "call {f}",
        "mov esp, ebp", "pop ebp", "ret",
        f = sym term_aligned,
    );
}

extern "C" fn term_aligned() {
    if !shared::attached() {
        return;
    }
    lock::enter_quiet();
    if state::get().is_some() {
        server::process_gone(state::pid());
    }
    lock::leave();
}
