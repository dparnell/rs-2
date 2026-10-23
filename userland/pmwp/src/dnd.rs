//! Dragging and dropping objects (PMDRAG's protocol, as RS2SHELL speaks
//! it):
//!
//! - From a view (CN_INITDRAG): a DRAGITEM per selected object with the
//!   view's client window and the object as its id; files and folders
//!   are `<DRM_OS2FILE,DRF_UNKNOWN>` too (their directory and name), so
//!   other programs take them; each object can change its item
//!   (wpFormatDragItem).
//! - Onto a view (CN_DRAGOVER, CN_DROP): the object under the pointer,
//!   else the view's folder, decides (wpDragOver) and takes each item
//!   (wpDrop). Folders move, copy or make shadows of the objects (Ctrl
//!   copies, Shift moves, Ctrl+Shift links; by default objects move, and
//!   files are copied to another drive); program objects and program
//!   files start with the dropped files; shadows and drives pass drops on
//!   to their objects. Icons dragged within their own icon view move.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OBJSTYLE_NOCOPY, OBJSTYLE_NOLINK, OBJSTYLE_NOMOVE};
use crate::pm::*;
use crate::util;
use crate::view::{self, wins};
use crate::{abs, som, wps};

struct Source {
    client: u32,
    objs: Vec<usize>,
    start: PointL,
}

static mut SOURCE: Option<Source> = None;

#[allow(static_mut_refs)]
fn source() -> Option<&'static Source> {
    unsafe { SOURCE.as_ref() }
}

const DC_CONTAINER: u16 = 0x0002;

// ---- The source ----

pub fn begin(w: usize, cdi: u32) -> u32 {
    let o = view::obj_of_rec(w, rd(cdi, 4));
    if o == 0 {
        return 0;
    }
    let list = view::selected(w, o);
    let pdinfo = unsafe { DrgAllocDraginfo(list.len() as u32) };
    if pdinfo == 0 {
        return 0;
    }
    let (client, cnr) = (wins()[w].client, wins()[w].child);
    for (k, &o) in list.iter().enumerate() {
        let mut d = [0u8; DRAGITEM_SIZE as usize];
        let w32 = |d: &mut [u8], off: usize, x: u32| d[off..off + 4].copy_from_slice(&x.to_le_bytes());
        w32(&mut d, DI_HWND, client);
        w32(&mut d, DI_ID, o as u32);
        let style = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32;
        let mut ops = 0u16;
        if style & OBJSTYLE_NOCOPY == 0 {
            ops |= DO_COPYABLE as u16;
        }
        if style & OBJSTYLE_NOMOVE == 0 {
            ops |= DO_MOVEABLE as u16;
        }
        if style & OBJSTYLE_NOLINK == 0 {
            ops |= DO_LINKABLE as u16;
        }
        let (rmf, cont, name): (&[u8], Vec<u8>, Vec<u8>) = match wps::path_of(o) {
            Some(p) if wps::is(o, "WPFileSystem") => {
                let (dir, n) = util::split_path(&p);
                (b"<DRM_OS2FILE,DRF_UNKNOWN>,<DRM_OBJECT,DRF_OBJECT>", dir, n)
            }
            _ => (b"<DRM_OBJECT,DRF_OBJECT>", Vec::new(), alloc::format!("{}", obj::handle(o)).into_bytes()),
        };
        w32(&mut d, DI_TYPE, add_str(b"Unknown"));
        w32(&mut d, DI_RMF, add_str(rmf));
        w32(&mut d, DI_CONTAINER, add_str(&cont));
        w32(&mut d, DI_SOURCE, add_str(&name));
        w32(&mut d, DI_TARGET, add_str(&name));
        let ctl = if wps::is(o, "WPFolder") { DC_CONTAINER } else { 0 };
        d[DI_CONTROL..DI_CONTROL + 2].copy_from_slice(&ctl.to_le_bytes());
        d[DI_OPS..DI_OPS + 2].copy_from_slice(&ops.to_le_bytes());
        obj::call(m::WPObject::wpFormatDragItem, &[o, d.as_mut_ptr() as usize]);
        unsafe { DrgSetDragitem(pdinfo, d.as_ptr(), DRAGITEM_SIZE, k as u32) };
        send(cnr, CM_SETRECORDEMPHASIS, view::record_of(w, o), mp(1, CRA_SOURCE));
    }
    let icon = if list.len() > 1 {
        unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_MULTFILE, 0) }
    } else {
        crate::templates::view_icon(list[0])
    };
    let mut img = [0u8; 24];
    img[0..2].copy_from_slice(&24u16.to_le_bytes());
    img[4..8].copy_from_slice(&icon.to_le_bytes());
    img[16..20].copy_from_slice(&DRG_ICON.to_le_bytes());
    unsafe { SOURCE = Some(Source { client, objs: list.clone(), start: PointL { x: rd(cdi, 8) as i32, y: rd(cdi, 12) as i32 } }) };
    let target = unsafe { DrgDrag(client, pdinfo, img.as_ptr(), 1, VK_ENDDRAG, 0) };
    unsafe { SOURCE = None };
    if target == 0 {
        unsafe { DrgDeleteDraginfoStrHandles(pdinfo) };
    }
    unsafe { DrgFreeDraginfo(pdinfo) };
    if let Some(w) = view::find_client(client) {
        for &o in &list {
            let r = view::record_of(w, o);
            if r != 0 {
                send(wins()[w].child, CM_SETRECORDEMPHASIS, r, mp(0, CRA_SOURCE));
            }
        }
    }
    1
}

// ---- Items ----

/// The object a dragged item stands for: one of ours (by its view and
/// id), or another program's file (woken from its path).
pub fn item_object(it: u32) -> usize {
    let (hwnd, id) = (rd(it, DI_HWND), rd(it, DI_ID) as usize);
    if view::find_client(hwnd).is_some() && wins().iter().any(|w| w.recs.iter().any(|r| r.obj == id)) {
        return id;
    }
    if unsafe { DrgVerifyRMF(it, b"DRM_OS2FILE\0".as_ptr(), core::ptr::null()) } != 0 {
        let (c, s) = (str_handle(rd(it, DI_CONTAINER)), str_handle(rd(it, DI_SOURCE)));
        if !c.is_empty() && !s.is_empty() {
            return wps::object_from_path(&util::join(&c, &s));
        }
    }
    0
}

fn items(pdinfo: u32) -> Vec<u32> {
    let n = unsafe { DrgQueryDragitemCount(pdinfo) };
    (0..n).map(|i| unsafe { DrgQueryDragitemPtr(pdinfo, i) }).filter(|&p| p != 0).collect()
}

fn user_op(pdinfo: u32) -> u32 {
    rd(pdinfo, 4) >> 16
}

/// The object a drop goes to: the record under the pointer (unless it
/// is being dragged), else the view's folder.
fn target(w: usize, rec: u32) -> usize {
    let o = view::obj_of_rec(w, rec);
    let dragged = source().is_some_and(|s| s.client == wins()[w].client && s.objs.contains(&o));
    if o != 0 && !dragged { o } else { wins()[w].obj }
}

/// Icons moved within their own icon view.
fn repositioning(w: usize, pdinfo: u32, to: usize) -> bool {
    to == wins()[w].obj
        && wins()[w].view == obj::OPEN_CONTENTS
        && source().is_some_and(|s| s.client == wins()[w].client)
        && matches!(user_op(pdinfo), DO_DEFAULT | DO_MOVE)
}

pub fn drag_over(w: usize, cdi: u32) -> u32 {
    let (pdinfo, rec) = (rd(cdi, 0), rd(cdi, 4));
    if pdinfo == 0 || unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
        return DOR_NEVERDROP;
    }
    let to = target(w, rec);
    let r = if repositioning(w, pdinfo, to) {
        mp(DOR_DROP, DO_MOVE)
    } else if to == 0 {
        // Find Results' background takes nothing.
        mp(DOR_NEVERDROP, 0)
    } else {
        obj::call(m::WPObject::wpDragOver, &[to, wins()[w].child as usize, pdinfo as usize]) as u32
    };
    unsafe { DrgFreeDraginfo(pdinfo) };
    r
}

pub fn drop(w: usize, cdi: u32) {
    let (pdinfo, rec) = (rd(cdi, 0), rd(cdi, 4));
    if pdinfo == 0 || unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
        return;
    }
    let to = target(w, rec);
    let list = items(pdinfo);
    let mut ok = true;
    if repositioning(w, pdinfo, to) {
        let s = source().unwrap();
        let at = PointL { x: (rd(pdinfo, 12) & 0xffff) as i16 as i32, y: (rd(pdinfo, 12) >> 16) as i16 as i32 };
        let (dx, dy) = (at.x - s.start.x, at.y - s.start.y);
        for &o in &s.objs {
            let r = view::record_of(w, o);
            if r != 0 {
                wr(r, R_X, (rd(r, R_X) as i32 + dx).max(0) as u32);
                wr(r, R_Y, (rd(r, R_Y) as i32 + dy).max(0) as u32);
            }
        }
        send(wins()[w].child, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
        view::save_positions(w);
    } else {
        let over = obj::call(m::WPObject::wpDragOver, &[to, wins()[w].child as usize, pdinfo as usize]) as u32;
        ok = over & 0xffff == DOR_DROP;
        if ok {
            // Folders' copies and moves of file system objects gather
            // into one job (progress, name collisions).
            unsafe { BATCH = Some(Vec::new()) };
            for &it in &list {
                obj::call(m::WPObject::wpDrop, &[to, wins()[w].child as usize, pdinfo as usize, it as usize]);
            }
            let batch = unsafe { (*&raw mut BATCH).take() }.unwrap_or_default();
            run_batch(batch);
        }
    }
    for &it in &list {
        post(rd(it, DI_HWND), DM_ENDCONVERSATION, rd(it, DI_ID), if ok { DMFL_TARGETSUCCESSFUL } else { DMFL_TARGETFAIL });
    }
    unsafe {
        DrgDeleteDraginfoStrHandles(pdinfo);
        DrgFreeDraginfo(pdinfo);
    }
}

// ---- The methods ----

extern "C" fn wpFormatDragItem(_o: usize, _item: u32) -> u32 {
    1
}

extern "C" fn wpDragOver(_o: usize, _cnr: u32, _pdinfo: u32) -> u32 {
    mp(DOR_NODROP, 0)
}

extern "C" fn wpDrop(_o: usize, _cnr: u32, _pdinfo: u32, _item: u32) -> u32 {
    0
}

/// Whether `inner` is `outer` or within it (folders by path, the others
/// by their folders).
fn within(inner: usize, outer: usize) -> bool {
    let mut o = inner;
    for _ in 0..64 {
        if o == 0 {
            return false;
        }
        if o == outer {
            return true;
        }
        o = obj::folder_of(o);
    }
    false
}

fn drive_of(o: usize) -> u8 {
    wps::path_of(o).and_then(|p| p.first().copied()).unwrap_or(0).to_ascii_uppercase()
}

/// The operation for an item dropped into a folder by default.
fn default_op(src: usize, folder: usize) -> u32 {
    if wps::is(src, "WPFileSystem") && drive_of(src) != drive_of(folder) { DO_COPY } else { DO_MOVE }
}

fn allowed(it: u32, op: u32) -> bool {
    let ops = (rd(it, DI_CONTROL) >> 16) & 0xffff;
    match op {
        DO_COPY => ops & DO_COPYABLE as u32 != 0,
        DO_MOVE => ops & DO_MOVEABLE as u32 != 0,
        DO_LINK => ops & DO_LINKABLE as u32 != 0,
        _ => false,
    }
}

extern "C" fn folder_drag_over(f: usize, _cnr: u32, pdinfo: u32) -> u32 {
    let list = items(pdinfo);
    if list.is_empty() {
        return mp(DOR_NEVERDROP, 0);
    }
    let user = user_op(pdinfo);
    let mut op = 0;
    for &it in &list {
        let src = item_object(it);
        if src == 0 {
            return mp(DOR_NEVERDROP, 0);
        }
        if within(f, src) {
            return mp(DOR_NODROP, 0);
        }
        let o = if user == DO_DEFAULT { default_op(src, f) } else { user };
        // A template dropped makes a new object (a copy).
        let o = if crate::templates::is_template(src) && o != DO_LINK { DO_COPY } else { o };
        // Moving an object to where it is does nothing.
        if o == DO_MOVE && obj::folder_of(src) == f {
            return mp(DOR_NODROP, 0);
        }
        if !allowed(it, o) {
            return mp(DOR_NODROPOP, 0);
        }
        if op == 0 {
            op = o;
        }
    }
    mp(DOR_DROP, op)
}

/// Drops gathered while a drop is handled: (operation, object, folder).
static mut BATCH: Option<Vec<(u32, usize, usize)>> = None;

fn run_batch(batch: Vec<(u32, usize, usize)>) {
    let mut groups: Vec<(u32, usize, Vec<usize>)> = Vec::new();
    for (op, o, f) in batch {
        match groups.iter_mut().find(|g| g.0 == op && g.1 == f) {
            Some(g) => g.2.push(o),
            None => groups.push((op, f, alloc::vec![o])),
        }
    }
    for (op, f, objs) in groups {
        let op = if op == DO_COPY { crate::filejob::Op::Copy } else { crate::filejob::Op::Move };
        crate::filejob::transfer(op, &objs, f, None);
    }
}

extern "C" fn folder_drop(f: usize, _cnr: u32, pdinfo: u32, it: u32) -> u32 {
    let src = item_object(it);
    if src == 0 {
        return 0;
    }
    let user = user_op(pdinfo);
    let op = if user == DO_DEFAULT { default_op(src, f) } else { user };
    if crate::templates::is_template(src) && op != DO_LINK {
        crate::templates::create_from(src, f, None);
        return 1;
    }
    #[allow(static_mut_refs)]
    if let (Some(b), DO_MOVE | DO_COPY) = (unsafe { BATCH.as_mut() }, op) {
        if wps::is(src, "WPFileSystem") && wps::is(f, "WPFileSystem") {
            b.push((op, src, f));
            return 1;
        }
    }
    match op {
        DO_MOVE => obj::call(m::WPObject::wpMoveObject, &[src, f]),
        DO_COPY => obj::call(m::WPObject::wpCopyObject, &[src, f, 0]),
        DO_LINK => obj::call(m::WPObject::wpCreateShadowObject, &[src, f, 0]),
        _ => 0,
    };
    1
}

/// The files dropped (paths).
fn dropped_files(pdinfo: u32) -> Option<Vec<Vec<u8>>> {
    let mut v = Vec::new();
    for it in items(pdinfo) {
        let o = item_object(it);
        let p = if o != 0 { wps::path_of(o) } else { None }?;
        v.push(p);
    }
    Some(v)
}

extern "C" fn program_drag_over(_o: usize, _cnr: u32, pdinfo: u32) -> u32 {
    match dropped_files(pdinfo) {
        Some(v) if !v.is_empty() => mp(DOR_DROP, DO_UNKNOWN),
        _ => mp(DOR_NODROPOP, 0),
    }
}

/// A file dropped on a program: the program starts with the file.
extern "C" fn program_drop(o: usize, _cnr: u32, _pdinfo: u32, it: u32) -> u32 {
    let f = item_object(it);
    let Some(path) = (if f != 0 { wps::path_of(f) } else { None }) else { return 0 };
    let mut buf = alloc::vec![0u8; 2048];
    let mut cb = buf.len() as u32;
    let det = if wps::is(o, "WPProgram") {
        wps::call("WPProgram", m::WPProgram::wpQueryProgDetails, &[o, buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize])
    } else {
        wps::call("WPProgramFile", m::WPProgramFile::wpQueryProgDetails, &[o, buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize])
    };
    if det == 0 {
        return 0;
    }
    let w = buf.as_ptr() as u32;
    let s = |i: usize| cstr(rd(w, i * 4) as *const u8);
    let mut params = s(5);
    if !params.is_empty() {
        params.push(b' ');
    }
    if path.contains(&b' ') {
        params.push(b'"');
        params.extend_from_slice(&path);
        params.push(b'"');
    } else {
        params.extend_from_slice(&path);
    }
    crate::launch::start(&s(4), &params, &s(6), &s(3), rd(w, 4));
    1
}

extern "C" fn shadow_drag_over(o: usize, cnr: u32, pdinfo: u32) -> u32 {
    let l = abs::linked(o);
    if l == 0 { mp(DOR_NODROP, 0) } else { obj::call(m::WPObject::wpDragOver, &[l, cnr as usize, pdinfo as usize]) as u32 }
}

extern "C" fn shadow_drop(o: usize, cnr: u32, pdinfo: u32, it: u32) -> u32 {
    let l = abs::linked(o);
    if l == 0 { 0 } else { obj::call(m::WPObject::wpDrop, &[l, cnr as usize, pdinfo as usize, it as usize]) as u32 }
}

extern "C" fn disk_drag_over(o: usize, cnr: u32, pdinfo: u32) -> u32 {
    let r = abs::root_of(o);
    if r == 0 { mp(DOR_NODROP, 0) } else { obj::call(m::WPObject::wpDragOver, &[r, cnr as usize, pdinfo as usize]) as u32 }
}

extern "C" fn disk_drop(o: usize, cnr: u32, pdinfo: u32, it: u32) -> u32 {
    let r = abs::root_of(o);
    if r == 0 { 0 } else { obj::call(m::WPObject::wpDrop, &[r, cnr as usize, pdinfo as usize, it as usize]) as u32 }
}

methods! {
    __m_wpFormatDragItem => wpFormatDragItem, 2;
    __m_wpDragOver => wpDragOver, 3;
    __m_wpDrop => wpDrop, 4;
    __m_folder_drag_over => folder_drag_over, 3;
    __m_folder_drop => folder_drop, 4;
    __m_program_drag_over => program_drag_over, 3;
    __m_program_drop => program_drop, 4;
    __m_shadow_drag_over => shadow_drag_over, 3;
    __m_shadow_drop => shadow_drop, 4;
    __m_disk_drag_over => disk_drag_over, 3;
    __m_disk_drop => disk_drop, 4;
}

pub fn object_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpFormatDragItem", __m_wpFormatDragItem as usize),
        ("wpDragOver", __m_wpDragOver as usize),
        ("wpDrop", __m_wpDrop as usize),
    ]
}

/// Overrides for a class (folders, programs, shadows, disks).
pub fn overrides(class: &str) -> Vec<(&'static str, usize)> {
    let (o, d) = match class {
        "WPFolder" => (__m_folder_drag_over as usize, __m_folder_drop as usize),
        "WPProgram" | "WPProgramFile" => (__m_program_drag_over as usize, __m_program_drop as usize),
        "WPShadow" => (__m_shadow_drag_over as usize, __m_shadow_drop as usize),
        "WPDisk" => (__m_disk_drag_over as usize, __m_disk_drop as usize),
        _ => return Vec::new(),
    };
    alloc::vec![("wpDragOver", o), ("wpDrop", d)]
}

pub fn _unused() -> usize {
    som::ret0 as usize
}
