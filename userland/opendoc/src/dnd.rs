//! Drag and drop on PM's (rs2's PMDRAG):
//!
//! - **Dropping files.** A document window takes files dropped on it
//!   (`DRM_OS2FILE`, from a Workplace Shell folder or any other program)
//!   whose kind has a part handler. A file `NAME.EXT` is of the kind
//!   `<DRM_OS2FILE,DRF_EXT>`, the kind IBM's multimedia parts register
//!   (MMAudioPart: `<DRM_OS2FILE,DRF_WAVE>`); a few extensions name
//!   another (JPG, BMP and GIF are images, MID is MIDI). The file becomes
//!   a part as IBM's parts expect a dropped file: a new storage unit
//!   naming the handler, with the file name as OS/2 OpenDoc's file
//!   contents (`Contents`, `FileType`), read by the part's
//!   InitPartFromStorage. The part is embedded in the window's root frame
//!   where it was dropped.
//! - **Dragging parts.** A part is dragged with mouse button 2
//!   (WM_BEGINDRAG): an embedded part whose part does not take the event,
//!   or one that calls ODDragAndDrop's StartDrag (IBM's parts do). It goes
//!   as an item of rs2's mechanism `<DRM_OPENDOC,DRF_PART>`, which names
//!   its frame. Dropped in its own window it moves there; dropped in
//!   another document window of the process, a copy of it is embedded
//!   there.

use alloc::vec::Vec;

use crate::layout;
use crate::obj::release;
use crate::rt::*;
use crate::services::{self, win};
use crate::storage;
use crate::*;

unsafe extern "C" {
    #[link_name = "PMDRAG.34"]
    fn DrgAllocDraginfo(n: u32) -> usize;
    #[link_name = "PMDRAG.32"]
    fn DrgAccessDraginfo(p: usize) -> u32;
    #[link_name = "PMDRAG.39"]
    fn DrgFreeDraginfo(p: usize) -> u32;
    #[link_name = "PMDRAG.45"]
    fn DrgQueryDragitemCount(p: usize) -> u32;
    #[link_name = "PMDRAG.46"]
    fn DrgQueryDragitemPtr(p: usize, i: u32) -> usize;
    #[link_name = "PMDRAG.49"]
    fn DrgQueryStrName(h: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "PMDRAG.33"]
    fn DrgAddStrHandle(s: *const u8) -> u32;
    #[link_name = "PMDRAG.57"]
    fn DrgSetDragitem(p: usize, item: *const DragItem, cb: u32, i: u32) -> u32;
    #[link_name = "PMDRAG.38"]
    fn DrgDrag(src: u32, p: usize, img: *const DragImage, n: u32, vk: i32, rsvd: usize) -> u32;
    #[link_name = "PMDRAG.36"]
    fn DrgDeleteDraginfoStrHandles(p: usize) -> u32;
    #[link_name = "PMWIN.788"]
    fn WinMapWindowPoints(from: u32, to: u32, pts: *mut [i32; 2], n: i32) -> u32;
    #[link_name = "PMWIN.765"]
    fn WinInvalidateRect(hwnd: u32, rect: usize, children: u32) -> u32;
    #[link_name = "PMWIN.828"]
    fn WinQuerySysPointer(desktop: u32, id: i32, copy: u32) -> u32;
}

pub const DM_DRAGOVER: u32 = 0x032e;
pub const DM_DRAGLEAVE: u32 = 0x032d;
pub const DM_DROP: u32 = 0x032f;
const DOR_DROP: usize = 1;
const DOR_NODROP: usize = 0;
const DO_COPY: usize = 0x0010;
const DO_MOVE: usize = 0x0020;
const DO_DEFAULT: u16 = 0xbffe;

/// DRAGITEM.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DragItem {
    pub hwnd: u32,
    pub id: u32,
    pub ty: u32,
    pub rmf: u32,
    pub container: u32,
    pub source: u32,
    pub target: u32,
    pub cx: i16,
    pub cy: i16,
    pub control: u16,
    pub ops: u16,
}

/// DRAGIMAGE.
#[repr(C)]
pub struct DragImage {
    pub cb: u16,
    pub cptl: u16,
    pub image: u32,
    pub cx: i32,
    pub cy: i32,
    pub fl: u32,
    pub xoff: i16,
    pub yoff: i16,
}

const RMF_PART: &[u8] = b"<DRM_OPENDOC,DRF_PART>";

fn str_of(h: u32) -> Vec<u8> {
    let mut b = [0u8; 260];
    let n = unsafe { DrgQueryStrName(h, b.len() as u32, b.as_mut_ptr()) } as usize;
    b[..n.min(b.len())].iter().copied().take_while(|&x| x != 0).collect()
}

fn items(pd: usize) -> Vec<DragItem> {
    let n = unsafe { DrgQueryDragitemCount(pd) };
    (0..n).filter_map(|i| {
        let p = unsafe { DrgQueryDragitemPtr(pd, i) };
        if p == 0 { None } else { Some(read::<DragItem>(p)) }
    }).collect()
}

/// The part handler class for a dropped file (by its kind).
pub fn handler_for_file(name: &[u8]) -> Option<Vec<u8>> {
    let dot = name.iter().rposition(|&b| b == b'.')?;
    let ext: Vec<u8> = name[dot + 1..].iter().map(|b| b.to_ascii_uppercase()).collect();
    let mut kinds = Vec::new();
    let k = |e: &[u8]| {
        let mut v = b"<DRM_OS2FILE,DRF_".to_vec();
        v.extend_from_slice(e);
        v.push(b'>');
        v
    };
    kinds.push(k(&ext));
    let alias: &[&[u8]] = match &ext[..] {
        b"WAV" => &[b"WAVE"],
        b"JPG" | b"JPEG" | b"BMP" | b"GIF" | b"TIF" | b"PCX" => &[b"JPEG", b"BITMAP", b"OS2BMP"],
        b"MID" => &[b"MIDI"],
        b"AVI" => &[b"AVI", b"MOVIE"],
        _ => &[],
    };
    for a in alias {
        kinds.push(k(a));
    }
    kinds.iter().find_map(|k| crate::part::handler_for_kind(k)).or_else(|| match &ext[..] {
        // IBM's multimedia parts by their class names, when registered.
        b"WAV" if crate::part::handler_dll(b"MMAudioPart").is_some() => Some(b"MMAudioPart".to_vec()),
        b"JPG" | b"JPEG" | b"BMP" | b"GIF" if crate::part::handler_dll(b"MMImagePart").is_some() => Some(b"MMImagePart".to_vec()),
        b"MID" if crate::part::handler_dll(b"MMMIDIPart").is_some() => Some(b"MMMIDIPart".to_vec()),
        b"AVI" if crate::part::handler_dll(b"MMVideoPart").is_some() => Some(b"MMVideoPart".to_vec()),
        _ => None,
    })
}

fn path_of(it: &DragItem) -> Vec<u8> {
    let mut p = str_of(it.container);
    if !p.is_empty() && !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(&str_of(it.source));
    p
}

fn acceptable(it: &DragItem) -> bool {
    let rmf = str_of(it.rmf);
    if rmf.windows(RMF_PART.len()).any(|w| w == RMF_PART) {
        return true;
    }
    rmf.windows(11).any(|w| w == b"DRM_OS2FILE") && handler_for_file(&str_of(it.source)).is_some()
}

const PROP_CONTENTS: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:Contents";
const TYPE_FILETYPE: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OS2:Type:FileType";
const VIEW_FRAME: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:Frame";
const PRES_NORMAL: &[u8] = b"rs2:Presentation:Normal";

/// A part made from a file in the root frame's draft: its storage unit
/// names the handler and holds the file name as file contents.
pub fn part_from_file(dr: usize, ev: usize, class: &[u8], path: &[u8]) -> usize {
    let su = storage::CreateStorageUnit(dr, ev);
    storage::set_value(su, storage::PROP_EDITOR, storage::TYPE_ISO_STR, class);
    storage::set_value(su, PROP_CONTENTS, TYPE_FILETYPE, path);
    let id = storage::su_id(su);
    release(su, ev);
    call(cd!(ODDraftClassData), storage::draft_ix::ACQUIRE_PART, &[dr, ev, id as usize])
}

/// Embed a part in a window's root frame with its frame's origin at `at`
/// (client coordinates): a new frame and a facet in the root facet.
pub fn embed(w: usize, ev: usize, part: usize, at: [i32; 2], size: [i32; 2]) -> usize {
    let rf = win(w).root_frame;
    let rfc = win(w).root_facet;
    if rf == 0 || rfc == 0 || part == 0 {
        return 0;
    }
    let dr = layout::frame(rf).draft;
    let shp = layout::new_shape(Rect { left: 0, bottom: 0, right: size[0] << 16, top: size[1] << 16 });
    let s = services::session();
    let view = services::tokenize(s, VIEW_FRAME);
    let pres = services::tokenize(s, PRES_NORMAL);
    let ft = storage::OBJ_FRAME.to_vec();
    let fz = z(&ft);
    let cf = call(cd!(ODDraftClassData), storage::draft_ix::CREATE_FRAME, &[dr, ev, fz.as_ptr() as usize, rf, shp, 0, part, view, pres, 0, 0]);
    release(shp, ev);
    if cf == 0 {
        return 0;
    }
    let t = layout::new_transform(layout::Xf { sx: ONE, sy: ONE, ox: at[0] << 16, oy: at[1] << 16 });
    layout::CreateEmbeddedFacet(rfc, ev, cf, 0, t, 0, 0, 0, 0);
    release(t, ev);
    if win(w).client != 0 {
        unsafe { WinInvalidateRect(win(w).client, 0, 1) };
    }
    cf
}

fn drop_point(pd: usize, client: u32) -> [i32; 2] {
    let x = read::<i16>(pd + 12) as i32;
    let y = read::<i16>(pd + 14) as i32;
    let mut p = [x, y];
    unsafe { WinMapWindowPoints(1, client, &mut p, 1) };
    p
}

/// A frame's (first) facet in a window, its offset moved so that the
/// frame's origin is at `at`.
fn move_frame(frame: usize, at: [i32; 2]) -> bool {
    let Some(&fc) = layout::frame(frame).facets.first() else { return false };
    let s = layout::facet(fc);
    let parent = s.containing;
    // The new offset in the containing frame's content coordinates.
    let p = if parent != 0 { layout::window_content_xf(parent).invert(Point { x: at[0] << 16, y: at[1] << 16 }) } else { Point { x: at[0] << 16, y: at[1] << 16 } };
    s.external.ox = p.x;
    s.external.oy = p.y;
    true
}

static mut DRAGGING: usize = 0;

/// The window a frame shows in: its own, else its containing frame's.
pub fn window_of_frame(mut f: usize) -> usize {
    while f != 0 {
        let s = layout::frame(f);
        if s.window != 0 {
            return s.window;
        }
        f = s.containing;
    }
    0
}

/// DM_DRAGOVER and DM_DROP on a document window's client.
pub fn client_message(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> Option<usize> {
    if msg != DM_DRAGOVER && msg != DM_DROP && msg != DM_DRAGLEAVE {
        return None;
    }
    if msg == DM_DRAGLEAVE {
        return Some(0);
    }
    let pd = mp1;
    if pd == 0 || unsafe { DrgAccessDraginfo(pd) } == 0 {
        return Some(DOR_NODROP);
    }
    let its = items(pd);
    let ok = !its.is_empty() && its.iter().all(acceptable);
    if msg == DM_DRAGOVER {
        let own = its.iter().any(|it| str_of(it.rmf).windows(RMF_PART.len()).any(|w| w == RMF_PART));
        unsafe { DrgFreeDraginfo(pd) };
        return Some(if ok { DOR_DROP | (if own { DO_MOVE } else { DO_COPY } << 16) } else { DOR_NODROP });
    }
    // DM_DROP
    let w = crate::events::window_for(hwnd);
    let mut ev = somsup::Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };
    let e = &raw mut ev as usize;
    let mut at = drop_point(pd, hwnd);
    if ok && w != 0 {
        for it in &its {
            let rmf = str_of(it.rmf);
            if rmf.windows(RMF_PART.len()).any(|x| x == RMF_PART) {
                let frame = it.id as usize;
                if frame != 0 && window_of_frame(frame) == w {
                    move_frame(frame, at);
                    if win(w).client != 0 {
                        unsafe { WinInvalidateRect(win(w).client, 0, 1) };
                    }
                } else if frame != 0 {
                    // Another window: a copy of the part.
                    let part = layout::FAcquirePart(frame, e);
                    let dr = layout::frame(win(w).root_frame).draft;
                    let su = storage::CreateStorageUnit(dr, e);
                    call(cd!(ODPersistentObjectClassData), crate::obj::ix::EXTERNALIZE, &[part, e]);
                    storage::clone_into(crate::obj::su_of(part), su, 0);
                    let id = storage::su_id(su);
                    release(su, e);
                    release(part, e);
                    let copy = call(cd!(ODDraftClassData), storage::draft_ix::ACQUIRE_PART, &[dr, e, id as usize]);
                    let r = layout::frame(frame).frame_shape;
                    embed(w, e, copy, at, [(r.right - r.left) >> 16, (r.top - r.bottom) >> 16]);
                    release(copy, e);
                }
            } else {
                let path = path_of(it);
                if let Some(class) = handler_for_file(&str_of(it.source)) {
                    let dr = layout::frame(win(w).root_frame).draft;
                    let part = part_from_file(dr, e, &class, &path);
                    if part != 0 {
                        embed(w, e, part, at, [160, 100]);
                        release(part, e);
                    }
                }
            }
            at = [at[0] + 16, at[1] - 16];
        }
    }
    if ev.major != 0 {
        unsafe { somsup::somExceptionFree(&raw mut ev) };
    }
    unsafe {
        DrgDeleteDraginfoStrHandles(pd);
        DrgFreeDraginfo(pd);
    }
    Some(0)
}

/// Drag a frame (mouse button 2): DrgDrag with one `<DRM_OPENDOC,DRF_PART>`
/// item naming the frame. The window it was dropped on (0: none).
pub fn drag_frame(frame: usize, client: u32) -> u32 {
    unsafe {
        if DRAGGING != 0 || frame == 0 || client == 0 {
            return 0;
        }
        DRAGGING = frame;
    }
    let pd = unsafe { DrgAllocDraginfo(1) };
    if pd == 0 {
        unsafe { DRAGGING = 0 };
        return 0;
    }
    let ty = z(b"OpenDoc part");
    let rmf = z(RMF_PART);
    let item = DragItem {
        hwnd: client,
        id: frame as u32,
        ty: unsafe { DrgAddStrHandle(ty.as_ptr()) },
        rmf: unsafe { DrgAddStrHandle(rmf.as_ptr()) },
        container: 0,
        source: unsafe { DrgAddStrHandle(b"part\0".as_ptr()) },
        target: 0,
        cx: 0,
        cy: 0,
        control: 0,
        ops: (DO_MOVE | DO_COPY) as u16,
    };
    unsafe { DrgSetDragitem(pd, &item, core::mem::size_of::<DragItem>() as u32, 0) };
    let img = DragImage { cb: core::mem::size_of::<DragImage>() as u16, cptl: 0, image: unsafe { WinQuerySysPointer(1, 9, 0) }, cx: 0, cy: 0, fl: 0x0001, xoff: 0, yoff: 0 };
    let target = unsafe { DrgDrag(client, pd, &img, 1, 0x0002, 0) }; // VK_BUTTON2
    unsafe {
        DrgFreeDraginfo(pd);
        DRAGGING = 0;
    }
    let _ = DO_DEFAULT;
    target
}
