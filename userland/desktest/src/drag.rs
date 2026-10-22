//! Direct manipulation through PMDRAG: string handles, DRAGINFOs and
//! DRAGITEMs, RMF and type checks, then a drag from this process onto a
//! container in another one (`DESKTEST DROPTARGET`): DM_DRAGOVER answered
//! through the container's CN_DRAGOVER, DM_DRAGOVERNOTIFY to the source,
//! the drop (CN_DROP), one item read from its directory and name, one
//! rendered by the source (DM_RENDER, DM_RENDERCOMPLETE), and
//! DM_ENDCONVERSATION for each; then a drag cancelled with Esc.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::check;
use crate::files;
use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMDRAG.47"]
    fn DrgQueryNativeRMF(pditem: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "PMDRAG.58"]
    fn DrgVerifyNativeRMF(pditem: u32, rmf: *const u8) -> u32;
    #[link_name = "PMDRAG.60"]
    fn DrgVerifyTrueType(pditem: u32, ty: *const u8) -> u32;
    #[link_name = "PMDRAG.62"]
    fn DrgVerifyTypeSet(pditem: u32, ty: *const u8, cb: u32, out: *mut u8) -> u32;
    #[link_name = "PMDRAG.51"]
    fn DrgQueryTrueType(pditem: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "PMDRAG.44"]
    fn DrgQueryDragitem(pdinfo: u32, cb: u32, out: *mut u8, i: u32) -> u32;
    #[link_name = "PMDRAG.72"]
    fn DrgQueryDraginfoPtrFromDragitem(pditem: u32) -> u32;
    #[link_name = "PMDRAG.73"]
    fn DrgQueryDragStatus() -> u32;
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
}

const READY: &[u8] = b"\\SEM32\\DESKTEST\\DROPREADY\0";
const RENDERED: &[u8] = b"C:\\DTRENDER.TMP";
const SOURCE_FILE: &[u8] = b"C:\\DTDRAG.TXT";

fn buf_str(b: &[u8]) -> &[u8] {
    until_nul(b)
}

/// String handles, a DRAGINFO and its items, the RMF and type checks.
fn basics() {
    let a = add_str(b"Plain Text,C Code");
    check("DrgAddStrHandle", a != 0);
    check("the same string, the same handle", add_str(b"Plain Text,C Code") == a);
    check("DrgQueryStrName", str_handle(a) == b"Plain Text,C Code");
    let h = add_str(b"#123");
    check("a '#' string is not an integer atom", h != 0 && str_handle(h) == b"#123" && unsafe { DrgQueryStrNameLen(h) } == 4);
    let pd = unsafe { DrgAllocDraginfo(2) };
    check("DrgAllocDraginfo", pd != 0 && rd(pd, 4) & 0xffff == 36 && rd(pd, 4) >> 16 == DO_DEFAULT && unsafe { DrgQueryDragitemCount(pd) } == 2);
    let mut it = [0u8; 36];
    it[8..12].copy_from_slice(&a.to_le_bytes());
    let rmf = add_str(b"(DRM_OS2FILE,DRM_PRINT)x(DRF_TEXT,DRF_UNKNOWN)");
    it[12..16].copy_from_slice(&rmf.to_le_bytes());
    it[4..8].copy_from_slice(&77u32.to_le_bytes());
    check("DrgSetDragitem", unsafe { DrgSetDragitem(pd, it.as_ptr(), 36, 1) } != 0 && unsafe { DrgSetDragitem(pd, it.as_ptr(), 36, 2) } == 0);
    let mut back = [0u8; 36];
    check("DrgQueryDragitem", unsafe { DrgQueryDragitem(pd, 36, back.as_mut_ptr(), 1) } != 0 && back == it);
    let p = unsafe { DrgQueryDragitemPtr(pd, 1) };
    check("DrgQueryDragitemPtr", p != 0 && rd(p, 4) == 77 && unsafe { DrgQueryDragitemPtr(pd, 2) } == 0);
    check("DrgQueryDraginfoPtrFromDragitem", unsafe { DrgQueryDraginfoPtrFromDragitem(p) } == pd);
    check("DrgVerifyRMF (cross product)", unsafe { DrgVerifyRMF(p, b"DRM_PRINT\0".as_ptr(), b"DRF_UNKNOWN\0".as_ptr()) } != 0 && unsafe { DrgVerifyRMF(p, b"DRM_OBJECT\0".as_ptr(), core::ptr::null()) } == 0);
    let mut nb = [0u8; 64];
    check("DrgQueryNativeRMF", unsafe { DrgQueryNativeRMF(p, 64, nb.as_mut_ptr()) } != 0 && buf_str(&nb) == b"<DRM_OS2FILE,DRF_TEXT>");
    check("DrgVerifyNativeRMF", unsafe { DrgVerifyNativeRMF(p, b"<DRM_OS2FILE,DRF_TEXT>\0".as_ptr()) } != 0 && unsafe { DrgVerifyNativeRMF(p, b"<DRM_PRINT,DRF_TEXT>\0".as_ptr()) } == 0);
    check("DrgVerifyTrueType / DrgVerifyType", unsafe { DrgVerifyTrueType(p, b"Plain Text\0".as_ptr()) } != 0 && unsafe { DrgVerifyTrueType(p, b"C Code\0".as_ptr()) } == 0 && unsafe { DrgVerifyType(p, b"C Code\0".as_ptr()) } != 0);
    let mut m = [0u8; 32];
    check("DrgVerifyTypeSet", unsafe { DrgVerifyTypeSet(p, b"Bitmap,C Code\0".as_ptr(), 32, m.as_mut_ptr()) } != 0 && buf_str(&m) == b"C Code");
    let mut tt = [0u8; 32];
    check("DrgQueryTrueType", unsafe { DrgQueryTrueType(p, 32, tt.as_mut_ptr()) } != 0 && buf_str(&tt) == b"Plain Text");
    check("no drag in progress", unsafe { DrgQueryDragStatus() } == 0);
    check("DrgDeleteDraginfoStrHandles", unsafe { DrgDeleteDraginfoStrHandles(pd) } != 0);
    check("DrgFreeDraginfo", unsafe { DrgFreeDraginfo(pd) } != 0);
    unsafe {
        DrgDeleteStrHandle(a);
        DrgDeleteStrHandle(h);
    }
    let x = unsafe { DrgAllocDragtransfer(2) };
    check("DrgAllocDragtransfer / DrgFreeDragtransfer", x != 0 && unsafe { DrgFreeDragtransfer(x + 28) } != 0);
}

// ---- The source (this process) ----

static NOTIFY: AtomicU32 = AtomicU32::new(0xffff_ffff);
static ENDED: AtomicU32 = AtomicU32::new(0);
static ENDED_OK: AtomicU32 = AtomicU32::new(0);
static RENDERS: AtomicU32 = AtomicU32::new(0);

extern "C" fn source_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        DM_DRAGOVERNOTIFY => {
            NOTIFY.store(b, Ordering::Relaxed);
            0
        }
        DM_RENDER => {
            // DRAGTRANSFER: cb, hwndClient, pditem, hstrSelectedRMF,
            // hstrRenderToName, ulTargetInfo, usOperation, fsReply.
            let to = str_handle(rd(a, 16));
            let ok = files::write(&to, b"rendered by the source");
            RENDERS.fetch_add(1, Ordering::Relaxed);
            unsafe { DrgPostTransferMsg(rd(a, 4), DM_RENDERCOMPLETE, a, if ok { DMFL_RENDEROK } else { DMFL_RENDERFAIL }, 0, 0) };
            1
        }
        DM_ENDCONVERSATION => {
            ENDED.fetch_add(1, Ordering::Relaxed);
            if b & DMFL_TARGETSUCCESSFUL != 0 {
                ENDED_OK.fetch_add(1 << a, Ordering::Relaxed);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}

fn item(hwnd: u32, id: u32, ty: &[u8], rmf: &[u8], dir: &[u8], name: &[u8]) -> [u8; 36] {
    let mut d = [0u8; 36];
    let put = |d: &mut [u8; 36], off: usize, v: u32| d[off..off + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut d, 0, hwnd);
    put(&mut d, 4, id);
    put(&mut d, 8, add_str(ty));
    put(&mut d, 12, add_str(rmf));
    put(&mut d, 16, add_str(dir));
    put(&mut d, 20, add_str(name));
    put(&mut d, 24, add_str(name));
    d[34..36].copy_from_slice(&(DO_COPYABLE | DO_MOVEABLE).to_le_bytes());
    d
}

/// Run messages for up to `ms` or until `done`.
fn pump(hab: u32, ms: u32, done: impl Fn() -> bool) {
    let mut q = QMsg::default();
    for _ in 0..ms / 20 {
        while unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, 1) } != 0 {
            unsafe { WinDispatchMsg(hab, &q) };
        }
        if done() {
            return;
        }
        unsafe { os2::DosSleep(20) };
    }
}

pub fn checks(hab: u32) {
    basics();
    files::write(SOURCE_FILE, b"dragged");
    crate::fileops::delete_file(RENDERED);
    // The target, in its own process.
    let mut sem = 0;
    unsafe { os2::DosCreateEventSem(READY.as_ptr(), &mut sem, 0, 0) };
    let prog = crate::objects::sys_path(b"RS2\\DESKTEST.EXE");
    let mut args = prog.clone();
    args.push(0);
    args.extend_from_slice(b"DROPTARGET\0\0");
    let pz = z(&prog);
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, os2::EXEC_ASYNC, args.as_ptr(), core::ptr::null(), &mut res, pz.as_ptr()) };
    let ready = rc == 0 && unsafe { os2::DosWaitEventSem(sem, 15000) } == 0;
    check("DESKTEST DROPTARGET started (a container in another process)", ready);
    if !ready {
        return;
    }
    unsafe { WinRegisterClass(hab, b"DeskTestSource\0".as_ptr(), source_proc, 0, 0) };
    let mut fcf = FCF_TITLEBAR;
    let mut src = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, WS_VISIBLE, &mut fcf, b"DeskTestSource\0".as_ptr(), b"Drag source\0".as_ptr(), 0, 0, 1, &mut src) };
    unsafe { WinSetWindowPos(frame, HWND_TOP, 20, 20, 200, 100, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER) };
    // Over the target's container (its frame is at 500,300 300x200).
    unsafe { WinSetPointerPos(HWND_DESKTOP, 650, 380) };
    let target_cnr = unsafe { WinWindowFromPoint(HWND_DESKTOP, &PointL { x: 650, y: 380 }, 1) };
    check("the target's container is under the pointer", target_cnr != 0 && window_pid(target_cnr) != window_pid(src));
    let pd = unsafe { DrgAllocDraginfo(2) };
    let (dir, name) = (crate::fileops::parent(SOURCE_FILE), crate::fileops::name_of(SOURCE_FILE).to_vec());
    let i0 = item(src, 0, b"Plain Text", b"<DRM_OS2FILE,DRF_TEXT>", &dir, &name);
    // The second item has no source name: the target asks for it.
    let mut i1 = item(src, 1, b"Plain Text", b"<DRM_OS2FILE,DRF_TEXT>", &dir, b"");
    i1[20..24].copy_from_slice(&0u32.to_le_bytes());
    unsafe {
        DrgSetDragitem(pd, i0.as_ptr(), 36, 0);
        DrgSetDragitem(pd, i1.as_ptr(), 36, 1);
    }
    let mut img = [0u8; 24];
    img[0..2].copy_from_slice(&24u16.to_le_bytes());
    img[4..8].copy_from_slice(&unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_FILE, 0) }.to_le_bytes());
    img[16..20].copy_from_slice(&DRG_ICON.to_le_bytes());
    // The mouse moves and button 2 goes up (as DrgDrag reads them).
    post(src, WM_MOUSEMOVE, mp(130, 60), 0);
    post(src, WM_BUTTON2UP, mp(130, 60), 0);
    let got = unsafe { DrgDrag(src, pd, img.as_ptr(), 1, 2, 0) };
    check("DrgDrag returns the window that took the drop", got == target_cnr);
    let n = NOTIFY.load(Ordering::Relaxed);
    check("DM_DRAGOVERNOTIFY: the target's DOR_DROP, DO_COPY", n & 0xffff == DOR_DROP && n >> 16 == DO_COPY);
    pump(hab, 10000, || ENDED.load(Ordering::Relaxed) >= 2);
    check("DM_RENDER for the item without a name", RENDERS.load(Ordering::Relaxed) == 1 && files::read(RENDERED).as_deref() == Some(&b"rendered by the source"[..]));
    check("DM_ENDCONVERSATION for both items, successful", ENDED.load(Ordering::Relaxed) == 2 && ENDED_OK.load(Ordering::Relaxed) == 3);
    check("the source frees its DRAGINFO", unsafe { DrgFreeDraginfo(pd) } != 0);
    // A drag cancelled with Esc: nothing dropped.
    let pd = unsafe { DrgAllocDraginfo(1) };
    unsafe { DrgSetDragitem(pd, i0.as_ptr(), 36, 0) };
    post(src, WM_MOUSEMOVE, 0, 0);
    // WM_CHAR: KC_VIRTUALKEY, VK_ESC.
    post(src, WM_CHAR, KC_VIRTUALKEY, 0x0f << 16);
    let got = unsafe { DrgDrag(src, pd, img.as_ptr(), 1, 2, 0) };
    check("Esc cancels a drag (DrgDrag returns 0)", got == 0);
    unsafe {
        DrgDeleteDraginfoStrHandles(pd);
        DrgFreeDraginfo(pd);
    }
    // The target ends when it gets WM_CLOSE.
    post(unsafe { WinQueryWindow(target_cnr, 5) }, WM_CLOSE, 0, 0);
    unsafe {
        WinDestroyWindow(frame);
        os2::DosCloseEventSem(sem);
    }
    crate::fileops::delete_file(SOURCE_FILE);
    crate::fileops::delete_file(RENDERED);
}

// ---- The target (DESKTEST DROPTARGET) ----

static CNR: AtomicU32 = AtomicU32::new(0);
static PENDING: AtomicU32 = AtomicU32::new(0);

const WM_RENDER_ITEMS: u32 = WM_USER + 5;

extern "C" fn target_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_CONTROL if a >> 16 == CN_DRAGOVER => {
            // CNRDRAGINFO: pDragInfo, pRecord.
            let pd = rd(b, 0);
            if unsafe { DrgAccessDraginfo(pd) } == 0 {
                return DOR_NEVERDROP;
            }
            let n = unsafe { DrgQueryDragitemCount(pd) };
            let ok = (0..n).all(|i| {
                let it = unsafe { DrgQueryDragitemPtr(pd, i) };
                unsafe { DrgVerifyRMF(it, b"DRM_OS2FILE\0".as_ptr(), core::ptr::null()) != 0 && DrgVerifyType(it, b"Plain Text\0".as_ptr()) != 0 }
            });
            unsafe { DrgFreeDraginfo(pd) };
            if ok { mp(DOR_DROP, DO_COPY) } else { DOR_NODROPOP }
        }
        WM_CONTROL if a >> 16 == CN_DROP => {
            let pd = rd(b, 0);
            unsafe { DrgAccessDraginfo(pd) };
            // After the drop returns: the conversation (as the Workplace
            // Shell does it, posted).
            PENDING.store(pd, Ordering::Relaxed);
            post(h, WM_RENDER_ITEMS, pd, 0);
            0
        }
        WM_RENDER_ITEMS => {
            let pd = a;
            for i in 0..unsafe { DrgQueryDragitemCount(pd) } {
                let it = unsafe { DrgQueryDragitemPtr(pd, i) };
                let (dir, name) = (str_handle(rd(it, 16)), str_handle(rd(it, 20)));
                if !name.is_empty() {
                    let ok = files::read(&crate::fileops::join(&dir, &name)).as_deref() == Some(&b"dragged"[..]) && rd(pd, 4) >> 16 == DO_COPY;
                    post(rd(it, 0), DM_ENDCONVERSATION, rd(it, 4), if ok { DMFL_TARGETSUCCESSFUL } else { DMFL_TARGETFAIL });
                } else {
                    let x = unsafe { DrgAllocDragtransfer(1) };
                    let w = |off: u32, v: u32| wr(x, off as usize, v);
                    w(0, 28);
                    w(4, h);
                    w(8, it);
                    w(12, add_str(b"<DRM_OS2FILE,DRF_TEXT>"));
                    w(16, add_str(RENDERED));
                    w(24, DO_COPY);
                    if unsafe { DrgSendTransferMsg(rd(it, 0), DM_RENDER, x, 0) } == 0 {
                        post(rd(it, 0), DM_ENDCONVERSATION, rd(it, 4), DMFL_TARGETFAIL);
                    }
                }
            }
            0
        }
        DM_RENDERCOMPLETE => {
            // DRAGTRANSFER, flags.
            let it = rd(a, 8);
            let ok = b & DMFL_RENDEROK != 0 && crate::fileops::exists(RENDERED);
            post(rd(it, 0), DM_ENDCONVERSATION, rd(it, 4), if ok { DMFL_TARGETSUCCESSFUL } else { DMFL_TARGETFAIL });
            unsafe { DrgFreeDragtransfer(a) };
            let pd = PENDING.swap(0, Ordering::Relaxed);
            if pd != 0 {
                unsafe {
                    DrgDeleteDraginfoStrHandles(pd);
                    DrgFreeDraginfo(pd);
                }
            }
            0
        }
        WM_SIZE => {
            let mut r = RectL::default();
            unsafe {
                WinQueryWindowRect(h, &mut r);
                WinSetWindowPos(CNR.load(Ordering::Relaxed), 0, 0, 0, r.r, r.t, SWP_SIZE | SWP_MOVE);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}

pub fn target(hab: u32) -> u32 {
    unsafe { WinRegisterClass(hab, b"DeskTestTarget\0".as_ptr(), target_proc, 0, 0) };
    let mut fcf = FCF_TITLEBAR | FCF_SIZEBORDER;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"DeskTestTarget\0".as_ptr(), b"Drop target\0".as_ptr(), 0, 0, 1, &mut client) };
    let cnr = unsafe { WinCreateWindow(client, WC_CONTAINER, b"\0".as_ptr(), WS_VISIBLE, 0, 0, 10, 10, client, HWND_TOP, 100, core::ptr::null(), core::ptr::null()) };
    CNR.store(cnr, Ordering::Relaxed);
    unsafe { WinSetWindowPos(frame, HWND_TOP, 500, 300, 300, 200, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER) };
    let mut sem = 0;
    if unsafe { DosOpenEventSem(READY.as_ptr(), &mut sem) } == 0 {
        unsafe { os2::DosPostEventSem(sem) };
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    0
}
