//! Rendering engine for rs2's Presentation Manager.
//!
//! Draws into a [`Surface`] of 32-bit pixels using PM device coordinates:
//! the origin is the bottom-left corner and y grows upwards. Every drawing
//! operation is clipped to a list of device rectangles (the visible region
//! of a window, intersected with the application's clip region and clip
//! path). No OS dependencies, so it is unit-tested on the host.

#![no_std]

extern crate alloc;

pub mod bitmap;
pub mod color;
pub mod damage;
pub mod font;
pub mod goca;
pub mod icon;
pub mod math;
pub mod metafile;
pub mod pattern;
mod ps;
pub mod psdev;
pub mod raster;
pub mod ttf;
pub mod type1;
mod rect;
pub mod region;
mod surface;
pub mod xform;

pub use bitmap::Bitmap;
pub use damage::Damage;
pub use ps::*;
pub use rect::{Point, Rect};
pub use region::Region;
pub use surface::Surface;
pub use xform::Matrix;
