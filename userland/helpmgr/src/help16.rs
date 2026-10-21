//! The 16-bit entry points (OS/2 1.x, `_pascal far`) for 16-bit programs.
//! Their HELPINIT has 16-bit fields where the 32-bit one has 32-bit ones,
//! and help tables in memory hold 16:16 pointers to their subtables.

use alloc::vec::Vec;
use thunk16::{flat, Pascal};

thunk16::entries16_pascal! {
    dispatch = dispatch16;
    WIN16CREATEHELPINSTANCE = 1, 8;
    WIN16DESTROYHELPINSTANCE = 2, 4;
    WIN16QUERYHELPINSTANCE = 3, 4;
    WIN16ASSOCIATEHELPINSTANCE = 4, 8;
    WIN16LOADHELPTABLE = 5, 8;
    WIN16CREATEHELPTABLE = 6, 8;
}

extern "C" fn dispatch16(idx: u32, args: *const u8) -> u32 {
    let mut a = Pascal::new(args, if matches!(idx, 2 | 3) { 4 } else { 8 });
    match idx {
        1 => {
            let (hab, p) = (a.l(), a.p());
            create(hab, p)
        }
        2 => crate::WinDestroyHelpInstance(a.l()),
        3 => crate::WinQueryHelpInstance(a.l()),
        4 => {
            let (help, app) = (a.l(), a.l());
            crate::WinAssociateHelpInstance(help, app)
        }
        // WinLoadHelpTable(hwndHelp, idHelpTable, hmod)
        5 => {
            let (help, id, hmod) = (a.l(), a.w() as u32, a.w() as u32);
            crate::WinLoadHelpTable(help, id, hmod)
        }
        6 => {
            let (help, t) = (a.l(), a.l());
            crate::WinCreateHelpTable(help, table(t))
        }
        _ => 0,
    }
}

fn rd16(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u16).read_unaligned() as u32 }
}
fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}

/// A pointer (a resource ID, selector 0xFFFF, stays one).
fn ptr(v: u32) -> u32 {
    if v == 0 || v >> 16 == 0xffff { v } else { flat(v) as u32 }
}

/// HELPINIT, 16-bit: cb, ulReturnCode, pszTutorialName, phtHelpTable,
/// hmodHelpTableModule, hmodAccelActionBarModule, idAccelTable,
/// idActionBar, pszHelpWindowTitle, usShowPanelId, pszHelpLibraryName
/// (cb, the modules, IDs and usShowPanelId 16-bit: 32 bytes).
fn create(hab: u32, p: *mut u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let v: [u32; 11] = [
        44,
        0,
        ptr(rd32(p, 6)),
        table(rd32(p, 10)),
        rd16(p, 14),
        rd16(p, 16),
        rd16(p, 18),
        rd16(p, 20),
        ptr(rd32(p, 22)),
        rd16(p, 26),
        ptr(rd32(p, 28)),
    ];
    let mut v = v;
    let r = crate::WinCreateHelpInstance(hab, v.as_mut_ptr());
    unsafe { (p.add(2) as *mut u32).write_unaligned(v[1]) };
    r
}

/// A help table: a resource ID as it is; one in memory copied with its
/// subtable pointers made flat (the copy is kept for the instance).
fn table(t: u32) -> u32 {
    if t == 0 || t >> 16 == 0xffff {
        return t;
    }
    let p = flat(t);
    let mut out: Vec<u8> = Vec::new();
    for k in 0..4096 {
        let id = rd16(p, 8 * k);
        out.extend_from_slice(&(id as u16).to_le_bytes());
        if id == 0 {
            out.extend_from_slice(&[0; 6]);
            break;
        }
        out.extend_from_slice(&ptr(rd32(p, 8 * k + 2)).to_le_bytes());
        out.extend_from_slice(&(rd16(p, 8 * k + 6) as u16).to_le_bytes());
    }
    out.leak().as_ptr() as u32
}
