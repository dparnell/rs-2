//! Memory suballocation: DosSubSetMem, DosSubAllocMem, DosSubFreeMem,
//! DosSubUnsetMem.
//!
//! The heap's control information lives inside the memory it manages, as
//! on OS/2, so a heap in shared memory can be used from every process that
//! has the memory (REXX does this). Layout at the base:
//!
//! ```text
//! +0 magic  +4 size of the heap  +8 DOSSUB_* flags  +12 first free block
//! ```
//!
//! Free blocks form an address-ordered list: `+0 next (offset from the
//! base, 0 = end)`, `+4 size`. Sizes are multiples of 8.

use super::{err, Ctx};
use crate::vm::{pag, PAGE};

const MAGIC: u32 = 0x4255_5344; // "DSUB"
const HEADER: u32 = 16;
const GRAIN: u32 = 8;

const DOSSUB_INIT: u32 = 0x01;
const DOSSUB_GROW: u32 = 0x02;
const DOSSUB_SPARSE_OBJ: u32 = 0x04;
const DOSSUB_SERIALIZE: u32 = 0x08;

struct Heap {
    base: u32,
    size: u32,
    flags: u32,
}

/// Make sure the pages of `[a, a + n)` are committed (sparse heaps).
fn commit(c: &Ctx, a: u32, n: u32) {
    let mut p = a & !(PAGE - 1);
    while p < a + n {
        let r = c.space().query(p, PAGE);
        if let Some((_, f)) = r {
            if f & pag::COMMIT == 0 {
                let _ = c.space().set_mem(p, PAGE, pag::COMMIT | pag::READ | pag::WRITE);
            }
        }
        p += PAGE;
    }
}

fn wr(c: &Ctx, h: &Heap, a: u32, v: u32) -> bool {
    if h.flags & DOSSUB_SPARSE_OBJ != 0 {
        commit(c, a, 4);
    }
    c.write_u32(a, v)
}

fn open(c: &Ctx, base: u32) -> Result<Heap, u32> {
    match (c.read_u32(base), c.read_u32(base + 4), c.read_u32(base + 8)) {
        (Some(MAGIC), Some(size), Some(flags)) => Ok(Heap { base, size, flags }),
        (None, ..) => Err(err::INVALID_ADDRESS),
        _ => Err(err::DOSSUB_BADSELECTOR),
    }
}

/// Insert a free block (offset `off`, `len` bytes) into the list,
/// merging it with its neighbours.
fn insert_free(c: &Ctx, h: &Heap, off: u32, len: u32) -> Result<(), u32> {
    let mut prev = 0u32; // 0 = the header's head field
    let mut cur = c.read_u32(h.base + 12).unwrap_or(0);
    while cur != 0 && cur < off {
        prev = cur;
        cur = c.read_u32(h.base + cur).unwrap_or(0);
    }
    let prev_len = if prev != 0 { c.read_u32(h.base + prev + 4).unwrap_or(0) } else { 0 };
    if (prev != 0 && prev + prev_len > off) || (cur != 0 && off + len > cur) {
        return Err(err::DOSSUB_OVERLAP);
    }
    let (mut start, mut size, mut next) = (off, len, cur);
    if cur != 0 && off + len == cur {
        size += c.read_u32(h.base + cur + 4).unwrap_or(0);
        next = c.read_u32(h.base + cur).unwrap_or(0);
    }
    if prev != 0 && prev + prev_len == off {
        start = prev;
        size += prev_len;
    } else {
        let link = if prev == 0 { h.base + 12 } else { h.base + prev };
        wr(c, h, link, start);
    }
    wr(c, h, h.base + start, next);
    wr(c, h, h.base + start + 4, size);
    Ok(())
}

/// DosSubSetMem(pbBase, flag, cb)
pub fn dos_sub_set_mem(c: &mut Ctx) -> u32 {
    let (base, flags, cb) = (c.arg(0), c.arg(1), c.arg(2));
    set_mem(c, base, flags, cb)
}

fn set_mem(c: &Ctx, base: u32, flags: u32, cb: u32) -> u32 {
    let cb = cb & !(GRAIN - 1);
    if flags & !(DOSSUB_INIT | DOSSUB_GROW | DOSSUB_SPARSE_OBJ | DOSSUB_SERIALIZE) != 0 {
        return err::DOSSUB_BADFLAG;
    }
    if base == 0 || base % GRAIN != 0 {
        return err::INVALID_ADDRESS;
    }
    if flags & DOSSUB_INIT != 0 {
        if cb < HEADER + GRAIN {
            return err::DOSSUB_BADSIZE;
        }
        let h = Heap { base, size: cb, flags };
        if flags & DOSSUB_SPARSE_OBJ != 0 {
            commit(c, base, HEADER + GRAIN);
        }
        if !(wr(c, &h, base, MAGIC) && wr(c, &h, base + 4, cb) && wr(c, &h, base + 8, flags) && wr(c, &h, base + 12, HEADER)) {
            return err::INVALID_ADDRESS;
        }
        wr(c, &h, base + HEADER, 0);
        wr(c, &h, base + HEADER + 4, cb - HEADER);
        return 0;
    }
    let h = match open(c, base) {
        Ok(h) => h,
        Err(e) => return e,
    };
    if flags & DOSSUB_GROW != 0 {
        if cb < h.size {
            return err::DOSSUB_SHRINK;
        }
        if cb > h.size {
            if let Err(e) = insert_free(c, &h, h.size, cb - h.size) {
                return e;
            }
            wr(c, &h, base + 4, cb);
        }
    }
    // Otherwise: attach to a heap another process set up.
    0
}

/// DosSubAllocMem(pbBase, ppb, cb)
pub fn dos_sub_alloc_mem(c: &mut Ctx) -> u32 {
    let (base, ppb, cb) = (c.arg(0), c.arg(1), c.arg(2));
    match alloc(c, base, cb) {
        Ok(a) => {
            c.write_u32(ppb, a);
            0
        }
        Err(e) => e,
    }
}

fn alloc(c: &Ctx, base: u32, cb: u32) -> Result<u32, u32> {
    let h = open(c, base)?;
    if cb == 0 || cb > h.size {
        return Err(err::DOSSUB_BADSIZE);
    }
    let want = cb.next_multiple_of(GRAIN);
    let mut link = base + 12;
    let mut cur = c.read_u32(link).unwrap_or(0);
    while cur != 0 {
        let next = c.read_u32(base + cur).unwrap_or(0);
        let size = c.read_u32(base + cur + 4).unwrap_or(0);
        if size >= want {
            if size - want >= GRAIN {
                let rest = cur + want;
                wr(c, &h, base + rest, next);
                wr(c, &h, base + rest + 4, size - want);
                wr(c, &h, link, rest);
            } else {
                wr(c, &h, link, next);
            }
            if h.flags & DOSSUB_SPARSE_OBJ != 0 {
                commit(c, base + cur, want);
            }
            return Ok(base + cur);
        }
        link = base + cur;
        cur = next;
    }
    Err(err::DOSSUB_NOMEM)
}

/// DosSubFreeMem(pbBase, pb, cb)
pub fn dos_sub_free_mem(c: &mut Ctx) -> u32 {
    let (base, pb, cb) = (c.arg(0), c.arg(1), c.arg(2));
    free(c, base, pb, cb)
}

fn free(c: &Ctx, base: u32, pb: u32, cb: u32) -> u32 {
    let h = match open(c, base) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let len = cb.next_multiple_of(GRAIN);
    if cb == 0 || pb < base + HEADER || (pb - base) % GRAIN != 0 || pb - base + len > h.size {
        return err::DOSSUB_BADSIZE;
    }
    match insert_free(c, &h, pb - base, len) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// DosSubUnsetMem(pbBase)
pub fn dos_sub_unset_mem(c: &mut Ctx) -> u32 {
    match open(c, c.arg(0)) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

// ---- 16-bit: a heap in one segment, offsets instead of pointers ----

fn seg_base(c: &Ctx, sel: u32) -> u32 {
    c.far16_to_flat(sel << 16)
}

// The 16-bit calls (OS/2 1.x) have a heap of their own kind, as IBM's
// DOSCALL1 (16-bit code there): at the start of the segment an 8-byte
// header (+0 offset of the first free block, 0 = none; +2 size of the
// area after the header; +4 a RAM semaphore), then the area; free blocks
// are `+0 next free block, +2 size`, in address order. Sizes are rounded
// up to multiples of 4.

/// Size rounded up to 4 bytes (0 for 0 and for sizes that wrap).
fn round4(cb: u32) -> u32 {
    (cb + 3) & 0xfffc
}

/// DosSubSet(sel, fFlags, usSize): 1 = initialise, 0 = new (larger)
/// size; a size of 0 is 64 KB, at least 12 bytes.
pub fn dos16_sub_set(c: &mut Ctx) -> u32 {
    let (sel, flags, cb) = (c.arg(0), c.arg(1), c.arg(2));
    if flags & 0xfffe != 0 {
        return err::DOSSUB_BADFLAG;
    }
    let size = round4(cb);
    if size != 0 && size < 12 {
        return err::DOSSUB_BADSIZE;
    }
    let size = (size as u16).wrapping_sub(8);
    let base = seg_base(c, sel);
    if base == 0 || base == 0xffff_ffff {
        return err::DOSSUB_BADSELECTOR;
    }
    let rd = |o: u16| c.read_u16(base + o as u32).unwrap_or(0);
    let wr = |o: u16, v: u16| c.write_u16(base + o as u32, v);
    if flags == 1 {
        for (o, v) in [(0, 8), (2, size), (4, 0), (6, 0), (8, 0), (10, size)] {
            if !wr(o, v) {
                return err::INVALID_ADDRESS;
            }
        }
        return 0;
    }
    let old = rd(2);
    if size <= old {
        return if size == old { 0 } else { err::DOSSUB_SHRINK };
    }
    // The last free block grows if it ends where the area did; else the
    // new room is a free block of its own.
    let mut last = 0;
    while rd(last) != 0 {
        last = rd(last);
    }
    wr(2, size);
    let (more, end) = (size - old, old + 8);
    if last != 0 && last + rd(last + 2) == end {
        wr(last + 2, rd(last + 2) + more);
    } else {
        wr(end, 0);
        wr(end + 2, more);
        wr(last, end);
    }
    0
}

/// DosSubAlloc(sel, pusOffset, usSize): the first free block large enough.
pub fn dos16_sub_alloc(c: &mut Ctx) -> u32 {
    let (sel, poff, cb) = (c.arg(0), c.ptr(1), c.arg(2));
    let base = seg_base(c, sel);
    let rd = |o: u16| c.read_u16(base + o as u32).unwrap_or(0);
    let wr = |o: u16, v: u16| c.write_u16(base + o as u32, v);
    c.write_u16(poff, 0);
    let size = round4(cb) as u16;
    if size == 0 {
        return err::DOSSUB_BADSIZE;
    }
    let (mut prev, mut cur, mut largest) = (0u16, rd(0), 0u16);
    while cur != 0 {
        let have = rd(cur + 2);
        if size <= have {
            if size == have {
                wr(prev, rd(cur));
            } else {
                let rest = cur + size;
                wr(prev, rest);
                wr(rest, rd(cur));
                wr(rest + 2, have - size);
            }
            c.write_u16(poff, cur);
            return 0;
        }
        largest = largest.max(have);
        prev = cur;
        cur = rd(cur);
    }
    // Not enough: the largest free block's size goes back.
    c.write_u16(poff, largest);
    err::DOSSUB_NOMEM
}

/// DosSubFree(sel, usOffset, usSize): back into the free list, merged
/// with its neighbours.
pub fn dos16_sub_free(c: &mut Ctx) -> u32 {
    let (sel, off, cb) = (c.arg(0), c.arg(1) as u16, c.arg(2));
    let base = seg_base(c, sel);
    let rd = |o: u16| c.read_u16(base + o as u32).unwrap_or(0);
    let wr = |o: u16, v: u16| c.write_u16(base + o as u32, v);
    let size = round4(cb) as u16;
    if size == 0 {
        return err::DOSSUB_BADSIZE;
    }
    // The free blocks before and after it.
    let (mut prev, mut next) = (0u16, rd(0));
    while next != 0 && next < off {
        prev = next;
        next = rd(next);
    }
    if off == 0 || next == off {
        return err::DOSSUB_OVERLAP;
    }
    let prev_end = if prev == 0 { 0 } else { prev + rd(prev + 2) };
    let end = off.wrapping_add(size);
    let fits = if next == 0 { end.wrapping_sub(1) <= rd(2).wrapping_add(7) } else { end <= next };
    if off < prev_end || !fits {
        return err::DOSSUB_OVERLAP;
    }
    let mut block = prev;
    if prev != 0 && prev_end == off {
        wr(prev + 2, rd(prev + 2) + size);
    } else {
        wr(prev, off);
        wr(off, next);
        wr(off + 2, size);
        block = off;
    }
    if next != 0 && block + rd(block + 2) == next {
        wr(block, rd(next));
        wr(block + 2, rd(block + 2) + rd(next + 2));
    }
    0
}
