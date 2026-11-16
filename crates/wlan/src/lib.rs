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
//! * [`crypto`] (the `rs2crypto` crate): SHA-1, HMAC, PBKDF2, the 802.11 PRF, AES-128 and the
//!   RFC 3394 key wrap: what WPA2-Personal key management needs.
//! * [`wpa`]: the WPA2-PSK supplicant (4-way and group key handshakes,
//!   CCMP pairwise), driven by the EAPOL frames the network layer passes.

#![no_std]

extern crate alloc;

pub mod config;
pub use rs2crypto as crypto;
pub mod ie;
pub mod mwifiex;
pub mod wpa;

pub use ie::Bss;

/// `aa:bb:cc:dd:ee:ff`.
pub struct MacFmt(pub [u8; 6]);

impl core::fmt::Display for MacFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let m = self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
    }
}
