//! The CD-ROM IOCtls on a CD drive opened with OPEN_FLAGS_DASD ("E:"):
//! category 0x80 (IOCTL_CDROMDISK: device status, raw READ LONG, eject,
//! lock, head location, volume size, UPC) and 0x81 (IOCTL_CDROMAUDIO:
//! play through the drive's audio output, stop, resume, disc and track
//! information, the Q sub-channel, audio status), with the parameter and
//! data packets of IBM's OS2CDROM.DMD.
//!
//! Parameter packets start with the signature "CD01"; addresses are
//! DWORDs in the addressing mode the packet gives: 0 HSG (logical block
//! numbers), 1 Red Book (frame, second, minute, 0 from the low byte up).
//! Track and disc information is in Red Book; Q sub-channel track and
//! index numbers are BCD, as OS2CDROM.DMD reports them.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::ioctl::{put_data, set_lens, Req};
use super::{err, Ctx};
use crate::fs::block::BlockDev;
use crate::fs::cdaudio::{self as cd, FRAME};
use crate::sync::SpinLock;

/// What OS2CDROM.DMD remembers per drive between audio IOCtls: the last
/// play's range (resumed after a stop), whether it is paused, the door
/// lock and the channel control.
#[derive(Clone, Copy)]
struct Audio {
    dev: usize,
    paused: bool,
    start: u32,
    end: u32,
    locked: bool,
    channels: [u8; 8],
}

static AUDIO: SpinLock<Vec<Audio>> = SpinLock::new(Vec::new());

fn state(dev: &Arc<dyn BlockDev>) -> Audio {
    let key = Arc::as_ptr(dev) as *const () as usize;
    let mut a = AUDIO.lock();
    if let Some(s) = a.iter().find(|s| s.dev == key) {
        return *s;
    }
    let s = Audio { dev: key, paused: false, start: 0, end: 0, locked: false, channels: [0, 0xff, 1, 0xff, 2, 0, 3, 0] };
    a.push(s);
    s
}

fn update(dev: &Arc<dyn BlockDev>, f: impl FnOnce(&mut Audio)) {
    let mut s = state(dev);
    f(&mut s);
    let mut a = AUDIO.lock();
    if let Some(x) = a.iter_mut().find(|x| x.dev == s.dev) {
        *x = s;
    }
}

/// An LBA as a Red Book DWORD.
fn red_book(lba: u32) -> u32 {
    let (m, s, f) = cd::msf(lba);
    f as u32 | (s as u32) << 8 | (m as u32) << 16
}

/// An address in the packet's addressing mode as an LBA.
fn address(mode: u8, v: u32) -> Result<u32, u32> {
    match mode {
        0 => Ok(v),
        1 => cd::lba((v >> 16) as u8, (v >> 8) as u8, v as u8).ok_or(err::INVALID_PARAMETER),
        _ => Err(err::INVALID_PARAMETER),
    }
}

fn bcd(v: u8) -> u8 {
    (v / 10) << 4 | v % 10
}

fn le32(p: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]])
}

fn playing(dev: &dyn BlockDev) -> bool {
    cd::position(dev).is_ok_and(|p| p.audio == cd::AUDIO_PLAYING)
}

pub(super) fn ioctl(c: &Ctx, dev: &Arc<dyn BlockDev>, r: &Req) -> u32 {
    // The longest parameter packet: PLAY AUDIO's 13 bytes.
    let plen = match (r.category, r.function) {
        (0x80, 0x46 | 0x70) | (0x81, 0x62) => 5,
        (0x80, 0x50) => 9,
        (0x80, 0x72) => 13,
        (0x81, 0x50) => 13,
        _ => 4,
    };
    let Some(p) = c.read_vec(r.params, plen) else { return err::INVALID_ADDRESS };
    let d: &dyn BlockDev = &**dev;
    let res: Result<Vec<u8>, u32> = match (r.category, r.function) {
        // Reset drive.
        (0x80, 0x40) => Ok(Vec::new()),
        // Eject; close the tray.
        (0x80, 0x44 | 0x45) => cd::eject(d, r.function == 0x44).map(|_| Vec::new()),
        // Lock or unlock the door.
        (0x80, 0x46) => cd::lock(d, p[4] & 1 != 0).map(|_| {
            update(dev, |s| s.locked = p[4] & 1 != 0);
            Vec::new()
        }),
        (0x80, 0x50) => address(p[4], le32(&p, 5)).and_then(|a| cd::seek(d, a)).map(|_| Vec::new()),
        // Device status: door unlocked (bit 1), cooked and raw (2), data
        // and audio (4), audio channel control (8), HSG and Red Book (9);
        // audio playing (12); no disc (11) with the door open (0).
        (0x80, 0x60) => {
            let mut st = 0x314u32;
            if !state(dev).locked {
                st |= 0x2;
            }
            if !cd::ready(d) {
                st |= 0x801;
            } else if playing(d) {
                st |= 0x1000;
            }
            Ok(st.to_le_bytes().to_vec())
        }
        // Driver identification.
        (0x80, 0x61) => Ok(b"CD01".to_vec()),
        (0x80, 0x63) => Ok(2048u16.to_le_bytes().to_vec()),
        // Head location, in the requested addressing mode.
        (0x80, 0x70) => cd::position(d).and_then(|q| match p[4] {
            0 => Ok(q.abs.to_le_bytes().to_vec()),
            1 => Ok(red_book(q.abs).to_le_bytes().to_vec()),
            _ => Err(err::INVALID_PARAMETER),
        }),
        // READ LONG: (mode, count, start) -> count raw 2352-byte frames.
        (0x80, 0x72) => {
            let n = u16::from_le_bytes([p[5], p[6]]) as usize;
            let start = match address(p[4], le32(&p, 7)) {
                Ok(a) => a,
                Err(e) => return e,
            };
            if (n * FRAME) as u32 > r.data_len {
                return err::BUFFER_OVERFLOW;
            }
            let mut buf = alloc::vec![0u8; n * FRAME];
            cd::read_frames(d, start, &mut buf).map(|_| buf)
        }
        // Volume size: the lead-out (or the data capacity).
        (0x80, 0x78) => Ok(cd::read_toc(d).map_or(d.sector_count() as u32, |t| t.leadout).to_le_bytes().to_vec()),
        // UPC: control/ADR, 7 BCD bytes (13 digits), 0, frame.
        (0x80, 0x79) => cd::catalog_number(d).ok_or(err::NOT_SUPPORTED).map(|mcn| {
            let mut v = alloc::vec![0u8; 10];
            v[0] = 0x02;
            for (i, ch) in mcn.iter().enumerate() {
                v[1 + i / 2] |= (ch & 0xf) << if i % 2 == 0 { 4 } else { 0 };
            }
            v
        }),
        (0x81, 0x40) => {
            let mut ch = [0u8; 8];
            if !c.read(r.data, &mut ch) {
                return err::INVALID_ADDRESS;
            }
            update(dev, |s| s.channels = ch);
            set_lens(c, r, 4, 8);
            return 0;
        }
        (0x81, 0x50) => {
            let (start, end) = match (address(p[4], le32(&p, 5)), address(p[4], le32(&p, 9))) {
                (Ok(s), Ok(e)) => (s, e),
                _ => return err::INVALID_PARAMETER,
            };
            // Up to the end of the disc: one frame short of the lead-out,
            // which some drives refuse to play to.
            let end = match cd::read_toc(d) {
                Ok(t) if end >= t.leadout && t.leadout > 0 => t.leadout - 1,
                _ => end,
            };
            if end <= start {
                return err::INVALID_PARAMETER;
            }
            cd::play(d, start, end).map(|_| {
                update(dev, |s| {
                    s.paused = false;
                    s.start = start;
                    s.end = end;
                });
                Vec::new()
            })
        }
        // Stop: pause, keeping the place for RESUME.
        (0x81, 0x51) => {
            if !playing(d) {
                Ok(Vec::new())
            } else {
                cd::pause(d, true).map(|_| {
                    let at = cd::position(d).map(|q| q.abs).ok();
                    update(dev, |s| {
                        s.paused = true;
                        if let Some(a) = at {
                            s.start = a;
                        }
                    });
                    Vec::new()
                })
            }
        }
        (0x81, 0x52) => {
            let s = state(dev);
            if !s.paused {
                return err::INVALID_PARAMETER;
            }
            cd::play(d, s.start, s.end).map(|_| {
                update(dev, |s| s.paused = false);
                Vec::new()
            })
        }
        (0x81, 0x60) => Ok(state(dev).channels.to_vec()),
        // Disc: first and last track, lead-out (Red Book).
        (0x81, 0x61) => cd::read_toc(d).map(|t| {
            let mut v = alloc::vec![t.first, t.last];
            v.extend_from_slice(&red_book(t.leadout).to_le_bytes());
            v
        }),
        // Track: start (Red Book), control (high nibble) and ADR.
        (0x81, 0x62) => cd::read_toc(d).and_then(|t| {
            let tr = t.track(p[4]).ok_or(err::INVALID_PARAMETER)?;
            let mut v = red_book(tr.lba).to_le_bytes().to_vec();
            v.push(tr.adr_ctrl << 4 | tr.adr_ctrl >> 4);
            Ok(v)
        }),
        // Q sub-channel: control/ADR, track and index (BCD), the time in
        // the track (M, S, F), 0, the time on the disc (M, S, F).
        (0x81, 0x63) => cd::position(d).map(|q| {
            // In the pause before a track the time counts down to it.
            let f = q.rel.unsigned_abs();
            let abs = cd::msf(q.abs);
            alloc::vec![
                q.adr_ctrl << 4 | q.adr_ctrl >> 4,
                bcd(q.track),
                bcd(q.index),
                (f / 4500).min(255) as u8,
                (f / 75 % 60) as u8,
                (f % 75) as u8,
                0,
                abs.0,
                abs.1,
                abs.2,
            ]
        }),
        // Audio status: paused (bit 0), the last play's start and end.
        (0x81, 0x65) => {
            let s = state(dev);
            let mut v = (s.paused as u16).to_le_bytes().to_vec();
            v.extend_from_slice(&red_book(s.start).to_le_bytes());
            v.extend_from_slice(&red_book(s.end).to_le_bytes());
            Ok(v)
        }
        _ => return err::INVALID_FUNCTION,
    };
    match res {
        Ok(v) => put_data(c, r, plen as u32, &v),
        Err(e) => e,
    }
}
