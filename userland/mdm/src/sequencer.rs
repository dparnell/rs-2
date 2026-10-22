//! The MIDI sequencer device: a Standard MIDI file (the element) played
//! by crates/midi's software synthesizer, whose output streams through
//! RS2API's audio calls like the wave audio device's. Without sound
//! hardware the kernel paces the stream by the clock, so PLAY WAIT and
//! notifications still take the song's time.

use alloc::vec;

use crate::dev::{self, *};
use crate::req::*;
use crate::sys::*;
use mci::*;

/// Synthesizer rate (the kernel mixer resamples).
const RATE: u32 = 32000;
/// Frames rendered and written at a time (about 64 ms).
const CHUNK: usize = 2048;

pub struct Seq {
    player: Option<midi::Player>,
    /// Where PLAY stops (microseconds; u64::MAX for the end).
    end: u64,
    stream: u32,
    /// The song position (us) when the current stream started, and the
    /// bytes it had played then.
    base_us: u64,
    volume: u32,
    paused: bool,
}

impl Seq {
    pub fn new(element: Option<&str>) -> Result<Seq, u32> {
        let mut s = Seq { player: None, end: u64::MAX, stream: 0, base_us: 0, volume: 100, paused: false };
        if let Some(e) = element {
            s.load(e)?;
        }
        Ok(s)
    }

    pub fn load(&mut self, name: &str) -> Result<(), u32> {
        let data = read_file(name).ok_or(MCIERR_FILE_NOT_FOUND)?;
        let song = midi::parse(&data).map_err(|_| MCIERR_INVALID_MEDIA_TYPE)?;
        self.player = Some(midi::Player::new(song, RATE));
        Ok(())
    }

    fn length_us(&self) -> u64 {
        self.player.as_ref().map_or(0, |p| p.length_us())
    }

    /// The position heard now.
    fn position_us(&self, playing: bool) -> u64 {
        let Some(p) = &self.player else { return 0 };
        if playing && self.stream != 0 {
            let mut bytes = 0;
            unsafe { Rs2AudioQueryPos(self.stream, &mut bytes) };
            return (self.base_us + bytes as u64 * 1_000_000 / (RATE as u64 * 4)).min(p.position_us());
        }
        p.position_us()
    }

    pub fn stop(&mut self) {
        if self.stream != 0 {
            unsafe { Rs2AudioStop(self.stream) };
        }
    }
}

fn to_time(us: u64, f: u32) -> u32 {
    (match f {
        MCI_FORMAT_MILLISECONDS => us / 1000,
        _ => us * 3 / 1000,
    }) as u32
}

fn from_time(t: u32, f: u32) -> u64 {
    match f {
        MCI_FORMAT_MILLISECONDS => t as u64 * 1000,
        _ => t as u64 * 1000 / 3,
    }
}

fn seq(d: &mut Device) -> &mut Seq {
    match &mut d.kind {
        Kind::Seq(s) => s,
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
            let s = seq(d);
            s.paused = pause;
            if s.stream != 0 {
                unsafe { Rs2AudioPause(s.stream, pause as u32) };
            }
            d.mode = if pause { MCI_MODE_PAUSE } else { MCI_MODE_PLAY };
            Ok(Reply::None)
        })
        .unwrap(),
        MCI_SEEK => {
            stop(id, true);
            with(id, |d| {
                let tf = d.time_format;
                let s = seq(d);
                let len = s.length_us();
                let to = if r.flags & MCI_TO_START != 0 {
                    0
                } else if r.flags & MCI_TO_END != 0 {
                    len
                } else if r.flags & MCI_TO != 0 {
                    let t = from_time(r.to, tf);
                    if t > len {
                        return Err(MCIERR_OUTOFRANGE);
                    }
                    t
                } else {
                    return Err(MCIERR_MISSING_PARAMETER);
                };
                if let Some(p) = &mut s.player {
                    p.seek(to);
                }
                Ok(Reply::None)
            })
            .unwrap()
        }
        MCI_LOAD => {
            stop(id, true);
            let name = r.element.clone().ok_or(MCIERR_MISSING_PARAMETER)?;
            let mut n = Seq::new(Some(&name))?;
            with(id, |d| {
                n.volume = seq(d).volume;
                *seq(d) = n;
                d.element = Some(name);
            });
            Ok(Reply::None)
        }
        MCI_CUE => Ok(Reply::None),
        MCI_SET => with(id, |d| {
            let s = seq(d);
            if r.flags & MCI_SET_AUDIO != 0 && r.flags & MCI_SET_VOLUME != 0 {
                s.volume = r.value.min(100);
                if s.stream != 0 {
                    unsafe { Rs2AudioSetVolume(s.stream, s.volume) };
                }
            }
            Ok(Reply::None)
        })
        .unwrap(),
        MCI_STATUS => with(id, |d| {
            let tf = d.time_format;
            let playing = d.mode == MCI_MODE_PLAY || d.mode == MCI_MODE_PAUSE;
            let s = seq(d);
            Ok(match r.item {
                MCI_STATUS_LENGTH => Reply::Num(to_time(s.length_us(), tf)),
                MCI_STATUS_POSITION => Reply::Num(to_time(s.position_us(playing), tf)),
                MCI_STATUS_VOLUME => Reply::Num(s.volume | s.volume << 16),
                // MCI_SEQ_STATUS_TEMPO: beats per minute at the position.
                0x4002 => {
                    let p = s.player.as_ref();
                    let us = p.map_or(500_000, |p| {
                        let tick = p.song.us_to_tick(p.position_us());
                        p.song.tempos.iter().rev().find(|t| t.0 <= tick).map_or(500_000, |t| t.1 as u64)
                    });
                    Reply::Num((60_000_000 / us.max(1)) as u32)
                }
                _ => return Err(MCIERR_INVALID_ITEM_FLAG),
            })
        })
        .unwrap(),
        MCI_GETDEVCAPS => Err(MCIERR_INVALID_ITEM_FLAG),
        _ => Err(MCIERR_UNSUPPORTED_FUNCTION),
    }
}

/// Stop playing; `abort` settles a pending PLAY notification as aborted.
pub fn stop(id: u16, abort: bool) {
    with(id, |d| {
        d.epoch = d.epoch.wrapping_add(1);
        if abort {
            d.finish(MCI_NOTIFY_ABORTED);
        }
        let s = seq(d);
        s.stop();
        if let Some(p) = &mut s.player {
            p.synth.all_notes_off();
        }
        s.paused = false;
        d.mode = MCI_MODE_STOP;
    });
}

fn play(id: u16, r: &Req) -> Result<Reply, u32> {
    let epoch = with(id, |d| -> Result<u32, u32> {
        let tf = d.time_format;
        d.epoch = d.epoch.wrapping_add(1);
        d.finish(MCI_NOTIFY_SUPERSEDED);
        let s = seq(d);
        s.stop();
        let len = s.length_us();
        let Some(p) = &mut s.player else { return Err(MCIERR_MISSING_PARAMETER) };
        if r.flags & MCI_FROM != 0 {
            p.seek(from_time(r.from, tf).min(len));
        } else if p.position_us() >= len {
            p.seek(0);
        } else {
            // Continue from where it stopped (the notes there start again).
            let at = p.position_us();
            p.seek(at);
        }
        s.end = if r.flags & MCI_TO != 0 { from_time(r.to, tf) } else { u64::MAX };
        s.paused = false;
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

unsafe extern "C" fn player(param: u32) {
    let id = param as u16;
    let epoch = param >> 16;
    let mut stream = 0;
    let have_audio = unsafe { Rs2AudioOpen(RATE, 2, 16, &mut stream) } == 0;
    let ok = with(id, |d| {
        if !current(d, epoch) {
            return false;
        }
        let s = seq(d);
        s.stream = if have_audio { stream } else { 0 };
        s.base_us = s.player.as_ref().map_or(0, |p| p.position_us());
        if have_audio {
            unsafe { Rs2AudioSetVolume(stream, s.volume) };
        }
        true
    })
    .unwrap_or(false);
    let mut buf = vec![0i16; CHUNK * 2];
    let mut stopped = !ok;
    let mut done = false;
    while !stopped && !done {
        // Render a chunk under the lock (the player is the device's).
        enum Step {
            Wait,
            Data,
            End,
            Stopped,
        }
        let step = with(id, |d| {
            if !current(d, epoch) {
                return Step::Stopped;
            }
            let s = seq(d);
            if s.paused {
                return Step::Wait;
            }
            let end = s.end;
            let Some(p) = &mut s.player else { return Step::End };
            if p.finished() || p.position_us() >= end {
                return Step::End;
            }
            p.render(&mut buf);
            Step::Data
        })
        .unwrap_or(Step::Stopped);
        match step {
            Step::Stopped => stopped = true,
            Step::End => done = true,
            Step::Wait => unsafe {
                DosSleep(30);
            },
            Step::Data => {
                if have_audio {
                    unsafe { Rs2AudioWrite(stream, buf.as_ptr() as *const u8, (buf.len() * 2) as u32) };
                } else {
                    unsafe { DosSleep((CHUNK as u64 * 1000 / RATE as u64) as u32) };
                }
            }
        }
    }
    // Let the queued sound play out (unless stopped meanwhile; a stream
    // that stands still for 3 s is given up on, as the wave device does).
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
            if still_for >= 3000 {
                unsafe { Rs2AudioStop(stream) };
                break;
            }
        }
    }
    let auto_close = with(id, |d| {
        let mine = current(d, epoch);
        let s = seq(d);
        if mine {
            s.stream = 0;
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
