//! The I/O procedures: one entry point per FOURCC, all sharing `ioproc`.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use mmfmt::sound::{self, Sound};
use mmimage::{Format, Image};

unsafe extern "C" {
    #[link_name = "MMIO.27"]
    fn mmioOpen(name: *const u8, info: *mut MmioInfo, flags: u32) -> u32;
    #[link_name = "MMIO.28"]
    fn mmioRead(h: u32, buf: *mut u8, n: i32) -> i32;
    #[link_name = "MMIO.29"]
    fn mmioWrite(h: u32, buf: *const u8, n: i32) -> i32;
    #[link_name = "MMIO.30"]
    fn mmioSeek(h: u32, off: i32, origin: i32) -> i32;
    #[link_name = "MMIO.32"]
    fn mmioClose(h: u32, flags: u32) -> u32;
}

/// MMIOINFO (mmioos2.h).
#[repr(C)]
pub struct MmioInfo {
    pub ulFlags: u32,
    pub fccIOProc: u32,
    pub pIOProc: usize,
    pub ulErrorRet: u32,
    pub cchBuffer: i32,
    pub pchBuffer: *mut u8,
    pub pchNext: *mut u8,
    pub pchEndRead: *mut u8,
    pub pchEndWrite: *mut u8,
    pub lBufOffset: i32,
    pub lDiskOffset: i32,
    pub aulInfo: [u32; 4],
    pub lLogicalFilePos: i32,
    pub ulTranslate: u32,
    pub fccChildIOProc: u32,
    pub pExtraInfoStruct: usize,
    pub hmmio: u32,
}

const fn fourcc(c: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*c)
}

const MMIOM_CLOSE: u32 = 0xE03;
const MMIOM_OPEN: u32 = 0xE04;
const MMIOM_READ: u32 = 0xE05;
const MMIOM_SEEK: u32 = 0xE06;
const MMIOM_WRITE: u32 = 0xE07;
const MMIOM_IDENTIFYFILE: u32 = 0xE08;
const MMIOM_GETHEADER: u32 = 0xE09;
const MMIOM_SETHEADER: u32 = 0xE0A;
const MMIOM_QUERYHEADERLENGTH: u32 = 0xE0B;
const MMIOM_GETFORMATNAME: u32 = 0xE0C;
const MMIOM_GETFORMATINFO: u32 = 0xE0D;
const MMIOM_QUERYIMAGECOUNT: u32 = 0xE27;
const MMIOM_SETIMAGE: u32 = 0xE29;

const MMIO_CREATE: u32 = 0x1;
const MMIO_READ: u32 = 0x4;
const MMIO_WRITE: u32 = 0x8;
const MMIO_READWRITE: u32 = 0x10;
const MMIO_DENYWRITE: u32 = 0x80;
const MMIO_DENYNONE: u32 = 0x200;
const MMIO_DELETE: u32 = 0x800;
const MMIO_NOIDENTIFY: u32 = 0x40000;
const MMIO_TRANSLATEDATA: u32 = 1;
const MMIO_TRANSLATEHEADER: u32 = 2;

const MMIO_SUCCESS: i32 = 0;
const MMIO_ERROR: i32 = -1;
const MMIOM_UNSUPPORTED: i32 = -2;

const MMIOERR_INVALID_PARAMETER: u32 = 6505;
const MMIOERR_INVALID_BUFFER_LENGTH: u32 = 6510;
const MMIOERR_READ_ONLY_FILE: u32 = 6514;
const MMIOERR_WRITE_ONLY_FILE: u32 = 6515;
const MMIOERR_WRITE_FAILED: u32 = 6518;
const MMIOERR_OUTOFMEMORY: u32 = 6534;
const MMIOERR_INVALID_FILE: u32 = 6539;
const MMIOERR_INVALID_STRUCTURE: u32 = 6547;

const MEDIA_IMAGE: u32 = 1;
const MEDIA_AUDIO: u32 = 2;
const MEDIA_MIDI: u32 = 4;
const MEDIA_MOVIE: u32 = 0x100;

/// The formats, by I/O procedure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Os13,
    Os20,
    Wi30,
    Jpeg,
    Gif,
    Pcx,
    TiffIntelComp,
    TiffIntel,
    TiffMotComp,
    TiffMot,
    TgaComp,
    Tga,
    Au,
    Snd,
    Voc,
    Aiff,
    Iff,
    Midi,
    Avi,
}

/// (FOURCC, media type, capability flags, default extension, name).
fn info(k: Kind) -> (u32, u32, u32, &'static [u8], &'static [u8]) {
    // MMIO_CANREADTRANSLATED | CANWRITETRANSLATED | CANSEEKTRANSLATED
    // (IBM's image procedures register 67); read-only formats 65.
    const RW: u32 = 0x43;
    const RO: u32 = 0x41;
    match k {
        Kind::Os13 => (fourcc(b"OS13"), MEDIA_IMAGE, RW, b"BMP", b"OS/2 1.3 Bitmap"),
        Kind::Os20 => (fourcc(b"OS20"), MEDIA_IMAGE, RW, b"BMP", b"OS/2 2.0 Bitmap"),
        Kind::Wi30 => (fourcc(b"WI30"), MEDIA_IMAGE, RW, b"BMP", b"Windows 3.x Bitmap"),
        Kind::Jpeg => (fourcc(b"JPEG"), MEDIA_IMAGE, RW, b"JPG", b"JPEG Image"),
        Kind::Gif => (fourcc(b"GIFC"), MEDIA_IMAGE, RW, b"GIF", b"GIF Image"),
        Kind::Pcx => (fourcc(b"PCXC"), MEDIA_IMAGE, RW, b"PCX", b"PCX Image"),
        Kind::TiffIntelComp => (fourcc(b"TFIC"), MEDIA_IMAGE, RW, b"TIF", b"TIFF Intel Compressed"),
        Kind::TiffIntel => (fourcc(b"TFIU"), MEDIA_IMAGE, RW, b"TIF", b"TIFF Intel Uncompressed"),
        Kind::TiffMotComp => (fourcc(b"TFMC"), MEDIA_IMAGE, RW, b"TIF", b"TIFF Motorola Compressed"),
        Kind::TiffMot => (fourcc(b"TFMU"), MEDIA_IMAGE, RW, b"TIF", b"TIFF Motorola Uncompressed"),
        Kind::TgaComp => (fourcc(b"TGAC"), MEDIA_IMAGE, RW, b"TGA", b"Targa Compressed"),
        Kind::Tga => (fourcc(b"TGAU"), MEDIA_IMAGE, RW, b"TGA", b"Targa Uncompressed"),
        Kind::Au => (fourcc(b"AU  "), MEDIA_AUDIO, RW, b"AU", b"Sun/NeXT Audio (AU)"),
        Kind::Snd => (fourcc(b"SND "), MEDIA_AUDIO, RW, b"SND", b"Sun/NeXT Audio (SND)"),
        Kind::Voc => (fourcc(b"VOC "), MEDIA_AUDIO, RW, b"VOC", b"Creative Voice"),
        Kind::Aiff => (fourcc(b"AIFF"), MEDIA_AUDIO, RW, b"AIF", b"Apple AIFF Audio"),
        Kind::Iff => (fourcc(b"IFF "), MEDIA_AUDIO, RO, b"IFF", b"Amiga IFF 8SVX Audio"),
        Kind::Midi => (fourcc(b"MIDI"), MEDIA_MIDI, RO, b"MID", b"MIDI"),
        Kind::Avi => (fourcc(b"AVI "), MEDIA_MOVIE, RO, b"AVI", b"AVI Movie"),
    }
}

fn is_image(k: Kind) -> bool {
    info(k).1 == MEDIA_IMAGE
}

fn is_audio(k: Kind) -> bool {
    info(k).1 == MEDIA_AUDIO
}

/// The procedure whose format the start of a file is.
fn identify(head: &[u8], len: u64) -> Option<Kind> {
    if let Some(f) = mmimage::identify(head, len) {
        return Some(match f {
            Format::Os13Bmp => Kind::Os13,
            Format::Os20Bmp => Kind::Os20,
            Format::WinBmp => Kind::Wi30,
            Format::Jpeg => Kind::Jpeg,
            Format::Gif => Kind::Gif,
            Format::Pcx => Kind::Pcx,
            Format::TiffIntel { compressed: true } => Kind::TiffIntelComp,
            Format::TiffIntel { compressed: false } => Kind::TiffIntel,
            Format::TiffMotorola { compressed: true } => Kind::TiffMotComp,
            Format::TiffMotorola { compressed: false } => Kind::TiffMot,
            Format::Targa { compressed: true } => Kind::TgaComp,
            Format::Targa { compressed: false } => Kind::Tga,
        });
    }
    if let Some(s) = sound::identify(head) {
        return Some(match s {
            // `.snd` files are both AU and SND: the AU procedure claims them
            // (the SND procedure takes them when opened by its FOURCC).
            sound::Kind::Au => Kind::Au,
            sound::Kind::Voc => Kind::Voc,
            sound::Kind::Aiff => Kind::Aiff,
            sound::Kind::Svx => Kind::Iff,
        });
    }
    if head.starts_with(b"MThd") || (head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"RMID") {
        return Some(Kind::Midi);
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"AVI " {
        return Some(Kind::Avi);
    }
    None
}

/// Whether a file identified as `found` may be opened by procedure `k`.
fn claims(k: Kind, found: Kind) -> bool {
    k == found || (k == Kind::Snd && found == Kind::Au)
}

/// An open file's state (pExtraInfoStruct).
struct Inst {
    kind: Kind,
    /// The storage-system handle.
    child: u32,
    writing: bool,
    /// The translated header (MMIMAGEHEADER, MMAUDIOHEADER, MMMIDIHEADER).
    header: Vec<u8>,
    header_set: bool,
    /// What reads give and writes collect.
    data: Vec<u8>,
    pos: usize,
}

const IMAGE_HEADER_LEN: usize = 1116;
const AUDIO_HEADER_LEN: usize = 40;
const MIDI_HEADER_LEN: usize = 30;

fn put32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn get32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn get16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}

/// MMIMAGEHEADER for an image: ulHeaderLength, ulContentType,
/// ulMediaType, the XDIB prefix (ulMemSize, ulPelFormat, usTransType,
/// ulTransVal), BITMAPINFOHEADER2 (64 bytes) at 28 and 256 RGB2 at 92.
fn image_header(img: &Image) -> Vec<u8> {
    let mut h = vec![0u8; IMAGE_HEADER_LEN];
    let size = img.row_bytes() * img.height as usize;
    put32(&mut h, 0, IMAGE_HEADER_LEN as u32);
    put32(&mut h, 4, if img.bits == 24 { 4 } else { 0 });
    put32(&mut h, 8, MEDIA_IMAGE);
    put32(&mut h, 12, size as u32);
    let b = 28;
    put32(&mut h, b, 64);
    put32(&mut h, b + 4, img.width);
    put32(&mut h, b + 8, img.height);
    put16(&mut h, b + 12, 1);
    put16(&mut h, b + 14, img.bits as u16);
    put32(&mut h, b + 20, size as u32);
    if img.bits <= 8 {
        put32(&mut h, b + 32, img.palette.len() as u32);
    }
    for (i, c) in img.palette.iter().take(256).enumerate() {
        let o = 92 + i * 4;
        h[o] = c[2];
        h[o + 1] = c[1];
        h[o + 2] = c[0];
    }
    h
}

/// An image from a header (as an application sets it) and bitmap data
/// (bottom-up rows).
fn image_from(header: &[u8], data: &[u8]) -> Option<Image> {
    let b = 28;
    let w = get32(header, b + 4);
    let hgt = get32(header, b + 8);
    let bits = get16(header, b + 14) as u8;
    if w == 0 || hgt == 0 || w > 65535 || hgt > 65535 || !matches!(bits, 1 | 4 | 8 | 24) {
        return None;
    }
    let mut img = Image::new(w, hgt, bits);
    if bits <= 8 {
        let used = get32(header, b + 32) as usize;
        let n = if used == 0 || used > 1 << bits { 1 << bits } else { used };
        img.palette = (0..n).map(|i| {
            let o = 92 + i * 4;
            [header[o + 2], header[o + 1], header[o]]
        }).collect();
    }
    let rb = img.row_bytes();
    for y in 0..hgt as usize {
        let src = (hgt as usize - 1 - y) * rb;
        let row = img.row_mut(y as u32);
        if let Some(s) = data.get(src..src + rb) {
            row.copy_from_slice(s);
        } else if let Some(s) = data.get(src..) {
            row[..s.len()].copy_from_slice(s);
        }
    }
    Some(img)
}

/// The bitmap data of an image: rows bottom-up.
fn bitmap_bytes(img: &Image) -> Vec<u8> {
    let rb = img.row_bytes();
    let mut out = Vec::with_capacity(rb * img.height as usize);
    for y in (0..img.height).rev() {
        out.extend_from_slice(&img.row(y)[..rb]);
    }
    out
}

/// MMAUDIOHEADER: ulHeaderLength, ulContentType, ulMediaType, WAVE_HEADER
/// (tag, channels, rate, avg bytes, block align, bits), then
/// ulAudioLengthInMS, ulAudioLengthInBytes, pAdditionalInformation.
fn audio_header(s: &Sound) -> Vec<u8> {
    let mut h = vec![0u8; AUDIO_HEADER_LEN];
    let f = &s.format;
    put32(&mut h, 0, AUDIO_HEADER_LEN as u32);
    put32(&mut h, 8, MEDIA_AUDIO);
    put16(&mut h, 12, f.tag);
    put16(&mut h, 14, f.channels);
    put32(&mut h, 16, f.rate);
    put32(&mut h, 20, f.avg_bytes);
    put16(&mut h, 24, f.block_align);
    put16(&mut h, 26, f.bits);
    put32(&mut h, 28, f.duration_ms(s.data.len() as u32));
    put32(&mut h, 32, s.data.len() as u32);
    h
}

fn audio_format_from(header: &[u8]) -> Option<mmfmt::wav::WaveFormat> {
    let ch = get16(header, 14);
    let rate = get32(header, 16);
    let bits = get16(header, 26);
    if ch == 0 || ch > 8 || rate == 0 || !matches!(bits, 8 | 16) {
        return None;
    }
    Some(sound::pcm_format(ch, rate, bits))
}

/// MMMIDIHEADER (packed): lengths and types, then MIDIHEADER: "MThd",
/// header length, format, tracks, division, a pointer.
fn midi_header(smf: &[u8]) -> Vec<u8> {
    let mut h = vec![0u8; MIDI_HEADER_LEN];
    put32(&mut h, 0, MIDI_HEADER_LEN as u32);
    put32(&mut h, 8, MEDIA_MIDI);
    h[12..16].copy_from_slice(b"MThd");
    if smf.len() >= 14 {
        put32(&mut h, 16, u32::from_be_bytes(smf[4..8].try_into().unwrap()));
        put16(&mut h, 20, u16::from_be_bytes([smf[8], smf[9]]));
        put16(&mut h, 22, u16::from_be_bytes([smf[10], smf[11]]));
        put16(&mut h, 24, u16::from_be_bytes([smf[12], smf[13]]));
    }
    h
}

/// The Standard MIDI data of a MIDI or RMID file.
fn smf_of(file: &[u8]) -> &[u8] {
    if file.len() >= 12 && &file[0..4] == b"RIFF" {
        for c in mmfmt::riff::chunks(&file[12..]) {
            if c.header.id == fourcc(b"data") {
                return c.data;
            }
        }
    }
    file
}

/// Read a whole storage-system file.
fn read_all(h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = unsafe { mmioRead(h, buf.as_mut_ptr(), buf.len() as i32) };
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    out
}

fn inst<'a>(pinfo: *mut MmioInfo) -> Option<&'a mut Inst> {
    let i = unsafe { pinfo.as_mut() }?;
    unsafe { (i.pExtraInfoStruct as *mut Inst).as_mut() }
}

/// Open the storage system under a file (the DOS procedure, or the
/// child procedure the caller named).
fn open_child(name: *const u8, i: &MmioInfo, flags: u32) -> u32 {
    let mut ci: MmioInfo = unsafe { core::mem::zeroed() };
    ci.fccIOProc = if i.fccChildIOProc != 0 { i.fccChildIOProc } else { fourcc(b"DOS ") };
    if ci.fccIOProc == fourcc(b"MEM ") {
        ci.pchBuffer = i.pchBuffer;
        ci.cchBuffer = i.cchBuffer;
        ci.aulInfo = i.aulInfo;
    }
    unsafe { mmioOpen(name, &mut ci, flags | MMIO_NOIDENTIFY) }
}

fn decode_file(kind: Kind, file: &[u8], translate: bool) -> Result<(Vec<u8>, Vec<u8>), u32> {
    if is_image(kind) {
        let img = mmimage::decode(file).map_err(|_| MMIOERR_INVALID_FILE)?;
        let header = image_header(&img);
        let data = if translate { bitmap_bytes(&img) } else { file.to_vec() };
        Ok((header, data))
    } else if is_audio(kind) {
        let s = sound::parse(file).map_err(|_| MMIOERR_INVALID_FILE)?;
        let header = audio_header(&s);
        Ok((header, s.data))
    } else if kind == Kind::Midi {
        let smf = smf_of(file);
        if !smf.starts_with(b"MThd") {
            return Err(MMIOERR_INVALID_FILE);
        }
        Ok((midi_header(smf), smf.to_vec()))
    } else {
        Ok((Vec::new(), file.to_vec()))
    }
}

fn encode_file(kind: Kind, header: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if is_image(kind) {
        let img = image_from(header, data)?;
        use mmimage::*;
        return Some(match kind {
            Kind::Os13 => bmp::encode(&img, bmp::Kind::Os13),
            Kind::Os20 => bmp::encode(&img, bmp::Kind::Os20),
            Kind::Wi30 => bmp::encode(&img, bmp::Kind::Win3),
            Kind::Jpeg => jpeg::encode(&img.to_24(), 85),
            Kind::Gif => gif::encode(&img),
            Kind::Pcx => pcx::encode(&img),
            Kind::TiffIntelComp => tiff::encode(&img, false, true),
            Kind::TiffIntel => tiff::encode(&img, false, false),
            Kind::TiffMotComp => tiff::encode(&img, true, true),
            Kind::TiffMot => tiff::encode(&img, true, false),
            Kind::TgaComp => targa::encode(&img, true),
            _ => targa::encode(&img, false),
        });
    }
    let f = audio_format_from(header)?;
    let s = Sound { format: f, data: data[..data.len() / f.block_align as usize * f.block_align as usize].to_vec() };
    match kind {
        Kind::Au | Kind::Snd => Some(sound::build_au(&s)),
        Kind::Voc => Some(sound::build_voc(&s)),
        Kind::Aiff => Some(sound::build_aiff(&s)),
        _ => None,
    }
}

fn open(kind: Kind, i: &mut MmioInfo, name: *const u8) -> i32 {
    let flags = i.ulFlags;
    if flags & MMIO_DELETE != 0 {
        let h = open_child(name, i, MMIO_DELETE);
        return if h != 0 { MMIO_SUCCESS } else { MMIO_ERROR };
    }
    let writing = flags & (MMIO_WRITE | MMIO_READWRITE) != 0;
    if flags & MMIO_READWRITE != 0 {
        // As IBM's procedures: reading and writing the same file is refused.
        i.ulErrorRet = MMIOERR_INVALID_PARAMETER;
        return MMIO_ERROR;
    }
    if writing && info(kind).2 & 2 == 0 {
        i.ulErrorRet = MMIOERR_READ_ONLY_FILE;
        return MMIO_ERROR;
    }
    let share = flags & 0x3e0;
    let child = if writing {
        open_child(name, i, MMIO_WRITE | MMIO_CREATE | if share != 0 { share } else { MMIO_DENYWRITE })
    } else {
        open_child(name, i, MMIO_READ | if share != 0 { share } else { MMIO_DENYNONE })
    };
    if child == 0 {
        if i.ulErrorRet == 0 {
            i.ulErrorRet = MMIOERR_INVALID_FILE;
        }
        return MMIO_ERROR;
    }
    let mut st = Box::new(Inst { kind, child, writing, header: Vec::new(), header_set: false, data: Vec::new(), pos: 0 });
    if !writing {
        let file = read_all(child);
        let head = &file[..file.len().min(4096)];
        if !identify(head, file.len() as u64).is_some_and(|f| claims(kind, f)) {
            unsafe { mmioClose(child, 0) };
            i.ulErrorRet = MMIOERR_INVALID_FILE;
            return MMIO_ERROR;
        }
        match decode_file(kind, &file, i.ulTranslate & MMIO_TRANSLATEDATA != 0) {
            Ok((h, d)) => {
                st.header = h;
                st.data = d;
            }
            Err(e) => {
                unsafe { mmioClose(child, 0) };
                i.ulErrorRet = e;
                return MMIO_ERROR;
            }
        }
    }
    i.pExtraInfoStruct = Box::into_raw(st) as usize;
    i.lLogicalFilePos = 0;
    MMIO_SUCCESS
}

fn close(i: &mut MmioInfo) -> i32 {
    let p = i.pExtraInfoStruct as *mut Inst;
    if p.is_null() {
        return MMIO_SUCCESS;
    }
    let st = unsafe { Box::from_raw(p) };
    i.pExtraInfoStruct = 0;
    let mut rc = MMIO_SUCCESS;
    if st.writing {
        if !st.header_set {
            rc = MMIOERR_INVALID_STRUCTURE as i32;
        } else if let Some(file) = encode_file(st.kind, &st.header, &st.data) {
            let n = unsafe { mmioWrite(st.child, file.as_ptr(), file.len() as i32) };
            if n != file.len() as i32 {
                rc = MMIOERR_WRITE_FAILED as i32;
            }
        } else {
            rc = MMIOERR_INVALID_STRUCTURE as i32;
        }
    }
    unsafe { mmioClose(st.child, 0) };
    rc
}

fn header_len(k: Kind) -> usize {
    if is_image(k) {
        IMAGE_HEADER_LEN
    } else if is_audio(k) {
        AUDIO_HEADER_LEN
    } else if k == Kind::Midi {
        MIDI_HEADER_LEN
    } else {
        0
    }
}

fn read(i: &mut MmioInfo, buf: *mut u8, n: i32) -> i32 {
    let Some(st) = inst(i) else { return MMIO_ERROR };
    if st.writing {
        i.ulErrorRet = MMIOERR_WRITE_ONLY_FILE;
        return MMIO_ERROR;
    }
    if buf.is_null() || n < 0 {
        return MMIO_ERROR;
    }
    let take = (n as usize).min(st.data.len().saturating_sub(st.pos));
    unsafe { core::ptr::copy_nonoverlapping(st.data.as_ptr().add(st.pos), buf, take) };
    st.pos += take;
    i.lLogicalFilePos = st.pos as i32;
    take as i32
}

fn write(i: &mut MmioInfo, buf: *const u8, n: i32) -> i32 {
    let Some(st) = inst(i) else { return MMIO_ERROR };
    if !st.writing {
        i.ulErrorRet = MMIOERR_READ_ONLY_FILE;
        return MMIO_ERROR;
    }
    if buf.is_null() || n < 0 {
        return MMIO_ERROR;
    }
    let src = unsafe { core::slice::from_raw_parts(buf, n as usize) };
    let end = st.pos + src.len();
    if st.data.try_reserve(end.saturating_sub(st.data.len())).is_err() {
        i.ulErrorRet = MMIOERR_OUTOFMEMORY;
        return MMIO_ERROR;
    }
    if st.data.len() < end {
        st.data.resize(end, 0);
    }
    st.data[st.pos..end].copy_from_slice(src);
    st.pos = end;
    i.lLogicalFilePos = st.pos as i32;
    n
}

fn seek(i: &mut MmioInfo, off: i32, origin: i32) -> i32 {
    let Some(st) = inst(i) else { return MMIO_ERROR };
    let base = match origin {
        0 => 0i64,
        1 => st.pos as i64,
        2 => st.data.len() as i64,
        _ => return MMIO_ERROR,
    };
    let p = base + off as i64;
    if p < 0 {
        return MMIO_ERROR;
    }
    let p = if st.writing { p as usize } else { (p as usize).min(st.data.len()) };
    st.pos = p;
    i.lLogicalFilePos = p as i32;
    p as i32
}

fn get_header(i: &mut MmioInfo, out: *mut u8, len: i32) -> i32 {
    let Some(st) = inst(i) else { return 0 };
    if st.writing || st.header.is_empty() {
        return 0;
    }
    if out.is_null() {
        i.ulErrorRet = MMIOERR_INVALID_STRUCTURE;
        return 0;
    }
    if (len as usize) < st.header.len() {
        i.ulErrorRet = MMIOERR_INVALID_BUFFER_LENGTH;
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(st.header.as_ptr(), out, st.header.len()) };
    st.header.len() as i32
}

fn set_header(i: &mut MmioInfo, src: *const u8, len: i32) -> i32 {
    let Some(st) = inst(i) else { return 0 };
    let need = header_len(st.kind);
    if !st.writing || need == 0 {
        return 0;
    }
    if src.is_null() {
        i.ulErrorRet = MMIOERR_INVALID_STRUCTURE;
        return 0;
    }
    // An image header may leave out palette entries it does not use.
    let min = if is_image(st.kind) { 92 } else { need };
    if (len as usize) < min {
        i.ulErrorRet = MMIOERR_INVALID_BUFFER_LENGTH;
        return 0;
    }
    let n = (len as usize).min(need);
    let mut h = vec![0u8; need];
    unsafe { core::ptr::copy_nonoverlapping(src, h.as_mut_ptr(), n) };
    st.header = h;
    st.header_set = true;
    n as i32
}

/// MMIOM_IDENTIFYFILE: by name (lParam1) or an open handle (lParam2).
fn identify_msg(kind: Kind, name: *const u8, h: u32) -> i32 {
    let mut head = vec![0u8; 4096];
    let (n, len);
    if h != 0 {
        let here = unsafe { mmioSeek(h, 0, 1) };
        let end = unsafe { mmioSeek(h, 0, 2) };
        unsafe { mmioSeek(h, 0, 0) };
        n = unsafe { mmioRead(h, head.as_mut_ptr(), head.len() as i32) };
        unsafe { mmioSeek(h, here.max(0), 0) };
        len = end.max(0) as u64;
    } else {
        if name.is_null() {
            return MMIO_ERROR;
        }
        let mut ci: MmioInfo = unsafe { core::mem::zeroed() };
        ci.fccIOProc = fourcc(b"DOS ");
        let t = unsafe { mmioOpen(name, &mut ci, MMIO_READ | MMIO_DENYNONE | MMIO_NOIDENTIFY) };
        if t == 0 {
            return MMIO_ERROR;
        }
        n = unsafe { mmioRead(t, head.as_mut_ptr(), head.len() as i32) };
        len = unsafe { mmioSeek(t, 0, 2) }.max(0) as u64;
        unsafe { mmioClose(t, 0) };
    }
    if n <= 0 {
        return MMIO_ERROR;
    }
    match identify(&head[..n as usize], len) {
        Some(f) if f == kind => MMIO_SUCCESS,
        _ => MMIO_ERROR,
    }
}

/// MMFORMATINFO.
#[repr(C)]
struct FormatInfo {
    ulStructLen: u32,
    fccIOProc: u32,
    ulIOProcType: u32,
    ulMediaType: u32,
    ulFlags: u32,
    szDefaultFormatExt: [u8; 5],
    ulCodePage: u32,
    ulLanguage: u32,
    lNameLength: i32,
}

fn ioproc(kind: Kind, pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    let (fcc, media, flags, ext, name) = info(kind);
    match msg {
        MMIOM_GETFORMATINFO => {
            let Some(f) = (unsafe { (p1 as *mut FormatInfo).as_mut() }) else { return MMIO_ERROR };
            let mut e = [0u8; 5];
            e[..ext.len()].copy_from_slice(ext);
            *f = FormatInfo {
                ulStructLen: core::mem::size_of::<FormatInfo>() as u32,
                fccIOProc: fcc,
                ulIOProcType: 2,
                ulMediaType: media,
                ulFlags: flags,
                szDefaultFormatExt: e,
                ulCodePage: 437,
                ulLanguage: 9,
                lNameLength: name.len() as i32,
            };
            return MMIO_SUCCESS;
        }
        MMIOM_GETFORMATNAME => {
            let n = name.len().min(p2.max(0) as usize);
            if p1 != 0 {
                unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), p1 as *mut u8, n) };
            }
            return n as i32;
        }
        MMIOM_IDENTIFYFILE => return identify_msg(kind, p1 as *const u8, p2 as u32),
        _ => {}
    }
    let Some(i) = (unsafe { pinfo.as_mut() }) else { return MMIO_ERROR };
    match msg {
        MMIOM_OPEN => open(kind, i, p1 as *const u8),
        MMIOM_CLOSE => close(i),
        MMIOM_READ => read(i, p1 as *mut u8, p2),
        MMIOM_WRITE => write(i, p1 as *const u8, p2),
        MMIOM_SEEK => seek(i, p1, p2),
        MMIOM_GETHEADER => get_header(i, p1 as *mut u8, p2),
        MMIOM_SETHEADER => set_header(i, p1 as *const u8, p2),
        MMIOM_QUERYHEADERLENGTH => {
            if i.ulTranslate & MMIO_TRANSLATEHEADER != 0 || !is_image(kind) {
                header_len(kind) as i32
            } else {
                inst(i).map_or(0, |s| s.header.len() as i32)
            }
        }
        // One image per file.
        MMIOM_QUERYIMAGECOUNT => {
            if let Some(c) = unsafe { (p1 as *mut i32).as_mut() } {
                *c = 1;
            }
            if is_image(kind) { MMIO_SUCCESS } else { MMIOM_UNSUPPORTED }
        }
        MMIOM_SETIMAGE => {
            if is_image(kind) && p1 == 0 { MMIO_SUCCESS } else { MMIO_ERROR }
        }
        _ => MMIOM_UNSUPPORTED,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn OS13IOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Os13, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn OS20IOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Os20, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn WI30IOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Wi30, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn JPEGIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Jpeg, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn GIFCIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Gif, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn PCXCIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Pcx, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn TFICIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::TiffIntelComp, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn TFIUIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::TiffIntel, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn TFMCIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::TiffMotComp, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn TFMUIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::TiffMot, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn TGACIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::TgaComp, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn TGAUIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Tga, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn AUIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Au, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn SNDIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Snd, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn VOCIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Voc, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn AIFFIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Aiff, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn IFFIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Iff, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn MIDIIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Midi, pinfo, msg, p1, p2)
}

#[unsafe(no_mangle)]
pub extern "C" fn AVIIOProc(pinfo: *mut MmioInfo, msg: u32, p1: i32, p2: i32) -> i32 {
    ioproc(Kind::Avi, pinfo, msg, p1, p2)
}
