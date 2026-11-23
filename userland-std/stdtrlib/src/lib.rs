//! A 64-bit DLL with a Rust interface (crate-type dylib): std and rs2libc
//! are in it, and programs that use it share them.

use std::cell::{Cell, RefCell};
use std::fmt::Debug;

use rs2libc as _;

thread_local! {
    pub static DEPTH: Cell<u32> = const { Cell::new(7) };
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub trait Shape: Send {
    fn area(&self) -> f64;
}

pub fn total(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

/// A generic function, compiled into its users.
pub fn note<T: Debug>(t: T) {
    log(format!("{t:?}"));
}

pub fn log(s: String) {
    LOG.with_borrow_mut(|l| l.push(s));
}

pub fn logged() -> Vec<String> {
    LOG.take()
}

pub fn depth() -> u32 {
    DEPTH.get()
}

/// Panics with `s` after calling `f`.
pub fn then_panic(f: impl FnOnce(), s: String) {
    f();
    std::panic::panic_any(s);
}

#[inline(never)]
pub fn call(f: &dyn Fn()) {
    f();
}
