//! Pictures through MMIO, as Warp 4's image classes read them: any
//! format an installed image I/O procedure knows (MMPMMMIO.INI: OS/2 and
//! Windows bitmaps, JPEG, GIF, PCX, TIFF, Targa) is opened with
//! translation (MMIO_TRANSLATEHEADER | MMIO_TRANSLATEDATA) and read as an
//! OS/2 bitmap: the MMIMAGEHEADER's BITMAPINFOHEADER2 and colour table,
//! then the rows. MMIO.DLL is loaded when first needed, so that the
//! shell runs without multimedia.

use alloc::vec::Vec;

use crate::pm::*;

type MmioOpen = extern "C" fn(*const u8, *mut u8, u32) -> u32;
type MmioRead = extern "C" fn(u32, *mut u8, i32) -> i32;
type MmioClose = extern "C" fn(u32, u32) -> u32;
type MmioGetHeader = extern "C" fn(u32, *mut u8, i32, *mut i32, u32, u32) -> u32;

/// MMIO's entry points (mmioOpen 27, mmioRead 28, mmioClose 32,
/// mmioGetHeader 77), or None without MMIO.
fn mmio() -> Option<(MmioOpen, MmioRead, MmioClose, MmioGetHeader)> {
    static mut FNS: Option<(usize, usize, usize, usize)> = None;
    static mut TRIED: bool = false;
    unsafe {
        if !TRIED {
            TRIED = true;
            let mut hmod = 0u32;
            let mut fail = [0u8; 32];
            if os2::DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, b"MMIO\0".as_ptr(), &mut hmod) == 0 {
                let mut p = [0usize; 4];
                let ok = [27u32, 28, 32, 77].iter().zip(p.iter_mut()).all(|(&ord, f)| os2::DosQueryProcAddr(hmod, ord, core::ptr::null(), f) == 0);
                if ok {
                    FNS = Some((p[0], p[1], p[2], p[3]));
                }
            }
        }
        FNS.map(|(a, b, c, d)| (core::mem::transmute::<usize, MmioOpen>(a), core::mem::transmute::<usize, MmioRead>(b), core::mem::transmute::<usize, MmioClose>(c), core::mem::transmute::<usize, MmioGetHeader>(d)))
    }
}

/// A bitmap (HBITMAP, width, height) from a picture file in any format
/// MMIO reads.
pub fn load(path: &[u8]) -> Option<(u32, i32, i32)> {
    let Some((open, read, close, header)) = mmio() else {
        crate::util::debug("image: no MMIO");
        return None;
    };
    let name = crate::util::z(path);
    // MMIOINFO: ulTranslate (offset 64) = MMIO_TRANSLATEHEADER |
    // MMIO_TRANSLATEDATA; MMIO_READ | MMIO_DENYWRITE.
    let mut info = [0u8; 80];
    info[64..68].copy_from_slice(&3u32.to_le_bytes());
    let h = open(name.as_ptr(), info.as_mut_ptr(), 0x0004 | 0x0080);
    crate::util::debug(&alloc::format!("image: mmioOpen {:?} -> {:x}", core::str::from_utf8(path), h));
    if h == 0 {
        return None;
    }
    // MMIMAGEHEADER: the BITMAPINFOHEADER2 at 28, 256 RGB2 at 92.
    let mut hdr = alloc::vec![0u8; 1116];
    let mut n = 0i32;
    header(h, hdr.as_mut_ptr(), hdr.len() as i32, &mut n, 0, 0);
    let rd32 = |o: usize| u32::from_le_bytes([hdr[o], hdr[o + 1], hdr[o + 2], hdr[o + 3]]);
    let (cx, cy, bits) = (rd32(32), rd32(36), u16::from_le_bytes([hdr[42], hdr[43]]));
    crate::util::debug(&alloc::format!("image: header {} bytes, {}x{} {} bits", n, cx, cy, bits));
    if n < 92 || cx == 0 || cy == 0 || cx > 16384 || cy > 16384 || !matches!(bits, 1 | 4 | 8 | 24) {
        close(h, 0);
        return None;
    }
    let row = (cx as usize * bits as usize).div_ceil(32) * 4;
    let mut data: Vec<u8> = alloc::vec![0u8; row * cy as usize];
    let mut got = 0usize;
    while got < data.len() {
        let r = read(h, data[got..].as_mut_ptr(), (data.len() - got).min(1 << 20) as i32);
        if r <= 0 {
            break;
        }
        got += r as usize;
    }
    close(h, 0);
    if got < data.len() {
        return None;
    }
    // The header (cbFix 64) and its colour table, as BITMAPINFO2.
    let mut bih = [0u8; 16];
    bih[0..4].copy_from_slice(&16u32.to_le_bytes());
    bih[4..8].copy_from_slice(&cx.to_le_bytes());
    bih[8..12].copy_from_slice(&cy.to_le_bytes());
    bih[12..14].copy_from_slice(&1u16.to_le_bytes());
    bih[14..16].copy_from_slice(&bits.to_le_bytes());
    let mut bmi = hdr[28..].to_vec();
    bmi[0..4].copy_from_slice(&64u32.to_le_bytes());
    // CBM_INIT
    let hbm = unsafe { GpiCreateBitmap(0, bih.as_ptr(), 4, data.as_ptr(), bmi.as_ptr()) };
    (hbm != 0).then_some((hbm, cx as i32, cy as i32))
}
