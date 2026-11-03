//! Scripted-segment tests of the TCP state machine: two control blocks
//! exchanging segments directly, with losses and reordering on demand.

use super::*;
use alloc::vec;

fn ca() -> Endpoint {
    Endpoint::new(Ipv4::new(10, 0, 0, 1), 40000)
}
fn sa() -> Endpoint {
    Endpoint::new(Ipv4::new(10, 0, 0, 2), 80)
}

fn out(t: &mut Tcb, now: u64) -> Vec<Segment> {
    let mut v = Vec::new();
    t.output(now, &mut v);
    v
}

/// Deliver everything both sides have to say until they are quiet;
/// `drop(direction, segment)` loses segments (direction 0: client to
/// server). Returns the segments exchanged.
fn pump(c: &mut Tcb, s: &mut Tcb, now: u64, drop: &mut dyn FnMut(u8, &Segment) -> bool) -> usize {
    let mut n = 0;
    for _ in 0..1000 {
        let a = out(c, now);
        let b = out(s, now);
        if a.is_empty() && b.is_empty() {
            return n;
        }
        n += a.len() + b.len();
        for seg in a {
            if !drop(0, &seg) {
                s.input(&seg, now);
            }
        }
        for seg in b {
            if !drop(1, &seg) {
                c.input(&seg, now);
            }
        }
    }
    panic!("no quiescence");
}

fn keep() -> impl FnMut(u8, &Segment) -> bool {
    |_, _| false
}

/// A connected pair.
fn connected(now: u64) -> (Tcb, Tcb) {
    let mut c = Tcb::connect(ca(), sa(), 1000, 1460);
    let syn = out(&mut c, now);
    assert_eq!(syn.len(), 1);
    assert_eq!(syn[0].flags, SYN);
    assert_eq!(syn[0].mss, Some(1460));
    let mut s = Tcb::accept(sa(), ca(), &syn[0], 5000, 1460);
    pump(&mut c, &mut s, now, &mut keep());
    assert_eq!(c.state, State::Established);
    assert_eq!(s.state, State::Established);
    (c, s)
}

fn read_all(t: &mut Tcb) -> Vec<u8> {
    let mut v = vec![0u8; t.available()];
    let n = t.recv(&mut v);
    v.truncate(n);
    v
}

#[test]
fn handshake_data_and_close() {
    let (mut c, mut s) = connected(0);
    assert_eq!(c.send(b"GET / HTTP/1.0\r\n\r\n").unwrap(), 18);
    pump(&mut c, &mut s, 10, &mut keep());
    assert_eq!(read_all(&mut s), b"GET / HTTP/1.0\r\n\r\n");
    s.send(b"HTTP/1.0 200 OK\r\n").unwrap();
    s.close();
    assert_eq!(s.state, State::FinWait1);
    pump(&mut c, &mut s, 20, &mut keep());
    assert_eq!(read_all(&mut c), b"HTTP/1.0 200 OK\r\n");
    assert!(c.at_eof());
    assert_eq!(c.state, State::CloseWait);
    assert_eq!(s.state, State::FinWait2);
    c.close();
    pump(&mut c, &mut s, 30, &mut keep());
    assert_eq!(c.state, State::Closed);
    assert_eq!(s.state, State::TimeWait);
    s.poll(30 + TIME_WAIT);
    assert_eq!(s.state, State::Closed);
    assert!(c.error.is_none() && s.error.is_none());
}

#[test]
fn bulk_transfer_with_losses() {
    let (mut c, mut s) = connected(0);
    let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 + i / 251) as u8).collect();
    let mut sent = 0;
    let mut got = Vec::new();
    let mut now = 0;
    let mut count = 0u32;
    // Lose every 13th segment from the client.
    let mut lossy = |dir: u8, _: &Segment| {
        count += 1;
        dir == 0 && count % 13 == 0
    };
    while got.len() < data.len() {
        now += 10;
        if sent < data.len() {
            sent += c.send(&data[sent..]).unwrap();
        }
        pump(&mut c, &mut s, now, &mut lossy);
        got.extend(read_all(&mut s));
        c.poll(now);
        s.poll(now);
        assert!(now < 10_000_000, "stalled at {} of {}", got.len(), data.len());
    }
    assert!(got == data);
}

#[test]
fn out_of_order_is_kept_and_joined() {
    let (mut c, mut s) = connected(0);
    c.nodelay = true;
    c.send(b"first").unwrap();
    let first = out(&mut c, 0);
    c.send(b"second").unwrap();
    let second = out(&mut c, 0);
    c.send(b"third").unwrap();
    let third = out(&mut c, 0);
    // The third and second segments arrive before the first: kept, and
    // answered at once with duplicate ACKs naming the gap and SACK blocks.
    s.input(&third[0], 1);
    let dup = out(&mut s, 1);
    assert_eq!(dup.len(), 1);
    assert_eq!(dup[0].ack, first[0].seq);
    assert_eq!(dup[0].sack, vec![(third[0].seq, third[0].seq + 5)]);
    s.input(&second[0], 1);
    let dup = out(&mut s, 1);
    assert_eq!(dup[0].sack, vec![(second[0].seq, third[0].seq + 5)]);
    assert_eq!(s.available(), 0);
    // The gap is filled: everything is readable and ACKed at once.
    s.input(&first[0], 2);
    assert_eq!(read_all(&mut s), b"firstsecondthird");
    let ack = out(&mut s, 2);
    assert_eq!(ack.len(), 1);
    assert_eq!(ack[0].ack, third[0].seq + 5);
    assert!(ack[0].sack.is_empty());
    assert_eq!(s.stats.out_of_order, 2);
}

#[test]
fn out_of_order_fin_and_overlaps() {
    let (mut c, mut s) = connected(0);
    c.nodelay = true;
    c.send(b"abcdef").unwrap();
    let a = out(&mut c, 0);
    c.send(b"ghij").unwrap();
    c.close();
    let b = out(&mut c, 0);
    // "ghij" + FIN first, then an overlapping piece, then the start.
    assert!(b.last().unwrap().has(FIN));
    for seg in &b {
        s.input(seg, 1);
    }
    let mut part = a[0].clone();
    part.seq += 3;
    part.data = b"defg".to_vec();
    s.input(&part, 1);
    assert_eq!(s.available(), 0);
    assert_eq!(s.state, State::Established);
    s.input(&a[0], 2);
    assert_eq!(read_all(&mut s), b"abcdefghij");
    assert!(s.at_eof());
    assert_eq!(s.state, State::CloseWait);
}

#[test]
fn sack_avoids_resending_what_arrived() {
    let (mut c, mut s) = connected(0);
    let data: Vec<u8> = (0..20_000u32).map(|i| i as u8).collect();
    c.send(&data).unwrap();
    let segs = out(&mut c, 1);
    assert!(segs.len() >= 3);
    // The first segment is lost; the rest arrive and are SACKed.
    let mut acks = Vec::new();
    for seg in &segs[1..] {
        s.input(seg, 2);
        acks.extend(out(&mut s, 2));
    }
    assert!(acks.len() >= 3);
    for a in &acks {
        c.input(a, 3);
    }
    // Three duplicate ACKs: fast retransmit of the hole only.
    let again = out(&mut c, 3);
    assert_eq!(again[0].seq, segs[0].seq);
    let resent: usize = again.iter().filter(|x| lt(x.seq, segs.last().unwrap().seq)).map(|x| x.data.len()).sum();
    assert_eq!(resent, segs[0].data.len());
    assert_eq!(c.stats.retransmitted_bytes, segs[0].data.len() as u64);
    for x in &again {
        s.input(x, 4);
    }
    let mut got = read_all(&mut s);
    pump(&mut c, &mut s, 5, &mut keep());
    for i in 0..50 {
        c.poll(200 + i * 100);
        s.poll(200 + i * 100);
        pump(&mut c, &mut s, 200 + i * 100, &mut keep());
        got.extend(read_all(&mut s));
    }
    assert_eq!(got, data);
}

#[test]
fn options_negotiated() {
    let (c, s) = connected(0);
    assert_eq!(c.options(), (Some((0, 0)), true, true));
    assert_eq!(s.options(), (Some((0, 0)), true, true));
    // A large receive buffer: window scaling announces more than 64 KB.
    let mut c = Tcb::connect(ca(), sa(), 7, 1460);
    c.set_buffers(SEND_BUFFER, 500 * 1024);
    let syn = out(&mut c, 0);
    assert_eq!(syn[0].wscale, Some(3));
    assert!(syn[0].sack_ok && syn[0].ts.is_some());
    let mut s = Tcb::accept(sa(), ca(), &syn[0], 9, 1460);
    s.set_buffers(SEND_BUFFER, 200 * 1024);
    let synack = out(&mut s, 0);
    assert_eq!(synack[0].wscale, Some(2));
    c.input(&synack[0], 1);
    assert_eq!(c.options(), (Some((3, 2)), true, true));
    let ack = out(&mut c, 1);
    assert_eq!(ack[0].window as u32, (500 * 1024) >> 3);
    s.input(&ack[0], 1);
    assert_eq!(s.windows().0, 500 * 1024);
    // A peer without options gets none.
    let mut plain = syn[0].clone();
    plain.wscale = None;
    plain.ts = None;
    plain.sack_ok = false;
    let mut s = Tcb::accept(sa(), ca(), &plain, 9, 1460);
    let synack = out(&mut s, 0);
    assert_eq!((synack[0].wscale, synack[0].ts, synack[0].sack_ok), (None, None, false));
    assert_eq!(s.options(), (None, false, false));
}

#[test]
fn large_window_bulk_transfer() {
    let mut c = Tcb::connect(ca(), sa(), 100, 1460);
    c.set_buffers(1 << 20, 1 << 20);
    let syn = out(&mut c, 0);
    let mut s = Tcb::accept(sa(), ca(), &syn[0], 900, 1460);
    s.set_buffers(1 << 20, 1 << 20);
    pump(&mut c, &mut s, 0, &mut keep());
    let data: Vec<u8> = (0..2_000_000u32).map(|i| (i % 253) as u8).collect();
    let (mut sent, mut got, mut now) = (0, Vec::new(), 0);
    let mut count = 0u32;
    let mut lossy = |dir: u8, _: &Segment| {
        count += 1;
        dir == 0 && count % 97 == 0
    };
    while got.len() < data.len() {
        now += 10;
        if sent < data.len() {
            sent += c.send(&data[sent..]).unwrap();
        }
        pump(&mut c, &mut s, now, &mut lossy);
        got.extend(read_all(&mut s));
        c.poll(now);
        s.poll(now);
        assert!(now < 1_000_000, "stalled at {}", got.len());
    }
    assert!(got == data);
    assert!(c.windows().0 > 65535);
}

#[test]
fn paws_drops_old_duplicates() {
    let (mut c, mut s) = connected(0);
    c.send(b"one").unwrap();
    let old = out(&mut c, 0);
    s.input(&old[0], 1);
    c.nodelay = true;
    s.poll(1000);
    pump(&mut c, &mut s, 1000, &mut keep());
    c.send(b"two").unwrap();
    pump(&mut c, &mut s, 3000, &mut keep());
    assert_eq!(read_all(&mut s), b"onetwo");
    // A copy of the first segment with its old timestamp, made to look
    // new: dropped.
    let mut replay = old[0].clone();
    replay.seq = s.rcv_nxt;
    s.input(&replay, 3001);
    assert_eq!(s.available(), 0);
}

#[test]
fn delayed_ack_and_nagle() {
    let (mut c, mut s) = connected(0);
    // One small segment: the ACK waits.
    c.send(b"a").unwrap();
    let a = out(&mut c, 10);
    assert_eq!(a.len(), 1);
    s.input(&a[0], 10);
    assert!(out(&mut s, 10).is_empty());
    assert_eq!(s.next_timer(), Some(10 + DELAYED_ACK));
    // Nagle: more small data waits for that ACK.
    c.send(b"b").unwrap();
    c.send(b"c").unwrap();
    assert!(out(&mut c, 20).is_empty());
    s.poll(10 + DELAYED_ACK);
    let ack = out(&mut s, 10 + DELAYED_ACK);
    assert_eq!(ack.len(), 1);
    c.input(&ack[0], 10 + DELAYED_ACK);
    let bc = out(&mut c, 10 + DELAYED_ACK);
    assert_eq!(bc.len(), 1);
    assert_eq!(bc[0].data, b"bc");
    // With TCP_NODELAY small segments go at once.
    c.nodelay = true;
    s.input(&bc[0], 120);
    c.send(b"d").unwrap();
    assert_eq!(out(&mut c, 120).len(), 1);
    // Two full segments are ACKed at once.
    let big = [1u8; 2896];
    c.send(&big).unwrap();
    let segs = out(&mut c, 130);
    assert_eq!(segs.len(), 2);
    for x in &segs {
        s.input(x, 130);
    }
    assert_eq!(out(&mut s, 130).len(), 1);
}

#[test]
fn keepalive_probes_and_timeout() {
    let (mut c, mut s) = connected(0);
    let k = Keepalive { idle: 10_000, interval: 1000, count: 3 };
    c.set_keepalive(Some(k));
    // A peer that answers keeps the connection.
    let t = c.next_timer().unwrap();
    assert_eq!(t, 10_000);
    c.poll(t);
    let probe = out(&mut c, t);
    assert_eq!(probe.len(), 1);
    assert_eq!(probe[0].seq, c.snd_una.wrapping_sub(1));
    assert!(probe[0].data.is_empty());
    s.input(&probe[0], t);
    let answer = out(&mut s, t);
    assert_eq!(answer.len(), 1);
    c.input(&answer[0], t);
    assert_eq!(c.next_timer(), Some(t + 10_000));
    // A silent one does not.
    let mut now = t;
    let mut probes = 0;
    while !c.is_closed() {
        now = c.next_timer().unwrap();
        c.poll(now);
        probes += out(&mut c, now).iter().filter(|x| !x.has(RST)).count();
    }
    assert_eq!(probes, 3);
    assert_eq!(now, t + 10_000 + 3 * 1000);
    assert_eq!(c.error, Some(Error::TimedOut));
}

#[test]
fn urgent_data_out_of_line_and_inline() {
    for inline in [false, true] {
        let (mut c, mut s) = connected(0);
        s.oobinline = inline;
        c.nodelay = true;
        c.send(b"abc").unwrap();
        c.send_urgent(b"!").unwrap();
        c.send(b"def").unwrap();
        let segs = out(&mut c, 1);
        assert!(segs[0].has(URG));
        assert_eq!(segs[0].urgent, 4);
        for x in &segs {
            s.input(x, 1);
        }
        assert!(s.urgent_pending());
        let mut buf = [0u8; 64];
        // Reads stop at the mark.
        let n = s.recv(&mut buf);
        assert_eq!(&buf[..n], b"abc");
        assert!(s.at_mark());
        if inline {
            assert_eq!(s.recv_urgent(false), Err(Error::InvalidArg));
            let n = s.recv(&mut buf);
            assert_eq!(&buf[..n], b"!def");
        } else {
            assert_eq!(s.recv_urgent(true), Ok(b'!'));
            assert_eq!(s.recv_urgent(false), Ok(b'!'));
            assert_eq!(s.recv_urgent(false), Err(Error::InvalidArg));
            let n = s.recv(&mut buf);
            assert_eq!(&buf[..n], b"def");
        }
        assert!(!s.at_mark());
        assert!(!s.urgent_pending());
        // The ACK ends urgent mode at the sender.
        s.poll(500);
        pump(&mut c, &mut s, 500, &mut keep());
        c.send(b"x").unwrap();
        let more = out(&mut c, 501);
        assert!(!more[0].has(URG));
    }
}

#[test]
fn option_codec() {
    let src = Ipv4::new(1, 2, 3, 4);
    let dst = Ipv4::new(5, 6, 7, 8);
    let syn = Segment { flags: SYN, mss: Some(1460), wscale: Some(7), sack_ok: true, ts: Some((1, 0)), ..Default::default() };
    assert_eq!(parse(src, dst, &build(src, dst, &syn)), Some(syn));
    let ack = Segment {
        flags: ACK | URG,
        urgent: 9,
        ts: Some((5, 6)),
        sack: vec![(10, 20), (30, 40), (50, 60)],
        data: vec![1],
        ..Default::default()
    };
    let b = build(src, dst, &ack);
    assert_eq!((b[12] >> 4) as usize * 4, 20 + 40);
    assert_eq!(parse(src, dst, &b), Some(ack));
}

#[test]
fn refused_connection() {
    let mut c = Tcb::connect(ca(), sa(), 1, 1460);
    let syn = out(&mut c, 0);
    let rst = reset_for(&syn[0]).unwrap();
    assert_eq!(rst.flags, RST | ACK);
    assert_eq!(rst.ack, 2);
    c.input(&rst, 1);
    assert_eq!(c.state, State::Closed);
    assert_eq!(c.error, Some(Error::ConnRefused));
}

#[test]
fn syn_retries_then_times_out() {
    let mut c = Tcb::connect(ca(), sa(), 1, 1460);
    let mut now = 0;
    let mut syns = out(&mut c, now).len();
    while !c.is_closed() {
        now = c.next_timer().unwrap();
        c.poll(now);
        syns += out(&mut c, now).iter().filter(|s| s.has(SYN)).count();
    }
    assert_eq!(syns as u32, 1 + SYN_RETRIES);
    assert_eq!(c.error, Some(Error::TimedOut));
    // 1 + 2 + 4 + 8 + 16 + 32 seconds of back-off.
    assert_eq!(now, 63_000);
}

#[test]
fn zero_window_and_persist() {
    let mut c = Tcb::connect(ca(), sa(), 100, 1460);
    let syn = out(&mut c, 0);
    let mut s = Tcb::accept(sa(), ca(), &syn[0], 900, 1460);
    s.set_buffers(SEND_BUFFER, 1000);
    pump(&mut c, &mut s, 0, &mut keep());
    let data = [0x5au8; 3000];
    assert_eq!(c.send(&data).unwrap(), 3000);
    pump(&mut c, &mut s, 1, &mut keep());
    assert_eq!(s.available(), 1000);
    // The window is closed: after the persist timer a 1-byte probe goes
    // out and is refused.
    let t = c.next_timer().unwrap();
    c.poll(t);
    let probe = out(&mut c, t);
    assert_eq!(probe.len(), 1);
    assert_eq!(probe[0].data.len(), 1);
    s.input(&probe[0], t);
    assert_eq!(s.available(), 1000);
    pump(&mut c, &mut s, t, &mut keep());
    // Reading opens the window: the update lets the rest flow.
    let mut got = read_all(&mut s);
    for i in 0..20 {
        pump(&mut c, &mut s, t + 1 + i, &mut keep());
        got.extend(read_all(&mut s));
        if let Some(x) = c.next_timer() {
            c.poll(x);
        }
    }
    assert_eq!(got.len(), 3000);
}

#[test]
fn simultaneous_close() {
    let (mut c, mut s) = connected(0);
    c.close();
    s.close();
    let a = out(&mut c, 1);
    let b = out(&mut s, 1);
    s.input(&a[0], 1);
    c.input(&b[0], 1);
    assert_eq!(c.state, State::Closing);
    assert_eq!(s.state, State::Closing);
    pump(&mut c, &mut s, 2, &mut keep());
    assert_eq!(c.state, State::TimeWait);
    assert_eq!(s.state, State::TimeWait);
}

#[test]
fn blind_reset_is_challenged() {
    let (mut c, mut s) = connected(0);
    let mut rst = out(&mut s, 0).pop().unwrap_or_else(|| s.segment(s.snd_nxt, ACK));
    rst.flags = RST;
    rst.seq = rst.seq.wrapping_add(10);
    c.input(&rst, 1);
    assert_eq!(c.state, State::Established);
    let ch = out(&mut c, 1);
    assert_eq!(ch.len(), 1);
    assert_eq!(ch[0].flags, ACK);
    // An exact one resets.
    rst.seq = rst.seq.wrapping_sub(10);
    c.input(&rst, 2);
    assert_eq!(c.state, State::Closed);
    assert_eq!(c.error, Some(Error::ConnReset));
}

#[test]
fn abort_sends_reset() {
    let (mut c, mut s) = connected(0);
    let mut v = Vec::new();
    c.abort(&mut v);
    assert_eq!(v.len(), 1);
    assert!(v[0].has(RST));
    s.input(&v[0], 1);
    assert_eq!(s.error, Some(Error::ConnReset));
}

#[test]
fn segment_codec() {
    let src = Ipv4::new(1, 2, 3, 4);
    let dst = Ipv4::new(5, 6, 7, 8);
    let seg = Segment { src_port: 1, dst_port: 2, seq: 3, ack: 4, flags: SYN | ACK, window: 5, mss: Some(1460), data: vec![1, 2, 3], ..Default::default() };
    let b = build(src, dst, &seg);
    assert_eq!(parse(src, dst, &b), Some(seg));
    assert!(parse(src, Ipv4::new(5, 6, 7, 9), &b).is_none());
}

#[test]
fn sequence_wraparound() {
    let mut c = Tcb::connect(ca(), sa(), u32::MAX - 2, 1460);
    let syn = out(&mut c, 0);
    let mut s = Tcb::accept(sa(), ca(), &syn[0], u32::MAX - 100, 1460);
    pump(&mut c, &mut s, 0, &mut keep());
    let data = [7u8; 5000];
    c.send(&data).unwrap();
    pump(&mut c, &mut s, 1, &mut keep());
    assert_eq!(read_all(&mut s).len(), 5000);
}
