//! I/O procedures: the built-in DOS (files), MEM (memory files) and WAVE
//! (RIFF WAVE audio) procedures, and the per-process list that
//! mmioInstallIOProc keeps.
//!
//! The DOS procedure keeps the file handle in aulInfo[0] (on open, a
//! non-zero aulInfo[0] with no file name is a handle to use). The WAVE
//! procedure keeps its instance in aulInfo[1]: the DOS file under it, the
//! format and where the sample data is; its positions are offsets in the
//! sample data.

use alloc::boxed::Box;
use alloc::vec::Vec;

use mmfmt::wav::WaveFormat;
use os2::*;

use crate::*;

const MMIO_INSTALLPROC: u32 = 1;
const MMIO_REMOVEPROC: u32 = 2;
const MMIO_FINDPROC: u32 = 4;

/// Procedures installed by this process: (FOURCC, procedure), newest last.
static INSTALLED: Locked<Vec<(u32, usize)>> = Locked::new(Vec::new());

fn builtin(fcc: u32) -> Option<usize> {
    Some(match fcc {
        FOURCC_DOS => crate::exports::__a_DOSIOProc as *const () as usize,
        FOURCC_MEM => crate::exports::__a_MEMIOProc as *const () as usize,
        FOURCC_WAVE => crate::exports::__a_WAVEIOProc as *const () as usize,
        _ => return None,
    })
}

/// The procedure for `fcc`: the newest installed one, else a built-in.
pub fn find(fcc: u32) -> Option<usize> {
    INSTALLED.with(|l| l.iter().rev().find(|e| e.0 == fcc).map(|e| e.1)).or_else(|| builtin(fcc))
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioInstallIOProc(fcc: u32, proc_: usize, flags: u32) -> usize {
    match flags & (MMIO_INSTALLPROC | MMIO_REMOVEPROC | MMIO_FINDPROC) {
        MMIO_INSTALLPROC if proc_ != 0 && fcc != 0 => {
            INSTALLED.with(|l| l.push((fcc, proc_)));
            proc_
        }
        MMIO_REMOVEPROC => INSTALLED.with(|l| match l.iter().rposition(|e| e.0 == fcc) {
            Some(i) => l.remove(i).1,
            None => 0,
        }),
        MMIO_FINDPROC => find(fcc).unwrap_or(0),
        _ => 0,
    }
}

/// The file-format procedure that recognises file `name` (installed
/// procedures first, then WAVE), by MMIOM_IDENTIFYFILE.
pub fn identify(name: *const u8) -> Option<u32> {
    let installed: Vec<(u32, usize)> = INSTALLED.with(|l| l.clone());
    for &(fcc, p) in installed.iter().rev() {
        if fcc == FOURCC_DOS || fcc == FOURCC_MEM {
            continue;
        }
        let f: IoProc = unsafe { core::mem::transmute(p) };
        if unsafe { f(core::ptr::null_mut(), MMIOM_IDENTIFYFILE, name as i32, 0) } == 0 {
            return Some(fcc);
        }
    }
    (wave_identify(name, 0) == 0).then_some(FOURCC_WAVE)
}

/// MMFORMATINFO.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FormatInfo {
    pub ulStructLen: u32,
    pub fccIOProc: u32,
    pub ulIOProcType: u32,
    pub ulMediaType: u32,
    pub ulFlags: u32,
    pub szDefaultFormatExt: [u8; 5],
    pub ulCodePage: u32,
    pub ulLanguage: u32,
    pub lNameLength: i32,
}

pub const MMIO_IOPROC_STORAGESYSTEM: u32 = 1;
pub const MMIO_IOPROC_FILEFORMAT: u32 = 2;
pub const MMIO_MEDIATYPE_AUDIO: u32 = 2;
pub const MMIO_MEDIATYPE_UNKNOWN: u32 = 0x20;

pub const WAVE_NAME: &[u8] = b"RIFF WAVE Digital Audio";

/// The format information of a built-in procedure.
pub fn format_info(fcc: u32) -> Option<FormatInfo> {
    let (kind, media, flags, ext, name): (u32, u32, u32, &[u8], &[u8]) = match fcc {
        FOURCC_WAVE => (MMIO_IOPROC_FILEFORMAT, MMIO_MEDIATYPE_AUDIO, 0xfff, b"WAV", WAVE_NAME),
        FOURCC_DOS => (MMIO_IOPROC_STORAGESYSTEM, MMIO_MEDIATYPE_UNKNOWN, 0, b"", b"DOS"),
        FOURCC_MEM => (MMIO_IOPROC_STORAGESYSTEM, MMIO_MEDIATYPE_UNKNOWN, 0, b"", b"MEM"),
        _ => return None,
    };
    let mut e = [0u8; 5];
    e[..ext.len()].copy_from_slice(ext);
    Some(FormatInfo {
        ulStructLen: core::mem::size_of::<FormatInfo>() as u32,
        fccIOProc: fcc,
        ulIOProcType: kind,
        ulMediaType: media,
        ulFlags: flags,
        szDefaultFormatExt: e,
        ulCodePage: 437,
        ulLanguage: 9,
        lNameLength: name.len() as i32,
    })
}

pub fn format_name(fcc: u32) -> &'static [u8] {
    match fcc {
        FOURCC_WAVE => WAVE_NAME,
        FOURCC_DOS => b"DOS",
        FOURCC_MEM => b"MEM",
        _ => b"",
    }
}

/// GETFORMATINFO and GETFORMATNAME for a built-in procedure.
fn format_msg(fcc: u32, msg: u32, p1: i32, p2: i32) -> i32 {
    match msg {
        MMIOM_GETFORMATINFO => {
            let Some(out) = (unsafe { (p1 as *mut FormatInfo).as_mut() }) else { return -1 };
            *out = format_info(fcc).unwrap();
            0
        }
        _ => {
            let name = format_name(fcc);
            let n = name.len().min(p2.max(0) as usize);
            if p1 != 0 {
                unsafe { core::ptr::copy(name.as_ptr(), p1 as *mut u8, n) };
            }
            n as i32
        }
    }
}

fn unsupported(info: *mut MmioInfo) -> i32 {
    if let Some(i) = unsafe { info.as_mut() } {
        i.ulErrorRet = err::UNSUPPORTED_MESSAGE;
    }
    -2
}

/// OS/2 file open mode for MMIO flags (as IBM's DOS procedure maps them).
fn open_mode(f: u32) -> u32 {
    let mut mode = 0x80; // no inherit
    mode |= if f & MMIO_READWRITE != 0 {
        2
    } else if f & MMIO_WRITE != 0 {
        1
    } else {
        0
    };
    mode |= if f & MMIO_EXCLUSIVE != 0 {
        0x10
    } else if f & MMIO_DENYWRITE != 0 {
        0x20
    } else if f & MMIO_DENYREAD != 0 {
        0x30
    } else {
        0x40
    };
    mode
}

/// The DOS (file system) I/O procedure.
#[unsafe(no_mangle)]
pub extern "C" fn DOSIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    let msg = msg & 0xffff;
    if msg == MMIOM_IDENTIFYFILE {
        let name = p1 as *const u8;
        if name.is_null() {
            return match unsafe { pinfo.as_ref() } {
                Some(i) if i.aulInfo[0] != 0 => 0,
                _ => -1,
            };
        }
        let (mut hf, mut action) = (0u32, 0u32);
        let rc = unsafe { DosOpen(name, &mut hf, &mut action, 0, 0, 1, 0xc0, core::ptr::null_mut()) };
        if rc == 0 {
            unsafe { DosClose(hf) };
            return 0;
        }
        let rc = if rc == 0x7b { 2 } else { rc };
        if let Some(i) = unsafe { pinfo.as_mut() } {
            i.ulErrorRet = rc;
        }
        return rc as i32;
    }
    if msg == MMIOM_GETFORMATINFO || msg == MMIOM_GETFORMATNAME {
        return format_msg(FOURCC_DOS, msg, p1, p2);
    }
    let Some(i) = (unsafe { pinfo.as_mut() }) else { return -1 };
    let hf = i.aulInfo[0];
    match msg {
        MMIOM_OPEN => dos_open(i, p1 as *const u8),
        MMIOM_CLOSE => {
            if p1 as u32 == MMIO_FHOPEN && i.fccIOProc == FOURCC_DOS {
                return 0;
            }
            unsafe { DosClose(hf) as i32 }
        }
        MMIOM_READ => {
            let mut n = 0;
            match unsafe { DosRead(hf, p1 as *mut u8, p2 as u32, &mut n) } {
                0 => n as i32,
                rc => {
                    i.ulErrorRet = rc;
                    -1
                }
            }
        }
        MMIOM_WRITE => {
            let mut n = 0;
            match unsafe { DosWrite(hf, p1 as *const u8, p2 as u32, &mut n) } {
                0 if n == p2 as u32 => n as i32,
                0 => {
                    i.ulErrorRet = err::CANNOTWRITE;
                    -1
                }
                rc => {
                    i.ulErrorRet = rc;
                    -1
                }
            }
        }
        MMIOM_SEEK => {
            let mut pos = 0;
            match unsafe { DosSetFilePtr(hf, p1, p2 as u32, &mut pos) } {
                0 => pos as i32,
                rc => {
                    i.ulErrorRet = rc;
                    -1
                }
            }
        }
        _ => unsupported(i),
    }
}

/// Open (or delete) the file `name`, or take the handle in aulInfo[0].
fn dos_open(i: &mut MmioInfo, name: *const u8) -> i32 {
    let f = i.ulFlags;
    if i.aulInfo[0] != 0 && !name.is_null() {
        return err::INVALID_PARAMETER as i32;
    }
    if i.aulInfo[0] == 0 && name.is_null() {
        return err::INVALID_FILENAME as i32;
    }
    if f & MMIO_DELETE != 0 {
        return unsafe { DosDelete(name) as i32 };
    }
    if f & (MMIO_WRITE | MMIO_READWRITE) == 0 {
        i.ulFlags |= MMIO_READ;
    }
    let mut pos = 0u32;
    if name.is_null() {
        let rc = unsafe { DosSetFilePtr(i.aulInfo[0], 0, FILE_CURRENT, &mut pos) };
        if rc != 0 {
            return rc as i32;
        }
    } else {
        let action_flags = if f & MMIO_CREATE != 0 { 0x12 } else { 0x01 };
        let (mut hf, mut action) = (0u32, 0u32);
        let rc = unsafe { DosOpen(name, &mut hf, &mut action, 0, 0, action_flags, open_mode(f), core::ptr::null_mut()) };
        if rc != 0 {
            return rc as i32;
        }
        i.aulInfo[0] = hf;
    }
    if f & MMIO_APPEND != 0 {
        let rc = unsafe { DosSetFilePtr(i.aulInfo[0], 0, FILE_END, &mut pos) };
        if rc != 0 {
            return rc as i32;
        }
    }
    i.lLogicalFilePos = pos as i32;
    i.lDiskOffset = pos as i32;
    0
}

/// The MEM I/O procedure. MMIO keeps a memory file's data in the handle's
/// buffer, so the procedure has nothing to read or write.
#[unsafe(no_mangle)]
pub extern "C" fn MEMIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    match msg & 0xffff {
        MMIOM_OPEN | MMIOM_CLOSE | MMIOM_READ => 0,
        MMIOM_SEEK => p1,
        MMIOM_WRITE => {
            if let Some(i) = unsafe { pinfo.as_mut() } {
                i.ulErrorRet = err::CANNOTEXPAND;
            }
            -1
        }
        m @ (MMIOM_GETFORMATINFO | MMIOM_GETFORMATNAME) => format_msg(FOURCC_MEM, m, p1, p2),
        _ => unsupported(pinfo),
    }
}

/// The WAVE procedure's instance.
struct Wave {
    /// The file (a DOS procedure MMIOINFO).
    file: MmioInfo,
    format: WaveFormat,
    data_start: u32,
    data_len: u32,
    pos: u32,
    /// The header needs writing (a new file, or one written to).
    dirty: bool,
    writable: bool,
}

impl Wave {
    fn dos(&mut self, msg: u32, p1: i32, p2: i32) -> i32 {
        DOSIOProc(&mut self.file, msg, p1, p2)
    }

    fn read_at(&mut self, at: u32, buf: &mut [u8]) -> i32 {
        if self.dos(MMIOM_SEEK, at as i32, SEEK_SET) < 0 {
            return -1;
        }
        self.dos(MMIOM_READ, buf.as_mut_ptr() as i32, buf.len() as i32)
    }

    /// Find the format and data chunks.
    fn scan(&mut self) -> bool {
        let mut b = [0u8; 16];
        if self.read_at(0, &mut b[..12]) != 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return false;
        }
        let file_len = self.dos(MMIOM_SEEK, 0, SEEK_END).max(0) as u32;
        let mut off = 12u32;
        let mut format = None;
        while off.saturating_add(8) <= file_len {
            if self.read_at(off, &mut b[..8]) != 8 {
                return false;
            }
            let size = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            match &b[0..4] {
                b"fmt " => {
                    let n = size.min(16) as usize;
                    b = [0; 16];
                    if self.read_at(off + 8, &mut b[..n]) != n as i32 {
                        return false;
                    }
                    format = WaveFormat::parse(&b[..n.max(14)]);
                }
                b"data" => {
                    let Some(f) = format else { return false };
                    self.format = f;
                    self.data_start = off + 8;
                    self.data_len = size.min(file_len - self.data_start);
                    return true;
                }
                _ => {}
            }
            off = off.saturating_add(8).saturating_add(mmfmt::riff::padded(size));
        }
        false
    }

    /// Write the RIFF, format and data chunk headers for the data so far.
    fn write_header(&mut self) -> bool {
        let mut hdr = mmfmt::wav::build(&self.format, &[]);
        let riff = 36 + mmfmt::riff::padded(self.data_len);
        hdr[4..8].copy_from_slice(&riff.to_le_bytes());
        hdr[40..44].copy_from_slice(&self.data_len.to_le_bytes());
        if self.dos(MMIOM_SEEK, 0, SEEK_SET) < 0 || self.dos(MMIOM_WRITE, hdr.as_ptr() as i32, 44) != 44 {
            return false;
        }
        if self.data_len & 1 != 0 {
            let end = (self.data_start + self.data_len) as i32;
            if self.dos(MMIOM_SEEK, end, SEEK_SET) < 0 || self.dos(MMIOM_WRITE, [0u8].as_ptr() as i32, 1) != 1 {
                return false;
            }
        }
        true
    }

    /// MMAUDIOHEADER.
    fn audio_header(&self) -> [u8; 40] {
        let f = &self.format;
        let mut h = [0u8; 40];
        h[0..4].copy_from_slice(&40u32.to_le_bytes());
        h[8..12].copy_from_slice(&MMIO_MEDIATYPE_AUDIO.to_le_bytes());
        h[12..28].copy_from_slice(&f.to_bytes());
        h[28..32].copy_from_slice(&f.duration_ms(self.data_len).to_le_bytes());
        h[32..36].copy_from_slice(&self.data_len.to_le_bytes());
        h
    }
}

/// Whether file `name` (or the open file `hmmio`) is a RIFF WAVE file:
/// 0 if so, else MMIO_ERROR.
fn wave_identify(name: *const u8, hmmio: u32) -> i32 {
    let mut b = [0u8; 12];
    let got = if let Some(h) = handle(hmmio) {
        let pos = h.info.lLogicalFilePos;
        let got = if io::seek(h, 0, SEEK_SET) == 0 { io::read(h, b.as_mut_ptr(), 12) } else { -1 };
        io::seek(h, pos, SEEK_SET);
        got
    } else if !name.is_null() {
        let mut f = MmioInfo::zeroed();
        f.ulFlags = MMIO_READ | MMIO_DENYNONE;
        f.fccIOProc = FOURCC_DOS;
        if dos_open(&mut f, name) != 0 {
            return -1;
        }
        let got = DOSIOProc(&mut f, MMIOM_READ, b.as_mut_ptr() as i32, 12);
        DOSIOProc(&mut f, MMIOM_CLOSE, 0, 0);
        got
    } else {
        -1
    };
    if got == 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" { 0 } else { -1 }
}

fn wave<'a>(i: &MmioInfo) -> Option<&'a mut Wave> {
    unsafe { (i.aulInfo[1] as *mut Wave).as_mut() }
}

/// The WAVE file-format I/O procedure (IBM's is WAVEPROC.DLL's
/// WAVEIOProc): reads, writes and seeks within the sample data, and gives
/// (or for a new file takes) the MMAUDIOHEADER.
#[unsafe(no_mangle)]
pub extern "C" fn WAVEIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    let msg = msg & 0xffff;
    match msg {
        MMIOM_IDENTIFYFILE => return wave_identify(p1 as *const u8, p2 as u32),
        MMIOM_GETFORMATINFO | MMIOM_GETFORMATNAME => return format_msg(FOURCC_WAVE, msg, p1, p2),
        MMIOM_QUERYHEADERLENGTH => return 40,
        _ => {}
    }
    let Some(i) = (unsafe { pinfo.as_mut() }) else { return -1 };
    i.ulErrorRet = 0;
    if msg == MMIOM_OPEN {
        return wave_open(i, p1 as *const u8);
    }
    let Some(w) = wave(i) else { return unsupported(i) };
    match msg {
        MMIOM_CLOSE => {
            let ok = !w.dirty || !w.writable || w.write_header();
            let rc = w.dos(MMIOM_CLOSE, p1, 0);
            drop(unsafe { Box::from_raw(w as *mut Wave) });
            i.aulInfo[1] = 0;
            if !ok { err::CANNOTWRITE as i32 } else { rc }
        }
        MMIOM_READ => {
            let n = (p2.max(0) as u32).min(w.data_len.saturating_sub(w.pos));
            if n == 0 {
                return 0;
            }
            if w.dos(MMIOM_SEEK, (w.data_start + w.pos) as i32, SEEK_SET) < 0 {
                i.ulErrorRet = err::SEEK_FAILED;
                return -1;
            }
            let r = w.dos(MMIOM_READ, p1, n as i32);
            if r < 0 {
                i.ulErrorRet = w.file.ulErrorRet;
                return -1;
            }
            w.pos += r as u32;
            r
        }
        MMIOM_WRITE => {
            if !w.writable {
                i.ulErrorRet = err::READ_ONLY_FILE;
                return -1;
            }
            if w.dos(MMIOM_SEEK, (w.data_start + w.pos) as i32, SEEK_SET) < 0 {
                i.ulErrorRet = err::SEEK_FAILED;
                return -1;
            }
            let r = w.dos(MMIOM_WRITE, p1, p2);
            if r < 0 {
                i.ulErrorRet = w.file.ulErrorRet;
                return -1;
            }
            w.pos += r as u32;
            w.data_len = w.data_len.max(w.pos);
            w.dirty = true;
            r
        }
        MMIOM_SEEK => {
            let base = match p2 {
                SEEK_SET => 0,
                SEEK_CUR => w.pos as i64,
                SEEK_END => w.data_len as i64,
                _ => -1,
            };
            let new = base + p1 as i64;
            if base < 0 || new < 0 || new > i32::MAX as i64 {
                i.ulErrorRet = err::SEEK_FAILED;
                return -1;
            }
            w.pos = new as u32;
            new as i32
        }
        MMIOM_GETHEADER => {
            if p1 == 0 || p2 < 40 {
                i.ulErrorRet = err::INVALID_BUFFER_LENGTH;
                if p1 != 0 && p2 >= 4 {
                    unsafe { (p1 as *mut u32).write_unaligned(40) };
                }
                return 0;
            }
            let h = w.audio_header();
            unsafe { core::ptr::copy(h.as_ptr(), p1 as *mut u8, 40) };
            40
        }
        MMIOM_SETHEADER => {
            if p1 == 0 || p2 < 40 {
                i.ulErrorRet = err::INVALID_BUFFER_LENGTH;
                return 0;
            }
            if !w.writable {
                i.ulErrorRet = err::READ_ONLY_FILE;
                return 0;
            }
            let h = unsafe { core::slice::from_raw_parts(p1 as *const u8, 40) };
            let Some(f) = WaveFormat::parse(&h[12..28]) else { return 0 };
            w.format = f;
            w.dirty = true;
            40
        }
        MMIOM_SET => {
            // MMEXTENDINFO: no tracks or CODECs to report or set.
            let ext = p1 as *mut u32;
            if p2 as u32 & 6 != 0 && !ext.is_null() {
                unsafe { ext.add(4).write_unaligned(0) };
            }
            0
        }
        _ => unsupported(i),
    }
}

fn wave_open(i: &mut MmioInfo, name: *const u8) -> i32 {
    let f = i.ulFlags;
    let mut file = MmioInfo::zeroed();
    file.ulFlags = f;
    file.fccIOProc = FOURCC_DOS;
    file.aulInfo[0] = i.aulInfo[0];
    let rc = dos_open(&mut file, name);
    if rc != 0 {
        return rc;
    }
    if f & MMIO_DELETE != 0 {
        return 0;
    }
    i.ulFlags = file.ulFlags;
    let format = WaveFormat { tag: 1, channels: 1, rate: 22050, bits: 8, block_align: 1, avg_bytes: 22050 };
    let writable = f & (MMIO_WRITE | MMIO_READWRITE) != 0;
    let mut w = Box::new(Wave { file, format, data_start: 44, data_len: 0, pos: 0, dirty: false, writable });
    if f & MMIO_CREATE != 0 {
        w.dirty = true;
        if !w.write_header() {
            w.dos(MMIOM_CLOSE, 0, 0);
            return err::CANNOTWRITE as i32;
        }
    } else if !w.scan() {
        w.dos(MMIOM_CLOSE, 0, 0);
        return err::INVALID_FILE as i32;
    }
    i.aulInfo[1] = Box::into_raw(w) as u32;
    i.lLogicalFilePos = 0;
    i.lDiskOffset = 0;
    0
}
