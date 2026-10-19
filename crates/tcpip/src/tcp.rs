//! TCP (RFC 9293): the segment format and the connection state machine.
//!
//! A [`Tcb`] is one connection. It is fed the segments that arrive for
//! it ([`Tcb::input`]), the passage of time ([`Tcb::poll`]) and the
//! application's calls ([`Tcb::send`], [`Tcb::recv`], [`Tcb::close`]);
//! [`Tcb::output`] produces the segments to transmit. Demultiplexing,
//! listening sockets and the IP layer are the stack's business.
//!
//! The implementation aims at being correct rather than fast:
//!
//! * Retransmission is go-back-N from the oldest unacknowledged byte, on
//!   the retransmission timer (RFC 6298 estimator, Karn's rule, binary
//!   back-off) or on three duplicate ACKs.
//! * Congestion control is slow start and congestion avoidance
//!   (RFC 5681) without SACK.
//! * Out-of-order segments are dropped (and answered with a duplicate
//!   ACK); the peer's retransmission fills the gap.
//! * The receive window is the free space of the receive buffer (no
//!   window scaling, so at most 65535 bytes); a window that opens by at
//!   least one segment or half the buffer is announced at once. A zero
//!   send window is probed with one byte on the persist timer.
//! * RSTs must hit RCV.NXT exactly and SYNs in the window are answered
//!   with a challenge ACK (RFC 5961).
//! * TIME_WAIT lasts [`TIME_WAIT`]; an orphaned connection (closed by the
//!   application) stuck in FIN_WAIT_2 is dropped after [`FIN_WAIT2_TIMEOUT`].

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::ipv4::PROTO_TCP;
use crate::stack::Error;
use crate::wire::{be16, be32, put16, transport_checksum};
use crate::{Endpoint, Ipv4};

pub const HEADER: usize = 20;

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const PSH: u8 = 0x08;
pub const ACK: u8 = 0x10;
pub const URG: u8 = 0x20;

/// MSS assumed when the peer sends none (RFC 9293 3.7.1).
pub const DEFAULT_MSS: u16 = 536;
pub const INITIAL_RTO: u64 = 1000;
pub const MIN_RTO: u64 = 200;
pub const MAX_RTO: u64 = 60_000;
/// Retransmissions of a SYN before the connection attempt fails.
pub const SYN_RETRIES: u32 = 5;
/// Retransmissions of data before the connection is given up.
pub const DATA_RETRIES: u32 = 12;
/// 2 * MSL.
pub const TIME_WAIT: u64 = 60_000;
pub const FIN_WAIT2_TIMEOUT: u64 = 60_000;
/// Default buffer sizes.
pub const SEND_BUFFER: usize = 65536;
pub const RECV_BUFFER: usize = 65535;

/// Sequence number comparisons (modulo 2^32).
pub fn lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}
pub fn le(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) <= 0
}
pub fn gt(a: u32, b: u32) -> bool {
    lt(b, a)
}
pub fn ge(a: u32, b: u32) -> bool {
    le(b, a)
}

/// A TCP segment (parsed, or to be sent).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    /// The MSS option (SYN segments).
    pub mss: Option<u16>,
    pub data: Vec<u8>,
}

impl Segment {
    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }

    /// Sequence space the segment occupies.
    pub fn seq_len(&self) -> u32 {
        self.data.len() as u32 + self.has(SYN) as u32 + self.has(FIN) as u32
    }
}

/// Parse a segment carried from `src` to `dst` (checksum verified).
pub fn parse(src: Ipv4, dst: Ipv4, b: &[u8]) -> Option<Segment> {
    if b.len() < HEADER || transport_checksum(src, dst, PROTO_TCP, b) != 0 {
        return None;
    }
    let off = (b[12] >> 4) as usize * 4;
    if off < HEADER || off > b.len() {
        return None;
    }
    let mut mss = None;
    let mut i = HEADER;
    while i < off {
        match b[i] {
            0 => break,
            1 => i += 1,
            kind => {
                if i + 1 >= off {
                    break;
                }
                let len = b[i + 1] as usize;
                if len < 2 || i + len > off {
                    break;
                }
                if kind == 2 && len == 4 {
                    mss = Some(be16(b, i + 2));
                }
                i += len;
            }
        }
    }
    Some(Segment {
        src_port: be16(b, 0),
        dst_port: be16(b, 2),
        seq: be32(b, 4),
        ack: be32(b, 8),
        flags: b[13] & 0x3f,
        window: be16(b, 14),
        mss,
        data: b[off..].to_vec(),
    })
}

/// Encode a segment from `src` to `dst`.
pub fn build(src: Ipv4, dst: Ipv4, s: &Segment) -> Vec<u8> {
    let opt = if s.mss.is_some() { 4 } else { 0 };
    let mut v = Vec::with_capacity(HEADER + opt + s.data.len());
    v.extend_from_slice(&s.src_port.to_be_bytes());
    v.extend_from_slice(&s.dst_port.to_be_bytes());
    v.extend_from_slice(&s.seq.to_be_bytes());
    v.extend_from_slice(&s.ack.to_be_bytes());
    v.push((((HEADER + opt) / 4) as u8) << 4);
    v.push(s.flags);
    v.extend_from_slice(&s.window.to_be_bytes());
    v.extend_from_slice(&[0, 0, 0, 0]);
    if let Some(m) = s.mss {
        v.extend_from_slice(&[2, 4]);
        v.extend_from_slice(&m.to_be_bytes());
    }
    v.extend_from_slice(&s.data);
    let c = transport_checksum(src, dst, PROTO_TCP, &v);
    put16(&mut v, 16, c);
    v
}

/// The reset that answers `seg` arriving where no connection exists
/// (RFC 9293 3.10.7.1). None if `seg` is itself a reset.
pub fn reset_for(seg: &Segment) -> Option<Segment> {
    if seg.has(RST) {
        return None;
    }
    let mut r = Segment { src_port: seg.dst_port, dst_port: seg.src_port, ..Default::default() };
    if seg.has(ACK) {
        r.seq = seg.ack;
        r.flags = RST;
    } else {
        r.ack = seg.seq.wrapping_add(seg.seq_len());
        r.flags = RST | ACK;
    }
    Some(r)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Closed => "CLOSED",
            State::Listen => "LISTEN",
            State::SynSent => "SYN_SENT",
            State::SynReceived => "SYN_RECEIVED",
            State::Established => "ESTABLISHED",
            State::FinWait1 => "FIN_WAIT_1",
            State::FinWait2 => "FIN_WAIT_2",
            State::CloseWait => "CLOSE_WAIT",
            State::Closing => "CLOSING",
            State::LastAck => "LAST_ACK",
            State::TimeWait => "TIME_WAIT",
        }
    }
}

/// A connection's transmission control block.
pub struct Tcb {
    pub state: State,
    pub local: Endpoint,
    pub remote: Endpoint,
    /// Opened by a listening socket.
    pub passive: bool,
    /// Why the connection ended abnormally.
    pub error: Option<Error>,
    /// The application has closed its socket: nobody reads any more.
    pub orphaned: bool,

    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    /// Highest sequence number sent so far (+1).
    snd_max: u32,
    snd_wnd: u32,
    snd_wl1: u32,
    snd_wl2: u32,
    /// Largest segment the peer takes.
    mss: u32,
    /// Our MSS (announced in the SYN).
    local_mss: u16,
    cwnd: u32,
    ssthresh: u32,
    dupacks: u32,
    /// Unacknowledged and unsent data; the first byte has sequence number
    /// `buf_seq`.
    sndbuf: VecDeque<u8>,
    buf_seq: u32,
    sndbuf_cap: usize,
    /// The application closed the sending side: FIN after the data.
    fin_pending: bool,
    /// Sequence number of our FIN once sent.
    fin_seq: Option<u32>,

    irs: u32,
    rcv_nxt: u32,
    rcvbuf: VecDeque<u8>,
    rcvbuf_cap: usize,
    /// Right edge of the window last announced.
    rcv_adv: u32,
    fin_received: bool,
    ack_now: bool,

    rto: u64,
    srtt: Option<u64>,
    rttvar: u64,
    /// RTT measurement in progress: sequence number to be acked, send time.
    rtt_sample: Option<(u32, u64)>,
    rtx_at: Option<u64>,
    rtx_count: u32,
    /// Send one byte beyond a zero window (persist timer fired).
    probe: bool,
    /// TIME_WAIT or orphaned FIN_WAIT_2 deadline.
    linger_at: Option<u64>,
    /// A reset to send (answer to an unacceptable segment, or abort).
    pending_rst: Option<Segment>,
}

impl Tcb {
    fn new(local: Endpoint, remote: Endpoint, iss: u32, local_mss: u16) -> Tcb {
        Tcb {
            state: State::Closed,
            local,
            remote,
            passive: false,
            error: None,
            orphaned: false,
            iss,
            snd_una: iss,
            snd_nxt: iss,
            snd_max: iss,
            snd_wnd: 0,
            snd_wl1: 0,
            snd_wl2: 0,
            mss: DEFAULT_MSS as u32,
            local_mss,
            cwnd: 0,
            ssthresh: 65535,
            dupacks: 0,
            sndbuf: VecDeque::new(),
            buf_seq: iss.wrapping_add(1),
            sndbuf_cap: SEND_BUFFER,
            fin_pending: false,
            fin_seq: None,
            irs: 0,
            rcv_nxt: 0,
            rcvbuf: VecDeque::new(),
            rcvbuf_cap: RECV_BUFFER,
            rcv_adv: 0,
            fin_received: false,
            ack_now: false,
            rto: INITIAL_RTO,
            srtt: None,
            rttvar: 0,
            rtt_sample: None,
            rtx_at: None,
            rtx_count: 0,
            probe: false,
            linger_at: None,
            pending_rst: None,
        }
    }

    /// Active open: SYN_SENT; the SYN goes out with the next `output`.
    pub fn connect(local: Endpoint, remote: Endpoint, iss: u32, local_mss: u16) -> Tcb {
        let mut t = Tcb::new(local, remote, iss, local_mss);
        t.state = State::SynSent;
        t
    }

    /// Passive open: a listening socket received `syn`; SYN_RECEIVED, the
    /// SYN-ACK goes out with the next `output`.
    pub fn accept(local: Endpoint, remote: Endpoint, syn: &Segment, iss: u32, local_mss: u16) -> Tcb {
        let mut t = Tcb::new(local, remote, iss, local_mss);
        t.state = State::SynReceived;
        t.passive = true;
        t.irs = syn.seq;
        t.rcv_nxt = syn.seq.wrapping_add(1);
        t.peer_syn(syn);
        t
    }

    /// Options and window of the peer's SYN.
    fn peer_syn(&mut self, syn: &Segment) {
        self.mss = syn.mss.unwrap_or(DEFAULT_MSS).clamp(64, self.local_mss.max(64)) as u32;
        self.snd_wnd = syn.window as u32;
        self.snd_wl1 = syn.seq;
        self.snd_wl2 = syn.ack;
        // RFC 5681 initial window.
        self.cwnd = (4 * self.mss).min((2 * self.mss).max(4380));
    }

    pub fn set_buffers(&mut self, send: usize, recv: usize) {
        self.sndbuf_cap = send.max(1);
        self.rcvbuf_cap = recv.clamp(1, 65535);
    }

    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    /// Data can be sent: the connection is (or was) synchronised and the
    /// sending side is open.
    pub fn can_send(&self) -> bool {
        matches!(self.state, State::Established | State::CloseWait) && !self.fin_pending
    }

    pub fn is_connecting(&self) -> bool {
        matches!(self.state, State::SynSent | State::SynReceived)
    }

    /// Bytes waiting to be read.
    pub fn available(&self) -> usize {
        self.rcvbuf.len()
    }

    /// The peer closed its side and everything before the FIN was read.
    pub fn at_eof(&self) -> bool {
        self.fin_received && self.rcvbuf.is_empty()
    }

    /// Free space in the send buffer.
    pub fn send_space(&self) -> usize {
        self.sndbuf_cap.saturating_sub(self.sndbuf.len())
    }

    /// Bytes sent but not acknowledged, plus unsent ones.
    pub fn send_queued(&self) -> usize {
        self.sndbuf.len()
    }

    /// Queue data; returns how much fitted.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, Error> {
        if let Some(e) = self.error {
            return Err(e);
        }
        match self.state {
            State::Established | State::CloseWait if !self.fin_pending => {}
            State::SynSent | State::SynReceived if !self.fin_pending => {}
            State::Closed => return Err(Error::NotConnected),
            _ => return Err(Error::Shutdown),
        }
        let n = data.len().min(self.send_space());
        self.sndbuf.extend(&data[..n]);
        Ok(n)
    }

    /// Take received data.
    pub fn recv(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.rcvbuf.len());
        for (d, s) in buf.iter_mut().zip(self.rcvbuf.drain(..n)) {
            *d = s;
        }
        if n > 0 && self.synchronised() && !self.fin_received {
            // Announce a window that opened by a segment or half the buffer.
            let edge = self.rcv_nxt.wrapping_add(self.rcv_wnd());
            let grown = edge.wrapping_sub(self.rcv_adv);
            if self.rcv_adv == self.rcv_nxt || grown >= self.mss.min(self.rcvbuf_cap as u32 / 2) {
                self.ack_now = true;
            }
        }
        n
    }

    /// Copy received data without taking it.
    pub fn peek(&self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.rcvbuf.len());
        for (d, s) in buf.iter_mut().zip(self.rcvbuf.iter()) {
            *d = *s;
        }
        n
    }

    /// The application closed its socket: nobody reads any more, and a
    /// connection stuck in FIN_WAIT_2 is dropped after a while.
    pub fn orphan(&mut self, now: u64) {
        self.orphaned = true;
        if self.state == State::FinWait2 && self.linger_at.is_none() {
            self.linger_at = Some(now + FIN_WAIT2_TIMEOUT);
        }
    }

    /// Discard received data (shutdown for reading).
    pub fn discard_received(&mut self) {
        let n = self.rcvbuf.len();
        self.rcvbuf.clear();
        if n > 0 {
            self.ack_now = true;
        }
    }

    /// Close the sending side (FIN after the queued data).
    pub fn close(&mut self) {
        match self.state {
            State::Listen | State::SynSent => self.state = State::Closed,
            State::SynReceived | State::Established => {
                self.fin_pending = true;
                self.state = State::FinWait1;
            }
            State::CloseWait => {
                self.fin_pending = true;
                self.state = State::LastAck;
            }
            _ => {}
        }
    }

    /// Abort: a reset (if synchronised) and CLOSED.
    pub fn abort(&mut self, out: &mut Vec<Segment>) {
        if matches!(self.state, State::SynReceived | State::Established | State::FinWait1 | State::FinWait2 | State::CloseWait) {
            let mut s = self.segment(self.snd_nxt, RST | ACK);
            s.window = 0;
            out.push(s);
        }
        self.state = State::Closed;
    }

    fn synchronised(&self) -> bool {
        !matches!(self.state, State::Closed | State::Listen | State::SynSent)
    }

    fn rcv_wnd(&self) -> u32 {
        self.rcvbuf_cap.saturating_sub(self.rcvbuf.len()).min(65535) as u32
    }

    fn segment(&self, seq: u32, flags: u8) -> Segment {
        Segment {
            src_port: self.local.port,
            dst_port: self.remote.port,
            seq,
            ack: if flags & ACK != 0 { self.rcv_nxt } else { 0 },
            flags,
            window: self.rcv_wnd() as u16,
            mss: None,
            data: Vec::new(),
        }
    }

    fn enter_time_wait(&mut self, now: u64) {
        self.state = State::TimeWait;
        self.linger_at = Some(now + TIME_WAIT);
        self.rtx_at = None;
    }

    /// Process an arriving segment (RFC 9293 3.10.7).
    pub fn input(&mut self, seg: &Segment, now: u64) {
        match self.state {
            State::Closed | State::Listen => {}
            State::SynSent => self.input_syn_sent(seg, now),
            _ => self.input_synchronised(seg, now),
        }
    }

    fn input_syn_sent(&mut self, seg: &Segment, now: u64) {
        let ack_ok = seg.has(ACK) && gt(seg.ack, self.iss) && le(seg.ack, self.snd_max);
        if seg.has(ACK) && !ack_ok {
            // Answered by the stack's reset logic: the caller sees no state.
            if !seg.has(RST) {
                self.ack_now = false;
                self.pending_rst = Some(Segment {
                    src_port: self.local.port,
                    dst_port: self.remote.port,
                    seq: seg.ack,
                    flags: RST,
                    ..Default::default()
                });
            }
            return;
        }
        if seg.has(RST) {
            if ack_ok {
                self.error = Some(Error::ConnRefused);
                self.state = State::Closed;
            }
            return;
        }
        if !seg.has(SYN) {
            return;
        }
        self.irs = seg.seq;
        self.rcv_nxt = seg.seq.wrapping_add(1);
        self.peer_syn(seg);
        if ack_ok {
            self.ack_advance(seg.ack, now);
            self.state = State::Established;
            self.ack_now = true;
        } else {
            // Simultaneous open: SYN-ACK with our original ISS.
            self.state = State::SynReceived;
            self.snd_nxt = self.iss;
        }
        // Data in the SYN is dropped (rare); the peer retransmits it.
    }

    fn acceptable(&self, seq: u32, len: u32, wnd: u32) -> bool {
        let in_wnd = |s: u32| le(self.rcv_nxt, s) && lt(s, self.rcv_nxt.wrapping_add(wnd));
        match (len, wnd) {
            (0, 0) => seq == self.rcv_nxt,
            (0, _) => in_wnd(seq),
            (_, 0) => false,
            _ => in_wnd(seq) || in_wnd(seq.wrapping_add(len - 1)),
        }
    }

    fn input_synchronised(&mut self, seg: &Segment, now: u64) {
        let wnd = self.rcv_wnd();
        let mut seq = seg.seq;
        let mut data: &[u8] = &seg.data;
        let mut syn = seg.has(SYN);
        let mut fin = seg.has(FIN);
        // 1. Sequence number check.
        if !self.acceptable(seq, seg.seq_len(), wnd) {
            if wnd == 0 && seq == self.rcv_nxt {
                // Zero window: take the ACK (and RST), not the data.
                data = &[];
                fin = false;
            } else {
                if !seg.has(RST) {
                    self.ack_now = true;
                    if self.state == State::TimeWait && fin {
                        self.linger_at = Some(now + TIME_WAIT);
                    }
                }
                return;
            }
        }
        // Trim what lies before RCV.NXT and beyond the window.
        if syn && lt(seq, self.rcv_nxt) {
            syn = false;
            seq = seq.wrapping_add(1);
        }
        if lt(seq, self.rcv_nxt) {
            let dup = self.rcv_nxt.wrapping_sub(seq) as usize;
            if dup > data.len() {
                data = &[];
                fin = false;
            } else {
                data = &data[dup..];
            }
            seq = self.rcv_nxt;
        }
        if data.len() > wnd as usize {
            data = &data[..wnd as usize];
            fin = false;
        }
        // 2. Reset: only exactly at RCV.NXT (RFC 5961 3.2).
        if seg.has(RST) {
            if seg.seq != self.rcv_nxt {
                self.ack_now = true;
                return;
            }
            self.error = match self.state {
                State::SynReceived if self.passive => None,
                State::SynReceived => Some(Error::ConnRefused),
                State::Established | State::FinWait1 | State::FinWait2 | State::CloseWait => Some(Error::ConnReset),
                _ => None,
            };
            self.state = State::Closed;
            return;
        }
        // 4. A SYN in the window: challenge ACK (RFC 5961 4.2).
        if syn {
            self.ack_now = true;
            return;
        }
        // 5. ACK.
        if !seg.has(ACK) {
            return;
        }
        if self.state == State::SynReceived {
            if lt(self.snd_una, seg.ack) && le(seg.ack, self.snd_max) {
                self.state = State::Established;
                self.snd_wnd = seg.window as u32;
                self.snd_wl1 = seg.seq;
                self.snd_wl2 = seg.ack;
            } else {
                self.pending_rst = Some(Segment {
                    src_port: self.local.port,
                    dst_port: self.remote.port,
                    seq: seg.ack,
                    flags: RST,
                    ..Default::default()
                });
                return;
            }
        }
        if gt(seg.ack, self.snd_max) {
            self.ack_now = true;
            return;
        }
        if gt(seg.ack, self.snd_una) {
            self.ack_advance(seg.ack, now);
        } else if seg.ack == self.snd_una
            && seg.data.is_empty()
            && !fin
            && seg.window as u32 == self.snd_wnd
            && self.snd_una != self.snd_max
        {
            self.dupacks += 1;
            if self.dupacks == 3 {
                // Fast retransmit.
                let flight = self.snd_max.wrapping_sub(self.snd_una);
                self.ssthresh = (flight / 2).max(2 * self.mss);
                self.cwnd = self.ssthresh + 3 * self.mss;
                self.snd_nxt = self.snd_una;
                self.rtt_sample = None;
            }
        }
        // Window update (not from old segments).
        if lt(self.snd_wl1, seg.seq) || (self.snd_wl1 == seg.seq && le(self.snd_wl2, seg.ack)) {
            self.snd_wnd = seg.window as u32;
            self.snd_wl1 = seg.seq;
            self.snd_wl2 = seg.ack;
            if self.snd_wnd > 0 && self.probe {
                self.probe = false;
            }
        }
        if self.snd_wnd == 0 {
            // The peer is alive: probing goes on as long as it answers.
            self.rtx_count = 0;
        }
        let fin_acked = self.fin_seq.is_some_and(|f| gt(self.snd_una, f));
        match self.state {
            State::FinWait1 if fin_acked => {
                self.state = State::FinWait2;
                if self.orphaned {
                    self.linger_at = Some(now + FIN_WAIT2_TIMEOUT);
                }
            }
            State::Closing if fin_acked => self.enter_time_wait(now),
            State::LastAck if fin_acked => {
                self.state = State::Closed;
                return;
            }
            State::TimeWait => {
                if fin {
                    self.ack_now = true;
                    self.linger_at = Some(now + TIME_WAIT);
                }
                return;
            }
            _ => {}
        }
        // 7. Data.
        if !data.is_empty() {
            match self.state {
                State::Established | State::FinWait1 | State::FinWait2 => {
                    if seq == self.rcv_nxt {
                        let room = self.rcvbuf_cap.saturating_sub(self.rcvbuf.len());
                        let n = data.len().min(room);
                        self.rcvbuf.extend(&data[..n]);
                        self.rcv_nxt = self.rcv_nxt.wrapping_add(n as u32);
                        if n < data.len() {
                            fin = false;
                        }
                    } else {
                        // Out of order: dropped; the duplicate ACK says
                        // what is missing.
                        fin = false;
                    }
                    self.ack_now = true;
                }
                _ => {}
            }
        }
        // 8. FIN, if everything before it is in.
        if fin && seq.wrapping_add(data.len() as u32) == self.rcv_nxt && !self.fin_received {
            self.fin_received = true;
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            self.ack_now = true;
            match self.state {
                State::SynReceived | State::Established => self.state = State::CloseWait,
                State::FinWait1 => {
                    if self.fin_seq.is_some_and(|f| gt(self.snd_una, f)) {
                        self.enter_time_wait(now);
                    } else {
                        self.state = State::Closing;
                    }
                }
                State::FinWait2 => self.enter_time_wait(now),
                _ => {}
            }
        } else if fin && self.fin_received && self.state == State::TimeWait {
            self.ack_now = true;
            self.linger_at = Some(now + TIME_WAIT);
        }
    }

    /// The peer acknowledged everything below `ack`.
    fn ack_advance(&mut self, ack: u32, now: u64) {
        let acked = ack.wrapping_sub(self.snd_una);
        // Data bytes acknowledged.
        if gt(ack, self.buf_seq) {
            let n = (ack.wrapping_sub(self.buf_seq) as usize).min(self.sndbuf.len());
            self.sndbuf.drain(..n);
            self.buf_seq = self.buf_seq.wrapping_add(n as u32);
        }
        self.snd_una = ack;
        if lt(self.snd_nxt, ack) {
            self.snd_nxt = ack;
        }
        // RTT sample (Karn: cleared on retransmission).
        if let Some((s, t)) = self.rtt_sample {
            if ge(ack, s) {
                self.rtt_sample = None;
                self.rtt_update(now.saturating_sub(t));
            }
        }
        // Congestion window.
        if self.dupacks >= 3 {
            self.cwnd = self.ssthresh;
        } else if self.cwnd < self.ssthresh {
            self.cwnd += acked.min(self.mss);
        } else {
            self.cwnd += (self.mss * self.mss / self.cwnd.max(1)).max(1);
        }
        self.dupacks = 0;
        self.rtx_count = 0;
        self.rtx_at = if self.snd_una == self.snd_max { None } else { Some(now + self.rto) };
    }

    /// RFC 6298 estimator.
    fn rtt_update(&mut self, r: u64) {
        match self.srtt {
            None => {
                self.srtt = Some(r);
                self.rttvar = r / 2;
            }
            Some(s) => {
                let diff = s.abs_diff(r);
                self.rttvar = (3 * self.rttvar + diff) / 4;
                self.srtt = Some((7 * s + r) / 8);
            }
        }
        self.rto = (self.srtt.unwrap() + (4 * self.rttvar).max(10)).clamp(MIN_RTO, MAX_RTO);
    }

    /// Timers: retransmission/persist, TIME_WAIT and the orphan timeout.
    pub fn poll(&mut self, now: u64) {
        if let Some(t) = self.linger_at {
            if now >= t && matches!(self.state, State::TimeWait | State::FinWait2) {
                self.state = State::Closed;
                return;
            }
        }
        let Some(t) = self.rtx_at else { return };
        if now < t {
            return;
        }
        self.rtx_at = None;
        if self.snd_una == self.snd_max {
            // Persist timer: probe a zero window.
            if self.sndbuf.len() > self.snd_nxt.wrapping_sub(self.buf_seq) as usize {
                self.probe = true;
                self.rto = (self.rto * 2).min(MAX_RTO);
            }
            return;
        }
        self.rtx_count += 1;
        let limit = if self.is_connecting() { SYN_RETRIES } else { DATA_RETRIES };
        if self.rtx_count > limit {
            self.error = Some(Error::TimedOut);
            let mut out = Vec::new();
            self.abort(&mut out);
            self.pending_rst = out.pop();
            return;
        }
        let flight = self.snd_max.wrapping_sub(self.snd_una);
        self.ssthresh = (flight / 2).max(2 * self.mss);
        self.cwnd = self.mss;
        self.rto = (self.rto * 2).min(MAX_RTO);
        self.snd_nxt = self.snd_una;
        self.rtt_sample = None;
        self.dupacks = 0;
        // A probe into a still-closed window goes out again.
        self.probe = self.snd_wnd == 0;
    }

    /// The earliest time `poll` has work to do.
    pub fn next_timer(&self) -> Option<u64> {
        match (self.rtx_at, self.linger_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Produce the segments that are due.
    pub fn output(&mut self, now: u64, out: &mut Vec<Segment>) {
        if let Some(r) = self.pending_rst.take() {
            out.push(r);
        }
        let start = out.len();
        let unacked_syn = self.snd_una == self.iss;
        match self.state {
            State::Closed | State::Listen | State::TimeWait => {}
            State::SynSent | State::SynReceived | State::FinWait1 if unacked_syn => {
                // Our SYN (again after a timeout); no data before it is
                // acknowledged.
                if self.snd_nxt == self.iss {
                    let flags = if self.state == State::SynSent { SYN } else { SYN | ACK };
                    let mut s = self.segment(self.iss, flags);
                    s.mss = Some(self.local_mss);
                    out.push(s);
                    self.sent(self.iss.wrapping_add(1), now);
                }
            }
            _ => self.output_data(now, out),
        }
        if out.len() == start && self.ack_now && self.synchronised() && self.state != State::Closed {
            out.push(self.segment(self.snd_nxt, ACK));
        }
        if out.len() > start || self.ack_now {
            self.rcv_adv = self.rcv_nxt.wrapping_add(self.rcv_wnd());
        }
        self.ack_now = false;
    }

    fn output_data(&mut self, now: u64, out: &mut Vec<Segment>) {
        loop {
            let off = self.snd_nxt.wrapping_sub(self.buf_seq) as usize;
            let unsent = self.sndbuf.len().saturating_sub(off);
            let edge = self.snd_una.wrapping_add(self.snd_wnd.min(self.cwnd.max(self.mss)));
            let usable = if lt(self.snd_nxt, edge) { edge.wrapping_sub(self.snd_nxt) as usize } else { 0 };
            if unsent > 0 && (usable > 0 || self.probe) {
                let n = if usable > 0 { unsent.min(usable).min(self.mss as usize) } else { 1 };
                self.probe = false;
                let mut s = self.segment(self.snd_nxt, ACK);
                s.data = self.sndbuf.range(off..off + n).copied().collect();
                if n == unsent {
                    s.flags |= PSH;
                }
                out.push(s);
                self.sent(self.snd_nxt.wrapping_add(n as u32), now);
                continue;
            }
            if unsent > 0 && self.snd_una == self.snd_max && self.rtx_at.is_none() {
                // Window closed and nothing in flight: persist timer.
                self.rtx_at = Some(now + self.rto);
            }
            if unsent == 0 && self.fin_pending && off == self.sndbuf.len() {
                let seq = self.snd_nxt;
                out.push(self.segment(seq, FIN | ACK));
                self.fin_seq = Some(seq);
                self.sent(seq.wrapping_add(1), now);
            }
            break;
        }
    }

    /// Bookkeeping after sending up to `end`.
    fn sent(&mut self, end: u32, now: u64) {
        if gt(end, self.snd_max) {
            if self.rtt_sample.is_none() && self.rtx_count == 0 {
                self.rtt_sample = Some((end, now));
            }
            self.snd_max = end;
        }
        self.snd_nxt = end;
        if self.rtx_at.is_none() {
            self.rtx_at = Some(now + self.rto);
        }
    }

    pub fn rto(&self) -> u64 {
        self.rto
    }
}

#[cfg(test)]
mod tests;
