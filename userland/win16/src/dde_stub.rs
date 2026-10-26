//! A minimal DDEML until WIN16's own (stage 3, docs/WIN16.md): enough
//! for programs that set up a DDE server at startup (PFE, Calmira) to
//! get past it. Initialisation and name service succeed, string handles
//! are atoms, and no conversation ever connects.

use thunk16::Pascal;

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // DdeInitialize(lpidInst, pfnCallback, afCmd, ulRes): an instance.
        6102 => {
            let (p, _cb, _cmd, _res) = (a.p(), a.l(), a.l(), a.l());
            if !p.is_null() {
                unsafe { (p as *mut u32).write_unaligned(1) };
            }
            0
        }
        // DdeUninitialize(idInst)
        6103 => {
            a.l();
            1
        }
        // DdeGetLastError(idInst): DMLERR_NO_ERROR.
        6120 => {
            a.l();
            0
        }
        // DdeCreateStringHandle(idInst, psz, codepage): a global atom.
        6121 => {
            let (_i, s, _cp) = (a.l(), a.l(), a.w());
            crate::atom::add(s, true) as u32
        }
        // DdeFreeStringHandle(idInst, hsz), DdeKeepStringHandle
        6122 | 6124 => {
            let (_i, _h) = (a.l(), a.l());
            1
        }
        // DdeQueryString(idInst, hsz, buf, cch, codepage): the atom's name.
        6123 => {
            let (_i, h, buf, n, _cp) = (a.l(), a.l(), a.p(), a.l(), a.w());
            let mut tmp = [0u8; 256];
            let len = unsafe { WinQueryAtomName(WinQuerySystemAtomTable(), h & 0xffff, tmp.as_mut_ptr(), 256) } as usize;
            if buf.is_null() {
                return Some(len as u32);
            }
            crate::user::copy_out(&tmp[..len], buf, n as i32) as u32
        }
        // DdeNameService(idInst, hsz1, hsz2, afCmd): registered.
        6127 => {
            let (_i, _a, _b, _c) = (a.l(), a.l(), a.l(), a.w());
            1
        }
        // DdeConnect(idInst, service, topic, context): no server answers.
        6107 => {
            let (_i, _s, _t, _c) = (a.l(), a.l(), a.l(), a.l());
            0
        }
        _ => return None,
    })
}

unsafe extern "C" {
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.802"]
    fn WinQueryAtomName(table: u32, atom: u32, buf: *mut u8, cch: u32) -> u32;
}
