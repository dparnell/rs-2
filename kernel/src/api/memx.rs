//! More memory services: DosAliasMem, DosQueryMemState, DosReallocHuge,
//! DosLockSeg/DosUnlockSeg.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::sync::SpinLock;
use crate::vm::{self, pag, PageState, SegKind, SharedMem};

const OBJ_SELMAPALL: u32 = 0x800;
const SEL_CODE: u32 = 1;
const SEL_USE32: u32 = 2;

/// DosAliasMem(pb, cb, ppbAlias, fl): a second address (in the private
/// arena, tiled) for committed memory of the caller. The alias maps the
/// same physical pages; it must be freed (DosFreeMem) before the original.
pub fn dos_alias_mem(c: &mut Ctx) -> u32 {
    let (pb, cb, pp, fl) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    if cb == 0 || fl & !(pag::OBJ_TILE | OBJ_SELMAPALL | SEL_CODE | SEL_USE32) != 0 {
        return err::INVALID_PARAMETER;
    }
    let first = pb & !(vm::PAGE - 1);
    let end = (pb + cb).next_multiple_of(vm::PAGE);
    let r = {
        let mut sp = c.space();
        // Every page must be committed; bring them in so they have frames.
        let mut frames = Vec::new();
        let mut a = first;
        let mut ok = true;
        while a < end {
            let committed = sp.query(a, vm::PAGE).is_some_and(|(_, f)| f & pag::COMMIT != 0);
            if !committed || !sp.check(a, 1, false) {
                ok = false;
                break;
            }
            match crate::mm::paging::translate(sp.root, a as u64) {
                Some(p) => frames.push(p & !0xfff),
                None => {
                    ok = false;
                    break;
                }
            }
            a += vm::PAGE;
        }
        if !ok {
            Err(err::INVALID_ADDRESS)
        } else {
            let size = end - first;
            match sp.find_private(size) {
                Some(base) => {
                    let n = frames.len();
                    // Frames owned by the original object: the alias never
                    // frees them ("device" memory).
                    let obj = Arc::new(SharedMem {
                        base,
                        size,
                        name: None,
                        flags: pag::READ | pag::WRITE,
                        frames: SpinLock::new(frames),
                        pages: SpinLock::new(alloc::vec![PageState::Committed; n]),
                        device: true,
                    });
                    let prot = sp.region(pb).map_or(pag::READ, |r| r.flags & (pag::READ | pag::WRITE | pag::EXECUTE));
                    let kind = match (fl & SEL_CODE != 0, fl & SEL_USE32 != 0) {
                        (true, true) => SegKind::Code32,
                        (true, false) => SegKind::Code16,
                        _ => SegKind::Data,
                    };
                    sp.map_shared(obj, prot, kind);
                    Ok(base + (pb - first))
                }
                None => Err(err::NOT_ENOUGH_MEMORY),
            }
        }
    };
    match r {
        Ok(a) => {
            c.write_u32(pp, a);
            0
        }
        Err(e) => e,
    }
}

/// DosQueryMemState(pb, pcb, pFlag): the run of pages with the same
/// state, as PAG_PRESENT (1) / PAG_NPOUT (0) plus PAG_RESIDENT (0x10) or
/// PAG_INVALID (0) for uncommitted pages. Nothing is ever swapped out.
pub fn dos_query_mem_state(c: &mut Ctx) -> u32 {
    let (pb, pcb, pfl) = (c.arg(0), c.arg(1), c.arg(2));
    let Some(want) = c.read_u32(pcb) else { return err::INVALID_ADDRESS };
    let start = pb & !(vm::PAGE - 1);
    let want = want.max(1);
    let end = pb.saturating_add(want).next_multiple_of(vm::PAGE);
    let state = |sp: &crate::vm::AddressSpace, a: u32| -> u32 {
        match sp.query(a, vm::PAGE) {
            Some((_, f)) if f & pag::COMMIT != 0 => {
                let present = crate::mm::paging::translate(sp.root, a as u64).is_some();
                if present { 0x11 } else { 0x10 }
            }
            _ => 0,
        }
    };
    let (flags, n) = {
        let sp = c.space();
        if sp.region(start).is_none() {
            return err::INVALID_ADDRESS;
        }
        let first = state(&sp, start);
        let mut a = start + vm::PAGE;
        while a < end && sp.region(a).is_some() && state(&sp, a) == first {
            a += vm::PAGE;
        }
        (first, a - start)
    };
    c.write_u32(pcb, n);
    c.write_u32(pfl, flags);
    0
}

/// 16-bit DosReallocHuge(usNumSeg, usPartialSeg, sel): grow (or keep) a
/// huge allocation made by DosAllocHuge.
pub fn dos16_realloc_huge(c: &mut Ctx) -> u32 {
    let (nsegs, partial, sel) = (c.arg(0), c.arg(1), c.arg(2) as u16);
    let base = vm::tiled_flat(sel, 0);
    let size = nsegs * 0x10000 + partial;
    if size == 0 {
        return err::INVALID_PARAMETER;
    }
    let mut sp = c.space();
    let Some(cur) = sp.regions.get(&base).map(|r| r.size) else { return err::INVALID_SEGMENT_NUMBER };
    if size > cur && sp.resize(base, size).is_err() {
        return err::NOT_ENOUGH_MEMORY;
    }
    // Full 64 KB segments, then the partial one.
    for i in 0..nsegs {
        sp.set_tile_limit(base + i * 0x10000, 0xffff);
    }
    if partial != 0 {
        sp.set_tile_limit(base + nsegs * 0x10000, partial - 1);
    }
    0
}

/// DosLockSeg / DosUnlockSeg(sel): memory is never moved or discarded.
pub fn dos16_lock_seg(c: &mut Ctx) -> u32 {
    let sel = c.arg(0) as u16;
    if c.space().descriptor(sel) & (1 << 47) == 0 { err::INVALID_ACCESS } else { 0 }
}
