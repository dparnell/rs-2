//! DrgDragFiles and DrgAcceptDroppedFiles: dragging files with the
//! rendering done for the program, as IBM's PMCTLS does it (DRGFILES.C:
//! `DragFilesWndProc`, `DropFilesWndProc`, `FileThread`).
//!
//! - The source: DrgDragFiles makes an object window ("DragFile Object
//!   Window", owned by the caller's window) that stands for the files, a
//!   DRAGINFO of `<DRM_OS2FILE,DRF_UNKNOWN>` items (the file name's handle
//!   in ulItemID, the type given or the file's .TYPE EA, else "Unknown";
//!   no source name if the caller renders) and DrgDrag. After a drop the
//!   object window answers DM_RENDERPREPARE and DM_RENDER: a thread
//!   copies or moves the file to the name the target asks for (DM_DRAGERROR
//!   to the owner when that fails: abort, skip, replace, retry or a new
//!   name), or the owner renders it (DM_RENDERFILE, then DM_FILERENDERED
//!   to the object window); DM_RENDERCOMPLETE goes to the target. On each
//!   DM_ENDCONVERSATION the owner gets DM_DRAGFILECOMPLETE (DF_SOURCE);
//!   the object window goes after the last.
//! - The target: DrgAcceptDroppedFiles subclasses a window. It takes
//!   DO_COPY and DO_MOVE (and the default, as its own default operation)
//!   of files with one of the given types (any if none), asks its window
//!   to show target emphasis (DM_EMPHASIZETARGET), and on DM_DROP asks the
//!   source to render each file into its directory (the current one if
//!   none was given), one after another; each file done is
//!   DM_DRAGFILECOMPLETE to the window.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::*;

unsafe extern "C" {
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: *const u8, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.839"]
    fn WinQueryWindowPtr(h: u32, i: i32) -> u32;
    #[link_name = "PMWIN.876"]
    fn WinSetWindowPtr(h: u32, i: i32, p: u32) -> u32;
    #[link_name = "PMWIN.929"]
    fn WinSubclassWindow(h: u32, proc_: WndProc) -> Option<WndProc>;
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.822"]
    fn WinQueryPointerInfo(hptr: u32, info: *mut [u32; 8]) -> u32;
    #[link_name = "PMGPI.573"]
    fn GpiQueryBitmapParameters(hbm: u32, info: *mut [u16; 6]) -> u32;
}

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

const HWND_OBJECT: u32 = 2;
const HWND_BOTTOM: u32 = 4;
const QW_OWNER: i32 = 4;
const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;

const DM_RENDERFILE: u32 = 0x0322;
const DM_FILERENDERED: u32 = 0x0323;
const DM_DRAGERROR: u32 = 0x0324;
const DM_EMPHASIZETARGET: u32 = 0x0325;
const DM_DRAGFILECOMPLETE: u32 = 0x0326;
const DM_RENDERCOMPLETE: u32 = 0x0328;
const DM_ENDCONVERSATION: u32 = 0x032b;
/// The object window's own: has the drag been aborted (DME_IGNOREABORT)?
const WM_DF_ABORTED: u32 = 0x1001;
const WM_DF_ABORT: u32 = 0x100a;

const DMFL_TARGETSUCCESSFUL: u32 = 0x01;
const DMFL_TARGETFAIL: u32 = 0x02;
const DMFL_NATIVERENDER: u16 = 0x04;
const DMFL_RENDERRETRY: u16 = 0x08;
const DMFL_RENDEROK: u32 = 0x10;
const DMFL_RENDERFAIL: u32 = 0x20;
const DC_PREPARE: u16 = 0x10;
const DF_MOVE: u32 = 1;
const DF_SOURCE: u32 = 2;
const DF_SUCCESSFUL: u32 = 4;
const DOR_NEVERDROP: u32 = 3;
const DFF_MOVE: u32 = 1;
const DFF_COPY: u32 = 2;
const DFF_DELETE: u32 = 3;
const DME_IGNOREABORT: u16 = 1;
const DME_IGNORECONTINUE: u16 = 2;
const DME_REPLACE: u16 = 3;
const DME_RETRY: u16 = 4;
const DCPY_EXISTING: u32 = 1;

const RMF_FILE: &[u8] = b"<DRM_OS2FILE,DRF_UNKNOWN>\0";
const CLASS: &[u8] = b"DragFile Object Window\0";

// DRAGITEM fields (beside IT_TYPE, IT_RMF).
const IT_HWND: u32 = 0;
const IT_ID: u32 = 4;
const IT_CONTAINER: u32 = 16;
const IT_SOURCE: u32 = 20;
const IT_TARGET: u32 = 24;
const IT_CONTROL: u32 = 32;

// DRAGTRANSFER fields.
const DX_CLIENT: u32 = 4;
const DX_ITEM: u32 = 8;
const DX_RMF: u32 = 12;
const DX_RENDERTO: u32 = 16;
const DX_TARGETINFO: u32 = 20;
const DX_OP: u32 = 24;
const DX_REPLY: u32 = 26;

fn add_str(s: &[u8]) -> u32 {
    let mut z = s.to_vec();
    z.push(0);
    DrgAddStrHandle(z.as_ptr())
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// A file's .TYPE extended attribute as a list of types ("a,b").
fn type_from_ea(path: &[u8]) -> Option<Vec<u8>> {
    // EAOP2: a GEA2LIST with ".TYPE" and room for the FEA2LIST.
    let name = b".TYPE";
    let mut gea = Vec::new();
    gea.extend_from_slice(&(name.len() as u32 + 10).to_le_bytes());
    gea.extend_from_slice(&0u32.to_le_bytes());
    gea.push(name.len() as u8);
    gea.extend_from_slice(name);
    gea.push(0);
    let mut fea = alloc::vec![0u8; 0x200];
    fea[0..4].copy_from_slice(&0x200u32.to_le_bytes());
    let eaop = [gea.as_ptr() as u32, fea.as_mut_ptr() as u32, 0];
    let p = z(path);
    if unsafe { os2::DosQueryPathInfo(p.as_ptr(), 3, eaop.as_ptr() as *mut u8, 12) } != 0 {
        return None;
    }
    // FEA2: oNext, fEA, cbName, cbValue, name, NUL, value.
    let cb_name = fea[9] as usize;
    let cb_value = u16::from_le_bytes([fea[10], fea[11]]) as usize;
    let v = fea.get(4 + 8 + cb_name + 1..4 + 8 + cb_name + 1 + cb_value)?;
    let mut out = Vec::new();
    let mut at = 0;
    let mut left = 1usize;
    while left > 0 && at + 4 <= v.len() {
        let t = u16::from_le_bytes([v[at], v[at + 1]]);
        match t {
            // EAT_MVMT: code page, count.
            0xffdf => {
                left = u16::from_le_bytes([v[at + 4], v[at + 5]]) as usize + 1;
                at += 6;
            }
            // EAT_ASCII.
            0xfffd => {
                let n = u16::from_le_bytes([v[at + 2], v[at + 3]]) as usize;
                let s = v.get(at + 4..at + 4 + n)?;
                if !out.is_empty() {
                    out.push(b',');
                }
                out.extend_from_slice(s);
                at += 4 + n;
            }
            _ => break,
        }
        left -= 1;
    }
    (!out.is_empty()).then_some(out)
}

fn ptr_at(array: u32, i: u32) -> u32 {
    if array == 0 { 0 } else { rd(array, i * 4) }
}

// ---- The source ----

/// The object window's data.
#[repr(C)]
struct Source {
    /// Conversations not ended yet.
    count: u32,
    moving: u32,
    source_render: u32,
    aborted: u32,
}

/// DrgDragFiles(hwnd, apszFiles, apszTypes, apszTargets, cFiles,
/// hptrDrag, vkTerm, fSourceRender, ulRsvd)
#[unsafe(no_mangle)]
pub extern "C" fn DrgDragFiles(h: u32, files: u32, types: u32, targets: u32, n: u32, hptr: u32, vk: u32, render: u32, _rsvd: u32) -> u32 {
    if n == 0 {
        return 1;
    }
    let hab = unsafe { WinQueryAnchorBlock(h) };
    unsafe { WinRegisterClass(hab, CLASS.as_ptr(), source_proc, 0, 4) };
    let data = Box::into_raw(Box::new(Source { count: n, moving: 0, source_render: render, aborted: 0 }));
    let obj = unsafe { WinCreateWindow(HWND_OBJECT, CLASS.as_ptr(), core::ptr::null(), 0, 0, 0, 0, 0, h, HWND_BOTTOM, 0, data as *const u8, core::ptr::null()) };
    let di = if obj != 0 { DrgAllocDraginfo(n) } else { 0 };
    let mut img = [0u8; 0x18];
    if obj == 0 || di == 0 || !init_draginfo(obj, di, files, types, targets, n, render != 0) || !init_image(&mut img, hptr) {
        if obj != 0 {
            unsafe { WinDestroyWindow(obj) };
        } else {
            drop(unsafe { Box::from_raw(data) });
        }
        if di != 0 {
            DrgFreeDraginfo(di);
        }
        return 0;
    }
    if DrgDrag(obj, di, img.as_ptr() as u32, 1, vk, 0) != 0 {
        return 1;
    }
    for i in 0..count(di) {
        let it = DrgQueryDragitemPtr(di, i);
        for off in [IT_TYPE, IT_ID, IT_TARGET] {
            DrgDeleteStrHandle(rd(it, off));
        }
    }
    DrgDeleteStrHandle(rd(DrgQueryDragitemPtr(di, 0), IT_RMF));
    DrgFreeDraginfo(di);
    unsafe { WinDestroyWindow(obj) };
    0
}

fn init_draginfo(obj: u32, di: u32, files: u32, types: u32, targets: u32, n: u32, source_render: bool) -> bool {
    wr(di, DI_SOURCE, obj);
    let rmf = DrgAddStrHandle(RMF_FILE.as_ptr());
    for i in 0..n {
        let file = cstr(ptr_at(files, i));
        let mut full = alloc::vec![0u8; 0x104];
        let f = z(&file);
        // FIL_QUERYFULLNAME
        if unsafe { os2::DosQueryPathInfo(f.as_ptr(), 5, full.as_mut_ptr(), 0x104) } != 0 {
            error(0x1503);
            return false;
        }
        let full = cstr(full.as_ptr() as u32);
        let cut = full.iter().rposition(|&c| c == b'\\').map_or(0, |p| p + 1);
        let (dir, name) = full.split_at(cut);
        let container = add_str(dir);
        let name_h = add_str(name);
        let target = match ptr_at(targets, i) {
            0 => add_str(name),
            p => add_str(&cstr(p)),
        };
        let ty = match ptr_at(types, i) {
            0 => add_str(&type_from_ea(&full).unwrap_or_else(|| b"Unknown".to_vec())),
            p => add_str(&cstr(p)),
        };
        let mut it = [0u32; 9];
        it[0] = obj;
        it[1] = name_h;
        it[2] = ty;
        it[3] = rmf;
        it[4] = container;
        it[5] = if source_render { 0 } else { name_h };
        it[6] = target;
        // fsControl 0, fsSupportedOps DO_COPYABLE | DO_MOVEABLE.
        it[8] = 3 << 16;
        DrgSetDragitem(di, it.as_ptr() as u32, DRAGITEM, i);
    }
    true
}

/// A DRAGIMAGE of the pointer, its hot spot under the mouse pointer's.
fn init_image(img: &mut [u8; 0x18], hptr: u32) -> bool {
    let mut info = [0u32; 8];
    let mut bmp = [12u16, 0, 0, 0, 0, 0];
    // POINTERINFO: fPointer, xHotspot, yHotspot, hbmPointer, ...
    if unsafe { WinQueryPointerInfo(hptr, &mut info) } == 0 || unsafe { GpiQueryBitmapParameters(info[3], &mut bmp) } == 0 {
        return false;
    }
    img[0..2].copy_from_slice(&0x18u16.to_le_bytes());
    img[4..8].copy_from_slice(&hptr.to_le_bytes());
    img[16..20].copy_from_slice(&DRG_ICON.to_le_bytes());
    let (mut x, mut y) = (info[1] as i16, info[2] as i16);
    // An icon (not a pointer): its middle, a quarter up.
    if info[0] == 0 {
        x = (bmp[2] / 2) as i16;
        y = (bmp[3] / 4) as i16;
    }
    img[20..22].copy_from_slice(&(-x).to_le_bytes());
    img[22..24].copy_from_slice(&(-y).to_le_bytes());
    true
}

fn source_of(h: u32) -> Option<&'static mut Source> {
    unsafe { (WinQueryWindowPtr(h, 0) as *mut Source).as_mut() }
}

extern "C" fn source_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_CREATE {
        unsafe { WinSetWindowPtr(h, 0, mp1) };
        return 0;
    }
    let Some(s) = source_of(h) else { return 0 };
    match msg {
        DM_FILERENDERED => file_rendered(mp1, mp2 & 0xffff != 0),
        DM_RENDERPREPARE => return (s.aborted == 0) as u32,
        DM_RENDER => return render(h, s, mp1) as u32,
        DM_ENDCONVERSATION => {
            if s.aborted == 0 {
                let fl = (s.moving != 0) as u32 * DF_MOVE | DF_SOURCE | if mp2 & 0xffff != 0 { DF_SUCCESSFUL } else { 0 };
                let owner = unsafe { WinQueryWindow(h, QW_OWNER) };
                unsafe { WinSendMsg(owner, DM_DRAGFILECOMPLETE, mp1, fl) };
            }
            s.count -= 1;
            if s.count == 0 {
                unsafe { WinDestroyWindow(h) };
            }
        }
        WM_DF_ABORT => s.aborted = 1,
        WM_DF_ABORTED => return s.aborted,
        WM_DESTROY => {
            unsafe { WinSetWindowPtr(h, 0, 0) };
            drop(unsafe { Box::from_raw(s as *mut Source) });
        }
        _ => return unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
    0
}

/// RENDERFILE (and the DRAGTRANSFER it renders for, behind it).
#[repr(C)]
struct RenderFile {
    obj: u32,
    source: u32,
    target: u32,
    moving: u16,
    rsvd: u16,
    xfer: u32,
}

/// The owner rendered a file it was given with DM_RENDERFILE.
fn file_rendered(prf: u32, ok: bool) {
    let rf = unsafe { Box::from_raw(prf as *mut RenderFile) };
    let fl = if ok { DMFL_RENDEROK } else { DMFL_RENDERFAIL };
    DrgPostTransferMsg(rd(rf.xfer, DX_CLIENT), DM_RENDERCOMPLETE, rf.xfer, fl, 0, 1);
    DrgFreeDragtransfer(rf.xfer);
}

fn render(obj: u32, s: &mut Source, xfer: u32) -> bool {
    if s.aborted != 0 {
        return false;
    }
    let it = rd(xfer, DX_ITEM);
    // The source name the item left out.
    wr(it, IT_SOURCE, rd(it, IT_ID));
    let op = rd16(xfer, DX_OP);
    if op != rmf::DO_MOVE && op != rmf::DO_COPY {
        wr16(xfer, DX_REPLY, DMFL_NATIVERENDER);
        return false;
    }
    s.moving = (op == rmf::DO_MOVE) as u32;
    let mut owner_does = false;
    if s.source_render != 0 {
        if !rmf_of(rd(xfer, DX_RMF)) {
            wr16(xfer, DX_REPLY, DMFL_RENDERRETRY);
            return false;
        }
        let rf = Box::into_raw(Box::new(RenderFile { obj, source: rd(it, IT_SOURCE), target: rd(xfer, DX_RENDERTO), moving: s.moving as u16, rsvd: 0, xfer }));
        let owner = unsafe { WinQueryWindow(obj, QW_OWNER) };
        owner_does = unsafe { WinSendMsg(owner, DM_RENDERFILE, rf as u32, 0) } != 0;
        if !owner_does {
            drop(unsafe { Box::from_raw(rf) });
        }
    }
    if !owner_does {
        let name = |h| str_name(h).unwrap_or_default();
        let mut src = name(rd(it, IT_CONTAINER));
        src.extend_from_slice(&name(rd(it, IT_SOURCE)));
        let job = Box::into_raw(Box::new(Job { obj, source: src, target: name(rd(xfer, DX_RENDERTO)), xfer, moving: s.moving != 0, copy_opts: 0 }));
        let mut tid = 0;
        if unsafe { os2::DosCreateThread(&mut tid, file_thread, job as u32, 0, 0x4000) } != 0 {
            drop(unsafe { Box::from_raw(job) });
            wr16(xfer, DX_REPLY, DMFL_RENDERRETRY);
            return false;
        }
    }
    true
}

/// The selected RMF is the file mechanism.
fn rmf_of(h: u32) -> bool {
    str_name(h).is_some_and(|s| rmf::rmf_has(&s, Some(b"DRM_OS2FILE"), Some(b"DRF_UNKNOWN")))
}

/// A copy or move the object window's thread runs.
struct Job {
    obj: u32,
    source: Vec<u8>,
    target: Vec<u8>,
    xfer: u32,
    moving: bool,
    copy_opts: u32,
}

unsafe extern "C" fn file_thread(p: u32) {
    let mut job = unsafe { Box::from_raw(p as *mut Job) };
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let ok = if unsafe { WinSendMsg(job.obj, WM_DF_ABORTED, 0, 0) } != 0 {
        false
    } else if job.moving {
        move_file(&mut job)
    } else {
        render_file(&mut job, DFF_COPY) != 0
    };
    let fl = if ok { DMFL_RENDEROK } else { DMFL_RENDERFAIL };
    DrgPostTransferMsg(rd(job.xfer, DX_CLIENT), DM_RENDERCOMPLETE, job.xfer, fl, 0, 1);
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

fn same_drive(a: &[u8], b: &[u8]) -> bool {
    if a.first() != b.first() {
        return false;
    }
    // UNC names: the same server and share.
    if a.first() == Some(&b'\\') {
        let mut slashes = 0;
        for (x, y) in a.iter().zip(b.iter()) {
            if slashes >= 4 {
                break;
            }
            if x != y {
                return false;
            }
            if *x == b'\\' {
                slashes += 1;
            }
        }
    }
    true
}

fn move_file(job: &mut Job) -> bool {
    if !same_drive(&job.source, &job.target) {
        return render_file(job, DFF_COPY) != 0 && render_file(job, DFF_DELETE) != 0;
    }
    match render_file(job, DFF_MOVE) {
        // Replaced: copied over the target; the source goes.
        2 => render_file(job, DFF_DELETE) != 0,
        r => r != 0,
    }
}

/// Copy, move or delete; on an error the owner decides (DM_DRAGERROR).
/// What was done (DFF_*), or 0.
fn render_file(job: &mut Job, mut mode: u32) -> u32 {
    loop {
        let (s, t) = (z(&job.source), z(&job.target));
        let rc = match mode {
            DFF_COPY => unsafe { os2::DosCopy(s.as_ptr(), t.as_ptr(), job.copy_opts) },
            DFF_MOVE => unsafe { os2::DosMove(s.as_ptr(), t.as_ptr()) },
            DFF_DELETE => unsafe { os2::DosDelete(s.as_ptr()) },
            _ => 0,
        };
        if rc == 0 {
            return mode;
        }
        let h = add_str(&job.source);
        let owner = unsafe { WinQueryWindow(job.obj, QW_OWNER) };
        let r = unsafe { WinSendMsg(owner, DM_DRAGERROR, (rc & 0xffff) | mode << 16, h) };
        DrgDeleteStrHandle(h);
        match r as u16 {
            DME_IGNOREABORT => {
                unsafe { WinSendMsg(job.obj, WM_DF_ABORT, 0, 0) };
                return 0;
            }
            0 | DME_IGNORECONTINUE => return 0,
            DME_REPLACE => {
                job.copy_opts = DCPY_EXISTING;
                if mode == DFF_MOVE {
                    mode = DFF_COPY;
                }
            }
            DME_RETRY => {}
            // A new name for the target.
            _ => {
                job.target = str_name(r).unwrap_or_default();
                wr(job.xfer, DX_RENDERTO, r);
                if mode == DFF_MOVE {
                    return move_file(job) as u32;
                }
            }
        }
    }
}

// ---- The target ----

struct Accept {
    hwnd: u32,
    old: WndProc,
    path: Vec<u8>,
    types: Option<Vec<u8>>,
    op: u16,
    /// The drop being rendered: its DRAGTRANSFER and the next item.
    xfer: u32,
    next: u32,
    busy: bool,
}

static mut ACCEPTS: Vec<Accept> = Vec::new();

fn accepts() -> &'static mut Vec<Accept> {
    unsafe { &mut *core::ptr::addr_of_mut!(ACCEPTS) }
}

fn accept_of(h: u32) -> Option<&'static mut Accept> {
    accepts().iter_mut().find(|a| a.hwnd == h)
}

/// DrgAcceptDroppedFiles(hwnd, pszPath, pszTypes, ulDefaultOp, ulRsvd)
#[unsafe(no_mangle)]
pub extern "C" fn DrgAcceptDroppedFiles(h: u32, path: u32, types: u32, op: u32, _rsvd: u32) -> u32 {
    if accept_of(h).is_some() || (op != rmf::DO_MOVE as u32 && op != rmf::DO_COPY as u32) {
        return 0;
    }
    let path = if path == 0 {
        let (mut disk, mut map) = (0u32, 0u32);
        unsafe { os2::DosQueryCurrentDisk(&mut disk, &mut map) };
        let mut buf = alloc::vec![0u8; 0x101];
        let mut cb = buf.len() as u32;
        unsafe { os2::DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut cb) };
        let mut p = alloc::vec![b'@' + disk as u8, b':', b'\\'];
        p.extend_from_slice(&cstr(buf.as_ptr() as u32));
        p
    } else {
        cstr(path)
    };
    let Some(old) = (unsafe { WinSubclassWindow(h, drop_proc) }) else { return 0 };
    accepts().push(Accept { hwnd: h, old, path, types: (types != 0).then(|| cstr(types)), op: op as u16, xfer: 0, next: 0, busy: false });
    1
}

fn acceptable(it: u32, types: &Option<Vec<u8>>) -> bool {
    let Some(t) = types else { return true };
    if !rmf::rmf_has(&str_name(rd(it, IT_RMF)).unwrap_or_default(), Some(b"DRM_OS2FILE"), Some(b"DRF_UNKNOWN")) {
        return false;
    }
    rmf::has_type(&str_name(rd(it, IT_TYPE)).unwrap_or_default(), t)
}

extern "C" fn drop_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(a) = accept_of(h) else { return 0 };
    match msg {
        DM_DRAGOVER => {
            DrgAccessDraginfo(mp1);
            unsafe { WinSendMsg(h, DM_EMPHASIZETARGET, mp2, 1) };
            let Some(a) = accept_of(h) else { return 0 };
            if a.busy {
                return DOR_NODROP as u32;
            }
            let op = rd16(mp1, DI_OP);
            if op != rmf::DO_COPY && op != rmf::DO_MOVE && op != rmf::DO_DEFAULT {
                return DOR_NODROPOP as u32;
            }
            if (0..count(mp1)).any(|i| acceptable(DrgQueryDragitemPtr(mp1, i), &a.types)) {
                return DOR_DROP as u32 | (a.op as u32) << 16;
            }
            DOR_NEVERDROP
        }
        DM_DRAGLEAVE => {
            unsafe { WinSendMsg(h, DM_EMPHASIZETARGET, rd(mp1, DI_X), 0) };
            DrgFreeDraginfo(mp1);
            0
        }
        DM_DROP => {
            drop_files(h, mp1);
            0
        }
        DM_RENDERCOMPLETE => {
            render_complete(h, mp1, mp2 & 0xffff);
            0
        }
        WM_DESTROY => {
            let old = a.old;
            accepts().retain(|x| x.hwnd != h);
            old(h, msg, mp1, mp2)
        }
        _ => (a.old)(h, msg, mp1, mp2),
    }
}

fn drop_files(h: u32, di: u32) {
    let Some(a) = accept_of(h) else { return };
    a.busy = true;
    a.xfer = DrgAllocDragtransfer(1);
    if a.xfer == 0 {
        for i in 0..count(di) {
            let it = DrgQueryDragitemPtr(di, i);
            DrgSendTransferMsg(rd(it, IT_HWND), DM_ENDCONVERSATION, rd(it, IT_ID), DMFL_TARGETFAIL);
        }
        cleanup(di, 0);
        a.busy = false;
        return;
    }
    send_render(h, di, 0);
}

/// Ask the source to render the items from `from` on, until one says it
/// will; the rest wait for its DM_RENDERCOMPLETE. None left: cleaned up.
fn send_render(h: u32, di: u32, from: u32) {
    let Some(a) = accept_of(h) else { return };
    let (xfer, path, types) = (a.xfer, a.path.clone(), a.types.clone());
    let mut i = from;
    while i < count(di) {
        let it = DrgQueryDragitemPtr(di, i);
        i += 1;
        if !acceptable(it, &types) {
            DrgSendTransferMsg(rd(it, IT_HWND), DM_ENDCONVERSATION, rd(it, IT_ID), DMFL_TARGETFAIL);
            continue;
        }
        let mut to = path.clone();
        if to.last() != Some(&b'\\') {
            to.push(b'\\');
        }
        to.extend_from_slice(&str_name(rd(it, IT_TARGET)).unwrap_or_default());
        wr(xfer, 0, DRAGTRANSFER);
        wr(xfer, DX_CLIENT, h);
        wr(xfer, DX_ITEM, it);
        wr(xfer, DX_RMF, DrgAddStrHandle(RMF_FILE.as_ptr()));
        wr(xfer, DX_RENDERTO, add_str(&to));
        wr16(xfer, DX_OP, rd16(di, DI_OP));
        wr(xfer, DX_TARGETINFO, rd(it, IT_TARGET));
        let mut go = true;
        if rd16(it, IT_CONTROL) & DC_PREPARE != 0 {
            go = DrgSendTransferMsg(rd(it, IT_HWND), DM_RENDERPREPARE, xfer, 0) & 0xffff != 0;
            if !go {
                DrgDeleteStrHandle(rd(xfer, DX_RMF));
                DrgDeleteStrHandle(rd(xfer, DX_RENDERTO));
            }
        }
        if go && render_retrying(it, xfer) {
            if let Some(a) = accept_of(h) {
                a.next = i;
            }
            return;
        }
    }
    cleanup(di, xfer);
    if let Some(a) = accept_of(h) {
        a.busy = false;
        a.xfer = 0;
    }
}

/// DM_RENDER, tried again (a second apart, three times in all) while the
/// source asks for a retry; a refusal ends the item's conversation.
fn render_retrying(it: u32, xfer: u32) -> bool {
    for left in (0..3).rev() {
        if DrgSendTransferMsg(rd(it, IT_HWND), DM_RENDER, xfer, 0) != 0 {
            return true;
        }
        if left == 0 || rd16(xfer, DX_REPLY) & DMFL_RENDERRETRY == 0 {
            break;
        }
        unsafe { os2::DosSleep(1000) };
    }
    DrgDeleteStrHandle(rd(xfer, DX_RMF));
    DrgDeleteStrHandle(rd(xfer, DX_RENDERTO));
    DrgSendTransferMsg(rd(it, IT_HWND), DM_ENDCONVERSATION, rd(it, IT_ID), DMFL_TARGETFAIL);
    false
}

fn render_complete(h: u32, xfer: u32, fl: u32) {
    let it = rd(xfer, DX_ITEM);
    let di = DrgQueryDraginfoPtrFromDragitem(it);
    let ok = fl == DMFL_RENDEROK;
    if ok {
        let moved = (rd16(di, DI_OP) == rmf::DO_MOVE) as u32 * DF_MOVE;
        unsafe { WinSendMsg(h, DM_DRAGFILECOMPLETE, rd(xfer, DX_TARGETINFO), DF_SUCCESSFUL | moved) };
    }
    DrgSendTransferMsg(rd(it, IT_HWND), DM_ENDCONVERSATION, rd(it, IT_ID), if ok { DMFL_TARGETSUCCESSFUL } else { DMFL_TARGETFAIL });
    DrgDeleteStrHandle(rd(xfer, DX_RMF));
    DrgDeleteStrHandle(rd(xfer, DX_RENDERTO));
    let next = accept_of(h).map_or(u32::MAX, |a| a.next);
    send_render(h, di, next);
}

fn cleanup(di: u32, xfer: u32) {
    DrgDeleteDraginfoStrHandles(di);
    DrgFreeDraginfo(di);
    if xfer != 0 {
        DrgFreeDragtransfer(xfer);
    }
}
