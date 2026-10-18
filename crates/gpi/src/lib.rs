//! Rendering engine for rs2's Presentation Manager.
//!
//! Draws into a [`Surface`] of 32-bit pixels using PM device coordinates:
//! the origin is the bottom-left corner and y grows upwards. Every drawing
//! operation is clipped to a list of device rectangles (the visible region
//! of a window). No OS dependencies, so it is unit-tested on the host.

#![no_std]

extern crate alloc;

pub mod color;
pub mod font;
mod ps;
mod rect;
mod surface;

pub use ps::{Mix, Ps};
pub use rect::{Point, Rect};
pub use surface::Surface;
