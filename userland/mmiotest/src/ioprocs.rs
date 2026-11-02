//! The file-format I/O procedures of MMPMMMIO.INI (rs2's MMIOPROC.DLL):
//! images and sounds written with translation (mmioSetHeader, mmioWrite)
//! in every format, identified again (mmioOpen without a procedure,
//! mmioIdentifyFile) and read back translated; the INI list
//! (mmioIniFileHandler) and the format list (mmioGetFormats).

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use crate::*;

unsafe extern "C" {
    #[link_name = "MMIO.85"]
    fn mmioIniFileHandler(info: *mut IniInfo, flags: u32) -> u32;
}

#[repr(C)]
struct IniInfo {
    fcc: u32,
    dll: [u8; 260],
    proc_: [u8; 32],
    flags: u32,
    extend: u32,
    media: u32,
    kind: u32,
    ext: [u8; 4],
}

fn ini_info() -> IniInfo {
    unsafe { core::mem::zeroed() }
}

const MMIO_MATCHFIRST: u32 = 0x10;
const MMIO_MATCHNEXT: u32 = 0x20;
const MMIO_MATCHFOURCC: u32 = 0x40;
const MMIO_NOVERIFY: u32 = 0x400;
const TRANSLATE: u32 = 3;
const IMAGE_HEADER: usize = 1116;

const W: u32 = 37;
const H: u32 = 23;

/// A test picture's pel (R, G, B).
fn pel(x: u32, y: u32) -> [u8; 3] {
    [(x * 6) as u8, (y * 11) as u8, ((x + y) * 3) as u8]
}

/// MMIMAGEHEADER of a W x H picture.
fn header(bits: u16, colors: &[[u8; 3]]) -> Vec<u8> {
    let mut h = vec![0u8; IMAGE_HEADER];
    let row = ((W * bits as u32 + 31) / 32 * 4) as usize;
    h[0..4].copy_from_slice(&(IMAGE_HEADER as u32).to_le_bytes());
    h[8..12].copy_from_slice(&1u32.to_le_bytes());
    h[28..32].copy_from_slice(&64u32.to_le_bytes());
    h[32..36].copy_from_slice(&W.to_le_bytes());
    h[36..40].copy_from_slice(&H.to_le_bytes());
    h[40..42].copy_from_slice(&1u16.to_le_bytes());
    h[42..44].copy_from_slice(&bits.to_le_bytes());
    h[48..52].copy_from_slice(&((row * H as usize) as u32).to_le_bytes());
    h[60..64].copy_from_slice(&(colors.len() as u32).to_le_bytes());
    for (i, c) in colors.iter().enumerate() {
        h[92 + i * 4] = c[2];
        h[93 + i * 4] = c[1];
        h[94 + i * 4] = c[0];
    }
    h
}

/// The 24-bit bitmap (bottom-up rows, B G R, padded).
fn bitmap24() -> Vec<u8> {
    let row = ((W * 3 + 3) / 4 * 4) as usize;
    let mut d = vec![0u8; row * H as usize];
    for y in 0..H {
        for x in 0..W {
            let c = pel(x, H - 1 - y);
            let o = y as usize * row + x as usize * 3;
            d[o] = c[2];
            d[o + 1] = c[1];
            d[o + 2] = c[0];
        }
    }
    d
}

fn palette() -> Vec<[u8; 3]> {
    (0..256u32).map(|i| [i as u8, (255 - i) as u8, (i * 3) as u8]).collect()
}

/// The 8-bit bitmap (bottom-up rows of indexes).
fn bitmap8() -> Vec<u8> {
    let row = ((W + 3) / 4 * 4) as usize;
    let mut d = vec![0u8; row * H as usize];
    for y in 0..H {
        for x in 0..W {
            d[y as usize * row + x as usize] = ((x * 7 + y * 13) % 256) as u8;
        }
    }
    d
}

fn write_file(name: &[u8], fcc: u32, hdr: &[u8], data: &[u8]) -> bool {
    unsafe {
        let mut i = info();
        i.fccIOProc = fcc;
        i.ulTranslate = TRANSLATE;
        let h = mmioOpen(name.as_ptr(), &mut i, MMIO_CREATE | MMIO_WRITE);
        if h == 0 {
            return false;
        }
        let mut n = 0;
        let ok = mmioSetHeader(h, hdr.as_ptr(), hdr.len() as i32, &mut n, 0, 0) == 0
            && mmioWrite(h, data.as_ptr(), data.len() as i32) == data.len() as i32;
        mmioClose(h, 0) == 0 && ok
    }
}

/// Open by name (identified), translated: (procedure, header, data).
fn read_file(name: &[u8]) -> Option<(u32, Vec<u8>, Vec<u8>)> {
    unsafe {
        let mut i = info();
        i.ulTranslate = TRANSLATE;
        let h = mmioOpen(name.as_ptr(), &mut i, MMIO_READ);
        if h == 0 {
            return None;
        }
        let mut len = 0;
        mmioQueryHeaderLength(h, &mut len, 0, 0);
        let mut hdr = vec![0u8; len.max(1) as usize];
        let mut n = 0;
        mmioGetHeader(h, hdr.as_mut_ptr(), hdr.len() as i32, &mut n, 0, 0);
        hdr.truncate(n as usize);
        let mut data = Vec::new();
        let mut buf = vec![0u8; 4000];
        loop {
            let r = mmioRead(h, buf.as_mut_ptr(), buf.len() as i32);
            if r <= 0 {
                break;
            }
            data.extend_from_slice(&buf[..r as usize]);
        }
        mmioClose(h, 0);
        Some((i.fccIOProc, hdr, data))
    }
}

fn identified(name: &[u8]) -> u32 {
    let mut f = FormatInfo::default();
    let mut ss = 0;
    if unsafe { mmioIdentifyFile(name.as_ptr(), null(), &mut f, &mut ss, 0, 0) } != 0 {
        return 0;
    }
    f.fccIOProc
}

fn delete(name: &[u8]) {
    unsafe { DosDelete(name.as_ptr()) };
}

fn fcc_text(f: u32) -> alloc::string::String {
    f.to_le_bytes().iter().map(|&b| b as char).collect()
}

fn images() {
    let pic = bitmap24();
    // (procedure, file, lossless)
    let list: [(&[u8; 4], &[u8], bool); 11] = [
        (b"OS20", b"\\MMIOI1.BMP\0", true),
        (b"OS13", b"\\MMIOI2.BMP\0", true),
        (b"WI30", b"\\MMIOI3.BMP\0", true),
        (b"PCXC", b"\\MMIOI4.PCX\0", true),
        (b"TFIU", b"\\MMIOI5.TIF\0", true),
        (b"TFIC", b"\\MMIOI6.TIF\0", true),
        (b"TFMU", b"\\MMIOI7.TIF\0", true),
        (b"TFMC", b"\\MMIOI8.TIF\0", true),
        (b"TGAU", b"\\MMIOI9.TGA\0", true),
        (b"TGAC", b"\\MMIOIA.TGA\0", true),
        (b"JPEG", b"\\MMIOIB.JPG\0", false),
    ];
    for (fcc, name, lossless) in list {
        let f = fourcc(fcc);
        let t = fcc_text(f);
        check(&alloc::format!("{t}: write a 37x23 24-bit image"), write_file(name, f, &header(24, &[]), &pic));
        check(&alloc::format!("{t}: identified"), identified(name) == f);
        match read_file(name) {
            Some((p, hdr, data)) => {
                check(&alloc::format!("{t}: opened by its procedure"), p == f);
                let ok_hdr = hdr.len() == IMAGE_HEADER && le32(&hdr, 32) == W && le32(&hdr, 36) == H && le16(&hdr, 42) == 24;
                check(&alloc::format!("{t}: MMIMAGEHEADER 37x23x24"), ok_hdr);
                let same = if lossless {
                    data == pic
                } else {
                    data.len() == pic.len() && data.iter().zip(&pic).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs()).sum::<u32>() / (pic.len() as u32) < 12
                };
                check(&alloc::format!("{t}: the bitmap read back"), same);
            }
            None => check(&alloc::format!("{t}: open translated"), false),
        }
        delete(name);
    }
    // Palette images: GIF and an 8-bit OS/2 bitmap.
    let pal = palette();
    let pic8 = bitmap8();
    for (fcc, name) in [(b"GIFC", &b"\\MMIOIC.GIF\0"[..]), (b"OS20", &b"\\MMIOID.BMP\0"[..])] {
        let f = fourcc(fcc);
        let t = fcc_text(f);
        check(&alloc::format!("{t}: write an 8-bit image"), write_file(name, f, &header(8, &pal), &pic8));
        match read_file(name) {
            Some((p, hdr, data)) => {
                let same_pal = hdr.len() == IMAGE_HEADER && (0..256).all(|i| hdr[92 + i * 4..95 + i * 4] == [pal[i][2], pal[i][1], pal[i][0]]);
                check(&alloc::format!("{t}: 8-bit, its palette"), p == f && le16(&hdr, 42) == 8 && same_pal);
                check(&alloc::format!("{t}: the indexes read back"), data == pic8);
            }
            None => check(&alloc::format!("{t}: open translated"), false),
        }
        delete(name);
    }
}

/// 16-bit mono 22050 Hz samples.
fn tone() -> Vec<u8> {
    let mut d = Vec::new();
    for i in 0..2205i32 {
        d.extend_from_slice(&(((i * 523) % 30000 - 15000) as i16).to_le_bytes());
    }
    d
}

fn audio_header() -> Vec<u8> {
    let mut h = vec![0u8; 40];
    h[0..4].copy_from_slice(&40u32.to_le_bytes());
    h[8..12].copy_from_slice(&2u32.to_le_bytes());
    h[12..14].copy_from_slice(&1u16.to_le_bytes());
    h[14..16].copy_from_slice(&1u16.to_le_bytes());
    h[16..20].copy_from_slice(&22050u32.to_le_bytes());
    h[20..24].copy_from_slice(&44100u32.to_le_bytes());
    h[24..26].copy_from_slice(&2u16.to_le_bytes());
    h[26..28].copy_from_slice(&16u16.to_le_bytes());
    h
}

fn sounds() {
    let s = tone();
    for (fcc, name) in [(b"AU  ", &b"\\MMIOA1.AU\0"[..]), (b"VOC ", &b"\\MMIOA2.VOC\0"[..]), (b"AIFF", &b"\\MMIOA3.AIF\0"[..])] {
        let f = fourcc(fcc);
        let t = fcc_text(f);
        check(&alloc::format!("{t}: write 0.1 s of 16-bit sound"), write_file(name, f, &audio_header(), &s));
        check(&alloc::format!("{t}: identified"), identified(name) == f);
        match read_file(name) {
            Some((p, hdr, data)) => {
                let ok = hdr.len() == 40 && le32(&hdr, 16) == 22050 && le16(&hdr, 26) == 16 && le32(&hdr, 32) == s.len() as u32 && le32(&hdr, 28) == 100;
                check(&alloc::format!("{t}: MMAUDIOHEADER (22050 Hz, 16 bits, 100 ms)"), p == f && ok);
                check(&alloc::format!("{t}: the samples read back"), data == s);
            }
            None => check(&alloc::format!("{t}: open translated"), false),
        }
        delete(name);
    }
}

fn ini() {
    unsafe {
        // Every installed procedure, as MMBROWSE lists them.
        let mut e = ini_info();
        let mut n = 0;
        let mut jpeg = false;
        let mut rc = mmioIniFileHandler(&mut e, MMIO_FINDPROC | MMIO_MATCHFIRST);
        while rc == 0 && n < 100 {
            n += 1;
            jpeg |= e.fcc == fourcc(b"JPEG") && e.dll.starts_with(b"MMIOPROC\0") && e.proc_.starts_with(b"JPEGIOProc\0") && e.media == 1;
            rc = mmioIniFileHandler(&mut e, MMIO_FINDPROC | MMIO_MATCHNEXT);
        }
        check("mmioIniFileHandler lists the procedures (19 or more)", n >= 19);
        check("JPEG: MMIOPROC JPEGIOProc, image", jpeg);
        // Install a procedure under another FOURCC, find it, remove it.
        let mut t = ini_info();
        t.fcc = fourcc(b"ZZTS");
        let path = b"C:\\OS2\\DLL\\MMIOPROC.DLL\0";
        t.dll[..path.len()].copy_from_slice(path);
        t.proc_[..11].copy_from_slice(b"GIFCIOProc\0");
        check("install an I/O procedure in MMPMMMIO.INI", mmioIniFileHandler(&mut t, MMIO_INSTALLPROC) == 0);
        let mut f = ini_info();
        f.fcc = fourcc(b"ZZTS");
        let found = mmioIniFileHandler(&mut f, MMIO_FINDPROC | MMIO_MATCHFOURCC) == 0;
        check("find it (module name, media type from the procedure)", found && f.dll.starts_with(b"MMIOPROC\0") && f.media == 1);
        check("mmioInstallIOProc finds it", mmioInstallIOProc(fourcc(b"ZZTS"), 0, MMIO_FINDPROC) != 0);
        check("remove it", mmioIniFileHandler(&mut f, MMIO_REMOVEPROC | MMIO_MATCHFOURCC) == 0);
        let mut g = ini_info();
        g.fcc = fourcc(b"ZZTS");
        check("it is gone", mmioIniFileHandler(&mut g, MMIO_FINDPROC | MMIO_MATCHFOURCC) == 6537);
        let mut bad = ini_info();
        bad.fcc = fourcc(b"ZZTT");
        bad.dll[..8].copy_from_slice(b"NOSUCHDL");
        bad.proc_[..2].copy_from_slice(b"X\0");
        check("a missing DLL is refused", mmioIniFileHandler(&mut bad, MMIO_INSTALLPROC) == 6535);
        let _ = MMIO_NOVERIFY;
        // The format list has the image formats.
        let want = FormatInfo { ulMediaType: 1, ..Default::default() };
        let mut count = 0;
        mmioQueryFormatCount(&want, &mut count, 0, 0);
        check("mmioQueryFormatCount: 12 image formats", count == 12);
        let mut list = vec![FormatInfo::default(); 20];
        let mut got = 0;
        mmioGetFormats(&want, 20, list.as_mut_ptr(), &mut got, 0, 0);
        check("mmioGetFormats", got == 12 && list.iter().take(12).any(|f| f.fccIOProc == fourcc(b"GIFC") && &f.ext[..3] == b"GIF"));
        let _ = null_mut::<u8>();
    }
}

pub fn run() {
    ini();
    images();
    sounds();
    devices();
}

unsafe extern "C" {
    #[link_name = "MDM.2"]
    fn mciSendString(cmd: *const u8, ret: *mut u8, cb: u32, hwnd: u32, user: u32) -> u32;
}

fn mci(cmd: &str) -> (u32, alloc::string::String) {
    let mut c: Vec<u8> = cmd.bytes().collect();
    c.push(0);
    let mut ret = [0u8; 128];
    let rc = unsafe { mciSendString(c.as_ptr(), ret.as_mut_ptr(), ret.len() as u32, 0, 0) };
    let n = ret.iter().position(|&b| b == 0).unwrap_or(0);
    (rc & 0xffff, ret[..n].iter().map(|&b| b as char).collect())
}

fn exists(name: &[u8]) -> bool {
    let mut i = info();
    i.fccIOProc = DOS;
    let h = unsafe { mmioOpen(name.as_ptr(), &mut i, MMIO_READ | MMIO_NOIDENTIFY) };
    if h != 0 {
        unsafe { mmioClose(h, 0) };
    }
    h != 0
}

/// MDM plays the other formats: an AU file on the wave audio device, an
/// AVI movie (Video 1 with a PCM track, 2 s at 15 frames/s) on the
/// digital video device (`\TEST.AVI`, `\TONE.AU`: generated by the test
/// setup; skipped when missing).
fn devices() {
    if exists(b"\\TONE.AU\0") {
        check("waveaudio: open an AU file", mci("open \\TONE.AU alias au wait").0 == 0);
        mci("set au time format milliseconds wait");
        let (rc, len) = mci("status au length wait");
        check(&alloc::format!("waveaudio: AU length 1000 ms ({len})"), rc == 0 && len == "1000");
        check("waveaudio: play the AU file", mci("play au wait").0 == 0);
        mci("close au wait");
    } else {
        println!("SKIP no \\TONE.AU");
    }
    if exists(b"\\TEST.AVI\0") {
        check("digitalvideo: open an AVI file", mci("open \\TEST.AVI type digitalvideo alias v wait").0 == 0);
        mci("set v time format frames wait");
        let (rc, len) = mci("status v length wait");
        check(&alloc::format!("digitalvideo: AVI length 30 frames ({len})"), rc == 0 && len == "30");
        let (_, w) = mci("where v source wait");
        check(&alloc::format!("digitalvideo: AVI source 160x120 ({w})"), w.ends_with("160 120"));
        check("digitalvideo: play the AVI file", mci("play v wait").0 == 0);
        let (_, pos) = mci("status v position wait");
        check(&alloc::format!("digitalvideo: at the end ({pos})"), pos == "29");
        mci("close v wait");
    } else {
        println!("SKIP no \\TEST.AVI");
    }
}
