//! Shared memory: DosAllocSharedMem family and 16-bit shared segments.
//!
//! Shared objects live in the shared arena, so they have the same address
//! (and tiled selector) in every process. The registry holds weak
//! references: an object disappears when the last process frees it.

use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::sync::SpinLock;
use crate::task::process;
use crate::vm::{self, pag, PageState, SegKind, SharedMem};

struct Entry {
    obj: Weak<SharedMem>,
}

static OBJECTS: SpinLock<Vec<Entry>> = SpinLock::new(Vec::new());

fn register(o: &Arc<SharedMem>) {
    let mut v = OBJECTS.lock();
    v.retain(|e| e.obj.strong_count() > 0);
    v.push(Entry { obj: Arc::downgrade(o) });
}

fn find_named(name: &str) -> Option<Arc<SharedMem>> {
    OBJECTS.lock().iter().filter_map(|e| e.obj.upgrade()).find(|o| o.name.as_deref() == Some(name))
}

fn find_at(addr: u32) -> Option<Arc<SharedMem>> {
    OBJECTS.lock().iter().filter_map(|e| e.obj.upgrade()).find(|o| addr >= o.base && addr < o.base + o.size)
}

/// Create a shared object and map it into the caller.
fn create(c: &Ctx, size: u32, flags: u32, name: Option<String>, seg: SegKind, limit: Option<u32>) -> Result<u32, u32> {
    let size = size.next_multiple_of(vm::PAGE);
    let base = vm::reserve_shared(size, None).ok_or(err::NOT_ENOUGH_MEMORY)?;
    let obj = SharedMem::new(base, size, flags, name);
    if flags & pag::COMMIT != 0 {
        obj.pages.lock().fill(PageState::Committed);
    }
    register(&obj);
    let mut sp = c.space();
    sp.map_shared(obj, flags & (pag::READ | pag::WRITE | pag::EXECUTE), seg);
    if let Some(l) = limit {
        sp.set_seg_limit(base, l);
    }
    Ok(base)
}

fn map_into(space: &mut vm::AddressSpace, obj: Arc<SharedMem>, prot: u32, seg: SegKind) -> Result<(), u32> {
    if space.region(obj.base).is_some() {
        return Ok(()); // already mapped
    }
    space.map_shared(obj, prot, seg);
    Ok(())
}

fn shared_name(c: &Ctx, p: u32) -> Result<Option<String>, u32> {
    if p == 0 {
        return Ok(None);
    }
    let n = c.read_str(p, 260).ok_or(err::INVALID_ADDRESS)?.to_ascii_uppercase();
    if !n.starts_with("\\SHAREMEM\\") {
        return Err(err::INVALID_NAME);
    }
    Ok(Some(n))
}

pub fn dos_alloc_shared_mem(c: &mut Ctx) -> u32 {
    // DosAllocSharedMem(ppb, pszName, cb, flag)
    let (ppb, pname, cb, flags) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let name = match shared_name(c, pname) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if cb == 0 {
        return err::INVALID_PARAMETER;
    }
    if name.is_none() && flags & (pag::OBJ_GIVEABLE | pag::OBJ_GETTABLE) == 0 {
        return err::INVALID_PARAMETER;
    }
    if let Some(n) = &name {
        if find_named(n).is_some() {
            return 183; // ERROR_ALREADY_EXISTS
        }
    }
    match create(c, cb, flags, name, SegKind::Data, None) {
        Ok(base) => {
            c.write_u32(ppb, base);
            0
        }
        Err(e) => e,
    }
}

pub fn dos_get_named_shared_mem(c: &mut Ctx) -> u32 {
    // DosGetNamedSharedMem(ppb, pszName, flag)
    let (ppb, pname, flags) = (c.arg(0), c.arg(1), c.arg(2));
    let name = match shared_name(c, pname) {
        Ok(Some(n)) => n,
        Ok(None) => return err::INVALID_NAME,
        Err(e) => return e,
    };
    let Some(obj) = find_named(&name) else { return err::FILE_NOT_FOUND };
    let base = obj.base;
    let r = map_into(&mut c.space(), obj, flags & 7, SegKind::Data);
    if let Err(e) = r {
        return e;
    }
    c.write_u32(ppb, base);
    0
}

pub fn dos_get_shared_mem(c: &mut Ctx) -> u32 {
    // DosGetSharedMem(pb, flag): attach a gettable object.
    let (pb, flags) = (c.arg(0), c.arg(1));
    let Some(obj) = find_at(pb) else { return err::INVALID_ADDRESS };
    if obj.flags & pag::OBJ_GETTABLE == 0 {
        return err::ACCESS_DENIED;
    }
    let r = map_into(&mut c.space(), obj, flags & 7, SegKind::Data);
    r.err().unwrap_or(0)
}

pub fn dos_give_shared_mem(c: &mut Ctx) -> u32 {
    // DosGiveSharedMem(pb, pid, flag)
    let (pb, pid, flags) = (c.arg(0), c.arg(1), c.arg(2));
    let Some(obj) = find_at(pb) else { return err::INVALID_ADDRESS };
    if obj.flags & pag::OBJ_GIVEABLE == 0 {
        return err::ACCESS_DENIED;
    }
    let Some(p) = process::by_pid(pid) else { return err::INVALID_PROCID };
    let r = map_into(&mut p.space.lock(), obj, flags & 7, SegKind::Data);
    r.err().unwrap_or(0)
}

// ---- 16-bit shared segments ----

pub fn dos16_alloc_shr_seg(c: &mut Ctx) -> u32 {
    // DosAllocShrSeg(usSize, pszSegName, psel)
    let size = match c.arg(0) {
        0 => 0x10000,
        n => n,
    };
    let name = match shared_name(c, c.ptr(1)) {
        Ok(Some(n)) => n,
        Ok(None) => return err::INVALID_NAME,
        Err(e) => return e,
    };
    if find_named(&name).is_some() {
        return 183;
    }
    let flags = pag::READ | pag::WRITE | pag::COMMIT | pag::OBJ_GETTABLE | pag::OBJ_GIVEABLE;
    match create(c, size, flags, Some(name), SegKind::Data, Some(size - 1)) {
        Ok(base) => {
            c.write_u16(c.ptr(2), vm::tile_selector(base));
            0
        }
        Err(e) => e,
    }
}

pub fn dos16_get_shr_seg(c: &mut Ctx) -> u32 {
    let name = match shared_name(c, c.ptr(0)) {
        Ok(Some(n)) => n,
        Ok(None) => return err::INVALID_NAME,
        Err(e) => return e,
    };
    let Some(obj) = find_named(&name) else { return err::FILE_NOT_FOUND };
    let base = obj.base;
    let r = map_into(&mut c.space(), obj, pag::READ | pag::WRITE, SegKind::Data);
    if let Err(e) = r {
        return e;
    }
    c.write_u16(c.ptr(1), vm::tile_selector(base));
    0
}

pub fn dos16_give_seg(c: &mut Ctx) -> u32 {
    // DosGiveSeg(sel, pid, pselRecipient)
    let (sel, pid, pout) = (c.arg(0) as u16, c.arg(1), c.ptr(2));
    let base = vm::tiled_flat(sel, 0);
    let Some(obj) = find_at(base) else { return err::INVALID_ACCESS };
    let Some(p) = process::by_pid(pid) else { return err::INVALID_PROCID };
    let r = map_into(&mut p.space.lock(), obj, pag::READ | pag::WRITE, SegKind::Data);
    if let Err(e) = r {
        return e;
    }
    c.write_u16(pout, sel);
    0
}

pub fn dos16_get_seg(c: &mut Ctx) -> u32 {
    let base = vm::tiled_flat(c.arg(0) as u16, 0);
    let Some(obj) = find_at(base) else { return err::INVALID_ACCESS };
    let r = map_into(&mut c.space(), obj, pag::READ | pag::WRITE, SegKind::Data);
    r.err().unwrap_or(0)
}

/// DosAllocSeg with SEG_GIVEABLE / SEG_GETTABLE: a shared segment.
pub fn alloc_shared_seg(c: &Ctx, size: u32, giveable: bool, gettable: bool) -> Result<u16, u32> {
    let mut flags = pag::READ | pag::WRITE | pag::COMMIT;
    if giveable {
        flags |= pag::OBJ_GIVEABLE;
    }
    if gettable {
        flags |= pag::OBJ_GETTABLE;
    }
    create(c, size, flags, None, SegKind::Data, Some(size - 1)).map(vm::tile_selector)
}
