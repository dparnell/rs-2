//! WEB.EXE: WebRS2, rs2's web browser for Presentation Manager.
//!
//! ```text
//! WEB [URL]
//! ```
//!
//! A frame with a tool bar (Back, Forward, Reload, Home, the address
//! field, Go), the page and its scroll bar, and a status line. Pages are
//! fetched, parsed and styled by webcore on a worker thread; layout and
//! painting happen on the window's thread, with GPI fonts (draw.rs).
//! Images arrive afterwards (another thread) and the page is laid out
//! again once they have settled. Links are followed with mouse button 1
//! (the status line shows where a link goes); forms take text in an entry
//! field put over the input and are sent with GET or POST.
//!
//! Keys: arrows, Page Up/Down, Space, Home/End scroll; Alt+Left or
//! Backspace go back, Alt+Right forward, F5 reloads, Ctrl+L goes to the
//! address field.

mod draw;
mod pm;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use rs2libc as _;
use webcore::dom::NodeId;
use webcore::layout::{ControlKind, ImageSizes, Item, Laid};
use webcore::page::Page;
use webcore::Url;

use draw::{Bitmap, ControlState, GpiFonts, Painter};
use pm::*;

const ID_BACK: u32 = 101;
const ID_FORWARD: u32 = 102;
const ID_RELOAD: u32 = 103;
const ID_HOME: u32 = 104;
const ID_GO: u32 = 105;
const ID_URL: u32 = 106;
const ID_VIEW: u32 = 107;
const ID_SCROLL: u32 = 108;
const ID_EDIT: u32 = 109;

const WMU_LOADED: u32 = WM_USER + 1;
const WMU_STATUS: u32 = WM_USER + 2;
const WMU_IMAGE: u32 = WM_USER + 3;
const WMU_IMAGES_DONE: u32 = WM_USER + 4;

const TIMER_RELAYOUT: u32 = 1;

const TOOLBAR_H: i32 = 34;
const STATUS_H: i32 = 22;
const HOME: &str = "about:home";

struct App {
    hab: u32,
    frame: u32,
    client: u32,
    view: u32,
    url_field: u32,
    scrollbar: u32,
    buttons: [u32; 5],
    hps: u32,
    fonts: Option<GpiFonts>,
    page: Option<Page>,
    laid: Option<Laid>,
    images: HashMap<NodeId, Bitmap>,
    image_sizes: ImageSizes,
    controls: HashMap<NodeId, ControlState>,
    /// The entry field over a text input being edited.
    editing: Option<(NodeId, u32)>,
    scroll: i32,
    view_w: i32,
    view_h: i32,
    history: Vec<Url>,
    hist_pos: usize,
    status: String,
    loading: bool,
    hover: Option<usize>,
    relayout_pending: bool,
    pending_fragment: Option<String>,
    /// The link Tab has moved to (a link number).
    focus_link: Option<usize>,
    scroll_scale: i32,
}

static GENERATION: AtomicU32 = AtomicU32::new(0);
static RETRY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    #[allow(static_mut_refs)]
    unsafe {
        APP.as_mut().unwrap()
    }
}

/// What the worker thread hands back.
enum Loaded {
    Page(Box<Page>),
    Failed(Url, String),
}

fn main() {
    // Panics are reported in the status line, not on a console.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}");
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open("WEB.LOG");
        if let Ok(f) = f.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "{msg}");
        }
    }));
    // -retry: retry failed loads for a minute (the network may still be
    // coming up when WEB is the first program, as in tests).
    let args: Vec<String> = std::env::args().skip(1).collect();
    RETRY.store(args.iter().any(|a| a.eq_ignore_ascii_case("-retry")), Ordering::Relaxed);
    let start = args.iter().find(|a| !a.starts_with('-')).and_then(|a| webcore::parse_user_url(a)).unwrap_or_else(|| Url::parse(HOME).unwrap());
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if hmq == 0 {
        std::process::exit(1);
    }
    unsafe {
        APP = Some(App {
            hab,
            frame: 0,
            client: 0,
            view: 0,
            url_field: 0,
            scrollbar: 0,
            buttons: [0; 5],
            hps: 0,
            fonts: None,
            page: None,
            laid: None,
            images: HashMap::new(),
            image_sizes: HashMap::new(),
            controls: HashMap::new(),
            editing: None,
            scroll: 0,
            view_w: 0,
            view_h: 0,
            history: Vec::new(),
            hist_pos: 0,
            status: String::new(),
            loading: false,
            hover: None,
            relayout_pending: false,
            pending_fragment: None,
            focus_link: None,
            scroll_scale: 1,
        });
    }
    let a = app();
    let class = b"WebRS2\0";
    let view_class = b"WebRS2.view\0";
    unsafe {
        WinRegisterClass(hab, class.as_ptr(), client_proc, CS_SIZEREDRAW, 0);
        WinRegisterClass(hab, view_class.as_ptr(), view_proc, 0, 0);
    }
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_SHELLPOSITION | FCF_NOBYTEALIGN;
    let title = z("WebRS2");
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, class.as_ptr(), title.as_ptr(), WS_CLIPCHILDREN, 0, 1, &mut client) };
    if frame == 0 {
        std::process::exit(1);
    }
    a.frame = frame;
    a.client = client;
    let icon = unsafe { WinLoadPointer(HWND_DESKTOP, 0, 1) };
    if icon != 0 {
        send(frame, WM_SETICON, icon, 0);
    }
    // Most of the screen.
    let (sw, sh) = unsafe { (WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN), WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN)) };
    let (w, h) = ((sw * 9 / 10).min(1100), (sh * 9 / 10).min(900));
    unsafe { WinSetWindowPos(frame, HWND_TOP, (sw - w) / 2, (sh - h) / 2, w, h, SWP_MOVE | SWP_SIZE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
    navigate(start, true, None);
    let mut msg = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut msg, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &msg) };
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    std::process::exit(0);
}

fn create_children(client: u32) {
    let a = app();
    let btn = |label: &str, id: u32| -> u32 {
        let t = z(label);
        unsafe { WinCreateWindow(client, WC | WC_BUTTON as u32, t.as_ptr(), WS_VISIBLE | BS_PUSHBUTTON | BS_NOPOINTERFOCUS, 0, 0, 10, 10, client, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
    };
    a.buttons = [btn("<", ID_BACK), btn(">", ID_FORWARD), btn("Reload", ID_RELOAD), btn("Home", ID_HOME), btn("Go", ID_GO)];
    let empty = z("");
    a.url_field = unsafe {
        WinCreateWindow(client, WC | WC_ENTRYFIELD as u32, empty.as_ptr(), WS_VISIBLE | ES_MARGIN | ES_AUTOSCROLL, 0, 0, 10, 10, client, HWND_TOP, ID_URL, core::ptr::null(), core::ptr::null())
    };
    send(a.url_field, EM_SETTEXTLIMIT, 2000, 0);
    URL_FIELD_PROC.store(unsafe { WinSubclassWindow(a.url_field, url_field_proc) }, Ordering::Relaxed);
    let vc = b"WebRS2.view\0";
    a.view = unsafe { WinCreateWindow(client, vc.as_ptr() as u32, empty.as_ptr(), WS_VISIBLE | WS_CLIPCHILDREN, 0, 0, 10, 10, client, HWND_TOP, ID_VIEW, core::ptr::null(), core::ptr::null()) };
    a.scrollbar = unsafe { WinCreateWindow(client, WC | WC_SCROLLBAR as u32, empty.as_ptr(), WS_VISIBLE | SBS_VERT, 0, 0, 10, 10, client, HWND_TOP, ID_SCROLL, core::ptr::null(), core::ptr::null()) };
    let hdc = unsafe { WinOpenWindowDC(a.view) };
    a.hps = unsafe { GpiCreatePS(a.hab, hdc, &SizeL { cx: 0, cy: 0 }, PU_PELS | GPIF_DEFAULT | GPIT_NORMAL | GPIA_ASSOC) };
    unsafe { GpiCreateLogColorTable(a.hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
    a.fonts = Some(GpiFonts::new(a.hps));
}

static URL_FIELD_PROC: AtomicU32 = AtomicU32::new(0);
static EDIT_PROC: AtomicU32 = AtomicU32::new(0);

fn call_old(old: u32, h: u32, m: u32, a: u32, b: u32) -> u32 {
    if old == 0 {
        return unsafe { WinDefWindowProc(h, m, a, b) };
    }
    let f: WndProc = unsafe { std::mem::transmute(old as usize) };
    f(h, m, a, b)
}

fn is_enter(m: u32, mp1: u32, mp2: u32) -> bool {
    m == WM_CHAR && lo(mp1) & KC_KEYUP == 0 && lo(mp1) & KC_VIRTUALKEY != 0 && matches!(hi(mp2) as u16, VK_ENTER | VK_NEWLINE)
}

extern "C" fn url_field_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if is_enter(m, mp1, mp2) {
        post(app().client, WM_COMMAND, ID_GO, 0);
        return 1;
    }
    call_old(URL_FIELD_PROC.load(Ordering::Relaxed), h, m, mp1, mp2)
}

extern "C" fn edit_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if is_enter(m, mp1, mp2) {
        let a = app();
        if let Some((node, _)) = a.editing {
            finish_edit();
            submit(node);
        }
        return 1;
    }
    if m == WM_CHAR && lo(mp1) & KC_KEYUP == 0 && lo(mp1) & KC_VIRTUALKEY != 0 && hi(mp2) as u16 == VK_ESC {
        finish_edit();
        return 1;
    }
    call_old(EDIT_PROC.load(Ordering::Relaxed), h, m, mp1, mp2)
}

fn layout_children() {
    let a = app();
    let r = rect(a.client);
    let (w, h) = (r.r, r.t);
    let top = h - TOOLBAR_H;
    let bh = TOOLBAR_H - 8;
    let mut x = 4;
    for (i, &b) in a.buttons[..4].iter().enumerate() {
        let bw = if i < 2 { 30 } else { 64 };
        move_window(b, x, top + 4, bw, bh);
        x += bw + 4;
    }
    let go_w = 40;
    move_window(a.url_field, x + 4, top + 6, w - x - go_w - 16, bh - 4);
    move_window(a.buttons[4], w - go_w - 4, top + 4, go_w, bh);
    let sbw = unsafe { WinQuerySysValue(HWND_DESKTOP, SV_CXVSCROLL) };
    let vh = top - STATUS_H;
    move_window(a.view, 0, STATUS_H, w - sbw, vh);
    move_window(a.scrollbar, w - sbw, STATUS_H, sbw, vh);
    let new_w = w - sbw;
    let resized = new_w != a.view_w;
    a.view_w = new_w;
    a.view_h = vh;
    if resized && a.page.is_some() {
        relayout(true);
    } else {
        update_scrollbar();
    }
}

fn set_status(s: &str) {
    let a = app();
    a.status = s.to_string();
    let r = RectL::new(0, 0, rect(a.client).r, STATUS_H);
    unsafe { WinInvalidateRect(a.client, &r, 0) };
}

/// Go to a URL (`record`: add it to the history), with a form body for POST.
fn navigate(url: Url, record: bool, body: Option<(String, Vec<u8>)>) {
    let a = app();
    finish_edit();
    // A fragment of the page shown: just scroll.
    if body.is_none() {
        if let (Some(p), Some(frag)) = (&a.page, url.fragment()) {
            let mut here = p.url.clone();
            here.set_fragment(None);
            let mut there = url.clone();
            there.set_fragment(None);
            if here == there {
                scroll_to_fragment(frag);
                if record {
                    push_history(url.clone());
                }
                set_text(a.url_field, url.as_str());
                return;
            }
        }
    }
    if record {
        push_history(url.clone());
    }
    set_text(a.url_field, url.as_str());
    a.loading = true;
    a.pending_fragment = url.fragment().map(String::from);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    set_status(&format!("Loading {url}..."));
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_WAIT, 0)) };
    let client = a.client;
    let size = (a.view_w.max(320) as f32, a.view_h.max(200) as f32);
    std::thread::spawn(move || {
        let progress = |s: &str| {
            if GENERATION.load(Ordering::SeqCst) == generation {
                let b = Box::new(s.to_string());
                post(client, WMU_STATUS, Box::into_raw(b) as u32, generation);
            }
        };
        let t0 = std::time::Instant::now();
        let r = loop {
            // A bug in the engine must not take the browser with it.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &body {
                None => webcore::page::load(&url, size, &progress),
                Some((ctype, data)) => webcore::page::load_post(&url, ctype, data, size, &progress),
            }))
            .unwrap_or_else(|_| Err(webcore::http::Error::Protocol("WebRS2 failed on this page (a panic)".into())));
            if r.is_err() && RETRY.load(Ordering::Relaxed) && t0.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            break r;
        };
        let l = match r {
            Ok(p) => Loaded::Page(Box::new(p)),
            Err(e) => Loaded::Failed(url.clone(), e.to_string()),
        };
        post(client, WMU_LOADED, Box::into_raw(Box::new(l)) as u32, generation);
    });
}

fn push_history(u: Url) {
    let a = app();
    if a.history.get(a.hist_pos).is_some_and(|h| *h == u) {
        return;
    }
    if !a.history.is_empty() {
        a.history.truncate(a.hist_pos + 1);
    }
    a.history.push(u);
    a.hist_pos = a.history.len() - 1;
}

fn loaded(l: Loaded) {
    let a = app();
    a.loading = false;
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_ARROW, 0)) };
    let page = match l {
        Loaded::Page(p) => *p,
        Loaded::Failed(url, e) => {
            let html = format!(
                "<html><head><title>Cannot open the page</title></head><body><h1>Cannot open the page</h1><p>{}</p><p><b>{}</b></p><p><a href=\"{}\">Try again</a></p></body></html>",
                webcore::http::escape(url.as_str()),
                webcore::http::escape(&e),
                webcore::http::escape(url.as_str())
            );
            let dom = webcore::dom::parse(&html);
            webcore::page::from_dom(url, dom, (a.view_w as f32, a.view_h as f32), &|_| {})
        }
    };
    let title = format!("{} - WebRS2", page.title);
    set_text(a.frame, &title);
    if page.url.scheme() != "about" {
        set_text(a.url_field, page.url.as_str());
    }
    // The history keeps the address the page really came from.
    if let Some(h) = a.history.get_mut(a.hist_pos) {
        *h = page.url.clone();
    }
    a.images.clear();
    a.image_sizes.clear();
    a.controls.clear();
    a.focus_link = None;
    init_controls(&page);
    let images = page.images.clone();
    a.page = Some(page);
    a.scroll = 0;
    relayout(false);
    if let Some(f) = a.pending_fragment.take() {
        scroll_to_fragment(&f);
    }
    invalidate(a.view);
    if images.is_empty() {
        set_status("Done");
        return;
    }
    set_status(&format!("Done; loading {} images...", images.len()));
    let generation = GENERATION.load(Ordering::SeqCst);
    let client = a.client;
    std::thread::spawn(move || {
        let mut seen: HashMap<Url, Option<std::sync::Arc<webcore::image::Image>>> = HashMap::new();
        for (node, url) in images {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let img = seen
                .entry(url.clone())
                .or_insert_with(|| {
                    let r = webcore::http::get(&url).ok()?;
                    webcore::image::decode(&r.body).map(std::sync::Arc::new)
                })
                .clone();
            if let Some(img) = img {
                post(client, WMU_IMAGE, Box::into_raw(Box::new((node, img))) as u32, generation);
            }
        }
        post(client, WMU_IMAGES_DONE, 0, generation);
    });
}

fn init_controls(page: &Page) {
    let a = app();
    let dom = &page.dom;
    for n in dom.descendants(webcore::dom::Dom::DOCUMENT) {
        let Some(e) = dom.element(n) else { continue };
        let st = match &*e.name.local {
            "input" => {
                let ty = e.attr("type").unwrap_or("text").to_ascii_lowercase();
                let value = e.attr("value").map(String::from).unwrap_or_else(|| match ty.as_str() {
                    "submit" => "Submit".into(),
                    "reset" => "Reset".into(),
                    _ => String::new(),
                });
                ControlState { value, checked: e.attr("checked").is_some() }
            }
            "textarea" => ControlState { value: dom.text_content(n), checked: false },
            "select" => {
                let opts: Vec<NodeId> = dom.descendants(n).filter(|&o| dom.element(o).is_some_and(|e| e.is("option"))).collect();
                let sel = opts.iter().copied().find(|&o| dom.element(o).unwrap().attr("selected").is_some()).or(opts.first().copied());
                ControlState { value: sel.map(|o| dom.text_content(o).trim().to_string()).unwrap_or_default(), checked: false }
            }
            _ => continue,
        };
        a.controls.insert(n, st);
    }
}

fn relayout(restyle: bool) {
    let a = app();
    let Some(page) = a.page.as_mut() else { return };
    let width = a.view_w.max(100) as f32;
    if restyle && (page.style_width - width).abs() >= 1.0 && !page.sheets.is_empty() {
        page.restyle((width, a.view_h as f32));
    }
    let fonts = a.fonts.as_mut().unwrap();
    fonts.forget_current();
    let laid = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| webcore::layout::layout(&page.dom, &page.styles, &page.url, width, fonts, &a.image_sizes)));
    match laid {
        Ok(l) => a.laid = Some(l),
        Err(_) => {
            a.laid = None;
            set_status("WebRS2 failed to lay out this page (a panic)");
        }
    }
    clamp_scroll();
    update_scrollbar();
    invalidate(a.view);
}

fn max_scroll() -> i32 {
    let a = app();
    let h = a.laid.as_ref().map_or(0.0, |l| l.height) as i32;
    (h - a.view_h).max(0)
}

fn clamp_scroll() {
    let a = app();
    a.scroll = a.scroll.clamp(0, max_scroll());
}

fn update_scrollbar() {
    let a = app();
    let max = max_scroll();
    a.scroll_scale = (max / 30000 + 1).max(1);
    let s = a.scroll_scale;
    send(a.scrollbar, SBM_SETSCROLLBAR, (a.scroll / s) as u32, mp(0, (max / s) as u32));
    let total = a.laid.as_ref().map_or(1.0, |l| l.height) as i32;
    send(a.scrollbar, SBM_SETTHUMBSIZE, mp((a.view_h / s) as u32, (total.max(1) / s) as u32), 0);
}

fn scroll_to(y: i32) {
    let a = app();
    let old = a.scroll;
    a.scroll = y;
    clamp_scroll();
    if a.scroll != old {
        finish_edit();
        update_scrollbar();
        invalidate(a.view);
    }
}

fn scroll_to_fragment(frag: &str) {
    let a = app();
    let Some(l) = &a.laid else { return };
    let frag = webcore::http::percent_decode(frag);
    if let Some((_, y)) = l.anchors.iter().find(|(id, _)| *id == frag) {
        scroll_to(*y as i32);
    }
}

fn paint_view(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, a.hps, &mut upd) };
    let bg = a.laid.as_ref().map_or(0xffffff, |l| l.background & 0xff_ffff) as i32;
    unsafe {
        GpiSetColor(hps, bg);
        GpiMove(hps, &pt(upd.l, upd.b));
        GpiBox(hps, DRO_FILL, &pt(upd.r - 1, upd.t - 1), 0, 0);
    }
    if let (Some(laid), Some(page)) = (&a.laid, &a.page) {
        let top = (a.scroll + (a.view_h - upd.t)) as f32;
        let bottom = (a.scroll + (a.view_h - upd.b)) as f32;
        let fonts = a.fonts.as_mut().unwrap();
        let mut p = Painter { hps, fonts, height: a.view_h, scroll: a.scroll, x_off: 0 };
        p.paint(laid, top, bottom, &a.images, &a.controls, &page.dom);
        if let Some(f) = a.focus_link {
            for (r, i) in &laid.link_rects {
                if *i == f {
                    p.focus_rect(r.x - 1.0, r.y, r.w + 2.0, r.h);
                }
            }
        }
    }
    unsafe { WinEndPaint(hps) };
}

fn paint_client(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, 0, &mut upd) };
    let r = rect(hwnd);
    unsafe {
        WinFillRect(hps, &r, SYSCLR_DIALOGBACKGROUND);
        let mut sr = RectL::new(6, 0, r.r - 6, STATUS_H);
        let t = webcore::codepage::encode(&a.status, 850);
        WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut sr, SYSCLR_WINDOWTEXT, SYSCLR_DIALOGBACKGROUND, DT_LEFT | DT_VCENTER);
        WinEndPaint(hps);
    }
}

/// Page coordinates of a view point.
fn page_xy(mp1: u32) -> (f32, f32) {
    let a = app();
    let (x, y) = ((mp1 & 0xffff) as i16 as i32, (mp1 >> 16) as i16 as i32);
    (x as f32, (a.scroll + a.view_h - y) as f32)
}

fn link_at(x: f32, y: f32) -> Option<usize> {
    let a = app();
    let l = a.laid.as_ref()?;
    l.link_rects.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, i)| *i)
}

fn control_at(x: f32, y: f32) -> Option<(NodeId, ControlKind, webcore::layout::Rect)> {
    let a = app();
    let l = a.laid.as_ref()?;
    l.items.iter().rev().find_map(|it| match it {
        Item::Control { r, node, kind, .. } if r.contains(x, y) => Some((*node, *kind, *r)),
        _ => None,
    })
}

fn click(mp1: u32) {
    let a = app();
    let (x, y) = page_xy(mp1);
    finish_edit();
    if let Some((node, kind, r)) = control_at(x, y) {
        match kind {
            ControlKind::Text | ControlKind::Password | ControlKind::TextArea => start_edit(node, r),
            ControlKind::Checkbox => {
                if let Some(s) = a.controls.get_mut(&node) {
                    s.checked = !s.checked;
                }
                invalidate(a.view);
            }
            ControlKind::Radio => {
                let name = a.page.as_ref().and_then(|p| p.dom.element(node)).and_then(|e| e.attr("name")).map(String::from);
                if let (Some(name), Some(p)) = (name, a.page.as_ref()) {
                    for (n, s) in a.controls.iter_mut() {
                        if p.dom.element(*n).is_some_and(|e| e.attr("name") == Some(&name) && e.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("radio"))) {
                            s.checked = *n == node;
                        }
                    }
                }
                invalidate(a.view);
            }
            ControlKind::Select => {
                // The next option.
                if let Some(p) = &a.page {
                    let opts: Vec<String> = p.dom.descendants(node).filter(|&o| p.dom.element(o).is_some_and(|e| e.is("option"))).map(|o| p.dom.text_content(o).trim().to_string()).collect();
                    if let Some(s) = a.controls.get_mut(&node) {
                        if !opts.is_empty() {
                            let i = opts.iter().position(|o| *o == s.value).map_or(0, |i| (i + 1) % opts.len());
                            s.value = opts[i].clone();
                        }
                    }
                }
                invalidate(a.view);
            }
            ControlKind::Submit | ControlKind::Image => submit(node),
            ControlKind::Button => {
                let is_submit = a.page.as_ref().and_then(|p| p.dom.element(node)).is_some_and(|e| e.is("button") && e.attr("type").is_none_or(|t| t.eq_ignore_ascii_case("submit")));
                if is_submit {
                    submit(node);
                }
            }
        }
        return;
    }
    if let Some(i) = link_at(x, y) {
        let url = a.laid.as_ref().unwrap().links[i].clone();
        navigate(url, true, None);
    }
}

fn start_edit(node: NodeId, r: webcore::layout::Rect) {
    let a = app();
    let x = r.x as i32;
    let ytop = a.view_h - (r.y as i32 - a.scroll);
    let h = r.h as i32;
    let value = a.controls.get(&node).map(|s| s.value.clone()).unwrap_or_default();
    let t = z(&value);
    let hwnd = unsafe {
        WinCreateWindow(a.view, WC | WC_ENTRYFIELD as u32, t.as_ptr(), WS_VISIBLE | ES_AUTOSCROLL, x + 2, ytop - h + 2, r.w as i32 - 4, h - 4, a.view, HWND_TOP, ID_EDIT, core::ptr::null(), core::ptr::null())
    };
    if hwnd == 0 {
        return;
    }
    send(hwnd, EM_SETTEXTLIMIT, 1000, 0);
    EDIT_PROC.store(unsafe { WinSubclassWindow(hwnd, edit_proc) }, Ordering::Relaxed);
    unsafe { WinSetFocus(HWND_DESKTOP, hwnd) };
    a.editing = Some((node, hwnd));
}

fn finish_edit() {
    let a = app();
    if let Some((node, hwnd)) = a.editing.take() {
        let v = text(hwnd);
        if let Some(s) = a.controls.get_mut(&node) {
            s.value = v;
        }
        unsafe {
            WinSetFocus(HWND_DESKTOP, a.view);
            WinDestroyWindow(hwnd);
        }
        invalidate(a.view);
    }
}

fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => out.push(b as char),
            b' ' => out.push('+'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Send the form `from` (a submit control) belongs to.
fn submit(from: NodeId) {
    let a = app();
    let Some(page) = &a.page else { return };
    let dom = &page.dom;
    let mut f = dom.nodes[from].parent;
    while let Some(n) = f {
        if dom.element(n).is_some_and(|e| e.is("form")) {
            break;
        }
        f = dom.nodes[n].parent;
    }
    let Some(form) = f else { return };
    let fe = dom.element(form).unwrap();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for n in dom.descendants(form) {
        let Some(e) = dom.element(n) else { continue };
        let Some(name) = e.attr("name") else { continue };
        if e.attr("disabled").is_some() {
            continue;
        }
        let st = a.controls.get(&n);
        match &*e.name.local {
            "input" => {
                let ty = e.attr("type").unwrap_or("text").to_ascii_lowercase();
                match ty.as_str() {
                    "checkbox" | "radio" => {
                        if st.is_some_and(|s| s.checked) {
                            pairs.push((name.into(), e.attr("value").unwrap_or("on").into()));
                        }
                    }
                    "submit" | "image" | "button" | "reset" => {
                        if n == from && ty == "submit" {
                            pairs.push((name.into(), e.attr("value").unwrap_or("Submit").into()));
                        }
                    }
                    "hidden" => pairs.push((name.into(), e.attr("value").unwrap_or("").into())),
                    _ => pairs.push((name.into(), st.map(|s| s.value.clone()).unwrap_or_default())),
                }
            }
            "textarea" => pairs.push((name.into(), st.map(|s| s.value.clone()).unwrap_or_default())),
            "select" => {
                let shown = st.map(|s| s.value.clone()).unwrap_or_default();
                let opt = dom.descendants(n).find(|&o| dom.element(o).is_some_and(|e| e.is("option")) && dom.text_content(o).trim() == shown);
                let v = opt.and_then(|o| dom.element(o).unwrap().attr("value").map(String::from)).unwrap_or(shown);
                pairs.push((name.into(), v));
            }
            "button" => {
                if n == from {
                    pairs.push((name.into(), e.attr("value").unwrap_or("").into()));
                }
            }
            _ => {}
        }
    }
    let query = pairs.iter().map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v))).collect::<Vec<_>>().join("&");
    let action = fe.attr("action").unwrap_or("");
    let Ok(mut url) = page.url.join(action) else { return };
    url.set_fragment(None);
    if fe.attr("method").is_some_and(|m| m.eq_ignore_ascii_case("post")) {
        navigate(url, true, Some(("application/x-www-form-urlencoded".into(), query.into_bytes())));
    } else {
        url.set_query(Some(&query));
        navigate(url, true, None);
    }
}

fn command(id: u32) {
    let a = app();
    match id {
        ID_BACK => {
            if a.hist_pos > 0 {
                a.hist_pos -= 1;
                navigate(a.history[a.hist_pos].clone(), false, None);
            }
        }
        ID_FORWARD => {
            if a.hist_pos + 1 < a.history.len() {
                a.hist_pos += 1;
                navigate(a.history[a.hist_pos].clone(), false, None);
            }
        }
        ID_RELOAD => {
            if let Some(u) = a.history.get(a.hist_pos).cloned() {
                navigate(u, false, None);
            }
        }
        ID_HOME => navigate(Url::parse(HOME).unwrap(), true, None),
        ID_GO => {
            let t = text(a.url_field);
            match webcore::parse_user_url(&t) {
                Some(u) => {
                    navigate(u, true, None);
                    unsafe { WinSetFocus(HWND_DESKTOP, a.view) };
                }
                None => message(a.frame, &format!("\"{t}\" is not an address."), "WebRS2"),
            }
        }
        _ => {}
    }
}

/// Move the focus to the next (or previous) link in document order.
fn next_link(forward: bool) {
    let a = app();
    let Some(l) = &a.laid else { return };
    // Links in the order they appear.
    let mut order: Vec<usize> = Vec::new();
    for (_, i) in &l.link_rects {
        if !order.contains(i) {
            order.push(*i);
        }
    }
    if order.is_empty() {
        return;
    }
    let pos = a.focus_link.and_then(|f| order.iter().position(|&i| i == f));
    let next = match (pos, forward) {
        (None, true) => 0,
        (None, false) => order.len() - 1,
        (Some(p), true) => (p + 1) % order.len(),
        (Some(p), false) => (p + order.len() - 1) % order.len(),
    };
    let link = order[next];
    a.focus_link = Some(link);
    let url = l.links[link].to_string();
    if let Some((r, _)) = l.link_rects.iter().find(|(_, i)| *i == link) {
        let (top, bottom) = (r.y as i32, (r.y + r.h) as i32);
        if top < a.scroll || bottom > a.scroll + a.view_h {
            scroll_to(top - a.view_h / 3);
        }
    }
    set_status(&url);
    invalidate(a.view);
}

fn key(mp1: u32, mp2: u32) -> bool {
    let a = app();
    let fl = lo(mp1);
    if fl & KC_KEYUP != 0 {
        return false;
    }
    let line = 40;
    let page = (a.view_h - 40).max(line);
    if fl & KC_VIRTUALKEY != 0 {
        let vk = hi(mp2) as u16;
        match vk {
            VK_DOWN => scroll_to(a.scroll + line),
            VK_UP => scroll_to(a.scroll - line),
            VK_PAGEDOWN | VK_SPACE => scroll_to(a.scroll + page),
            VK_PAGEUP => scroll_to(a.scroll - page),
            VK_HOME if fl & KC_CTRL != 0 || true => scroll_to(0),
            VK_END => scroll_to(i32::MAX / 2),
            VK_LEFT if fl & KC_ALT != 0 => command(ID_BACK),
            VK_RIGHT if fl & KC_ALT != 0 => command(ID_FORWARD),
            VK_BACKSPACE => command(ID_BACK),
            VK_F5 => command(ID_RELOAD),
            VK_TAB => next_link(fl & KC_SHIFT == 0),
            VK_ENTER | VK_NEWLINE => {
                let target = a.focus_link.and_then(|i| a.laid.as_ref().map(|l| l.links[i].clone()));
                match target {
                    Some(u) => navigate(u, true, None),
                    None => return false,
                }
            }
            0x07 /* VK_BACKTAB */ => next_link(false),
            _ => return false,
        }
        return true;
    }
    if fl & KC_CHAR != 0 && fl & KC_CTRL != 0 && matches!(lo(mp2), 0x0c | 0x6c | 0x4c) {
        unsafe { WinSetFocus(HWND_DESKTOP, a.url_field) };
        send(a.url_field, EM_SETSEL, mp(0, 2000), 0);
        return true;
    }
    false
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            app().client = hwnd;
            create_children(hwnd);
            0
        }
        WM_SIZE => {
            layout_children();
            0
        }
        WM_PAINT => {
            paint_client(hwnd);
            0
        }
        WM_ERASEBACKGROUND => 0,
        WM_COMMAND => {
            command(lo(mp1));
            0
        }
        WM_CONTROL if lo(mp1) == ID_EDIT && hi(mp1) == EN_KILLFOCUS => {
            finish_edit();
            0
        }
        WM_VSCROLL => {
            let a = app();
            let s = a.scroll_scale;
            let cmd = hi(mp2);
            let page = (a.view_h - 40).max(40);
            match cmd {
                SB_LINEUP => scroll_to(a.scroll - 40),
                SB_LINEDOWN => scroll_to(a.scroll + 40),
                SB_PAGEUP => scroll_to(a.scroll - page),
                SB_PAGEDOWN => scroll_to(a.scroll + page),
                SB_SLIDERTRACK | SB_SLIDERPOSITION => scroll_to(lo(mp2) as i16 as i32 * s),
                _ => {}
            }
            0
        }
        WM_CHAR => {
            if key(mp1, mp2) {
                1
            } else {
                unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
            }
        }
        WMU_STATUS => {
            let s = unsafe { Box::from_raw(mp1 as *mut String) };
            if mp2 == GENERATION.load(Ordering::SeqCst) {
                set_status(&s);
            }
            0
        }
        WMU_LOADED => {
            let l = unsafe { Box::from_raw(mp1 as *mut Loaded) };
            if mp2 == GENERATION.load(Ordering::SeqCst) {
                loaded(*l);
            }
            0
        }
        WMU_IMAGE => {
            let b = unsafe { Box::from_raw(mp1 as *mut (NodeId, std::sync::Arc<webcore::image::Image>)) };
            if mp2 == GENERATION.load(Ordering::SeqCst) {
                let a = app();
                let (node, img) = *b;
                let bg = a.laid.as_ref().map_or(0xffffff, |l| l.background);
                a.images.insert(node, Bitmap::from_image(&img, bg));
                let had = a.image_sizes.insert(node, (img.w, img.h));
                if had.is_none() {
                    // New sizes: lay out again soon (once for many).
                    if !a.relayout_pending {
                        a.relayout_pending = true;
                        unsafe { WinStartTimer(a.hab, hwnd, TIMER_RELAYOUT, 400) };
                    }
                } else {
                    invalidate(a.view);
                }
            }
            0
        }
        WMU_IMAGES_DONE => {
            if mp2 == GENERATION.load(Ordering::SeqCst) {
                set_status("Done");
            }
            0
        }
        WM_TIMER if lo(mp1) == TIMER_RELAYOUT => {
            let a = app();
            unsafe { WinStopTimer(a.hab, hwnd, TIMER_RELAYOUT) };
            if a.relayout_pending {
                a.relayout_pending = false;
                relayout(false);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

extern "C" fn view_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_view(hwnd);
            0
        }
        WM_ERASEBACKGROUND => 0,
        WM_BUTTON1DOWN => {
            unsafe { WinSetFocus(HWND_DESKTOP, hwnd) };
            1
        }
        WM_BUTTON1CLICK => {
            click(mp1);
            1
        }
        WM_MOUSEMOVE => {
            let a = app();
            let (x, y) = page_xy(mp1);
            let l = link_at(x, y);
            if l != a.hover {
                a.hover = l;
                match l {
                    Some(i) => {
                        let u = a.laid.as_ref().unwrap().links[i].to_string();
                        set_status(&u);
                    }
                    None => set_status(if a.loading { "Loading..." } else { "Done" }),
                }
            }
            unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, if a.loading { SPTR_WAIT } else { SPTR_ARROW }, 0)) };
            1
        }
        WM_CHAR => {
            if key(mp1, mp2) {
                1
            } else {
                unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
            }
        }
        WM_VSCROLL => send(app().client, msg, mp1, mp2),
        WM_CONTROL if lo(mp1) == ID_EDIT && hi(mp1) == EN_KILLFOCUS => {
            finish_edit();
            0
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}
