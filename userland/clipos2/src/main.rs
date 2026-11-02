//! CLIPOS2.EXE: rs2's Clipboard Viewer, a re-implementation of OS/2's.
//!
//! As OS/2's (studied in its binary): the window is the clipboard viewer
//! (WinSetClipbrdViewer; a second copy says "Viewer already installed."
//! and ends), its title names the format shown, "Clipboard (Text)", and
//! it shows the clipboard's data as it changes (WM_DRAWCLIPBOARD): text,
//! a bitmap or a metafile, the first of them that is there (the display
//! formats CF_DSPTEXT, CF_DSPBITMAP and CF_DSPMETAFILE first), with
//! scroll bars when it is larger than the window. Data the owner draws
//! (CFI_OWNERDISPLAY) is painted by the owner through WM_PAINTCLIPBOARD,
//! WM_SIZECLIPBOARD and the scroll messages. Display > Render lists the
//! formats on the clipboard by name (registered formats by their atom's
//! name) and shows the one chosen. Menus and help panel numbers are
//! OS/2's; the help is CLIPOS2.HLP.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use pmkit::gpi;
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

os2::entry!(main);

// Menu ids (OS/2's).
const IDM_FILE: u16 = 1;
const IDM_DISPLAY: u16 = 2;
const IDM_RENDER: u16 = 10;
const IDM_HELP: u16 = 13;
const IDM_USINGHELP: u16 = 17;
const IDM_GENERALHELP: u16 = 0x8014;
const IDM_HELPINDEX: u16 = 0x8013;
const IDM_PRODUCT: u16 = 12;

const DLG_PRODUCT: u16 = 1;
const DLG_RENDER: u16 = 2;
const ID_FORMATS: u32 = 32;
const ID_HELP: u32 = 3;

const FRAME_ID: u32 = 1;
const HELP_FILE: &[u8] = b"CLIPOS2.HLP";
const TITLE: &[u8] = b"Clipboard Viewer";
const INI_APP: &[u8] = b"CLIPOS2";

struct App {
    hab: u32,
    frame: u32,
    client: u32,
    menu: u32,
    vscroll: u32,
    hscroll: u32,
    help: Option<&'static mut Help>,
    /// The format shown (0: the clipboard is empty).
    format: u32,
    /// Chosen with Render (else the first known one is shown).
    chosen: Option<u32>,
    /// The picture drawn off screen (bitmaps and metafiles).
    picture: Option<gpi::MemPs>,
    /// The text shown, by lines.
    lines: Vec<Vec<u8>>,
    /// The content's size in pels and the scroll position.
    cx: i32,
    cy: i32,
    x: i32,
    y: i32,
    line_h: i32,
    char_w: i32,
}

static mut APP: App = App {
    hab: 0,
    frame: 0,
    client: 0,
    menu: 0,
    vscroll: 0,
    hscroll: 0,
    help: None,
    format: 0,
    chosen: None,
    picture: None,
    lines: Vec::new(),
    cx: 0,
    cy: 0,
    x: 0,
    y: 0,
    line_h: 16,
    char_w: 8,
};

fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

fn trace(s: &[u8]) {
    pmkit::trace("clipos2", s);
}

fn main(_s: &os2::Startup) -> u32 {
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    if unsafe { WinQueryClipbrdViewer(a.hab) } != 0 {
        message(0, b"Viewer already installed.", TITLE, MB_OK | MB_INFORMATION);
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return 1;
    }
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_VERTSCROLL | FCF_HORZSCROLL | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, b"ClipViewer", client_proc, CS_SIZEREDRAW, 0, fcf, b"Clipboard", FRAME_ID);
    if frame == 0 {
        return 1;
    }
    a.frame = frame;
    a.client = client;
    a.vscroll = item(frame, FID_VERTSCROLL);
    a.hscroll = item(frame, FID_HORZSCROLL);
    a.menu = set_menu(
        frame,
        &[
            Menu::Sub(IDM_FILE, b"~File", alloc::vec![Menu::Sys(SC_CLOSE, b"E~xit")]),
            Menu::Sub(IDM_DISPLAY, b"~Display", alloc::vec![Menu::Item(IDM_RENDER, b"~Render...")]),
            Menu::Sub(
                IDM_HELP,
                b"~Help",
                alloc::vec![
                    Menu::Sys(IDM_HELPINDEX, b"Help ~index"),
                    Menu::Item(IDM_GENERALHELP, b"~General help"),
                    Menu::Item(IDM_USINGHELP, b"~Using help"),
                    Menu::Sep,
                    Menu::Item(IDM_PRODUCT, b"~Product information"),
                ]
            ),
        ],
    );
    a.help = Help::create(
        a.hab,
        frame,
        HELP_FILE,
        b"Clipboard Viewer Help",
        &[
            HelpWindow {
                id: FRAME_ID as u16,
                ext: 21221,
                items: &[(IDM_FILE, 21214), (IDM_DISPLAY, 21217), (IDM_RENDER, 21218), (3, 21219), (IDM_HELP, 64022), (IDM_USINGHELP, 64027), (IDM_GENERALHELP, 21221), (IDM_HELPINDEX, 64044), (IDM_PRODUCT, 64096)],
            },
            HelpWindow { id: DLG_RENDER, ext: 21224, items: &[(ID_FORMATS as u16, 21224), (1, 21222), (2, 21223), (ID_HELP as u16, 21224)] },
            HelpWindow { id: DLG_PRODUCT, ext: 64096, items: &[(1, 64096)] },
        ],
    );
    unsafe { WinSetClipbrdViewer(a.hab, client) };
    refresh();
    show_at_saved(a.hab, frame, INI_APP, b"WINDOWPOS", None);
    message_loop(a.hab);
    save_position(frame, INI_APP, b"WINDOWPOS");
    unsafe {
        WinSetClipbrdViewer(a.hab, 0);
        if let Some(h) = a.help.as_mut() {
            h.destroy(frame);
        }
        a.picture = None;
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

/// The name of a format: OS/2's for the standard ones, the atom's for
/// registered ones.
fn format_name(f: u32) -> Vec<u8> {
    let std: &[u8] = match f {
        0 => b"empty",
        CF_TEXT => b"Text",
        CF_BITMAP => b"BITMAP",
        CF_DSPTEXT => b"Display Text",
        CF_DSPBITMAP => b"Display BITMAP",
        CF_METAFILE => b"Metafile",
        CF_DSPMETAFILE => b"Display Metafile",
        CF_PALETTE => b"Palette",
        _ => b"",
    };
    if !std.is_empty() {
        return std.to_vec();
    }
    let mut buf = [0u8; 64];
    let n = unsafe { WinQueryAtomName(WinQuerySystemAtomTable(), f, buf.as_mut_ptr(), 50) };
    if n == 0 {
        return b"unknown".to_vec();
    }
    until_nul(&buf).to_vec()
}

/// The formats on the clipboard (open).
fn formats(hab: u32) -> Vec<u32> {
    let mut v = Vec::new();
    let mut f = 0;
    loop {
        f = unsafe { WinEnumClipbrdFmts(hab, f) };
        if f == 0 || v.len() > 64 {
            break;
        }
        v.push(f);
    }
    v
}

fn fmt_info(hab: u32, f: u32) -> Option<u32> {
    let mut info = 0;
    (unsafe { WinQueryClipbrdFmtInfo(hab, f, &mut info) } != 0).then_some(info)
}

/// Look at the clipboard again: choose the format, take its data and
/// set the title, menu and scroll bars.
fn refresh() {
    let a = app();
    a.picture = None;
    a.lines.clear();
    a.cx = 0;
    a.cy = 0;
    a.x = 0;
    a.y = 0;
    a.format = 0;
    if unsafe { WinOpenClipbrd(a.hab) } != 0 {
        let fmts = formats(a.hab);
        let pick = match a.chosen {
            Some(c) if fmts.contains(&c) => Some(c),
            _ => {
                a.chosen = None;
                // The display formats first, then text, bitmaps and
                // metafiles; failing those, data the owner draws.
                [CF_DSPTEXT, CF_DSPBITMAP, CF_DSPMETAFILE, CF_TEXT, CF_BITMAP, CF_METAFILE]
                    .into_iter()
                    .find(|f| fmts.contains(f))
                    .or_else(|| fmts.iter().copied().find(|&f| fmt_info(a.hab, f).is_some_and(|i| i & CFI_OWNERDISPLAY != 0)))
                    .or(fmts.first().copied())
            }
        };
        if let Some(f) = pick {
            a.format = f;
            take(f);
        }
        unsafe { WinCloseClipbrd(a.hab) };
    }
    let mut t = b"Clipboard (".to_vec();
    t.extend_from_slice(&format_name(a.format));
    t.push(b')');
    set_text(a.frame, &t);
    menu_enable(a.menu, IDM_RENDER, a.format != 0);
    trace(&t);
    scroll_bars();
    invalidate(a.client);
}

/// Take the data of format `f` (the clipboard is open).
fn take(f: u32) {
    let a = app();
    let data = unsafe { WinQueryClipbrdData(a.hab, f) };
    match f {
        CF_TEXT | CF_DSPTEXT if data != 0 => {
            let text = unsafe { cstr(data as *const u8) };
            a.lines = split_lines(text);
            let hps = unsafe { WinGetPS(a.client) };
            let (h, _) = gpi::font_height(hps);
            a.line_h = h.max(8);
            a.char_w = gpi::text_width(hps, b"0").max(4);
            a.cx = a.lines.iter().map(|l| gpi::text_width(hps, l)).max().unwrap_or(0) + 8;
            unsafe { WinReleasePS(hps) };
            a.cy = a.lines.len() as i32 * a.line_h + 4;
        }
        CF_BITMAP | CF_DSPBITMAP if data != 0 => {
            let mut bih = BitmapInfoHeader2::new(0, 0, 0);
            bih.cb = 16;
            unsafe { GpiQueryBitmapInfoHeader(data, &mut bih) };
            let (w, h) = (bih.cx as i32, bih.cy as i32);
            if w > 0 && h > 0 {
                if let Some(m) = gpi::MemPs::new(a.hab, w, h) {
                    unsafe {
                        WinDrawBitmap(m.hps, data, core::ptr::null(), &pt(0, 0), CLR_BLACK, CLR_WHITE, DBM_NORMAL);
                    }
                    a.cx = w;
                    a.cy = h;
                    a.picture = Some(m);
                }
            }
        }
        CF_METAFILE | CF_DSPMETAFILE if data != 0 => play_metafile(data),
        _ => {}
    }
}

/// Lines of text (CR LF, LF or CR ends one; tabs to every 8th column).
fn split_lines(t: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < t.len() {
        match t[i] {
            b'\r' => {
                out.push(core::mem::take(&mut cur));
                if t.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => out.push(core::mem::take(&mut cur)),
            b'\t' => {
                cur.push(b' ');
                while cur.len() % 8 != 0 {
                    cur.push(b' ');
                }
            }
            0x1a => break,
            c => cur.push(c),
        }
        i += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Play a metafile at its own page size into a picture: once without
/// drawing to learn its size (RES_RESET sets the page from the
/// metafile), then into a bitmap of that size.
fn play_metafile(hmf: u32) {
    let a = app();
    let mut opts = [0i32; 10];
    opts[PMF_RESET] = RES_RESET;
    opts[PMF_SUPPRESS] = SUP_SUPPRESS;
    opts[PMF_LOADTYPE] = LT_ORIGINALVIEW;
    opts[PMF_LCIDS] = LC_LOADDISC;
    opts[PMF_COLORTABLES] = CTAB_REPLACE;
    let Some(probe) = gpi::MemPs::new(a.hab, 1, 1) else { return };
    let mut desc = [0u8; 64];
    let mut segs = 0;
    unsafe { GpiPlayMetaFile(probe.hps, hmf, 9, opts.as_ptr(), &mut segs, desc.len() as i32, desc.as_mut_ptr()) };
    let mut size = SizeL::default();
    unsafe { GpiQueryPS(probe.hps, &mut size) };
    let mut vp = RectL::default();
    unsafe { GpiQueryPageViewport(probe.hps, &mut vp) };
    drop(probe);
    let (w, h) = ((vp.r - vp.l).clamp(16, 2400), (vp.t - vp.b).clamp(16, 2400));
    trace(alloc::format!("metafile page {}x{} viewport {}x{}", size.cx, size.cy, w, h).as_bytes());
    let Some(m) = gpi::MemPs::new(a.hab, w, h) else { return };
    gpi::fill(m.hps, RectL::new(0, 0, w, h), 0xffffff);
    opts[PMF_SUPPRESS] = SUP_NOSUPPRESS;
    unsafe { GpiPlayMetaFile(m.hps, hmf, 9, opts.as_ptr(), &mut segs, 0, core::ptr::null_mut()) };
    a.cx = w;
    a.cy = h;
    a.picture = Some(m);
}

fn view() -> RectL {
    window_rect(app().client)
}

/// The scroll bars for the content and the window.
fn scroll_bars() {
    let a = app();
    let r = view();
    let (vw, vh) = (r.width(), r.height());
    a.x = a.x.clamp(0, (a.cx - vw).max(0));
    a.y = a.y.clamp(0, (a.cy - vh).max(0));
    let hmax = (a.cx - vw).max(0);
    let vmax = (a.cy - vh).max(0);
    send(a.hscroll, SBM_SETSCROLLBAR, a.x as u32, mp(0, hmax as u32));
    send(a.hscroll, SBM_SETTHUMBSIZE, mp(vw as u32, a.cx.max(1) as u32), 0);
    send(a.vscroll, SBM_SETSCROLLBAR, a.y as u32, mp(0, vmax as u32));
    send(a.vscroll, SBM_SETTHUMBSIZE, mp(vh as u32, a.cy.max(1) as u32), 0);
    unsafe {
        WinEnableWindow(a.hscroll, (hmax > 0) as u32);
        WinEnableWindow(a.vscroll, (vmax > 0) as u32);
    }
}

fn scroll(vertical: bool, cmd: u32, pos: i32) {
    let a = app();
    let r = view();
    let (line, page, cur, max) = if vertical {
        (a.line_h, r.height(), a.y, (a.cy - r.height()).max(0))
    } else {
        (a.char_w * 4, r.width(), a.x, (a.cx - r.width()).max(0))
    };
    let new = match cmd {
        SB_LINEUP => cur - line,
        SB_LINEDOWN => cur + line,
        SB_PAGEUP => cur - page,
        SB_PAGEDOWN => cur + page,
        SB_SLIDERTRACK | SB_SLIDERPOSITION => pos,
        _ => return,
    }
    .clamp(0, max);
    if new == cur {
        return;
    }
    if vertical {
        a.y = new;
        send(a.vscroll, SBM_SETPOS, new as u32, 0);
    } else {
        a.x = new;
        send(a.hscroll, SBM_SETPOS, new as u32, 0);
    }
    owner_scroll(vertical, cmd, pos);
    invalidate(a.client);
}

/// Data the owner draws: pass the message on (OS/2's viewer does).
fn owner_draws() -> Option<u32> {
    let a = app();
    let info = fmt_info(a.hab, a.format)?;
    if info & CFI_OWNERDISPLAY == 0 {
        return None;
    }
    let o = unsafe { WinQueryClipbrdOwner(a.hab) };
    (o != 0).then_some(o)
}

fn owner_scroll(vertical: bool, cmd: u32, pos: i32) {
    if let Some(o) = owner_draws() {
        let m = if vertical { WM_VSCROLLCLIPBOARD } else { WM_HSCROLLCLIPBOARD };
        send(o, m, app().client, mp(pos as u32, cmd));
    }
}

fn paint(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, 0, &mut upd) };
    let r = view();
    if let Some(o) = owner_draws() {
        // The owner paints into a PS of ours (WM_PAINTCLIPBOARD).
        unsafe { WinEndPaint(hps) };
        send(o, WM_PAINTCLIPBOARD, hwnd, 0);
        return;
    }
    unsafe { WinFillRect(hps, &upd, SYSCLR_WINDOW) };
    match a.format {
        CF_TEXT | CF_DSPTEXT => {
            let top = r.t + a.y;
            let (_, desc) = gpi::font_height(hps);
            for (i, l) in a.lines.iter().enumerate() {
                let y = top - (i as i32 + 1) * a.line_h + desc;
                if y > r.t || y + a.line_h < r.b {
                    continue;
                }
                gpi::text_at(hps, 4 - a.x, y, l, SYSCLR_WINDOWTEXT);
            }
        }
        _ => {
            if let Some(p) = &a.picture {
                // The picture's top left at the window's top left.
                let x = -a.x;
                let y = r.t - a.cy + a.y;
                let pts = [pt(x, y), pt(x + p.cx, y + p.cy), pt(0, 0)];
                unsafe { GpiBitBlt(hps, p.hps, 3, pts.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) };
            } else if a.format != 0 {
                let mut t = b"The clipboard holds data in the format ".to_vec();
                t.extend_from_slice(&format_name(a.format));
                t.extend_from_slice(b", which the viewer cannot show. Choose Render from the Display menu to show another format.");
                let mut rr = RectL::new(r.l + 8, r.b + 8, r.r - 8, r.t - 8);
                draw_wrapped(hps, &mut rr, &t);
            }
        }
    }
    unsafe { WinEndPaint(hps) };
}

fn draw_wrapped(hps: u32, r: &mut RectL, t: &[u8]) {
    let mut rest = t;
    while !rest.is_empty() && r.t > r.b {
        let mut rr = *r;
        let n = unsafe { WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut rr, SYSCLR_WINDOWTEXT, 0, DT_LEFT | DT_TOP | DT_WORDBREAK) };
        if n <= 0 {
            break;
        }
        rest = &rest[n as usize..];
        while rest.first() == Some(&b' ') {
            rest = &rest[1..];
        }
        r.t -= app().line_h.max(gpi::font_height(hps).0);
    }
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_DRAWCLIPBOARD => {
            refresh();
            0
        }
        WM_SIZE => {
            scroll_bars();
            if let Some(o) = owner_draws() {
                send(o, WM_SIZECLIPBOARD, hwnd, mp2);
            }
            0
        }
        WM_VSCROLL => {
            scroll(true, hi(mp2), lo(mp2) as i16 as i32);
            0
        }
        WM_HSCROLL => {
            scroll(false, hi(mp2), lo(mp2) as i16 as i32);
            0
        }
        WM_CHAR => {
            let fl = lo(mp1);
            if fl & KC_KEYUP == 0 && fl & KC_VIRTUALKEY != 0 {
                match hi(mp2) as u16 {
                    VK_UP => scroll(true, SB_LINEUP, 0),
                    VK_DOWN => scroll(true, SB_LINEDOWN, 0),
                    VK_PAGEUP => scroll(true, SB_PAGEUP, 0),
                    VK_PAGEDOWN => scroll(true, SB_PAGEDOWN, 0),
                    VK_LEFT => scroll(false, SB_LINEUP, 0),
                    VK_RIGHT => scroll(false, SB_LINEDOWN, 0),
                    _ => return unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
                }
                return 1;
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_COMMAND => {
            match lo(mp1) as u16 {
                IDM_RENDER => render_dialog(),
                IDM_USINGHELP => help_request(HM_DISPLAY_HELP, 0, 0),
                IDM_GENERALHELP => help_request(HM_EXT_HELP, 0, 0),
                IDM_PRODUCT => product_info(a.frame, DLG_PRODUCT, b"Product Information", b"Clipboard Viewer", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Shows what is on the clipboard as it changes."]),
                _ => {}
            }
            0
        }
        WM_HELP => 0,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        HM_QUERY_KEYS_HELP => 0,
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

fn help_request(msg: u32, mp1: u32, mp2: u32) {
    // HM_DISPLAY_HELP with no panel is "Using help".
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, mp1, mp2);
}

// ---- Display > Render ----

static mut RENDER_FORMATS: Vec<u32> = Vec::new();

extern "C" fn render_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let fmts = unsafe { &mut *core::ptr::addr_of_mut!(RENDER_FORMATS) };
    match m {
        WM_INITDLG => {
            for (i, &f) in fmts.iter().enumerate() {
                lb_add(h, ID_FORMATS, &format_name(f));
                if f == app().format {
                    dlg_msg(h, ID_FORMATS, LM_SELECTITEM, i as u32, 1);
                }
            }
            0
        }
        WM_CONTROL if lo(mp1) == ID_FORMATS && hi(mp1) == LN_ENTER => {
            post(h, WM_COMMAND, DID_OK, 0);
            0
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == DID_OK {
                let sel = dlg_msg(h, ID_FORMATS, LM_QUERYSELECTION, LIT_FIRST, 0);
                if (sel as usize) < fmts.len() {
                    app().chosen = Some(fmts[sel as usize]);
                }
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn render_dialog() {
    let a = app();
    let fmts = if unsafe { WinOpenClipbrd(a.hab) } != 0 {
        let f = formats(a.hab);
        unsafe { WinCloseClipbrd(a.hab) };
        f
    } else {
        Vec::new()
    };
    if fmts.is_empty() {
        message(a.frame, b"Clipboard format not present.", TITLE, MB_OK | MB_INFORMATION);
        return;
    }
    unsafe { RENDER_FORMATS = fmts };
    let tpl = template(
        dialog_frame(b"Render", DLG_RENDER, 160, 96),
        alloc::vec![
            label(b"Available formats:", (10, 80, 140, 10), DT_LEFT),
            list_box((10, 24, 140, 56), ID_FORMATS, 0),
            button(b"~OK", (9, 4, 45), DID_OK, BS_DEFAULT),
            button(b"~Cancel", (59, 4, 45), DID_CANCEL, 0),
            help_button((107, 4, 45), ID_HELP),
        ],
    );
    if run_dialog(a.frame, render_proc, &tpl, 0) == DID_OK {
        refresh();
    }
}
