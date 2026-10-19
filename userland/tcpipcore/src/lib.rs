//! The implementation behind rs2's OS/2 TCP/IP DLLs: TCPIP32.DLL (the
//! 4.1+ API with 4.4BSD structures), SO32DLL.DLL and TCP32DLL.DLL (the 4.0
//! socket and network database calls with 4.3BSD structures). Each DLL is
//! a thin crate of exported entry points calling in here; the sockets
//! themselves are the kernel's (RS2API 200-219, `os2::net`).
//!
//! Conventions shared with IBM's DLLs: calls return -1 (or NULL) on
//! failure and leave the error for `sock_errno()`, kept per thread, as
//! SOCBASEERR (10000) plus the BSD errno; resolver failures go to
//! `h_errno`. Configuration files come from `%ETC%` (`SET
//! ETC=C:\MPTN\ETC` in CONFIG.SYS): HOSTS, SERVICES, PROTOCOL, NETWORKS,
//! RESOLV2/RESOLV; built-in tables stand in for missing ones.

#![no_std]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

pub mod cstr;
pub mod errno;
pub mod files;
pub mod inet;
pub mod netdb;
pub mod resolv;
pub mod sock;
pub mod sync;
pub mod tls;
