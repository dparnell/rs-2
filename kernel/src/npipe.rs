//! Named pipes (`\PIPE\name`).
//!
//! A server creates instances with DosCreateNPipe; each instance connects
//! one client (DosOpen of the pipe name) at a time. An instance carries two
//! one-way channels, server-to-client and client-to-server, each a byte
//! queue plus message boundaries for message pipes. Disconnecting bumps the
//! instance's generation so a client handle from an earlier connection no
//! longer reaches the new one.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::api::err;
use crate::sync::SpinLock;
use crate::task::WaitQueue;

pub const ERROR_BAD_PIPE: u32 = 230;
pub const ERROR_PIPE_BUSY: u32 = 231;
pub const ERROR_NO_DATA: u32 = 232;
pub const ERROR_PIPE_NOT_CONNECTED: u32 = 233;
pub const ERROR_MORE_DATA: u32 = 234;

// Open mode (DosCreateNPipe).
pub const NP_ACCESS_INBOUND: u32 = 0;
pub const NP_ACCESS_OUTBOUND: u32 = 1;
pub const NP_ACCESS_DUPLEX: u32 = 2;
pub const NP_NOINHERIT: u32 = 0x0080;

// Pipe mode / handle state.
pub const NP_UNLIMITED_INSTANCES: u32 = 0xff;
pub const NP_READMODE_MESSAGE: u32 = 0x0100;
pub const NP_TYPE_MESSAGE: u32 = 0x0400;
pub const NP_END_SERVER: u32 = 0x4000;
pub const NP_NOWAIT: u32 = 0x8000;

// Pipe states (DosPeekNPipe).
pub const NP_STATE_DISCONNECTED: u32 = 1;
pub const NP_STATE_LISTENING: u32 = 2;
pub const NP_STATE_CONNECTED: u32 = 3;
pub const NP_STATE_CLOSING: u32 = 4;

/// One direction of an instance.
#[derive(Default)]
struct Chan {
    data: VecDeque<u8>,
    /// Lengths of the queued messages (message pipes only).
    msgs: VecDeque<usize>,
}

impl Chan {
    fn clear(&mut self) {
        self.data.clear();
        self.msgs.clear();
    }

    /// Remove `n` bytes from the front, keeping message boundaries.
    fn take(&mut self, n: usize) -> Vec<u8> {
        let v: Vec<u8> = self.data.drain(..n).collect();
        let mut left = n;
        while left > 0 {
            let Some(m) = self.msgs.front_mut() else { break };
            if *m > left {
                *m -= left;
                break;
            }
            left -= *m;
            self.msgs.pop_front();
        }
        v
    }
}

struct Inner {
    state: u32,
    generation: u32,
    to_client: Chan,
    to_server: Chan,
    /// Event semaphores attached with DosSetNPipeSem: (handle, key) for the
    /// server and the client end.
    sem: [Option<(u32, u16)>; 2],
}

pub struct Instance {
    pub name: String,
    open_mode: u32,
    pipe_mode: u32,
    pub out_size: usize,
    pub in_size: usize,
    inner: SpinLock<Inner>,
    /// Woken on every state or data change of this instance.
    cond: WaitQueue,
}

/// All instances by upper-case name.
static PIPES: SpinLock<Vec<Arc<Instance>>> = SpinLock::new(Vec::new());
/// Woken when an instance starts listening (DosWaitNPipe).
static LISTEN: WaitQueue = WaitQueue::new();
/// Default DosWaitNPipe timeouts by name.
static DEFAULT_WAIT: SpinLock<Vec<(String, u32)>> = SpinLock::new(Vec::new());

/// A handle to one end of an instance.
pub struct End {
    pub inst: Arc<Instance>,
    pub server: bool,
    generation: u32,
    /// Handle state bits that the end may change: NP_NOWAIT and
    /// NP_READMODE_MESSAGE.
    mode: AtomicU32,
}

/// Whether `path` names a pipe (`\PIPE\...`), returning the canonical name.
pub fn pipe_name(path: &str) -> Option<String> {
    let p = path.replace('/', "\\").to_ascii_uppercase();
    let p = p.strip_prefix("\\\\.").unwrap_or(&p);
    let rest = p.strip_prefix("\\PIPE\\")?;
    if rest.is_empty() {
        return None;
    }
    Some(alloc::format!("\\PIPE\\{rest}"))
}

fn instances(name: &str) -> Vec<Arc<Instance>> {
    PIPES.lock().iter().filter(|i| i.name == name).cloned().collect()
}

/// DosCreateNPipe: a new instance, listening for a client.
pub fn create(name: String, open_mode: u32, pipe_mode: u32, out_size: u32, in_size: u32, wait_ms: u32) -> Result<Arc<End>, u32> {
    if open_mode & 3 > NP_ACCESS_DUPLEX {
        return Err(err::INVALID_PARAMETER);
    }
    let max = pipe_mode & 0xff;
    if max == 0 {
        return Err(err::INVALID_PARAMETER);
    }
    // Message read mode needs a message pipe.
    if pipe_mode & NP_READMODE_MESSAGE != 0 && pipe_mode & NP_TYPE_MESSAGE == 0 {
        return Err(err::INVALID_PARAMETER);
    }
    let mut all = PIPES.lock();
    let existing: Vec<&Arc<Instance>> = all.iter().filter(|i| i.name == name).collect();
    if let Some(first) = existing.first() {
        // Further instances must agree with the first one.
        if first.pipe_mode & 0xff != max || first.pipe_mode & NP_TYPE_MESSAGE != pipe_mode & NP_TYPE_MESSAGE {
            return Err(err::ACCESS_DENIED);
        }
        if max != NP_UNLIMITED_INSTANCES && existing.len() as u32 >= max {
            return Err(ERROR_PIPE_BUSY);
        }
    } else {
        let mut d = DEFAULT_WAIT.lock();
        d.retain(|(n, _)| *n != name);
        d.push((name.clone(), if wait_ms == 0 { 50 } else { wait_ms }));
    }
    let inst = Arc::new(Instance {
        name,
        open_mode,
        pipe_mode,
        out_size: (out_size as usize).max(1),
        in_size: (in_size as usize).max(1),
        inner: SpinLock::new(Inner {
            state: NP_STATE_LISTENING,
            generation: 0,
            to_client: Chan::default(),
            to_server: Chan::default(),
            sem: [None, None],
        }),
        cond: WaitQueue::new(),
    });
    all.push(inst.clone());
    drop(all);
    LISTEN.wake_all();
    let mode = pipe_mode & (NP_NOWAIT | NP_READMODE_MESSAGE);
    Ok(Arc::new(End { inst, server: true, generation: 0, mode: AtomicU32::new(mode) }))
}

/// DosOpen of a pipe name: connect to a listening instance.
pub fn open(name: &str) -> Result<Arc<End>, u32> {
    let insts = instances(name);
    if insts.is_empty() {
        return Err(err::FILE_NOT_FOUND);
    }
    for inst in insts {
        let mut g = inst.inner.lock();
        if g.state != NP_STATE_LISTENING {
            continue;
        }
        g.state = NP_STATE_CONNECTED;
        g.generation = g.generation.wrapping_add(1);
        let generation = g.generation;
        drop(g);
        inst.notify();
        // A client starts in byte read mode, blocking.
        return Ok(Arc::new(End { inst, server: false, generation, mode: AtomicU32::new(0) }));
    }
    Err(ERROR_PIPE_BUSY)
}

/// DosWaitNPipe: wait until an instance of `name` is listening.
pub fn wait(name: &str, ms: u32) -> u32 {
    let ms = match ms {
        0 => DEFAULT_WAIT.lock().iter().find(|(n, _)| n == name).map_or(50, |&(_, w)| w),
        x => x,
    };
    let deadline = if ms == u32::MAX { None } else { Some(crate::irq::ticks() + ms as u64 * crate::irq::HZ / 1000) };
    let listening = || {
        let insts = instances(name);
        insts.is_empty() || insts.iter().any(|i| i.inner.lock().state == NP_STATE_LISTENING)
    };
    if LISTEN.wait_until(deadline, listening) {
        if instances(name).is_empty() { err::FILE_NOT_FOUND } else { 0 }
    } else if crate::task::sched::current().exit_requested() {
        err::INTERRUPT
    } else {
        err::SEM_TIMEOUT
    }
}

impl Instance {
    /// Wake waiters and post attached semaphores.
    fn notify(&self) {
        let sems = self.inner.lock().sem;
        self.cond.wake_all();
        for (h, _) in sems.into_iter().flatten() {
            crate::api::sem::post_event_handle(h);
        }
    }

    pub fn max_instances(&self) -> u32 {
        self.pipe_mode & 0xff
    }

    pub fn current_instances(&self) -> u32 {
        instances(&self.name).len() as u32
    }

    fn is_message(&self) -> bool {
        self.pipe_mode & NP_TYPE_MESSAGE != 0
    }
}

impl End {
    fn connected(&self, g: &Inner) -> bool {
        g.state == NP_STATE_CONNECTED && (self.server || g.generation == self.generation)
    }

    /// The client of an earlier connection.
    fn stale(&self, g: &Inner) -> bool {
        !self.server && g.generation != self.generation
    }

    fn nowait(&self) -> bool {
        self.mode.load(Ordering::Relaxed) & NP_NOWAIT != 0
    }

    fn can_read(&self) -> bool {
        let a = self.inst.open_mode & 3;
        a == NP_ACCESS_DUPLEX || (a == NP_ACCESS_INBOUND) == self.server
    }

    fn can_write(&self) -> bool {
        let a = self.inst.open_mode & 3;
        a == NP_ACCESS_DUPLEX || (a == NP_ACCESS_OUTBOUND) == self.server
    }

    fn rx<'a>(&self, g: &'a mut Inner) -> &'a mut Chan {
        if self.server { &mut g.to_server } else { &mut g.to_client }
    }

    fn tx<'a>(&self, g: &'a mut Inner) -> &'a mut Chan {
        if self.server { &mut g.to_client } else { &mut g.to_server }
    }

    fn tx_cap(&self) -> usize {
        if self.server { self.inst.out_size } else { self.inst.in_size }
    }

    /// Handle state for DosQueryNPHState.
    pub fn state_bits(&self) -> u32 {
        let fixed = self.inst.pipe_mode & (NP_TYPE_MESSAGE | 0xff);
        fixed | self.mode.load(Ordering::Relaxed) | if self.server { NP_END_SERVER } else { 0 }
    }

    /// DosSetNPHState: blocking and read mode.
    pub fn set_state(&self, s: u32) -> Result<(), u32> {
        if s & !(NP_NOWAIT | NP_READMODE_MESSAGE) != 0 {
            return Err(err::INVALID_PARAMETER);
        }
        if s & NP_READMODE_MESSAGE != 0 && !self.inst.is_message() {
            return Err(err::INVALID_PARAMETER);
        }
        self.mode.store(s, Ordering::Relaxed);
        Ok(())
    }

    pub fn open_mode(&self) -> u32 {
        self.inst.open_mode
    }

    /// Read. Message read mode returns one message (ERROR_MORE_DATA when it
    /// does not fit); an empty pipe whose peer has gone reads as end of file.
    pub fn read(&self, n: usize) -> Result<(Vec<u8>, u32), u32> {
        if !self.can_read() {
            return Err(err::ACCESS_DENIED);
        }
        let msg_mode = self.mode.load(Ordering::Relaxed) & NP_READMODE_MESSAGE != 0;
        loop {
            let mut g = self.inst.inner.lock();
            if self.stale(&g) {
                return Err(ERROR_PIPE_NOT_CONNECTED);
            }
            let has = {
                let rx = self.rx(&mut g);
                !rx.data.is_empty() || !rx.msgs.is_empty()
            };
            if has {
                let rx = self.rx(&mut g);
                let r = if msg_mode {
                    let m = rx.msgs.front().copied().unwrap_or(rx.data.len());
                    if n < m {
                        (rx.take(n), ERROR_MORE_DATA)
                    } else {
                        let v = rx.take(m);
                        if m == 0 {
                            rx.msgs.pop_front();
                        }
                        (v, 0)
                    }
                } else {
                    let k = n.min(rx.data.len());
                    (rx.take(k), 0)
                };
                drop(g);
                self.inst.notify();
                return Ok(r);
            }
            match g.state {
                NP_STATE_CONNECTED => {}
                NP_STATE_CLOSING => return Ok((Vec::new(), 0)),
                _ => return Err(ERROR_PIPE_NOT_CONNECTED),
            }
            drop(g);
            if self.nowait() {
                return Err(ERROR_NO_DATA);
            }
            let ready = || {
                let mut g = self.inst.inner.lock();
                let rx = self.rx(&mut g);
                !rx.data.is_empty() || !rx.msgs.is_empty() || g.state != NP_STATE_CONNECTED || self.stale(&g)
            };
            if !self.inst.cond.wait_until(None, ready) {
                return Err(err::INTERRUPT);
            }
        }
    }

    /// Write. A message is queued whole; in non-blocking mode a write that
    /// does not fit writes nothing (message) or what fits (byte stream).
    pub fn write(&self, data: &[u8]) -> Result<usize, u32> {
        if !self.can_write() {
            return Err(err::ACCESS_DENIED);
        }
        let message = self.inst.is_message();
        let cap = self.tx_cap();
        let mut done = 0;
        loop {
            let mut g = self.inst.inner.lock();
            if !self.connected(&g) {
                return Err(if self.stale(&g) || g.state != NP_STATE_CLOSING { ERROR_PIPE_NOT_CONNECTED } else { err::BROKEN_PIPE });
            }
            let tx = self.tx(&mut g);
            let room = cap.saturating_sub(tx.data.len());
            if message {
                // A message larger than the buffer goes in once it is empty.
                if data.len() <= room || tx.data.is_empty() {
                    tx.data.extend(data);
                    tx.msgs.push_back(data.len());
                    drop(g);
                    self.inst.notify();
                    return Ok(data.len());
                }
            } else if room > 0 {
                let k = room.min(data.len() - done);
                tx.data.extend(&data[done..done + k]);
                done += k;
                drop(g);
                self.inst.notify();
                if done == data.len() {
                    return Ok(done);
                }
                continue;
            } else if data.is_empty() {
                return Ok(0);
            }
            drop(g);
            if self.nowait() {
                return Ok(done);
            }
            let ready = || {
                let mut g = self.inst.inner.lock();
                let len = self.tx(&mut g).data.len();
                len < cap || len == 0 || !self.connected(&g)
            };
            if !self.inst.cond.wait_until(None, ready) {
                return Err(err::INTERRUPT);
            }
        }
    }

    /// DosPeekNPipe: (data, bytes in pipe, bytes in first message, state).
    pub fn peek(&self, n: usize) -> (Vec<u8>, u32, u32, u32) {
        let mut g = self.inst.inner.lock();
        let state = if self.stale(&g) { NP_STATE_DISCONNECTED } else { g.state };
        let rx = self.rx(&mut g);
        let first = rx.msgs.front().copied().unwrap_or(0);
        let want = if self.inst.is_message() && self.mode.load(Ordering::Relaxed) & NP_READMODE_MESSAGE != 0 { n.min(first) } else { n };
        let v: Vec<u8> = rx.data.iter().take(want).copied().collect();
        (v, rx.data.len() as u32, first as u32, state)
    }

    /// DosConnectNPipe (server).
    pub fn connect(&self) -> u32 {
        if !self.server {
            return ERROR_BAD_PIPE;
        }
        let mut g = self.inst.inner.lock();
        match g.state {
            NP_STATE_CONNECTED => return 0,
            NP_STATE_CLOSING => return err::BROKEN_PIPE,
            NP_STATE_DISCONNECTED => {
                g.state = NP_STATE_LISTENING;
                drop(g);
                LISTEN.wake_all();
            }
            _ => drop(g),
        }
        if self.nowait() {
            return ERROR_PIPE_NOT_CONNECTED;
        }
        let ok = self.inst.cond.wait_until(None, || self.inst.inner.lock().state != NP_STATE_LISTENING);
        if !ok {
            return err::INTERRUPT;
        }
        match self.inst.inner.lock().state {
            NP_STATE_CONNECTED => 0,
            NP_STATE_CLOSING => err::BROKEN_PIPE,
            _ => ERROR_PIPE_NOT_CONNECTED,
        }
    }

    /// DosDisConnectNPipe (server): drop the client and discard data.
    pub fn disconnect(&self) -> u32 {
        if !self.server {
            return ERROR_BAD_PIPE;
        }
        let mut g = self.inst.inner.lock();
        g.state = NP_STATE_DISCONNECTED;
        g.generation = g.generation.wrapping_add(1);
        g.to_client.clear();
        g.to_server.clear();
        g.sem[1] = None;
        drop(g);
        self.inst.notify();
        0
    }

    /// DosSetNPipeSem.
    pub fn set_sem(&self, h: u32, key: u16) {
        self.inst.inner.lock().sem[!self.server as usize] = Some((h, key));
        self.inst.notify();
    }

    /// Records for DosQueryNPipeSemState: (status, wait flag, key, avail).
    pub fn sem_records(&self, h: u32) -> Vec<(u8, u8, u16, u16)> {
        let mut g = self.inst.inner.lock();
        let Some((sh, key)) = g.sem[!self.server as usize] else { return Vec::new() };
        if sh != h {
            return Vec::new();
        }
        let mut v = Vec::new();
        let state = g.state;
        let rx_len = self.rx(&mut g).data.len();
        let tx_len = self.tx(&mut g).data.len();
        if rx_len > 0 {
            v.push((1, 0, key, rx_len.min(0xffff) as u16)); // NPSS_RDATA
        }
        if state == NP_STATE_CONNECTED && tx_len < self.tx_cap() {
            v.push((2, 0, key, (self.tx_cap() - tx_len).min(0xffff) as u16)); // NPSS_WSPACE
        }
        if state == NP_STATE_CLOSING {
            v.push((3, 0, key, 0)); // NPSS_CLOSE
        }
        v
    }
}

impl Drop for End {
    fn drop(&mut self) {
        let mut g = self.inst.inner.lock();
        if self.server {
            // The client, if any, sees the pipe closing.
            g.state = NP_STATE_CLOSING;
            g.sem[0] = None;
            drop(g);
            PIPES.lock().retain(|i| !Arc::ptr_eq(i, &self.inst));
        } else {
            if g.generation != self.generation {
                return;
            }
            if g.state == NP_STATE_CONNECTED {
                g.state = NP_STATE_CLOSING;
            }
            g.sem[1] = None;
            drop(g);
        }
        self.inst.notify();
        LISTEN.wake_all();
    }
}

/// Every pipe sharing semaphore `h` (DosQueryNPipeSemState).
pub fn sem_records(ends: &[Arc<End>], h: u32) -> Vec<(u8, u8, u16, u16)> {
    let mut v = Vec::new();
    for e in ends {
        v.extend(e.sem_records(h));
    }
    v.push((0, 0, 0, 0)); // NPSS_EOI
    v
}
