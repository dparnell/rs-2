//! The map (list) file: segments, groups, public symbols and the entry
//! point, laid out as IBM's linkers lay theirs out (MAPSYM reads it).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::link::{Addr, Linked, Linker, SymDef};

/// Address of a symbol as unit number (1-based) and offset.
fn sym_place(o: &Linked, s: usize) -> Option<(u16, u32, bool)> {
    let mut s = s;
    for _ in 0..16 {
        match o.syms[s].def {
            SymDef::Alias(t) => s = t,
            _ => break,
        }
    }
    let (ls, off) = match &o.syms[s].def {
        SymDef::Public { module, seg, offset, .. } => {
            let (ls, base) = o.mods[*module].seg(*seg)?;
            (ls, base + offset)
        }
        SymDef::Linker { lseg, offset } => (*lseg, *offset),
        SymDef::Absolute { frame, offset } => return Some((*frame, *offset, true)),
        _ => return None,
    };
    let seg = &o.lsegs[ls];
    if let Some(f) = seg.absolute {
        return Some((f, off, true));
    }
    let mut a = seg.offset + off;
    if !o.lx {
        // Offsets are relative to the unit (the group's frame).
        a = seg.offset + off;
    }
    Some((seg.unit as u16 + 1, a, false))
}

pub fn write(l: &Linker, o: &Linked, exe: &str) -> String {
    let _ = exe;
    let mut s = String::new();
    if o.lx {
        s.push_str(&l.msgs.log);
    }
    s.push_str("\r\n ");
    s.push_str(&o.module_name);
    s.push_str("\r\n\r\n");
    let wide = o.lx;
    if wide {
        s.push_str(" Start         Length     Name                   Class\r\n");
    } else {
        s.push_str(" Start     Length     Name                   Class\r\n");
    }
    for &i in &o.order {
        let ls = &o.lsegs[i];
        if ls.absolute.is_some() {
            continue;
        }
        let unit = ls.unit + 1;
        if wide {
            let bits = if ls.use32 { "32-bit" } else { "16-bit" };
            s.push_str(&format!(" {:04X}:{:08X} {:09X}H {:<22} {} {bits}\r\n", unit, ls.offset, ls.size, ls.name, ls.class));
        } else {
            s.push_str(&format!(" {:04X}:{:04X} {:05X}H     {:<22} {}\r\n", unit, ls.offset, ls.size, ls.name, ls.class));
        }
    }
    s.push_str("\r\n Origin   Group\r\n");
    for g in &o.groups {
        let first = g.lsegs.iter().map(|&i| &o.lsegs[i]).min_by_key(|x| (x.unit, x.offset));
        let (u, off) = match first {
            Some(f) if !(o.lx && g.name.eq_ignore_ascii_case("FLAT")) => (f.unit + 1, f.offset >> 4),
            _ => (0, 0),
        };
        s.push_str(&format!(" {:04X}:{:X}   {}\r\n", u, off, g.name));
    }
    if l.opt.map {
        let mut pubs: Vec<(String, u16, u32, bool, bool)> = Vec::new();
        for (i, sy) in o.syms.iter().enumerate() {
            if sy.local {
                continue;
            }
            match &sy.def {
                SymDef::Import { module, entry } => {
                    if !sy.referenced {
                        continue;
                    }
                    let what = match entry {
                        crate::omf::ImpEntry::Ordinal(n) => format!("{module}.{n}"),
                        crate::omf::ImpEntry::Name(n) => format!("{module}.{n}"),
                    };
                    pubs.push((format!("{:<20} ({what})", sy.name), 0, 0, true, true));
                }
                SymDef::Undefined | SymDef::Common { .. } => {}
                _ => {
                    if let Some((u, off, abs)) = sym_place(o, i) {
                        pubs.push((sy.name.clone(), u, off, abs, false));
                    }
                }
            }
        }
        let line = |p: &(String, u16, u32, bool, bool)| -> String {
            let tag = if p.4 {
                "  Imp  "
            } else if p.3 {
                "  Abs  "
            } else {
                "       "
            };
            if wide {
                format!(" {:04X}:{:08X}{}{}\r\n", p.1, p.2, tag, p.0)
            } else {
                format!(" {:04X}:{:04X}{}{}\r\n", p.1, p.2, tag, p.0)
            }
        };
        s.push_str("\r\n  Address         Publics by Name\r\n\r\n");
        let mut by_name = pubs.clone();
        by_name.sort_by(|a, b| a.0.to_ascii_uppercase().cmp(&b.0.to_ascii_uppercase()));
        for p in &by_name {
            s.push_str(&line(p));
        }
        s.push_str("\r\n  Address         Publics by Value\r\n\r\n");
        let mut by_val = pubs;
        by_val.sort_by_key(|p| (p.1, p.2));
        for p in &by_val {
            s.push_str(&line(p));
        }
    }
    match o.start {
        Some((u, off)) => {
            if wide {
                s.push_str(&format!("\r\nProgram entry point at {:04X}:{:08X}\r\n", u + 1, off));
            } else {
                s.push_str(&format!("\r\nProgram entry point at {:04X}:{:04X}\r\n", u + 1, off));
            }
        }
        None => {}
    }
    let _ = Addr::Undefined;
    s
}
