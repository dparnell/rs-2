//! Self-checks of MMIO.DLL: writing a WAVE file with RIFF chunk calls,
//! reading it back through the WAVE I/O procedure (mmioGetHeader,
//! mmioRead, as OS/2's games do, also through DosLoadModule), descending
//! through its chunks, memory files, buffered I/O with direct buffer
//! access, and error codes. Each check prints PASS or FAIL; the last line
//! is "mmiotest done: N failure(s)" and the exit code N. Files are
//! written to the current drive's root and deleted at the end.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

#[repr(C)]
#[derive(Clone, Copy)]
struct MmioInfo {
    ulFlags: u32,
    fccIOProc: u32,
    pIOProc: usize,
    ulErrorRet: u32,
    cchBuffer: i32,
    pchBuffer: *mut u8,
    pchNext: *mut u8,
    pchEndRead: *mut u8,
    pchEndWrite: *mut u8,
    lBufOffset: i32,
    lDiskOffset: i32,
    aulInfo: [u32; 4],
    lLogicalFilePos: i32,
    ulTranslate: u32,
    fccChildIOProc: u32,
    pExtraInfoStruct: usize,
    hmmio: u32,
}

fn info() -> MmioInfo {
    unsafe { core::mem::zeroed() }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MmckInfo {
    ckid: u32,
    ckSize: u32,
    fccType: u32,
    ulDataOffset: u32,
    ulFlags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FormatInfo {
    ulStructLen: u32,
    fccIOProc: u32,
    ulIOProcType: u32,
    ulMediaType: u32,
    ulFlags: u32,
    ext: [u8; 5],
    ulCodePage: u32,
    ulLanguage: u32,
    lNameLength: i32,
}

type IoProc = unsafe extern "C" fn(*mut MmioInfo, u32, i32, i32) -> i32;

unsafe extern "C" {
    #[link_name = "MMIO.24"]
    fn mmioStringToFOURCC(s: *const u8, flags: u32) -> u32;
    #[link_name = "MMIO.25"]
    fn mmioGetLastError(h: u32) -> u32;
    #[link_name = "MMIO.26"]
    fn mmioInstallIOProc(fcc: u32, p: usize, flags: u32) -> usize;
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
    #[link_name = "MMIO.36"]
    fn mmioAscend(h: u32, ck: *mut MmckInfo, flags: u32) -> u32;
    #[link_name = "MMIO.37"]
    fn mmioDescend(h: u32, ck: *mut MmckInfo, parent: *const MmckInfo, flags: u32) -> u32;
    #[link_name = "MMIO.38"]
    fn mmioCreateChunk(h: u32, ck: *mut MmckInfo, flags: u32) -> u32;
    #[link_name = "MMIO.39"]
    fn mmioGetInfo(h: u32, info: *mut MmioInfo, flags: u32) -> u32;
    #[link_name = "MMIO.40"]
    fn mmioSetInfo(h: u32, info: *mut MmioInfo, flags: u32) -> u32;
    #[link_name = "MMIO.42"]
    fn mmioAdvance(h: u32, info: *mut MmioInfo, flags: u32) -> u32;
    #[link_name = "MMIO.74"]
    fn mmioQueryFormatCount(f: *const FormatInfo, n: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "MMIO.75"]
    fn mmioGetFormats(f: *const FormatInfo, max: i32, list: *mut FormatInfo, n: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "MMIO.76"]
    fn mmioQueryHeaderLength(h: u32, n: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "MMIO.77"]
    fn mmioGetHeader(h: u32, hdr: *mut u8, len: i32, read: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "MMIO.78"]
    fn mmioSetHeader(h: u32, hdr: *const u8, len: i32, written: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "MMIO.79"]
    fn mmioIdentifyFile(name: *const u8, info: *const MmioInfo, f: *mut FormatInfo, ss: *mut u32, r: u32, flags: u32) -> u32;
}

const MMIO_CREATE: u32 = 0x1;
const MMIO_READ: u32 = 0x4;
const MMIO_WRITE: u32 = 0x8;
const MMIO_READWRITE: u32 = 0x10;
const MMIO_ALLOCBUF: u32 = 0x400;
const MMIO_DELETE: u32 = 0x800;
const MMIO_DIRTY: u32 = 0x4000;
const MMIO_NOIDENTIFY: u32 = 0x40000;
const MMIO_CREATERIFF: u32 = 1;
const MMIO_FINDCHUNK: u32 = 4;
const MMIO_FINDRIFF: u32 = 8;
const MMIO_FINDLIST: u32 = 0x10;
const MMIO_INSTALLPROC: u32 = 1;
const MMIO_REMOVEPROC: u32 = 2;
const MMIO_FINDPROC: u32 = 4;
const SEEK_SET: i32 = 0;
const SEEK_CUR: i32 = 1;
const SEEK_END: i32 = 2;
const MMIOERR_CHUNKNOTFOUND: u32 = 6503;
const MMIOERR_INVALID_HANDLE: u32 = 6504;
const MMIOERR_UNBUFFERED: u32 = 6501;
const MMIOERR_READ_ONLY_FILE: u32 = 6514;
const MMIOERR_WRITE_ONLY_FILE: u32 = 6515;
const MMIOERR_CANNOTEXPAND: u32 = 6520;
const MMIOERR_EOF_SEEN: u32 = 6522;
const MMIOERR_INVALID_ACCESS_FLAG: u32 = 6523;

const fn fourcc(c: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*c)
}

const RIFF: u32 = fourcc(b"RIFF");
const WAVE: u32 = fourcc(b"WAVE");
const FMT: u32 = fourcc(b"fmt ");
const DATA: u32 = fourcc(b"data");
const DOS: u32 = fourcc(b"DOS ");
const MEM: u32 = fourcc(b"MEM ");

const WAV1: &[u8] = b"\\MMIOTST1.WAV\0";
const WAV2: &[u8] = b"\\MMIOTST2.WAV\0";
const BIN: &[u8] = b"\\MMIOTST3.BIN\0";

/// 8-bit mono 11025 Hz, 1001 samples.
const SAMPLES: usize = 1001;

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

fn samples() -> Vec<u8> {
    (0..SAMPLES).map(|i| (i * 7 % 251) as u8).collect()
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// Write WAV1 with the chunk calls through the DOS procedure.
fn write_wave() {
    unsafe {
        let h = mmioOpen(WAV1.as_ptr(), null_mut(), MMIO_CREATE | MMIO_WRITE);
        check("create a file", h != 0);
        if h == 0 {
            return;
        }
        let mut riff = MmckInfo { fccType: WAVE, ..Default::default() };
        check("mmioCreateChunk RIFF WAVE", mmioCreateChunk(h, &mut riff, MMIO_CREATERIFF) == 0);
        check("RIFF chunk: id, data offset, dirty", riff.ckid == RIFF && riff.ulDataOffset == 8 && riff.ulFlags == MMIO_DIRTY);
        let mut fmt = MmckInfo { ckid: FMT, ckSize: 16, ..Default::default() };
        check("mmioCreateChunk fmt", mmioCreateChunk(h, &mut fmt, 0) == 0 && fmt.ulDataOffset == 20);
        let mut f = [0u8; 16];
        f[0..2].copy_from_slice(&1u16.to_le_bytes());
        f[2..4].copy_from_slice(&1u16.to_le_bytes());
        f[4..8].copy_from_slice(&11025u32.to_le_bytes());
        f[8..12].copy_from_slice(&11025u32.to_le_bytes());
        f[12..14].copy_from_slice(&1u16.to_le_bytes());
        f[14..16].copy_from_slice(&8u16.to_le_bytes());
        check("mmioWrite format", mmioWrite(h, f.as_ptr(), 16) == 16);
        check("mmioAscend fmt", mmioAscend(h, &mut fmt, 0) == 0 && fmt.ckSize == 16);
        // The data chunk's size is left 0, for mmioAscend to correct.
        let mut data = MmckInfo { ckid: DATA, ..Default::default() };
        check("mmioCreateChunk data", mmioCreateChunk(h, &mut data, 0) == 0 && data.ulDataOffset == 44);
        let s = samples();
        check("mmioWrite samples", mmioWrite(h, s.as_ptr(), SAMPLES as i32) == SAMPLES as i32);
        check("mmioAscend data (size, odd padding)", mmioAscend(h, &mut data, 0) == 0 && data.ckSize == SAMPLES as u32);
        check("position after the padding", mmioSeek(h, 0, SEEK_CUR) == 44 + SAMPLES as i32 + 1);
        check("mmioAscend RIFF", mmioAscend(h, &mut riff, 0) == 0 && riff.ckSize == 4 + 24 + 8 + SAMPLES as u32 + 1);
        check("mmioClose", mmioClose(h, 0) == 0);
    }
}

/// Read WAV1 back as the games do: mmioOpen(name, NULL, MMIO_READ)
/// identifies the WAVE file; mmioGetHeader gives the MMAUDIOHEADER and
/// mmioRead the samples.
fn read_wave() {
    unsafe {
        let h = mmioOpen(WAV1.as_ptr(), null_mut(), MMIO_READ);
        check("open the WAVE file (identified)", h != 0);
        if h == 0 {
            return;
        }
        let mut len = 0;
        check("mmioQueryHeaderLength = 40", mmioQueryHeaderLength(h, &mut len, 0, 0) == 0 && len == 40);
        let mut hdr = [0u8; 40];
        let mut got = 0;
        check("mmioGetHeader", mmioGetHeader(h, hdr.as_mut_ptr(), 40, &mut got, 0, 0) == 0 && got == 40);
        check("header: length, audio media type", le32(&hdr, 0) == 40 && le32(&hdr, 8) == 2);
        check(
            "header: PCM, mono, 11025 Hz, 8 bits",
            le16(&hdr, 12) == 1 && le16(&hdr, 14) == 1 && le32(&hdr, 16) == 11025 && le16(&hdr, 24) == 1 && le16(&hdr, 26) == 8,
        );
        check("header: data length in bytes and ms", le32(&hdr, 32) == SAMPLES as u32 && le32(&hdr, 28) == 90);
        let mut small = [0u8; 40];
        check("mmioGetHeader with a short buffer fails", mmioGetHeader(h, small.as_mut_ptr(), 20, &mut got, 0, 0) != 0 && got == 0);
        let mut buf = vec![0u8; 2000];
        let n = mmioRead(h, buf.as_mut_ptr(), 2000);
        check("mmioRead reads the samples only", n == SAMPLES as i32 && buf[..SAMPLES] == samples()[..]);
        check(
            "mmioRead at the end: 0, MMIOERR_EOF_SEEN",
            mmioRead(h, buf.as_mut_ptr(), 10) == 0 && mmioGetLastError(h) == MMIOERR_EOF_SEEN,
        );
        check("mmioSeek in the samples", mmioSeek(h, 10, SEEK_SET) == 10);
        check("mmioRead after seeking", mmioRead(h, buf.as_mut_ptr(), 5) == 5 && buf[..5] == samples()[10..15]);
        check("mmioSeek to the end", mmioSeek(h, 0, SEEK_END) == SAMPLES as i32);
        check("mmioWrite to a read-only file fails", mmioWrite(h, buf.as_ptr(), 1) == -1 && mmioGetLastError(h) == MMIOERR_READ_ONLY_FILE);
        check("mmioClose", mmioClose(h, 0) == 0);

        let mut fi = FormatInfo::default();
        let mut ss = 0;
        let rc = mmioIdentifyFile(WAV1.as_ptr(), null(), &mut fi, &mut ss, 0, 0);
        check("mmioIdentifyFile: WAVE on DOS", rc == 0 && fi.fccIOProc == WAVE && ss == DOS && fi.ulMediaType == 2);
    }
}

/// The games load MMIO with DosLoadModule and get the calls by name.
fn by_name() {
    unsafe {
        let mut hmod = 0;
        let mut fail = [0u8; 32];
        check("DosLoadModule MMIO", DosLoadModule(fail.as_mut_ptr(), 32, b"MMIO\0".as_ptr(), &mut hmod) == 0);
        let mut addr = [0usize; 4];
        let names: [&[u8]; 4] = [b"mmioOpen\0", b"mmioGetHeader\0", b"mmioRead\0", b"mmioClose\0"];
        let ok = names.iter().zip(addr.iter_mut()).all(|(n, a)| DosQueryProcAddr(hmod, 0, n.as_ptr(), a) == 0);
        check("DosQueryProcAddr mmioOpen, mmioGetHeader, mmioRead, mmioClose", ok);
        let mut dummy = 0usize;
        check("DosQueryProcAddr mmioSet", DosQueryProcAddr(hmod, 0, b"mmioSet\0".as_ptr(), &mut dummy) == 0);
        if !ok {
            return;
        }
        let open: unsafe extern "C" fn(*const u8, *mut MmioInfo, u32) -> u32 = core::mem::transmute(addr[0]);
        let get: unsafe extern "C" fn(u32, *mut u8, i32, *mut i32, u32, u32) -> u32 = core::mem::transmute(addr[1]);
        let read: unsafe extern "C" fn(u32, *mut u8, i32) -> i32 = core::mem::transmute(addr[2]);
        let close: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(addr[3]);
        let h = open(WAV1.as_ptr(), null_mut(), MMIO_READ);
        let mut hdr = [0u8; 40];
        let mut got = 0;
        let ok = h != 0 && get(h, hdr.as_mut_ptr(), 40, &mut got, 0, 0) == 0;
        let len = le32(&hdr, 32) as usize;
        let mut buf = vec![0u8; len];
        let ok = ok && read(h, buf.as_mut_ptr(), len as i32) == len as i32 && buf == samples();
        check("play a sound the games' way", ok && close(h, 0) == 0);
    }
}

/// Descend through WAV1's chunks with the DOS procedure (no identification).
fn descend() {
    unsafe {
        let mut i = info();
        i.fccIOProc = DOS;
        let h = mmioOpen(WAV1.as_ptr(), &mut i, MMIO_READ);
        check("open with the DOS procedure", h != 0 && i.hmmio == h && i.fccIOProc == DOS);
        if h == 0 {
            return;
        }
        let mut riff = MmckInfo { fccType: WAVE, ..Default::default() };
        check("mmioDescend MMIO_FINDRIFF WAVE", mmioDescend(h, &mut riff, null(), MMIO_FINDRIFF) == 0);
        check("RIFF chunk", riff.ckid == RIFF && riff.ckSize == 4 + 24 + 8 + SAMPLES as u32 + 1 && riff.ulDataOffset == 8);
        let mut data = MmckInfo { ckid: DATA, ..Default::default() };
        check("mmioDescend data (skipping fmt)", mmioDescend(h, &mut data, &riff, MMIO_FINDCHUNK) == 0);
        check("data chunk", data.ckSize == SAMPLES as u32 && data.ulDataOffset == 44);
        let mut b = [0u8; 4];
        check("read the first samples", mmioRead(h, b.as_mut_ptr(), 4) == 4 && b[..] == samples()[..4]);
        check("mmioAscend data", mmioAscend(h, &mut data, 0) == 0 && mmioSeek(h, 0, SEEK_CUR) == 44 + SAMPLES as i32 + 1);
        // Searching with the parent starts again at its first sub-chunk.
        let mut fmt = MmckInfo { ckid: FMT, ..Default::default() };
        check("mmioDescend fmt", mmioDescend(h, &mut fmt, &riff, MMIO_FINDCHUNK) == 0 && fmt.ckSize == 16 && fmt.ulDataOffset == 20);
        let mut f = [0u8; 16];
        check("read the format", mmioRead(h, f.as_mut_ptr(), 16) == 16 && le32(&f, 4) == 11025);
        check("mmioAscend fmt", mmioAscend(h, &mut fmt, 0) == 0 && mmioSeek(h, 0, SEEK_CUR) == 36);
        let mut next = MmckInfo::default();
        check("mmioDescend without flags: the next chunk", mmioDescend(h, &mut next, null(), 0) == 0 && next.ckid == DATA);
        check("mmioAscend", mmioAscend(h, &mut next, 0) == 0);
        let mut list = MmckInfo { fccType: fourcc(b"INFO"), ..Default::default() };
        let pos = mmioSeek(h, 12, SEEK_SET);
        check(
            "mmioDescend MMIO_FINDLIST: MMIOERR_CHUNKNOTFOUND, position kept",
            mmioDescend(h, &mut list, &riff, MMIO_FINDLIST) == MMIOERR_CHUNKNOTFOUND && mmioSeek(h, 0, SEEK_CUR) == pos,
        );
        let mut c = MmckInfo { ckid: fourcc(b"junk"), ..Default::default() };
        check("mmioDescend a missing chunk", mmioDescend(h, &mut c, null(), MMIO_FINDCHUNK) == MMIOERR_CHUNKNOTFOUND);
        check("mmioClose", mmioClose(h, 0) == 0);
    }
}

/// A WAVE file written through the WAVE procedure with mmioSetHeader.
fn wave_proc_write() {
    unsafe {
        let mut i = info();
        i.fccIOProc = WAVE;
        let h = mmioOpen(WAV2.as_ptr(), &mut i, MMIO_CREATE | MMIO_READWRITE);
        check("create a WAVE file with the WAVE procedure", h != 0);
        if h == 0 {
            return;
        }
        let mut hdr = [0u8; 40];
        hdr[0..4].copy_from_slice(&40u32.to_le_bytes());
        hdr[8..12].copy_from_slice(&2u32.to_le_bytes());
        hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
        hdr[14..16].copy_from_slice(&2u16.to_le_bytes());
        hdr[16..20].copy_from_slice(&22050u32.to_le_bytes());
        hdr[20..24].copy_from_slice(&88200u32.to_le_bytes());
        hdr[24..26].copy_from_slice(&4u16.to_le_bytes());
        hdr[26..28].copy_from_slice(&16u16.to_le_bytes());
        let mut n = 0;
        check("mmioSetHeader", mmioSetHeader(h, hdr.as_ptr(), 40, &mut n, 0, 0) == 0 && n == 40);
        let data: Vec<u8> = (0..400u32).map(|i| i as u8).collect();
        check("mmioWrite samples", mmioWrite(h, data.as_ptr(), 400) == 400);
        check("mmioClose writes the header", mmioClose(h, 0) == 0);
        let h = mmioOpen(WAV2.as_ptr(), null_mut(), MMIO_READ);
        let mut got = 0;
        let mut back = [0u8; 40];
        let ok = h != 0 && mmioGetHeader(h, back.as_mut_ptr(), 40, &mut got, 0, 0) == 0;
        check(
            "read back: 16-bit stereo 22050 Hz, 400 bytes = 4 ms",
            ok && le16(&back, 14) == 2 && le32(&back, 16) == 22050 && le16(&back, 26) == 16 && le32(&back, 32) == 400 && le32(&back, 28) == 4,
        );
        let mut buf = [0u8; 400];
        check("read back the samples", mmioRead(h, buf.as_mut_ptr(), 400) == 400 && buf[..] == data[..]);
        mmioClose(h, 0);
    }
}

/// Memory files: a caller's fixed buffer and one MMIO allocates and grows.
fn memory_files() {
    unsafe {
        let mut mem = [0u8; 64];
        let mut i = info();
        i.fccIOProc = MEM;
        i.pchBuffer = mem.as_mut_ptr();
        i.cchBuffer = 64;
        let h = mmioOpen(null(), &mut i, MMIO_CREATE | MMIO_READWRITE);
        check("open a memory file in a caller's buffer", h != 0);
        let text = b"multimedia I/O";
        check("mmioWrite to the memory file", mmioWrite(h, text.as_ptr(), 14) == 14 && mem[..14] == text[..]);
        check("mmioSeek to its end", mmioSeek(h, 0, SEEK_END) == 14);
        check("mmioSeek past its end fails", mmioSeek(h, 15, SEEK_SET) == -1);
        let mut b = [0u8; 20];
        check("read it back", mmioSeek(h, 0, SEEK_SET) == 0 && mmioRead(h, b.as_mut_ptr(), 20) == 14 && b[..14] == text[..]);
        let big = [7u8; 100];
        check(
            "a fixed memory file does not grow",
            mmioWrite(h, big.as_ptr(), 100) == 50 && mmioWrite(h, big.as_ptr(), 1) == -1 && mmioGetLastError(h) == MMIOERR_CANNOTEXPAND,
        );
        let mut gi = info();
        check("mmioGetInfo", mmioGetInfo(h, &mut gi, 0) == 0 && gi.pchBuffer == mem.as_mut_ptr() && gi.hmmio == h);
        check("mmioAdvance on a fixed memory file: MMIOERR_UNBUFFERED", mmioAdvance(h, &mut gi, MMIO_WRITE) == MMIOERR_UNBUFFERED);
        check("mmioClose", mmioClose(h, 0) == 0);

        // MMIO's own buffer, 16 bytes growing by 16.
        let mut i = info();
        i.fccIOProc = MEM;
        i.cchBuffer = 16;
        i.aulInfo[0] = 16;
        let h = mmioOpen(null(), &mut i, MMIO_CREATE | MMIO_READWRITE);
        check("open a growing memory file", h != 0);
        // A RIFF file in memory.
        let mut riff = MmckInfo { fccType: fourcc(b"TEST"), ..Default::default() };
        let mut ck = MmckInfo { ckid: fourcc(b"abcd"), ..Default::default() };
        let payload: Vec<u8> = (0..100u8).collect();
        let ok = mmioCreateChunk(h, &mut riff, MMIO_CREATERIFF) == 0
            && mmioCreateChunk(h, &mut ck, 0) == 0
            && mmioWrite(h, payload.as_ptr(), 100) == 100
            && mmioAscend(h, &mut ck, 0) == 0
            && mmioAscend(h, &mut riff, 0) == 0;
        check("write RIFF chunks into the memory file (it grows)", ok && riff.ckSize == 4 + 8 + 100);
        check("its size", mmioSeek(h, 0, SEEK_END) == 120);
        mmioSeek(h, 0, SEEK_SET);
        let mut r = MmckInfo { fccType: fourcc(b"TEST"), ..Default::default() };
        let mut c = MmckInfo { ckid: fourcc(b"abcd"), ..Default::default() };
        let ok = mmioDescend(h, &mut r, null(), MMIO_FINDRIFF) == 0 && mmioDescend(h, &mut c, &r, MMIO_FINDCHUNK) == 0;
        let mut b = vec![0u8; 100];
        check(
            "descend and read in the memory file",
            ok && c.ckSize == 100 && mmioRead(h, b.as_mut_ptr(), 100) == 100 && b == payload,
        );
        let mut gi = info();
        let ok = mmioGetInfo(h, &mut gi, 0) == 0;
        let cap = gi.cchBuffer;
        check("mmioAdvance MMIO_WRITE grows the buffer", ok && mmioAdvance(h, &mut gi, MMIO_WRITE) == 0 && gi.cchBuffer == cap + 16);
        check("mmioClose", mmioClose(h, 0) == 0);
    }
}

/// Buffered files: mmioWrite and mmioRead through a small buffer, and
/// direct access with mmioGetInfo, mmioAdvance and mmioSetInfo.
fn buffered() {
    unsafe {
        let mut i = info();
        i.cchBuffer = 16;
        let h = mmioOpen(BIN.as_ptr(), &mut i, MMIO_CREATE | MMIO_READWRITE | MMIO_ALLOCBUF);
        check("create a buffered file", h != 0);
        if h == 0 {
            return;
        }
        let data: Vec<u8> = (0..100u8).map(|b| b ^ 0x5a).collect();
        check("mmioWrite through a 16-byte buffer", mmioWrite(h, data.as_ptr(), 100) == 100);
        check("mmioSeek back", mmioSeek(h, 37, SEEK_SET) == 37);
        let mut b = [0u8; 30];
        check("mmioRead through the buffer", mmioRead(h, b.as_mut_ptr(), 30) == 30 && b[..] == data[37..67]);
        check("mmioSeek to the end", mmioSeek(h, 0, SEEK_END) == 100);
        // Direct buffer access: append 16 bytes of 0xEE.
        let mut gi = info();
        check("mmioGetInfo", mmioGetInfo(h, &mut gi, 0) == 0 && gi.cchBuffer == 16);
        for k in 0..16 {
            *gi.pchNext.add(k) = 0xEE;
        }
        gi.pchNext = gi.pchNext.add(16);
        gi.ulFlags |= MMIO_DIRTY;
        check("mmioAdvance MMIO_WRITE", mmioAdvance(h, &mut gi, MMIO_WRITE) == 0);
        check("mmioSetInfo", mmioSetInfo(h, &mut gi, 0) == 0);
        check("mmioClose", mmioClose(h, 0) == 0);

        let mut i = info();
        let h = mmioOpen(BIN.as_ptr(), &mut i, MMIO_READ | MMIO_ALLOCBUF);
        check("open buffered for reading (8 KB buffer)", h != 0 && i.hmmio == h);
        let mut gi = info();
        check("mmioGetInfo fills the buffer", mmioGetInfo(h, &mut gi, 0) == 0 && gi.cchBuffer == 8192);
        let n = gi.pchEndRead as usize - gi.pchNext as usize;
        let got = core::slice::from_raw_parts(gi.pchNext, n);
        check("the buffer holds the file", n == 116 && got[..100] == data[..] && got[100..].iter().all(|&b| b == 0xEE));
        gi.pchNext = gi.pchEndRead;
        check("mmioAdvance MMIO_READ at the end", mmioAdvance(h, &mut gi, MMIO_READ) == 0 && gi.pchEndRead == gi.pchNext);
        check("mmioAdvance MMIO_WRITE on a read-only file", mmioAdvance(h, &mut gi, MMIO_WRITE) == MMIOERR_READ_ONLY_FILE);
        mmioClose(h, 0);

        let mut i = info();
        i.fccIOProc = DOS;
        let h = mmioOpen(BIN.as_ptr(), &mut i, MMIO_WRITE | MMIO_NOIDENTIFY);
        let mut b = [0u8; 4];
        check("mmioRead on a write-only file", h != 0 && mmioRead(h, b.as_mut_ptr(), 4) == -1 && mmioGetLastError(h) == MMIOERR_WRITE_ONLY_FILE);
        let mut gi = info();
        check("mmioGetInfo on an unbuffered file", mmioGetInfo(h, &mut gi, 0) == MMIOERR_UNBUFFERED);
        mmioClose(h, 0);
    }
}

unsafe extern "C" fn test_proc(_i: *mut MmioInfo, _msg: u32, _p1: i32, _p2: i32) -> i32 {
    -2
}

fn misc() {
    unsafe {
        check("mmioStringToFOURCC upper case", mmioStringToFOURCC(b"wave\0".as_ptr(), 1) == WAVE);
        check("mmioStringToFOURCC pads with blanks", mmioStringToFOURCC(b"ab\0".as_ptr(), 0) == fourcc(b"ab  "));
        check("mmioInstallIOProc MMIO_FINDPROC DOS", mmioInstallIOProc(DOS, 0, MMIO_FINDPROC) != 0);
        let p = test_proc as *const () as usize;
        let tst = fourcc(b"TST ");
        check("mmioInstallIOProc unknown: none", mmioInstallIOProc(tst, 0, MMIO_FINDPROC) == 0);
        check("mmioInstallIOProc install", mmioInstallIOProc(tst, p, MMIO_INSTALLPROC) == p);
        check("mmioInstallIOProc find it", mmioInstallIOProc(tst, 0, MMIO_FINDPROC) == p);
        check("mmioInstallIOProc remove", mmioInstallIOProc(tst, 0, MMIO_REMOVEPROC) == p && mmioInstallIOProc(tst, 0, MMIO_FINDPROC) == 0);
        let dos = mmioInstallIOProc(DOS, 0, MMIO_FINDPROC);
        let f: IoProc = core::mem::transmute(dos);
        let mut fi = FormatInfo::default();
        check("DOSIOProc MMIOM_GETFORMATINFO", f(null_mut(), 0xE0D, &mut fi as *mut _ as i32, 0) == 0 && fi.fccIOProc == DOS);

        let want = FormatInfo::default();
        let mut n = 0;
        check("mmioQueryFormatCount", mmioQueryFormatCount(&want, &mut n, 0, 0) == 0 && n >= 1);
        let mut list = [FormatInfo::default(); 4];
        let mut got = 0;
        let ok = mmioGetFormats(&want, 4, list.as_mut_ptr(), &mut got, 0, 0) == 0;
        check("mmioGetFormats lists WAVE", ok && got == n && list[..got as usize].iter().any(|f| f.fccIOProc == WAVE && &f.ext[..3] == b"WAV"));

        let mut i = info();
        let h = mmioOpen(b"\\NOSUCH.WAV\0".as_ptr(), &mut i, MMIO_READ);
        check("mmioOpen a missing file: the DosOpen error", h == 0 && (i.ulErrorRet == 2 || i.ulErrorRet == 110));
        let mut i = info();
        check(
            "mmioOpen MMIO_READ | MMIO_WRITE: MMIOERR_INVALID_ACCESS_FLAG",
            mmioOpen(WAV1.as_ptr(), &mut i, MMIO_READ | MMIO_WRITE) == 0 && i.ulErrorRet == MMIOERR_INVALID_ACCESS_FLAG,
        );
        check("mmioClose a bad handle", mmioClose(0x1234, 0) == MMIOERR_INVALID_HANDLE);
        check("mmioGetLastError of a bad handle", mmioGetLastError(0) == MMIOERR_INVALID_HANDLE);
        let mut fi = FormatInfo::default();
        let mut ss = 0;
        let rc = mmioIdentifyFile(BIN.as_ptr(), null(), &mut fi, &mut ss, 0, 0);
        check("mmioIdentifyFile: other files are DOS files", rc == 0 && fi.fccIOProc == DOS);
        for name in [WAV1, WAV2, BIN] {
            check("mmioOpen MMIO_DELETE", mmioOpen(name.as_ptr(), null_mut(), MMIO_DELETE) == 1);
        }
        check("the files are gone", mmioOpen(WAV1.as_ptr(), null_mut(), MMIO_READ) == 0);
    }
}

fn main(_s: &Startup) -> u32 {
    write_wave();
    read_wave();
    by_name();
    descend();
    wave_proc_write();
    memory_files();
    buffered();
    misc();
    let f = FAILS.load(Ordering::Relaxed);
    println!("mmiotest done: {f} failure(s)");
    f
}
