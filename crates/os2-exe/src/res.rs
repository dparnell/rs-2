//! Resources straight from an executable file (without loading it): the
//! (type, name) list and a resource's bytes, for LX (from the object's
//! pages, decoded) and NE (OS/2 resource segments, the last ones of the
//! segment table).

use alloc::vec::Vec;

use crate::{lx, ne, Format};

/// The (type, name) ids of the resources in `data`.
pub fn list(data: &[u8]) -> Vec<(u16, u16)> {
    match crate::detect(data) {
        Ok(Format::Lx { header }) => lx::Module::parse(data, header)
            .ok()
            .and_then(|m| m.resources().ok())
            .map(|v| v.into_iter().map(|r| (r.type_id, r.name_id)).collect())
            .unwrap_or_default(),
        Ok(Format::Ne { header }) => ne_table(data, header).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn ne_table(data: &[u8], header: usize) -> Option<Vec<(u16, u16)>> {
    let m = ne::Module::parse(data, header).ok()?;
    let table = header + m.header.resource_table as usize;
    let mut v = Vec::new();
    for i in 0..m.header.resource_count as usize {
        let o = table + i * 4;
        let t = u16::from_le_bytes([*data.get(o)?, *data.get(o + 1)?]);
        let n = u16::from_le_bytes([*data.get(o + 2)?, *data.get(o + 3)?]);
        v.push((t, n));
    }
    Some(v)
}

/// The bytes of resource (`ty`, `name`).
pub fn get(data: &[u8], ty: u16, name: u16) -> Option<Vec<u8>> {
    match crate::detect(data).ok()? {
        Format::Lx { header } => {
            let m = lx::Module::parse(data, header).ok()?;
            let r = m.resources().ok()?.into_iter().find(|r| r.type_id == ty && r.name_id == name)?;
            let obj = *m.objects().ok()?.get((r.object as usize).checked_sub(1)?)?;
            let ps = m.header.page_size.max(1) as usize;
            let (start, end) = (r.offset as usize, r.offset as usize + r.size as usize);
            if r.size > 16 << 20 {
                return None;
            }
            let mut out = Vec::with_capacity(r.size as usize);
            let mut page = alloc::vec![0u8; ps];
            for p in start / ps..end.div_ceil(ps) {
                if p as u32 >= obj.page_count {
                    break;
                }
                m.load_page(obj.page_index + p as u32, &mut page).ok()?;
                let a = start.max(p * ps) - p * ps;
                let b = end.min((p + 1) * ps) - p * ps;
                out.extend_from_slice(&page[a..b]);
            }
            (out.len() == r.size as usize).then_some(out)
        }
        Format::Ne { header } => {
            let m = ne::Module::parse(data, header).ok()?;
            let t = ne_table(data, header)?;
            let i = t.iter().position(|&(a, b)| a == ty && b == name)?;
            let segs = m.segments().ok()?;
            let first = segs.len().checked_sub(t.len())?;
            let seg = segs.get(first + i)?;
            m.segment_data(seg).ok().map(|d| d.to_vec())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    #[test]
    fn toolkit_sample_icon() {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/boot/toolkit/os2tk45/samples/pm/controls/style.exe");
        let Ok(data) = std::fs::read(p) else { return };
        let l = super::list(&data);
        // ICON IDR_MAIN etc.: RT_POINTER (1) resources.
        assert!(l.iter().any(|&(t, _)| t == 1), "{l:?}");
        let (t, n) = *l.iter().find(|&&(t, _)| t == 1).unwrap();
        let b = super::get(&data, t, n).unwrap();
        assert_eq!(&b[..2], b"BA");
        let i = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/boot/toolkit/os2tk45/samples/pm/controls/sty_main.ico")).unwrap();
        assert!(l.iter().any(|&(t, n)| t == 1 && super::get(&data, t, n).as_deref() == Some(&i[..])), "the main icon is in the EXE as it is in the .ICO");
    }
}
