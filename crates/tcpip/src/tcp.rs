//! TCP (RFC 9293): the segment format and the connection state machine.
//!
//! A [`Tcb`] is one connection. It is fed the segments that arrive for
//! it ([`Tcb::input`]), the passage of time ([`Tcb::poll`]) and the
//! application's calls ([`Tcb::send`], [`Tcb::recv`], [`Tcb::close`]);
//! [`Tcb::output`] produces the segments to transmit. Demultiplexing,
//! listening sockets and the IP layer are the stack's business.
//!
//! * Retransmission goes back to the oldest unacknowledged byte, on the
//!   retransmission timer (RFC 6298 estimator, Karn's rule or timestamp
//!   samples, binary back-off) or on three duplicate ACKs, skipping what
//!   the peer reported with SACK blocks (RFC 2018).
//! * Congestion control is slow start and congestion avoidance
//!   (RFC 5681).
//! * Out-of-order segments are kept (up to the window) and joined to the
//!   stream when the gap is filled; while they wait, ACKs carry SACK
//!   blocks for them.
//! * Window scaling and timestamps (RFC 7323, with PAWS) and
//!   SACK-permitted are offered in every SYN and used when the peer
//!   offers them too.
//! * The receive window is the free space of the receive buffer (up to
//!   [`MAX_BUFFER`] with window scaling); a window that opens by at least
//!   one segment or half the buffer is announced at once. A zero send
//!   window is probed with one byte on the persist timer.
//! * ACKs are delayed ([`DELAYED_ACK`]) unless two full segments are
//!   unacknowledged, data arrived out of order or filled a gap; small
//!   segments wait while data is in flight (Nagle, RFC 896) unless
//!   `nodelay` is set.
//! * Keepalives (RFC 1122 4.2.3.6): after [`KEEPIDLE`] without a segment
//!   from the peer, probes every [`KEEPINTVL`]; [`KEEPCNT`] unanswered
//!   ones end the connection with ETIMEDOUT.
//! * Urgent data (RFC 9293 3.8.5, BSD semantics): the urgent pointer
//!   points just past the urgent byte; on input that byte is taken out of
//!   the stream (or left in it with `oobinline`) and the mark kept, and
//!   reads stop at the mark.
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
/// Largest receive buffer (window scaling makes windows above 64 KB).
pub const MAX_BUFFER: usize = 1 << 20;
/// How long an ACK may wait for more data or an answer to ride on.
pub const DELAYED_ACK: u64 = 100;
/// Keepalive defaults (BSD's): idle time, probe interval, probes.
pub const KEEPIDLE: u64 = 7_200_000;
pub const KEEPINTVL: u64 = 75_000;
pub const KEEPCNT: u32 = 8;
/// Largest window shift (RFC 7323 2.3).
pub const MAX_WSCALE: u8 = 14;
/// Out-of-order segments kept per connection.
const MAX_OOO: usize = 64;
/// Bytes of the timestamps option (with its padding).
const TS_LEN: u32 = 12;

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
    /// The window field as sent (scaled).
    pub window: u16,
    /// Urgent pointer (with URG).
    pub urgent: u16,
    /// The MSS option (SYN segments).
    pub mss: Option<u16>,
    /// The window scale option (SYN segments).
    pub wscale: Option<u8>,
    /// SACK-permitted (SYN segments).
    pub sack_ok: bool,
    /// Timestamps option: TSval, TSecr.
    pub ts: Option<(u32, u32)>,
    /// SACK blocks: [left, right) edges.
    pub sack: Vec<(u32, u32)>,
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
    let mut s = Segment {
        src_port: be16(b, 0),
        dst_port: be16(b, 2),
        seq: be32(b, 4),
        ack: be32(b, 8),
        flags: b[13] & 0x3f,
        window: be16(b, 14),
        urgent: be16(b, 18),
        ..Default::default()
    };
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
                match (kind, len) {
                    (2, 4) => s.mss = Some(be16(b, i + 2)),
                    (3, 3) => s.wscale = Some(b[i + 2].min(MAX_WSCALE)),
                    (4, 2) => s.sack_ok = true,
                    (5, _) if len >= 10 && (len - 2) % 8 == 0 => {
                        for k in 0..(len - 2) / 8 {
                            let o = i + 2 + 8 * k;
                            s.sack.push((be32(b, o), be32(b, o + 4)));
                        }
                    }
                    (8, 10) => s.ts = Some((be32(b, i + 2), be32(b, i + 6))),
                    _ => {}
                }
                i += len;
            }
        }
    }
    s.data = b[off..].to_vec();
    Some(s)
}

/// The options of a segment, padded to a multiple of 4 bytes.
fn options(s: &Segment) -> Vec<u8> {
    let mut o = Vec::new();
    if let Some(m) = s.mss {
        o.extend_from_slice(&[2, 4]);
        o.extend_from_slice(&m.to_be_bytes());
    }
    match s.ts {
        Some((v, e)) => {
            o.extend_from_slice(if s.sack_ok { &[4, 2, 8, 10] } else { &[1, 1, 8, 10] });
            o.extend_from_slice(&v.to_be_bytes());
            o.extend_from_slice(&e.to_be_bytes());
        }
        None if s.sack_ok => o.extend_from_slice(&[1, 1, 4, 2]),
        None => {}
    }
    if let Some(w) = s.wscale {
        o.extend_from_slice(&[1, 3, 3, w]);
    }
    let n = s.sack.len().min(40usize.saturating_sub(o.len() + 4) / 8);
    if n > 0 {
        o.extend_from_slice(&[1, 1, 5, 2 + 8 * n as u8]);
        for &(l, r) in &s.sack[..n] {
            o.extend_from_slice(&l.to_be_bytes());
            o.extend_from_slice(&r.to_be_bytes());
        }
    }
    o
}

/// Encode a segment from `src` to `dst`.
pub fn build(src: Ipv4, dst: Ipv4, s: &Segment) -> Vec<u8> {
    let opt = options(s);
    let mut v = Vec::with_capacity(HEADER + opt.len() + s.data.len());
    v.extend_from_slice(&s.src_port.to_be_bytes());
    v.extend_from_slice(&s.dst_port.to_be_bytes());
    v.extend_from_slice(&s.seq.to_be_bytes());
    v.extend_from_slice(&s.ack.to_be_bytes());
    v.push((((HEADER + opt.len()) / 4) as u8) << 4);
    v.push(s.flags);
    v.extend_from_slice(&s.window.to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&(if s.has(URG) { s.urgent } else { 0 }).to_be_bytes());
    v.extend_from_slice(&opt);
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

/// Keepalive timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keepalive {
    pub idle: u64,
    pub interval: u64,
    pub count: u32,
}

impl Default for Keepalive {
    fn default() -> Keepalive {
        Keepalive { idle: KEEPIDLE, interval: KEEPINTVL, count: KEEPCNT }
    }
}

/// Counters of a connection (and, summed, of the stack).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcpStats {
    pub segs_sent: u64,
    pub segs_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub retransmits: u64,
    pub retransmitted_bytes: u64,
    pub timeouts: u64,
    pub out_of_order: u64,
    pub duplicates: u64,
    pub delayed_acks: u64,
    pub keepalive_probes: u64,
    pub keepalive_drops: u64,
    pub sack_blocks: u64,
    pub urgent_received: u64,
}

impl TcpStats {
    pub fn add(&mut self, o: &TcpStats) {
        self.segs_sent += o.segs_sent;
        self.segs_received += o.segs_received;
        self.bytes_sent += o.bytes_sent;
        self.bytes_received += o.bytes_received;
        self.retransmits += o.retransmits;
        self.retransmitted_bytes += o.retransmitted_bytes;
        self.timeouts += o.timeouts;
        self.out_of_order += o.out_of_order;
        self.duplicates += o.duplicates;
        self.delayed_acks += o.delayed_acks;
        self.keepalive_probes += o.keepalive_probes;
        self.keepalive_drops += o.keepalive_drops;
        self.sack_blocks += o.sack_blocks;
        self.urgent_received += o.urgent_received;
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
    /// Send small segments at once (TCP_NODELAY): no Nagle.
    pub nodelay: bool,
    /// Leave urgent data in the stream (SO_OOBINLINE).
    pub oobinline: bool,
    pub stats: TcpStats,

    now: u64,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    /// Highest sequence number sent so far (+1).
    snd_max: u32,
    snd_wnd: u32,
    max_sndwnd: u32,
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
    /// What the peer reported received beyond SND.UNA (SACK).
    sacked: Vec<(u32, u32)>,
    /// Send urgent pointer: just past the last urgent byte.
    snd_up: Option<u32>,

    // Options.
    /// Window scaling agreed: the peer's shift and ours.
    snd_wscale: u8,
    rcv_wscale: u8,
    ws_ok: bool,
    ts_ok: bool,
    ts_offset: u32,
    ts_recent: u32,
    sack_ok: bool,

    irs: u32,
    rcv_nxt: u32,
    rcvbuf: VecDeque<u8>,
    rcvbuf_cap: usize,
    /// Right edge of the window last announced.
    rcv_adv: u32,
    /// RCV.NXT in the last ACK sent.
    last_ack_sent: u32,
    fin_received: bool,
    ack_now: bool,
    delack_at: Option<u64>,
    /// Segments beyond RCV.NXT, sorted, not overlapping.
    ooo: Vec<(u32, Vec<u8>)>,
    /// Sequence number of a FIN that arrived out of order.
    ooo_fin: Option<u32>,
    /// Start of the out-of-order block last added to (first SACK block).
    last_ooo: Option<u32>,
    /// Receive urgent pointer whose byte has not arrived yet.
    rcv_up: Option<u32>,
    /// Highest urgent pointer seen.
    rcv_up_seen: Option<u32>,
    /// The urgent byte taken out of the stream.
    oob_byte: Option<u8>,
    /// Bytes in the receive buffer before the urgent mark.
    oob_mark: Option<usize>,

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

    keepalive: Option<Keepalive>,
    /// When the peer was last heard from.
    last_rx: u64,
    ka_probes: u32,
    ka_send: bool,
}

/// The smallest window shift that lets a 16-bit window cover `cap`.
fn shift_for(cap: usize) -> u8 {
    let mut s = 0;
    while s < MAX_WSCALE && (cap >> s) > 65535 {
        s += 1;
    }
    s
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
            nodelay: false,
            oobinline: false,
            stats: TcpStats::default(),
            now: 0,
            iss,
            snd_una: iss,
            snd_nxt: iss,
            snd_max: iss,
            snd_wnd: 0,
            max_sndwnd: 0,
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
            sacked: Vec::new(),
            snd_up: None,
            snd_wscale: 0,
            rcv_wscale: 0,
            ws_ok: false,
            ts_ok: false,
            ts_offset: iss.rotate_left(13) ^ 0x5a5a_0000,
            ts_recent: 0,
            sack_ok: false,
            irs: 0,
            rcv_nxt: 0,
            rcvbuf: VecDeque::new(),
            rcvbuf_cap: RECV_BUFFER,
            rcv_adv: 0,
            last_ack_sent: 0,
            fin_received: false,
            ack_now: false,
            delack_at: None,
            ooo: Vec::new(),
            ooo_fin: None,
            last_ooo: None,
            rcv_up: None,
            rcv_up_seen: None,
            oob_byte: None,
            oob_mark: None,
            rto: INITIAL_RTO,
            srtt: None,
            rttvar: 0,
            rtt_sample: None,
            rtx_at: None,
            rtx_count: 0,
            probe: false,
            linger_at: None,
            pending_rst: None,
            keepalive: None,
            last_rx: 0,
            ka_probes: 0,
            ka_send: false,
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
        t.last_ack_sent = t.rcv_nxt;
        t.peer_syn(syn);
        t
    }

    /// Options and window of the peer's SYN. We offer every option in our
    /// SYN (and answer a SYN only with the options it had), so what the
    /// peer's SYN carries is what is agreed.
    fn peer_syn(&mut self, syn: &Segment) {
        self.mss = syn.mss.unwrap_or(DEFAULT_MSS).clamp(64, self.local_mss.max(64)) as u32;
        self.snd_wnd = syn.window as u32;
        self.max_sndwnd = self.snd_wnd;
        self.snd_wl1 = syn.seq;
        self.snd_wl2 = syn.ack;
        if let Some(w) = syn.wscale {
            self.ws_ok = true;
            self.snd_wscale = w;
            self.rcv_wscale = shift_for(self.rcvbuf_cap);
        }
        if let Some((v, _)) = syn.ts {
            self.ts_ok = true;
            self.ts_recent = v;
        }
        self.sack_ok = syn.sack_ok;
        // RFC 5681 initial window.
        let smss = self.smss();
        self.cwnd = (4 * smss).min((2 * smss).max(4380));
    }

    pub fn set_buffers(&mut self, send: usize, recv: usize) {
        self.sndbuf_cap = send.max(1);
        self.rcvbuf_cap = recv.clamp(1, MAX_BUFFER);
        if self.ws_ok && self.state == State::SynReceived {
            self.rcv_wscale = shift_for(self.rcvbuf_cap);
        }
    }

    /// Turn keepalives on (with their timing) or off.
    pub fn set_keepalive(&mut self, k: Option<Keepalive>) {
        self.keepalive = k;
        self.ka_probes = 0;
        if self.last_rx == 0 {
            self.last_rx = self.now;
        }
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

    /// Which options were agreed: window scaling (our shift, the peer's),
    /// timestamps, SACK.
    pub fn options(&self) -> (Option<(u8, u8)>, bool, bool) {
        (self.ws_ok.then_some((self.rcv_wscale, self.snd_wscale)), self.ts_ok, self.sack_ok)
    }

    /// Smoothed round-trip time and the retransmission timeout (ms).
    pub fn rtt(&self) -> (Option<u64>, u64) {
        (self.srtt, self.rto)
    }

    /// Send and congestion windows.
    pub fn windows(&self) -> (u32, u32, u32) {
        (self.snd_wnd, self.cwnd, self.rcv_wnd())
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

    /// Queue urgent data (MSG_OOB): the urgent pointer moves past its last
    /// byte. All of it must fit.
    pub fn send_urgent(&mut self, data: &[u8]) -> Result<usize, Error> {
        if data.len() > self.send_space() {
            if let Some(e) = self.error {
                return Err(e);
            }
            return Err(Error::WouldBlock);
        }
        let n = self.send(data)?;
        self.snd_up = Some(self.buf_seq.wrapping_add(self.sndbuf.len() as u32));
        Ok(n)
    }

    /// Take received data. Reads stop at the urgent mark.
    pub fn recv(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.rcvbuf.len()).min(self.mark_limit());
        for (d, s) in buf.iter_mut().zip(self.rcvbuf.drain(..n)) {
            *d = s;
        }
        if n > 0 {
            self.oob_mark = match self.oob_mark {
                Some(m) if m >= n => Some(m - n),
                _ => None,
            };
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

    fn mark_limit(&self) -> usize {
        match self.oob_mark {
            Some(m) if m > 0 => m,
            _ => usize::MAX,
        }
    }

    /// Copy received data without taking it.
    pub fn peek(&self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.rcvbuf.len()).min(self.mark_limit());
        for (d, s) in buf.iter_mut().zip(self.rcvbuf.iter()) {
            *d = *s;
        }
        n
    }

    /// The urgent byte (recv with MSG_OOB): EINVAL with SO_OOBINLINE or
    /// when there is none, EWOULDBLOCK while it is announced but has not
    /// arrived.
    pub fn recv_urgent(&mut self, peek: bool) -> Result<u8, Error> {
        if self.oobinline {
            return Err(Error::InvalidArg);
        }
        match self.oob_byte {
            Some(b) => {
                if !peek {
                    self.oob_byte = None;
                }
                Ok(b)
            }
            None if self.rcv_up.is_some() => Err(Error::WouldBlock),
            None => Err(Error::InvalidArg),
        }
    }

    /// The next byte read is the one after (or, inline, at) the urgent
    /// mark (SIOCATMARK).
    pub fn at_mark(&self) -> bool {
        self.oob_mark == Some(0)
    }

    /// Urgent data is pending (select's exception set).
    pub fn urgent_pending(&self) -> bool {
        self.oob_mark.is_some() || self.rcv_up.is_some() || self.oob_byte.is_some()
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
        self.oob_mark = None;
        self.oob_byte = None;
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
            s.ts = None;
            out.push(s);
        }
        self.state = State::Closed;
    }

    fn synchronised(&self) -> bool {
        !matches!(self.state, State::Closed | State::Listen | State::SynSent)
    }

    /// Bytes we can take beyond RCV.NXT.
    fn rcv_wnd(&self) -> u32 {
        (self.rcvbuf_cap.saturating_sub(self.rcvbuf.len()) as u32).min(65535 << self.rcv_wscale)
    }

    /// Largest data payload of a segment (the MSS less the options sent
    /// with every segment, RFC 6691).
    fn smss(&self) -> u32 {
        (self.mss - if self.ts_ok { TS_LEN } else { 0 }).max(32)
    }

    fn ts_val(&self) -> u32 {
        (self.now as u32).wrapping_add(self.ts_offset)
    }

    fn segment(&self, seq: u32, flags: u8) -> Segment {
        let wnd = if flags & SYN != 0 { self.rcv_wnd().min(65535) } else { (self.rcv_wnd() >> self.rcv_wscale).min(65535) };
        let mut s = Segment {
            src_port: self.local.port,
            dst_port: self.remote.port,
            seq,
            ack: if flags & ACK != 0 { self.rcv_nxt } else { 0 },
            flags,
            window: wnd as u16,
            ..Default::default()
        };
        if self.ts_ok {
            s.ts = Some((self.ts_val(), self.ts_recent));
        }
        if let Some(up) = self.snd_up {
            if gt(up, seq) && flags & SYN == 0 {
                s.flags |= URG;
                s.urgent = up.wrapping_sub(seq).min(65535) as u16;
            }
        }
        s
    }

    /// Our SYN or SYN-ACK with its options.
    fn syn_segment(&mut self) -> Segment {
        let active = !self.passive;
        let flags = if self.state == State::SynSent { SYN } else { SYN | ACK };
        let want = shift_for(self.rcvbuf_cap);
        if self.ws_ok {
            self.rcv_wscale = want;
        }
        let mut s = self.segment(self.iss, flags);
        s.mss = Some(self.local_mss);
        if active || self.ws_ok {
            s.wscale = Some(want);
        }
        if active || self.ts_ok {
            s.ts = Some((self.ts_val(), if self.ts_ok { self.ts_recent } else { 0 }));
        }
        s.sack_ok = active || self.sack_ok;
        s
    }

    fn enter_time_wait(&mut self, now: u64) {
        self.state = State::TimeWait;
        self.linger_at = Some(now + TIME_WAIT);
        self.rtx_at = None;
        self.delack_at = None;
    }

    /// A SYN for this connection's endpoints while it is in TIME_WAIT may
    /// open a new connection (RFC 1122 4.2.2.13, RFC 6191): its sequence
    /// number is beyond what the old one received, or its timestamp is
    /// newer. The caller then ends this one and treats the SYN as new.
    pub fn new_syn_in_time_wait(&self, seg: &Segment) -> bool {
        if self.state != State::TimeWait || !seg.has(SYN) || seg.has(ACK) || seg.has(RST) {
            return false;
        }
        // (Either test will do: timestamp clocks need not run on across
        // connections, ours start at a per-connection offset.)
        gt(seg.seq, self.rcv_nxt) || (self.ts_ok && seg.ts.is_some_and(|(v, _)| gt(v, self.ts_recent)))
    }

    /// End a connection in TIME_WAIT at once (its endpoints are reused).
    pub fn end_time_wait(&mut self) {
        if self.state == State::TimeWait {
            self.state = State::Closed;
            self.linger_at = None;
        }
    }

    /// Process an arriving segment (RFC 9293 3.10.7).
    pub fn input(&mut self, seg: &Segment, now: u64) {
        self.now = now;
        self.last_rx = now;
        self.ka_probes = 0;
        self.stats.segs_received += 1;
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
        self.last_ack_sent = self.rcv_nxt;
        self.peer_syn(seg);
        if ack_ok {
            self.ack_advance(seg.ack, now, None);
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
        // PAWS (RFC 7323 5.3): a timestamp older than the last one is an
        // old duplicate.
        if let (true, Some((v, _))) = (self.ts_ok, seg.ts) {
            if !seg.has(RST) && lt(v, self.ts_recent) {
                self.ack_now = true;
                self.stats.duplicates += 1;
                return;
            }
        }
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
                    self.stats.duplicates += 1;
                    if self.state == State::TimeWait && fin {
                        self.linger_at = Some(now + TIME_WAIT);
                    }
                }
                return;
            }
        }
        // RFC 7323 4.3: remember the timestamp to echo.
        if let (true, Some((v, _))) = (self.ts_ok, seg.ts) {
            if le(seg.seq, self.last_ack_sent) {
                self.ts_recent = v;
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
        let room = wnd.saturating_sub(seq.wrapping_sub(self.rcv_nxt)) as usize;
        if data.len() > room {
            data = &data[..room];
            fin = false;
        }
        // 2. Reset: only exactly at RCV.NXT (RFC 5961 3.2).
        if seg.has(RST) {
            // TIME_WAIT ignores resets (RFC 1337): the peer's answer to a
            // duplicate ACK must not cut TIME_WAIT short.
            if self.state == State::TimeWait {
                return;
            }
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
                self.snd_wnd = (seg.window as u32) << self.snd_wscale;
                self.max_sndwnd = self.max_sndwnd.max(self.snd_wnd);
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
        if self.sack_ok && !seg.sack.is_empty() {
            self.note_sack(&seg.sack);
        }
        let window = (seg.window as u32) << self.snd_wscale;
        if gt(seg.ack, self.snd_una) {
            self.ack_advance(seg.ack, now, seg.ts.map(|t| t.1));
        } else if seg.ack == self.snd_una
            && seg.data.is_empty()
            && !fin
            && window == self.snd_wnd
            && self.snd_una != self.snd_max
        {
            self.dupacks += 1;
            if self.dupacks == 3 {
                // Fast retransmit: from SND.UNA, around what was SACKed.
                let flight = self.snd_max.wrapping_sub(self.snd_una);
                self.ssthresh = (flight / 2).max(2 * self.smss());
                self.cwnd = self.ssthresh + 3 * self.smss();
                self.snd_nxt = self.snd_una;
                self.rtt_sample = None;
                self.stats.retransmits += 1;
            } else if self.dupacks > 3 {
                // Each further duplicate means a segment left the network.
                self.cwnd += self.smss();
            }
        }
        // Window update (not from old segments).
        if lt(self.snd_wl1, seg.seq) || (self.snd_wl1 == seg.seq && le(self.snd_wl2, seg.ack)) {
            self.snd_wnd = window;
            self.max_sndwnd = self.max_sndwnd.max(window);
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
        let receiving = matches!(self.state, State::Established | State::FinWait1 | State::FinWait2);
        // 6. Urgent pointer (BSD: it points just past the urgent byte).
        if receiving && seg.has(URG) && seg.urgent > 0 {
            let up = seg.seq.wrapping_add(seg.urgent as u32);
            if gt(up, self.rcv_nxt) && self.rcv_up_seen.is_none_or(|u| gt(up, u)) {
                self.rcv_up_seen = Some(up);
                self.rcv_up = Some(up);
                self.oob_byte = None;
                self.stats.urgent_received += 1;
            }
        }
        // 7. Data.
        if !data.is_empty() && receiving {
            if seq == self.rcv_nxt {
                let n = self.deliver(data);
                if n < data.len() {
                    fin = false;
                }
                if self.ooo.is_empty() && !fin {
                    self.ack_soon(now);
                } else {
                    // A gap filled: the ACK says so at once.
                    let had = !self.ooo.is_empty();
                    self.pull_ooo();
                    if had {
                        self.ack_now = true;
                    } else {
                        self.ack_soon(now);
                    }
                }
            } else {
                self.add_ooo(seq, data);
                if fin {
                    self.ooo_fin = Some(seq.wrapping_add(data.len() as u32));
                }
                fin = false;
                self.ack_now = true;
            }
        } else if fin && receiving && gt(seq, self.rcv_nxt) {
            // A FIN beyond a gap.
            self.ooo_fin = Some(seq);
            fin = false;
            self.ack_now = true;
        }
        // 8. FIN, if everything before it is in.
        let fin_here = (fin && seq.wrapping_add(data.len() as u32) == self.rcv_nxt) || self.ooo_fin == Some(self.rcv_nxt);
        if fin_here && !self.fin_received {
            self.fin_received = true;
            self.ooo_fin = None;
            self.ooo.clear();
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

    /// In-order data at RCV.NXT into the receive buffer; an urgent byte in
    /// it is taken out (or marked, inline). Returns the bytes taken.
    fn deliver(&mut self, data: &[u8]) -> usize {
        let room = self.rcvbuf_cap.saturating_sub(self.rcvbuf.len());
        let n = data.len().min(room);
        let start = self.rcv_nxt;
        let mut urgent_at = None;
        if let Some(up) = self.rcv_up {
            let k = up.wrapping_sub(1).wrapping_sub(start) as usize;
            if k < n {
                urgent_at = Some(k);
            }
        }
        match urgent_at {
            Some(k) => {
                let before = self.rcvbuf.len() + k;
                if self.oobinline {
                    self.rcvbuf.extend(&data[..n]);
                } else {
                    self.rcvbuf.extend(&data[..k]);
                    self.oob_byte = Some(data[k]);
                    self.rcvbuf.extend(&data[k + 1..n]);
                }
                self.oob_mark = Some(before);
                self.rcv_up = None;
            }
            None => self.rcvbuf.extend(&data[..n]),
        }
        self.rcv_nxt = self.rcv_nxt.wrapping_add(n as u32);
        self.stats.bytes_received += n as u64;
        n
    }

    /// Keep a segment that arrived beyond RCV.NXT.
    fn add_ooo(&mut self, seq: u32, data: &[u8]) {
        self.stats.out_of_order += 1;
        if self.ooo.len() >= MAX_OOO {
            return;
        }
        self.last_ooo = Some(seq);
        self.ooo.push((seq, data.to_vec()));
        let base = self.rcv_nxt;
        self.ooo.sort_by_key(|(s, _)| s.wrapping_sub(base));
        let mut merged: Vec<(u32, Vec<u8>)> = Vec::with_capacity(self.ooo.len());
        for (s, d) in core::mem::take(&mut self.ooo) {
            if let Some((ps, pd)) = merged.last_mut() {
                let end = ps.wrapping_add(pd.len() as u32);
                if le(s, end) {
                    let skip = end.wrapping_sub(s) as usize;
                    if skip < d.len() {
                        pd.extend_from_slice(&d[skip..]);
                    }
                    if self.last_ooo == Some(s) {
                        self.last_ooo = Some(*ps);
                    }
                    continue;
                }
            }
            merged.push((s, d));
        }
        self.ooo = merged;
    }

    /// Join the kept segments that RCV.NXT has reached.
    fn pull_ooo(&mut self) {
        while let Some((s, _)) = self.ooo.first() {
            if gt(*s, self.rcv_nxt) {
                break;
            }
            let (s, d) = self.ooo.remove(0);
            let skip = self.rcv_nxt.wrapping_sub(s) as usize;
            if skip < d.len() {
                self.deliver(&d[skip..]);
            }
        }
        if self.ooo.is_empty() {
            self.last_ooo = None;
        }
    }

    /// SACK blocks for an ACK: the most recently changed block first.
    fn sack_blocks(&self) -> Vec<(u32, u32)> {
        let mut v: Vec<(u32, u32)> = self.ooo.iter().map(|(s, d)| (*s, s.wrapping_add(d.len() as u32))).collect();
        if let Some(i) = self.last_ooo.and_then(|l| v.iter().position(|b| b.0 == l)) {
            let b = v.remove(i);
            v.insert(0, b);
        }
        v.truncate(4);
        v
    }

    /// Record SACK blocks the peer sent (RFC 2018).
    fn note_sack(&mut self, blocks: &[(u32, u32)]) {
        for &(l, r) in blocks {
            if !lt(l, r) || !gt(r, self.snd_una) || gt(r, self.snd_max) {
                continue;
            }
            let l = if lt(l, self.snd_una) { self.snd_una } else { l };
            self.sacked.push((l, r));
            self.stats.sack_blocks += 1;
        }
        let base = self.snd_una;
        self.sacked.sort_by_key(|b| b.0.wrapping_sub(base));
        let mut merged: Vec<(u32, u32)> = Vec::new();
        for (l, r) in core::mem::take(&mut self.sacked) {
            if let Some(last) = merged.last_mut() {
                if le(l, last.1) {
                    if gt(r, last.1) {
                        last.1 = r;
                    }
                    continue;
                }
            }
            merged.push((l, r));
        }
        self.sacked = merged;
    }

    /// An ACK is due: at once when two full segments are unacknowledged,
    /// else within [`DELAYED_ACK`].
    fn ack_soon(&mut self, now: u64) {
        // (Also at once when the buffer is about full: the sender must
        // hear that the window closed.)
        if self.rcv_nxt.wrapping_sub(self.last_ack_sent) >= 2 * self.local_mss as u32 || self.rcv_wnd() < self.local_mss as u32 {
            self.ack_now = true;
        } else if self.delack_at.is_none() {
            self.delack_at = Some(now + DELAYED_ACK);
        }
    }

    /// The peer acknowledged everything below `ack`; `tsecr` is the
    /// timestamp it echoed.
    fn ack_advance(&mut self, ack: u32, now: u64, tsecr: Option<u32>) {
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
        if self.snd_up.is_some_and(|u| le(u, ack)) {
            self.snd_up = None;
        }
        self.sacked.retain_mut(|b| {
            if lt(b.0, ack) {
                b.0 = ack;
            }
            lt(b.0, b.1)
        });
        // RTT sample: the echoed timestamp, else the timed segment (Karn:
        // cleared on retransmission).
        match tsecr.filter(|&e| self.ts_ok && e != 0) {
            Some(e) => {
                let r = self.ts_val().wrapping_sub(e);
                if r < 0x8000_0000 {
                    self.rtt_update(r as u64);
                }
                self.rtt_sample = None;
            }
            None => {
                if let Some((s, t)) = self.rtt_sample {
                    if ge(ack, s) {
                        self.rtt_sample = None;
                        self.rtt_update(now.saturating_sub(t));
                    }
                }
            }
        }
        // Congestion window.
        let smss = self.smss();
        if self.dupacks >= 3 {
            self.cwnd = self.ssthresh;
        } else if self.cwnd < self.ssthresh {
            self.cwnd += acked.min(smss);
        } else {
            self.cwnd += (smss * smss / self.cwnd.max(1)).max(1);
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

    fn keepalive_due(&self) -> Option<u64> {
        let k = self.keepalive?;
        if !matches!(self.state, State::Established | State::CloseWait) {
            return None;
        }
        Some(self.last_rx + k.idle + self.ka_probes as u64 * k.interval)
    }

    /// Timers: retransmission/persist, delayed ACK, keepalive, TIME_WAIT
    /// and the orphan timeout.
    pub fn poll(&mut self, now: u64) {
        self.now = now;
        if let Some(t) = self.linger_at {
            if now >= t && matches!(self.state, State::TimeWait | State::FinWait2) {
                self.state = State::Closed;
                return;
            }
        }
        if self.delack_at.is_some_and(|t| now >= t) {
            self.delack_at = None;
            self.ack_now = true;
            self.stats.delayed_acks += 1;
        }
        if let (Some(t), Some(k)) = (self.keepalive_due(), self.keepalive) {
            if now >= t {
                if self.ka_probes >= k.count {
                    self.stats.keepalive_drops += 1;
                    self.error = Some(Error::TimedOut);
                    let mut out = Vec::new();
                    self.abort(&mut out);
                    self.pending_rst = out.pop();
                    return;
                }
                self.ka_probes += 1;
                self.ka_send = true;
                self.stats.keepalive_probes += 1;
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
        self.stats.timeouts += 1;
        let limit = if self.is_connecting() { SYN_RETRIES } else { DATA_RETRIES };
        if self.rtx_count > limit {
            self.error = Some(Error::TimedOut);
            let mut out = Vec::new();
            self.abort(&mut out);
            self.pending_rst = out.pop();
            return;
        }
        let flight = self.snd_max.wrapping_sub(self.snd_una);
        let smss = self.smss();
        self.ssthresh = (flight / 2).max(2 * smss);
        self.cwnd = smss;
        self.rto = (self.rto * 2).min(MAX_RTO);
        self.snd_nxt = self.snd_una;
        self.rtt_sample = None;
        self.dupacks = 0;
        // The peer may have dropped what it SACKed (RFC 2018 8).
        self.sacked.clear();
        // A probe into a still-closed window goes out again.
        self.probe = self.snd_wnd == 0;
    }

    /// The earliest time `poll` has work to do.
    pub fn next_timer(&self) -> Option<u64> {
        [self.rtx_at, self.linger_at, self.delack_at, self.keepalive_due()].into_iter().flatten().min()
    }

    /// Produce the segments that are due.
    pub fn output(&mut self, now: u64, out: &mut Vec<Segment>) {
        self.now = now;
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
                    let s = self.syn_segment();
                    out.push(s);
                    self.sent(self.iss.wrapping_add(1), now);
                }
            }
            _ => self.output_data(now, out),
        }
        if out.len() == start && self.ka_send && self.synchronised() && self.state != State::Closed {
            // Keepalive probe: an old sequence number the peer must ACK.
            out.push(self.segment(self.snd_una.wrapping_sub(1), ACK));
        }
        self.ka_send = false;
        if out.len() == start && self.ack_now && self.synchronised() && self.state != State::Closed {
            let mut s = self.segment(self.snd_nxt, ACK);
            if self.sack_ok && !self.ooo.is_empty() {
                s.sack = self.sack_blocks();
            }
            out.push(s);
        }
        if out.len() > start {
            self.stats.segs_sent += (out.len() - start) as u64;
            if self.synchronised() {
                self.last_ack_sent = self.rcv_nxt;
                self.delack_at = None;
            }
        }
        if out.len() > start || self.ack_now {
            self.rcv_adv = self.rcv_nxt.wrapping_add((self.rcv_wnd() >> self.rcv_wscale) << self.rcv_wscale);
        }
        self.ack_now = false;
    }

    fn output_data(&mut self, now: u64, out: &mut Vec<Segment>) {
        let smss = self.smss();
        loop {
            // Skip what the peer already has (SACK) when going back.
            if lt(self.snd_nxt, self.snd_max) {
                if let Some(&(_, r)) = self.sacked.iter().find(|b| le(b.0, self.snd_nxt) && lt(self.snd_nxt, b.1)) {
                    self.snd_nxt = if gt(r, self.snd_max) { self.snd_max } else { r };
                    continue;
                }
            }
            let off = self.snd_nxt.wrapping_sub(self.buf_seq) as usize;
            let unsent = self.sndbuf.len().saturating_sub(off);
            let edge = self.snd_una.wrapping_add(self.snd_wnd.min(self.cwnd.max(smss)));
            let usable = if lt(self.snd_nxt, edge) { edge.wrapping_sub(self.snd_nxt) as usize } else { 0 };
            if unsent > 0 && (usable > 0 || self.probe) {
                let mut n = if usable > 0 { unsent.min(usable).min(smss as usize) } else { 1 };
                let resend = lt(self.snd_nxt, self.snd_max);
                if resend {
                    // Stop at the next SACKed block.
                    if let Some(&(l, _)) = self.sacked.iter().find(|b| gt(b.0, self.snd_nxt)) {
                        n = n.min(l.wrapping_sub(self.snd_nxt) as usize).max(1);
                    }
                }
                // Nagle: a small segment of new data waits while data is in
                // flight, unless it is all there is to say and nothing is
                // outstanding, urgent data is queued or the peer's window
                // is small.
                if usable > 0
                    && !self.probe
                    && !resend
                    && n < smss as usize
                    && n == unsent
                    && !self.nodelay
                    && self.snd_una != self.snd_max
                    && self.snd_up.is_none()
                    && (n as u32) < self.max_sndwnd / 2
                {
                    break;
                }
                self.probe = false;
                let mut s = self.segment(self.snd_nxt, ACK);
                s.data = self.sndbuf.range(off..off + n).copied().collect();
                if n == unsent {
                    s.flags |= PSH;
                }
                if resend {
                    self.stats.retransmitted_bytes += n as u64;
                } else {
                    self.stats.bytes_sent += n as u64;
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
