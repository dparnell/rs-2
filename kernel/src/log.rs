//! Kernel message output to the serial port and the screen.

use core::fmt::{self, Write};

use crate::sync::SpinLock;

static LOCK: SpinLock<()> = SpinLock::new(());

/// When false, messages only go to the serial port (set once a VIO session
/// owns the screen).
pub static mut TO_SCREEN: bool = true;

struct Out;

impl Write for Out {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::serial::write_str(s);
        if unsafe { TO_SCREEN } {
            if let Some(mut g) = crate::screen::SCREEN.try_lock() {
                if let Some(scr) = g.as_mut() {
                    scr.write_str(s);
                }
            }
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    let _g = LOCK.lock();
    let _ = Out.write_fmt(args);
}

/// Print without taking the lock; for the panic path.
pub fn print_unlocked(args: fmt::Arguments) {
    let _ = Out.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::log::print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::log::print(format_args!("{}\n", format_args!($($arg)*))) };
}

/// Serial-only debug trace.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        struct S;
        impl core::fmt::Write for S {
            fn write_str(&mut self, s: &str) -> core::fmt::Result { $crate::serial::write_str(s); Ok(()) }
        }
        let _ = writeln!(S, $($arg)*);
    }};
}
