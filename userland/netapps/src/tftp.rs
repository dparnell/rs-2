//! The trivial file transfer protocol (RFC 1350, with the block size
//! option of RFC 2348): what TFTP and TFTPD share.

use alloc::string::String;
use alloc::vec::Vec;

use crate::*;

pub const RRQ: u16 = 1;
pub const WRQ: u16 = 2;
pub const DATA: u16 = 3;
pub const ACK: u16 = 4;
pub const ERROR: u16 = 5;
pub const OACK: u16 = 6;

pub fn op(p: &[u8]) -> u16 {
    if p.len() < 2 { 0 } else { u16::from_be_bytes([p[0], p[1]]) }
}

pub fn num(p: &[u8]) -> u16 {
    if p.len() < 4 { 0 } else { u16::from_be_bytes([p[2], p[3]]) }
}

pub fn ack(block: u16) -> [u8; 4] {
    let b = block.to_be_bytes();
    [0, ACK as u8, b[0], b[1]]
}

pub fn data(block: u16, d: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(d.len() + 4);
    v.extend_from_slice(&[0, DATA as u8]);
    v.extend_from_slice(&block.to_be_bytes());
    v.extend_from_slice(d);
    v
}

pub fn error(code: u16, msg: &str) -> Vec<u8> {
    let mut v = alloc::vec![0, ERROR as u8];
    v.extend_from_slice(&code.to_be_bytes());
    v.extend_from_slice(msg.as_bytes());
    v.push(0);
    v
}

/// A request: opcode, file name, mode and options (name, value).
pub fn request(opcode: u16, file: &str, mode: &str, opts: &[(&str, String)]) -> Vec<u8> {
    let mut v = alloc::vec![0, opcode as u8];
    for s in [file, mode] {
        v.extend_from_slice(s.as_bytes());
        v.push(0);
    }
    for (k, val) in opts {
        v.extend_from_slice(k.as_bytes());
        v.push(0);
        v.extend_from_slice(val.as_bytes());
        v.push(0);
    }
    v
}

/// The zero-terminated strings after the opcode.
pub fn strings(p: &[u8]) -> Vec<String> {
    p.get(2..).unwrap_or(&[]).split(|&b| b == 0).map(|s| s.iter().map(|&b| b as char).collect()).collect()
}

/// An ERROR packet's text.
pub fn error_text(p: &[u8]) -> String {
    let msg: String = p.get(4..).unwrap_or(&[]).iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    alloc::format!("Error code {}: {}", num(p), msg)
}

/// Send `pkt` to `to` and wait for an answer from `to`'s address (from
/// any port when `any_port`), sending again every `rexmt` ms until
/// `total` ms have passed. Returns the answer and where it came from.
pub fn exchange(s: &Sock, pkt: &[u8], to: SockAddrIn, any_port: bool, rexmt: u32, total: u32, accept: &dyn Fn(&[u8]) -> bool) -> Option<(Vec<u8>, SockAddrIn)> {
    let start = ms();
    let mut buf = alloc::vec![0u8; 65536];
    loop {
        if !pkt.is_empty() && s.send_to(pkt, to).is_err() {
            return None;
        }
        let sent = ms();
        loop {
            let left = rexmt as i32 - ms().wrapping_sub(sent) as i32;
            if left <= 0 {
                break;
            }
            if !s.readable(left) {
                break;
            }
            let Ok((n, from)) = s.recv_from(&mut buf) else { break };
            if from.addr != to.addr || (!any_port && from.port != to.port) {
                continue;
            }
            let p = &buf[..n];
            if op(p) == ERROR || accept(p) {
                return Some((p.to_vec(), from));
            }
        }
        if ms().wrapping_sub(start) >= total {
            return None;
        }
    }
}

/// netascii: CR LF for line ends (and CR NUL for a lone CR) going out.
pub fn to_netascii(d: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(d.len() + d.len() / 32);
    let mut last = 0;
    for &b in d {
        if b == b'\n' && last != b'\r' {
            v.push(b'\r');
        }
        v.push(b);
        last = b;
    }
    v
}

/// netascii coming in: CR NUL becomes CR; CR LF stays (OS/2's line end).
pub fn from_netascii(d: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(d.len());
    let mut i = 0;
    while i < d.len() {
        if d[i] == b'\r' && d.get(i + 1) == Some(&0) {
            v.push(b'\r');
            i += 2;
            continue;
        }
        v.push(d[i]);
        i += 1;
    }
    v
}
