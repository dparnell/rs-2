//! Wireless LAN protocol code for rs2, independent of hardware and of
//! the kernel (docs/SURFACE-PRO-4.md, "Phase 4: WiFi"). The kernel's
//! `net::wlan` layer and its chip drivers build on it; the host test
//! suite checks it without any hardware.
//!
//! * [`ie`]: 802.11 information elements and the scan result ([`ie::Bss`]):
//!   SSID, channel, signal, and the security a network asks for (RSN/WPA).
//! * [`config`]: `\MPTN\ETC\WIFI.CFG`, the networks rs2 may join.
//! * [`mwifiex`]: the host command format of Marvell's full-MAC firmware
//!   (88W8897 and relatives, Linux `mwifiex`): commands, responses, events
//!   and scan results, without the bus transport.

#![no_std]

extern crate alloc;

pub mod config;
pub mod ie;
pub mod mwifiex;

pub use ie::Bss;

/// `aa:bb:cc:dd:ee:ff`.
pub struct MacFmt(pub [u8; 6]);

impl core::fmt::Display for MacFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let m = self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
    }
}
