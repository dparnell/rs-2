//! The OS-independent part of rs2's guest agent, VMAGENT.EXE (docs/AGENT.md),
//! tested on the host:
//!
//! - [`text`]: clipboard text between an OS/2 code page (CR LF line ends)
//!   and UTF-8 (LF), as hosts exchange it;
//! - [`vdagent`]: the SPICE agent protocol (QEMU's qemu-vdagent, SPICE
//!   clients, UTM, Proxmox, libvirt), framing and clipboard messages;
//! - [`vmware`]: VMware's GuestRPC and copy/paste;
//! - [`qga`]: the QEMU guest agent's commands (with [`json`]).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod json;
pub mod qga;
pub mod text;
pub mod vdagent;
pub mod vmware;
