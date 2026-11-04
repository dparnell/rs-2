//! rs2libc: the C library Rust's standard library needs, on top of the
//! OS/2 API, so that programs for rs2 can use `std`.
//!
//! Programs are built for `i686-unknown-linux-gnu` with the precompiled std
//! rustup ships for that target. That std calls glibc functions by name
//! (`open64`, `pthread_create`, `getaddrinfo`, `syscall(SYS_futex)`...);
//! this crate defines them, with glibc's i386 structure layouts (taken from
//! the `libc` crate, as std's own declarations are), in terms of DOSCALLS
//! and rs2's kernel socket interface. The program is then linked statically
//! (userland-std/link.ld) and converted to LX by tools/elf2lx like rs2's
//! no_std programs. See docs/BROWSER.md for the design.
//!
//! Every program must link this crate: `extern crate rs2libc;` (or `use
//! rs2libc as _;`) in its main.rs.
//!
//! Outline:
//! - `start`: the LX entry point, argv/environ, `.init_array`, exit.
//! - `tls`: ELF thread-local storage (the thread pointer is GS's base: a
//!   per-thread LDT alias selector, RS2API 421), errno.
//! - `malloc`: dlmalloc over DosAllocMem.
//! - `thread`: pthreads over DosCreateThread, futexes over event semaphores.
//! - `file`: descriptors, files and directories over Dos* file calls.
//! - `net`: BSD sockets over RS2API 201-218, getaddrinfo, poll.
//! - `time`, `misc` (environment, signals as no-ops, random numbers...),
//!   `string` (mem*/str*), `unwind` (the `unwinding` crate provides the
//!   `_Unwind_*` functions std's panics use).

#![no_std]
#![allow(clippy::missing_safety_doc, non_camel_case_types)]

extern crate alloc;

pub mod sys;
pub mod errno;
pub mod tls;
pub mod start;
pub mod malloc;
pub mod string;
pub mod thread;
pub mod futex;
pub mod file;
pub mod net;
pub mod time;
pub mod misc;
pub mod process;
pub mod spin;

// The unwinder (`_Unwind_RaiseException` and friends) comes from the
// `unwinding` crate; referencing it keeps it linked.
#[allow(unused_imports)]
pub use unwinding as _;

pub use errno::set_errno;
