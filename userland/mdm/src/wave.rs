//! The wave audio device: a WAV file (the element) or a memory playlist
//! of PCM buffers, played through RS2API's audio streams. Without sound
//! hardware it plays silently in real time, so that programs waiting for
//! a sound (PLAY WAIT) or its notification behave the same.
//!
//! Recording (RECORD FROM/TO, INSERT or OVERWRITE, the default) reads
//! the kernel's capture stream (RS2API 309-312) in the device's format
//! into the element at the position, or into a playlist's DATA buffers;
//! without input hardware it records silence in real time. SAVE writes
//! the element as a WAV file, DELETE cuts a range out of it. CUE INPUT
//! opens the capture stream ahead of a RECORD, so STATUS LEVEL shows the
//! input's level. CONNECTOR ENABLE TYPE MICROPHONE or LINE IN selects the
//! source; SET MONITOR ON plays what is recorded as it comes. CONNECTION
//! QUERY TYPE WAVE STREAM ALIAS name names the device's amp-mixer
//! connection: rs2 has no amp-mixer device, so that alias reaches the
//! wave device itself (as IBM's RECORD.CMD uses it, for SET MONITOR and
//! CONNECTOR).

use alloc::vec::Vec;

use crate::dev::{self, *};
use crate::req::*;
use crate::sys::*;
use mci::playlist::{self, Processor, Step};
use mci::*;

/// Bytes handed to the audio stream at a time.
const CHUNK: usize = 8192;

// MCI_WAVE_STATUS_* items.
pub const MCI_WAVE_STATUS_LEVEL: u32 = 0x6000;
pub const MCI_WAVE_STATUS_SAMPLESPERSEC: u32 = 0x6001;
pub const MCI_WAVE_STATUS_AVGBYTESPERSEC: u32 = 0x6002;
pub const MCI_WAVE_STATUS_BLOCKALIGN: u32 = 0x6003;
pub const MCI_WAVE_STATUS_FORMATTAG: u32 = 0x6004;
pub const MCI_WAVE_STATUS_CHANNELS: u32 = 0x6005;
pub const MCI_WAVE_STATUS_BITSPERSAMPLE: u32 = 0x6006;
const MCI_STATUS_MONITOR: u32 = 0x0c;

// Connector types (MCI_CONNECTOR, MCI_CONNECTION).
pub const MCI_WAVE_STREAM_CONNECTOR: u32 = 3;
pub const MCI_AMP_STREAM_CONNECTOR: u32 = 4;
pub const MCI_HEADPHONES_CONNECTOR: u32 = 6;
pub const MCI_SPEAKERS_CONNECTOR: u32 = 7;
pub const MCI_MICROPHONE_CONNECTOR: u32 = 8;
pub const MCI_LINE_IN_CONNECTOR: u32 = 9;
pub const MCI_LINE_OUT_CONNECTOR: u32 = 10;

/// RS2API's input sources.
const SOURCE_LINE: u32 = 1;
const SOURCE_MIC: u32 = 2;

/// Milliseconds of sound read from the capture stream at a time.
const REC_MS: u32 = 100;
const ERROR_BUSY: u32 = 170;

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
    /// The capture stream (CUE INPUT or RECORD), and whether a recorder
    /// thread has it.
    pub input: u32,
    pub recording: bool,
    /// RECORD INSERT (else overwrite).
    insert: bool,
    /// SET MONITOR: play what is recorded.
    pub monitor: bool,
    /// The input source (connector) chosen.
    source: u32,
    /// Recorded since the last SAVE (or the load).
    pub dirty: bool,
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
            input: 0,
            recording: false,
            insert: false,
            monitor: false,
            source: SOURCE_LINE,
            dirty: false,
        };
        if let Some(e) = element {
            match w.load(e) {
                // A new file: an empty element, written by SAVE.
                Err(MCIERR_FILE_NOT_FOUND) => w.data = Some(Vec::new()),
                r => r?,
            }
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
        if self.input != 0 {
            if self.recording {
                // Wakes the recorder, which closes the stream.
                unsafe { Rs2AudioStop(self.input) };
            } else {
                unsafe { Rs2AudioClose(self.input) };
            }
            self.input = 0;
        }
        self.recording = false;
    }

    /// Open the capture stream in the device's format (for CUE INPUT and
    /// RECORD); false without input hardware.
    fn open_input(&mut self) -> bool {
        if self.input != 0 {
            return true;
        }
        let mut h = 0;
        unsafe { Rs2AudioSetInput(self.source, core::ptr::null_mut()) };
        // A recorder just stopped may still hold the capture stream.
        let mut rc = 0;
        for _ in 0..50 {
            rc = unsafe { Rs2AudioOpenInput(self.fmt.rate, self.fmt.channels, self.fmt.bits, &mut h) };
            if rc != ERROR_BUSY {
                break;
            }
            unsafe { DosSleep(10) };
        }
        if rc != 0 {
            return false;
        }
        unsafe { Rs2AudioSetVolume(h, 100) };
        self.input = h;
        true
    }

    /// Recorded bytes into the element at the position (inserted or over
    /// what is there), stopping at `end`; how many went in.
    fn put(&mut self, mut bytes: &[u8]) -> u32 {
        let pos = self.pos as usize;
        if self.end != u32::MAX {
            let room = (self.end as usize).saturating_sub(pos);
            bytes = &bytes[..bytes.len().min(room)];
        }
        let data = self.data.get_or_insert_with(Vec::new);
        let pos = pos.min(data.len());
        if self.insert {
            data.splice(pos..pos, bytes.iter().copied());
        } else {
            let over = (data.len() - pos).min(bytes.len());
            data[pos..pos + over].copy_from_slice(&bytes[..over]);
            data.extend_from_slice(&bytes[over..]);
        }
        self.pos = (pos + bytes.len()) as u32;
        self.dirty = true;
        bytes.len() as u32
    }

    /// The element as a WAV file.
    fn wav(&self) -> Vec<u8> {
        let f = &self.fmt;
        let wf = mmfmt::wav::WaveFormat { tag: 1, channels: f.channels as u16, rate: f.rate, bits: f.bits as u16, block_align: f.block as u16, avg_bytes: f.avg };
        mmfmt::wav::build(&wf, self.data.as_deref().unwrap_or(&[]))
    }

    /// The input's level: left and right, 0-100 each (low and high word).
    fn level(&self) -> u32 {
        if self.input == 0 {
            return 0;
        }
        let mut v = 0;
        unsafe { Rs2AudioQueryLevel(self.input, &mut v) };
        let pct = |x: u32| (x & 0xffff) * 100 / 32767;
        pct(v) | pct(v >> 16) << 16
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
        if playing && self.stream != 0 && !self.recording {
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
        MCI_RECORD => record(id, r),
        MCI_SAVE => save(id, r),
        MCI_DELETE => {
            stop(id, true);
            with(id, |d| {
                let tf = d.time_format;
                let w = wave(d);
                let len = w.length();
                let from = if r.flags & MCI_FROM != 0 { w.from_time(r.from, tf) } else { w.pos.min(len) };
                let to = if r.flags & MCI_TO != 0 { w.from_time(r.to, tf) } else { len };
                if w.playlist.is_some() {
                    return Err(MCIERR_UNSUPPORTED_FUNCTION);
                }
                if from > len || to > len || from > to {
                    return Err(MCIERR_OUTOFRANGE);
                }
                if let Some(data) = w.data.as_mut() {
                    data.drain(from as usize..to as usize);
                }
                w.pos = from;
                w.dirty = true;
                Ok(Reply::None)
            })
            .unwrap()
        }
        MCI_CONNECTOR => with(id, |d| connector(wave(d), r)).unwrap(),
        MCI_CONNECTION => {
            // The wave stream's connection is the amp-mixer, here the
            // device itself: its alias reaches this device.
            if r.flags & MCI_QUERY_CONNECTION == 0 {
                return Err(MCIERR_UNSUPPORTED_FLAG);
            }
            if r.flags & MCI_CONNECTOR_TYPE != 0 && !matches!(r.item, MCI_WAVE_STREAM_CONNECTOR | MCI_AMP_STREAM_CONNECTOR) {
                return Err(MCIERR_INVALID_CONNECTOR_TYPE);
            }
            if let Some(a) = &r.text {
                if dev::find(a).is_some_and(|other| other != id) {
                    return Err(MCIERR_DUPLICATE_ALIAS);
                }
            }
            with(id, |d| {
                if r.text.is_some() {
                    d.conn_alias = r.text.clone();
                }
                Ok(Reply::Named(d.id as u32, "Ampmix01"))
            })
            .unwrap()
        }
        MCI_STOP => {
            stop(id, true);
            Ok(Reply::None)
        }
        MCI_PAUSE | MCI_RESUME => with(id, |d| {
            let pause = r.msg == MCI_PAUSE;
            if !matches!(d.mode, MCI_MODE_PLAY | MCI_MODE_PAUSE | MCI_MODE_RECORD) {
                return Ok(Reply::None);
            }
            let w = wave(d);
            w.paused = pause;
            if w.stream != 0 {
                unsafe { Rs2AudioPause(w.stream, pause as u32) };
            }
            if w.recording && w.input != 0 {
                unsafe { Rs2AudioPause(w.input, pause as u32) };
            }
            let active = if w.recording { MCI_MODE_RECORD } else { MCI_MODE_PLAY };
            d.mode = if pause { MCI_MODE_PAUSE } else { active };
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
            // No name: a new, untitled element.
            let name = r.element.clone();
            let mut nw = Wave::new(name.as_deref(), None)?;
            if name.is_none() {
                nw.data = Some(Vec::new());
            }
            with(id, |d| {
                let w = wave(d);
                nw.volume = w.volume;
                nw.monitor = w.monitor;
                nw.source = w.source;
                nw.fmt = if name.is_none() { w.fmt } else { nw.fmt };
                *w = nw;
                d.element = name;
            });
            Ok(Reply::None)
        }
        MCI_CUE => with(id, |d| {
            let idle = !matches!(d.mode, MCI_MODE_PLAY | MCI_MODE_PAUSE | MCI_MODE_RECORD);
            let w = wave(d);
            if r.flags & MCI_CUE_INPUT != 0 {
                // Without input hardware there is nothing to prepare.
                w.open_input();
            } else if idle && !w.recording && w.input != 0 {
                unsafe { Rs2AudioClose(w.input) };
                w.input = 0;
            }
            Ok(Reply::None)
        })
        .unwrap(),
        MCI_SET => with(id, |d| set(wave(d), r)).unwrap(),
        MCI_STATUS => with(id, |d| {
            let tf = d.time_format;
            let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
            let w = wave(d);
            Ok(match r.item {
                MCI_STATUS_LENGTH => Reply::Num(w.to_time(w.length(), tf)),
                MCI_STATUS_POSITION => Reply::Num(w.to_time(w.position(playing), tf)),
                MCI_STATUS_VOLUME => Reply::Num(w.volume | w.volume << 16),
                MCI_STATUS_MONITOR => Reply::Bool(w.monitor),
                MCI_WAVE_STATUS_LEVEL => Reply::Num(w.level()),
                MCI_WAVE_STATUS_FORMATTAG => Reply::Num(w.fmt.tag),
                MCI_WAVE_STATUS_CHANNELS => Reply::Num(w.fmt.channels),
                MCI_WAVE_STATUS_SAMPLESPERSEC => Reply::Num(w.fmt.rate),
                MCI_WAVE_STATUS_AVGBYTESPERSEC => Reply::Num(w.fmt.avg),
                MCI_WAVE_STATUS_BLOCKALIGN => Reply::Num(w.fmt.block),
                MCI_WAVE_STATUS_BITSPERSAMPLE => Reply::Num(w.fmt.bits),
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
    // The format of an element with sound in it stays.
    let fmt_flags = MCI_WAVE_SET_FORMATTAG | MCI_WAVE_SET_CHANNELS | MCI_WAVE_SET_SAMPLESPERSEC | MCI_WAVE_SET_BITSPERSAMPLE;
    if f & fmt_flags != 0 && w.recording {
        return Err(MCIERR_INVALID_MODE);
    }
    if f & MCI_SET_ITEM != 0 && matches!(r.item, MCI_SET_MONITOR | MCI_SET_MONITOR_AUDIO) {
        if f & (MCI_SET_ON | MCI_SET_OFF) == 0 {
            return Err(MCIERR_MISSING_PARAMETER);
        }
        w.monitor = f & MCI_SET_ON != 0;
    }
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
        // A capture stream opened in the old format goes.
        if w.input != 0 && !w.recording {
            unsafe { Rs2AudioClose(w.input) };
            w.input = 0;
        }
    }
    if f & MCI_SET_AUDIO != 0 && f & MCI_SET_VOLUME != 0 {
        w.volume = r.value.min(100);
        if w.stream != 0 {
            unsafe { Rs2AudioSetVolume(w.stream, w.volume) };
        }
    }
    Ok(Reply::None)
}

/// MCI_CONNECTOR: the wave stream is always on; the microphone and line
/// in are the input's sources (one of them enabled); the outputs are on.
fn connector(w: &mut Wave, r: &Req) -> Result<Reply, u32> {
    let typ = if r.flags & MCI_CONNECTOR_TYPE != 0 { r.item } else { MCI_WAVE_STREAM_CONNECTOR };
    if r.flags & MCI_CONNECTOR_INDEX != 0 && r.value > 1 {
        return Err(MCIERR_INVALID_CONNECTOR_INDEX);
    }
    let source = match typ {
        MCI_MICROPHONE_CONNECTOR => Some(SOURCE_MIC),
        MCI_LINE_IN_CONNECTOR => Some(SOURCE_LINE),
        MCI_WAVE_STREAM_CONNECTOR | MCI_AMP_STREAM_CONNECTOR | MCI_SPEAKERS_CONNECTOR | MCI_HEADPHONES_CONNECTOR | MCI_LINE_OUT_CONNECTOR => None,
        _ => return Err(MCIERR_INVALID_CONNECTOR_TYPE),
    };
    if r.flags & MCI_ENABLE_CONNECTOR != 0 {
        if let Some(src) = source {
            w.source = src;
            let mut taken = 0;
            unsafe { Rs2AudioSetInput(src, &mut taken) };
        }
        return Ok(Reply::None);
    }
    if r.flags & MCI_DISABLE_CONNECTOR != 0 {
        // The wave stream cannot be turned off; nor can every source.
        return match source {
            Some(src) if src != w.source => Ok(Reply::None),
            _ => Err(MCIERR_CANNOT_MODIFY_CONNECTOR),
        };
    }
    if r.flags & MCI_QUERY_CONNECTOR_STATUS != 0 {
        return Ok(Reply::Bool(source.is_none_or(|s| s == w.source)));
    }
    Err(MCIERR_MISSING_FLAG)
}

/// MCI_SAVE: the element (or the playlist's sound so far: not saved) as
/// a WAV file, under the element's name or the one given.
fn save(id: u16, r: &Req) -> Result<Reply, u32> {
    let (name, file) = with(id, |d| {
        if matches!(d.mode, MCI_MODE_PLAY | MCI_MODE_PAUSE | MCI_MODE_RECORD) {
            return Err(MCIERR_INVALID_MODE);
        }
        let name = r.element.clone().or_else(|| d.element.clone()).ok_or(MCIERR_MISSING_PARAMETER)?;
        let w = wave(d);
        if w.data.is_none() {
            return Err(MCIERR_UNSUPPORTED_FUNCTION);
        }
        Ok((name, w.wav()))
    })
    .ok_or(MCIERR_INVALID_DEVICE_ID)??;
    if !write_file(&name, &file) {
        return Err(MCIERR_TARGET_DEVICE_FULL);
    }
    with(id, |d| {
        d.element = Some(name);
        wave(d).dirty = false;
    });
    Ok(Reply::None)
}

fn record(id: u16, r: &Req) -> Result<Reply, u32> {
    if r.flags & MCI_RECORD_INSERT != 0 && r.flags & MCI_RECORD_OVERWRITE != 0 {
        return Err(MCIERR_FLAGS_NOT_COMPATIBLE);
    }
    // A RECORD or PLAY in progress stops (its notification superseded).
    stop(id, false);
    let epoch = with(id, |d| -> Result<u32, u32> {
        let tf = d.time_format;
        d.epoch = d.epoch.wrapping_add(1);
        d.finish(MCI_NOTIFY_SUPERSEDED);
        let w = wave(d);
        if w.data.is_none() && w.playlist.is_none() {
            w.data = Some(Vec::new());
        }
        if r.flags & MCI_FROM != 0 {
            let p = w.from_time(r.from, tf);
            if w.data.is_some() && p > w.length() {
                return Err(MCIERR_OUTOFRANGE);
            }
            w.pos = p;
            if w.playlist.is_some() && r.from == 0 {
                w.proc_.rewind();
            }
        }
        w.pos = w.pos.min(if w.data.is_some() { w.length() } else { u32::MAX });
        w.end = if r.flags & MCI_TO != 0 { w.from_time(r.to, tf) } else { u32::MAX };
        if w.end != u32::MAX && w.end <= w.pos {
            return Err(MCIERR_OUTOFRANGE);
        }
        w.insert = r.flags & MCI_RECORD_INSERT != 0;
        w.paused = false;
        w.recording = true;
        if !w.open_input() {
            trace(format_args!("mdm: no sound input; recording silence"));
        }
        if w.input != 0 {
            // What was cued before now goes.
            unsafe { Rs2AudioStop(w.input) };
        }
        d.mode = MCI_MODE_RECORD;
        d.take_notify(r);
        Ok(d.epoch)
    })
    .ok_or(MCIERR_INVALID_DEVICE_ID)??;
    let param = id as u32 | (epoch & 0xffff) << 16;
    if r.flags & MCI_WAIT != 0 {
        unsafe { recorder(param) };
    } else {
        let mut tid = 0;
        if unsafe { DosCreateThread(&mut tid, recorder, param, 0, 65536) } != 0 {
            with(id, |d| {
                d.mode = MCI_MODE_STOP;
                wave(d).recording = false;
            });
            return Err(MCIERR_OUT_OF_MEMORY);
        }
    }
    Ok(Reply::None)
}

enum RecNext {
    /// Record into the element (up to this many bytes).
    Element(u32),
    /// Record into a playlist DATA buffer: address, room, operand 3.
    Buffer(u32, u32, u32),
    Wait,
    Sem(u32, u32, bool),
    End,
    Stopped,
}

unsafe extern "C" fn recorder(param: u32) {
    let id = param as u16;
    let epoch = param >> 16;
    let Some((fmt, input, monitor)) = with(id, |d| {
        let w = wave(d);
        (w.fmt, w.input, w.monitor)
    }) else {
        return;
    };
    let piece = (fmt.avg * REC_MS / 1000).max(fmt.block) / fmt.block * fmt.block;
    let mut buf = alloc::vec![0u8; piece as usize];
    let mut mon = 0;
    if monitor && unsafe { Rs2AudioOpen(fmt.rate, fmt.channels, fmt.bits, &mut mon) } != 0 {
        mon = 0;
    }
    // Silence ahead of the first piece (and after a pause) on the
    // monitor: the mixer takes 0.1 s at once, and each piece must arrive
    // before the output runs dry.
    let lead = alloc::vec![if fmt.bits == 8 { 0x80u8 } else { 0 }; (piece * 3) as usize];
    let mut relead = true;
    // The playlist DATA buffer being filled: (address, room left,
    // operand 3's address).
    let mut pl: Option<(u32, u32, u32)> = None;
    let mut done = false;
    let mut stopped = false;
    while !done && !stopped {
        let next = with(id, |d| {
            if !current(d, epoch) {
                return RecNext::Stopped;
            }
            let notify_hwnd = d.pending.map_or(0, |n| n.hwnd);
            let did = d.id;
            let w = wave(d);
            if w.paused {
                return RecNext::Wait;
            }
            if w.end != u32::MAX && w.pos >= w.end {
                return RecNext::End;
            }
            let left = if w.end == u32::MAX { u32::MAX } else { w.end - w.pos };
            if w.playlist.is_none() {
                return RecNext::Element(left);
            }
            loop {
                if let Some((a, room, at)) = pl {
                    if room > 0 {
                        return RecNext::Buffer(a, room.min(left), at);
                    }
                    pl = None;
                }
                match w.proc_.next(&mut AppMem) {
                    Step::Data { addr, len, pos_at } => {
                        wr(pos_at, 0, 0);
                        pl = Some((addr, len, pos_at));
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
                    Step::SemWait { sem, timeout, reset } => return RecNext::Sem(sem, timeout, reset),
                    Step::Exit => return RecNext::End,
                }
            }
        })
        .unwrap_or(RecNext::Stopped);
        let want = match next {
            RecNext::Stopped => {
                stopped = true;
                continue;
            }
            RecNext::End => {
                done = true;
                continue;
            }
            RecNext::Wait => {
                relead = true;
                unsafe { DosSleep(30) };
                continue;
            }
            RecNext::Sem(sem, timeout, reset) => {
                unsafe {
                    if reset {
                        let mut n = 0;
                        DosResetEventSem(sem, &mut n);
                    }
                    DosWaitEventSem(sem, timeout);
                }
                continue;
            }
            RecNext::Element(left) => left,
            RecNext::Buffer(_, room, _) => room,
        };
        let want = (want.min(piece) / fmt.block * fmt.block).max(fmt.block.min(want)) as usize;
        let mut n = 0u32;
        if input != 0 {
            unsafe { Rs2AudioRead(input, buf.as_mut_ptr(), want as u32, REC_MS * 5, &mut n) };
        } else {
            // Silence in real time.
            unsafe { DosSleep((want as u64 * 1000 / fmt.avg.max(1) as u64) as u32) };
            buf[..want].fill(if fmt.bits == 8 { 0x80 } else { 0 });
            n = want as u32;
        }
        if n == 0 {
            continue;
        }
        let got = &buf[..n as usize];
        let kept = with(id, |d| {
            if !current(d, epoch) {
                return None;
            }
            let w = wave(d);
            if w.paused {
                return Some(0);
            }
            Some(match next {
                RecNext::Buffer(a, room, at) => {
                    let k = n.min(room);
                    unsafe { core::ptr::copy_nonoverlapping(got.as_ptr(), a as *mut u8, k as usize) };
                    wr(at, 0, rd(at, 0) + k);
                    if let Some((pa, proom, pat)) = pl {
                        pl = Some((pa + k, proom - k, pat));
                    }
                    w.pos += k;
                    k
                }
                _ => w.put(got),
            })
        })
        .flatten();
        match kept {
            None => stopped = true,
            Some(k) if mon != 0 && k > 0 => unsafe {
                if relead {
                    relead = false;
                    Rs2AudioWrite(mon, lead.as_ptr(), lead.len() as u32);
                }
                Rs2AudioWrite(mon, got.as_ptr(), k);
            },
            _ => {}
        }
    }
    let auto_close = with(id, |d| {
        let mine = current(d, epoch);
        let w = wave(d);
        if w.input == input {
            w.input = 0;
            w.recording = false;
        }
        if mine && !stopped {
            d.mode = MCI_MODE_STOP;
            d.finish(MCI_NOTIFY_SUCCESSFUL);
            return d.auto_close;
        }
        false
    })
    .unwrap_or(false);
    if input != 0 {
        unsafe { Rs2AudioClose(input) };
    }
    if mon != 0 {
        unsafe {
            // A STOP silences the monitor at once.
            if stopped {
                Rs2AudioStop(mon);
            } else {
                Rs2AudioDrain(mon, 1000);
            }
            Rs2AudioClose(mon);
        }
    }
    if auto_close {
        let _ = dev::close(id);
    }
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
        if playing && !w.recording {
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
        if playing && !w.recording {
            w.pos = w.position(true);
        }
        if playing || w.recording || w.input != 0 {
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
