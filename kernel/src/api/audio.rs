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
//! | 309 | Rs2AudioOpenInput(ULONG ulRate, ULONG ulChannels, ULONG ulBits, PULONG phStream) |
//! | 310 | Rs2AudioRead(ULONG hStream, PVOID pBuf, ULONG cb, ULONG ulTimeoutMs, PULONG pcbRead) |
//! | 311 | Rs2AudioQueryLevel(ULONG hStream, PULONG pulLevel) |
//! | 312 | Rs2AudioSetInput(ULONG ulSource, PULONG pulSource) |
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
//!
//! Recording: Rs2AudioOpenInput opens the one capture stream (in the
//! formats of Rs2AudioOpen) and starts the input; ERROR_NOT_SUPPORTED
//! without input hardware, ERROR_BUSY (170) while another capture stream
//! is open. Rs2AudioRead waits until `cb` bytes (at most a second's
//! worth) have been recorded or `ulTimeoutMs` has passed (0: no wait,
//! 0xFFFFFFFF: forever), then copies what there is, in whole frames, up
//! to `cb`, and stores the count; a stream holds at most 2 s, the oldest
//! going first. Rs2AudioQueryLevel gives the recent peak of the input
//! (after the stream's volume), left in the low word and right in the
//! high one, 0-32767. Rs2AudioSetInput selects the source of the input
//! (1 line in, 2 microphone, 0 keeps it) and stores the one taken, which
//! is another when the hardware lacks it; the choice lasts for later
//! captures. A capture handle works with Rs2AudioQueryPos (bytes
//! recorded since it opened), Stop (discards what is queued), Close (ends
//! the capture), SetVolume (the recording level, 0-100) and Pause (what
//! is recorded meanwhile is dropped).

use alloc::vec;

use super::{err, Ctx};
use crate::audio::{self, MIXER, WAITERS};

const ERROR_TIMEOUT: u32 = 640;
const ERROR_BUSY: u32 = 170;
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
    let played = {
        let mut m = MIXER.lock();
        match m.stream(h, c.process.pid) {
            Some(s) => s.played,
            None => match m.capture(h, c.process.pid) {
                Some(cap) => cap.captured,
                None => return err::INVALID_HANDLE,
            },
        }
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
    let found = {
        let mut m = MIXER.lock();
        match m.stream(h, c.process.pid) {
            Some(s) => {
                s.discard();
                true
            }
            None => m.capture(h, c.process.pid).map(|cap| cap.discard()).is_some(),
        }
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
    let pid = c.process.pid;
    if let Some(s) = m.stream(h, pid) {
        s.volume = v;
        return 0;
    }
    match m.capture(h, pid) {
        Some(cap) => {
            cap.volume = v;
            0
        }
        None => err::INVALID_HANDLE,
    }
}

/// Rs2AudioPause.
pub fn pause(c: &mut Ctx) -> u32 {
    let (h, f) = (c.arg(0), c.arg(1));
    let mut m = MIXER.lock();
    let pid = c.process.pid;
    if let Some(s) = m.stream(h, pid) {
        s.paused = f != 0;
        return 0;
    }
    match m.capture(h, pid) {
        Some(cap) => {
            cap.paused = f != 0;
            0
        }
        None => err::INVALID_HANDLE,
    }
}

/// Rs2AudioOpenInput.
pub fn open_input(c: &mut Ctx) -> u32 {
    let (rate, channels, bits, ph) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if !audio::has_input() {
        return err::NOT_SUPPORTED;
    }
    if !(4000..=96000).contains(&rate) || !(1..=2).contains(&channels) || !matches!(bits, 8 | 16) || ph == 0 {
        return err::INVALID_PARAMETER;
    }
    let pid = c.process.pid;
    let h = match MIXER.lock().open_input(pid, rate, channels, bits) {
        Ok(h) => h,
        Err(true) => return ERROR_BUSY,
        Err(false) => return err::NOT_SUPPORTED,
    };
    if !c.write_u32(ph, h) {
        MIXER.lock().close(h, pid);
        return err::INVALID_PARAMETER;
    }
    0
}

/// Rs2AudioRead.
pub fn read(c: &mut Ctx) -> u32 {
    let (h, buf, cb, ms, pn) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    let pid = c.process.pid;
    let (generation, want) = match MIXER.lock().capture(h, pid) {
        Some(cap) => (cap.generation, (cb as usize).min(cap.rate as usize * cap.frame_bytes()).max(cap.frame_bytes())),
        None => return err::INVALID_HANDLE,
    };
    if pn == 0 {
        return err::INVALID_PARAMETER;
    }
    let deadline = (ms != u32::MAX).then(|| crate::irq::ticks() + (ms as u64 * crate::irq::HZ).div_ceil(1000));
    let ready = || {
        let mut m = MIXER.lock();
        m.capture(h, pid).is_none_or(|cap| cap.generation != generation || cap.queued() >= want)
    };
    if !WAITERS.wait_until(deadline, ready) && (c.thread.exit_requested() || c.thread.interrupted()) {
        return err::INTERRUPT;
    }
    let data = match MIXER.lock().capture(h, pid) {
        Some(cap) => cap.take(cb as usize),
        None => return err::INVALID_HANDLE,
    };
    if !c.write(buf, &data) || !c.write_u32(pn, data.len() as u32) {
        return err::INVALID_PARAMETER;
    }
    0
}

/// Rs2AudioQueryLevel.
pub fn query_level(c: &mut Ctx) -> u32 {
    let (h, p) = (c.arg(0), c.arg(1));
    let level = match MIXER.lock().capture(h, c.process.pid) {
        Some(cap) => cap.level.map(|v| v.min(32767) as u32),
        None => return err::INVALID_HANDLE,
    };
    if !c.write_u32(p, level[0] | level[1] << 16) {
        return err::INVALID_PARAMETER;
    }
    0
}

/// Rs2AudioSetInput.
pub fn set_input(c: &mut Ctx) -> u32 {
    let (source, p) = (c.arg(0), c.arg(1));
    if !audio::has_input() {
        return err::NOT_SUPPORTED;
    }
    if source > audio::SOURCE_MIC {
        return err::INVALID_PARAMETER;
    }
    let taken = MIXER.lock().set_source(source);
    if p != 0 && !c.write_u32(p, taken) {
        return err::INVALID_PARAMETER;
    }
    0
}
