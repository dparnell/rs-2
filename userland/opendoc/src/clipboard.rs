//! ODClipboard on PM's clipboard.
//!
//! The clipboard's contents are a storage unit in a scratch draft. The
//! clipboard's methods:
//!
//! - **ExportClipboard** puts the contents on PM's clipboard. Each value
//!   whose type was registered with RegisterClipboardFormat(type, format,
//!   info) goes there in that PM format, as IBM's parts expect (MMODPTS
//!   registers `CF_WAVE` as format 12). CF_TEXT also carries the units'
//!   text values. The whole contents, with the units their references
//!   name, go there in rs2's private format `rs2 OpenDoc storage units`,
//!   so a part copied in one document can be pasted into another, in any
//!   process.
//! - **GetContentStorageUnit** reads PM's clipboard back when another
//!   program (or another process) has changed it since our last export:
//!   rs2's private format if it is there, else CF_TEXT as a text value.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::rt::*;
use crate::storage::{self, Prop, Value};
use crate::*;

unsafe extern "C" {
    #[link_name = "PMWIN.793"]
    fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.707"]
    fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    fn WinSetClipbrdData(hab: u32, data: usize, fmt: u32, info: u32) -> u32;
    #[link_name = "PMWIN.806"]
    fn WinQueryClipbrdData(hab: u32, fmt: u32) -> usize;
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(table: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(hwnd: u32) -> u32;
}

const CF_TEXT: u32 = 1;
const CFI_POINTER: u32 = 0x400;
pub const TYPE_TEXT: &[u8] = b"rs2:Type:Text";
pub const PROP_CONTENTS: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:Contents";
const PRIVATE: &[u8] = b"rs2 OpenDoc storage units\0";

/// Value types registered for PM formats: (type, format, info).
static mut FORMATS: Vec<(Vec<u8>, u32, u32)> = Vec::new();
/// The tag of our last export (in the private data), so that our own
/// clipboard is not read back.
static mut EXPORTED: u32 = 0;

#[allow(static_mut_refs)]
fn formats() -> &'static mut Vec<(Vec<u8>, u32, u32)> {
    unsafe {
        if FORMATS.is_empty() {
            FORMATS.push((TYPE_TEXT.to_vec(), CF_TEXT, CFI_POINTER));
            FORMATS.push((b"CF_TEXT".to_vec(), CF_TEXT, CFI_POINTER));
        }
        &mut FORMATS
    }
}

pub fn register(ty: &[u8], fmt: u32, info: u32) {
    let f = formats();
    match f.iter_mut().find(|e| e.0 == ty) {
        Some(e) => {
            e.1 = fmt;
            e.2 = info;
        }
        None => f.push((ty.to_vec(), fmt, info)),
    }
}

fn private_format() -> u32 {
    unsafe { WinAddAtom(WinQuerySystemAtomTable(), PRIVATE.as_ptr()) & 0xffff }
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(1) }
}

/// Giveable shared memory holding `data` (PM's clipboard owns it after).
fn shared(data: &[u8]) -> usize {
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), data.len().max(1) as u32, 0x1 | 0x2 | 0x10 | 0x200) } != 0 {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len()) };
    p as usize
}

/// The units a clipboard unit's strong references reach, with it.
fn reachable(su: usize) -> BTreeMap<u32, Vec<Prop>> {
    let dr = storage::sus(su).draft;
    let mut out = BTreeMap::new();
    let mut todo = alloc::vec![storage::su_id(su)];
    while let Some(id) = todo.pop() {
        if out.contains_key(&id) {
            continue;
        }
        let u = storage::draft_su(dr, 0, id);
        if u == 0 {
            continue;
        }
        for e in storage::refs(u) {
            if e != 0 && e & 0x8000_0000 == 0 {
                todo.push(e);
            }
        }
        out.insert(id, storage::sus(u).props.clone());
        crate::obj::release(u, 0);
    }
    out
}

/// Every text value of a unit (for CF_TEXT).
fn text_of(su: usize) -> Option<Vec<u8>> {
    let s = storage::sus(su);
    for p in &s.props {
        for v in &p.vals {
            if v.ty == TYPE_TEXT || v.ty == b"CF_TEXT" {
                return Some(v.data.iter().copied().take_while(|&b| b != 0).collect());
            }
        }
    }
    None
}

/// ExportClipboard: the contents on PM's clipboard.
pub fn export(su: usize) -> bool {
    let h = hab();
    if su == 0 || unsafe { WinOpenClipbrd(h) } == 0 {
        return false;
    }
    unsafe { WinEmptyClipbrd(h) };
    let mut done: Vec<u32> = Vec::new();
    if let Some(t) = text_of(su) {
        let mut d = t;
        d.push(0);
        let p = shared(&d);
        if p != 0 {
            unsafe { WinSetClipbrdData(h, p, CF_TEXT, CFI_POINTER) };
            done.push(CF_TEXT);
        }
    }
    let props = storage::sus(su).props.clone();
    for p in &props {
        for v in &p.vals {
            if let Some(&(_, fmt, info)) = formats().iter().find(|f| f.0 == v.ty) {
                if done.contains(&fmt) {
                    continue;
                }
                let d = shared(&v.data);
                if d != 0 {
                    unsafe { WinSetClipbrdData(h, d, fmt, info) };
                    done.push(fmt);
                }
            }
        }
    }
    // rs2's private format: tag, root unit ID, then the units.
    let tag = unsafe {
        let (mut tib, mut pib) = (core::ptr::null_mut(), core::ptr::null_mut());
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        let pid = if pib.is_null() { 0 } else { (*pib).pid };
        ((EXPORTED & 0xffff).wrapping_add(1)) | (pid << 16)
    };
    unsafe { EXPORTED = tag };
    let mut d = Vec::new();
    d.extend_from_slice(&tag.to_le_bytes());
    d.extend_from_slice(&storage::su_id(su).to_le_bytes());
    let units = storage::encode(&reachable(su));
    d.extend_from_slice(&(units.len() as u32).to_le_bytes());
    d.extend_from_slice(&units);
    let p = shared(&d);
    if p != 0 {
        unsafe { WinSetClipbrdData(h, p, private_format(), CFI_POINTER) };
    }
    unsafe { WinCloseClipbrd(h) };
    true
}

/// GetContentStorageUnit: PM's clipboard read back into `t`'s draft when
/// another program changed it (else the unit as it is). The new unit's ID.
pub fn import(draft: usize, current: usize) -> usize {
    let h = hab();
    if unsafe { WinOpenClipbrd(h) } == 0 {
        return current;
    }
    let mut result = current;
    let p = unsafe { WinQueryClipbrdData(h, private_format()) };
    if p != 0 {
        let head = unsafe { core::slice::from_raw_parts(p as *const u8, 12) };
        let tag = bytes_u32(&head[0..4]);
        if tag != unsafe { EXPORTED } || current == 0 {
            let root = bytes_u32(&head[4..8]);
            let n = bytes_u32(&head[8..12]) as usize;
            let data = unsafe { core::slice::from_raw_parts((p + 12) as *const u8, n) };
            if let Some(units) = storage::decode(data) {
                result = load_units(draft, units, root);
            }
        }
    } else {
        let t = unsafe { WinQueryClipbrdData(h, CF_TEXT) };
        if t != 0 {
            let text = cstr(t).to_vec();
            if current == 0 || text_of(current).as_deref() != Some(&text[..]) {
                let su = storage::CreateStorageUnit(draft, 0);
                let s = storage::sus(su);
                s.props.push(Prop {
                    name: PROP_CONTENTS.to_vec(),
                    vals: alloc::vec![Value { ty: TYPE_TEXT.to_vec(), data: text.clone() }, Value { ty: b"CF_TEXT".to_vec(), data: text }],
                });
                crate::obj::release(su, 0);
                result = su;
            }
        } else if current != 0 {
            // Another program emptied the clipboard.
            storage::sus(current).props.clear();
        }
    }
    unsafe { WinCloseClipbrd(h) };
    result
}

/// Units read from the private format, as new units of `draft` (their
/// reference tables renumbered): the root's unit.
fn load_units(draft: usize, units: BTreeMap<u32, Vec<Prop>>, root: u32) -> usize {
    let mut map = BTreeMap::new();
    let mut objs = Vec::new();
    for (&id, props) in &units {
        let su = storage::CreateStorageUnit(draft, 0);
        storage::sus(su).props = props.clone();
        map.insert(id, storage::su_id(su));
        objs.push((id, su));
    }
    for &(_, su) in &objs {
        let r: Vec<u32> = storage::refs(su)
            .into_iter()
            .map(|e| match map.get(&(e & 0x7fff_ffff)) {
                Some(&n) if e != 0 => n | (e & 0x8000_0000),
                _ => 0,
            })
            .collect();
        storage::set_refs_pub(su, &r);
        crate::obj::release(su, 0);
    }
    objs.iter().find(|o| o.0 == root).map_or(0, |o| o.1)
}
