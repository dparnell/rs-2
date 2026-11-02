//! MPPM: rs2's media player, the program the multimedia data files open
//! in (as IBM's MPPM opens MMPARTS's players).
//!
//! `MPPM [file]` plays a sound (WAV, AU, SND, VOC, AIF, IFF), a MIDI song
//! (MID, RMI) or a movie (AVI, FLC, FLI) through MDM's devices, with
//! Play, Pause, Stop, Rewind and Forward buttons, a position bar to seek
//! with, the time and a volume bar; a movie shows in the window. An image
//! (BMP, DIB, JPG, GIF, PCX, TIF, TGA) is shown fitted into the window,
//! read through MMIO's image I/O procedures. Without a file, or with
//! Open..., the open dialog asks for one.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use mmapps::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "MMIO.27"]
    fn mmioOpen(name: *const u8, info: *mut u8, flags: u32) -> u32;
    #[link_name = "MMIO.28"]
    fn mmioRead(h: u32, buf: *mut u8, n: i32) -> i32;
    #[link_name = "MMIO.32"]
    fn mmioClose(h: u32, flags: u32) -> u32;
    #[link_name = "MMIO.77"]
    fn mmioGetHeader(h: u32, hdr: *mut u8, len: i32, read: *mut i32, r: u32, flags: u32) -> u32;
}

const CLASS: &[u8] = b"Rs2MediaPlayer\0";
const VIDEO_CLASS: &[u8] = b"Rs2MediaPlayerVideo\0";
const ID_PLAY: u32 = 10;
const ID_PAUSE: u32 = 11;
const ID_STOP: u32 = 12;
const ID_REWIND: u32 = 13;
const ID_FORWARD: u32 = 14;
const ID_OPEN: u32 = 15;
const ID_POS: u32 = 20;
const ID_TIME: u32 = 21;
const ID_VOL: u32 = 22;
const ID_VOLTEXT: u32 = 23;
const ID_INFO: u32 = 24;
const ID_VIDEO: u32 = 30;

const BAR_H: i32 = 108;

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    None,
    Sound,
    Midi,
    Movie,
    Image,
}

struct Image {
    width: u32,
    height: u32,
    bits: u16,
    palette: Vec<[u8; 4]>,
    data: Vec<u8>,
}

struct App {
    hab: u32,
    frame: u32,
    client: u32,
    video: u32,
    file: String,
    kind: Kind,
    open: bool,
    length: u32,
    tracking: bool,
    volume: u32,
    image: Option<Image>,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn kind_of(path: &str) -> Kind {
    match extension(path).as_str() {
        "WAV" | "AU" | "_AU" | "SND" | "VOC" | "AIF" | "AIFF" | "IFF" => Kind::Sound,
        "MID" | "RMI" | "_MI" => Kind::Midi,
        "AVI" | "FLC" | "FLI" => Kind::Movie,
        "BMP" | "DIB" | "JPG" | "JPEG" | "JPE" | "GIF" | "PCX" | "TIF" | "TIFF" | "TGA" | "VGA" => Kind::Image,
        _ => Kind::None,
    }
}

/// Read an image through MMIO (translated: an OS/2 bitmap).
fn load_image(path: &str) -> Option<Image> {
    let name = zs(path);
    // MMIOINFO with ulTranslate = MMIO_TRANSLATEHEADER | MMIO_TRANSLATEDATA.
    let mut info = vec![0u8; 80];
    info[64..68].copy_from_slice(&3u32.to_le_bytes());
    let h = unsafe { mmioOpen(name.as_ptr(), info.as_mut_ptr(), 4) };
    if h == 0 {
        return None;
    }
    let mut hdr = vec![0u8; 1116];
    let mut n = 0;
    unsafe { mmioGetHeader(h, hdr.as_mut_ptr(), hdr.len() as i32, &mut n, 0, 0) };
    let rd32 = |o: usize| u32::from_le_bytes(hdr[o..o + 4].try_into().unwrap());
    if n < 92 {
        unsafe { mmioClose(h, 0) };
        return None;
    }
    let (width, height, bits) = (rd32(32), rd32(36), u16::from_le_bytes([hdr[42], hdr[43]]));
    let row = ((width * bits as u32 + 31) / 32 * 4) as usize;
    let mut data = vec![0u8; row * height as usize];
    let mut got = 0usize;
    while got < data.len() {
        let r = unsafe { mmioRead(h, data[got..].as_mut_ptr(), (data.len() - got).min(1 << 20) as i32) };
        if r <= 0 {
            break;
        }
        got += r as usize;
    }
    unsafe { mmioClose(h, 0) };
    let colors = if bits <= 8 { 1usize << bits } else { 0 };
    let palette = (0..colors).map(|i| [hdr[92 + i * 4], hdr[93 + i * 4], hdr[94 + i * 4], 0]).collect();
    Some(Image { width, height, bits, palette, data })
}

fn close_media() {
    let a = app();
    if a.open {
        mci("close mppm wait");
        a.open = false;
    }
    a.image = None;
    a.length = 0;
}

fn status(text: &str) {
    set_text(item(app().client, ID_INFO), text);
}

/// Open `path` and show it.
fn load(path: &str) {
    close_media();
    let a = app();
    a.file = String::from(path);
    a.kind = kind_of(path);
    let title = alloc::format!("Media Player - {}", base_name(path));
    set_text(a.frame, &title);
    if a.kind == Kind::Image {
        match load_image(path) {
            Some(img) => {
                status(&alloc::format!("{} x {} pels, {} bits per pel", img.width, img.height, img.bits));
                a.image = Some(img);
            }
            None => status("The image cannot be read (no I/O procedure knows it)."),
        }
        layout();
        return;
    }
    let (rc, _) = mci(&alloc::format!("open \"{}\" alias mppm wait", path));
    if rc != 0 {
        status(&alloc::format!("Cannot open the file: {}", mci_error(rc)));
        layout();
        return;
    }
    a.open = true;
    let (_, dtype) = mci("capability mppm device type wait");
    if dtype.contains("digitalvideo") || dtype.contains("video") {
        a.kind = Kind::Movie;
    }
    mci("set mppm time format milliseconds wait");
    a.length = mci_num("status mppm length wait");
    if a.kind == Kind::Movie {
        mci(&alloc::format!("window mppm handle {} wait", a.video));
    }
    mci(&alloc::format!("set mppm audio volume {} wait", a.volume));
    let what = match a.kind {
        Kind::Sound => "Digital audio",
        Kind::Midi => "MIDI",
        _ => "Movie",
    };
    status(&alloc::format!("{what}: {}", fmt_ms(a.length)));
    layout();
    update_position();
}

fn update_position() {
    let a = app();
    let (pos, mode) = if a.open { (mci_num("status mppm position wait"), mci("status mppm mode wait").1) } else { (0, String::new()) };
    let bar = item(a.client, ID_POS);
    if !a.tracking {
        let max = a.length.max(1);
        set_scroll(bar, (pos as u64 * 1000 / max as u64) as u32, 1000, 20);
    }
    let m = if mode.is_empty() { String::new() } else { alloc::format!("  ({mode})") };
    set_text(item(a.client, ID_TIME), &alloc::format!("{} / {}{m}", fmt_ms(pos), fmt_ms(a.length)));
}

fn command(id: u32) {
    let a = app();
    match id {
        ID_OPEN => {
            if let Some(f) = file_dialog(a.frame, "Media Player - Open", "*.*", false) {
                load(&f);
            }
        }
        ID_PLAY if a.open => {
            let mode = mci("status mppm mode wait").1;
            if mode == "paused" {
                mci("resume mppm wait");
            } else {
                let pos = mci_num("status mppm position wait");
                if a.length > 0 && pos + 50 >= a.length {
                    mci("seek mppm to start wait");
                }
                mci_to("play mppm notify", a.client);
            }
        }
        ID_PAUSE if a.open => {
            mci("pause mppm wait");
        }
        ID_STOP if a.open => {
            mci("stop mppm wait");
            mci("seek mppm to start wait");
        }
        ID_REWIND if a.open => {
            let playing = mci("status mppm mode wait").1 == "playing";
            let pos = mci_num("status mppm position wait").saturating_sub(5000);
            mci(&alloc::format!("seek mppm to {pos} wait"));
            if playing {
                mci_to("play mppm notify", a.client);
            }
        }
        ID_FORWARD if a.open => {
            let playing = mci("status mppm mode wait").1 == "playing";
            let pos = (mci_num("status mppm position wait") + 5000).min(a.length);
            mci(&alloc::format!("seek mppm to {pos} wait"));
            if playing {
                mci_to("play mppm notify", a.client);
            }
        }
        _ => {}
    }
    update_position();
}

/// Place the controls (and the movie or image area) for the client size.
fn layout() {
    let a = app();
    let r = window_rect(a.client);
    let (w, h) = (r.r, r.t);
    let media = matches!(a.kind, Kind::Movie | Kind::Image);
    let bar = if a.kind == Kind::Image { 40 } else { BAR_H };
    unsafe {
        WinSetWindowPos(a.video, HWND_TOP, 6, bar, (w - 12).max(1), (h - bar - 6).max(1), SWP_SIZE | SWP_MOVE | if media { SWP_SHOW } else { 0 });
        if !media {
            WinShowWindow(a.video, 0);
        }
    }
    let controls = a.kind != Kind::Image;
    for id in [ID_PLAY, ID_PAUSE, ID_STOP, ID_REWIND, ID_FORWARD, ID_POS, ID_TIME, ID_VOL, ID_VOLTEXT] {
        unsafe { WinShowWindow(item(a.client, id), controls as u32) };
    }
    let place = |id: u32, x: i32, y: i32, cx: i32, cy: i32| unsafe {
        WinSetWindowPos(item(a.client, id), HWND_TOP, x, y, cx, cy, SWP_SIZE | SWP_MOVE);
    };
    place(ID_INFO, 8, 6, w - 290, 24);
    place(ID_OPEN, w - 104, 6, 96, 28);
    place(ID_POS, 8, 76, w - 16, 18);
    let bx = 8;
    for (i, id) in [ID_REWIND, ID_PLAY, ID_PAUSE, ID_STOP, ID_FORWARD].iter().enumerate() {
        place(*id, bx + i as i32 * 64, 40, 60, 30);
    }
    place(ID_TIME, bx + 5 * 64 + 6, 42, (w - (bx + 5 * 64 + 6) - 8).max(60), 26);
    place(ID_VOLTEXT, w - 278, 8, 66, 24);
    place(ID_VOL, w - 208, 11, 96, 18);
    unsafe { WinInvalidateRect(a.client, core::ptr::null(), 1) };
}

extern "C" fn video_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg != WM_PAINT {
        return unsafe { WinDefWindowProc(h, msg, mp1, mp2) };
    }
    let mut r = RectL::default();
    let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
    let a = app();
    let whole = window_rect(h);
    fill(hps, &whole, CLR_BLACK);
    if let Some(img) = &a.image {
        let dst = fit(whole, img.width as i32, img.height as i32, 1);
        draw_dib(hps, img.width, img.height, img.bits, &img.palette, &img.data, dst);
    } else if a.open {
        // A frame of a movie: the device draws it again.
        mci("update mppm wait");
    }
    unsafe { WinEndPaint(hps) };
    0
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_plain(h, CLR_PALEGRAY);
            0
        }
        WM_SIZE => {
            if unsafe { (*core::ptr::addr_of!(APP)).is_some() } {
                layout();
            }
            0
        }
        WM_COMMAND => {
            command(mp1 & 0xffff);
            0
        }
        WM_TIMER => {
            update_position();
            0
        }
        WM_HSCROLL => {
            let a = app();
            let (id, code, pos) = (mp1 & 0xffff, mp2 >> 16, mp2 & 0xffff);
            let bar = item(h, id);
            let cur = unsafe { WinSendMsg(bar, SBM_QUERYPOS, 0, 0) };
            let step = |d: i32| (cur as i32 + d).clamp(0, if id == ID_VOL { 100 } else { 1000 }) as u32;
            let new = match code {
                SB_LINEUP => step(-10),
                SB_LINEDOWN => step(10),
                SB_PAGEUP => step(-100),
                SB_PAGEDOWN => step(100),
                SB_SLIDERTRACK | SB_SLIDERPOSITION => pos,
                _ => cur,
            };
            unsafe { WinSendMsg(bar, SBM_SETPOS, new, 0) };
            if id == ID_VOL {
                a.volume = new.min(100);
                set_text(item(h, ID_VOLTEXT), &alloc::format!("Vol {}", a.volume));
                if a.open {
                    mci(&alloc::format!("set mppm audio volume {} wait", a.volume));
                }
            } else if id == ID_POS && a.open {
                a.tracking = code == SB_SLIDERTRACK;
                if code != SB_SLIDERTRACK && code != SB_ENDSCROLL {
                    let playing = mci("status mppm mode wait").1 == "playing";
                    let to = (new as u64 * a.length as u64 / 1000) as u32;
                    mci(&alloc::format!("seek mppm to {to} wait"));
                    if playing {
                        mci_to("play mppm notify", h);
                    }
                    update_position();
                }
            }
            0
        }
        MM_MCINOTIFY => {
            update_position();
            0
        }
        WM_CLOSE => {
            close_media();
            unsafe { WinPostMsg(h, WM_QUIT, 0, 0) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn main(s: &os2::Startup) -> u32 {
    let args = args(s);
    let file = args.first().cloned();
    let Some(pm) = pm_start() else {
        os2::print("MPPM needs Presentation Manager.\r\n");
        return 1;
    };
    let kind = file.as_deref().map_or(Kind::None, kind_of);
    let (cw, ch) = match kind {
        Kind::Movie | Kind::Image => (560, 460),
        _ => (520, BAR_H + 30),
    };
    unsafe { WinRegisterClass(pm.hab, VIDEO_CLASS.as_ptr(), video_proc, 0, 0) };
    let (frame, client) = main_window(&pm, CLASS, client_proc, "Media Player", cw, ch, true);
    let video = unsafe { WinCreateWindow(client, VIDEO_CLASS.as_ptr(), b"\0".as_ptr(), 0, 6, BAR_H, cw - 12, ch - BAR_H - 6, client, HWND_TOP, ID_VIDEO, core::ptr::null(), core::ptr::null()) };
    unsafe { *core::ptr::addr_of_mut!(APP) = Some(App { hab: pm.hab, frame, client, video, file: String::new(), kind: Kind::None, open: false, length: 0, tracking: false, volume: 100, image: None }) };
    button(client, "<<", ID_REWIND, 0, 0, 10, 10);
    button(client, "Play", ID_PLAY, 0, 0, 10, 10);
    button(client, "Pause", ID_PAUSE, 0, 0, 10, 10);
    button(client, "Stop", ID_STOP, 0, 0, 10, 10);
    button(client, ">>", ID_FORWARD, 0, 0, 10, 10);
    button(client, "~Open...", ID_OPEN, 0, 0, 10, 10);
    scrollbar(client, false, ID_POS, 0, 0, 10, 10);
    let vol = scrollbar(client, false, ID_VOL, 0, 0, 10, 10);
    set_scroll(vol, 100, 100, 10);
    label(client, "Vol 100", ID_VOLTEXT, 0, 0, 10, 10, DT_RIGHT);
    label(client, "0:00.0 / 0:00.0", ID_TIME, 0, 0, 10, 10, DT_LEFT);
    label(client, "", ID_INFO, 0, 0, 10, 10, DT_LEFT);
    layout();
    show(frame);
    match &file {
        Some(f) => load(f),
        None => command(ID_OPEN),
    }
    if app().open && app().kind != Kind::Image {
        command(ID_PLAY);
    }
    unsafe { WinStartTimer(pm.hab, client, 1, 250) };
    pm.run();
    close_media();
    let _ = app().hab;
    unsafe { WinDestroyWindow(frame) };
    0
}
