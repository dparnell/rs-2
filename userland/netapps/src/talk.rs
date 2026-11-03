//! The talk protocol (4.3BSD "ntalk", UDP port 518) shared by TALK and
//! TALKD: the control message a client sends a talk daemon and the
//! daemon's response.

use alloc::string::String;
use alloc::vec::Vec;

pub const VERSION: u8 = 1;

pub const LEAVE_INVITE: u8 = 0;
pub const LOOK_UP: u8 = 1;
pub const DELETE: u8 = 2;
pub const ANNOUNCE: u8 = 3;

pub const SUCCESS: u8 = 0;
pub const NOT_HERE: u8 = 1;
pub const FAILED: u8 = 2;
pub const MACHINE_UNKNOWN: u8 = 3;
pub const PERMISSION_DENIED: u8 = 4;
pub const UNKNOWN_REQUEST: u8 = 5;
pub const BADVERSION: u8 = 6;
pub const BADADDR: u8 = 7;
pub const BADCTLADDR: u8 = 8;

pub const MSG_SIZE: usize = 84;
pub const RESPONSE_SIZE: usize = 24;

/// An address as the protocol carries it (a 4.3BSD `sockaddr` with the
/// family in network byte order): address and port.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Addr {
    pub ip: [u8; 4],
    pub port: u16,
}

impl Addr {
    fn put(&self, v: &mut Vec<u8>) {
        v.extend_from_slice(&2u16.to_be_bytes());
        v.extend_from_slice(&self.port.to_be_bytes());
        v.extend_from_slice(&self.ip);
        v.extend_from_slice(&[0; 8]);
    }

    fn get(b: &[u8]) -> (Addr, u16) {
        let fam = u16::from_be_bytes([b[0], b[1]]);
        (Addr { ip: [b[4], b[5], b[6], b[7]], port: u16::from_be_bytes([b[2], b[3]]) }, fam)
    }
}

#[derive(Clone, Default, Debug)]
pub struct CtlMsg {
    pub typ: u8,
    pub id: u32,
    pub addr: Addr,
    pub ctl_addr: Addr,
    pub pid: u32,
    pub l_name: String,
    pub r_name: String,
    pub r_tty: String,
}

fn put_name(v: &mut Vec<u8>, s: &str, n: usize) {
    let b = s.as_bytes();
    for i in 0..n {
        v.push(if i < b.len() && i < n - 1 { b[i] } else { 0 });
    }
}

fn get_name(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

impl CtlMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(MSG_SIZE);
        v.extend_from_slice(&[VERSION, self.typ, 0, 0]);
        v.extend_from_slice(&self.id.to_be_bytes());
        self.addr.put(&mut v);
        self.ctl_addr.put(&mut v);
        v.extend_from_slice(&self.pid.to_be_bytes());
        put_name(&mut v, &self.l_name, 12);
        put_name(&mut v, &self.r_name, 12);
        put_name(&mut v, &self.r_tty, 16);
        v
    }

    /// The message and its version, the families of its two addresses.
    pub fn decode(b: &[u8]) -> Option<(CtlMsg, u8, u16, u16)> {
        if b.len() < MSG_SIZE {
            return None;
        }
        let (addr, f1) = Addr::get(&b[8..24]);
        let (ctl_addr, f2) = Addr::get(&b[24..40]);
        let m = CtlMsg {
            typ: b[1],
            id: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            addr,
            ctl_addr,
            pid: u32::from_be_bytes([b[40], b[41], b[42], b[43]]),
            l_name: get_name(&b[44..56]),
            r_name: get_name(&b[56..68]),
            r_tty: get_name(&b[68..84]),
        };
        Some((m, b[0], f1, f2))
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Response {
    pub typ: u8,
    pub answer: u8,
    pub id: u32,
    pub addr: Addr,
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(RESPONSE_SIZE);
        v.extend_from_slice(&[VERSION, self.typ, self.answer, 0]);
        v.extend_from_slice(&self.id.to_be_bytes());
        self.addr.put(&mut v);
        v
    }

    pub fn decode(b: &[u8]) -> Option<Response> {
        if b.len() < RESPONSE_SIZE || b[0] != VERSION {
            return None;
        }
        Some(Response { typ: b[1], answer: b[2], id: u32::from_be_bytes([b[4], b[5], b[6], b[7]]), addr: Addr::get(&b[8..24]).0 })
    }
}

/// What a refused request means.
pub fn answer_text(a: u8) -> &'static str {
    match a {
        NOT_HERE => "the other party is not there",
        FAILED => "the other machine's talk daemon failed",
        MACHINE_UNKNOWN => "the other machine does not know this one",
        PERMISSION_DENIED => "the other party does not take messages",
        UNKNOWN_REQUEST => "the other machine does not handle remote talk",
        BADVERSION => "the other machine speaks another talk protocol version",
        BADADDR => "the other machine rejected our address",
        BADCTLADDR => "the other machine rejected our control address",
        _ => "the other machine refused the request",
    }
}
