//! A TCP/IP protocol stack for rs2, independent of hardware and of the
//! kernel: packets go in and come out as byte buffers, time is a
//! millisecond counter the caller passes in, and nothing blocks. The
//! kernel (`kernel/src/net`) feeds it frames from the network drivers,
//! sends the frames it produces, runs its timers and builds the blocking
//! socket API on top; the host test suite drives it the same way.
//!
//! * [`wire`]: the Internet checksum and byte order helpers.
//! * [`eth`], [`arp`], [`ipv4`], [`icmp`], [`udp`], [`tcp`]: packet
//!   formats (RFC 894, 826, 791, 792, 768, 9293); `tcp` also holds the
//!   connection state machine.
//! * [`dhcp`] (RFC 2131/2132) and [`dns`] (RFC 1035): the address
//!   configuration client and the stub resolver.
//! * [`stack`]: interfaces, routing, the ARP cache, fragment reassembly
//!   and the sockets (TCP, UDP, raw ICMP).

#![no_std]

extern crate alloc;

pub mod arp;
pub mod dhcp;
pub mod dns;
pub mod eth;
pub mod icmp;
pub mod ipv4;
pub mod stack;
pub mod tcp;
pub mod udp;
pub mod wire;

pub use stack::{Error, Stack};

/// An IPv4 address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Ipv4(pub [u8; 4]);

impl Ipv4 {
    pub const UNSPECIFIED: Ipv4 = Ipv4([0, 0, 0, 0]);
    pub const BROADCAST: Ipv4 = Ipv4([255, 255, 255, 255]);
    pub const LOOPBACK: Ipv4 = Ipv4([127, 0, 0, 1]);

    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Ipv4 {
        Ipv4([a, b, c, d])
    }

    pub fn from_u32(v: u32) -> Ipv4 {
        Ipv4(v.to_be_bytes())
    }

    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub fn is_unspecified(self) -> bool {
        self.0 == [0; 4]
    }

    pub fn is_broadcast(self) -> bool {
        self.0 == [255; 4]
    }

    pub fn is_loopback(self) -> bool {
        self.0[0] == 127
    }

    pub fn is_multicast(self) -> bool {
        self.0[0] & 0xf0 == 0xe0
    }

    /// Whether `self` and `other` are on the same network under `mask`.
    pub fn same_net(self, other: Ipv4, mask: Ipv4) -> bool {
        self.to_u32() & mask.to_u32() == other.to_u32() & mask.to_u32()
    }

    /// Netmask with `bits` leading ones.
    pub fn mask(bits: u8) -> Ipv4 {
        let bits = bits.min(32);
        Ipv4::from_u32(if bits == 0 { 0 } else { u32::MAX << (32 - bits) })
    }

    /// Leading ones of a netmask.
    pub fn prefix_len(self) -> u8 {
        self.to_u32().leading_ones() as u8
    }

    /// Parse dotted-quad notation ("10.0.2.15").
    pub fn parse(s: &str) -> Option<Ipv4> {
        let mut out = [0u8; 4];
        let mut parts = s.split('.');
        for o in out.iter_mut() {
            let p = parts.next()?;
            if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let v: u32 = p.parse().ok()?;
            if v > 255 {
                return None;
            }
            *o = v as u8;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Ipv4(out))
    }
}

impl core::fmt::Display for Ipv4 {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

impl core::fmt::Debug for Ipv4 {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

/// An Ethernet (MAC) address.
pub type Mac = [u8; 6];

pub const MAC_BROADCAST: Mac = [0xff; 6];

/// Formats a MAC address as "52:54:00:12:34:56".
pub struct MacFmt(pub Mac);

impl core::fmt::Display for MacFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let m = self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
    }
}

/// An IPv4 socket address.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Endpoint {
    pub addr: Ipv4,
    pub port: u16,
}

impl Endpoint {
    pub const fn new(addr: Ipv4, port: u16) -> Endpoint {
        Endpoint { addr, port }
    }
}

impl core::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}:{}", self.addr, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_masks() {
        assert_eq!(Ipv4::parse("10.0.2.15"), Some(Ipv4::new(10, 0, 2, 15)));
        assert_eq!(Ipv4::parse("256.0.0.1"), None);
        assert_eq!(Ipv4::parse("1.2.3"), None);
        assert_eq!(Ipv4::parse("1.2.3.4.5"), None);
        assert_eq!(Ipv4::mask(24), Ipv4::new(255, 255, 255, 0));
        assert_eq!(Ipv4::mask(0), Ipv4::UNSPECIFIED);
        assert_eq!(Ipv4::new(255, 255, 240, 0).prefix_len(), 20);
        assert!(Ipv4::new(10, 0, 2, 15).same_net(Ipv4::new(10, 0, 2, 2), Ipv4::mask(24)));
    }
}
