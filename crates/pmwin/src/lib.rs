//! Window system core for rs2's Presentation Manager: the window tree,
//! z-order, positions, visible regions, update regions and hit testing.
//! Message queues, window procedures and drawing are the DLL's business;
//! this crate is plain data structures so it is unit-tested on the host.

#![no_std]

extern crate alloc;

pub mod container;
pub mod controls;
pub mod mle;
pub mod notebook;
mod queue;
pub mod minimize;
pub mod shellicons;
mod region;
pub mod sysptr;
mod tree;

pub use gpi::{Point, Rect};
pub use queue::{MsgQueue, QMsg};
pub use region::Region;
pub use tree::{Desktop, Hwnd, Window, HWND_DESKTOP, HWND_OBJECT};

/// Window styles (WS_*).
pub mod ws {
    pub const VISIBLE: u32 = 0x8000_0000;
    pub const DISABLED: u32 = 0x4000_0000;
    pub const CLIPCHILDREN: u32 = 0x2000_0000;
    pub const CLIPSIBLINGS: u32 = 0x1000_0000;
    pub const PARENTCLIP: u32 = 0x0800_0000;
    pub const SAVEBITS: u32 = 0x0400_0000;
    pub const SYNCPAINT: u32 = 0x0200_0000;
    pub const MINIMIZED: u32 = 0x0100_0000;
    pub const MAXIMIZED: u32 = 0x0080_0000;
}
