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
fn out_of_order_is_dropped_and_retransmitted() {
    let (mut c, mut s) = connected(0);
    c.send(b"first").unwrap();
    let first = out(&mut c, 0);
    c.send(b"second").unwrap();
    let second = out(&mut c, 0);
    // The second segment arrives first: dropped, answered with an ACK
    // for what is missing.
    s.input(&second[0], 1);
    assert_eq!(s.available(), 0);
    let dup = out(&mut s, 1);
    assert_eq!(dup.len(), 1);
    assert_eq!(dup[0].ack, first[0].seq);
    s.input(&first[0], 2);
    assert_eq!(read_all(&mut s), b"first");
    c.input(&out(&mut s, 2)[0], 2);
    // The retransmission timer resends "second".
    let t = 2 + c.rto();
    c.poll(t);
    pump(&mut c, &mut s, t, &mut keep());
    assert_eq!(read_all(&mut s), b"second");
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
    let seg = Segment { src_port: 1, dst_port: 2, seq: 3, ack: 4, flags: SYN | ACK, window: 5, mss: Some(1460), data: vec![1, 2, 3] };
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
