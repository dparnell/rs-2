//! Binary resource files (`.RES`): one entry per resource, each
//! `FF type:u16`, the name (`FF id:u16`, or a NUL-terminated string),
//! the flags word, a u32 size and the data.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Name {
    Id(u16),
    Str(Vec<u8>),
}

impl Name {
    pub fn describe(&self) -> String {
        match self {
            Name::Id(n) => alloc::format!("{n}"),
            Name::Str(s) => s.iter().map(|&b| b as char).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub ty: u16,
    pub name: Name,
    pub flags: u16,
    pub data: Vec<u8>,
}

/// The `.RES` file of `res`.
pub fn write(res: &[Resource]) -> Vec<u8> {
    let mut o = Vec::new();
    for r in res {
        o.push(0xff);
        o.extend_from_slice(&r.ty.to_le_bytes());
        match &r.name {
            Name::Id(n) => {
                o.push(0xff);
                o.extend_from_slice(&n.to_le_bytes());
            }
            Name::Str(s) => {
                o.extend_from_slice(s);
                o.push(0);
            }
        }
        o.extend_from_slice(&r.flags.to_le_bytes());
        o.extend_from_slice(&(r.data.len() as u32).to_le_bytes());
        o.extend_from_slice(&r.data);
    }
    o
}

/// Reads a `.RES` file; `None` if it is not one.
pub fn read(d: &[u8]) -> Option<Vec<Resource>> {
    let mut v = Vec::new();
    let mut p = 0;
    let u16_at = |p: usize| Some(u16::from_le_bytes([*d.get(p)?, *d.get(p + 1)?]));
    while p < d.len() {
        // Some tools pad the file with a final ^Z or zeros.
        if d[p..].iter().all(|&c| c == 0 || c == 0x1a) {
            break;
        }
        if d[p] != 0xff {
            return None;
        }
        let ty = u16_at(p + 1)?;
        p += 3;
        let name = if *d.get(p)? == 0xff {
            let n = u16_at(p + 1)?;
            p += 3;
            Name::Id(n)
        } else {
            let n = d[p..].iter().position(|&c| c == 0)?;
            let s = d[p..p + n].to_vec();
            p += n + 1;
            Name::Str(s)
        };
        let flags = u16_at(p)?;
        let size = u32::from_le_bytes(d.get(p + 2..p + 6)?.try_into().ok()?) as usize;
        p += 6;
        let data = d.get(p..p.checked_add(size)?)?.to_vec();
        p += size;
        v.push(Resource { ty, name, flags, data });
    }
    Some(v)
}

/// True if `d` looks like a `.RES` file (for `rc FILE` without -r: a
/// script or compiled resources).
pub fn is_res(d: &[u8]) -> bool {
    d.first() == Some(&0xff) && read(d).is_some_and(|v| !v.is_empty())
}
