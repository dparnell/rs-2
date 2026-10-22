//! The CD audio device: an audio CD in the first CD drive (or the n-th
//! for "cdaudio0n"), played digitally. The drive is opened as a volume
//! (OPEN_FLAGS_DASD) and driven through OS/2's CD-ROM IOCtls: the disc
//! and track information of category 0x81 for the table of contents,
//! device status for the disc, READ LONG (category 0x80) for the audio
//! frames, which go to RS2API's audio streams as 44.1 kHz 16-bit stereo,
//! like the wave audio device's samples. Playing through the drive's own
//! audio output (CDROMAUDIO_PLAYAUDIO) is not used: virtual machines and
//! most computers no longer wire it to the sound card.
//!
//! Times follow crates/mci's `cd` module; data tracks are not played.

use alloc::vec::Vec;

use crate::dev::{self, *};
use crate::req::*;
use crate::sys::*;
use mci::cd::{self, Toc, Track};
use mci::*;

pub const MCI_TRACK: u32 = 0x20;
pub const MCI_STATUS_START: u32 = 0x200;
pub const MCI_STATUS_CURRENT_TRACK: u32 = 1;
pub const MCI_STATUS_POSITION_IN_TRACK: u32 = 6;
pub const MCI_CD_STATUS_TRACK_TYPE: u32 = 0x2000;
pub const MCI_CD_STATUS_TRACK_COPYPERMITTED: u32 = 0x2001;
pub const MCI_CD_STATUS_TRACK_CHANNELS: u32 = 0x2002;
pub const MCI_CD_STATUS_TRACK_PREEMPHASIS: u32 = 0x2003;
pub const MCI_SET_DOOR_OPEN: u32 = 0x2000;
pub const MCI_SET_DOOR_CLOSED: u32 = 0x4000;
pub const MCI_SET_DOOR_LOCK: u32 = 0x2_0000;
pub const MCI_SET_DOOR_UNLOCK: u32 = 0x4_0000;
pub const MCI_CD_INFO_ID: u32 = 0x1_0000;
pub const MCI_CD_INFO_UPC: u32 = 0x2_0000;
pub const MCI_GETDEVCAPS_PREROLL_TYPE: u32 = 14;
pub const MCI_GETDEVCAPS_PREROLL_TIME: u32 = 13;

/// Frames read and handed to the stream at a time (0.32 s).
const FRAMES: u32 = 24;
const FRAME: usize = 2352;
/// CD audio: 44 100 Hz, 16-bit stereo.
const BYTES_PER_SECOND: u64 = 44100 * 4;

pub struct Cd {
    /// The drive opened as a volume.
    h: u32,
    toc: Option<Toc>,
    /// The position (Red Book address): where PLAY starts without FROM.
    pos: u32,
    /// Where the current PLAY stops.
    end: u32,
    stream: u32,
    /// The address the stream's first byte came from.
    base: u32,
    volume: u32,
    paused: bool,
}

// ---- The drive ----

/// The CD drives (letters), in order.
fn cd_drives() -> Vec<u8> {
    let mut out = Vec::new();
    for d in b'C'..=b'Z' {
        let name = [d, b':', 0];
        let mut buf = [0u8; 64];
        let mut cb = buf.len() as u32;
        if unsafe { DosQueryFSAttach(name.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } != 0 {
            continue;
        }
        // FSQBUFFER2: type, cbName, cbFSDName, cbFSAData, name, FSD name.
        let cb_name = u16::from_le_bytes([buf[2], buf[3]]) as usize;
        let cb_fsd = u16::from_le_bytes([buf[4], buf[5]]) as usize;
        let fsd = buf.get(8 + cb_name + 1..8 + cb_name + 1 + cb_fsd).unwrap_or(&[]);
        if fsd.eq_ignore_ascii_case(b"CDFS") {
            out.push(d);
        }
    }
    out
}

/// A CD-ROM IOCtl: Ok(data length).
fn ioctl(h: u32, cat: u32, func: u32, params: &[u8], data: &mut [u8]) -> Result<u32, u32> {
    let mut p = Vec::from(b"CD01".as_slice());
    p.extend_from_slice(params);
    let (mut pl, mut dl) = (p.len() as u32, data.len() as u32);
    let rc = unsafe { DosDevIOCtl(h, cat, func, p.as_mut_ptr(), pl, &mut pl, data.as_mut_ptr(), dl, &mut dl) };
    if rc == 0 { Ok(dl) } else { Err(rc) }
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// A Red Book DWORD (frame, second, minute) as an address.
fn red_book(v: u32) -> u32 {
    (v >> 16 & 0xff) * 4500 + (v >> 8 & 0xff) * 75 + (v & 0xff)
}

impl Cd {
    /// The `ordinal`-th CD drive (0 or 1: the first).
    pub fn new(ordinal: u16) -> Result<Cd, u32> {
        let drives = cd_drives();
        let d = *drives.get(ordinal.max(1) as usize - 1).ok_or(MCIERR_DEVICE_NOT_FOUND)?;
        let name = [d, b':', 0];
        let (mut h, mut act) = (0u32, 0u32);
        // OPEN_FLAGS_DASD | OPEN_FLAGS_FAIL_ON_ERROR, deny none, read only.
        if unsafe { DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, 1, 0xa040, core::ptr::null_mut()) } != 0 {
            return Err(MCIERR_DEVICE_NOT_FOUND);
        }
        let mut c = Cd { h, toc: None, pos: cd::LEAD_IN, end: 0, stream: 0, base: 0, volume: 100, paused: false };
        if c.refresh() {
            c.pos = c.toc.as_ref().map_or(cd::LEAD_IN, Toc::start);
        }
        Ok(c)
    }

    pub fn close(&mut self) {
        self.stop();
        unsafe { DosClose(self.h) };
    }

    pub fn stop(&mut self) {
        if self.stream != 0 {
            unsafe { Rs2AudioStop(self.stream) };
        }
    }

    /// Whether a disc is in the drive (device status bit 11 clear).
    fn disc(&self) -> bool {
        let mut st = [0u8; 4];
        ioctl(self.h, 0x80, 0x60, &[], &mut st).is_ok_and(|_| le32(&st) & 0x800 == 0)
    }

    /// Read the table of contents again unless it is the same disc: true
    /// if a disc with tracks is there. A new disc starts at its start.
    fn refresh(&mut self) -> bool {
        if !self.disc() {
            self.toc = None;
            return false;
        }
        let mut di = [0u8; 6];
        if ioctl(self.h, 0x81, 0x61, &[], &mut di).is_err() {
            self.toc = None;
            return false;
        }
        let leadout = red_book(le32(&di[2..]));
        if self.toc.as_ref().is_some_and(|t| t.leadout == leadout && t.first() == di[0] && t.last() == di[1]) {
            return true;
        }
        let mut toc = Toc { tracks: Vec::new(), leadout };
        for n in di[0]..=di[1] {
            let mut ti = [0u8; 5];
            if ioctl(self.h, 0x81, 0x62, &[n], &mut ti).is_err() {
                continue;
            }
            // Byte 4: CONTROL (high nibble) and ADR.
            let control = ti[4] >> 4;
            toc.tracks.push(Track { number: n, start: red_book(le32(&ti)), data: control & 4 != 0, control });
        }
        let ok = !toc.tracks.is_empty();
        self.pos = toc.start();
        self.toc = ok.then_some(toc);
        ok
    }

    /// Where playing has got to.
    fn position(&self, playing: bool) -> u32 {
        if playing && self.stream != 0 {
            let mut n = 0;
            unsafe { Rs2AudioQueryPos(self.stream, &mut n) };
            return (self.base + n / FRAME as u32).min(self.end.max(self.base));
        }
        self.pos
    }

    /// Where a play from `a` must stop: the next data track, if any.
    fn audio_end(&self, a: u32, end: u32) -> u32 {
        let Some(t) = &self.toc else { return a };
        t.tracks.iter().filter(|x| x.data && x.start > a).map(|x| x.start).min().unwrap_or(end).min(end).min(t.leadout)
    }
}

fn cdd(d: &mut Device) -> &mut Cd {
    match &mut d.kind {
        Kind::Cd(c) => c,
        _ => unreachable!(),
    }
}

/// A time in the device's format: a number, or for the string interface
/// "mm:ss:ff" / "tt:mm:ss:ff".
fn time(r: &Req, v: u32, tf: u32, length: bool) -> Reply {
    if r.from_string && (tf == cd::MCI_FORMAT_MSF || tf == cd::MCI_FORMAT_TMSF) {
        Reply::Text(cd::time_text(v, tf, length))
    } else {
        Reply::Num(v)
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
            let c = cdd(d);
            c.paused = pause;
            if c.stream != 0 {
                unsafe { Rs2AudioPause(c.stream, pause as u32) };
            }
            d.mode = if pause { MCI_MODE_PAUSE } else { MCI_MODE_PLAY };
            Ok(Reply::None)
        })
        .unwrap(),
        MCI_SEEK => {
            stop(id, true);
            with(id, |d| {
                let tf = d.time_format;
                let c = cdd(d);
                if !c.refresh() {
                    return Err(MCIERR_DEVICE_NOT_READY);
                }
                let toc = c.toc.as_ref().unwrap();
                c.pos = if r.flags & MCI_TO_START != 0 {
                    toc.start()
                } else if r.flags & MCI_TO_END != 0 {
                    toc.leadout
                } else if r.flags & MCI_TO != 0 {
                    match cd::position_from(toc, r.to, tf) {
                        Some(a) if a <= toc.leadout => a,
                        _ => return Err(MCIERR_OUTOFRANGE),
                    }
                } else {
                    return Err(MCIERR_MISSING_PARAMETER);
                };
                Ok(Reply::None)
            })
            .unwrap()
        }
        MCI_SET => with(id, |d| {
            let c = cdd(d);
            let f = r.flags;
            for (flag, cat, func, p) in [(MCI_SET_DOOR_OPEN, 0x80, 0x44, None), (MCI_SET_DOOR_CLOSED, 0x80, 0x45, None), (MCI_SET_DOOR_LOCK, 0x80, 0x46, Some(1u8)), (MCI_SET_DOOR_UNLOCK, 0x80, 0x46, Some(0))] {
                if f & flag != 0 {
                    let p: &[u8] = match &p {
                        Some(b) => core::slice::from_ref(b),
                        None => &[],
                    };
                    ioctl(c.h, cat, func, p, &mut []).map_err(|_| MCIERR_HARDWARE)?;
                }
            }
            if f & MCI_SET_AUDIO != 0 && f & MCI_SET_VOLUME != 0 {
                c.volume = r.value.min(100);
                if c.stream != 0 {
                    unsafe { Rs2AudioSetVolume(c.stream, c.volume) };
                }
            }
            Ok(Reply::None)
        })
        .unwrap(),
        MCI_STATUS => status(id, r),
        MCI_GETDEVCAPS => Ok(match r.item {
            MCI_GETDEVCAPS_CAN_PLAY | MCI_GETDEVCAPS_CAN_EJECT | MCI_GETDEVCAPS_CAN_LOCKEJECT | MCI_GETDEVCAPS_CAN_SETVOLUME | MCI_GETDEVCAPS_HAS_AUDIO | MCI_GETDEVCAPS_CAN_STREAM => Reply::Bool(true),
            MCI_GETDEVCAPS_CAN_RECORD
            | MCI_GETDEVCAPS_CAN_RECORD_INSERT
            | MCI_GETDEVCAPS_CAN_SAVE
            | MCI_GETDEVCAPS_USES_FILES
            | MCI_GETDEVCAPS_HAS_VIDEO
            | MCI_GETDEVCAPS_HAS_IMAGE
            | MCI_GETDEVCAPS_CAN_PROCESS_INTERNAL => Reply::Bool(false),
            MCI_GETDEVCAPS_DEVICE_TYPE => Reply::Named(DEVTYPE_CD_AUDIO as u32, "cdaudio"),
            // MCI_PREROLL_NONE.
            MCI_GETDEVCAPS_PREROLL_TYPE => Reply::Num(3),
            MCI_GETDEVCAPS_PREROLL_TIME => Reply::Num(0),
            _ => return Err(MCIERR_INVALID_ITEM_FLAG),
        }),
        MCI_INFO => with(id, |d| {
            let c = cdd(d);
            if !c.refresh() {
                return Err(MCIERR_DEVICE_NOT_READY);
            }
            if r.flags & MCI_CD_INFO_UPC != 0 {
                let mut u = [0u8; 10];
                ioctl(c.h, 0x80, 0x79, &[], &mut u).map_err(|_| MCIERR_UNSUPPORTED_FUNCTION)?;
                let digits: alloc::string::String = u[1..8].iter().flat_map(|b| [b >> 4, b & 0xf]).take(13).map(|n| (b'0' + n) as char).collect();
                return Ok(Reply::Text(digits));
            }
            // The disc's ID: the number of tracks and where they start,
            // and the lead-out, as hex digits.
            let t = c.toc.as_ref().unwrap();
            let mut s = alloc::format!("{:02X}{:06X}", t.tracks.len(), t.leadout);
            for x in &t.tracks {
                s.push_str(&alloc::format!("{:06X}", x.start));
            }
            Ok(Reply::Text(s))
        })
        .unwrap(),
        MCI_CUE | MCI_LOAD => Ok(Reply::None),
        _ => Err(MCIERR_UNSUPPORTED_FUNCTION),
    }
}

fn status(id: u16, r: &Req) -> Result<Reply, u32> {
    with(id, |d| {
        let tf = d.time_format;
        let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
        let mode = d.mode;
        let c = cdd(d);
        // While playing, the disc is the one being played.
        let ready = if playing { c.toc.is_some() } else { c.refresh() };
        match r.item {
            MCI_STATUS_MEDIA_PRESENT => return Ok(Reply::Bool(ready)),
            MCI_STATUS_READY => return Ok(Reply::Bool(ready)),
            MCI_STATUS_MODE => {
                let m = if ready { mode } else { MCI_MODE_NOT_READY };
                return Ok(Reply::Named(m, mode_name(m)));
            }
            MCI_STATUS_VOLUME => return Ok(Reply::Num(c.volume | c.volume << 16)),
            _ => {}
        }
        let Some(toc) = c.toc.as_ref().filter(|_| ready) else { return Err(MCIERR_DEVICE_NOT_READY) };
        let track = |n: u32| toc.track(n as u8).ok_or(MCIERR_OUTOFRANGE);
        let by_track = r.flags & MCI_TRACK != 0;
        let pos = c.position(playing);
        Ok(match r.item {
            MCI_STATUS_NUMBER_OF_TRACKS => Reply::Num(toc.tracks.len() as u32),
            MCI_STATUS_LENGTH if by_track => {
                let t = track(r.value)?;
                let end = toc.track_end(t.number).unwrap();
                time(r, cd::length_to(end - t.start, tf), tf, true)
            }
            MCI_STATUS_LENGTH => time(r, cd::length_to(toc.leadout - toc.tracks[0].start, tf), tf, true),
            MCI_STATUS_POSITION if by_track => time(r, cd::position_to(toc, track(r.value)?.start, tf), tf, false),
            MCI_STATUS_POSITION if r.flags & MCI_STATUS_START != 0 => time(r, cd::position_to(toc, toc.tracks[0].start, tf), tf, false),
            MCI_STATUS_POSITION => time(r, cd::position_to(toc, pos, tf), tf, false),
            MCI_STATUS_CURRENT_TRACK => Reply::Num(toc.track_at(pos).map_or(0, |t| t.number as u32)),
            MCI_STATUS_POSITION_IN_TRACK => {
                let t = toc.track_at(pos).unwrap();
                time(r, cd::length_to(pos.saturating_sub(t.start), tf), tf, true)
            }
            MCI_CD_STATUS_TRACK_TYPE => {
                let t = track(if by_track { r.value } else { toc.track_at(pos).map_or(0, |t| t.number as u32) })?;
                let (v, name) = if t.data { (2, "data") } else { (1, "audio") };
                Reply::Named(v, name)
            }
            MCI_CD_STATUS_TRACK_COPYPERMITTED | MCI_CD_STATUS_TRACK_CHANNELS | MCI_CD_STATUS_TRACK_PREEMPHASIS => {
                let t = track(if by_track { r.value } else { toc.track_at(pos).map_or(0, |t| t.number as u32) })?;
                match r.item {
                    MCI_CD_STATUS_TRACK_COPYPERMITTED => Reply::Bool(t.control & 2 != 0),
                    MCI_CD_STATUS_TRACK_CHANNELS => Reply::Num(if t.control & 8 != 0 { 4 } else { 2 }),
                    _ => Reply::Bool(t.control & 1 != 0),
                }
            }
            _ => return Err(MCIERR_INVALID_ITEM_FLAG),
        })
    })
    .unwrap()
}

/// Stop playing; `abort` settles a pending PLAY notification as aborted.
pub fn stop(id: u16, abort: bool) {
    with(id, |d| {
        d.epoch = d.epoch.wrapping_add(1);
        if abort {
            d.finish(MCI_NOTIFY_ABORTED);
        }
        let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
        let c = cdd(d);
        if playing {
            c.pos = c.position(true);
        }
        c.stop();
        c.paused = false;
        d.mode = MCI_MODE_STOP;
    });
}

fn play(id: u16, r: &Req) -> Result<Reply, u32> {
    let epoch = with(id, |d| -> Result<u32, u32> {
        let tf = d.time_format;
        d.epoch = d.epoch.wrapping_add(1);
        d.finish(MCI_NOTIFY_SUPERSEDED);
        let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
        d.mode = MCI_MODE_STOP;
        let c = cdd(d);
        if playing {
            c.pos = c.position(true);
            c.stop();
        }
        if !c.refresh() {
            return Err(MCIERR_DEVICE_NOT_READY);
        }
        let toc = c.toc.as_ref().unwrap();
        if !toc.has_audio() {
            return Err(MCIERR_INVALID_MEDIA_TYPE);
        }
        let leadout = toc.leadout;
        let at = |v| cd::position_from(toc, v, tf).filter(|&a| a <= leadout).ok_or(MCIERR_OUTOFRANGE);
        let mut from = if r.flags & MCI_FROM != 0 { at(r.from)? } else { c.pos };
        let to = if r.flags & MCI_TO != 0 { at(r.to)? } else { leadout };
        if from >= leadout && r.flags & MCI_FROM == 0 {
            from = toc.start();
        }
        // From a data track: from the next audio track.
        if let Some(t) = toc.track_at(from).filter(|t| t.data) {
            from = toc.tracks.iter().find(|x| x.number > t.number && !x.data).map_or(leadout, |x| x.start);
        }
        if to < from {
            return Err(MCIERR_OUTOFRANGE);
        }
        c.pos = from;
        c.end = c.audio_end(from, to);
        c.paused = false;
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

fn current(d: &Device, epoch: u32) -> bool {
    d.epoch & 0xffff == epoch
}

enum Next {
    /// Frames to read: (drive handle, address, count).
    Read(u32, u32, u32),
    Wait,
    End,
    Stopped,
}

unsafe extern "C" fn player(param: u32) {
    let id = param as u16;
    let epoch = param >> 16;
    let mut stream = 0;
    let have_audio = unsafe { Rs2AudioOpen(44100, 2, 16, &mut stream) } == 0;
    let ok = with(id, |d| {
        if !current(d, epoch) {
            return false;
        }
        let c = cdd(d);
        c.stream = if have_audio { stream } else { 0 };
        c.base = c.pos;
        if have_audio {
            unsafe { Rs2AudioSetVolume(stream, c.volume) };
        }
        true
    })
    .unwrap_or(false);
    let mut buf = alloc::vec![0u8; FRAMES as usize * FRAME];
    let (mut done, mut stopped, mut failed) = (false, !ok, false);
    while !stopped && !done {
        let next = with(id, |d| {
            if !current(d, epoch) {
                return Next::Stopped;
            }
            let c = cdd(d);
            if c.paused {
                return Next::Wait;
            }
            if c.pos >= c.end {
                return Next::End;
            }
            let n = (c.end - c.pos).min(FRAMES);
            let at = c.pos;
            c.pos += n;
            Next::Read(c.h, at, n)
        })
        .unwrap_or(Next::Stopped);
        match next {
            Next::Stopped => stopped = true,
            Next::End => done = true,
            Next::Wait => unsafe {
                DosSleep(30);
            },
            Next::Read(h, at, n) => {
                let bytes = n as usize * FRAME;
                if have_audio {
                    // READ LONG, HSG addressing: (mode, count, start).
                    let lba = at.saturating_sub(cd::LEAD_IN);
                    let mut p = alloc::vec![0u8];
                    p.extend_from_slice(&(n as u16).to_le_bytes());
                    p.extend_from_slice(&lba.to_le_bytes());
                    p.extend_from_slice(&[0, 0, 0]);
                    if ioctl(h, 0x80, 0x72, &p, &mut buf[..bytes]).is_err() {
                        failed = true;
                        done = true;
                        continue;
                    }
                    unsafe { Rs2AudioWrite(stream, buf.as_ptr(), bytes as u32) };
                } else {
                    // Real time without sound hardware.
                    unsafe { DosSleep((bytes as u64 * 1000 / BYTES_PER_SECOND) as u32) };
                }
            }
        }
    }
    const STUCK_MS: u32 = 3000;
    if done && have_audio && !failed {
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
        let c = cdd(d);
        if mine {
            if have_audio && !stopped {
                // Where the sound got to (a read error ends it early).
                c.pos = c.position(true);
            }
            c.stream = 0;
        }
        if mine && !stopped {
            d.mode = MCI_MODE_STOP;
            // MCI_NOTIFY_ERROR after a read error.
            d.finish(if failed { 3 } else { MCI_NOTIFY_SUCCESSFUL });
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
