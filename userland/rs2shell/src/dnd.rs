//! Dragging and dropping, through PMDRAG as the Workplace Shell does:
//!
//! - As the source (the container's CN_INITDRAG, mouse button 2): a
//!   DRAGINFO with a DRAGITEM per selected object, DrgDrag until the
//!   button goes up. Files and folders are `<DRM_OS2FILE,DRF_UNKNOWN>`
//!   items with their directory and name (hstrContainerName,
//!   hstrSourceName), so any program that takes files can use them;
//!   the shell's other objects are `<DRM_OBJECT,DRF_OBJECT>` with their
//!   object id. A target that wants a file rendered (DM_RENDER with a
//!   name to render to) gets a copy there and DM_RENDERCOMPLETE.
//! - As the target (CN_DRAGOVER, CN_DROP from the containers): files
//!   from anywhere (DRM_OS2FILE with a directory and a name) and the
//!   shell's own objects. The operation is the user's (Ctrl copies,
//!   Shift moves, Ctrl+Shift makes shadows) or the Workplace Shell's
//!   default: files move within a drive and are copied to another one,
//!   program files dropped on the desktop or a program group become
//!   program objects, other files there become shadows, program objects
//!   move between groups, and objects dropped in their own folder move
//!   there. Files dropped on a program object (or a program file) start
//!   it with their names as parameters. Each item gets
//!   DM_ENDCONVERSATION.

use alloc::vec::Vec;

use crate::fileops;
use crate::pm::*;
use crate::sh;
use crate::shadows::Target;
use crate::view::{self, Kind, Obj};

/// The drag this shell runs: its view and the keys of the dragged
/// items, and where it started (desktop coordinates).
struct Source {
    client: u32,
    keys: Vec<u32>,
    start: PointL,
}

static mut SOURCE: Option<Source> = None;

fn source() -> Option<&'static Source> {
    unsafe { (*core::ptr::addr_of!(SOURCE)).as_ref() }
}

/// What a dropped item is.
enum Src {
    /// One of the shell's objects: (view, item).
    Ours(usize, usize),
    /// A file or directory from another program.
    File(Vec<u8>),
}

/// What a drop goes into.
enum Dest {
    Folder(Kind),
    /// A program object: started with the files.
    Program(u32),
    /// A program file: run with the files.
    Exe(Vec<u8>),
}

// Operations an item allows (bits).
const C: u8 = 1;
const M: u8 = 2;
const L: u8 = 4;
const P: u8 = 8;

fn is_program_file(p: &[u8]) -> bool {
    let up = p.to_ascii_uppercase();
    [&b".EXE"[..], b".COM", b".CMD", b".BAT"].iter().any(|e| up.ends_with(e))
}

// ---- The source ----

/// The DRAGITEM type of a file.
fn file_type(p: &[u8], dir: bool) -> &'static [u8] {
    if dir {
        return b"Unknown";
    }
    let up = p.to_ascii_uppercase();
    let ext = up.iter().rposition(|&c| c == b'.').map_or(&b""[..], |i| &up[i + 1..]);
    match ext {
        b"TXT" | b"DOC" | b"LOG" | b"ME" | b"1ST" | b"SYS" | b"CFG" | b"NFO" | b"H" | b"C" | b"RS" | b"MD" | b"INI" => b"Plain Text",
        b"EXE" | b"COM" => b"Executable",
        b"CMD" => b"OS/2 Command File",
        b"BAT" => b"DOS Command File",
        b"DLL" => b"Dynamic Link Library",
        b"BMP" => b"Bitmap",
        b"ICO" => b"Icon",
        _ => b"Unknown",
    }
}

/// CN_INITDRAG (CNRDRAGINIT: hwndCnr, pRecord, x, y, cx, cy): drag the
/// record (and the other selected ones).
pub fn begin(v: usize, di: u32) -> u32 {
    let rec = rd(di, 4);
    if rec == 0 {
        return 0;
    }
    let key = rd(rec, R_KEY);
    let list: Vec<usize> = view::selected(v, key).into_iter().filter(|&i| !matches!(sh().views[v].items[i].obj, Obj::Placeholder)).collect();
    if list.is_empty() {
        return 0;
    }
    let pdinfo = unsafe { DrgAllocDraginfo(list.len() as u32) };
    if pdinfo == 0 {
        return 0;
    }
    let client = sh().views[v].client;
    let cnr = sh().views[v].cnr;
    for (k, &i) in list.iter().enumerate() {
        let it = &sh().views[v].items[i];
        let mut d = [0u8; DRAGITEM_SIZE as usize];
        let w32 = |d: &mut [u8], off: usize, x: u32| d[off..off + 4].copy_from_slice(&x.to_le_bytes());
        w32(&mut d, DI_HWND, client);
        w32(&mut d, DI_ID, it.key);
        let (ty, rmf, cont, srcn, ops, ctl): (&[u8], &[u8], Vec<u8>, Vec<u8>, u16, u16) = match it.obj {
            Obj::File { dir } => {
                let p = &it.path;
                (file_type(p, dir), b"<DRM_OS2FILE,DRF_UNKNOWN>", fileops::parent(p), fileops::name_of(p).to_vec(), DO_COPYABLE | DO_MOVEABLE | DO_LINKABLE, if dir { DC_CONTAINER } else { 0 })
            }
            Obj::Program(_) => (b"Unknown", b"<DRM_OBJECT,DRF_OBJECT>", view::folder_id(&sh().views[v].kind), view::object_id(it), DO_COPYABLE | DO_MOVEABLE | DO_LINKABLE, 0),
            // A shadow of a file is dragged as its file.
            Obj::Shadow(_) => match view::target_of(it) {
                Some(Target::Path(p)) => {
                    let dir = fileops::is_dir(&p);
                    (file_type(&p, dir), b"<DRM_OS2FILE,DRF_UNKNOWN>", fileops::parent(&p), fileops::name_of(&p).to_vec(), DO_COPYABLE | DO_MOVEABLE | DO_LINKABLE, if dir { DC_CONTAINER } else { 0 })
                }
                _ => (b"Unknown", b"<DRM_OBJECT,DRF_OBJECT>", view::folder_id(&sh().views[v].kind), view::object_id(it), DO_COPYABLE | DO_MOVEABLE | DO_LINKABLE, 0),
            },
            _ => (b"Unknown", b"<DRM_OBJECT,DRF_OBJECT>", view::folder_id(&sh().views[v].kind), view::object_id(it), DO_MOVEABLE | DO_LINKABLE, DC_CONTAINER),
        };
        w32(&mut d, DI_TYPE, add_str(ty));
        w32(&mut d, DI_RMF, add_str(rmf));
        w32(&mut d, DI_CONTAINER, add_str(&cont));
        w32(&mut d, DI_SOURCE, add_str(&srcn));
        w32(&mut d, DI_TARGET, add_str(&srcn));
        d[DI_CONTROL..DI_CONTROL + 2].copy_from_slice(&ctl.to_le_bytes());
        d[DI_OPS..DI_OPS + 2].copy_from_slice(&ops.to_le_bytes());
        unsafe { DrgSetDragitem(pdinfo, d.as_ptr(), DRAGITEM_SIZE, k as u32) };
        send(cnr, CM_SETRECORDEMPHASIS, it.rec, mp(1, CRA_SOURCE));
    }
    // DRAGIMAGE: cb, cptl, hImage, sizlStretch, fl, cxOffset, cyOffset.
    let icon = if list.len() > 1 { sys_icon_multi() } else { sh().views[v].items[list[0]].icon };
    let mut img = [0u8; 24];
    img[0..2].copy_from_slice(&24u16.to_le_bytes());
    img[4..8].copy_from_slice(&icon.to_le_bytes());
    img[16..20].copy_from_slice(&DRG_ICON.to_le_bytes());
    let keys: Vec<u32> = list.iter().map(|&i| sh().views[v].items[i].key).collect();
    unsafe { SOURCE = Some(Source { client, keys: keys.clone(), start: PointL { x: rd(di, 8) as i32, y: rd(di, 12) as i32 } }) };
    let target = unsafe { DrgDrag(client, pdinfo, img.as_ptr(), 1, VK_ENDDRAG, 0) };
    unsafe { SOURCE = None };
    if target == 0 {
        // Nobody took it: the string handles are the source's to delete.
        unsafe { DrgDeleteDraginfoStrHandles(pdinfo) };
    }
    unsafe { DrgFreeDraginfo(pdinfo) };
    // The view may have been refilled meanwhile.
    if let Some(v) = view::find_client(client) {
        for k in keys {
            if let Some(i) = view::item_by_key(v, k) {
                send(sh().views[v].cnr, CM_SETRECORDEMPHASIS, sh().views[v].items[i].rec, mp(0, CRA_SOURCE));
            }
        }
    }
    1
}

fn sys_icon_multi() -> u32 {
    crate::sys_icon(SPTR_MULTFILE)
}

/// DM_RENDER (DRAGTRANSFER: cb, hwndClient, pditem, hstrSelectedRMF,
/// hstrRenderToName, ulTargetInfo, usOperation, fsReply): a target wants
/// a file at a name of its choosing.
pub fn render(_client: u32, x: u32) -> u32 {
    if x == 0 {
        return 0;
    }
    let (to_win, item, dest, op) = (rd(x, 4), rd(x, 8), str_handle(rd(x, 16)), rd(x, 24) & 0xffff);
    let src = fileops::join(&str_handle(rd(item, DI_CONTAINER)), &str_handle(rd(item, DI_SOURCE)));
    if dest.is_empty() || !fileops::exists(&src) {
        return 0;
    }
    let mut ok = |_: &[u8], _: u32| true;
    let rc = if op == DO_MOVE { fileops::move_path(&src, &dest, false, &mut ok) } else { fileops::copy_tree(&src, &dest, false, &mut ok) };
    unsafe { DrgPostTransferMsg(to_win, DM_RENDERCOMPLETE, x, if rc == 0 { DMFL_RENDEROK } else { DMFL_RENDERFAIL }, 0, 0) };
    if op == DO_MOVE && rc == 0 {
        view::refresh_dir(&fileops::parent(&src));
    }
    1
}

/// DM_ENDCONVERSATION (ulItemID, flags): a target is done with an item;
/// it may have moved or deleted the file.
pub fn end_conversation(client: u32, _id: u32, flags: u32) {
    if flags & DMFL_TARGETSUCCESSFUL == 0 {
        return;
    }
    if let Some(v) = view::find_client(client) {
        if matches!(sh().views[v].kind, Kind::Dir(_)) && source().is_none() {
            view::fill(v);
        }
    }
}

// ---- The target ----

fn items(pdinfo: u32) -> Vec<(u32, Src)> {
    let n = unsafe { DrgQueryDragitemCount(pdinfo) };
    let mut out = Vec::new();
    for i in 0..n {
        let it = unsafe { DrgQueryDragitemPtr(pdinfo, i) };
        if it == 0 {
            continue;
        }
        let (hwnd, id) = (rd(it, DI_HWND), rd(it, DI_ID));
        if let Some(v) = view::find_client(hwnd) {
            if let Some(k) = view::item_by_key(v, id) {
                out.push((it, Src::Ours(v, k)));
                continue;
            }
        }
        let file = b"DRM_OS2FILE\0";
        if unsafe { DrgVerifyRMF(it, file.as_ptr(), core::ptr::null()) } != 0 {
            let (c, s) = (str_handle(rd(it, DI_CONTAINER)), str_handle(rd(it, DI_SOURCE)));
            if !c.is_empty() && !s.is_empty() {
                out.push((it, Src::File(fileops::join(&c, &s))));
                continue;
            }
        }
        // Something the shell cannot take.
        return Vec::new();
    }
    out
}

/// The path of a file item (ours or another program's).
fn file_path(s: &Src) -> Option<Vec<u8>> {
    match s {
        Src::File(p) => Some(p.clone()),
        Src::Ours(v, i) => {
            let it = &sh().views[*v].items[*i];
            match it.obj {
                Obj::File { .. } => Some(it.path.clone()),
                Obj::Shadow(_) => match view::target_of(it) {
                    Some(Target::Path(p)) => Some(p),
                    _ => None,
                },
                _ => None,
            }
        }
    }
}

/// What the drop goes into: the record under the pointer, else the
/// folder itself. Dragged objects are not targets in their own folder.
fn destination(v: usize, rec: u32) -> Option<Dest> {
    let key = if rec != 0 { rd(rec, R_KEY) } else { 0 };
    let own = source().is_some_and(|s| s.client == sh().views[v].client && s.keys.contains(&key));
    let i = if own { None } else { view::item_by_key(v, key) };
    let Some(i) = i else {
        return match &sh().views[v].kind {
            Kind::Drives | Kind::Minimized => None,
            Kind::Dir(_) if sh().views[v].view == CV_TREE => None,
            k => Some(Dest::Folder(k.clone())),
        };
    };
    let it = &sh().views[v].items[i];
    let dir_of = |p: &[u8]| {
        let mut d = p.to_vec();
        if !d.ends_with(b"\\") {
            d.push(b'\\');
        }
        Dest::Folder(Kind::Dir(d))
    };
    match it.obj {
        Obj::Group(h) => Some(Dest::Folder(Kind::Group(h))),
        Obj::Drive(c) => Some(Dest::Folder(Kind::Dir(alloc::vec![c, b':', b'\\']))),
        Obj::File { dir: true } => Some(dir_of(&it.path)),
        Obj::File { dir: false } if is_program_file(&it.path) => Some(Dest::Exe(it.path.clone())),
        Obj::Program(h) => Some(Dest::Program(h)),
        Obj::Shadow(_) => match view::target_of(it)? {
            Target::Path(p) if fileops::is_dir(&p) => Some(dir_of(&p)),
            Target::Path(p) if is_program_file(&p) => Some(Dest::Exe(p)),
            Target::Program(h) => Some(Dest::Program(h)),
            Target::Group(h) => Some(Dest::Folder(Kind::Group(h))),
            Target::Drive(c) => Some(Dest::Folder(Kind::Dir(alloc::vec![c, b':', b'\\']))),
            _ => None,
        },
        _ => None,
    }
}

/// (operations allowed, default) for one item dropped into a folder.
fn item_ops(s: &Src, to: &Kind, into_view: usize) -> (u8, u8) {
    let same_view = |sv: usize| sv == into_view && sh().views[into_view].kind == *to;
    if let Src::Ours(sv, i) = s {
        let it = &sh().views[*sv].items[*i];
        let from = &sh().views[*sv].kind;
        let reposition = same_view(*sv) && sh().views[into_view].view == CV_ICON;
        match it.obj {
            Obj::File { .. } => {}
            Obj::Program(_) => {
                return match to {
                    Kind::Desktop | Kind::Group(_) if view::group_of(from) == view::group_of(to) => (if reposition { M } else { 0 } | L, M),
                    Kind::Desktop | Kind::Group(_) => (C | M | L, M),
                    _ => (L, L),
                };
            }
            Obj::Shadow(_) => {
                let here = view::folder_id(from) == view::folder_id(to);
                return if here { (if reposition { M } else { 0 } | C | L, M) } else { (C | M | L, M) };
            }
            Obj::Placeholder | Obj::Window(_) => return (0, 0),
            _ => {
                // Groups, drives, the desktop's objects: they move in
                // their own folder; elsewhere they get shadows.
                return if reposition { (M | L, M) } else { (L, L) };
            }
        }
    }
    let Some(p) = file_path(s) else { return (0, 0) };
    match to {
        Kind::Dir(d) => {
            if fileops::is_within(d, &p) {
                return (0, 0);
            }
            if fileops::same_path(&fileops::parent(&p), d) {
                // Its own directory: a copy there (asking for a name), a
                // shadow, or moving the icon (a directory is arranged).
                return (C | L, 0);
            }
            let def = if pmwin::drag::file_default(fileops::drive_of(&p), fileops::drive_of(d)) == pmwin::drag::DO_MOVE { M } else { C };
            (C | M | L, def)
        }
        Kind::Desktop | Kind::Group(_) => {
            if is_program_file(&p) { (P | L, P) } else { (L, L) }
        }
        _ => (0, 0),
    }
}

fn op_bit(op: u32) -> u8 {
    match op {
        DO_COPY => C,
        DO_MOVE => M,
        DO_LINK => L,
        DO_CREATEPROGRAMOBJECT => P,
        _ => 0,
    }
}

fn bit_op(b: u8) -> u32 {
    match b {
        C => DO_COPY,
        M => DO_MOVE,
        L => DO_LINK,
        _ => DO_CREATEPROGRAMOBJECT,
    }
}

/// The drop's (DOR_*, DO_*).
fn decide(v: usize, pdinfo: u32, rec: u32) -> (u32, u32, Option<Dest>, Vec<(u32, Src)>) {
    let list = items(pdinfo);
    if list.is_empty() {
        return (DOR_NEVERDROP, 0, None, list);
    }
    let Some(dest) = destination(v, rec) else { return (DOR_NODROP, 0, None, list) };
    // The user's operation (DRAGINFO usOperation).
    let user = rd(pdinfo, 4) >> 16;
    match &dest {
        Dest::Program(_) | Dest::Exe(_) => {
            if list.iter().all(|(_, s)| file_path(s).is_some()) {
                let op = if user == DO_DEFAULT { DO_UNKNOWN } else { user };
                return (DOR_DROP, op, Some(dest), list);
            }
            (DOR_NODROPOP, 0, None, list)
        }
        Dest::Folder(k) => {
            let mut allowed = 0xffu8;
            let mut default = 0u8;
            for (n, (_, s)) in list.iter().enumerate() {
                let (a, d) = item_ops(s, k, v);
                allowed &= a;
                if n == 0 {
                    default = d;
                }
            }
            if allowed == 0 {
                return (DOR_NODROP, 0, None, list);
            }
            let want = if user == DO_DEFAULT {
                // No default (a file over its own directory): only with keys.
                if default == 0 {
                    return (DOR_NODROP, 0, None, list);
                }
                if allowed & default != 0 { default } else { [M, C, L, P].into_iter().find(|b| allowed & b != 0).unwrap_or(0) }
            } else {
                op_bit(user)
            };
            if want == 0 || allowed & want == 0 {
                return (DOR_NODROPOP, 0, None, list);
            }
            (DOR_DROP, bit_op(want), Some(dest), list)
        }
    }
}

/// CN_DRAGOVER (CNRDRAGINFO: pDragInfo, pRecord).
pub fn drag_over(v: usize, cdi: u32) -> u32 {
    let (pdinfo, rec) = (rd(cdi, 0), rd(cdi, 4));
    if pdinfo == 0 || unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
        return DOR_NEVERDROP;
    }
    let (dor, op, ..) = decide(v, pdinfo, rec);
    unsafe { DrgFreeDraginfo(pdinfo) };
    mp(dor, op)
}

/// CN_DROP: do it.
pub fn drop(v: usize, cdi: u32) {
    let (pdinfo, rec) = (rd(cdi, 0), rd(cdi, 4));
    if pdinfo == 0 || unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
        return;
    }
    let (dor, op, dest, list) = decide(v, pdinfo, rec);
    let drop_at = PointL { x: (rd(pdinfo, 12) & 0xffff) as i16 as i32, y: (rd(pdinfo, 12) >> 16) as i16 as i32 };
    let ok = dor == DOR_DROP;
    if ok {
        if let Some(dest) = dest {
            perform(v, dest, op, &list, drop_at);
        }
    }
    // Every item's conversation ends (posted: the source may be waiting
    // for this DM_DROP to come back).
    for (it, _) in &list {
        post(rd(*it, DI_HWND), DM_ENDCONVERSATION, rd(*it, DI_ID), if ok { DMFL_TARGETSUCCESSFUL } else { DMFL_TARGETFAIL });
    }
    unsafe {
        DrgDeleteDraginfoStrHandles(pdinfo);
        DrgFreeDraginfo(pdinfo);
    }
}

/// Parameters naming files (quoted where they have blanks).
fn file_params(list: &[(u32, Src)]) -> Vec<u8> {
    let mut p = Vec::new();
    for (_, s) in list {
        let Some(f) = file_path(s) else { continue };
        if !p.is_empty() {
            p.push(b' ');
        }
        if f.contains(&b' ') {
            p.push(b'"');
            p.extend_from_slice(&f);
            p.push(b'"');
        } else {
            p.extend_from_slice(&f);
        }
    }
    p
}

fn perform(v: usize, dest: Dest, op: u32, list: &[(u32, Src)], at: PointL) {
    match dest {
        Dest::Program(h) => {
            if let Some(mut p) = crate::objects::definition(h) {
                let files = file_params(list);
                if !p.params.is_empty() {
                    p.params.push(b' ');
                }
                p.params.extend_from_slice(&files);
                crate::launch::start_program(&p);
            }
        }
        Dest::Exe(path) => {
            crate::launch::start(&path, &file_params(list), &fileops::parent(&path), b"", PROG_DEFAULT);
        }
        Dest::Folder(to) => {
            // Moving icons within their own folder.
            let reposition = op == DO_MOVE
                && list.iter().all(|(_, s)| matches!(s, Src::Ours(sv, _) if *sv == v))
                && sh().views[v].kind == to
                && sh().views[v].view == CV_ICON
                && source().is_some();
            if reposition {
                let s = source().unwrap();
                let (dx, dy) = (at.x - s.start.x, at.y - s.start.y);
                let keys: Vec<u32> = list.iter().filter_map(|(_, s)| if let Src::Ours(_, i) = s { Some(sh().views[v].items[*i].key) } else { None }).collect();
                for &k in &keys {
                    if let Some(i) = view::item_by_key(v, k) {
                        let r = sh().views[v].items[i].rec;
                        wr(r, R_X, (rd(r, R_X) as i32 + dx).max(0) as u32);
                        wr(r, R_Y, (rd(r, R_Y) as i32 + dy).max(0) as u32);
                    }
                }
                view::reposition(v);
                view::save_positions(v, Some(&keys));
                return;
            }
            // The rest, as the menus do it; from another program's files
            // as if from their directory.
            let mut groups: Vec<(Kind, Vec<(Obj, Vec<u8>, Vec<u8>, Option<Target>)>)> = Vec::new();
            for (_, s) in list {
                let (from, entry) = match s {
                    Src::Ours(sv, i) => {
                        let it = &sh().views[*sv].items[*i];
                        (sh().views[*sv].kind.clone(), (it.obj, it.name().to_vec(), it.path.clone(), view::target_of(it)))
                    }
                    Src::File(p) => {
                        let dir = fileops::is_dir(p);
                        let mut d = fileops::parent(p);
                        if d.is_empty() {
                            d = p.clone();
                        }
                        (Kind::Dir(d), (Obj::File { dir }, fileops::name_of(p).to_vec(), p.clone(), Some(Target::Path(p.clone()))))
                    }
                };
                match groups.iter_mut().find(|g| g.0 == from) {
                    Some(g) => g.1.push(entry),
                    None => groups.push((from, alloc::vec![entry])),
                }
            }
            // The drop point in the target container's coordinates.
            let mut pt = [at];
            unsafe { WinMapWindowPoints(HWND_DESKTOP, sh().views[v].cnr, pt.as_mut_ptr(), 1) };
            let place = (sh().views[v].kind == to).then_some((pt[0].x, pt[0].y));
            for (from, ops) in groups {
                view::perform(&from, ops, &to, op, None, place);
            }
        }
    }
}
