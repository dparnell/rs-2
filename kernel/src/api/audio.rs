//! RS2API audio entries (ordinals 300-308): PCM output through the
//! kernel mixer (`crate::audio`), for rs2's MDM.DLL. 32-bit `_System`
//! calls returning OS/2 error codes.
//!
//! | Ord | Entry |
//! |-----|-------|
//! | 300 | Rs2AudioQuery(PULONG pulDevices) |
//! | 301 | Rs2AudioOpen(ULONG ulRate, ULONG ulChannels, ULONG ulBits, PULONG phStream) |
//! | 302 | Rs2AudioWrite(ULONG hStream, PVOID pData, ULONG cb) |
//! | 303 | Rs2AudioQueryPos(ULONG hStream, PULONG pulBytes) |
//! | 304 | Rs2AudioDrain(ULONG hStream, ULONG ulTimeoutMs) |
//! | 305 | Rs2AudioStop(ULONG hStream) |
//! | 306 | Rs2AudioClose(ULONG hStream) |
//! | 307 | Rs2AudioSetVolume(ULONG hStream, ULONG ulVolume) |
//! | 308 | Rs2AudioPause(ULONG hStream, ULONG fPause) |
//!
//! Rs2AudioQuery fails with ERROR_NOT_SUPPORTED (50) without sound
//! hardware (and so does Rs2AudioOpen); otherwise `*pulDevices` is 1. A
//! stream plays 8-bit unsigned or 16-bit signed little-endian PCM, mono
//! or stereo (interleaved left, right), at 4000-96000 Hz. Rs2AudioWrite
//! copies the data into the stream's queue, waiting while the queue
//! holds more than half a second; it returns early with 0 when another
//! thread stops the stream. Rs2AudioQueryPos gives the bytes of the
//! stream's input that have been played. Rs2AudioDrain waits until all
//! that was written has played (ERROR_TIMEOUT 640 after `ulTimeoutMs`;
//! 0xFFFFFFFF waits forever). Rs2AudioStop discards what is queued at
//! once (up to 0.1 s already mixed is still heard). Volumes are 0-100
//! (linear, default 100); stream handle 0 sets the master volume. A
//! paused stream keeps its queue and resumes where it was. Stream
//! handles belong to the process that opened them and are closed when
//! it ends. Calls that wait return ERROR_INTERRUPT (95) when the thread
//! is told to end.

use alloc::vec;

use super::{err, Ctx};
use crate::audio::{self, MIXER, WAITERS};

const ERROR_TIMEOUT: u32 = 640;
/// Most bytes copied in at a time by Rs2AudioWrite.
const PIECE: usize = 32 * 1024;

/// Rs2AudioQuery.
pub fn query(c: &mut Ctx) -> u32 {
    if !audio::present() {
        return err::NOT_SUPPORTED;
    }
    if !c.write_u32(c.arg(0), 1) {
        return err::INVALID_PARAMETER;
    }
    0
}

/// Rs2AudioOpen.
pub fn open(c: &mut Ctx) -> u32 {
    let (rate, channels, bits, ph) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if !audio::present() {
        return err::NOT_SUPPORTED;
    }
    if !(4000..=96000).contains(&rate) || !(1..=2).contains(&channels) || !matches!(bits, 8 | 16) || ph == 0 {
        return err::INVALID_PARAMETER;
    }
    let pid = c.process.pid;
    let Some(h) = MIXER.lock().open(pid, rate, channels, bits) else { return err::TOO_MANY_OPEN_FILES };
    if !c.write_u32(ph, h) {
        MIXER.lock().close(h, pid);
        return err::INVALID_PARAMETER;
    }
    0
}

/// Rs2AudioWrite.
pub fn write(c: &mut Ctx) -> u32 {
    let (h, data, cb) = (c.arg(0), c.arg(1), c.arg(2));
    let pid = c.process.pid;
    let generation = match MIXER.lock().stream(h, pid) {
        Some(s) => s.generation,
        None => return err::INVALID_HANDLE,
    };
    let mut buf = vec![0u8; (cb as usize).min(PIECE)];
    let mut done = 0u32;
    while done < cb {
        // Room in the queue, or the stream stopped or closed.
        let ready = || {
            let mut m = MIXER.lock();
            m.stream(h, pid).is_none_or(|s| s.generation != generation || s.queued() <= s.limit())
        };
        if !WAITERS.wait_until(None, ready) {
            return err::INTERRUPT;
        }
        let n = ((cb - done) as usize).min(PIECE);
        if !c.read(data + done, &mut buf[..n]) {
            return err::INVALID_PARAMETER;
        }
        let mut m = MIXER.lock();
        match m.stream(h, pid) {
            None => return err::INVALID_HANDLE,
            Some(s) if s.generation != generation => return 0,
            Some(s) => s.push(&buf[..n]),
        }
        done += n as u32;
    }
    0
}

/// Rs2AudioQueryPos.
pub fn query_pos(c: &mut Ctx) -> u32 {
    let (h, p) = (c.arg(0), c.arg(1));
    let played = match MIXER.lock().stream(h, c.process.pid) {
        Some(s) => s.played,
        None => return err::INVALID_HANDLE,
    };
    if !c.write_u32(p, played as u32) {
        return err::INVALID_PARAMETER;
    }
    0
}

/// Rs2AudioDrain.
pub fn drain(c: &mut Ctx) -> u32 {
    let (h, ms) = (c.arg(0), c.arg(1));
    let pid = c.process.pid;
    if MIXER.lock().stream(h, pid).is_none() {
        return err::INVALID_HANDLE;
    }
    let deadline = (ms != u32::MAX).then(|| crate::irq::ticks() + (ms as u64 * crate::irq::HZ).div_ceil(1000));
    let ready = || MIXER.lock().stream(h, pid).is_none_or(|s| s.drained());
    if WAITERS.wait_until(deadline, ready) {
        return 0;
    }
    if c.thread.exit_requested() || c.thread.interrupted() { err::INTERRUPT } else { ERROR_TIMEOUT }
}

/// Rs2AudioStop.
pub fn stop(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    let found = match MIXER.lock().stream(h, c.process.pid) {
        Some(s) => {
            s.discard();
            true
        }
        None => false,
    };
    if !found {
        return err::INVALID_HANDLE;
    }
    WAITERS.wake_all();
    0
}

/// Rs2AudioClose.
pub fn close(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    let closed = MIXER.lock().close(h, c.process.pid);
    if !closed {
        return err::INVALID_HANDLE;
    }
    WAITERS.wake_all();
    0
}

/// Rs2AudioSetVolume.
pub fn set_volume(c: &mut Ctx) -> u32 {
    let (h, v) = (c.arg(0), c.arg(1));
    if v > 100 {
        return err::INVALID_PARAMETER;
    }
    let mut m = MIXER.lock();
    if h == 0 {
        if !audio::present() {
            return err::NOT_SUPPORTED;
        }
        m.master = v;
        return 0;
    }
    match m.stream(h, c.process.pid) {
        Some(s) => {
            s.volume = v;
            0
        }
        None => err::INVALID_HANDLE,
    }
}

/// Rs2AudioPause.
pub fn pause(c: &mut Ctx) -> u32 {
    let (h, f) = (c.arg(0), c.arg(1));
    match MIXER.lock().stream(h, c.process.pid) {
        Some(s) => {
            s.paused = f != 0;
            0
        }
        None => err::INVALID_HANDLE,
    }
}
