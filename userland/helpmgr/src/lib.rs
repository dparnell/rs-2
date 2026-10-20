//! HELPMGR.DLL: the Information Presentation Facility's help manager.
//!
//! A help instance (WinCreateHelpInstance) is an object window holding
//! the help libraries (`.HLP`, found along HELP), the application's help
//! table and its settings; applications send it HM_ messages, and
//! WinAssociateHelpInstance ties it to their frames (WM_SETHELPINFO). F1
//! reaches it through the help hook (HK_HELP), which PMWIN calls when
//! WM_HELP gets to a frame: the hook finds the help instance of the active
//! window and looks up the panel in the help table, as IBM's HelpProcessor
//! does. Panels show in a help window (window.rs) laid out by `crates/ipf`.
//!
//! VIEW.EXE uses the same window for books (`Rs2ViewBook`).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod ddf;
mod exports;
mod library;
mod pm;
mod render;
mod window;

use alloc::boxed::Box;
use alloc::vec::Vec;
use library::Library;
use pm::*;

pub const HMERR_NO_FRAME_WND_IN_CHAIN: u32 = 0x1001;
pub const HMERR_INVALID_ASSOC_APP_WND: u32 = 0x1002;
pub const HMERR_INVALID_ASSOC_HELP_INST: u32 = 0x1003;
pub const HMERR_INVALID_DESTROY_HELP_INST: u32 = 0x1004;
pub const HMERR_NO_HELP_INST_IN_CHAIN: u32 = 0x1005;
pub const HMERR_INVALID_HELP_INSTANCE_HDL: u32 = 0x1006;
pub const HMERR_INVALID_QUERY_APP_WND: u32 = 0x1007;
pub const HMERR_HELP_INST_CALLED_INVALID: u32 = 0x1008;
pub const HMERR_HELPTABLE_UNDEFINE: u32 = 0x1009;
pub const HMERR_HELP_INSTANCE_UNDEFINE: u32 = 0x100a;
pub const HMERR_HELPITEM_NOT_FOUND: u32 = 0x100b;
pub const HMERR_INVALID_HELPSUBITEM_SIZE: u32 = 0x100c;
pub const HMERR_HELPSUBITEM_NOT_FOUND: u32 = 0x100d;
pub const HMERR_INDEX_NOT_FOUND: u32 = 0x2001;
pub const HMERR_CONTENT_NOT_FOUND: u32 = 0x2002;
pub const HMERR_OPEN_LIB_FILE: u32 = 0x2003;
pub const HMERR_READ_LIB_FILE: u32 = 0x2004;
pub const HMERR_LOAD_DLL_BAD_FORMAT: u32 = 0x2005;
pub const HMERR_PANEL_NOT_FOUND: u32 = 0x2010;
pub const HMERR_DATABASE_NOT_OPEN: u32 = 0x2011;
/// HM_ERROR when HM_QUERY_DDF_DATA did not return a DDF (IBM's
/// PnlGethDDF; not in the Toolkit).
pub const HMERR_DDF_INVALID: u32 = 0x2014;

pub const HM_DISMISS_WINDOW: u32 = 0x221;
pub const HM_DISPLAY_HELP: u32 = 0x222;
pub const HM_EXT_HELP: u32 = 0x223;
pub const HM_SET_ACTIVE_WINDOW: u32 = 0x224;
pub const HM_LOAD_HELP_TABLE: u32 = 0x225;
pub const HM_CREATE_HELP_TABLE: u32 = 0x226;
pub const HM_SET_HELP_WINDOW_TITLE: u32 = 0x227;
pub const HM_SET_SHOW_PANEL_ID: u32 = 0x228;
pub const HM_REPLACE_HELP_FOR_HELP: u32 = 0x229;
pub const HM_HELP_INDEX: u32 = 0x22a;
pub const HM_HELP_CONTENTS: u32 = 0x22b;
pub const HM_KEYS_HELP: u32 = 0x22c;
pub const HM_SET_HELP_LIBRARY_NAME: u32 = 0x22d;
pub const HM_ERROR: u32 = 0x22e;
pub const HM_HELPSUBITEM_NOT_FOUND: u32 = 0x22f;
pub const HM_QUERY_KEYS_HELP: u32 = 0x230;
pub const HM_EXT_HELP_UNDEFINED: u32 = 0x232;
pub const HM_SET_OBJCOM_WINDOW: u32 = 0x238;
pub const HM_QUERY_DDF_DATA: u32 = 0x23a;
pub const HM_INVALIDATE_DDF_DATA: u32 = 0x23b;
pub const HM_INFORM: u32 = 0x234;
pub const HM_NOTIFY: u32 = 0x242;
pub const HM_RESOURCEID: u32 = 0;
pub const HM_PANELNAME: u32 = 1;

pub const OPEN_PAGE: u32 = 2;
pub const OPEN_TOC: u32 = 4;
pub const OPEN_INDEX: u32 = 5;
pub const HELP_REQUESTED: u32 = 9;

const WM_QUERYHELPINFO: u32 = 0x005b;
const WM_SETHELPINFO: u32 = 0x005c;
const WM_QUERYFRAMEINFO: u32 = 0x0050;
const HK_HELP: i32 = 5;
const HMQ_CURRENT: u32 = 1;
const HLPM_MENU: u16 = 0xfffd;
const FID_SYSMENU: u16 = 0x8007;
const FID_CLIENT: u16 = 0x8008;
const RT_HELPTABLE: u32 = 18;
const RT_HELPSUBTABLE: u32 = 19;

const CLASS: &[u8] = b"HMHelpInstance\0";

/// The help table an application gave: in memory or a resource.
#[derive(Clone, Copy)]
pub enum HelpTable {
    None,
    Memory(u32),
    Resource { hmod: u32, id: u16 },
}

/// A panel: library number and TOC index in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelRef {
    pub lib: usize,
    pub toc: u16,
}

pub struct Instance {
    pub hwnd: u32,
    pub hab: u32,
    pub libs: Vec<Library>,
    /// The library names as given (for messages).
    pub names: Vec<u8>,
    pub table: HelpTable,
    pub title: Vec<u8>,
    pub show_ids: bool,
    pub help_for_help: u16,
    /// HM_SET_ACTIVE_WINDOW, or the window the last request came from.
    pub active: u32,
    /// HM_SET_OBJCOM_WINDOW: where HM_QUERY_DDF_DATA goes (0: the active
    /// window).
    pub objcom: u32,
    /// The frame last associated with the instance.
    pub assoc: u32,
    /// The help window's frame (0: not shown).
    pub frame: u32,
    /// The footnote window's frame (0: none).
    pub footnote: u32,
    /// A book shown by VIEW: no application, the program ends with it.
    pub book: bool,
}

struct Registry(core::cell::UnsafeCell<Vec<*mut Instance>>);
unsafe impl Sync for Registry {}
static INSTANCES: Registry = Registry(core::cell::UnsafeCell::new(Vec::new()));

fn instances() -> &'static mut Vec<*mut Instance> {
    unsafe { &mut *INSTANCES.0.get() }
}

/// The instance of help instance window `h`, if it is one of this
/// process's.
pub fn instance(h: u32) -> Option<&'static mut Instance> {
    if h == 0 {
        return None;
    }
    instances().iter().copied().find(|&p| unsafe { (*p).hwnd } == h).map(|p| unsafe { &mut *p })
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// A line on the serial port (RS2API.4), for debugging.
#[allow(dead_code)]
pub fn trace(args: core::fmt::Arguments) {
    unsafe extern "C" {
        #[link_name = "RS2API.4"]
        fn Rs2DebugTrace(psz: *const u8) -> u32;
    }
    let mut s = alloc::format!("{}", args).into_bytes();
    s.push(0);
    unsafe { Rs2DebugTrace(s.as_ptr()) };
}

pub fn set_error(e: u32) {
    let _ = e;
}

fn post_error(inst: &Instance, e: u32) {
    let to = inst.app_window();
    if to != 0 {
        post(to, HM_ERROR, e, 0);
    }
}

fn register(hab: u32) {
    unsafe {
        WinRegisterClass(hab, CLASS.as_ptr(), instance_proc, 0, 4);
    }
    window::register(hab);
}

/// Open the libraries of name list `names`, with the external databases
/// they name.
fn open_libraries(names: &[u8], book: bool) -> Result<Vec<Library>, u32> {
    let mut libs: Vec<Library> = Vec::new();
    for n in library::names(names) {
        libs.push(library::open(&n, book)?);
    }
    if libs.is_empty() {
        return Err(HMERR_OPEN_LIB_FILE);
    }
    // External databases (`:link database=`), when they can be found.
    let mut i = 0;
    while i < libs.len() {
        for db in libs[i].doc().databases() {
            let base = db.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(&db).to_vec();
            let known = libs.iter().any(|l| l.path.rsplit(|&c| c == b'\\').next().is_some_and(|p| p.eq_ignore_ascii_case(&base)));
            if !known && libs.len() < 32 {
                if let Ok(l) = library::open(&db, book).or_else(|_| library::open(&base, book)) {
                    libs.push(l);
                }
            }
        }
        i += 1;
    }
    let mut base = 0u32;
    for l in libs.iter_mut() {
        l.base = base;
        base += l.doc().header.toc_count as u32;
    }
    Ok(libs)
}

/// Create a help instance (also for VIEW's books).
pub fn create(hab: u32, names: &[u8], title: &[u8], table: HelpTable, show_ids: bool, book: bool) -> Result<u32, u32> {
    register(hab);
    let libs = open_libraries(names, book)?;
    let h = unsafe {
        WinCreateWindow(HWND_OBJECT, CLASS.as_ptr(), b"\0".as_ptr(), 0, 0, 0, 0, 0, 0, HWND_TOP, 0, core::ptr::null(), core::ptr::null())
    };
    if h == 0 {
        return Err(HMERR_HELP_INSTANCE_UNDEFINE);
    }
    let inst = Box::into_raw(Box::new(Instance {
        hwnd: h,
        hab,
        libs,
        names: names.to_vec(),
        table,
        title: title.to_vec(),
        show_ids,
        help_for_help: 0,
        active: 0,
        objcom: 0,
        assoc: 0,
        frame: 0,
        footnote: 0,
        book,
    }));
    let first = instances().is_empty();
    instances().push(inst);
    if first && !book {
        unsafe { WinSetHook(hab, HMQ_CURRENT, HK_HELP, HelpProcessor as *const () as u32, 0) };
    }
    Ok(h)
}

/// WinCreateHelpInstance(hab, phinit): HELPINIT is cb, ulReturnCode,
/// pszTutorialName, phtHelpTable, hmodHelpTableModule,
/// hmodAccelActionBarModule, idAccelTable, idActionBar,
/// pszHelpWindowTitle, fShowPanelId, pszHelpLibraryName.
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateHelpInstance(hab: u32, init: *mut u32) -> u32 {
    if init.is_null() {
        return 0;
    }
    let f = |i: usize| unsafe { init.add(i).read_unaligned() };
    let cb = f(0);
    let field = |i: usize| if (cb as usize) >= (i + 1) * 4 { f(i) } else { 0 };
    let pht = field(3);
    let table = if pht == 0 {
        HelpTable::None
    } else if pht & 0xffff_0000 == 0xffff_0000 {
        HelpTable::Resource { hmod: field(4), id: pht as u16 }
    } else {
        HelpTable::Memory(pht)
    };
    let title = unsafe { cstr(field(8) as *const u8) }.to_vec();
    let names = unsafe { cstr(field(10) as *const u8) }.to_vec();
    let show_ids = field(9) & 1 != 0;
    match create(hab, &names, &title, table, show_ids, false) {
        Ok(h) => {
            unsafe { init.add(1).write_unaligned(0) };
            h
        }
        Err(e) => {
            unsafe { init.add(1).write_unaligned(e) };
            set_error(e);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyHelpInstance(h: u32) -> u32 {
    if instance(h).is_none() {
        set_error(HMERR_INVALID_DESTROY_HELP_INST);
        return 0;
    }
    unsafe { WinDestroyWindow(h) }
}

/// The frame `h` is in (itself or a parent).
fn frame_of(mut h: u32) -> u32 {
    for _ in 0..64 {
        if h == 0 || h == HWND_DESKTOP {
            return 0;
        }
        if send(h, WM_QUERYFRAMEINFO, 0, 0) & 1 != 0 {
            return h;
        }
        h = unsafe { WinQueryWindow(h, QW_PARENT) };
    }
    0
}

/// The help instance for window `h`: the first frame up its parents (at
/// the desktop: owners) associated with one (HAPIFindWndWithHelp).
fn find_instance(h: u32) -> u32 {
    let mut w = h;
    for _ in 0..64 {
        let f = frame_of(w);
        if f == 0 {
            return 0;
        }
        let inst = send(f, WM_QUERYHELPINFO, 0, 0);
        if inst != 0 {
            return inst;
        }
        let p = unsafe { WinQueryWindow(f, QW_PARENT) };
        w = if p == 0 || p == HWND_DESKTOP { unsafe { WinQueryWindow(f, QW_OWNER) } } else { p };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryHelpInstance(h: u32) -> u32 {
    if unsafe { WinIsWindow(0, h) } == 0 {
        set_error(HMERR_INVALID_QUERY_APP_WND);
        return 0;
    }
    let inst = find_instance(h);
    if inst == 0 || unsafe { WinIsWindow(0, inst) } == 0 {
        set_error(HMERR_NO_HELP_INST_IN_CHAIN);
        return 0;
    }
    inst
}

#[unsafe(no_mangle)]
pub extern "C" fn WinAssociateHelpInstance(help: u32, app: u32) -> u32 {
    if unsafe { WinIsWindow(0, app) } == 0 {
        set_error(HMERR_INVALID_ASSOC_APP_WND);
        return 0;
    }
    if help != 0 && unsafe { WinIsWindow(0, help) } == 0 {
        set_error(HMERR_INVALID_ASSOC_HELP_INST);
        return 0;
    }
    let f = frame_of(app);
    if f == 0 {
        set_error(HMERR_NO_FRAME_WND_IN_CHAIN);
        return 0;
    }
    send(f, WM_SETHELPINFO, help, 0);
    if let Some(i) = instance(help) {
        i.assoc = f;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinLoadHelpTable(help: u32, id: u32, hmod: u32) -> u32 {
    (send(help, HM_LOAD_HELP_TABLE, (id & 0xffff) | 0xffff_0000, hmod) == 0) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCreateHelpTable(help: u32, table: u32) -> u32 {
    (send(help, HM_CREATE_HELP_TABLE, table, 0) == 0) as u32
}

/// rs2: show book(s) `names` ("A.INF+B.INF") as VIEW does, at the panel
/// whose title or index entry matches `topic` (or the contents); returns
/// the help instance (its window posts WM_QUIT when closed) or 0, with
/// the error in `*rc`.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2ViewBook(hab: u32, names: *const u8, topic: *const u8, rc: *mut u32) -> u32 {
    let names = unsafe { cstr(names) }.to_vec();
    let topic = unsafe { cstr(topic) }.to_vec();
    match create(hab, &names, b"", HelpTable::None, false, true) {
        Ok(h) => {
            let inst = instance(h).unwrap();
            let title = inst.libs[0].doc().title().to_vec();
            inst.title = title;
            let found = if topic.is_empty() { None } else { window::find_topic(inst, &topic) };
            match found {
                Some(p) => window::show_panel(inst, p, true),
                None => window::show_contents(inst),
            }
            if !rc.is_null() {
                unsafe { *rc = 0 };
            }
            h
        }
        Err(e) => {
            if !rc.is_null() {
                unsafe { *rc = e };
            }
            0
        }
    }
}

// ---- Panel lookup ----

impl Instance {
    /// Where messages to the application go (IBM's MscAppInterface): the
    /// active window (HM_SET_ACTIVE_WINDOW, or where the last request
    /// came from), else the frame associated last.
    pub fn app_window(&self) -> u32 {
        if self.active != 0 { self.active } else { self.assoc }
    }

    /// The panel with resource id `res` in any library.
    pub fn panel_by_res(&self, res: u16) -> Option<PanelRef> {
        self.libs.iter().enumerate().find_map(|(lib, l)| l.doc().panel_by_res(res).map(|toc| PanelRef { lib, toc }))
    }

    pub fn panel_by_name(&self, name: &[u8]) -> Option<PanelRef> {
        self.libs.iter().enumerate().find_map(|(lib, l)| l.doc().panel_by_name(name).map(|toc| PanelRef { lib, toc }))
    }

    /// Display panel `id` (a resource id or name) as HM_DISPLAY_HELP
    /// does; returns 0 or an error code.
    pub fn display(&mut self, id: u32, kind: u32) -> u32 {
        let p = if kind == HM_PANELNAME {
            self.panel_by_name(unsafe { cstr(id as *const u8) })
        } else {
            self.panel_by_res(id as u16)
        };
        match p {
            Some(p) => {
                window::show_panel(self, p, true);
                0
            }
            None => {
                post_error(self, HMERR_PANEL_NOT_FOUND);
                HMERR_PANEL_NOT_FOUND
            }
        }
    }
}

// ---- The help table ----

/// A help table entry: the window, its subtable (a flat pointer, or a
/// resource id in the low word with 0xFFFF above) and its extended help
/// panel.
struct Entry {
    sub: u32,
    ext: u16,
}

fn read16(p: u32, off: usize) -> u16 {
    unsafe { ((p as usize + off) as *const u16).read_unaligned() }
}

fn read32(p: u32, off: usize) -> u32 {
    unsafe { ((p as usize + off) as *const u32).read_unaligned() }
}

/// The entry for window id `id`.
fn table_entry(inst: &Instance, id: u16) -> Option<Entry> {
    let (base, _res) = match inst.table {
        HelpTable::None => return None,
        HelpTable::Memory(p) => (p, None),
        HelpTable::Resource { hmod, id: rid } => {
            let mut p: *const u8 = core::ptr::null();
            if unsafe { DosGetResource(hmod, RT_HELPTABLE, rid as u32, &mut p) } != 0 {
                return None;
            }
            (p as u32, Some(hmod))
        }
    };
    let mut o = 0;
    for _ in 0..4096 {
        let w = read16(base, o);
        if w == 0 {
            return None;
        }
        if w == id {
            return Some(Entry { sub: read32(base, o + 2), ext: read16(base, o + 6) });
        }
        o += 8;
    }
    None
}

/// Look up `key` in a subtable (first word: words per entry; entries:
/// window id, panel, ...; ended by 0).
fn subtable_lookup(inst: &Instance, sub: u32, key: u16) -> Option<u16> {
    let base = if sub & 0xffff_0000 == 0xffff_0000 {
        let HelpTable::Resource { hmod, .. } = inst.table else { return None };
        let mut p: *const u8 = core::ptr::null();
        if unsafe { DosGetResource(hmod, RT_HELPSUBTABLE, sub & 0xffff, &mut p) } != 0 {
            return None;
        }
        p as u32
    } else {
        sub
    };
    if base == 0 {
        return None;
    }
    let size = read16(base, 0) as usize;
    if size < 2 {
        return None;
    }
    let mut o = 2;
    for _ in 0..4096 {
        let w = read16(base, o);
        if w == 0 {
            return None;
        }
        if w == key {
            return Some(read16(base, o + 2));
        }
        o += size * 2;
    }
    None
}

/// The panel for a help request (IBM's HOOKSearchHelpTable): `mode`
/// (HLPM_*), `topic`, `sub`; `active` is the active window. Returns 0
/// when there is nothing to show (the application was told).
fn search_table(inst: &Instance, mode: u16, topic: u16, sub: u16, active: u32) -> u16 {
    let key = match mode {
        HLPM_MENU => {
            if sub != 0xffff {
                sub
            } else {
                topic
            }
        }
        0xfffe | 0xffff => {
            if sub == FID_CLIENT {
                0xffff
            } else {
                sub
            }
        }
        _ => topic,
    };
    if mode == HLPM_MENU && topic == FID_SYSMENU {
        // The system menu's own help is the help for help.
    }
    let Some(e) = table_entry(inst, id(active) as u16) else {
        post(active, HM_ERROR, HMERR_HELPITEM_NOT_FOUND, 0);
        return 0;
    };
    if key == 0xffff {
        if e.ext == 0 {
            send(active, HM_EXT_HELP_UNDEFINED, 0, 0);
        }
        return e.ext;
    }
    match subtable_lookup(inst, e.sub, key) {
        Some(p) => p,
        None => {
            if send(active, HM_HELPSUBITEM_NOT_FOUND, mode as u32, (sub as u32) << 16 | topic as u32) != 0 {
                0
            } else {
                e.ext
            }
        }
    }
}

/// The help hook (HK_HELP), called by PMWIN when WM_HELP gets to a frame
/// of this thread (IBM's HelpProcessor).
pub extern "C" fn HelpProcessor(_hab: u32, mode: u32, topic: u32, sub: u32, _rect: *const RectL) -> u32 {
    let active = unsafe { WinQueryActiveWindow(HWND_DESKTOP) };
    if active == 0 {
        return 0;
    }
    let (mut pid, mut tid) = (0, 0);
    unsafe { WinQueryWindowProcess(active, &mut pid, &mut tid) };
    if pid != pm::pid() {
        return 0;
    }
    // Help in a help window: nothing more to show.
    if window::is_help_window(active) {
        return 1;
    }
    let ih = find_instance(active);
    let Some(inst) = instance(ih) else {
        post(active, HM_ERROR, HMERR_HELP_INSTANCE_UNDEFINE, 0);
        return 0;
    };
    inst.active = active;
    if let HelpTable::None = inst.table {
        post(active, HM_ERROR, HMERR_HELPTABLE_UNDEFINE, 0);
        return 1;
    }
    let panel = search_table(inst, mode as u16, topic as u16, sub as u16, active);
    if panel != 0 {
        inst.display(panel as u32, HM_RESOURCEID);
    }
    1
}

// ---- The help instance window ----

extern "C" fn instance_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(inst) = instance(h) else { return unsafe { WinDefWindowProc(h, msg, mp1, mp2) } };
    match msg {
        HM_DISPLAY_HELP => {
            if mp1 == 0 && mp2 == 0 {
                // Help for help.
                return match inst.help_for_help {
                    0 => {
                        window::show_contents(inst);
                        0
                    }
                    p => inst.display(p as u32, HM_RESOURCEID),
                };
            }
            inst.display(mp1, mp2)
        }
        HM_EXT_HELP => {
            let active = if inst.active != 0 { inst.active } else { unsafe { WinQueryActiveWindow(HWND_DESKTOP) } };
            match table_entry(inst, id(active) as u16) {
                Some(e) if e.ext != 0 => inst.display(e.ext as u32, HM_RESOURCEID),
                _ => {
                    send(active, HM_EXT_HELP_UNDEFINED, 0, 0);
                    0
                }
            }
        }
        HM_SET_ACTIVE_WINDOW => {
            inst.active = mp1;
            0
        }
        HM_LOAD_HELP_TABLE => {
            inst.table = HelpTable::Resource { hmod: mp2, id: mp1 as u16 };
            0
        }
        HM_CREATE_HELP_TABLE => {
            inst.table = if mp1 == 0 { HelpTable::None } else { HelpTable::Memory(mp1) };
            0
        }
        HM_SET_HELP_WINDOW_TITLE => {
            inst.title = unsafe { cstr(mp1 as *const u8) }.to_vec();
            window::set_title(inst);
            0
        }
        HM_SET_SHOW_PANEL_ID => {
            inst.show_ids = match mp1 & 0xffff {
                0 => false,
                1 => true,
                _ => !inst.show_ids,
            };
            window::set_title(inst);
            0
        }
        HM_REPLACE_HELP_FOR_HELP => {
            inst.help_for_help = mp1 as u16;
            0
        }
        HM_HELP_INDEX => {
            if inst.libs.iter().all(|l| l.doc().header.index_count == 0) {
                return HMERR_INDEX_NOT_FOUND;
            }
            window::show_index(inst);
            0
        }
        HM_HELP_CONTENTS => {
            window::show_contents(inst);
            0
        }
        HM_KEYS_HELP => {
            let active = if inst.active != 0 { inst.active } else { unsafe { WinQueryActiveWindow(HWND_DESKTOP) } };
            let p = send(active, HM_QUERY_KEYS_HELP, 0, 0);
            if p != 0 { inst.display(p, HM_RESOURCEID) } else { 0 }
        }
        HM_SET_HELP_LIBRARY_NAME => {
            let names = unsafe { cstr(mp1 as *const u8) }.to_vec();
            match open_libraries(&names, inst.book) {
                Ok(libs) => {
                    window::close(inst);
                    inst.libs = libs;
                    inst.names = names;
                    0
                }
                Err(e) => e,
            }
        }
        HM_DISMISS_WINDOW => {
            window::close(inst);
            0
        }
        HM_SET_OBJCOM_WINDOW => core::mem::replace(&mut inst.objcom, mp1),
        // mp1: how many ids (0: all), mp2: the ids (u16 each).
        HM_INVALIDATE_DDF_DATA => {
            let ids: Vec<u16> = if mp1 == 0 || mp2 == 0 { Vec::new() } else { (0..mp1.min(4096) as usize).map(|i| read16(mp2, i * 2)).collect() };
            window::invalidate_ddf(inst, &ids);
            0
        }
        WM_DESTROY => {
            window::close(inst);
            let p = inst as *mut Instance;
            let v = instances();
            v.retain(|&q| q != p);
            let book = inst.book;
            if v.iter().all(|&q| unsafe { (*q).book }) && !book {
                unsafe { WinReleaseHook(inst.hab, HMQ_CURRENT, HK_HELP, HelpProcessor as *const () as u32, 0) };
            }
            unsafe { drop(Box::from_raw(p)) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}
