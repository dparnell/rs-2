//! Anonymous pipes (DosCreatePipe / DosMakePipe).

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::api::err;
use crate::sync::SpinLock;
use crate::task::WaitQueue;

pub struct Pipe {
    buf: SpinLock<VecDeque<u8>>,
    cap: usize,
    readers: AtomicU32,
    writers: AtomicU32,
    readable: WaitQueue,
    writable: WaitQueue,
}

pub struct PipeEnd {
    pipe: Arc<Pipe>,
    write: bool,
    mode: AtomicU32,
}

impl Drop for PipeEnd {
    fn drop(&mut self) {
        if self.write {
            self.pipe.writers.fetch_sub(1, Ordering::AcqRel);
        } else {
            self.pipe.readers.fetch_sub(1, Ordering::AcqRel);
        }
        self.pipe.readable.wake_all();
        self.pipe.writable.wake_all();
    }
}

/// Create a pipe; returns (read end, write end).
pub fn create(size: usize) -> (Arc<PipeEnd>, Arc<PipeEnd>) {
    let cap = if size == 0 { 4096 } else { size.clamp(512, 65536) };
    let p = Arc::new(Pipe {
        buf: SpinLock::new(VecDeque::with_capacity(cap)),
        cap,
        readers: AtomicU32::new(1),
        writers: AtomicU32::new(1),
        readable: WaitQueue::new(),
        writable: WaitQueue::new(),
    });
    (
        Arc::new(PipeEnd { pipe: p.clone(), write: false, mode: AtomicU32::new(0x0040) }),
        Arc::new(PipeEnd { pipe: p, write: true, mode: AtomicU32::new(0x0041) }),
    )
}

impl PipeEnd {
    pub fn mode(&self) -> u32 {
        self.mode.load(Ordering::Relaxed)
    }

    pub fn set_mode(&self, m: u32) {
        let keep = self.mode() & 0x7;
        self.mode.store((m & !0x7) | keep, Ordering::Relaxed);
    }

    /// Read up to `n` bytes; blocks until data arrives or all writers close
    /// (then returns empty = end of file).
    pub fn read(&self, n: usize) -> Result<Vec<u8>, u32> {
        if self.write {
            return Err(err::ACCESS_DENIED);
        }
        let p = &self.pipe;
        // Interrupted by a signal or a kill: ERROR_INTERRUPT, not end of file.
        if !p.readable.wait_until(None, || !p.buf.lock().is_empty() || p.writers.load(Ordering::Acquire) == 0) {
            return Err(err::INTERRUPT);
        }
        let mut b = p.buf.lock();
        let k = n.min(b.len());
        let v: Vec<u8> = b.drain(..k).collect();
        drop(b);
        p.writable.wake_all();
        Ok(v)
    }

    pub fn write(&self, data: &[u8]) -> Result<usize, u32> {
        if !self.write {
            return Err(err::ACCESS_DENIED);
        }
        let p = &self.pipe;
        let mut done = 0;
        while done < data.len() {
            if p.readers.load(Ordering::Acquire) == 0 {
                return Err(err::BROKEN_PIPE);
            }
            if !p.writable.wait_until(None, || p.buf.lock().len() < p.cap || p.readers.load(Ordering::Acquire) == 0) {
                // A writer that is killed or signalled while the pipe is full
                // gives up (it used to wait for a reader forever).
                return if done > 0 { Ok(done) } else { Err(err::INTERRUPT) };
            }
            let mut b = p.buf.lock();
            let room = p.cap - b.len();
            let k = room.min(data.len() - done);
            b.extend(&data[done..done + k]);
            done += k;
            drop(b);
            p.readable.wake_all();
        }
        Ok(done)
    }
}
