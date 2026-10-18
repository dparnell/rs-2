//! Device monitors: DosMonOpen, DosMonReg, DosMonRead, DosMonWrite,
//! DosMonClose (MONCALLS) for the keyboard (KBD$) and mouse (MOUSE$).
//!
//! A monitor registers an input and an output buffer for one session's
//! device. Device data then flows as packets through the chain of
//! registered monitors (each reads with DosMonRead and passes on what it
//! wants with DosMonWrite) before reaching the session. The buffers in the
//! program only carry their length word; the packets are queued here.
//!
//! Keyboard packet (14 bytes): mnflags (the raw scan code in the high
//! byte), KBDKEYINFO, ddflags (`conkbd::dd`). Mouse packet (12 bytes):
//! mnflags, MOUEVENTINFO (fs, time, row, col).

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::kbd::KeyInfo;
use crate::sync::SpinLock;
use crate::task::WaitQueue;

pub const ERROR_MON_INVALID_PARMS: u32 = 379;
pub const ERROR_MON_INVALID_DEVNAME: u32 = 380;
pub const ERROR_MON_INVALID_HANDLE: u32 = 381;
pub const ERROR_MON_BUFFER_TOO_SMALL: u32 = 382;
pub const ERROR_MON_BUFFER_EMPTY: u32 = 383;
pub const ERROR_MON_DATA_TOO_LARGE: u32 = 384;

/// Packets a monitor may have outstanding before the oldest is dropped.
const QUEUE_MAX: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dev {
    Kbd,
    Mouse,
}

struct Reg {
    hmon: u16,
    pid: u32,
    inbuf: u32,
    outbuf: u32,
    input: VecDeque<Vec<u8>>,
}

struct Chain {
    dev: Dev,
    sid: u32,
    regs: Vec<Reg>,
}

struct State {
    /// Open monitor handles: handle, device, process.
    open: Vec<(u16, Dev, u32)>,
    chains: Vec<Chain>,
    next: u16,
}

static STATE: SpinLock<State> = SpinLock::new(State { open: Vec::new(), chains: Vec::new(), next: 1 });
static WAIT: WaitQueue = WaitQueue::new();

/// The keyboard packet for a key event.
pub fn key_packet(raw: u8, k: &KeyInfo, ddflags: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(14);
    v.extend_from_slice(&((raw as u16) << 8).to_le_bytes());
    v.extend_from_slice(&[k.ch, k.scan, k.status, 0]);
    v.extend_from_slice(&k.shift.to_le_bytes());
    v.extend_from_slice(&k.time.to_le_bytes());
    v.extend_from_slice(&ddflags.to_le_bytes());
    v
}

pub fn mouse_packet(e: &crate::mouse::Event) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&e.fs.to_le_bytes());
    v.extend_from_slice(&e.time.to_le_bytes());
    v.extend_from_slice(&e.row.to_le_bytes());
    v.extend_from_slice(&e.col.to_le_bytes());
    v
}

/// Device data for session `sid`: true if a monitor chain took it.
pub fn input(dev: Dev, sid: u32, packet: &[u8]) -> bool {
    let mut s = STATE.lock();
    let Some(ch) = s.chains.iter_mut().find(|c| c.dev == dev && c.sid == sid && !c.regs.is_empty()) else { return false };
    let q = &mut ch.regs[0].input;
    if q.len() >= QUEUE_MAX {
        q.pop_front();
    }
    q.push_back(packet.to_vec());
    drop(s);
    WAIT.wake_all();
    true
}

/// Pass a packet on from monitor `i` of a chain, or to the device's
/// session when it was the last one.
fn forward(s: &mut State, dev: Dev, sid: u32, i: usize, packet: Vec<u8>) -> Option<(Dev, u32, Vec<u8>)> {
    let ch = s.chains.iter_mut().find(|c| c.dev == dev && c.sid == sid)?;
    if i + 1 < ch.regs.len() {
        let q = &mut ch.regs[i + 1].input;
        if q.len() >= QUEUE_MAX {
            q.pop_front();
        }
        q.push_back(packet);
        None
    } else {
        Some((dev, sid, packet))
    }
}

/// A packet left the end of a chain.
fn deliver(dev: Dev, sid: u32, p: &[u8]) {
    match dev {
        Dev::Kbd if p.len() >= 14 => {
            let k = KeyInfo {
                ch: p[2],
                scan: p[3],
                status: p[4],
                shift: u16::from_le_bytes([p[6], p[7]]),
                time: u32::from_le_bytes([p[8], p[9], p[10], p[11]]),
            };
            crate::conkbd::deliver_key(sid, k, u16::from_le_bytes([p[12], p[13]]));
        }
        Dev::Mouse if p.len() >= 12 => {
            let e = crate::mouse::Event {
                fs: u16::from_le_bytes([p[2], p[3]]),
                time: u32::from_le_bytes([p[4], p[5], p[6], p[7]]),
                row: i16::from_le_bytes([p[8], p[9]]),
                col: i16::from_le_bytes([p[10], p[11]]),
            };
            crate::mouse::monitor_deliver(sid, e);
        }
        _ => {}
    }
}

fn device(name: &str) -> Option<Dev> {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name).to_ascii_uppercase();
    match base.as_str() {
        "KBD$" => Some(Dev::Kbd),
        "MOUSE$" => Some(Dev::Mouse),
        _ => None,
    }
}

/// DosMonOpen(pszDevName): a monitor handle.
pub fn open(name: &str, pid: u32) -> Result<u16, u32> {
    let dev = device(name).ok_or(ERROR_MON_INVALID_DEVNAME)?;
    let mut s = STATE.lock();
    let h = s.next;
    s.next = s.next.wrapping_add(1).max(1);
    s.open.push((h, dev, pid));
    Ok(h)
}

/// Remove registrations matching `pred`, passing their queued packets on.
fn unregister(pred: impl Fn(&Reg) -> bool) {
    let mut out = Vec::new();
    {
        let mut s = STATE.lock();
        for ch in s.chains.iter_mut() {
            let mut i = 0;
            while i < ch.regs.len() {
                if !pred(&ch.regs[i]) {
                    i += 1;
                    continue;
                }
                let r = ch.regs.remove(i);
                for p in r.input {
                    if i < ch.regs.len() {
                        ch.regs[i].input.push_back(p);
                    } else {
                        out.push((ch.dev, ch.sid, p));
                    }
                }
            }
        }
        s.chains.retain(|c| !c.regs.is_empty());
    }
    for (d, sid, p) in out {
        deliver(d, sid, &p);
    }
    WAIT.wake_all();
}

/// DosMonClose(hmon).
pub fn close(h: u16, pid: u32) -> Result<(), u32> {
    {
        let mut s = STATE.lock();
        let n = s.open.len();
        s.open.retain(|o| !(o.0 == h && o.2 == pid));
        if s.open.len() == n {
            return Err(ERROR_MON_INVALID_HANDLE);
        }
    }
    unregister(|r| r.hmon == h && r.pid == pid);
    Ok(())
}

/// DosMonReg(hmon, pbInBuf, pbOutBuf, fPosition, index): `pos` 0 = no
/// preference (after the others), 1 = first, 2 = last; `sid` the session.
pub fn register(h: u16, pid: u32, inbuf: u32, outbuf: u32, pos: u16, sid: u32) -> Result<(), u32> {
    if pos > 2 || inbuf == outbuf || !crate::session::exists(sid) {
        return Err(ERROR_MON_INVALID_PARMS);
    }
    let mut s = STATE.lock();
    let dev = s.open.iter().find(|o| o.0 == h && o.2 == pid).map(|o| o.1).ok_or(ERROR_MON_INVALID_HANDLE)?;
    let taken = s.chains.iter().any(|c| c.regs.iter().any(|r| r.pid == pid && (r.inbuf == inbuf || r.outbuf == outbuf)));
    if taken {
        return Err(ERROR_MON_INVALID_PARMS);
    }
    let reg = Reg { hmon: h, pid, inbuf, outbuf, input: VecDeque::new() };
    match s.chains.iter_mut().find(|c| c.dev == dev && c.sid == sid) {
        Some(ch) => {
            if pos == 1 {
                ch.regs.insert(0, reg);
            } else {
                ch.regs.push(reg);
            }
        }
        None => s.chains.push(Chain { dev, sid, regs: alloc::vec![reg] }),
    }
    Ok(())
}

/// DosMonRead(pbInBuf, fWait, pbDataBuf, pcbData): the next packet for
/// the monitor whose input buffer is `inbuf`; `max` is the data buffer
/// size.
pub fn read(pid: u32, inbuf: u32, wait: bool, max: usize) -> Result<Vec<u8>, u32> {
    let find = |s: &mut State| -> Result<Option<Vec<u8>>, u32> {
        for ch in s.chains.iter_mut() {
            if let Some(r) = ch.regs.iter_mut().find(|r| r.pid == pid && r.inbuf == inbuf) {
                if r.input.front().is_some_and(|p| p.len() > max) {
                    return Err(ERROR_MON_BUFFER_TOO_SMALL);
                }
                return Ok(r.input.pop_front());
            }
        }
        Err(ERROR_MON_INVALID_PARMS)
    };
    loop {
        if let Some(p) = find(&mut STATE.lock())? {
            return Ok(p);
        }
        if !wait {
            return Err(ERROR_MON_BUFFER_EMPTY);
        }
        // Ready when a packet waits or the registration went away.
        let ready = || {
            let s = STATE.lock();
            let r = s.chains.iter().find_map(|c| c.regs.iter().find(|r| r.pid == pid && r.inbuf == inbuf));
            r.is_none_or(|r| !r.input.is_empty())
        };
        if !WAIT.wait_until(None, ready) {
            return Err(crate::api::err::INTERRUPT);
        }
    }
}

/// DosMonWrite(pbOutBuf, pbDataBuf, cbData): pass a packet on from the
/// monitor whose output buffer is `outbuf`.
pub fn write(pid: u32, outbuf: u32, data: &[u8]) -> Result<(), u32> {
    if data.is_empty() || data.len() > 127 {
        return Err(ERROR_MON_DATA_TOO_LARGE);
    }
    let out = {
        let mut s = STATE.lock();
        let at = s.chains.iter().find_map(|c| c.regs.iter().position(|r| r.pid == pid && r.outbuf == outbuf).map(|i| (c.dev, c.sid, i)));
        let Some((dev, sid, i)) = at else { return Err(ERROR_MON_INVALID_PARMS) };
        forward(&mut s, dev, sid, i, data.to_vec())
    };
    if let Some((d, sid, p)) = out {
        deliver(d, sid, &p);
    }
    WAIT.wake_all();
    Ok(())
}

/// A process ended: its monitors leave their chains.
pub fn process_ended(pid: u32) {
    let had = STATE.lock().open.iter().any(|o| o.2 == pid);
    if !had {
        return;
    }
    STATE.lock().open.retain(|o| o.2 != pid);
    unregister(|r| r.pid == pid);
}
