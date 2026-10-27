//! LZEXPAND: files compressed by Microsoft's COMPRESS (the "SZDD"
//! format: an 14-byte header, then LZSS with a 4 KB window that starts
//! filled with blanks), which setup programs copy and expand. A
//! compressed file opened with LZOpenFile or LZInit is expanded into
//! memory and read from there through an LZ handle; other files go
//! through to their DOS handles.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::Global;

const MAGIC: &[u8] = b"SZDD\x88\xf0\x27\x33";
/// LZ handles: LZ_BASE + index (above any DOS handle).
const LZ_BASE: u16 = 0x400;
const LZERROR_BADINHANDLE: u32 = 0xffff; // -1
const LZERROR_READ: u32 = 0xfffc; // -4
const LZERROR_WRITE: u32 = 0xfffb; // -5

struct Lz {
    data: Vec<u8>,
    pos: usize,
    /// The DOS handle it was read from.
    hf: u16,
}

static FILES: Global<Vec<Option<Lz>>> = Global::new(Vec::new());

fn lz(h: u16) -> Option<&'static mut Lz> {
    FILES.get().get_mut(h.checked_sub(LZ_BASE)? as usize)?.as_mut()
}

/// Expand SZDD data.
pub fn expand(src: &[u8]) -> Option<Vec<u8>> {
    if src.len() < 14 || &src[..8] != MAGIC {
        return None;
    }
    let size = u32::from_le_bytes([src[10], src[11], src[12], src[13]]) as usize;
    let mut out = Vec::with_capacity(size);
    let mut win = [b' '; 4096];
    let mut wp = 4096 - 16;
    let mut i = 14;
    while i < src.len() && out.len() < size {
        let flags = src[i];
        i += 1;
        for bit in 0..8 {
            if i >= src.len() || out.len() >= size {
                break;
            }
            if flags & (1 << bit) != 0 {
                let c = src[i];
                i += 1;
                out.push(c);
                win[wp] = c;
                wp = (wp + 1) & 4095;
            } else {
                if i + 1 >= src.len() {
                    break;
                }
                let (a, b) = (src[i] as usize, src[i + 1] as usize);
                i += 2;
                let mut p = a | (b & 0xf0) << 4;
                let n = (b & 0x0f) + 3;
                for _ in 0..n {
                    let c = win[p];
                    p = (p + 1) & 4095;
                    out.push(c);
                    win[wp] = c;
                    wp = (wp + 1) & 4095;
                }
            }
        }
    }
    out.truncate(size);
    Some(out)
}

/// The whole of a DOS file, from its start (the position is kept).
fn read_all(hf: u16) -> Option<Vec<u8>> {
    let mut pos = 0u32;
    let mut cur = 0u32;
    unsafe { os2::DosSetFilePtr(hf as u32, 0, 1, &mut cur) };
    unsafe { os2::DosSetFilePtr(hf as u32, 0, 0, &mut pos) };
    let mut v = Vec::new();
    let mut buf = alloc::vec![0u8; 0x8000];
    loop {
        let mut n = 0u32;
        if unsafe { os2::DosRead(hf as u32, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 {
            return None;
        }
        if n == 0 {
            break;
        }
        v.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosSetFilePtr(hf as u32, cur as i32, 0, &mut pos) };
    Some(v)
}

/// LZInit(hf): an LZ handle for a compressed file, else the file's own.
fn init(hf: u16) -> u32 {
    let mut head = [0u8; 8];
    let mut n = 0u32;
    let mut pos = 0u32;
    unsafe {
        os2::DosSetFilePtr(hf as u32, 0, 0, &mut pos);
        os2::DosRead(hf as u32, head.as_mut_ptr(), 8, &mut n);
        os2::DosSetFilePtr(hf as u32, 0, 0, &mut pos);
    }
    if n < 8 || head != *MAGIC {
        return hf as u32;
    }
    let Some(data) = read_all(hf).and_then(|v| expand(&v)) else { return LZERROR_READ };
    let f = FILES.get();
    let i = f.iter().position(|x| x.is_none()).unwrap_or_else(|| {
        f.push(None);
        f.len() - 1
    });
    f[i] = Some(Lz { data, pos: 0, hf });
    (LZ_BASE + i as u16) as u32
}

fn close(h: u16) {
    if let Some(i) = h.checked_sub(LZ_BASE) {
        if let Some(Some(l)) = FILES.get().get_mut(i as usize).map(|x| x.take()) {
            unsafe { os2::DosClose(l.hf as u32) };
            return;
        }
    }
    unsafe { os2::DosClose(h as u32) };
}

/// GetExpandedName(src, dst): the name a compressed file had (its last
/// character is in the header; COMPRESS -r leaves "_" in its place).
fn expanded_name(src: &[u8]) -> Vec<u8> {
    let mut name = src.to_vec();
    if let Some(data) = crate::module::read_file(src, 14) {
        if data.len() >= 10 && &data[..8] == MAGIC && data[9] != 0 && name.last() == Some(&b'_') {
            let n = name.len();
            name[n - 1] = data[9];
        }
    }
    name
}

/// LZCopy: write the expanded data (or a copy) to `dst`.
fn copy(src: u16, dst: u16) -> u32 {
    let h = init(src) as u16;
    let data = match lz(h) {
        Some(l) => core::mem::take(&mut l.data),
        None => match read_all(src) {
            Some(d) => d,
            None => return LZERROR_READ,
        },
    };
    if let Some(i) = h.checked_sub(LZ_BASE) {
        if let Some(x) = FILES.get().get_mut(i as usize) {
            *x = None;
        }
    }
    let mut n = 0u32;
    if unsafe { os2::DosWrite(dst as u32, data.as_ptr(), data.len() as u32, &mut n) } != 0 || n as usize != data.len() {
        return LZERROR_WRITE;
    }
    data.len() as u32
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // LZCopy(src, dst), CopyLZFile(src, dst)
        6401 | 6408 => {
            let (s, d) = (a.w(), a.w());
            copy(s, d)
        }
        // LZOpenFile(path, lpOfStruct, style)
        6402 => {
            let (p, of, style) = (a.l(), a.p(), a.w());
            let path = crate::far_str(p).unwrap_or(b"").to_vec();
            let hf = crate::kernel::open_file(&path, of, style);
            // Only files opened for reading are expanded (not OF_WRITE,
            // OF_READWRITE, OF_PARSE, OF_DELETE, OF_CREATE, OF_EXIST).
            if hf == 0xffff || style & 0x5303 != 0 {
                return Some(hf);
            }
            init(hf as u16)
        }
        // LZInit(hf)
        6403 => init(a.w()),
        // LZSeek(h, offset, origin)
        6404 => {
            let (h, off, origin) = (a.w(), a.l() as i32, a.w());
            match lz(h) {
                Some(l) => {
                    let base = match origin {
                        1 => l.pos as i64,
                        2 => l.data.len() as i64,
                        _ => 0,
                    };
                    let p = (base + off as i64).clamp(0, l.data.len() as i64);
                    l.pos = p as usize;
                    p as u32
                }
                None => {
                    let mut pos = 0u32;
                    if unsafe { os2::DosSetFilePtr(h as u32, off, origin as u32, &mut pos) } != 0 { LZERROR_BADINHANDLE } else { pos }
                }
            }
        }
        // LZRead(h, buf, n)
        6405 => {
            let (h, buf, n) = (a.w(), a.p(), a.w() as usize);
            match lz(h) {
                Some(l) => {
                    let k = n.min(l.data.len() - l.pos.min(l.data.len()));
                    unsafe { core::ptr::copy_nonoverlapping(l.data[l.pos..].as_ptr(), buf, k) };
                    l.pos += k;
                    k as u32
                }
                None => {
                    let mut r = 0u32;
                    if unsafe { os2::DosRead(h as u32, buf, n as u32, &mut r) } != 0 { LZERROR_READ } else { r }
                }
            }
        }
        // LZClose(h)
        6406 => {
            close(a.w());
            0
        }
        // LZStart(), LZDone()
        6407 => 1,
        6409 => 0,
        // GetExpandedName(src, dst)
        6410 => {
            let (s, d) = (a.l(), a.p());
            let n = expanded_name(crate::far_str(s).unwrap_or(b""));
            crate::user::copy_out(&n, d, 260);
            1
        }
        _ => return None,
    })
}
