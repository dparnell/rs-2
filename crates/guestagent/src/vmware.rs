//! VMware's guest↔host channel and its copy/paste protocol, text only
//! (work from open-vm-tools' sources; docs/AGENT.md):
//!
//! - the backdoor (an `in eax, dx` on port 0x5658 with the magic
//!   number), reached through [`Backdoor`] (the kernel makes the call);
//! - GuestRPC's message layer on backdoor command 0x1E (open, send, receive,
//!   close; four bytes per call), RPCI (guest→host commands, replies "1 "
//!   or "0 ") and TCLO (host→guest commands, which the guest polls for);
//! - copy/paste version 4: `copypaste.transport` packets (a 56-byte
//!   header, a CPClipboard blob whose text slot is NUL-terminated UTF-8)
//!   over the `toolbox-dnd` TCLO channel and RPCI;
//! - the legacy version 1 (backdoor commands 6-9, UTF-8 four bytes at a
//!   time), used when the host does not take version 4.
//!
//! [`Agent::poll`] does a round of work and returns what happened; the
//! caller supplies the guest's text when the host asks for it
//! ([`Agent::send_clipboard`]).

use alloc::string::String;
use alloc::vec::Vec;

pub const MAGIC: u32 = 0x564d_5868;
pub const PORT: u32 = 0x5658;

const CMD_GETSELLENGTH: u32 = 6;
const CMD_GETNEXTPIECE: u32 = 7;
const CMD_SETSELLENGTH: u32 = 8;
const CMD_SETNEXTPIECE: u32 = 9;
const CMD_MESSAGE: u32 = 0x1e;

const MSG_OPEN: u32 = 0;
const MSG_SENDSIZE: u32 = 1;
const MSG_SENDPAYLOAD: u32 = 2;
const MSG_RECVSIZE: u32 = 3;
const MSG_RECVPAYLOAD: u32 = 4;
const MSG_RECVSTATUS: u32 = 5;
const MSG_CLOSE: u32 = 6;

const ST_SUCCESS: u32 = 1;
const ST_DORECV: u32 = 2;
const ST_CPT: u32 = 0x10;

pub const RPCI: u32 = 0x4943_5052;
pub const TCLO: u32 = 0x4f4c_4354;
const COOKIE: u32 = 0x8000_0000;

pub const MAX_SELECTION: usize = 65436;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
}

/// One low-bandwidth backdoor call (`in eax, dx` with EAX = magic and the
/// port in DX's low half; the other registers as given).
pub trait Backdoor {
    fn call(&mut self, r: Regs) -> Regs;
}

/// A GuestRPC channel.
#[derive(Clone, Copy, Debug)]
pub struct Channel {
    id: u32,
    cookie_hi: u32,
    cookie_lo: u32,
}

fn msg(bd: &mut dyn Backdoor, ch: &Channel, ty: u32, ebx: u32) -> Regs {
    bd.call(Regs {
        eax: MAGIC,
        ebx,
        ecx: ty << 16 | CMD_MESSAGE,
        edx: ch.id << 16 | PORT,
        esi: ch.cookie_hi,
        edi: ch.cookie_lo,
    })
}

fn status(r: &Regs) -> u32 {
    r.ecx >> 16
}

impl Channel {
    pub fn open(bd: &mut dyn Backdoor, proto: u32) -> Option<Channel> {
        let none = Channel { id: 0, cookie_hi: 0, cookie_lo: 0 };
        for p in [proto | COOKIE, proto] {
            let r = msg(bd, &none, MSG_OPEN, p);
            if status(&r) & ST_SUCCESS != 0 {
                let (hi, lo) = if p & COOKIE != 0 { (r.esi, r.edi) } else { (0, 0) };
                return Some(Channel { id: r.edx >> 16, cookie_hi: hi, cookie_lo: lo });
            }
        }
        None
    }

    pub fn close(&self, bd: &mut dyn Backdoor) {
        msg(bd, self, MSG_CLOSE, 0);
    }

    /// Send one message (empty: TCLO's poll).
    pub fn send(&self, bd: &mut dyn Backdoor, data: &[u8]) -> bool {
        'retry: for _ in 0..4 {
            let r = msg(bd, self, MSG_SENDSIZE, data.len() as u32);
            if status(&r) & ST_SUCCESS == 0 {
                return false;
            }
            for c in data.chunks(4) {
                let mut w = [0u8; 4];
                w[..c.len()].copy_from_slice(c);
                let r = msg(bd, self, MSG_SENDPAYLOAD, u32::from_le_bytes(w));
                if status(&r) & ST_SUCCESS == 0 {
                    if status(&r) & ST_CPT != 0 {
                        continue 'retry;
                    }
                    return false;
                }
            }
            return true;
        }
        false
    }

    /// Receive a message: None on error, Some(empty) if none is waiting.
    pub fn recv(&self, bd: &mut dyn Backdoor) -> Option<Vec<u8>> {
        'retry: for _ in 0..4 {
            let r = msg(bd, self, MSG_RECVSIZE, 0);
            let st = status(&r);
            if st & ST_SUCCESS == 0 {
                return None;
            }
            if st & ST_DORECV == 0 {
                return Some(Vec::new());
            }
            if r.edx >> 16 != MSG_SENDSIZE {
                return None;
            }
            let len = r.ebx as usize;
            if len > 1 << 22 {
                return None;
            }
            let mut out = Vec::with_capacity(len);
            while out.len() < len {
                let r = msg(bd, self, MSG_RECVPAYLOAD, ST_SUCCESS);
                if status(&r) & ST_SUCCESS == 0 {
                    if status(&r) & ST_CPT != 0 {
                        continue 'retry;
                    }
                    return None;
                }
                if r.edx >> 16 != MSG_SENDPAYLOAD {
                    return None;
                }
                let n = (len - out.len()).min(4);
                out.extend_from_slice(&r.ebx.to_le_bytes()[..n]);
            }
            let r = msg(bd, self, MSG_RECVSTATUS, ST_SUCCESS);
            if status(&r) & ST_SUCCESS == 0 {
                if status(&r) & ST_CPT != 0 {
                    continue 'retry;
                }
                return None;
            }
            return Some(out);
        }
        None
    }
}

/// An RPCI command: Some(reply after "1 ") on success.
pub fn rpci(bd: &mut dyn Backdoor, cmd: &[u8]) -> Option<Vec<u8>> {
    let ch = Channel::open(bd, RPCI)?;
    let ok = ch.send(bd, cmd);
    let reply = if ok { ch.recv(bd) } else { None };
    ch.close(bd);
    let reply = reply?;
    (reply.starts_with(b"1 ") || reply == b"1").then(|| reply.get(2..).unwrap_or(&[]).to_vec())
}

// ---- Copy/paste version 4 ----

pub const CP_PING: u32 = 1;
pub const CP_PING_REPLY: u32 = 2;
pub const CP_REQUEST_NEXT: u32 = 3;
pub const CP_REPLY: u32 = 4;
pub const CP_REQUEST_CLIPBOARD: u32 = 2000;
pub const CP_RECV_CLIPBOARD: u32 = 2002;
pub const CP_SEND_CLIPBOARD: u32 = 2003;

/// VALID | CP | PLAIN_TEXT_CP.
pub const CP_CAPS: u32 = 0x15;
const CAP_PLAIN_TEXT_CP: u32 = 1 << 4;
pub const HEADER: usize = 56;
pub const MAX_PAYLOAD: usize = 65436 - HEADER;
const TRANSPORT: &[u8] = b"copypaste.transport";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Packet {
    pub cmd: u32,
    pub session: u32,
    pub status: u32,
    pub param: [u32; 6],
    pub binary_size: u32,
    pub offset: u32,
    pub payload: Vec<u8>,
}

impl Packet {
    /// A guest packet (type 3, source 2, as VMware's tools send them).
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER + self.payload.len());
        for w in [self.cmd, 3, 2, self.session, self.status] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        for w in self.param {
            b.extend_from_slice(&w.to_le_bytes());
        }
        for w in [self.binary_size, self.offset, self.payload.len() as u32] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(&self.payload);
        b
    }

    pub fn decode(b: &[u8]) -> Option<Packet> {
        if b.len() < HEADER {
            return None;
        }
        let w = |i: usize| u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        let size = w(13) as usize;
        if HEADER + size != b.len() || w(12) as usize + size > w(11) as usize {
            return None;
        }
        Some(Packet {
            cmd: w(0),
            session: w(3),
            status: w(4),
            param: [w(5), w(6), w(7), w(8), w(9), w(10)],
            binary_size: w(11),
            offset: w(12),
            payload: b[HEADER..].to_vec(),
        })
    }
}

/// A CPClipboard with the text slot (UTF-8 without NUL) or nothing.
pub fn clipboard_blob(text: Option<&[u8]>) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&11u32.to_le_bytes());
    for slot in 1..11 {
        match text {
            Some(t) if slot == 1 => {
                b.push(1);
                b.extend_from_slice(&(t.len() as u32 + 1).to_le_bytes());
                b.extend_from_slice(t);
                b.push(0);
            }
            _ => {
                b.push(0);
                b.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }
    b.push(1);
    b
}

/// The text slot of a CPClipboard (UTF-8, without its NUL).
pub fn blob_text(b: &[u8]) -> Option<Vec<u8>> {
    let max = u32::from_le_bytes(b.get(..4)?.try_into().ok()?).min(11);
    let mut p = 4;
    for slot in 1..max {
        let exists = *b.get(p)? != 0;
        let size = u32::from_le_bytes(b.get(p + 1..p + 5)?.try_into().ok()?) as usize;
        p += 5;
        let data = if exists { b.get(p..p + size)? } else { &[][..] };
        if exists {
            p += size;
        }
        if slot == 1 && exists {
            let end = data.iter().position(|&c| c == 0).unwrap_or(data.len());
            return Some(data[..end].to_vec());
        }
    }
    None
}

/// What [`Agent::poll`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The host's clipboard text (UTF-8).
    HostText(Vec<u8>),
    /// The host wants the guest's text: answer with send_clipboard.
    HostRequest,
    /// Copy/paste is on (version 4 or 1).
    Ready(u32),
}

struct Tclo {
    name: &'static [u8],
    ch: Option<Channel>,
    reply: Vec<u8>,
}

/// The guest's side: two TCLO channels, version negotiation, copy/paste.
pub struct Agent {
    tclo: [Tclo; 2],
    /// Negotiated version: 0 not yet, 4, or 1 (legacy).
    pub version: u32,
    /// The host turned copy/paste on (Set_Option copypaste 1).
    pub enabled: bool,
    host_caps: u32,
    /// The host's request being answered: (session, isActive).
    request: Option<(u32, u32)>,
    /// A big message from the host being assembled.
    incoming: Vec<u8>,
    /// Polls without negotiation; legacy after a while.
    idle_polls: u32,
    /// Legacy: last text pushed (not given back again).
    pushed: Option<Vec<u8>>,
    polls: u32,
}

impl Default for Agent {
    fn default() -> Agent {
        Agent::new()
    }
}

/// Polls (one every 10-100 ms) before falling back to version 1.
const LEGACY_AFTER: u32 = 300;

impl Agent {
    pub fn new() -> Agent {
        Agent {
            tclo: [Tclo { name: b"toolbox", ch: None, reply: Vec::new() }, Tclo { name: b"toolbox-dnd", ch: None, reply: Vec::new() }],
            version: 0,
            enabled: false,
            host_caps: 0,
            request: None,
            incoming: Vec::new(),
            idle_polls: 0,
            pushed: None,
            polls: 0,
        }
    }

    /// Whether the host speaks GuestRPC at all (QEMU's vmport answers the
    /// backdoor's version query but not this).
    pub fn probe(bd: &mut dyn Backdoor) -> bool {
        match Channel::open(bd, RPCI) {
            Some(c) => {
                c.close(bd);
                true
            }
            None => false,
        }
    }

    /// One round: each TCLO channel sends its reply (or polls) and takes a
    /// command. Returns what happened and whether anything arrived (the
    /// caller polls faster then).
    pub fn poll(&mut self, bd: &mut dyn Backdoor) -> (Vec<Event>, bool) {
        let mut ev = Vec::new();
        let mut busy = false;
        self.polls += 1;
        for i in 0..2 {
            if self.tclo[i].ch.is_none() {
                self.tclo[i].ch = Channel::open(bd, TCLO);
                // A reopened channel says who it is.
                if self.tclo[i].ch.is_some() && self.polls > 1 {
                    self.tclo[i].reply = atr(self.tclo[i].name);
                }
            }
            let Some(ch) = self.tclo[i].ch else { continue };
            let reply = core::mem::take(&mut self.tclo[i].reply);
            let got = if ch.send(bd, &reply) { ch.recv(bd) } else { None };
            match got {
                None => {
                    ch.close(bd);
                    self.tclo[i].ch = None;
                }
                Some(m) if m.is_empty() => {}
                Some(m) => {
                    busy = true;
                    let r = self.command(bd, i, &m, &mut ev);
                    self.tclo[i].reply = r;
                }
            }
        }
        if self.version == 0 {
            self.idle_polls += 1;
            if self.idle_polls >= LEGACY_AFTER {
                self.version = 1;
                self.enabled = true;
                ev.push(Event::Ready(1));
            }
        }
        if self.version == 1 && self.polls % 10 == 0 {
            if let Some(t) = legacy_get(bd) {
                ev.push(Event::HostText(t));
            }
        }
        (ev, busy)
    }

    fn command(&mut self, bd: &mut dyn Backdoor, ch: usize, m: &[u8], ev: &mut Vec<Event>) -> Vec<u8> {
        let sp = m.iter().position(|&c| c == b' ').unwrap_or(m.len());
        let (name, args) = (&m[..sp], m.get(sp + 1..).unwrap_or(&[]));
        let name_trim = name.strip_suffix(b"\0").unwrap_or(name);
        match name_trim {
            b"reset" => {
                let mut cmd = b"vmx.capability.unified_loop ".to_vec();
                cmd.extend_from_slice(self.tclo[ch].name);
                cmd.push(0);
                // Sent after the reply would be better; the host takes it either way.
                let _ = rpci(bd, &cmd);
                atr(self.tclo[ch].name)
            }
            b"ping" => b"OK ".to_vec(),
            b"Capabilities_Register" => {
                if ch == 0 {
                    if rpci(bd, b"tools.set.versiontype 2147483647 4\0").is_none() {
                        let _ = rpci(bd, b"tools.set.version 2147483647\0");
                    }
                } else {
                    self.negotiate(bd, ev);
                }
                b"OK ".to_vec()
            }
            b"Set_Option" => {
                let args: Vec<u8> = args.iter().copied().take_while(|&c| c != 0).collect();
                let mut it = args.split(|&c| c == b' ');
                match (it.next(), it.next()) {
                    (Some(b"copypaste"), Some(v)) => {
                        self.enabled = v == b"1";
                        if self.enabled && self.version == 0 {
                            self.negotiate(bd, ev);
                        }
                        if self.enabled && self.version == 4 {
                            self.ping(bd);
                        }
                        b"OK ".to_vec()
                    }
                    _ => b"OK ".to_vec(),
                }
            }
            TRANSPORT => {
                if let Some(p) = Packet::decode(args) {
                    self.packet(bd, p, ev);
                }
                b"OK ".to_vec()
            }
            _ => b"ERROR Unknown Command".to_vec(),
        }
    }

    fn negotiate(&mut self, bd: &mut dyn Backdoor, ev: &mut Vec<Event>) {
        if self.version == 4 {
            return;
        }
        let v = rpci(bd, b"tools.capability.copypaste_version 4")
            .and_then(|_| rpci(bd, b"vmx.capability.copypaste_version"))
            .map(|r| r.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |n, &c| n * 10 + (c - b'0') as u32));
        self.version = if v.is_some_and(|v| v >= 4) { 4 } else { 1 };
        if self.version == 1 {
            // Tell the host the guest is on version 1.
            let _ = rpci(bd, b"tools.capability.copypaste_version 1");
        }
        ev.push(Event::Ready(self.version));
        if self.version == 4 && self.enabled {
            self.ping(bd);
        }
    }

    fn send_packet(&self, bd: &mut dyn Backdoor, p: &Packet) {
        let mut cmd = TRANSPORT.to_vec();
        cmd.push(b' ');
        cmd.extend_from_slice(&p.encode());
        let _ = rpci(bd, &cmd);
    }

    fn ping(&self, bd: &mut dyn Backdoor) {
        self.send_packet(bd, &Packet { cmd: CP_PING, param: [4, 0, CP_CAPS, 0, 0, 0], ..Default::default() });
    }

    fn packet(&mut self, bd: &mut dyn Backdoor, p: Packet, ev: &mut Vec<Event>) {
        match p.cmd {
            CP_PING_REPLY => self.host_caps = p.param[2],
            CP_RECV_CLIPBOARD => {
                if p.offset == 0 {
                    self.incoming.clear();
                }
                if p.offset as usize != self.incoming.len() {
                    self.incoming.clear();
                    return;
                }
                self.incoming.extend_from_slice(&p.payload);
                if self.incoming.len() < p.binary_size as usize {
                    let next = Packet {
                        cmd: CP_REQUEST_NEXT,
                        session: p.session,
                        param: [p.cmd, p.binary_size, self.incoming.len() as u32, 0, 0, 0],
                        ..Default::default()
                    };
                    self.send_packet(bd, &next);
                    return;
                }
                let blob = core::mem::take(&mut self.incoming);
                if self.host_caps == 0 || self.host_caps & CAP_PLAIN_TEXT_CP != 0 {
                    if let Some(t) = blob_text(&blob) {
                        ev.push(Event::HostText(t));
                    }
                }
            }
            CP_REQUEST_CLIPBOARD => {
                self.request = Some((p.session, p.param[3]));
                ev.push(Event::HostRequest);
            }
            _ => {}
        }
    }

    /// The guest's text (UTF-8) for the host's request (version 4), or
    /// for the host's clipboard when it changes (version 1).
    pub fn send_clipboard(&mut self, bd: &mut dyn Backdoor, text: Option<&[u8]>) {
        let text = text.map(|t| &t[..t.len().min(MAX_PAYLOAD - 60)]);
        match self.version {
            4 => {
                let Some((session, active)) = self.request.take() else { return };
                let blob = clipboard_blob(text);
                let p = Packet {
                    cmd: CP_SEND_CLIPBOARD,
                    session,
                    status: 0,
                    param: [4, 0, 0, active, 0, 0],
                    binary_size: blob.len() as u32,
                    offset: 0,
                    payload: blob,
                };
                self.send_packet(bd, &p);
            }
            1 => {
                let t = text.unwrap_or(&[]);
                let t = &t[..t.len().min(MAX_SELECTION)];
                if self.pushed.as_deref() == Some(t) {
                    return;
                }
                self.pushed = Some(t.to_vec());
                legacy_set(bd, t);
            }
            _ => {}
        }
    }

    /// The host gave this text: not to be pushed back (version 1).
    pub fn note_host_text(&mut self, t: &[u8]) {
        self.pushed = Some(t.to_vec());
    }
}

fn atr(name: &[u8]) -> Vec<u8> {
    let mut r = b"OK ATR ".to_vec();
    r.extend_from_slice(name);
    r
}

fn bdoor(bd: &mut dyn Backdoor, cmd: u32, ebx: u32) -> Regs {
    bd.call(Regs { eax: MAGIC, ebx, ecx: cmd, edx: PORT, esi: 0, edi: 0 })
}

/// Version 1: the host's selection (UTF-8), if it has a new one.
pub fn legacy_get(bd: &mut dyn Backdoor) -> Option<Vec<u8>> {
    let len = bdoor(bd, CMD_GETSELLENGTH, 0).eax as i32;
    if len <= 0 || len as usize > MAX_SELECTION {
        return None;
    }
    let mut out = Vec::with_capacity(len as usize + 3);
    while out.len() < len as usize {
        out.extend_from_slice(&bdoor(bd, CMD_GETNEXTPIECE, 0).eax.to_le_bytes());
    }
    out.truncate(len as usize);
    Some(out)
}

/// Version 1: give the host the guest's selection (UTF-8).
pub fn legacy_set(bd: &mut dyn Backdoor, t: &[u8]) {
    bdoor(bd, CMD_SETSELLENGTH, t.len() as u32);
    for c in t.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        bdoor(bd, CMD_SETNEXTPIECE, u32::from_le_bytes(w));
    }
}

/// For messages: a printable form of a command (logs).
pub fn show(m: &[u8]) -> String {
    m.iter().take(64).map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '.' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::VecDeque;
    use alloc::vec;

    /// A VMX: channels with a send and a receive queue; RPCI answers from
    /// a table; legacy selection.
    #[derive(Default)]
    struct Vmx {
        chans: Vec<Option<(u32, VecDeque<Vec<u8>>, Vec<u8>, usize, Vec<u8>)>>,
        /// (channel, partial send, expected length)
        sending: Vec<(Vec<u8>, usize)>,
        rpci_log: Vec<Vec<u8>>,
        tclo_log: Vec<Vec<u8>>,
        to_guest: VecDeque<Vec<u8>>,
        version_reply: &'static [u8],
        host_sel: Vec<u8>,
        guest_sel: Vec<u8>,
        sel_cursor: usize,
    }

    impl Backdoor for Vmx {
        fn call(&mut self, r: Regs) -> Regs {
            assert_eq!(r.eax, MAGIC);
            assert_eq!(r.edx & 0xffff, PORT);
            let mut o = r;
            match r.ecx & 0xffff {
                CMD_GETSELLENGTH => {
                    self.sel_cursor = 0;
                    o.eax = if self.host_sel.is_empty() { u32::MAX } else { self.host_sel.len() as u32 };
                }
                CMD_GETNEXTPIECE => {
                    let mut w = [0u8; 4];
                    for (i, b) in w.iter_mut().enumerate() {
                        *b = *self.host_sel.get(self.sel_cursor + i).unwrap_or(&0);
                    }
                    self.sel_cursor += 4;
                    o.eax = u32::from_le_bytes(w);
                    if self.sel_cursor >= self.host_sel.len() {
                        self.host_sel.clear();
                    }
                }
                CMD_SETSELLENGTH => {
                    self.guest_sel.clear();
                    self.sel_cursor = r.ebx as usize;
                }
                CMD_SETNEXTPIECE => {
                    self.guest_sel.extend_from_slice(&r.ebx.to_le_bytes());
                    let n = self.sel_cursor;
                    self.guest_sel.truncate(n);
                }
                CMD_MESSAGE => o = self.message(r),
                _ => {}
            }
            o
        }
    }

    impl Vmx {
        fn message(&mut self, r: Regs) -> Regs {
            let ty = r.ecx >> 16;
            let id = (r.edx >> 16) as usize;
            let mut o = r;
            let ok = |o: &mut Regs, st: u32| o.ecx = (st | ST_SUCCESS) << 16;
            if ty == MSG_OPEN {
                self.chans.push(Some((r.ebx & !COOKIE, VecDeque::new(), Vec::new(), 0, Vec::new())));
                self.sending.push((Vec::new(), 0));
                ok(&mut o, 0);
                o.edx = ((self.chans.len() - 1) as u32) << 16;
                o.esi = 0x1234;
                o.edi = 0x5678;
                return o;
            }
            assert_eq!((r.esi, r.edi), (0x1234, 0x5678), "cookie");
            let Some(ch) = self.chans.get_mut(id).and_then(|c| c.as_mut()) else {
                o.ecx = 0;
                return o;
            };
            match ty {
                MSG_SENDSIZE => {
                    self.sending[id] = (Vec::new(), r.ebx as usize);
                    if r.ebx == 0 {
                        let m = Vec::new();
                        self.deliver(id, m);
                    }
                    ok(&mut o, 0);
                }
                MSG_SENDPAYLOAD => {
                    let (buf, len) = &mut self.sending[id];
                    buf.extend_from_slice(&r.ebx.to_le_bytes());
                    if buf.len() >= *len {
                        buf.truncate(*len);
                        let m = core::mem::take(buf);
                        self.deliver(id, m);
                    }
                    ok(&mut o, 0);
                }
                MSG_RECVSIZE => {
                    let ch = self.chans[id].as_mut().unwrap();
                    match ch.1.pop_front() {
                        Some(m) => {
                            ok(&mut o, ST_DORECV);
                            o.edx = MSG_SENDSIZE << 16;
                            o.ebx = m.len() as u32;
                            ch.4 = m;
                            ch.3 = 0;
                        }
                        None => ok(&mut o, 0),
                    }
                }
                MSG_RECVPAYLOAD => {
                    let mut w = [0u8; 4];
                    for (i, b) in w.iter_mut().enumerate() {
                        *b = *ch.4.get(ch.3 + i).unwrap_or(&0);
                    }
                    ch.3 += 4;
                    ok(&mut o, 0);
                    o.edx = MSG_SENDPAYLOAD << 16;
                    o.ebx = u32::from_le_bytes(w);
                }
                MSG_RECVSTATUS => ok(&mut o, 0),
                MSG_CLOSE => {
                    self.chans[id] = None;
                    ok(&mut o, 0);
                }
                _ => o.ecx = 0,
            }
            o
        }

        /// A message from the guest on channel `id`.
        fn deliver(&mut self, id: usize, m: Vec<u8>) {
            let proto = self.chans[id].as_ref().unwrap().0;
            if proto == RPCI {
                self.rpci_log.push(m.clone());
                let reply: Vec<u8> = if m == b"vmx.capability.copypaste_version" {
                    let mut r = b"1 ".to_vec();
                    r.extend_from_slice(self.version_reply);
                    r
                } else {
                    b"1 ".to_vec()
                };
                self.chans[id].as_mut().unwrap().1.push_back(reply);
            } else {
                if !m.is_empty() {
                    self.tclo_log.push(m);
                }
                // The next host command, for whichever TCLO channel polls.
                if let Some(c) = self.to_guest.pop_front() {
                    self.chans[id].as_mut().unwrap().1.push_back(c);
                }
            }
        }
    }

    fn transport(p: &Packet) -> Vec<u8> {
        let mut m = b"copypaste.transport ".to_vec();
        // Host packets: type 1, source 2.
        let mut b = p.encode();
        b[4..8].copy_from_slice(&1u32.to_le_bytes());
        m.extend_from_slice(&b);
        m
    }

    fn run(vmx: &mut Vmx, a: &mut Agent, rounds: usize) -> Vec<Event> {
        let mut ev = Vec::new();
        for _ in 0..rounds {
            ev.extend(a.poll(vmx).0);
        }
        ev
    }

    #[test]
    fn blob_round_trip() {
        let b = clipboard_blob(Some(b"hi"));
        assert_eq!(b.len(), 58);
        assert_eq!(&b[..10], &[11, 0, 0, 0, 1, 3, 0, 0, 0, b'h']);
        assert_eq!(blob_text(&b), Some(b"hi".to_vec()));
        assert_eq!(blob_text(&clipboard_blob(None)), None);
        let p = Packet { cmd: 2003, session: 9, param: [4, 0, 0, 1, 0, 0], binary_size: 58, payload: b.clone(), ..Default::default() };
        let e = p.encode();
        assert_eq!(&e[..12], &[0xd3, 7, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(Packet::decode(&e), Some(p));
    }

    #[test]
    fn version_4_session() {
        let mut vmx = Vmx { version_reply: b"4", ..Default::default() };
        let mut a = Agent::new();
        vmx.to_guest.extend([
            b"reset".to_vec(),
            b"reset".to_vec(),
            b"Capabilities_Register".to_vec(),
            b"Capabilities_Register".to_vec(),
            b"Set_Option copypaste 1".to_vec(),
        ]);
        let ev = run(&mut vmx, &mut a, 6);
        assert!(ev.contains(&Event::Ready(4)), "{ev:?}");
        assert!(vmx.tclo_log.iter().any(|m| m == b"OK ATR toolbox"));
        assert!(vmx.tclo_log.iter().any(|m| m == b"OK ATR toolbox-dnd"));
        assert!(vmx.rpci_log.iter().any(|m| m == b"tools.set.versiontype 2147483647 4\0"));
        assert!(vmx.rpci_log.iter().any(|m| m == b"tools.capability.copypaste_version 4"));
        // The ping.
        let ping = vmx.rpci_log.iter().find(|m| m.starts_with(b"copypaste.transport ")).unwrap();
        let p = Packet::decode(&ping[20..]).unwrap();
        assert_eq!((p.cmd, p.param[0], p.param[2]), (CP_PING, 4, CP_CAPS));

        // Host → guest.
        let blob = clipboard_blob(Some("Grüße".as_bytes()));
        vmx.to_guest.push_back(transport(&Packet { cmd: CP_RECV_CLIPBOARD, session: 5, binary_size: blob.len() as u32, payload: blob, ..Default::default() }));
        let ev = run(&mut vmx, &mut a, 2);
        assert!(ev.contains(&Event::HostText("Grüße".as_bytes().to_vec())), "{ev:?}");

        // Guest → host.
        vmx.to_guest.push_back(transport(&Packet { cmd: CP_REQUEST_CLIPBOARD, session: 7, param: [4, 0, 0, 1, 0, 0], ..Default::default() }));
        let ev = run(&mut vmx, &mut a, 2);
        assert!(ev.contains(&Event::HostRequest));
        a.send_clipboard(&mut vmx, Some(b"caf\xc3\xa9"));
        let sent = vmx.rpci_log.iter().rev().find(|m| m.starts_with(b"copypaste.transport ")).unwrap();
        let p = Packet::decode(&sent[20..]).unwrap();
        assert_eq!((p.cmd, p.session, p.param[3]), (CP_SEND_CLIPBOARD, 7, 1));
        assert_eq!(blob_text(&p.payload), Some(b"caf\xc3\xa9".to_vec()));
    }

    #[test]
    fn big_host_clipboard_in_pieces() {
        let mut vmx = Vmx { version_reply: b"4", ..Default::default() };
        let mut a = Agent::new();
        a.version = 4;
        a.enabled = true;
        let text = vec![b'x'; 100_000];
        let blob = clipboard_blob(Some(&text));
        let first = Packet { cmd: CP_RECV_CLIPBOARD, session: 1, binary_size: blob.len() as u32, payload: blob[..MAX_PAYLOAD].to_vec(), ..Default::default() };
        vmx.to_guest.push_back(transport(&first));
        run(&mut vmx, &mut a, 2);
        let next = vmx.rpci_log.iter().rev().find(|m| m.starts_with(b"copypaste.transport ")).unwrap();
        let p = Packet::decode(&next[20..]).unwrap();
        assert_eq!((p.cmd, p.param[2]), (CP_REQUEST_NEXT, MAX_PAYLOAD as u32));
        let rest = Packet { cmd: CP_RECV_CLIPBOARD, session: 1, binary_size: blob.len() as u32, offset: MAX_PAYLOAD as u32, payload: blob[MAX_PAYLOAD..].to_vec(), ..Default::default() };
        vmx.to_guest.push_back(transport(&rest));
        let ev = run(&mut vmx, &mut a, 2);
        assert_eq!(ev, vec![Event::HostText(text)]);
    }

    #[test]
    fn legacy_when_host_has_no_version_4() {
        let mut vmx = Vmx { version_reply: b"1", host_sel: b"from host".to_vec(), ..Default::default() };
        let mut a = Agent::new();
        vmx.to_guest.extend([b"Capabilities_Register".to_vec(), b"Capabilities_Register".to_vec()]);
        let ev = run(&mut vmx, &mut a, 12);
        assert!(ev.contains(&Event::Ready(1)));
        assert!(ev.contains(&Event::HostText(b"from host".to_vec())), "{ev:?}");
        a.send_clipboard(&mut vmx, Some(b"guest text"));
        assert_eq!(vmx.guest_sel, b"guest text");
    }

    #[test]
    fn unknown_command() {
        let mut vmx = Vmx::default();
        let mut a = Agent::new();
        vmx.to_guest.push_back(b"OS_Halt".to_vec());
        run(&mut vmx, &mut a, 3);
        assert!(vmx.tclo_log.iter().any(|m| m == b"ERROR Unknown Command"));
    }
}
