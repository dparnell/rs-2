//! Resources: DosGetResource, DosQueryResourceSize, DosFreeResource.

use alloc::sync::Arc;

use super::{err, Ctx};
use crate::loader::{self, Format, Module};
use crate::vm;

/// OS/2 reports a missing resource as ERROR_INVALID_PARAMETER.
const ERROR_NOT_FOUND: u32 = 87;

/// (linear address, size) of a resource.
pub fn find(m: &Module, ty: u16, name: u16) -> Option<(u32, u32)> {
    let image = m.image.as_ref()?;
    match m.format {
        Format::Lx => {
            let Ok(os2_exe::Format::Lx { header }) = os2_exe::detect(image) else { return None };
            let lx = os2_exe::lx::Module::parse(image, header).ok()?;
            let r = lx.resources().ok()?.into_iter().find(|r| r.type_id == ty && r.name_id == name)?;
            let obj = m.objects.get((r.object as usize).checked_sub(1)?)?;
            Some((obj.base + r.offset, r.size))
        }
        Format::Ne => {
            // OS/2 NE: (type, name) pairs; the resources are the last
            // segments of the segment table.
            let Ok(os2_exe::Format::Ne { header }) = os2_exe::detect(image) else { return None };
            let ne = os2_exe::ne::Module::parse(image, header).ok()?;
            let n = ne.header.resource_count as usize;
            let table = header + ne.header.resource_table as usize;
            let first = m.objects.len().checked_sub(n)?;
            for i in 0..n {
                let o = table + i * 4;
                let t = u16::from_le_bytes([*image.get(o)?, *image.get(o + 1)?]);
                let nm = u16::from_le_bytes([*image.get(o + 2)?, *image.get(o + 3)?]);
                if t == ty && nm == name {
                    let obj = m.objects.get(first + i)?;
                    return Some((obj.base, obj.limit.map_or(obj.size, |l| l + 1)));
                }
            }
            None
        }
        Format::Virtual => None,
    }
}

fn module(c: &Ctx, h: u32) -> Option<Arc<Module>> {
    if h == 0 { Some(c.process.exe.clone()) } else { loader::by_handle(h as u16) }
}

pub fn dos_get_resource(c: &mut Ctx) -> u32 {
    // 32: (hmod, idType, idName, ppb)
    let (h, ty, name, pp) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let Some(m) = module(c, h) else { return err::INVALID_HANDLE };
    match find(&m, ty as u16, name as u16) {
        Some((addr, _)) => {
            c.write_u32(pp, addr);
            0
        }
        None => ERROR_NOT_FOUND,
    }
}

pub fn dos16_get_resource(c: &mut Ctx) -> u32 {
    // (hmod, idType, idName, psel)
    let (h, ty, name, psel) = (c.arg(0), c.arg(1), c.arg(2), c.ptr(3));
    let Some(m) = module(c, h) else { return err::INVALID_HANDLE };
    match find(&m, ty as u16, name as u16) {
        Some((addr, _)) => {
            c.write_u16(psel, vm::tile_selector(addr));
            0
        }
        None => ERROR_NOT_FOUND,
    }
}

pub fn dos16_get_resource2(c: &mut Ctx) -> u32 {
    // (hmod, idType, idName, PVOID FAR *ppData)
    let (h, ty, name, pp) = (c.arg(0), c.arg(1), c.arg(2), c.ptr(3));
    let Some(m) = module(c, h) else { return err::INVALID_HANDLE };
    match find(&m, ty as u16, name as u16) {
        Some((addr, _)) => {
            c.write_u32(pp, (vm::tile_selector(addr) as u32) << 16 | (addr & 0xffff));
            0
        }
        None => ERROR_NOT_FOUND,
    }
}

pub fn dos_query_resource_size(c: &mut Ctx) -> u32 {
    // (hmod, idt, idn, pulsize)
    let (h, ty, name, ps) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let Some(m) = module(c, h) else { return err::INVALID_HANDLE };
    match find(&m, ty as u16, name as u16) {
        Some((_, size)) => {
            c.write_u32(ps, size);
            0
        }
        None => ERROR_NOT_FOUND,
    }
}

pub fn dos_free_resource(_c: &mut Ctx) -> u32 {
    0
}
