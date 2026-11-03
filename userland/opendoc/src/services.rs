//! The session and its services: ODBaseSession/ODSession (with the type
//! tokenizer), ODArbitrator and ODFocusSet, ODWindowState and ODWindow
//! (PM frame windows with a root frame and facet), ODMenuBar, the
//! clipboard and drag and drop, ODHelp, the extensions, the dispatcher,
//! and the classes only kept for their tokens (M_ODFacetWindow,
//! OSAScriptingComponent).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::layout;
use crate::obj::{acquire, release};
use crate::rt::*;
use crate::storage::{self, new_of};
use crate::*;

unsafe extern "C" {
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(hwnd: u32) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(hwnd: u32, rect: usize, children: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, flags: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, class: *const u8, proc_: usize, style: u32, cb: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.883"]
    fn WinShowWindow(hwnd: u32, show: u32) -> u32;
    #[link_name = "PMWIN.851"]
    fn WinSetActiveWindow(desktop: u32, hwnd: u32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(hwnd: u32, rect: *mut [i32; 4]) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(hwnd: u32, hps: u32, rect: *mut [i32; 4]) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(hwnd: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(hwnd: u32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(hwnd: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.877"]
    fn WinSetWindowText(hwnd: u32, text: *const u8) -> u32;
}

const HWND_DESKTOP: u32 = 1;
const FID_CLIENT: u32 = 0x8008;
const WM_PAINT: u32 = 0x23;
const WM_SIZE: u32 = 0x07;

// ---- The session ----

static mut SESSION: usize = 0;

/// The process's session (the first one initialised).
pub fn session() -> usize {
    unsafe { SESSION }
}

#[derive(Default)]
pub struct SessionState {
    pub arbitrator: usize,
    pub clipboard: usize,
    pub dnd: usize,
    pub dispatcher: usize,
    pub info: usize,
    pub nsm: usize,
    pub window_state: usize,
    pub storage: usize,
    pub translation: usize,
    pub undo: usize,
    pub link_manager: usize,
    pub message: usize,
    pub resolver: usize,
    pub help: usize,
    pub types: Vec<Vec<u8>>,
    pub update_id: u32,
    pub hab: u32,
}

pub fn sess(this: usize) -> &'static mut SessionState {
    state(cd!(ODBaseSessionCClassData), this)
}

/// ODTypeToken for a type name (1-based index in the session's table).
pub fn tokenize(this: usize, ty: &[u8]) -> usize {
    if this == 0 {
        return 0;
    }
    let s = sess(this);
    match s.types.iter().position(|t| t == ty) {
        Some(i) => i + 1,
        None => {
            s.types.push(ty.to_vec());
            s.types.len()
        }
    }
}

pub fn type_of(this: usize, token: usize) -> Option<Vec<u8>> {
    if this == 0 || token == 0 {
        return None;
    }
    sess(this).types.get(token - 1).cloned()
}

macro_rules! getset {
    ($($get:ident, $set:ident, $f:ident;)*) => {
        methods! {$(
            fn $get(this: usize, ev: usize) -> usize { sess(this).$f }
            fn $set(this: usize, ev: usize, v: usize) -> usize {
                sess(this).$f = v;
                0
            }
        )*}
    };
}

getset! {
    GetArbitrator, SetArbitrator, arbitrator;
    GetClipboard, SetClipboard, clipboard;
    GetDragAndDrop, SetDragAndDrop, dnd;
    GetDispatcher, SetDispatcher, dispatcher;
    GetInfo, SetInfo, info;
    GetNameSpaceManager, SetNameSpaceManager, nsm;
    GetWindowState, SetWindowState, window_state;
    GetStorageSystem, SetStorageSystem, storage;
    GetTranslation, SetTranslation, translation;
    GetUndo, SetUndo, undo;
    GetLinkManager, SetLinkManager, link_manager;
    GetMessageInterface, SetMessageInterface, message;
    GetNameResolver, SetNameResolver, resolver;
    GetHelp, SetHelp, help;
}

/// A scratch draft (no document) for the clipboard and drag and drop.
fn scratch_draft() -> usize {
    let d = new_of(cd!(ODDraftClassData), "ODDraft");
    if d != 0 {
        storage::draft(d).next_id = 1;
    }
    d
}

fn make_services(this: usize) {
    let s = sess(this);
    if s.arbitrator == 0 {
        s.arbitrator = new_of(cd!(ODArbitratorClassData), "ODArbitrator");
    }
    if s.window_state == 0 {
        s.window_state = new_of(cd!(ODWindowStateClassData), "ODWindowState");
    }
    if s.storage == 0 {
        let st = new_of(cd!(ODStorageSystemClassData), "ODStorageSystem");
        if st != 0 {
            storage::InitStorageSystem(st, 0, this);
        }
        s.storage = st;
    }
    if s.clipboard == 0 {
        let c = new_of(cd!(ODClipboardClassData), "ODClipboard");
        if c != 0 {
            transfer(c).draft = scratch_draft();
        }
        s.clipboard = c;
    }
    if s.dnd == 0 {
        let c = new_of(cd!(ODDragAndDropClassData), "ODDragAndDrop");
        if c != 0 {
            dnd(c).draft = scratch_draft();
        }
        s.dnd = c;
    }
    if s.dispatcher == 0 {
        let d = new_of(cd!(ODDispatcherClassData), "ODDispatcher");
        if d != 0 {
            crate::events::InitDispatcher(d, 0, this);
        }
        s.dispatcher = d;
    }
    if s.nsm == 0 {
        s.nsm = new_of(cd!(ODNameSpaceManagerClassData), "ODNameSpaceManager");
    }
    if s.help == 0 {
        s.help = new_of(cd!(ODHelpClassData), "ODHelp");
    }
}

methods! {
    fn AcquireShellSemtInterface(this: usize, ev: usize) -> usize { 0 }
    fn GetUserName(this: usize, ev: usize, out: usize) -> usize {
        write::<usize>(out, dup(b"rs2"));
        0
    }
    fn UniqueUpdateID(this: usize, ev: usize) -> usize {
        sess(this).update_id += 1;
        sess(this).update_id as usize
    }
    fn InitBaseSession(this: usize, ev: usize) -> usize {
        unsafe {
            if SESSION == 0 {
                SESSION = this;
            }
        }
        make_services(this);
        0
    }
    fn Tokenize(this: usize, ev: usize, ty: usize) -> usize { tokenize(this, cstr(ty)) }
    fn RemoveEntry(this: usize, ev: usize, ty: usize) -> usize { 0 }
    fn GetType(this: usize, ev: usize, token: usize, out: usize) -> usize {
        match type_of(this, token) {
            Some(t) => {
                write::<usize>(out, dup(&t));
                1
            }
            None => 0,
        }
    }
    fn SessSomUninit(this: usize) -> usize {
        unsafe {
            if SESSION == this {
                SESSION = 0;
            }
        }
        drop_state::<SessionState>(cd!(ODBaseSessionCClassData), this);
        call_parent(cd!(ODBaseSessionCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    // ODSession (OS/2)
    fn InitSession(this: usize, ev: usize) -> usize { InitBaseSession(this, ev) }
    fn GetHAB(this: usize, ev: usize) -> usize { sess(this).hab as usize }
    fn SetHAB(this: usize, ev: usize, hab: usize) -> usize {
        sess(this).hab = hab as u32;
        0
    }
    fn CreatePlatformWindow(this: usize, ev: usize, flags: usize) -> usize { create_window(flags as u32) as usize }
}

pub static BASE_SESSION_M: &[(&str, M)] = &[
    ("GetArbitrator", GetArbitrator::t),
    ("SetArbitrator", SetArbitrator::t),
    ("GetClipboard", GetClipboard::t),
    ("SetClipboard", SetClipboard::t),
    ("GetDragAndDrop", GetDragAndDrop::t),
    ("SetDragAndDrop", SetDragAndDrop::t),
    ("GetDispatcher", GetDispatcher::t),
    ("SetDispatcher", SetDispatcher::t),
    ("GetInfo", GetInfo::t),
    ("SetInfo", SetInfo::t),
    ("GetNameSpaceManager", GetNameSpaceManager::t),
    ("SetNameSpaceManager", SetNameSpaceManager::t),
    ("SetWindowState", SetWindowState::t),
    ("GetWindowState", GetWindowState::t),
    ("GetStorageSystem", GetStorageSystem::t),
    ("SetStorageSystem", SetStorageSystem::t),
    ("GetTranslation", GetTranslation::t),
    ("SetTranslation", SetTranslation::t),
    ("GetUndo", GetUndo::t),
    ("SetUndo", SetUndo::t),
    ("GetLinkManager", GetLinkManager::t),
    ("SetLinkManager", SetLinkManager::t),
    ("GetMessageInterface", GetMessageInterface::t),
    ("SetMessageInterface", SetMessageInterface::t),
    ("GetNameResolver", GetNameResolver::t),
    ("SetNameResolver", SetNameResolver::t),
    ("AcquireShellSemtInterface", AcquireShellSemtInterface::t),
    ("GetUserName", GetUserName::t),
    ("UniqueUpdateID", UniqueUpdateID::t),
    ("InitBaseSession", InitBaseSession::t),
    ("Tokenize", Tokenize::t),
    ("RemoveEntry", RemoveEntry::t),
    ("GetType", GetType::t),
];
pub static BASE_SESSION_O: &[(&str, M)] = &[("SOMObject::somUninit", SessSomUninit::t)];
pub static SESSION_M: &[(&str, M)] = &[
    ("InitSession", InitSession::t),
    ("GetHAB", GetHAB::t),
    ("SetHAB", SetHAB::t),
    ("CreatePlatformWindow", CreatePlatformWindow::t),
    ("GetHelp", GetHelp::t),
    ("SetHelp", SetHelp::t),
];

pub mod session_ix {
    pub const GET_WINDOW_STATE: usize = 13;
    pub const GET_STORAGE_SYSTEM: usize = 14;
    pub const TOKENIZE: usize = 30;
    pub const INIT_SESSION: usize = 0;
}

// ---- PM windows: an OpenDoc document window draws its root facet ----

static mut CLASS_REGISTERED: bool = false;
const CLIENT_CLASS: &[u8] = b"rs2OpenDocWindow\0";

fn create_window(flags: u32) -> u32 {
    unsafe {
        let hab = WinQueryAnchorBlock(HWND_DESKTOP);
        if !CLASS_REGISTERED {
            CLASS_REGISTERED = WinRegisterClass(hab, CLIENT_CLASS.as_ptr(), client_proc_t as usize, 4, 0) != 0;
        }
        // No menu, icon or accelerator resources of the caller's: those
        // frame controls are left out; the shell places the window.
        let mut fl = (if flags == 0 { 0x783f } else { flags } & !(0x4 | 0x400 | 0x4000 | 0x8000));
        let mut client = 0u32;
        let hwnd = WinCreateStdWindow(HWND_DESKTOP, 0x2000_0000, &mut fl, CLIENT_CLASS.as_ptr(), b"\0".as_ptr(), 0x8000_0000, 0, 1, &mut client);
        if hwnd != 0 {
            WinSetWindowPos(hwnd, 0, 120, 120, 480, 320, 0x3); // SWP_SIZE | SWP_MOVE
        }
        hwnd
    }
}

/// The ODWindow a PM frame window belongs to.
fn window_of(frame_hwnd: u32) -> usize {
    let s = session();
    if s == 0 {
        return 0;
    }
    let ws = sess(s).window_state;
    if ws == 0 {
        return 0;
    }
    wstate(ws).windows.iter().copied().find(|&w| win(w).hwnd == frame_hwnd).unwrap_or(0)
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize {
    unsafe {
        let frame_hwnd = crate::pmparent(hwnd);
        if msg != WM_PAINT && msg != WM_SIZE && crate::events::client_message(hwnd, msg, mp1, mp2) {
            return if msg == 0x7a || (0x70..=0x79).contains(&msg) { 1 } else { 0 };
        }
        match msg {
            WM_PAINT => {
                let mut r = [0i32; 4];
                let hps = WinBeginPaint(hwnd, 0, &mut r);
                let w = window_of(frame_hwnd);
                if w != 0 {
                    let f = win(w).root_facet;
                    if f != 0 {
                        let c = layout::facet(f).canvas;
                        let pc = if c != 0 { layout::canvas(c).platform } else { 0 };
                        if pc != 0 {
                            let p = layout::pcanvas(pc);
                            let old = p.hps;
                            p.hps = hps;
                            let mut ev = somsup::Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };
                            crate::events::draw_tree(f, &raw mut ev as usize, hps);
                            crate::events::draw_active_border(w, hps);
                            layout::pcanvas(pc).hps = old;
                        }
                    }
                }
                WinEndPaint(hps);
                0
            }
            WM_SIZE => {
                let w = window_of(frame_hwnd);
                if w != 0 {
                    window_resized(w, hwnd);
                }
                WinDefWindowProc(hwnd, msg, mp1, mp2)
            }
            _ => WinDefWindowProc(hwnd, msg, mp1, mp2),
        }
    }
}

crate::tramp!(client_proc_t => client_proc, 4);


fn window_resized(w: usize, client: u32) {
    let mut r = [0i32; 4];
    unsafe { WinQueryWindowRect(client, &mut r) };
    let rect = Rect { left: 0, bottom: 0, right: r[2] << 16, top: r[3] << 16 };
    let f = win(w).root_frame;
    if f != 0 {
        layout::frame(f).frame_shape = rect;
    }
    let fc = win(w).root_facet;
    if fc != 0 {
        layout::facet(fc).clip = Some(rect);
    }
}

// ---- ODWindowState and ODWindow ----

#[derive(Default)]
pub struct WindowStateState {
    pub windows: Vec<usize>,
    pub next_id: u32,
    pub base_menu: usize,
    pub base_popup: usize,
}

pub fn wstate(this: usize) -> &'static mut WindowStateState {
    state(cd!(ODWindowStateCClassData), this)
}

#[derive(Default)]
pub struct WindowState {
    pub hwnd: u32,
    pub client: u32,
    pub id: u32,
    pub root_frame: usize,
    pub root_facet: usize,
    pub source_frame: usize,
    pub root: bool,
    pub resizable: bool,
    pub floating: bool,
    pub save: bool,
    pub dispose: bool,
    pub shown: bool,
    pub show_links: bool,
}

pub fn win(this: usize) -> &'static mut WindowState {
    state(cd!(ODWindowCClassData), this)
}

/// Make the ODWindow for a PM window and a root frame: the root facet on
/// a canvas around the client window.
fn register(ws: usize, ev: usize, hwnd: usize, frame: usize, root: usize, resizable: usize, floating: usize, save: usize, dispose: usize, source: usize) -> usize {
    let w = new_of(cd!(ODWindowClassData), "ODWindow");
    if w == 0 {
        return 0;
    }
    let st = wstate(ws);
    st.next_id += 1;
    let s = win(w);
    s.hwnd = hwnd as u32;
    s.client = if hwnd != 0 { crate::pmchild(hwnd as u32, FID_CLIENT) } else { 0 };
    s.id = st.next_id;
    s.root_frame = acquire(frame, ev);
    s.source_frame = source;
    s.root = root & 0xff != 0;
    s.resizable = resizable & 0xff != 0;
    s.floating = floating & 0xff != 0;
    s.save = save & 0xff != 0;
    s.dispose = dispose & 0xff != 0;
    st.windows.push(acquire(w, ev));
    if frame != 0 {
        layout::frame(frame).window = w;
    }
    w
}

fn open_window(w: usize, ev: usize) {
    let s = win(w);
    if s.root_facet != 0 || s.root_frame == 0 {
        return;
    }
    let pc = new_of(cd!(ODPlatformCanvasClassData), "ODPlatformCanvas");
    if pc != 0 {
        layout::pcanvas(pc).hwnd = s.client;
    }
    let cv = new_of(cd!(ODCanvasClassData), "ODCanvas");
    if cv != 0 {
        let c = layout::canvas(cv);
        c.platform = pc;
        c.dynamic = true;
    }
    let fc = layout::new_facet(ev, s.root_frame, 0, 0, cv, 0);
    if fc == 0 {
        return;
    }
    if cv != 0 {
        layout::canvas(cv).facet = fc;
    }
    {
        let f = layout::facet(fc);
        f.window = w;
        f.hwnd = s.client;
    }
    win(w).root_facet = fc;
    if s.client != 0 {
        window_resized(w, s.client);
    }
    layout::FFacetAdded(win(w).root_frame, ev, fc);
}

methods! {
    fn RegisterWindow(this: usize, ev: usize, hwnd: usize, frame_type: usize, root: usize, resizable: usize, floating: usize,
                      save: usize, dispose: usize, root_part: usize, view: usize, pres: usize, source: usize) -> usize {
        // The root frame, in the root part's draft.
        let su = if root_part != 0 { crate::obj::su_of(root_part) } else { 0 };
        let dr = if su != 0 { storage::sus(su).draft } else { 0 };
        let frame = if dr != 0 {
            let shp = layout::new_shape(Rect::default());
            let f = call(cd!(ODDraftClassData), storage::draft_ix::CREATE_FRAME, &[dr, ev, frame_type, 0, shp, 0, root_part, view, pres, 0, 0]);
            release(shp, ev);
            f
        } else {
            0
        };
        let w = register(this, ev, hwnd, frame, root, resizable, floating, save, dispose, source);
        release(frame, ev);
        w
    }
    fn RegisterWindowForFrame(this: usize, ev: usize, hwnd: usize, frame: usize, root: usize, resizable: usize, floating: usize,
                              save: usize, dispose: usize, source: usize) -> usize {
        register(this, ev, hwnd, frame, root, resizable, floating, save, dispose, source)
    }
    fn WsAcquireWindow(this: usize, ev: usize, id: usize) -> usize {
        acquire(wstate(this).windows.iter().copied().find(|&w| win(w).id as usize == id).unwrap_or(0), ev)
    }
    fn WsInternalize(this: usize, ev: usize, d: usize) -> usize { 0 }
    fn WsExternalize(this: usize, ev: usize, d: usize) -> usize { 0 }
    fn SetDefaultWindowTitles(this: usize, ev: usize, d: usize) -> usize { 0 }
    fn OpenWindows(this: usize, ev: usize, d: usize) -> usize {
        for w in wstate(this).windows.clone() {
            WOpen(w, ev);
            WShow(w, ev);
        }
        0
    }
    fn CloseWindows(this: usize, ev: usize, d: usize) -> usize {
        for w in wstate(this).windows.clone() {
            WClose(w, ev);
        }
        0
    }
    fn GetWindowCount(this: usize, ev: usize) -> usize { wstate(this).windows.len() }
    fn GetRootWindowCount(this: usize, ev: usize, d: usize) -> usize { wstate(this).windows.iter().filter(|&&w| win(w).root).count() }
    fn GetTotalRootWindowCount(this: usize, ev: usize) -> usize { wstate(this).windows.iter().filter(|&&w| win(w).root).count() }
    fn IsODWindow(this: usize, ev: usize, hwnd: usize) -> usize { wstate(this).windows.iter().any(|&w| win(w).hwnd as usize == hwnd) as usize }
    fn AcquireODWindow(this: usize, ev: usize, hwnd: usize) -> usize {
        acquire(wstate(this).windows.iter().copied().find(|&w| win(w).hwnd as usize == hwnd).unwrap_or(0), ev)
    }
    fn CreateWindowIterator(this: usize, ev: usize) -> usize {
        let it = new_of(cd!(ODWindowIteratorClassData), "ODWindowIterator");
        if it != 0 {
            witer(it).items = wstate(this).windows.clone();
        }
        it
    }
    fn AcquireActiveWindow(this: usize, ev: usize) -> usize { acquire(wstate(this).windows.last().copied().unwrap_or(0), ev) }
    fn SetBaseMenuBar(this: usize, ev: usize, m: usize) -> usize {
        wstate(this).base_menu = acquire(m, ev);
        0
    }
    fn CopyBaseMenuBar(this: usize, ev: usize) -> usize {
        let m = wstate(this).base_menu;
        if m == 0 { 0 } else { crate::events::MCopy(m, ev) }
    }
    fn SetBasePopup(this: usize, ev: usize, m: usize) -> usize {
        if m != 0 {
            crate::events::menu(m).popup = true;
        }
        wstate(this).base_popup = acquire(m, ev);
        0
    }
    fn CopyBasePopup(this: usize, ev: usize) -> usize {
        let m = wstate(this).base_popup;
        if m == 0 { 0 } else { crate::events::MCopy(m, ev) }
    }
    fn CreateMenuBar(this: usize, ev: usize, platform: usize) -> usize { new_of(cd!(ODMenuBarClassData), "ODMenuBar") }
    fn WsSomUninit(this: usize) -> usize {
        drop_state::<WindowStateState>(cd!(ODWindowStateCClassData), this);
        call_parent(cd!(ODWindowStateCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    // ODWindow
    fn GetPlatformWindow(this: usize, ev: usize) -> usize { win(this).hwnd as usize }
    fn AcquireRootFrame(this: usize, ev: usize) -> usize { acquire(win(this).root_frame, ev) }
    fn GetRootFacet(this: usize, ev: usize) -> usize { win(this).root_facet }
    fn WIsActive(this: usize, ev: usize) -> usize { 1 }
    fn WOpen(this: usize, ev: usize) -> usize {
        open_window(this, ev);
        0
    }
    fn WClose(this: usize, ev: usize) -> usize {
        let s = win(this);
        if s.root_frame != 0 {
            layout::FClose(s.root_frame, ev);
        }
        if s.hwnd != 0 {
            unsafe { WinDestroyWindow(s.hwnd) };
            s.hwnd = 0;
        }
        0
    }
    fn CloseAndRemove(this: usize, ev: usize) -> usize { WClose(this, ev) }
    fn WShow(this: usize, ev: usize) -> usize {
        let s = win(this);
        s.shown = true;
        if s.hwnd != 0 {
            unsafe { WinSetWindowPos(s.hwnd, 3, 0, 0, 0, 0, 0x8 | 0x80 | 0x4) }; // HWND_TOP, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER
        }
        0
    }
    fn WHide(this: usize, ev: usize) -> usize {
        let s = win(this);
        s.shown = false;
        if s.hwnd != 0 {
            unsafe { WinShowWindow(s.hwnd, 0) };
        }
        0
    }
    fn IsShown(this: usize, ev: usize) -> usize { win(this).shown as usize }
    fn IsResizable(this: usize, ev: usize) -> usize { win(this).resizable as usize }
    fn IsRootWindow(this: usize, ev: usize) -> usize { win(this).root as usize }
    fn ShouldSave(this: usize, ev: usize) -> usize { win(this).save as usize }
    fn SetShouldSave(this: usize, ev: usize, b: usize) -> usize {
        win(this).save = b & 0xff != 0;
        0
    }
    fn ShouldShowLinks(this: usize, ev: usize) -> usize { win(this).show_links as usize }
    fn SetShouldShowLinks(this: usize, ev: usize, b: usize) -> usize {
        win(this).show_links = b & 0xff != 0;
        0
    }
    fn ShouldDispose(this: usize, ev: usize) -> usize { win(this).dispose as usize }
    fn WGetID(this: usize, ev: usize) -> usize { win(this).id as usize }
    fn AcquireSourceFrame(this: usize, ev: usize) -> usize { acquire(win(this).source_frame, ev) }
    fn SetSourceFrame(this: usize, ev: usize, f: usize) -> usize {
        win(this).source_frame = f;
        0
    }
    fn Select(this: usize, ev: usize) -> usize {
        let h = win(this).hwnd;
        if h != 0 {
            unsafe { WinSetActiveWindow(HWND_DESKTOP, h) };
        }
        0
    }
    fn WUpdate(this: usize, ev: usize) -> usize {
        let c = win(this).client;
        if c != 0 {
            unsafe { WinInvalidateRect(c, 0, 1) };
        }
        0
    }
    fn SetWindowTitle(this: usize, ev: usize, t: usize) -> usize {
        let h = win(this).hwnd;
        if h != 0 {
            let s = z(cstr(t));
            unsafe { WinSetWindowText(h, s.as_ptr()) };
        }
        0
    }
    fn WSomUninit(this: usize) -> usize {
        drop_state::<WindowState>(cd!(ODWindowCClassData), this);
        call_parent(cd!(ODWindowCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static WINDOW_STATE_M: &[(&str, M)] = &[
    ("RegisterWindow", RegisterWindow::t),
    ("RegisterWindowForFrame", RegisterWindowForFrame::t),
    ("AcquireWindow", WsAcquireWindow::t),
    ("Internalize", WsInternalize::t),
    ("Externalize", WsExternalize::t),
    ("SetDefaultWindowTitles", SetDefaultWindowTitles::t),
    ("OpenWindows", OpenWindows::t),
    ("CloseWindows", CloseWindows::t),
    ("GetWindowCount", GetWindowCount::t),
    ("GetRootWindowCount", GetRootWindowCount::t),
    ("GetTotalRootWindowCount", GetTotalRootWindowCount::t),
    ("IsODWindow", IsODWindow::t),
    ("AcquireODWindow", AcquireODWindow::t),
    ("CreateWindowIterator", CreateWindowIterator::t),
    ("AcquireActiveWindow", AcquireActiveWindow::t),
    ("SetBaseMenuBar", SetBaseMenuBar::t),
    ("CopyBaseMenuBar", CopyBaseMenuBar::t),
    ("ODWindowState_reserved17", ret0),
    ("CreateMenuBar", CreateMenuBar::t),
    ("ODWindowState_reserved19", ret0),
    ("ODWindowState_reserved20", ret0),
    ("ODWindowState_reserved21", ret0),
    ("ODWindowState_reserved22", ret0),
    ("ODWindowState_reserved23", ret0),
    ("ODWindowState_reserved24", ret0),
    ("ODWindowState_reserved25", ret0),
    ("ODWindowState_reserved26", ret0),
    ("SetBasePopup", SetBasePopup::t),
    ("ODWindowState_reserved28", ret0),
    ("CopyBasePopup", CopyBasePopup::t),
];
pub static WINDOW_STATE_O: &[(&str, M)] = &[("SOMObject::somUninit", WsSomUninit::t)];

pub static WINDOW_M: &[(&str, M)] = &[
    ("GetPlatformWindow", GetPlatformWindow::t),
    ("AcquireRootFrame", AcquireRootFrame::t),
    ("GetRootFacet", GetRootFacet::t),
    ("ODWindow_reserved3", ret0),
    ("ODWindow_reserved4", ret0),
    ("IsActive", WIsActive::t),
    ("Open", WOpen::t),
    ("Close", WClose::t),
    ("CloseAndRemove", CloseAndRemove::t),
    ("Show", WShow::t),
    ("Hide", WHide::t),
    ("IsShown", IsShown::t),
    ("IsResizable", IsResizable::t),
    ("IsRootWindow", IsRootWindow::t),
    ("ShouldSave", ShouldSave::t),
    ("SetShouldSave", SetShouldSave::t),
    ("ShouldShowLinks", ShouldShowLinks::t),
    ("SetShouldShowLinks", SetShouldShowLinks::t),
    ("ShouldDispose", ShouldDispose::t),
    ("GetID", WGetID::t),
    ("AcquireSourceFrame", AcquireSourceFrame::t),
    ("SetSourceFrame", SetSourceFrame::t),
    ("Select", Select::t),
    ("Update", WUpdate::t),
    ("SetWindowTitle", SetWindowTitle::t),
];
pub static WINDOW_O: &[(&str, M)] = &[("SOMObject::somUninit", WSomUninit::t)];

pub mod window_ix {
    pub const OPEN: usize = 6;
    pub const SHOW: usize = 9;
    pub const GET_ROOT_FACET: usize = 2;
    pub const REGISTER_WINDOW: usize = 0;
    pub const REGISTER_WINDOW_FOR_FRAME: usize = 1;
}

// Window iterator.

#[derive(Default)]
pub struct WIterState {
    pub items: Vec<usize>,
    pub next: usize,
}

fn witer(this: usize) -> &'static mut WIterState {
    state(cd!(ODWindowIteratorCClassData), this)
}

methods! {
    fn WiFirst(this: usize, ev: usize) -> usize {
        let s = witer(this);
        s.next = 1;
        s.items.first().copied().unwrap_or(0)
    }
    fn WiNext(this: usize, ev: usize) -> usize {
        let s = witer(this);
        let r = s.items.get(s.next).copied().unwrap_or(0);
        s.next += 1;
        r
    }
    fn WiIsNotComplete(this: usize, ev: usize) -> usize {
        let s = witer(this);
        (s.next > 0 && s.next <= s.items.len()) as usize
    }
    fn WiSomUninit(this: usize) -> usize {
        drop_state::<WIterState>(cd!(ODWindowIteratorCClassData), this);
        call_parent(cd!(ODWindowIteratorCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static WINDOW_ITER_M: &[(&str, M)] = &[("First", WiFirst::t), ("Next", WiNext::t), ("IsNotComplete", WiIsNotComplete::t)];
pub static WINDOW_ITER_O: &[(&str, M)] = &[("SOMObject::somUninit", WiSomUninit::t)];

// ---- ODArbitrator and ODFocusSet ----

#[derive(Default)]
pub struct ArbState {
    pub owners: BTreeMap<usize, usize>,
    pub registered: Vec<usize>,
}

fn arb(this: usize) -> &'static mut ArbState {
    state(cd!(ODArbitratorCClassData), this)
}

#[derive(Default)]
pub struct SetState {
    pub foci: Vec<usize>,
}

fn fset(this: usize) -> &'static mut SetState {
    state(cd!(ODFocusSetCClassData), this)
}

/// The frame owning a focus (not acquired).
pub fn focus_owner(this: usize, focus: usize) -> usize {
    arb(this).owners.get(&focus).copied().unwrap_or(0)
}

fn frame_part(f: usize, ev: usize) -> usize {
    if f == 0 { 0 } else { layout::FAcquirePart(f, ev) }
}

/// Give a focus to a frame: its previous owner's part is told it lost it.
pub fn grant(this: usize, ev: usize, focus: usize, frame: usize) {
    let old = arb(this).owners.insert(focus, frame).unwrap_or(0);
    if old != 0 && old != frame {
        let p = frame_part(old, ev);
        if p != 0 {
            call(cd!(ODPartClassData), 26, &[p, ev, focus, old, frame]);
            release(p, ev);
        }
    }
}

methods! {
    fn RequestFocusSet(this: usize, ev: usize, set: usize, frame: usize) -> usize {
        for f in fset(set).foci.clone() {
            grant(this, ev, f, frame);
        }
        1
    }
    fn RequestFocus(this: usize, ev: usize, focus: usize, frame: usize) -> usize {
        grant(this, ev, focus, frame);
        1
    }
    fn RelinquishFocusSet(this: usize, ev: usize, set: usize, frame: usize) -> usize {
        for f in fset(set).foci.clone() {
            RelinquishFocus(this, ev, f, frame);
        }
        0
    }
    fn RelinquishFocus(this: usize, ev: usize, focus: usize, frame: usize) -> usize {
        let a = arb(this);
        if a.owners.get(&focus) == Some(&frame) {
            a.owners.remove(&focus);
        }
        0
    }
    fn TransferFocus(this: usize, ev: usize, focus: usize, from: usize, to: usize) -> usize {
        arb(this).owners.insert(focus, to);
        let p = frame_part(to, ev);
        if p != 0 {
            call(cd!(ODPartClassData), 28, &[p, ev, focus, to]);
            release(p, ev);
        }
        0
    }
    fn TransferFocusSet(this: usize, ev: usize, set: usize, from: usize, to: usize) -> usize {
        for f in fset(set).foci.clone() {
            TransferFocus(this, ev, f, from, to);
        }
        0
    }
    fn AcquireFocusOwner(this: usize, ev: usize, focus: usize) -> usize { acquire(arb(this).owners.get(&focus).copied().unwrap_or(0), ev) }
    fn CreateOwnerIterator(this: usize, ev: usize, focus: usize) -> usize { 0 }
    fn RegisterFocus(this: usize, ev: usize, focus: usize, module: usize) -> usize {
        arb(this).registered.push(focus);
        0
    }
    fn UnregisterFocus(this: usize, ev: usize, focus: usize) -> usize {
        arb(this).registered.retain(|&f| f != focus);
        0
    }
    fn IsFocusRegistered(this: usize, ev: usize, focus: usize) -> usize { 1 }
    fn IsFocusExclusive(this: usize, ev: usize, focus: usize) -> usize { 1 }
    fn AcquireFocusModule(this: usize, ev: usize, focus: usize) -> usize { 0 }
    fn CreateFocusSet(this: usize, ev: usize) -> usize { new_of(cd!(ODFocusSetClassData), "ODFocusSet") }
    fn ArbSomUninit(this: usize) -> usize {
        drop_state::<ArbState>(cd!(ODArbitratorCClassData), this);
        call_parent(cd!(ODArbitratorCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    fn Add(this: usize, ev: usize, focus: usize) -> usize {
        let s = fset(this);
        if !s.foci.contains(&focus) {
            s.foci.push(focus);
        }
        0
    }
    fn SetRemove(this: usize, ev: usize, focus: usize) -> usize {
        fset(this).foci.retain(|&f| f != focus);
        0
    }
    fn Contains(this: usize, ev: usize, focus: usize) -> usize { fset(this).foci.contains(&focus) as usize }
    fn SetSomUninit(this: usize) -> usize {
        drop_state::<SetState>(cd!(ODFocusSetCClassData), this);
        call_parent(cd!(ODFocusSetCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static ARBITRATOR_M: &[(&str, M)] = &[
    ("RequestFocusSet", RequestFocusSet::t),
    ("RequestFocus", RequestFocus::t),
    ("RelinquishFocusSet", RelinquishFocusSet::t),
    ("RelinquishFocus", RelinquishFocus::t),
    ("TransferFocus", TransferFocus::t),
    ("TransferFocusSet", TransferFocusSet::t),
    ("AcquireFocusOwner", AcquireFocusOwner::t),
    ("CreateOwnerIterator", CreateOwnerIterator::t),
    ("RegisterFocus", RegisterFocus::t),
    ("UnregisterFocus", UnregisterFocus::t),
    ("IsFocusRegistered", IsFocusRegistered::t),
    ("IsFocusExclusive", IsFocusExclusive::t),
    ("AcquireFocusModule", AcquireFocusModule::t),
    ("CreateFocusSet", CreateFocusSet::t),
];
pub static ARBITRATOR_O: &[(&str, M)] = &[("SOMObject::somUninit", ArbSomUninit::t)];
pub static FOCUS_SET_M: &[(&str, M)] = &[("Add", Add::t), ("Remove", SetRemove::t), ("Contains", Contains::t)];
pub static FOCUS_SET_O: &[(&str, M)] = &[("SOMObject::somUninit", SetSomUninit::t)];

// ---- Clipboard, drag and drop: a scratch draft's storage unit ----

#[derive(Default)]
pub struct TransferState {
    pub draft: usize,
    pub su: usize,
    pub update: u32,
}

fn transfer(this: usize) -> &'static mut TransferState {
    state(cd!(ODClipboardCClassData), this)
}

fn dnd(this: usize) -> &'static mut TransferState {
    state(cd!(ODBaseDragAndDropCClassData), this)
}

fn content(t: &mut TransferState, ev: usize) -> usize {
    if t.su == 0 && t.draft != 0 {
        t.su = storage::CreateStorageUnit(t.draft, ev);
    }
    t.su
}

methods! {
    fn GetUpdateID(this: usize, ev: usize) -> usize { transfer(this).update as usize }
    fn ClipClear(this: usize, ev: usize) -> usize {
        let t = transfer(this);
        if t.su != 0 {
            storage::sus(t.su).props.clear();
        }
        t.update += 1;
        0
    }
    fn ClipGetContentStorageUnit(this: usize, ev: usize) -> usize {
        let t = transfer(this);
        if t.draft != 0 {
            let su = crate::clipboard::import(t.draft, t.su);
            if su != 0 {
                t.su = su;
            }
        }
        content(transfer(this), ev)
    }
    fn ExportClipboard(this: usize, ev: usize) -> usize {
        let t = transfer(this);
        let su = content(t, ev);
        crate::clipboard::export(su);
        0
    }
    fn RegisterClipboardFormat(this: usize, ev: usize, ty: usize, fmt: usize, info: usize) -> usize {
        crate::clipboard::register(cstr(ty), fmt as u32 & 0xffff, info as u32);
        1
    }
    fn ClipNoArg(this: usize, ev: usize) -> usize { 0 }
    fn ClipOneArg(this: usize, ev: usize, a: usize) -> usize { 0 }
    fn ClipThreeArgs(this: usize, ev: usize, a: usize, b: usize, c: usize) -> usize { 0 }
    fn DndClear(this: usize, ev: usize) -> usize {
        let t = dnd(this);
        if t.su != 0 {
            storage::sus(t.su).props.clear();
        }
        0
    }
    fn DndGetContentStorageUnit(this: usize, ev: usize) -> usize { content(dnd(this), ev) }
    fn StartDrag(this: usize, ev: usize, src: usize, image_type: usize, image: usize, result: usize, refcon: usize) -> usize {
        // rs2 has no OpenDoc drag yet: the drop is refused.
        write::<usize>(result, 0);
        0
    }
    fn DndNoArg(this: usize, ev: usize) -> usize { 0 }
    fn DndTwoArgs(this: usize, ev: usize, a: usize, b: usize) -> usize { 0 }
    fn ItFirst(this: usize, ev: usize) -> usize { 0 }
    fn ItNext(this: usize, ev: usize) -> usize { 0 }
    fn ItIsNotComplete(this: usize, ev: usize) -> usize { 0 }
}

pub static CLIPBOARD_M: &[(&str, M)] = &[
    ("GetUpdateID", GetUpdateID::t),
    ("Clear", ClipClear::t),
    ("GetContentStorageUnit", ClipGetContentStorageUnit::t),
    ("ActionDone", ClipOneArg::t),
    ("ActionUndone", ClipOneArg::t),
    ("ActionRedone", ClipOneArg::t),
    ("SetPlatformClipboard", ClipOneArg::t),
    ("ExportClipboard", ExportClipboard::t),
    ("DraftClosing", ClipOneArg::t),
    ("DraftSaved", ClipOneArg::t),
    ("ShowPasteAsDialog", ClipThreeArgs::t),
    ("RegisterClipboardFormat", RegisterClipboardFormat::t),
];
pub static BASE_DND_M: &[(&str, M)] = &[
    ("Clear", DndClear::t),
    ("GetContentStorageUnit", DndGetContentStorageUnit::t),
    ("StartDrag", StartDrag::t),
    ("ShowPasteAsDialog", DndTwoArgs::t),
    ("GetDragAttributes", DndNoArg::t),
    ("GetDragReference", DndNoArg::t),
];
pub static DND_M: &[(&str, M)] = &[
    ("GetDragItemIterator", DndNoArg::t),
    ("GetDraggedFrame", DndNoArg::t),
    ("SetDraggedFrame", DndTwoArgs::t),
    ("CanEmbed", DndTwoArgs::t),
    ("CanIncorporate", DndTwoArgs::t),
    ("GetDropOperation", DndNoArg::t),
    ("SetDropOperation", DndTwoArgs::t),
    ("GetDragOperationAttributes", DndNoArg::t),
];
pub static DRAG_ITEM_ITER_M: &[(&str, M)] = &[("First", ItFirst::t), ("Next", ItNext::t), ("IsNotComplete", ItIsNotComplete::t)];

// ---- ODHelp, extensions, the dispatcher, name spaces, type lists ----

methods! {
    fn Nothing0(this: usize, ev: usize) -> usize { 0 }
    fn Nothing1(this: usize, ev: usize, a: usize) -> usize { 0 }
    fn Nothing2(this: usize, ev: usize, a: usize, b: usize) -> usize { 0 }
    fn Nothing3(this: usize, ev: usize, a: usize, b: usize, c: usize) -> usize { 0 }
    fn True0(this: usize, ev: usize) -> usize { 1 }
    fn InitExtension(this: usize, ev: usize, base: usize) -> usize {
        *somsup::data::<usize>(cd!(ODExtensionCClassData), this) = base;
        0
    }
    fn GetBase(this: usize, ev: usize) -> usize { acquire(*somsup::data::<usize>(cd!(ODExtensionCClassData), this), ev) }
    fn BaseRemoved(this: usize, ev: usize) -> usize {
        *somsup::data::<usize>(cd!(ODExtensionCClassData), this) = 0;
        0
    }
    fn IsValid(this: usize, ev: usize) -> usize { (*somsup::data::<usize>(cd!(ODExtensionCClassData), this) != 0) as usize }
    fn InitSettingsExtension(this: usize, ev: usize, base: usize) -> usize { InitExtension(this, ev, base) }
    fn InitStatusLineExtension(this: usize, ev: usize, base: usize) -> usize { InitExtension(this, ev, base) }
    fn IsFacetWindow(this: usize, ev: usize, hwnd: usize) -> usize { 0 }
}

pub static HELP_M: &[(&str, M)] = &[
    ("InitHelp", Nothing0::t),
    ("DisplayHelp", Nothing2::t),
    ("DisplayHelpIndex", Nothing1::t),
    ("DisplayHelpUsingHelp", Nothing1::t),
];
pub static EXTENSION_M: &[(&str, M)] = &[
    ("InitExtension", InitExtension::t),
    ("GetBase", GetBase::t),
    ("BaseRemoved", BaseRemoved::t),
    ("IsValid", IsValid::t),
    ("CheckValid", Nothing0::t),
];

macro_rules! numbered {
    ($cls:literal, $($i:literal => $f:expr,)*) => {
        &[$((concat!($cls, "_m", $i), $f),)*]
    };
}

pub static SETTINGS_EXTENSION_M: &[(&str, M)] = &[
    ("ODSettingsExtension_m0", Nothing0::t),
    ("ODSettingsExtension_m1", Nothing0::t),
    ("ODSettingsExtension_m2", Nothing0::t),
    ("ODSettingsExtension_m3", Nothing0::t),
    ("ODSettingsExtension_m4", Nothing0::t),
    ("ODSettingsExtension_m5", Nothing0::t),
    ("ODSettingsExtension_m6", Nothing0::t),
    ("ODSettingsExtension_m7", Nothing0::t),
    ("ODSettingsExtension_m8", Nothing0::t),
    ("ODSettingsExtension_m9", Nothing0::t),
    ("ODSettingsExtension_m10", Nothing0::t),
    ("ODSettingsExtension_m11", Nothing0::t),
    ("ODSettingsExtension_m12", Nothing0::t),
    ("ODSettingsExtension_m13", Nothing0::t),
    ("ODSettingsExtension_m14", Nothing0::t),
    ("ODSettingsExtension_m15", Nothing0::t),
    ("ODSettingsExtension_m16", Nothing0::t),
    ("InitSettingsExtension", InitSettingsExtension::t),
    ("AddNotebookPages", crate::settings::AddNotebookPages::t),
    ("TitleChanged", crate::settings::TitleChanged::t),
    ("ODSettingsExtension_m20", Nothing0::t),
    ("IconChanged", crate::settings::IconChanged::t),
    ("CloseSettings", crate::settings::CloseSettings::t),
    ("ShowSettings", crate::settings::ShowSettings::t),
];
pub static STATUS_LINE_M: &[(&str, M)] = &[
    ("SetStatusLineText", Nothing2::t),
    ("ClearStatusLineText", Nothing1::t),
    ("InitStatusLineExtension", InitStatusLineExtension::t),
];
pub static NAME_SPACE_MANAGER_M: &[(&str, M)] = &[
    ("InitNameSpaceManager", Nothing0::t),
    ("CreateNameSpace", Nothing3::t),
    ("DeleteNameSpace", Nothing1::t),
    ("HasNameSpace", Nothing1::t),
];
pub static TYPE_LIST_M: &[(&str, M)] = &[
    ("InitTypeList", Nothing1::t),
    ("AddLast", Nothing1::t),
    ("Remove", Nothing1::t),
    ("Contains", Nothing1::t),
    ("Count", Nothing0::t),
    ("CreateTypeListIterator", Nothing0::t),
];
pub static FACET_WINDOW_META_M: &[(&str, M)] = &[("IsFacetWindow", IsFacetWindow::t)];

pub static OSA_M: &[(&str, M)] = numbered!("OSAScriptingComponent",
    0 => Nothing0::t, 1 => Nothing0::t, 2 => Nothing0::t, 3 => Nothing0::t, 4 => Nothing0::t, 5 => Nothing0::t,
    6 => Nothing0::t, 7 => Nothing0::t, 8 => Nothing0::t, 9 => Nothing0::t, 10 => Nothing0::t, 11 => Nothing0::t,
    12 => Nothing0::t, 13 => Nothing0::t, 14 => Nothing0::t, 15 => Nothing0::t, 16 => Nothing0::t, 17 => Nothing0::t,
    18 => Nothing0::t, 19 => Nothing0::t, 20 => Nothing0::t, 21 => Nothing0::t, 22 => Nothing0::t, 23 => Nothing0::t,
    24 => Nothing0::t, 25 => Nothing0::t, 26 => Nothing0::t, 27 => Nothing0::t, 28 => Nothing0::t, 29 => Nothing0::t,
    30 => Nothing0::t, 31 => Nothing0::t, 32 => Nothing0::t, 33 => Nothing0::t, 34 => Nothing0::t, 35 => Nothing0::t,
    36 => Nothing0::t, 37 => Nothing0::t, 38 => Nothing0::t, 39 => Nothing0::t,
);

/// errAEEventNotHandled: what the Apple Event Manager calls return (rs2
/// has no Apple events).
pub extern "C" fn ae_not_handled() -> usize {
    (-1708i32) as usize
}

/// AEDisposeDesc(desc): clears the descriptor.
pub extern "C" fn ae_dispose_desc(desc: usize) -> usize {
    if desc != 0 {
        write::<[u32; 2]>(desc, [0x6e756c6c, 0]); // typeNull
    }
    0
}

pub extern "C" fn no_class(_major: i32, _minor: i32) -> usize {
    0
}

