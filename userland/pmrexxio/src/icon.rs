//! The window's icon, drawn here (rs2's modules carry no resources): a
//! sheet of paper with a large R.

use alloc::vec::Vec;

use crate::api::*;

const N: usize = 32;

/// The colour at (x, y), y counted from the top; None is transparent.
fn pixel(x: usize, y: usize) -> Option<u32> {
    const R: [&[u8; 5]; 7] = [b"####.", b"#...#", b"#...#", b"####.", b"#.#..", b"#..#.", b"#...#"];
    let (l, r, t, b) = (4, 27, 1, 30);
    let fold = 6;
    if x < l || x > r || y < t || y > b {
        return None;
    }
    // The folded corner at the top right.
    let (dx, dy) = (r - x, y - t);
    if dx + dy < fold {
        return None;
    }
    // The flap: a triangle below the fold, edged in black.
    if dx <= fold && dy <= fold {
        if dx + dy == fold || dx == fold || dy == fold {
            return Some(0x000000);
        }
        return Some(0xc0c0c0);
    }
    if x == l || x == r || y == t || y == b {
        return Some(0x000000);
    }
    // The letter: 5 x 7 cells of 3 x 3 pixels.
    let (gx, gy) = (x as isize - 9, y as isize - 6);
    if (0..15).contains(&gx) && (0..21).contains(&gy) && R[gy as usize / 3][gx as usize / 3] == b'#' {
        return Some(0x1838a0);
    }
    Some(0xffffff)
}

/// A GPI bitmap (rows bottom-up) of `rows` rows from `row(y_from_bottom)`.
fn bitmap(bpp: u16, rows: usize, row: impl Fn(usize, &mut [u8])) -> u32 {
    let stride = (N * bpp as usize).div_ceil(32) * 4;
    let mut bits = alloc::vec![0u8; stride * rows];
    for y in 0..rows {
        row(y, &mut bits[y * stride..(y + 1) * stride]);
    }
    // BITMAPINFOHEADER2 (16 bytes); for 1 bpp the colour table black, white.
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(&16u32.to_le_bytes());
    hdr[4..8].copy_from_slice(&(N as u32).to_le_bytes());
    hdr[8..12].copy_from_slice(&(rows as u32).to_le_bytes());
    hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
    hdr[14..16].copy_from_slice(&bpp.to_le_bytes());
    let mut info: Vec<u8> = hdr.to_vec();
    if bpp == 1 {
        info.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0]);
    }
    // CBM_INIT
    unsafe { GpiCreateBitmap(0, hdr.as_ptr(), 4, bits.as_ptr(), info.as_ptr()) }
}

/// The icon (HPOINTER), or 0.
pub fn make() -> u32 {
    // The mask: XOR in the lower half (all clear: a colour icon), AND in
    // the upper (set where transparent).
    let mask = bitmap(1, 2 * N, |y, row| {
        if y >= N {
            for x in 0..N {
                if pixel(x, N - 1 - (y - N)).is_none() {
                    row[x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
    });
    let colour = bitmap(24, N, |y, row| {
        for x in 0..N {
            let c = pixel(x, N - 1 - y).unwrap_or(0);
            row[x * 3] = c as u8;
            row[x * 3 + 1] = (c >> 8) as u8;
            row[x * 3 + 2] = (c >> 16) as u8;
        }
    });
    // POINTERINFO: fPointer, hotspot, hbmPointer, hbmColor, mini versions.
    let pi: [u32; 7] = [0, 16, 16, mask, colour, 0, 0];
    let h = unsafe { WinCreatePointerIndirect(HWND_DESKTOP, pi.as_ptr()) };
    unsafe {
        GpiDeleteBitmap(mask);
        GpiDeleteBitmap(colour);
    }
    h
}
