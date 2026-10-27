//! Atoms and window properties. Local atoms (KERNEL's AddAtom...) are a
//! PM atom table of the process; global atoms (USER's GlobalAddAtom...)
//! and registered window messages are PM's system atom table, shared
//! with every process as Windows' global atoms are shared by all tasks.
//! Both give 0xC000 and up for strings; "#123" and MAKEINTATOM (a far
//! pointer with selector 0) are integer atoms.

use thunk16::Pascal;

use crate::Global;

unsafe extern "C" {
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.714"]
    fn WinCreateAtomTable(initial: u32, buckets: u32) -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(table: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.744"]
    fn WinFindAtom(table: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.721"]
    fn WinDeleteAtom(table: u32, atom: u32) -> u32;
    #[link_name = "PMWIN.802"]
    fn WinQueryAtomName(table: u32, atom: u32, buf: *mut u8, cch: u32) -> u32;
}

static LOCAL: Global<u32> = Global::new(0);

fn table(global: bool) -> u32 {
    if global {
        return unsafe { WinQuerySystemAtomTable() };
    }
    let t = LOCAL.get();
    if *t == 0 {
        *t = unsafe { WinCreateAtomTable(0, 0) };
    }
    *t
}

/// A name argument as PM wants it: a string, or an integer atom.
fn name(far: u32) -> *const u8 {
    if far >> 16 == 0 { (0xffff_0000 | (far & 0xffff)) as *const u8 } else { thunk16::flat(far) }
}

pub fn add(far: u32, global: bool) -> u16 {
    unsafe { WinAddAtom(table(global), name(far)) as u16 }
}

pub fn find(far: u32, global: bool) -> u16 {
    unsafe { WinFindAtom(table(global), name(far)) as u16 }
}

fn delete(atom: u16, global: bool) -> u16 {
    // 0 when it is gone (or still used).
    unsafe { WinDeleteAtom(table(global), atom as u32) };
    0
}

fn get_name(atom: u16, buf: *mut u8, n: i32, global: bool) -> u32 {
    if buf.is_null() || n <= 0 {
        return 0;
    }
    if atom < 0xc000 {
        // An integer atom's name is "#n".
        let s = alloc::format!("#{atom}");
        return crate::user::copy_out(s.as_bytes(), buf, n) as u32;
    }
    unsafe { WinQueryAtomName(table(global), atom as u32, buf, n as u32) }
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // InitAtomTable(size)
        1068 => {
            a.w();
            table(false);
            1
        }
        // FindAtom(name), AddAtom(name), GlobalAddAtom, GlobalFindAtom
        1069 | 2270 => find(a.l(), ord > 2000) as u32,
        1070 | 2268 => add(a.l(), ord > 2000) as u32,
        // DeleteAtom(atom), GlobalDeleteAtom(atom)
        1071 | 2269 => delete(a.w(), ord > 2000) as u32,
        // GetAtomName(atom, buf, n), GlobalGetAtomName
        1072 | 2271 => {
            let (atom, buf, n) = (a.w(), a.p(), a.i());
            get_name(atom, buf, n, ord > 2000)
        }
        // GetAtomHandle(atom): no local handles here.
        1073 => {
            a.w();
            0
        }
        // RegisterWindowMessage(name): a global atom.
        2118 => add(a.l(), true) as u32,
        // SetProp(hwnd, name, data), GetProp(hwnd, name), RemoveProp
        2026 => {
            let (h, n, v) = (a.w(), a.l(), a.w());
            let key = if n >> 16 == 0 { n as u16 } else { add(n, true) };
            let Some(w) = crate::user::wnd(h) else { return Some(0) };
            match w.props.iter_mut().find(|p| p.0 == key) {
                Some(p) => p.1 = v,
                None => w.props.push((key, v)),
            }
            1
        }
        2025 | 2024 => {
            let (h, n) = (a.w(), a.l());
            let key = if n >> 16 == 0 { n as u16 } else { find(n, true) };
            let Some(w) = crate::user::wnd(h) else { return Some(0) };
            let Some(i) = w.props.iter().position(|p| p.0 == key) else { return Some(0) };
            let v = w.props[i].1;
            if ord == 2024 {
                w.props.remove(i);
            }
            v as u32
        }
        _ => return None,
    })
}
