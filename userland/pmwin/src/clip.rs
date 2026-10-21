//! The clipboard and atom tables, shared by every PM process.
//!
//! As on OS/2, data handed to WinSetClipbrdData belongs to PM from then
//! on: a CFI_POINTER item (CF_TEXT, CF_DSPTEXT, registered formats) is
//! giveable shared memory of the caller, which PMWIN copies into the
//! shared PM state and frees; WinQueryClipbrdData gives every process a
//! pointer to that copy (the shared state is mapped at the same address
//! in all PM processes). A CF_BITMAP item moves out of the setter's GPI
//! objects into the clipboard; each process that asks for it gets its
//! own bitmap handle with the picture, freed when the clipboard is
//! emptied. Other CFI_HANDLE items are kept as they are.
//!
//! The clipboard is open for one message queue at a time; WinOpenClipbrd
//! waits for it. The owner gets WM_RENDERFMT for items set without data
//! (delayed rendering) and WM_DESTROYCLIPBOARD when they go, the viewer
//! WM_DRAWCLIPBOARD when the contents changed.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::bitmap::Bitmap;

use crate::msg::send;
use crate::state::{pid, st};

pub const CF_TEXT: u32 = 1;
pub const CF_BITMAP: u32 = 2;
pub const CF_DSPTEXT: u32 = 3;
pub const CF_DSPBITMAP: u32 = 4;
const CFI_OWNERFREE: u32 = 0x0001;
pub const CFI_HANDLE: u32 = 0x0200;
pub const CFI_POINTER: u32 = 0x0400;

const WM_RENDERFMT: u32 = 0x0060;
const WM_DESTROYCLIPBOARD: u32 = 0x0062;
const WM_DRAWCLIPBOARD: u32 = 0x0067;

pub enum Data {
    /// Not rendered yet: the owner renders it on request.
    Delayed,
    Bytes(Vec<u8>),
    Bitmap(Bitmap),
    Handle(u32),
}

pub struct Item {
    pub fmt: u32,
    pub info: u32,
    pub data: Data,
}

#[derive(Default)]
pub struct Clipboard {
    /// Queue that has the clipboard open (0: closed).
    pub open_by: u32,
    pub items: Vec<Item>,
    pub owner: u32,
    pub viewer: u32,
    /// The contents changed while open (the viewer is told on close).
    pub changed: bool,
    /// Bitmap handles given out for CF_BITMAP items: (pid, hbm).
    pub given: Vec<(u32, u32)>,
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.306"]
    fn DosQueryMem(pb: *const u8, pcb: *mut u32, pfl: *mut u32) -> u32;
    #[link_name = "DOSCALLS.304"]
    fn DosFreeMem(pb: *const u8) -> u32;
}

fn clip() -> &'static mut Clipboard {
    &mut st().clip
}

/// The caller's queue has the clipboard open.
fn is_open() -> bool {
    let q = st().my_queue();
    q != 0 && clip().open_by == q
}

/// Bytes of the memory object at `p` (from `p` to the end of its
/// committed range).
fn object_size(p: *const u8) -> usize {
    let (mut cb, mut fl) = (u32::MAX, 0);
    if unsafe { DosQueryMem(p, &mut cb, &mut fl) } != 0 {
        return 0;
    }
    // PAG_COMMIT.
    if fl & 0x10 == 0 { 0 } else { cb as usize }
}

/// Free what an item holds (and the bitmaps given out for it).
fn release(item: Item) {
    if let Data::Bitmap(_) = item.data {
        let given = core::mem::take(&mut clip().given);
        for (p, h) in given {
            crate::gpiobj::delete_bitmap_of(p, h);
        }
    }
}

/// Remove every item; the owner hears about it.
fn empty() {
    let items = core::mem::take(&mut clip().items);
    let had = !items.is_empty();
    for it in items {
        release(it);
    }
    let owner = clip().owner;
    if had && owner != 0 && st().desk.exists(owner) {
        crate::msg::notify(owner, WM_DESTROYCLIPBOARD, 0, 0);
    }
    clip().changed |= had;
}

/// Store an item (the clipboard must be open or the caller is rendering
/// a delayed format).
pub fn put(fmt: u32, info: u32, data: Data) {
    let c = clip();
    if let Some(i) = c.items.iter().position(|x| x.fmt == fmt) {
        let old = c.items.remove(i);
        release(old);
    }
    clip().items.push(Item { fmt, info, data });
    clip().changed = true;
}

/// Text on the clipboard (CF_TEXT), without the NUL; renders delayed
/// text. For built-in controls.
pub fn text() -> Option<Vec<u8>> {
    let p = WinQueryClipbrdData(1, CF_TEXT);
    if p == 0 {
        return None;
    }
    let mut v = Vec::new();
    let max = clip().items.iter().find(|i| i.fmt == CF_TEXT).map_or(0, |i| match &i.data {
        Data::Bytes(b) => b.len(),
        _ => 0,
    });
    let p = p as *const u8;
    for i in 0..max {
        let b = unsafe { *p.add(i) };
        if b == 0 {
            break;
        }
        v.push(b);
    }
    Some(v)
}

/// Put text (CF_TEXT, CR LF line ends) on the clipboard, replacing its
/// contents; for built-in controls. False if another queue has it open.
pub fn set_text(t: &[u8]) -> bool {
    if WinOpenClipbrd(1) == 0 {
        return false;
    }
    empty();
    let mut v = t.to_vec();
    v.push(0);
    put(CF_TEXT, CFI_POINTER, Data::Bytes(v));
    WinCloseClipbrd(1);
    true
}

/// A process ended: what it had open or owned goes.
pub fn process_gone(p: u32, queues: &[u32]) {
    let c = clip();
    if queues.contains(&c.open_by) {
        c.open_by = 0;
    }
    c.given.retain(|&(gp, _)| gp != p);
    let s = st();
    if s.clip.owner != 0 && !s.desk.exists(s.clip.owner) {
        s.clip.owner = 0;
    }
    if s.clip.viewer != 0 && !s.desk.exists(s.clip.viewer) {
        s.clip.viewer = 0;
    }
}

// ---- The API ----

/// WinOpenClipbrd(hab): waits while another thread has it open.
#[unsafe(no_mangle)]
pub extern "C" fn WinOpenClipbrd(_hab: u32) -> u32 {
    let q = st().my_queue();
    if q == 0 {
        return 0;
    }
    loop {
        let s = st();
        let by = s.clip.open_by;
        if by == q {
            return 1;
        }
        if by == 0 || !s.queues.contains_key(&by) {
            s.clip.open_by = q;
            s.clip.changed = false;
            return 1;
        }
        // Messages sent to this thread meanwhile are answered.
        crate::msg::answer_sent();
        crate::lock::sleep(10);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCloseClipbrd(_hab: u32) -> u32 {
    if !is_open() {
        return 0;
    }
    let c = clip();
    c.open_by = 0;
    let (changed, viewer) = (c.changed, c.viewer);
    c.changed = false;
    if changed && viewer != 0 && st().desk.exists(viewer) {
        let owner = st().clip.owner;
        crate::msg::notify(viewer, WM_DRAWCLIPBOARD, owner, 0);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinEmptyClipbrd(_hab: u32) -> u32 {
    if !is_open() {
        return 0;
    }
    empty();
    1
}

/// WinSetClipbrdData(hab, ulData, fmt, rgfFmtInfo)
#[unsafe(no_mangle)]
pub extern "C" fn WinSetClipbrdData(_hab: u32, data: u32, fmt: u32, info: u32) -> u32 {
    if fmt == 0 {
        return 0;
    }
    // Rendering a delayed format happens without opening the clipboard.
    let rendering = clip().items.iter().any(|i| i.fmt == fmt && matches!(i.data, Data::Delayed));
    if !is_open() && !rendering {
        return 0;
    }
    let d = if data == 0 {
        Data::Delayed
    } else if info & CFI_POINTER != 0 {
        let p = data as *const u8;
        let size = object_size(p);
        if size == 0 {
            return 0;
        }
        let n = if fmt == CF_TEXT || fmt == CF_DSPTEXT {
            // Text: up to and with its NUL.
            (0..size).find(|&i| unsafe { *p.add(i) } == 0).map_or(size, |i| i + 1)
        } else {
            size
        };
        let v = unsafe { core::slice::from_raw_parts(p, n) }.to_vec();
        // The memory was given to PM.
        if info & CFI_OWNERFREE == 0 {
            unsafe { DosFreeMem(p) };
        }
        Data::Bytes(v)
    } else if fmt == CF_BITMAP || fmt == CF_DSPBITMAP {
        match crate::gpiobj::take_bitmap(data) {
            Some(b) => Data::Bitmap(b),
            None => return 0,
        }
    } else {
        Data::Handle(data)
    };
    put(fmt, info, d);
    1
}

/// WinQueryClipbrdData(hab, fmt): the item's data for the calling
/// process (0: none).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClipbrdData(_hab: u32, fmt: u32) -> u32 {
    let Some(i) = clip().items.iter().position(|x| x.fmt == fmt) else { return 0 };
    if matches!(clip().items[i].data, Data::Delayed) {
        let owner = clip().owner;
        if owner != 0 && st().desk.exists(owner) {
            send(owner, WM_RENDERFMT, fmt, 0);
        }
    }
    let me = pid();
    let Some(item) = clip().items.iter().find(|x| x.fmt == fmt) else { return 0 };
    match &item.data {
        Data::Delayed => 0,
        Data::Bytes(v) => v.as_ptr() as u32,
        Data::Handle(h) => *h,
        Data::Bitmap(b) => {
            if let Some(&(_, h)) = clip().given.iter().find(|(p, h)| *p == me && crate::gpiobj::bitmap_size(*h).is_some()) {
                return h;
            }
            let copy = Bitmap { width: b.width, height: b.height, planes: b.planes, bitcount: b.bitcount, pixels: b.pixels.clone() };
            let h = crate::gpiobj::new_bitmap(copy);
            clip().given.push((me, h));
            h
        }
    }
}

/// WinQueryClipbrdFmtInfo(hab, fmt, prgfFmtInfo)
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClipbrdFmtInfo(_hab: u32, fmt: u32, pfl: *mut u32) -> u32 {
    let Some(item) = clip().items.iter().find(|x| x.fmt == fmt) else { return 0 };
    if !pfl.is_null() {
        let info = if item.info & (CFI_POINTER | CFI_HANDLE) == 0 {
            item.info | if matches!(item.data, Data::Bytes(_)) { CFI_POINTER } else { CFI_HANDLE }
        } else {
            item.info
        };
        unsafe { pfl.write_unaligned(info) };
    }
    1
}

/// WinEnumClipbrdFmts(hab, fmt): the format after `fmt` (0: the first);
/// 0 at the end.
#[unsafe(no_mangle)]
pub extern "C" fn WinEnumClipbrdFmts(_hab: u32, fmt: u32) -> u32 {
    let items = &clip().items;
    let next = if fmt == 0 { items.first() } else { items.iter().position(|x| x.fmt == fmt).and_then(|i| items.get(i + 1)) };
    next.map_or(0, |x| x.fmt)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetClipbrdOwner(_hab: u32, h: u32) -> u32 {
    if !is_open() {
        return 0;
    }
    clip().owner = h;
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClipbrdOwner(_hab: u32) -> u32 {
    clip().owner
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetClipbrdViewer(_hab: u32, h: u32) -> u32 {
    clip().viewer = h;
    // As on OS/2 the new viewer draws what is there at once
    // (WM_DRAWCLIPBOARD, mp1 = the clipboard owner).
    if h != 0 && st().desk.exists(h) {
        let owner = st().clip.owner;
        crate::msg::notify(h, WM_DRAWCLIPBOARD, owner, 0);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClipbrdViewer(_hab: u32) -> u32 {
    clip().viewer
}

// ---- Atom tables ----

pub struct AtomTable {
    /// Atom n is 0xC000 + index; (name, use count).
    pub atoms: Vec<Option<(Vec<u8>, u32)>>,
    pub pid: u32,
}

/// The system atom table's handle (others are 2...).
const SYSTEM_TABLE: u32 = 1;
const ATOM_BASE: u32 = 0xc000;

fn table(h: u32) -> Option<&'static mut AtomTable> {
    let s = st();
    if s.atoms.is_empty() {
        s.atoms.push(Some(AtomTable { atoms: Vec::new(), pid: 0 }));
    }
    s.atoms.get_mut(h.wrapping_sub(1) as usize)?.as_mut()
}

fn cbytes(p: *const u8) -> Vec<u8> {
    crate::ctl::cstr(p)
}

/// "#123" names an integer atom.
fn int_atom(name: &[u8]) -> Option<u32> {
    let d = name.strip_prefix(b"#")?;
    if d.is_empty() || !d.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut v: u32 = 0;
    for &c in d {
        v = v.checked_mul(10)?.checked_add((c - b'0') as u32)?;
    }
    (v > 0 && v < ATOM_BASE).then_some(v)
}

/// An atom name argument: a string or MAKEINTATOM (0xFFFF0000 | atom).
fn atom_arg(p: *const u8) -> Result<u32, Vec<u8>> {
    let v = p as u32;
    if v >> 16 == 0xffff {
        return Ok(v & 0xffff);
    }
    let name = cbytes(p);
    match int_atom(&name) {
        Some(a) => Ok(a),
        None => Err(name),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySystemAtomTable() -> u32 {
    let _ = table(SYSTEM_TABLE);
    SYSTEM_TABLE
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCreateAtomTable(_initial: u32, _buckets: u32) -> u32 {
    let _ = table(SYSTEM_TABLE);
    let s = st();
    let t = AtomTable { atoms: Vec::new(), pid: pid() };
    match s.atoms.iter().position(|x| x.is_none()) {
        Some(i) => {
            s.atoms[i] = Some(t);
            i as u32 + 1
        }
        None => {
            s.atoms.push(Some(t));
            s.atoms.len() as u32
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyAtomTable(h: u32) -> u32 {
    if h == SYSTEM_TABLE {
        return h;
    }
    match st().atoms.get_mut(h.wrapping_sub(1) as usize) {
        Some(slot @ Some(_)) => {
            *slot = None;
            0
        }
        _ => h,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinAddAtom(h: u32, name: *const u8) -> u32 {
    if name.is_null() {
        return 0;
    }
    let Some(t) = table(h) else { return 0 };
    let name = match atom_arg(name) {
        Ok(a) => return a,
        Err(n) => n,
    };
    if name.is_empty() {
        return 0;
    }
    if let Some(i) = t.atoms.iter().position(|a| a.as_ref().is_some_and(|(n, _)| n.eq_ignore_ascii_case(&name))) {
        if let Some((_, c)) = t.atoms[i].as_mut() {
            *c += 1;
        }
        return ATOM_BASE + i as u32;
    }
    let i = match t.atoms.iter().position(|a| a.is_none()) {
        Some(i) => {
            t.atoms[i] = Some((name, 1));
            i
        }
        None => {
            t.atoms.push(Some((name, 1)));
            t.atoms.len() - 1
        }
    };
    ATOM_BASE + i as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinFindAtom(h: u32, name: *const u8) -> u32 {
    if name.is_null() {
        return 0;
    }
    let Some(t) = table(h) else { return 0 };
    let name = match atom_arg(name) {
        Ok(a) => return a,
        Err(n) => n,
    };
    t.atoms.iter().position(|a| a.as_ref().is_some_and(|(n, _)| n.eq_ignore_ascii_case(&name))).map_or(0, |i| ATOM_BASE + i as u32)
}

/// WinDeleteAtom: 0 when deleted (or its count went down).
#[unsafe(no_mangle)]
pub extern "C" fn WinDeleteAtom(h: u32, atom: u32) -> u32 {
    if atom < ATOM_BASE {
        return 0;
    }
    let Some(t) = table(h) else { return atom };
    let Some(slot) = t.atoms.get_mut((atom - ATOM_BASE) as usize) else { return atom };
    match slot {
        Some((_, c)) if *c > 1 => {
            *c -= 1;
            0
        }
        Some(_) => {
            *slot = None;
            0
        }
        None => atom,
    }
}

fn atom_name(h: u32, atom: u32) -> Option<Vec<u8>> {
    if atom == 0 {
        return None;
    }
    if atom < ATOM_BASE {
        return Some(alloc::format!("#{atom}").into_bytes());
    }
    let t = table(h)?;
    t.atoms.get((atom - ATOM_BASE) as usize)?.as_ref().map(|(n, _)| n.clone())
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryAtomName(h: u32, atom: u32, buf: *mut u8, cch: u32) -> u32 {
    match atom_name(h, atom) {
        Some(n) => crate::ctl::copy_out(&n, cch as usize, buf),
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryAtomLength(h: u32, atom: u32) -> u32 {
    atom_name(h, atom).map_or(0, |n| n.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryAtomUsage(h: u32, atom: u32) -> u32 {
    if atom != 0 && atom < ATOM_BASE {
        return 0xffff;
    }
    table(h).and_then(|t| t.atoms.get(atom.wrapping_sub(ATOM_BASE) as usize)?.as_ref().map(|(_, c)| *c)).unwrap_or(0)
}
