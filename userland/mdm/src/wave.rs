//! The wave audio device: a WAV file (the element) or a memory playlist
//! of PCM buffers, played through RS2API's audio streams. Without sound
//! hardware it plays silently in real time, so that programs waiting for
//! a sound (PLAY WAIT) or its notification behave the same.

use alloc::vec::Vec;

use crate::dev::{self, *};
use crate::req::*;
use crate::sys::*;
use mci::playlist::{self, Processor, Step};
use mci::*;

/// Bytes handed to the audio stream at a time.
const CHUNK: usize = 8192;

#[derive(Clone, Copy, Debug)]
pub struct Fmt {
    pub tag: u32,
    pub channels: u32,
    pub rate: u32,
    pub bits: u32,
    pub block: u32,
    pub avg: u32,
}

impl Fmt {
    fn fix(&mut self) {
        self.channels = self.channels.clamp(1, 2);
        self.bits = if self.bits <= 8 { 8 } else { 16 };
        self.block = self.channels * self.bits / 8;
        self.avg = self.rate * self.block;
    }
}

pub struct Wave {
    pub fmt: Fmt,
    /// PCM (8-bit unsigned or 16-bit signed) of the element.
    pub data: Option<Vec<u8>>,
    /// The playlist's address (MCI_OPEN_PLAYLIST).
    pub playlist: Option<u32>,
    pub proc_: Processor,
    /// The playing position, in bytes of `data` (or of the playlist's data
    /// played so far).
    pub pos: u32,
    /// Where PLAY stops (bytes; u32::MAX for the end).
    pub end: u32,
    pub stream: u32,
    /// What the stream had played when the current PLAY started at `pos`.
    pub base_played: u32,
    pub volume: u32,
    pub paused: bool,
}

impl Wave {
    pub fn new(element: Option<&str>, playlist: Option<u32>) -> Result<Wave, u32> {
        let mut w = Wave {
            fmt: Fmt { tag: 1, channels: 1, rate: 22050, bits: 8, block: 1, avg: 22050 },
            data: None,
            playlist,
            proc_: Processor::new(playlist.unwrap_or(0)),
            pos: 0,
            end: u32::MAX,
            stream: 0,
            base_played: 0,
            volume: 100,
            paused: false,
        };
        if let Some(e) = element {
            w.load(e)?;
        }
        Ok(w)
    }

    /// Read a WAV file as the element.
    pub fn load(&mut self, name: &str) -> Result<(), u32> {
        let file = read_file(name).ok_or(MCIERR_FILE_NOT_FOUND)?;
        let wav = mmfmt::wav::parse(&file).map_err(|_| MCIERR_INVALID_MEDIA_TYPE)?;
        let f = wav.format;
        let raw = wav.data(&file);
        let (fmt, pcm) = if f.is_plain_pcm() {
            (f, raw.to_vec())
        } else {
            let s = mmfmt::wav::decode_pcm16(&f, raw).ok_or(MCIERR_UNSUPP_FORMAT_TAG)?;
            (f.pcm16(), s.iter().flat_map(|v| v.to_le_bytes()).collect())
        };
        self.fmt = Fmt { tag: 1, channels: fmt.channels as u32, rate: fmt.rate, bits: fmt.bits as u32, block: 0, avg: 0 };
        self.fmt.fix();
        self.data = Some(pcm);
        self.playlist = None;
        self.pos = 0;
        Ok(())
    }

    fn length(&self) -> u32 {
        self.data.as_ref().map_or(0, |d| d.len() as u32)
    }

    pub fn stop(&mut self) {
        if self.stream != 0 {
            unsafe { Rs2AudioStop(self.stream) };
        }
    }

    /// Bytes to the device's time format and back.
    fn to_time(&self, bytes: u32, f: u32) -> u32 {
        let avg = self.fmt.avg.max(1) as u64;
        let b = bytes as u64;
        (match f {
            MCI_FORMAT_MILLISECONDS => b * 1000 / avg,
            MCI_FORMAT_BYTES => b,
            MCI_FORMAT_SAMPLES => b / self.fmt.block.max(1) as u64,
            _ => b * 3000 / avg,
        }) as u32
    }

    fn from_time(&self, t: u32, f: u32) -> u32 {
        let avg = self.fmt.avg.max(1) as u64;
        let t = t as u64;
        let b = match f {
            MCI_FORMAT_MILLISECONDS => t * avg / 1000,
            MCI_FORMAT_BYTES => t,
            MCI_FORMAT_SAMPLES => t * self.fmt.block.max(1) as u64,
            _ => t * avg / 3000,
        };
        let b = b.min(u32::MAX as u64) as u32;
        b - b % self.fmt.block.max(1)
    }

    /// Where playing has got to, in bytes.
    fn position(&self, playing: bool) -> u32 {
        if playing && self.stream != 0 {
            let mut n = 0;
            unsafe { Rs2AudioQueryPos(self.stream, &mut n) };
            return (self.base_played + n.saturating_sub(self.base_played)).min(self.end).max(self.pos.min(self.base_played));
        }
        self.pos
    }
}

fn wave(d: &mut Device) -> &mut Wave {
    match &mut d.kind {
        Kind::Wave(w) => w,
        _ => unreachable!(),
    }
}

pub fn command(id: u16, r: &Req) -> Result<Reply, u32> {
    match r.msg {
        MCI_PLAY => play(id, r),
        MCI_STOP => {
            stop(id, true);
            Ok(Reply::None)
        }
        MCI_PAUSE | MCI_RESUME => with(id, |d| {
            let pause = r.msg == MCI_PAUSE;
            if d.mode != MCI_MODE_PLAY && d.mode != MCI_MODE_PAUSE {
                return Ok(Reply::None);
            }
            let w = wave(d);
            w.paused = pause;
            if w.stream != 0 {
                unsafe { Rs2AudioPause(w.stream, pause as u32) };
            }
            d.mode = if pause { MCI_MODE_PAUSE } else { MCI_MODE_PLAY };
            Ok(Reply::None)
        })
        .unwrap(),
        MCI_SEEK => {
            stop(id, true);
            with(id, |d| {
                let tf = d.time_format;
                let w = wave(d);
                w.pos = if r.flags & MCI_TO_START != 0 {
                    w.proc_.rewind();
                    0
                } else if r.flags & MCI_TO_END != 0 {
                    w.length()
                } else if r.flags & MCI_TO != 0 {
                    let p = w.from_time(r.to, tf);
                    if w.data.is_some() && p > w.length() {
                        return Err(MCIERR_OUTOFRANGE);
                    }
                    p
                } else {
                    return Err(MCIERR_MISSING_PARAMETER);
                };
                Ok(Reply::None)
            })
            .unwrap()
        }
        MCI_LOAD => {
            stop(id, true);
            let name = r.element.clone().ok_or(MCIERR_MISSING_PARAMETER)?;
            let mut nw = Wave::new(Some(&name), None)?;
            with(id, |d| {
                let w = wave(d);
                nw.volume = w.volume;
                *w = nw;
                d.element = Some(name);
            });
            Ok(Reply::None)
        }
        MCI_CUE => Ok(Reply::None),
        MCI_SET => with(id, |d| set(wave(d), r)).unwrap(),
        MCI_STATUS => with(id, |d| {
            let tf = d.time_format;
            let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
            let w = wave(d);
            Ok(match r.item {
                MCI_STATUS_LENGTH => Reply::Num(w.to_time(w.length(), tf)),
                MCI_STATUS_POSITION => Reply::Num(w.to_time(w.position(playing), tf)),
                MCI_STATUS_VOLUME => Reply::Num(w.volume | w.volume << 16),
                // MCI_WAVE_STATUS_*: level, format tag, channels, rate,
                // average bytes, block align, bits per sample.
                0x4001 => Reply::Num(0),
                0x4002 => Reply::Num(w.fmt.tag),
                0x4003 => Reply::Num(w.fmt.channels),
                0x4004 => Reply::Num(w.fmt.rate),
                0x4005 => Reply::Num(w.fmt.avg),
                0x4006 => Reply::Num(w.fmt.block),
                0x4007 => Reply::Num(w.fmt.bits),
                _ => return Err(MCIERR_INVALID_ITEM_FLAG),
            })
        })
        .unwrap(),
        MCI_GETDEVCAPS => Err(MCIERR_INVALID_ITEM_FLAG),
        _ => Err(MCIERR_UNSUPPORTED_FUNCTION),
    }
}

fn set(w: &mut Wave, r: &Req) -> Result<Reply, u32> {
    let f = r.flags;
    if f & (MCI_WAVE_SET_FORMATTAG | MCI_WAVE_SET_CHANNELS | MCI_WAVE_SET_SAMPLESPERSEC | MCI_WAVE_SET_BITSPERSAMPLE | MCI_WAVE_SET_AVGBYTESPERSEC | MCI_WAVE_SET_BLOCKALIGN) != 0 {
        let [tag, ch, rate, _avg, _block, bits] = r.wave;
        if f & MCI_WAVE_SET_FORMATTAG != 0 {
            if tag != 1 {
                return Err(MCIERR_UNSUPP_FORMAT_TAG);
            }
            w.fmt.tag = 1;
        }
        if f & MCI_WAVE_SET_CHANNELS != 0 {
            if !(1..=2).contains(&ch) {
                return Err(MCIERR_UNSUPP_CHANNELS);
            }
            w.fmt.channels = ch;
        }
        if f & MCI_WAVE_SET_SAMPLESPERSEC != 0 {
            if !(4000..=48000).contains(&rate) {
                return Err(MCIERR_UNSUPP_SAMPLESPERSEC);
            }
            w.fmt.rate = rate;
        }
        if f & MCI_WAVE_SET_BITSPERSAMPLE != 0 {
            if bits != 8 && bits != 16 {
                return Err(MCIERR_UNSUPP_BITSPERSAMPLE);
            }
            w.fmt.bits = bits;
        }
        w.fmt.fix();
    }
    if f & MCI_SET_AUDIO != 0 && f & MCI_SET_VOLUME != 0 {
        w.volume = r.value.min(100);
        if w.stream != 0 {
            unsafe { Rs2AudioSetVolume(w.stream, w.volume) };
        }
    }
    Ok(Reply::None)
}

/// Stop playing; `abort` settles a pending PLAY notification as aborted.
pub fn stop(id: u16, abort: bool) {
    with(id, |d| {
        d.epoch = d.epoch.wrapping_add(1);
        if abort {
            d.finish(MCI_NOTIFY_ABORTED);
        }
        let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
        let w = wave(d);
        if playing {
            w.pos = w.position(true);
        }
        w.stop();
        w.paused = false;
        d.mode = MCI_MODE_STOP;
    });
}

fn play(id: u16, r: &Req) -> Result<Reply, u32> {
    let epoch = with(id, |d| -> Result<u32, u32> {
        let tf = d.time_format;
        // A new PLAY supersedes the one in progress.
        d.epoch = d.epoch.wrapping_add(1);
        d.finish(MCI_NOTIFY_SUPERSEDED);
        let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
        let w = wave(d);
        if playing {
            w.pos = w.position(true);
            w.stop();
        }
        if w.data.is_none() && w.playlist.is_none() {
            return Err(MCIERR_MISSING_PARAMETER);
        }
        if r.flags & MCI_FROM != 0 {
            w.pos = w.from_time(r.from, tf);
            if w.playlist.is_some() && r.from == 0 {
                w.proc_.rewind();
            }
        }
        w.end = if r.flags & MCI_TO != 0 { w.from_time(r.to, tf) } else { u32::MAX };
        if w.data.is_some() && w.pos >= w.length() {
            w.pos = 0;
        }
        w.paused = false;
        d.mode = MCI_MODE_PLAY;
        d.take_notify(r);
        Ok(d.epoch)
    })
    .ok_or(MCIERR_INVALID_DEVICE_ID)??;
    let param = id as u32 | (epoch & 0xffff) << 16;
    if r.flags & MCI_WAIT != 0 {
        unsafe { player(param) };
    } else {
        let mut tid = 0;
        if unsafe { DosCreateThread(&mut tid, player, param, 0, 65536) } != 0 {
            with(id, |d| d.mode = MCI_MODE_STOP);
            return Err(MCIERR_OUT_OF_MEMORY);
        }
    }
    Ok(Reply::None)
}

/// The playlist in the application's memory.
struct AppMem;

impl playlist::Memory for AppMem {
    fn read(&self, a: u32) -> u32 {
        rd(a, 0)
    }
    fn write(&mut self, a: u32, v: u32) {
        wr(a, 0, v)
    }
}

enum Next {
    /// Bytes to play (and for a playlist DATA: where to count them).
    Data(Vec<u8>, Option<u32>),
    Wait,
    /// SEMWAIT: wait outside the lock.
    Sem(u32, u32, bool),
    End,
    Stopped,
}

fn current(d: &Device, epoch: u32) -> bool {
    d.epoch & 0xffff == epoch
}

unsafe extern "C" fn player(param: u32) {
    let id = param as u16;
    let epoch = param >> 16;
    let Some(fmt) = with(id, |d| wave(d).fmt) else { return };
    let mut stream = 0;
    let have_audio = unsafe { Rs2AudioOpen(fmt.rate, fmt.channels, fmt.bits, &mut stream) } == 0;
    let ok = with(id, |d| {
        if !current(d, epoch) {
            return false;
        }
        let w = wave(d);
        w.stream = if have_audio { stream } else { 0 };
        w.base_played = w.pos;
        if have_audio {
            unsafe { Rs2AudioSetVolume(stream, w.volume) };
        }
        true
    })
    .unwrap_or(false);
    // The bytes of the current playlist DATA left to play: (address, left,
    // operand 3's address).
    let mut pl_data: Option<(u32, u32, u32)> = None;
    let mut played: u32 = 0;
    let mut done = false;
    let mut stopped = !ok;
    while !stopped && !done {
        let next = with(id, |d| {
            if !current(d, epoch) {
                return Next::Stopped;
            }
            let notify_hwnd = d.pending.map_or(0, |n| n.hwnd);
            let did = d.id;
            let w = wave(d);
            if w.paused {
                return Next::Wait;
            }
            if let Some(data) = &w.data {
                let end = w.end.min(data.len() as u32);
                if w.pos >= end {
                    return Next::End;
                }
                let n = (end - w.pos).min(CHUNK as u32);
                let c = data[w.pos as usize..(w.pos + n) as usize].to_vec();
                w.pos += n;
                return Next::Data(c, None);
            }
            loop {
                if let Some((a, left, at)) = pl_data {
                    if left > 0 {
                        let n = left.min(CHUNK as u32);
                        let c = unsafe { core::slice::from_raw_parts(a as *const u8, n as usize) }.to_vec();
                        pl_data = if left > n { Some((a + n, left - n, at)) } else { None };
                        w.pos += n;
                        return Next::Data(c, Some(at));
                    }
                    pl_data = None;
                }
                if w.pos >= w.end {
                    return Next::End;
                }
                match w.proc_.next(&mut AppMem) {
                    Step::Data { addr, len, pos_at } => {
                        wr(pos_at, 0, 0);
                        pl_data = Some((addr, len, pos_at));
                    }
                    Step::Message(v) => {
                        let ins = w.proc_.position().wrapping_sub(1);
                        if notify_hwnd != 0 {
                            unsafe { WinPostMsg(notify_hwnd, MM_MCIPLAYLISTMESSAGE, mp(ins, did as u32), v) };
                        }
                    }
                    Step::Cuepoint { user, offset } => {
                        if notify_hwnd != 0 {
                            unsafe { WinPostMsg(notify_hwnd, MM_MCICUEPOINT, mp(user, did as u32), offset) };
                        }
                    }
                    Step::SemPost(s) => unsafe {
                        DosPostEventSem(s);
                    },
                    Step::SemWait { sem, timeout, reset } => return Next::Sem(sem, timeout, reset),
                    Step::Exit => return Next::End,
                }
            }
        })
        .unwrap_or(Next::Stopped);
        match next {
            Next::Stopped => stopped = true,
            Next::End => done = true,
            Next::Wait => unsafe {
                DosSleep(30);
            },
            Next::Sem(sem, timeout, reset) => unsafe {
                if reset {
                    let mut n = 0;
                    DosResetEventSem(sem, &mut n);
                }
                DosWaitEventSem(sem, timeout);
            },
            Next::Data(c, at) => {
                if c.is_empty() {
                    continue;
                }
                if have_audio {
                    unsafe { Rs2AudioWrite(stream, c.as_ptr(), c.len() as u32) };
                } else {
                    // Real time without sound hardware.
                    unsafe { DosSleep((c.len() as u64 * 1000 / fmt.avg.max(1) as u64) as u32) };
                }
                played += c.len() as u32;
                if let Some(at) = at {
                    wr(at, 0, rd(at, 0) + c.len() as u32);
                }
            }
        }
    }
    let _ = played;
    // Let what is queued finish (unless stopped meanwhile). A stream
    // whose position stands still for STUCK_MS while not paused is given
    // up on: the play ends as if it had finished.
    const STUCK_MS: u32 = 3000;
    if done && have_audio {
        let (mut last, mut still_for) = (u32::MAX, 0);
        loop {
            let (still, paused) = with(id, |d| (current(d, epoch), d.mode == MCI_MODE_PAUSE)).unwrap_or((false, false));
            if !still {
                stopped = true;
                break;
            }
            if unsafe { Rs2AudioDrain(stream, 100) } == 0 {
                break;
            }
            let mut pos = 0;
            unsafe { Rs2AudioQueryPos(stream, &mut pos) };
            still_for = if pos != last || paused { 0 } else { still_for + 100 };
            last = pos;
            if still_for >= STUCK_MS {
                unsafe { Rs2AudioStop(stream) };
                break;
            }
        }
    }
    let auto_close = with(id, |d| {
        let mine = current(d, epoch);
        let w = wave(d);
        if mine {
            w.stream = 0;
        }
        if mine && !stopped {
            d.mode = MCI_MODE_STOP;
            d.finish(MCI_NOTIFY_SUCCESSFUL);
            return d.auto_close;
        }
        false
    })
    .unwrap_or(false);
    if have_audio {
        unsafe { Rs2AudioClose(stream) };
    }
    if auto_close {
        let _ = dev::close(id);
    }
}
