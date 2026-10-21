//! The help window: a frame whose client shows a panel (a page laid out
//! by `ipf::layout`, scrolled, with its links reachable by mouse and Tab)
//! or a list (contents, index, search results), with Previous, Search,
//! Index and Contents push buttons below. Following a link or choosing a
//! list entry remembers where the window was, for Previous (or Esc).

use crate::pm::*;
use crate::render::{Bitmaps, Draw, Gpi};
use crate::{Instance, PanelRef, HM_INFORM};
use alloc::vec;
use alloc::vec::Vec;
use ipf::layout::{layout_items, Hit, Options, Page};
use ipf::text::{decode, expand_ddf, Item, LinkData, Target};

const CLIENT: &[u8] = b"HMHelpWindow\0";
const PANEL: &[u8] = b"HMHelpPanel\0";
const FID_CLIENT: u32 = 0x8008;

const ID_PANEL: u32 = 10;
const ID_SCROLL: u32 = 11;
const ID_LIST: u32 = 12;
const ID_ENTRY: u32 = 13;
const ID_PREVIOUS: u32 = 1;
const ID_SEARCH: u32 = 2;
const ID_INDEX: u32 = 4;
const ID_CONTENTS: u32 = 5;
const BUTTONS: [(u32, &[u8]); 4] =
    [(ID_PREVIOUS, b"~Previous\0"), (ID_SEARCH, b"~Search...\0"), (ID_INDEX, b"~Index\0"), (ID_CONTENTS, b"~Contents\0")];

const BUTTON_ROW: i32 = 34;
const ENTRY_ROW: i32 = 30;

/// What the window shows.
#[derive(Clone, PartialEq, Eq)]
enum Mode {
    Panel(PanelRef),
    Contents,
    Index,
    Search(Vec<u8>),
}

struct View {
    inst: *mut Instance,
    frame: u32,
    client: u32,
    panel: u32,
    scroll: u32,
    list: u32,
    entry: u32,
    mode: Mode,
    history: Vec<(Mode, i32)>,
    page: Option<Page<'static>>,
    /// Width the page was laid out for.
    page_width: i32,
    top: i32,
    line: i32,
    sel: Option<u16>,
    /// The panels of the list's rows.
    rows: Vec<PanelRef>,
    bitmaps: Bitmaps,
    /// A footnote window: one panel, no buttons.
    footnote: bool,
    /// A window of its own in the help window (a viewport).
    vp: Option<Viewport>,
    /// Frames of the page's application-controlled viewports.
    acvps: Vec<u32>,
    /// The fonts of the page (font table and dynamic data).
    fonts: Vec<crate::render::FontSpec>,
    /// The dynamic data of the panel's `:ddf.`s: resource id, the text
    /// (pages borrow it) and whether it is still valid.
    ddf: Vec<(u16, alloc::boxed::Box<[u8]>, bool)>,
}

impl View {
    fn inst(&self) -> &'static mut Instance {
        unsafe { &mut *self.inst }
    }
}

fn view(h: u32) -> Option<&'static mut View> {
    let p = unsafe { WinQueryWindowULong(h, QWL_USER) } as *mut View;
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

fn view_of(inst: &Instance) -> Option<&'static mut View> {
    if inst.frame == 0 {
        return None;
    }
    view(unsafe { WinWindowFromID(inst.frame, FID_CLIENT) })
}

pub fn register(hab: u32) {
    unsafe {
        WinRegisterClass(hab, CLIENT.as_ptr(), client_proc, WS_CLIPCHILDREN, 4);
        WinRegisterClass(hab, PANEL.as_ptr(), panel_proc, 0, 4);
    }
}

/// Is `h` (or a window it is in) a help window?
pub fn is_help_window(h: u32) -> bool {
    help_window_instance(h).is_some()
}

/// The instance whose help (or footnote) window `h` is in.
pub fn help_window_instance(h: u32) -> Option<&'static mut Instance> {
    crate::instances().iter().copied().find(|&p| {
        let (f, n) = unsafe { ((*p).frame, (*p).footnote) };
        (f != 0 && is_inside(h, f)) || (n != 0 && is_inside(h, n))
    }).map(|p| unsafe { &mut *p })
}

fn is_inside(mut h: u32, f: u32) -> bool {
    for _ in 0..32 {
        if h == 0 || h == HWND_DESKTOP {
            return false;
        }
        if h == f {
            return true;
        }
        h = unsafe { WinQueryWindow(h, QW_PARENT) };
    }
    false
}

fn child(parent: u32, class: &[u8], text: &[u8], style: u32, id: u32) -> u32 {
    let t = z(text);
    unsafe {
        WinCreateWindow(parent, class.as_ptr(), t.as_ptr(), style, 0, 0, 0, 0, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null())
    }
}

/// The help window of `inst`, made if there is none.
fn ensure(inst: &mut Instance) -> &'static mut View {
    if let Some(v) = view_of(inst) {
        return v;
    }
    let v = make(inst, false, HWND_DESKTOP);
    inst.frame = v.frame;
    inst.compat[0x26..0x2a].copy_from_slice(&v.frame.to_le_bytes());
    // On the right of the screen, as IBM's help windows come up.
    let (sw, sh) = (sysval(SV_CXSCREEN), sysval(SV_CYSCREEN));
    let cx = (sw * 11 / 20).clamp(320.min(sw), 680);
    let cy = (sh * 3 / 4).clamp(240.min(sh), 640);
    unsafe { WinSetWindowPos(v.frame, HWND_TOP, sw - cx - 16, sh - cy - 32, cx, cy, SWP_SIZE | SWP_MOVE) };
    v
}

/// The footnote window of `inst` (made if there is none), over the
/// middle of the help window.
fn ensure_footnote(inst: &mut Instance) -> &'static mut View {
    if inst.footnote != 0 {
        if let Some(v) = view(unsafe { WinWindowFromID(inst.footnote, FID_CLIENT) }) {
            return v;
        }
    }
    let v = make(inst, true, HWND_DESKTOP);
    inst.footnote = v.frame;
    let mut swp = Swp::default();
    unsafe { WinQueryWindowPos(inst.frame, &mut swp) };
    let (cx, cy) = ((swp.cx * 3 / 4).max(240), (swp.cy * 2 / 5).max(120));
    unsafe { WinSetWindowPos(v.frame, HWND_TOP, swp.x + (swp.cx - cx) / 2, swp.y + (swp.cy - cy) / 2, cx, cy, SWP_SIZE | SWP_MOVE) };
    v
}

/// A help or footnote window, or (in `parent`, a help window's client) a
/// viewport.
fn make(inst: &mut Instance, footnote: bool, parent: u32) -> &'static mut View {
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_NOBYTEALIGN;
    if parent != HWND_DESKTOP {
        fcf |= FCF_MAXBUTTON;
    } else if !footnote {
        fcf |= FCF_MINBUTTON | FCF_MAXBUTTON | FCF_TASKLIST;
    }
    let mut client = 0u32;
    let title: &[u8] = if footnote { b"Footnote\0" } else { b"Help\0" };
    let frame = unsafe {
        WinCreateStdWindow(parent, WS_CLIPSIBLINGS, &mut fcf, CLIENT.as_ptr(), title.as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client)
    };
    let v = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(View {
        inst: inst as *mut Instance,
        frame,
        client,
        panel: 0,
        scroll: 0,
        list: 0,
        entry: 0,
        mode: Mode::Contents,
        history: Vec::new(),
        page: None,
        page_width: 0,
        top: 0,
        line: 16,
        sel: None,
        rows: Vec::new(),
        bitmaps: Bitmaps::new(),
        footnote,
        vp: None,
        acvps: Vec::new(),
        fonts: Vec::new(),
        ddf: Vec::new(),
    }));
    unsafe { WinSetWindowULong(client, QWL_USER, v as u32) };
    let v = unsafe { &mut *v };
    v.panel = child(client, PANEL, b"", WS_VISIBLE | WS_TABSTOP, ID_PANEL);
    unsafe { WinSetWindowULong(v.panel, QWL_USER, v as *mut View as u32) };
    // Scroll bars and lists are #8 and #7, entry fields #6, buttons #3.
    v.scroll = child(client, b"#8\0", b"", WS_VISIBLE | 1, ID_SCROLL);
    if !footnote && parent == HWND_DESKTOP {
        v.list = child(client, b"#7\0", b"", WS_TABSTOP, ID_LIST);
        v.entry = child(client, b"#6\0", b"", WS_TABSTOP | ES_AUTOSCROLL | ES_MARGIN, ID_ENTRY);
        send(v.entry, EM_SETTEXTLIMIT, 128, 0);
        for (id, text) in BUTTONS {
            child(client, b"#3\0", &text[..text.len() - 1], WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON | BS_NOPOINTERFOCUS, id);
        }
    }
    v
}

/// Show and activate the window.
fn activate(v: &View) {
    activate_now(v);
    // Again once the current message is done: a request from an
    // application's menu ends menu mode and reactivates its frame after
    // the help manager has shown the window.
    post(v.client, WM_ACTIVATE_LATER, 0, 0);
}

/// Posted by activate().
const WM_ACTIVATE_LATER: u32 = 0x1000 + 0x31;

fn activate_now(v: &View) {
    unsafe {
        WinSetWindowPos(v.frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, if matches!(v.mode, Mode::Panel(_)) { v.panel } else if matches!(v.mode, Mode::Search(_)) { v.entry } else { v.list });
    }
}

fn notify(inst: &Instance, code: u32) {
    if inst.app_window() != 0 && !inst.book {
        send(inst.app_window(), crate::HM_NOTIFY, code, inst.frame);
    }
}

// ---- Panels ----

/// Show panel `p`, remembering the current view for Previous.
pub fn show_panel(inst: &mut Instance, p: PanelRef, remember: bool) {
    let v = ensure(inst);
    go(v, Mode::Panel(p), remember);
    notify(inst, crate::OPEN_PAGE);
}

pub fn show_contents(inst: &mut Instance) {
    let v = ensure(inst);
    go(v, Mode::Contents, true);
    notify(inst, crate::OPEN_TOC);
}

pub fn show_index(inst: &mut Instance) {
    let v = ensure(inst);
    go(v, Mode::Index, true);
    notify(inst, crate::OPEN_INDEX);
}

/// Show footnote `p` in the footnote window.
fn show_footnote(inst: &mut Instance, p: PanelRef) {
    let v = ensure_footnote(inst);
    v.mode = Mode::Panel(p);
    v.top = 0;
    enter(v);
    activate(v);
}

fn close_footnote(inst: &mut Instance) {
    if inst.footnote != 0 {
        let f = inst.footnote;
        inst.footnote = 0;
        unsafe { WinDestroyWindow(f) };
    }
}

pub fn close(inst: &mut Instance) {
    close_footnote(inst);
    if inst.frame != 0 {
        let f = inst.frame;
        inst.frame = 0;
        unsafe { WinDestroyWindow(f) };
    }
}

fn go(v: &mut View, mode: Mode, remember: bool) {
    let first = v.page.is_none() && v.rows.is_empty() && v.history.is_empty();
    if remember && !first && v.mode != mode {
        v.history.push((v.mode.clone(), v.top));
    }
    v.mode = mode;
    v.top = 0;
    enter(v);
    activate(v);
    open_auto(v);
}

/// Set the window up for its mode.
fn enter(v: &mut View) {
    v.sel = None;
    v.page = None;
    v.ddf.clear();
    // Another panel: the viewports it opened go (all of them for the
    // help window's own panel, as IBM's PnlClearAllViewport; else the
    // dependent ones, PnlClearDependentViewport).
    if !v.footnote {
        close_viewports(v.inst(), v.frame, v.vp.is_some());
    }
    for f in core::mem::take(&mut v.acvps) {
        let inst = v.inst();
        if inst.objcom == f {
            inst.objcom = 0;
        }
        unsafe { WinDestroyWindow(f) };
    }
    // The panel's place first: it is laid out for its width.
    place(v);
    match v.mode.clone() {
        Mode::Panel(_) => {
            relayout(v);
        }
        Mode::Contents => fill_contents(v),
        Mode::Index => fill_index(v),
        Mode::Search(q) => fill_search(v, &q),
    }
    place(v);
    if v.footnote || v.vp.is_some() {
        let t = z(&panel_title(v.inst(), &v.mode));
        let t = if t.len() > 1 { t } else { z(b"Footnote") };
        unsafe { WinSetWindowText(v.frame, t.as_ptr()) };
    } else {
        set_title(v.inst());
    }
    unsafe { WinInvalidateRect(v.panel, core::ptr::null(), 0) };
}

fn panel_title(inst: &Instance, m: &Mode) -> Vec<u8> {
    match m {
        Mode::Panel(p) => inst.libs.get(p.lib).and_then(|l| l.doc().toc(p.toc).ok()).map(|e| e.title).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn back(v: &mut View) {
    if let Some((m, top)) = v.history.pop() {
        v.mode = m;
        enter(v);
        v.top = top;
        clamp(v);
        activate(v);
        open_auto(v);
    } else if v.vp.is_some() {
        // A viewport with nothing to go back to closes.
        unsafe { WinDestroyWindow(v.frame) };
    }
}

/// Esc: back to the previous panel; with nothing to go back to, the
/// window closes.
fn escape(v: &mut View) {
    if v.history.is_empty() && v.vp.is_none() {
        post(v.frame, WM_CLOSE, 0, 0);
    } else {
        back(v);
    }
}

/// The help window's title: the instance's title and what is shown.
pub fn set_title(inst: &mut Instance) {
    let Some(v) = view_of(inst) else { return };
    let mut t: Vec<u8> = Vec::new();
    let what: Vec<u8> = match &v.mode {
        Mode::Panel(p) => {
            let mut s = inst.libs.get(p.lib).and_then(|l| l.doc().toc(p.toc).ok()).map(|e| e.title).unwrap_or_default();
            if inst.show_ids {
                if let Some(r) = inst.libs[p.lib].doc().res_of_panel(p.toc) {
                    s.extend_from_slice(b" (");
                    s.extend_from_slice(&dec(r as u32));
                    s.push(b')');
                }
            }
            s
        }
        Mode::Contents => b"Contents".to_vec(),
        Mode::Index => b"Index".to_vec(),
        Mode::Search(_) => b"Search".to_vec(),
    };
    let title = if inst.title.is_empty() { inst.libs.first().map(|l| l.doc().title().to_vec()).unwrap_or_default() } else { inst.title.clone() };
    if !title.is_empty() {
        t.extend_from_slice(&title);
        if !what.is_empty() {
            t.extend_from_slice(b" - ");
        }
    }
    t.extend_from_slice(&what);
    let t = z(&t);
    unsafe { WinSetWindowText(v.frame, t.as_ptr()) };
}

fn dec(mut n: u32) -> Vec<u8> {
    let mut v = Vec::new();
    loop {
        v.insert(0, b'0' + (n % 10) as u8);
        n /= 10;
        if n == 0 {
            return v;
        }
    }
}

/// Lay the current panel out for the panel window's width.
fn relayout(v: &mut View) {
    let Mode::Panel(p) = v.mode else { return };
    let inst = v.inst();
    let Some(lib) = inst.libs.get(p.lib) else { return };
    let w = window_rect(v.panel).r.max(64);
    let doc = lib.doc();
    let Ok(entry) = doc.toc(p.toc) else { return };
    let mut cells: Vec<Vec<Item<'static>>> = entry.cells.iter().filter_map(|&c| doc.cell(c).ok()).map(|c| decode(doc, &c)).collect();
    // The page (and the window's text) is laid out again before the
    // dynamic data it borrows is replaced.
    v.page = None;
    let ddf = &mut v.ddf;
    let (panel, objcom, active) = (v.panel, inst.objcom, inst.app_window());
    expand_ddf(&mut cells, |res| {
        let i = match ddf.iter().position(|d| d.0 == res) {
            Some(i) if ddf[i].2 => i,
            known => {
                let data = query_ddf(panel, if objcom != 0 { objcom } else { active }, res);
                match known {
                    Some(i) => ddf[i] = (res, data, true),
                    None => ddf.push((res, data, true)),
                }
                known.unwrap_or(ddf.len() - 1)
            }
        };
        let b: &[u8] = &ddf[i].1;
        Some(unsafe { core::slice::from_raw_parts(b.as_ptr(), b.len()) })
    });
    let hps = unsafe { WinGetPS(v.panel) };
    let mut g = Gpi::new(hps, crate::render::doc_fonts(doc));
    let keys = keys();
    let page = layout_items(doc, &cells, &Options { width: w, keys: &keys }, &mut g);
    v.line = ipf::layout::Metrics::system(&mut g).height.max(8);
    v.fonts = core::mem::take(&mut g.fonts);
    drop(g);
    unsafe { WinReleasePS(hps) };
    v.page = Some(page);
    v.page_width = w;
    clamp(v);
    set_scroll(v);
}

/// The dynamic data of `:ddf res=` for panel window `panel`, asked of
/// the application (HM_QUERY_DDF_DATA; IBM's PnlGethDDF): its DDF's
/// text, the handle freed.
fn query_ddf(panel: u32, app: u32, res: u16) -> alloc::boxed::Box<[u8]> {
    if app == 0 {
        return alloc::boxed::Box::new([]);
    }
    let h = send(app, crate::HM_QUERY_DDF_DATA, panel, res as u32);
    let Some(d) = crate::ddf::get(h) else {
        post(app, crate::HM_ERROR, crate::HMERR_DDF_INVALID, 0);
        return alloc::boxed::Box::new([]);
    };
    let data = core::mem::take(&mut d.buf).into_boxed_slice();
    crate::ddf::free(h);
    data
}

/// Dynamic data of the panel shown changed (HM_INVALIDATE_DDF_DATA):
/// `ids` (empty: all) are asked for again.
pub fn invalidate_ddf(inst: &mut Instance, ids: &[u16]) {
    let Some(v) = view_of(inst) else { return };
    let mut any = false;
    for d in v.ddf.iter_mut() {
        if ids.is_empty() || ids.contains(&d.0) {
            d.2 = false;
            any = true;
        }
    }
    if any {
        relayout(v);
        unsafe { WinInvalidateRect(v.panel, core::ptr::null(), 0) };
    }
}

/// IPF_KEYS, which `:hide.` text needs.
fn keys() -> Vec<u8> {
    let mut p: *const u8 = core::ptr::null();
    if unsafe { os2::dos::DosScanEnv(b"IPF_KEYS\0".as_ptr(), &mut p) } != 0 {
        return Vec::new();
    }
    unsafe { cstr(p) }.to_vec()
}

fn visible_height(v: &View) -> i32 {
    window_rect(v.panel).t
}

fn clamp(v: &mut View) {
    let total = v.page.as_ref().map_or(0, |p| p.height + v.line);
    let max = (total - visible_height(v)).max(0);
    v.top = v.top.clamp(0, max);
}

fn set_scroll(v: &View) {
    let total = v.page.as_ref().map_or(0, |p| p.height + v.line);
    let vis = visible_height(v).max(1);
    let line = v.line.max(1);
    let max = ((total - vis).max(0) + line - 1) / line;
    send(v.scroll, SBM_SETSCROLLBAR, (v.top / line) as u32, (max as u32) << 16);
    send(v.scroll, SBM_SETTHUMBSIZE, ((vis / line) as u32) | (((total / line) as u32) << 16), 0);
}

fn scroll_to(v: &mut View, top: i32) {
    let old = v.top;
    v.top = top;
    clamp(v);
    if v.top != old {
        set_scroll(v);
        unsafe { WinInvalidateRect(v.panel, core::ptr::null(), 0) };
    }
}

// ---- Lists ----

fn list_add(v: &mut View, text: &[u8], p: PanelRef) {
    let t = z(text);
    send(v.list, LM_INSERTITEM, LIT_END, t.as_ptr() as u32);
    v.rows.push(p);
}

fn list_start(v: &mut View) {
    send(v.list, LM_DELETEALL, 0, 0);
    v.rows.clear();
}

fn fill_contents(v: &mut View) {
    list_start(v);
    let inst = v.inst();
    for (lib, l) in inst.libs.iter().enumerate() {
        let d = l.doc();
        for t in 0..d.header.toc_count {
            let Ok(e) = d.toc(t) else { continue };
            if e.hidden() || e.level() == 0 {
                continue;
            }
            let mut row = vec![b' '; (e.level() as usize - 1) * 3];
            row.extend_from_slice(&e.title);
            list_add(v, &row, PanelRef { lib, toc: t });
        }
        // A help library's contents are its own; books go on.
        if !inst.book {
            break;
        }
    }
    send(v.list, LM_SELECTITEM, 0, 1);
}

fn fill_index(v: &mut View) {
    list_start(v);
    let inst = v.inst();
    for (lib, l) in inst.libs.iter().enumerate().filter(|(i, _)| Some(*i) != inst.hmhelp) {
        for e in l.doc().index() {
            let mut row = if e.level() == 2 { b"   ".to_vec() } else { Vec::new() };
            row.extend_from_slice(&e.text);
            list_add(v, &row, PanelRef { lib, toc: e.toc });
        }
    }
    send(v.list, LM_SELECTITEM, 0, 1);
}

/// The panels holding every word of `q`.
fn fill_search(v: &mut View, q: &[u8]) {
    list_start(v);
    let words: Vec<&[u8]> = q.split(|&c| c == b' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return;
    }
    let inst = v.inst();
    for (lib, l) in inst.libs.iter().enumerate().filter(|(i, _)| Some(*i) != inst.hmhelp) {
        let d = l.doc();
        let mut hits: Option<Vec<u16>> = None;
        for w in &words {
            let found = d.find_word(w).and_then(|n| d.panels_with_word(n)).unwrap_or_default();
            hits = Some(match hits {
                None => found,
                Some(h) => h.into_iter().filter(|t| found.contains(t)).collect(),
            });
        }
        for t in hits.unwrap_or_default() {
            let Ok(e) = d.toc(t) else { continue };
            if e.level() == 0 || e.title.is_empty() {
                continue;
            }
            list_add(v, &e.title, PanelRef { lib, toc: t });
        }
    }
    if !v.rows.is_empty() {
        send(v.list, LM_SELECTITEM, 0, 1);
    }
}

fn open_selected(v: &mut View) {
    let i = send(v.list, LM_QUERYSELECTION, LIT_FIRST, 0);
    if let Some(&p) = v.rows.get(i as usize) {
        // A panel whose heading gives it a window of its own opens in a
        // viewport over the list.
        let inst = v.inst();
        let spec = Spec::of(inst, p, None);
        if spec.own_window(inst) {
            close_viewports(inst, v.frame, false);
            open_viewport(inst, v.frame, p, spec, false);
        } else {
            go(v, Mode::Panel(p), true);
        }
    }
}

/// The panel whose title (else an index entry) matches `topic`.
pub fn find_topic(inst: &Instance, topic: &[u8]) -> Option<PanelRef> {
    let has = |t: &[u8]| t.len() >= topic.len() && t.windows(topic.len()).any(|w| w.eq_ignore_ascii_case(topic));
    for (lib, l) in inst.libs.iter().enumerate() {
        let d = l.doc();
        for t in 0..d.header.toc_count {
            if d.toc(t).is_ok_and(|e| e.level() > 0 && has(&e.title)) {
                return Some(PanelRef { lib, toc: t });
            }
        }
    }
    for (lib, l) in inst.libs.iter().enumerate() {
        if let Some(e) = l.doc().index().into_iter().find(|e| has(&e.text)) {
            return Some(PanelRef { lib, toc: e.toc });
        }
    }
    None
}

// ---- Layout of the client ----

fn place(v: &View) {
    let r = window_rect(v.client);
    let (w, h) = (r.r, r.t);
    let sb = sysval(SV_CXVSCROLL).max(12);
    let row = if v.footnote || v.vp.is_some() { 0 } else { BUTTON_ROW };
    let top = (h - row).max(0);
    let panel_mode = matches!(v.mode, Mode::Panel(_));
    let search = matches!(v.mode, Mode::Search(_));
    // The help window's own panel goes where its heading's window data
    // puts it (IBM shows it in a viewport of the help window).
    let (px, py, pw, ph) = match v.mode {
        Mode::Panel(p) if v.vp.is_none() && !v.footnote => {
            let spec = Spec::of(v.inst(), p, None);
            if spec.size.is_some() { spec_rect(&spec, w, top) } else { (0, 0, w, top) }
        }
        _ => (0, 0, w, top),
    };
    unsafe {
        WinSetWindowPos(v.panel, 0, px, row + py, (pw - sb).max(0), ph, SWP_MOVE | SWP_SIZE);
        WinSetWindowPos(v.scroll, 0, px + pw - sb, row + py, sb, ph, SWP_MOVE | SWP_SIZE);
        let ltop = if search { (top - ENTRY_ROW).max(0) } else { top };
        WinSetWindowPos(v.list, 0, 0, BUTTON_ROW, w, ltop, SWP_MOVE | SWP_SIZE);
        WinSetWindowPos(v.entry, 0, 6, BUTTON_ROW + ltop + 4, (w - 12).max(0), ENTRY_ROW - 8, SWP_MOVE | SWP_SIZE);
        WinShowWindow(v.panel, panel_mode as u32);
        WinShowWindow(v.scroll, panel_mode as u32);
        WinShowWindow(v.list, (!panel_mode) as u32);
        WinShowWindow(v.entry, search as u32);
        let mut x = 6;
        for (id, _) in BUTTONS {
            WinSetWindowPos(WinWindowFromID(v.client, id), 0, x, 4, 100, BUTTON_ROW - 8, SWP_MOVE | SWP_SIZE);
            x += 106;
        }
        WinEnableWindow(WinWindowFromID(v.client, ID_PREVIOUS), (!v.history.is_empty()) as u32);
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    crate::stack::deep(|| client_proc_(h, msg, mp1, mp2))
}

fn client_proc_(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(v) = view(h) else { return unsafe { WinDefWindowProc(h, msg, mp1, mp2) } };
    match msg {
        WM_SIZE => {
            place(v);
            if v.vp.is_none() && !v.footnote {
                place_viewports(v.inst());
            }
            if matches!(v.mode, Mode::Panel(_)) && window_rect(v.panel).r != v.page_width {
                relayout(v);
            } else {
                clamp(v);
                set_scroll(v);
            }
            0
        }
        WM_PAINT => {
            let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
            let r = window_rect(h);
            unsafe {
                WinFillRect(hps, &r, SYSCLR_HELPBACKGROUND);
                WinEndPaint(hps);
            }
            0
        }
        WM_ACTIVATE_LATER => {
            activate_now(v);
            0
        }
        // The scroll bar's notifications go to its owner, the client:
        // the panel window scrolls.
        WM_VSCROLL => send(v.panel, msg, mp1, mp2),
        WM_COMMAND => {
            match lo(mp1) {
                ID_PREVIOUS => back(v),
                ID_SEARCH => {
                    let q = match &v.mode {
                        Mode::Search(q) => q.clone(),
                        _ => Vec::new(),
                    };
                    go(v, Mode::Search(q), true);
                }
                ID_INDEX => go(v, Mode::Index, true),
                ID_CONTENTS => go(v, Mode::Contents, true),
                _ => {}
            }
            0
        }
        WM_CONTROL => {
            if lo(mp1) == ID_LIST && hi(mp1) == LN_ENTER {
                open_selected(v);
            }
            0
        }
        // Keys the list and entry field do not use.
        WM_CHAR => {
            if mp1 & KC_KEYUP != 0 || mp1 & KC_VIRTUALKEY == 0 {
                return 0;
            }
            match hi(mp2) {
                VK_ESC => escape(v),
                VK_NEWLINE | VK_ENTER => {
                    if matches!(v.mode, Mode::Search(_)) && unsafe { WinQueryFocus(HWND_DESKTOP) } == v.entry {
                        let mut buf = [0u8; 130];
                        let n = unsafe { WinQueryWindowText(v.entry, 129, buf.as_mut_ptr()) };
                        v.mode = Mode::Search(buf[..n.max(0) as usize].to_vec());
                        enter(v);
                        unsafe { WinSetFocus(HWND_DESKTOP, if v.rows.is_empty() { v.entry } else { v.list }) };
                    } else if !matches!(v.mode, Mode::Panel(_)) {
                        open_selected(v);
                    }
                }
                _ => return 0,
            }
            1
        }
        WM_CLOSE => {
            let inst = v.inst();
            if v.footnote {
                close_footnote(inst);
                return 0;
            }
            if v.vp.is_some() {
                unsafe { WinDestroyWindow(v.frame) };
                return 0;
            }
            let book = inst.book;
            close(inst);
            if inst.owner != 0 {
                post(inst.owner, crate::HM_BOOK_CLOSED, inst.hwnd, 0);
            } else if book {
                post(inst.hwnd, WM_QUIT, 0, 0);
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, 0);
                WinSetWindowULong(v.panel, QWL_USER, 0);
                let inst = v.inst();
                if inst.frame == v.frame {
                    inst.frame = 0;
                }
                if inst.footnote == v.frame {
                    inst.footnote = 0;
                }
                inst.viewports.retain(|&f| f != v.frame);
                drop(alloc::boxed::Box::from_raw(v as *mut View));
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

// ---- The panel window ----

extern "C" fn panel_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    crate::stack::deep(|| panel_proc_(h, msg, mp1, mp2))
}

fn panel_proc_(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let Some(v) = view(h) else { return unsafe { WinDefWindowProc(h, msg, mp1, mp2) } };
    match msg {
        // Help manager messages sent to a panel window (as application-
        // controlled viewports do) go to the help instance.
        0x220..=0x24f => send(v.inst().hwnd, msg, mp1, mp2),
        WM_PAINT => {
            let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
            let r = window_rect(h);
            unsafe { WinFillRect(hps, &r, SYSCLR_HELPBACKGROUND) };
            if let (Some(page), Mode::Panel(p)) = (&v.page, &v.mode) {
                let lib = &v.inst().libs[p.lib];
                let mut d = Draw {
                    hps,
                    g: Gpi::new(hps, v.fonts.clone()),
                    page,
                    top: v.top,
                    w: r.r,
                    h: r.t,
                    sel: v.sel,
                    data: lib.doc().data(),
                    image_start: lib.doc().header.image_start,
                    lib: p.lib,
                    bitmaps: &mut v.bitmaps,
                };
                d.draw();
            }
            unsafe { WinEndPaint(hps) };
            0
        }
        WM_VSCROLL => {
            let line = v.line;
            let page = (visible_height(v) - line).max(line);
            let top = match hi(mp2) {
                SB_LINEUP => v.top - line,
                SB_LINEDOWN => v.top + line,
                SB_PAGEUP => v.top - page,
                SB_PAGEDOWN => v.top + page,
                SB_SLIDERTRACK | SB_SLIDERPOSITION => lo(mp2) as i32 * line,
                _ => return 0,
            };
            scroll_to(v, top);
            0
        }
        WM_BUTTON1DOWN => {
            unsafe { WinSetFocus(HWND_DESKTOP, h) };
            let (x, y) = (lo(mp1) as i16 as i32, hi(mp1) as i16 as i32);
            let py = visible_height(v) - y + v.top;
            if let Some(hit) = v.page.as_ref().and_then(|p| p.hit(x, py)) {
                follow(v, hit);
            }
            1
        }
        WM_CHAR => {
            if mp1 & KC_KEYUP != 0 {
                return 1;
            }
            if mp1 & KC_VIRTUALKEY == 0 {
                return unsafe { WinDefWindowProc(h, msg, mp1, mp2) };
            }
            let line = v.line;
            let page = (visible_height(v) - line).max(line);
            match hi(mp2) {
                VK_UP => scroll_to(v, v.top - line),
                VK_DOWN => scroll_to(v, v.top + line),
                VK_PAGEUP => scroll_to(v, v.top - page),
                VK_PAGEDOWN => scroll_to(v, v.top + page),
                VK_HOME => scroll_to(v, 0),
                VK_END => scroll_to(v, i32::MAX / 2),
                VK_TAB => select_link(v, 1),
                VK_BACKTAB => select_link(v, -1),
                VK_NEWLINE | VK_ENTER => {
                    if let Some(l) = v.sel {
                        follow(v, Hit::Link(l));
                    }
                }
                VK_ESC | VK_BACKSPACE if v.footnote => close_footnote(v.inst()),
                VK_ESC => escape(v),
                VK_BACKSPACE => back(v),
                _ => return unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
            }
            1
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

/// Select the next (`d` 1) or previous (-1) link, scrolling it into view.
fn select_link(v: &mut View, d: i32) {
    let Some(page) = &v.page else { return };
    // Links that show (words or bitmaps), in page order.
    let mut order: Vec<u16> = Vec::new();
    for r in &page.runs {
        if let Some(l) = r.link {
            if !order.contains(&l) {
                order.push(l);
            }
        }
    }
    if order.is_empty() {
        return;
    }
    let i = v.sel.and_then(|s| order.iter().position(|&l| l == s));
    let n = order.len() as i32;
    let next = match i {
        None => if d > 0 { 0 } else { n - 1 },
        Some(i) => (i as i32 + d).rem_euclid(n),
    };
    let l = order[next as usize];
    v.sel = Some(l);
    if let Some(&(_, t, _, b)) = page.link_boxes(l).first() {
        let vis = visible_height(v);
        if t < v.top {
            scroll_to(v, t - v.line);
        } else if b > v.top + vis {
            scroll_to(v, b - vis + v.line);
        }
    }
    unsafe { WinInvalidateRect(v.panel, core::ptr::null(), 0) };
}

/// Open a link's panel: footnotes (and panels without a contents entry)
/// in the footnote window; panels with a window of their own (the link's
/// data or the panel's TOC entry: origin, size, group) in a viewport;
/// others in the window of the link.
fn open(v: &mut View, p: PanelRef, footnote: bool, data: Option<LinkData>) {
    let inst = v.inst();
    let level0 = inst.libs.get(p.lib).and_then(|l| l.doc().toc(p.toc).ok()).is_some_and(|e| e.level() == 0);
    let spec = Spec::of(inst, p, data);
    if footnote || level0 {
        show_footnote(inst, p);
    } else if spec.own_window(inst) {
        open_viewport(inst, v.frame, p, spec, false);
    } else if v.footnote {
        show_panel(inst, p, true);
        close_footnote(inst);
    } else {
        go(v, Mode::Panel(p), true);
    }
}

/// Follow a link of the page.
fn follow(v: &mut View, hit: Hit) {
    let (Some(page), Mode::Panel(cur)) = (&v.page, v.mode.clone()) else { return };
    let target = match hit {
        Hit::Link(l) => page.links.get(l as usize).map(|l| (l.target.clone(), l.data)),
        Hit::Area(a) => page.areas.get(a).map(|(_, a)| (a.target.clone(), a.data)),
    };
    let Some((target, data)) = target else { return };
    let inst = v.inst();
    match target {
        Target::Panel(t) => open(v, PanelRef { lib: cur.lib, toc: t }, false, data),
        Target::Footnote(t) => open(v, PanelRef { lib: cur.lib, toc: t }, true, None),
        Target::Res(r) => {
            let p = inst.libs[cur.lib].doc().panel_by_res(r).map(|toc| PanelRef { lib: cur.lib, toc }).or_else(|| inst.panel_by_res(r));
            if let Some(p) = p {
                open(v, p, false, data);
            }
        }
        Target::Name(n) => {
            let p = inst.libs[cur.lib].doc().panel_by_name(n).map(|toc| PanelRef { lib: cur.lib, toc }).or_else(|| inst.panel_by_name(n));
            if let Some(p) = p {
                open(v, p, false, data);
            }
        }
        Target::Inform(r) => {
            if inst.app_window() != 0 {
                post(inst.app_window(), HM_INFORM, r as u32, 0);
            }
        }
        Target::Launch(cmd) => launch(cmd),
        Target::External { db, id } => {
            let dbs = inst.libs[cur.lib].doc().databases();
            let Some(name) = dbs.get(db as usize) else { return };
            let base = name.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(name);
            let Some(lib) = inst.libs.iter().position(|l| l.path.rsplit(|&c| c == b'\\').next().is_some_and(|p| p.eq_ignore_ascii_case(base))) else {
                return;
            };
            let d = inst.libs[lib].doc();
            let toc = if !id.is_empty() && id.iter().all(|c| c.is_ascii_digit()) {
                let n = id.iter().fold(0u32, |a, &c| a * 10 + (c - b'0') as u32);
                d.panel_by_res(n as u16)
            } else {
                d.panel_by_name(id)
            };
            if let Some(toc) = toc {
                open(v, PanelRef { lib, toc }, false, data);
            }
        }
    }
}

/// Start "program parameters" (`:link reftype=launch`).
fn launch(cmd: &[u8]) {
    let cmd = cmd.split(|&c| c == 0).next().unwrap_or(cmd);
    let mut parts = cmd.splitn(2, |&c| c == b' ');
    let prog = parts.next().unwrap_or(b"");
    let args = parts.next().unwrap_or(b"");
    if prog.is_empty() {
        return;
    }
    let mut name = prog.to_vec();
    if !name.contains(&b'.') {
        name.extend_from_slice(b".EXE");
    }
    let mut full = [0u8; 260];
    let n = z(&name);
    let path = if unsafe { DosSearchPath(2 | 1, b"PATH\0".as_ptr(), n.as_ptr(), full.as_mut_ptr(), 260) } == 0 {
        unsafe { cstr(full.as_ptr()) }.to_vec()
    } else {
        name.clone()
    };
    let mut a = path.clone();
    a.push(0);
    a.extend_from_slice(args);
    a.extend_from_slice(b"\0\0");
    let p = z(&path);
    let mut res = os2::dos::ResultCodes { code_terminate: 0, code_result: 0 };
    // EXEC_ASYNC.
    unsafe { os2::dos::DosExecPgm(core::ptr::null_mut(), 0, 1, a.as_ptr(), core::ptr::null(), &mut res, p.as_ptr()) };
}

// ---- Viewports: panels in windows of their own ----

/// Where a viewport goes and what it is: from the link's data, else the
/// panel's TOC entry (`units, x, y` each; IBM's PnlGetExtLnkData).
#[derive(Clone, Copy, Default)]
struct Spec {
    origin: Option<(u8, u16, u16)>,
    size: Option<(u8, u16, u16)>,
    group: Option<u16>,
    /// The link asks for a window of its own (0x80) or is `auto`.
    viewport: bool,
    dependent: bool,
}

impl Spec {
    fn of(inst: &Instance, p: PanelRef, data: Option<LinkData>) -> Spec {
        let e = inst.libs.get(p.lib).and_then(|l| l.doc().toc(p.toc).ok()).unwrap_or_default();
        let c = |w: ipf::WinCoord| (w.units, w.x, w.y);
        let d = data.unwrap_or_default();
        Spec {
            origin: d.origin.or(e.origin.map(c)),
            size: d.size.or(e.size.map(c)),
            group: d.group.or(e.group),
            viewport: d.flags & 0xc0 != 0,
            dependent: d.flags & 0x200 != 0,
        }
    }

    /// Does the panel open in a viewport? It has a size (a place alone
    /// keeps the window's size: the whole help window), its group's
    /// viewport is open, or the link asks for one.
    fn own_window(&self, inst: &Instance) -> bool {
        self.viewport || self.size.is_some_and(|s| s.1 != 0xffff || s.2 != 0xffff) || self.group.is_some_and(|g| find_group(inst, g).is_some())
    }
}

/// One coordinate of a viewport in pixels (IBM's PnlConvertCord): `v` in
/// `units`; `extent` the window's width or height, `size` the
/// viewport's (for origins), `unit` the character size.
fn coord(units: u8, v: u16, extent: i32, size: i32, unit: i32, is_size: bool) -> i32 {
    let v = v as i32;
    match units & 0xf {
        0 => v * unit,
        1 => (extent * v + 50) / 100,
        2 => v,
        // Points, at 96 dpi.
        3 => v * 96 / 72,
        4 if !is_size => match v {
            2 | 4 => extent - size.min(extent),
            0x10 => (extent - size.min(extent)) / 2,
            _ => 0,
        },
        _ => extent,
    }
}

/// The viewport's rectangle (x, y, cx, cy) in an area of `w` by `h`
/// (IBM's PnlConvertUnit and PnlCalcViewPortSize; a missing or 0xFFFF
/// value is the area's).
fn spec_rect(spec: &Spec, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let font = crate::render::char_size();
    let (cw, ch) = (font.0.max(1), font.1.max(1));
    let size = spec.size.unwrap_or((2, 0xffff, 0xffff));
    let origin = spec.origin.unwrap_or((2, 0xffff, 0xffff));
    let cx = if size.1 == 0xffff { w } else { coord(size.0 >> 4, size.1, w, 0, cw, true) }.clamp(16, w.max(16));
    let cy = if size.2 == 0xffff { h } else { coord(size.0, size.2, h, 0, ch, true) }.clamp(16, h.max(16));
    let x = if origin.1 == 0xffff { 0 } else { coord(origin.0 >> 4, origin.1, w, cx, cw, false) };
    let y = if origin.2 == 0xffff { 0 } else { coord(origin.0, origin.2, h, cy, ch, false) };
    let x = if x + cx > w { (w - cx).max(0) } else { x.max(0) };
    let y = if y + cy > h { (h - cy).max(0) } else { y.max(0) };
    (x, y, cx, cy)
}

/// HM_QUERY: a window of the help manager (HMQW_*).
pub fn query(inst: &Instance, what: u16, lo: u16, mp2: u32) -> u32 {
    let _ = mp2;
    match what {
        1 | 3 => inst.frame,
        5 | 7 => inst.viewports.iter().copied().find(|&f| {
            vp_view(f).is_some_and(|v| matches!(v.mode, Mode::Panel(p) if inst.libs.get(p.lib).and_then(|l| l.doc().res_of_panel(p.toc)) == Some(lo)))
        }).unwrap_or(0),
        8 => inst.objcom,
        9 => inst.hwnd,
        10 => {
            let f = unsafe { WinQueryFocus(HWND_DESKTOP) };
            inst.viewports.iter().copied().find(|&v| is_inside(f, v)).unwrap_or(inst.frame)
        }
        0xf1 => find_group(inst, lo).unwrap_or(0),
        _ => 0,
    }
}

/// The viewport showing group `g`.
fn find_group(inst: &Instance, g: u16) -> Option<u32> {
    inst.viewports.iter().copied().find(|&f| vp_view(f).is_some_and(|v| v.vp.as_ref().is_some_and(|p| p.group == Some(g))))
}

fn vp_view(frame: u32) -> Option<&'static mut View> {
    view(unsafe { WinWindowFromID(frame, FID_CLIENT) })
}

/// The help window's area for viewports (its client above the buttons).
fn vp_area(inst: &Instance) -> (i32, i32) {
    let Some(m) = view_of(inst) else { return (0, 0) };
    let r = window_rect(m.client);
    (r.r, (r.t - BUTTON_ROW).max(0))
}

/// Show panel `p` in a viewport of the help window: its group's, or a
/// new one placed by `spec`; `from` is the frame it is opened from.
fn open_viewport(inst: &mut Instance, from: u32, p: PanelRef, spec: Spec, auto: bool) {
    let main = ensure(inst);
    let parent = main.client;
    let v = match spec.group.and_then(|g| find_group(inst, g)).and_then(vp_view) {
        Some(v) => v,
        None => {
            let v = make(inst, false, parent);
            inst.viewports.push(v.frame);
            v
        }
    };
    v.vp = Some(Viewport { group: spec.group, parent: from, dependent: spec.dependent || auto, spec });
    let (w, h) = vp_area(inst);
    let (x, y, cx, cy) = spec_rect(&spec, w, h);
    unsafe { WinSetWindowPos(v.frame, HWND_TOP, x, y + BUTTON_ROW, cx, cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER) };
    v.history.clear();
    v.mode = Mode::Panel(p);
    v.top = 0;
    enter(v);
    if !auto {
        activate(v);
    }
    open_auto(v);
}

/// Viewports follow the help window's size (IBM's
/// SetChildViewportPosition).
fn place_viewports(inst: &mut Instance) {
    let (w, h) = vp_area(inst);
    for &f in inst.viewports.clone().iter() {
        if let Some(v) = vp_view(f) {
            let Some(vp) = &v.vp else { continue };
            let (x, y, cx, cy) = spec_rect(&vp.spec, w, h);
            unsafe { WinSetWindowPos(f, 0, x, y + BUTTON_ROW, cx, cy, SWP_SIZE | SWP_MOVE) };
        }
    }
}

/// Close the viewports opened from `frame` (only the dependent ones if
/// `dependent`), and theirs.
fn close_viewports(inst: &mut Instance, frame: u32, dependent: bool) {
    let main = inst.frame == frame;
    for f in inst.viewports.clone() {
        let Some(v) = vp_view(f) else { continue };
        let Some(vp) = &v.vp else { continue };
        if (main || vp.parent == frame) && (!dependent || vp.dependent) {
            unsafe { WinDestroyWindow(f) };
        }
    }
}

/// The page's automatic links (`auto`): panels open in viewports, inform
/// links tell the application, launch links start their program (IBM's
/// PnlOpenAutoViewports).
fn open_auto(v: &mut View) {
    start_acvps(v);
    let (Some(page), Mode::Panel(cur)) = (&v.page, v.mode.clone()) else { return };
    let autos: Vec<(Target<'static>, Option<LinkData>)> = page.auto.iter().map(|l| (l.target.clone(), l.data)).collect();
    let inst = v.inst();
    let from = v.frame;
    for (t, data) in autos {
        let p = match t {
            Target::Panel(t) | Target::Footnote(t) => Some(PanelRef { lib: cur.lib, toc: t }),
            Target::Res(r) => inst.libs[cur.lib].doc().panel_by_res(r).map(|toc| PanelRef { lib: cur.lib, toc }),
            Target::Name(n) => inst.libs[cur.lib].doc().panel_by_name(n).map(|toc| PanelRef { lib: cur.lib, toc }),
            Target::Inform(r) => {
                if inst.app_window() != 0 {
                    post(inst.app_window(), HM_INFORM, r as u32, from);
                }
                None
            }
            Target::Launch(cmd) => {
                launch(cmd);
                None
            }
            Target::External { .. } => None,
        };
        if let Some(p) = p {
            let mut spec = Spec::of(inst, p, data);
            spec.viewport = true;
            open_viewport(inst, from, p, spec, true);
        }
    }
}

/// A viewport's settings.
struct Viewport {
    group: Option<u16>,
    /// The frame it was opened from.
    parent: u32,
    dependent: bool,
    spec: Spec,
}

/// ACVP: what an application-controlled viewport's procedure gets.
#[repr(C)]
struct Acvp {
    cb: u32,
    hab: u32,
    hmq: u32,
    objid: u32,
    parent: u32,
    owner: u32,
    /// The procedure's frame window.
    acvp: u32,
}

/// Start the page's application-controlled viewports (IBM's
/// MscStartObject): load the DLL, call `name(PACVP, objectinfo)`, which
/// makes a frame in the panel window, and place that frame.
fn start_acvps(v: &mut View) {
    let Some(page) = &v.page else { return };
    let list: Vec<ipf::text::AcViewport<'static>> = page.acvps.clone();
    let inst = v.inst();
    for a in list {
        let mut hmod = 0u32;
        let dll = z(a.dll);
        if a.dll.is_empty() || unsafe { os2::dos::DosLoadModule(core::ptr::null_mut(), 0, dll.as_ptr(), &mut hmod) } != 0 {
            post(inst.app_window(), crate::HM_ERROR, crate::HMERR_LOAD_DLL, 0);
            continue;
        }
        let name = z(a.name);
        let mut pfn = 0usize;
        if unsafe { os2::dos::DosQueryProcAddr(hmod, 0, name.as_ptr(), &mut pfn) } != 0 || pfn == 0 {
            post(inst.app_window(), crate::HM_ERROR, crate::HMERR_LOAD_DLL, 0);
            continue;
        }
        let mut acvp = Acvp {
            cb: core::mem::size_of::<Acvp>() as u32,
            hab: inst.hab,
            hmq: unsafe { WinQueryWindowULong(v.frame, QWL_HMQ) },
            objid: a.objid as u32,
            parent: v.panel,
            owner: v.panel,
            acvp: 0,
        };
        let info = z(a.info);
        let f: extern "C" fn(*mut Acvp, *const u8) -> u32 = unsafe { core::mem::transmute(pfn) };
        f(&mut acvp, info.as_ptr());
        if acvp.acvp == 0 {
            continue;
        }
        let d = a.data.unwrap_or_default();
        let spec = Spec { origin: d.origin, size: d.size, ..Spec::default() };
        let r = window_rect(v.panel);
        let (x, y, cx, cy) = spec_rect(&spec, r.r, r.t);
        unsafe { WinSetWindowPos(acvp.acvp, HWND_TOP, x, y, cx, cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER) };
        v.acvps.push(acvp.acvp);
    }
}
