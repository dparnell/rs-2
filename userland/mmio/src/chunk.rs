//! RIFF chunks: mmioDescend, mmioAscend, mmioCreateChunk.
//!
//! A chunk's ulDataOffset is the file offset of its data; for RIFF and
//! LIST chunks that is the form type, which ckSize counts. Descending
//! with a parent searches the parent's sub-chunks from the first one (as
//! IBM's MMIO does); without one, from the current position to the end
//! of the file.

use crate::io::{read, seek, write};
use crate::*;

const MMIO_CREATERIFF: u32 = 1;
const MMIO_CREATELIST: u32 = 2;
const MMIO_FINDCHUNK: u32 = 4;
const MMIO_FINDRIFF: u32 = 8;
const MMIO_FINDLIST: u32 = 0x10;

fn is_form(id: u32) -> bool {
    id == FOURCC_RIFF || id == FOURCC_LIST
}

/// Read a chunk header (8 or 12 bytes): (bytes read, id, size, form).
fn header(h: &mut Handle, len: i32) -> (i32, u32, u32, u32) {
    let mut b = [0u8; 12];
    let got = read(h, b.as_mut_ptr(), len);
    let w = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    (got, w(0), w(4), w(8))
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioDescend(hmmio: u32, pck: *mut MmckInfo, pparent: *const MmckInfo, flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    h.info.ulErrorRet = 0;
    let Some(ck) = (unsafe { pck.as_mut() }) else { return err::INVALID_PARAMETER };
    let find = flags & (MMIO_FINDCHUNK | MMIO_FINDRIFF | MMIO_FINDLIST);
    if find.count_ones() > 1 {
        return err::INVALID_PARAMETER;
    }
    let start = h.info.lLogicalFilePos;
    let not_found = |h: &mut Handle| {
        seek(h, start, SEEK_SET);
        h.info.ulErrorRet = err::CHUNKNOTFOUND;
        err::CHUNKNOTFOUND
    };
    let (id, size);
    if find == 0 {
        // The chunk at the current position.
        let (got, cid, csize, form) = header(h, 12);
        if got < 8 {
            return not_found(h);
        }
        if is_form(cid) {
            ck.fccType = form;
        } else if got == 12 && seek(h, -4, SEEK_CUR) == -1 {
            return not_found(h);
        }
        (id, size) = (cid, csize);
    } else {
        let end = match unsafe { pparent.as_ref() } {
            Some(p) => {
                let end = p.ulDataOffset.wrapping_add(p.ckSize);
                let pos = h.info.lLogicalFilePos as u32;
                if pos < p.ulDataOffset || pos > end {
                    return not_found(h);
                }
                let first = p.ulDataOffset + 4;
                if pos != first && seek(h, first as i32, SEEK_SET) == -1 {
                    return not_found(h);
                }
                end
            }
            None => u32::MAX,
        };
        let mut found = None;
        while (h.info.lLogicalFilePos as u32) < end {
            // A plain chunk's header is 8 bytes; read 12 when it may be a form.
            let (len, extra) = if find == MMIO_FINDCHUNK && !is_form(ck.ckid) { (8, 4) } else { (12, 0) };
            let (got, cid, csize, form) = header(h, len);
            if got == 0 {
                break;
            }
            if got < 8 {
                return not_found(h);
            }
            let hit = match find {
                MMIO_FINDCHUNK => cid == ck.ckid,
                MMIO_FINDRIFF => cid == FOURCC_RIFF && form == ck.fccType,
                _ => cid == FOURCC_LIST && form == ck.fccType,
            };
            if hit {
                if is_form(cid) {
                    ck.fccType = form;
                } else if got == 12 && seek(h, -4, SEEK_CUR) == -1 {
                    return not_found(h);
                }
                found = Some((cid, csize));
                break;
            }
            if got < len {
                break;
            }
            let skip = (csize as i32).wrapping_sub(4).wrapping_add((csize & 1) as i32).wrapping_add(extra);
            if seek(h, skip, SEEK_CUR) == -1 {
                return not_found(h);
            }
        }
        let Some(f) = found else { return not_found(h) };
        (id, size) = f;
    }
    ck.ckid = id;
    ck.ckSize = size;
    ck.ulFlags = 0;
    let pos = h.info.lLogicalFilePos as u32;
    ck.ulDataOffset = if is_form(id) { pos - 4 } else { pos };
    0
}

/// mmioAscend: leave a chunk. For a chunk being written (MMIO_DIRTY) the
/// size is corrected in its header and an odd length padded.
#[unsafe(no_mangle)]
pub extern "C" fn mmioAscend(hmmio: u32, pck: *mut MmckInfo, _flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    h.info.ulErrorRet = 0;
    let Some(ck) = (unsafe { pck.as_mut() }) else { return err::INVALID_PARAMETER };
    let cannot = |h: &mut Handle| {
        h.info.ulErrorRet = err::CANNOTWRITE;
        err::CANNOTWRITE
    };
    if ck.ulFlags & (MMIO_DIRTY | 2) != 0 {
        let size = (h.info.lLogicalFilePos - ck.ulDataOffset as i32).max(0) as u32;
        if size & 1 != 0 && write(h, [0u8].as_ptr(), 1) == -1 {
            return cannot(h);
        }
        if ck.ckSize != size {
            if seek(h, ck.ulDataOffset as i32 - 4, SEEK_SET) == -1 {
                return cannot(h);
            }
            if write(h, size.to_le_bytes().as_ptr(), 4) == -1 {
                return cannot(h);
            }
            ck.ckSize = size;
        }
    }
    let end = ck.ulDataOffset.wrapping_add(ck.ckSize).wrapping_add(ck.ckSize & 1) as i32;
    if end != h.info.lLogicalFilePos && seek(h, end, SEEK_SET) == -1 {
        return cannot(h);
    }
    0
}

/// mmioCreateChunk: write a chunk header (with MMIO_CREATERIFF or
/// MMIO_CREATELIST a RIFF or LIST header with the form type) and descend
/// into it.
#[unsafe(no_mangle)]
pub extern "C" fn mmioCreateChunk(hmmio: u32, pck: *mut MmckInfo, flags: u32) -> u32 {
    let Some(h) = handle(hmmio) else { return err::INVALID_HANDLE };
    h.info.ulErrorRet = 0;
    if h.info.ulFlags & MMIO_READ != 0 {
        return err::CANNOTWRITE;
    }
    let Some(ck) = (unsafe { pck.as_mut() }) else { return err::INVALID_PARAMETER };
    if flags & MMIO_CREATERIFF != 0 && flags & MMIO_CREATELIST != 0 {
        return err::INVALID_PARAMETER;
    }
    let mut b = [0u8; 12];
    let len = if flags & (MMIO_CREATERIFF | MMIO_CREATELIST) == 0 {
        ck.fccType = 0;
        b[0..4].copy_from_slice(&ck.ckid.to_le_bytes());
        8
    } else {
        ck.ckid = if flags & MMIO_CREATERIFF != 0 { FOURCC_RIFF } else { FOURCC_LIST };
        b[0..4].copy_from_slice(&ck.ckid.to_le_bytes());
        b[8..12].copy_from_slice(&ck.fccType.to_le_bytes());
        12
    };
    b[4..8].copy_from_slice(&ck.ckSize.to_le_bytes());
    let start = h.info.lLogicalFilePos;
    if write(h, b.as_ptr(), len) != len {
        seek(h, start, SEEK_SET);
        return err::CANNOTWRITE;
    }
    ck.ulFlags = MMIO_DIRTY;
    ck.ulDataOffset = (start + 8) as u32;
    0
}
