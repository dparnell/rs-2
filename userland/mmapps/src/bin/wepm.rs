//! WEPM: the Digital Audio editor (what an audio file's Edit view opens).
//!
//! `WEPM [file]` reads a sound through MMIO (WAV, and AU, SND, VOC, AIFF,
//! 8SVX through their I/O procedures) and shows its waveform. Drag with
//! the mouse to select; Play plays the selection (or all of it), Record
//! records from the sound card's input (MDM's wave audio device) and
//! inserts it where the cursor is; Cut, Copy, Paste and Delete edit the
//! samples; Save writes the file in its format through MMIO (Save as...
//! takes the format from the new name's extension). Without a file it
//! starts with an empty 22050 Hz 16-bit mono sound.

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
    #[link_name = "MMIO.29"]
    fn mmioWrite(h: u32, buf: *const u8, n: i32) -> i32;
    #[link_name = "MMIO.32"]
    fn mmioClose(h: u32, flags: u32) -> u32;
    #[link_name = "MMIO.77"]
    fn mmioGetHeader(h: u32, hdr: *mut u8, len: i32, read: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "MMIO.78"]
    fn mmioSetHeader(h: u32, hdr: *const u8, len: i32, written: *mut i32, r: u32, flags: u32) -> u32;
    #[link_name = "PMWIN.852"]
    fn WinSetCapture(desk: u32, h: u32) -> u32;
}

const CLASS: &[u8] = b"Rs2AudioEditor\0";
const WAVE_CLASS: &[u8] = b"Rs2AudioEditorWave\0";
const ID_PLAY: u32 = 10;
const ID_STOP: u32 = 11;
const ID_RECORD: u32 = 12;
const ID_CUT: u32 = 13;
const ID_COPY: u32 = 14;
const ID_PASTE: u32 = 15;
const ID_DELETE: u32 = 16;
const ID_OPEN: u32 = 17;
const ID_SAVE: u32 = 18;
const ID_SAVEAS: u32 = 19;
const ID_NEW: u32 = 20;
const ID_INFO: u32 = 30;
const ID_WAVE: u32 = 31;

#[derive(Clone, Copy, PartialEq)]
struct Format {
    channels: u16,
    rate: u32,
    bits: u16,
}

impl Format {
    fn align(&self) -> usize {
        (self.channels as usize * self.bits as usize / 8).max(1)
    }
}

struct App {
    frame: u32,
    client: u32,
    wave: u32,
    file: String,
    fmt: Format,
    data: Vec<u8>,
    /// Selection, in frames: (anchor, cursor).
    sel: (usize, usize),
    clip: Vec<u8>,
    dragging: bool,
    playing: bool,
    recording: bool,
    /// Where the playing selection starts (frames).
    play_from: usize,
    play_pos: Option<usize>,
    dirty: bool,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn frames(a: &App) -> usize {
    a.data.len() / a.fmt.align()
}

fn selection(a: &App) -> (usize, usize) {
    let (x, y) = a.sel;
    (x.min(y), x.max(y))
}

/// Sample `i` of channel 0 as -32768..32767.
fn sample(a: &App, i: usize) -> i32 {
    let o = i * a.fmt.align();
    if a.fmt.bits == 8 {
        (a.data[o] as i32 - 128) << 8
    } else {
        i16::from_le_bytes([a.data[o], a.data[o + 1]]) as i32
    }
}

fn mmioinfo(translate: bool, fcc: Option<&[u8; 4]>) -> Vec<u8> {
    let mut i = vec![0u8; 80];
    if let Some(f) = fcc {
        i[4..8].copy_from_slice(f);
    }
    if translate {
        i[64..68].copy_from_slice(&3u32.to_le_bytes());
    }
    i
}

fn load(path: &str) -> Result<(Format, Vec<u8>), String> {
    let name = zs(path);
    let mut info = mmioinfo(true, None);
    let h = unsafe { mmioOpen(name.as_ptr(), info.as_mut_ptr(), 4) };
    if h == 0 {
        return Err(alloc::format!("{path} cannot be opened as a sound."));
    }
    let mut hdr = [0u8; 40];
    let mut n = 0;
    unsafe { mmioGetHeader(h, hdr.as_mut_ptr(), 40, &mut n, 0, 0) };
    let fmt = Format {
        channels: u16::from_le_bytes([hdr[14], hdr[15]]),
        rate: u32::from_le_bytes(hdr[16..20].try_into().unwrap()),
        bits: u16::from_le_bytes([hdr[26], hdr[27]]),
    };
    if n < 40 || u16::from_le_bytes([hdr[12], hdr[13]]) != 1 || !matches!(fmt.bits, 8 | 16) || fmt.channels == 0 {
        unsafe { mmioClose(h, 0) };
        return Err(String::from("Only PCM sounds (8 or 16 bits) can be edited."));
    }
    let mut data = Vec::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let r = unsafe { mmioRead(h, buf.as_mut_ptr(), buf.len() as i32) };
        if r <= 0 {
            break;
        }
        data.extend_from_slice(&buf[..r as usize]);
    }
    unsafe { mmioClose(h, 0) };
    let whole = data.len() / fmt.align() * fmt.align();
    data.truncate(whole);
    Ok((fmt, data))
}

/// Write PCM through MMIO, in the format of the name's extension.
fn save_as(path: &str, fmt: Format, data: &[u8]) -> bool {
    let fcc: &[u8; 4] = match extension(path).as_str() {
        "AU" | "_AU" => b"AU  ",
        "SND" => b"SND ",
        "VOC" => b"VOC ",
        "AIF" | "AIFF" => b"AIFF",
        _ => b"WAVE",
    };
    let name = zs(path);
    let mut info = mmioinfo(true, Some(fcc));
    let h = unsafe { mmioOpen(name.as_ptr(), info.as_mut_ptr(), 1 | 8) };
    if h == 0 {
        return false;
    }
    let mut hdr = [0u8; 40];
    hdr[0..4].copy_from_slice(&40u32.to_le_bytes());
    hdr[8..12].copy_from_slice(&2u32.to_le_bytes());
    hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
    hdr[14..16].copy_from_slice(&fmt.channels.to_le_bytes());
    hdr[16..20].copy_from_slice(&fmt.rate.to_le_bytes());
    hdr[20..24].copy_from_slice(&(fmt.rate * fmt.align() as u32).to_le_bytes());
    hdr[24..26].copy_from_slice(&(fmt.align() as u16).to_le_bytes());
    hdr[26..28].copy_from_slice(&fmt.bits.to_le_bytes());
    let mut n = 0;
    unsafe { mmioSetHeader(h, hdr.as_ptr(), 40, &mut n, 0, 0) };
    let w = unsafe { mmioWrite(h, data.as_ptr(), data.len() as i32) };
    let c = unsafe { mmioClose(h, 0) };
    w == data.len() as i32 && c == 0
}

fn mmbase() -> String {
    let mut dir: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"MMBASE\0".as_ptr(), &mut dir) } == 0 && !dir.is_null() {
        let s = text_of(unsafe { core::slice::from_raw_parts(dir, 260) });
        let d = s.split(';').next().unwrap_or("").trim_end_matches('\\');
        if !d.is_empty() {
            return String::from(d);
        }
    }
    String::from("C:\\MMOS2")
}

fn ms(a: &App, f: usize) -> u32 {
    (f as u64 * 1000 / a.fmt.rate.max(1) as u64) as u32
}

fn info() {
    let a = app();
    let (s, e) = selection(a);
    let n = frames(a);
    let what = if a.recording {
        String::from("Recording...")
    } else if s != e {
        alloc::format!("Selected {} - {} ({})", fmt_ms(ms(a, s)), fmt_ms(ms(a, e)), fmt_ms(ms(a, e - s)))
    } else {
        alloc::format!("Cursor {}", fmt_ms(ms(a, s)))
    };
    let t = alloc::format!(
        "{} Hz, {} bits, {} - length {}   {what}{}",
        a.fmt.rate,
        a.fmt.bits,
        if a.fmt.channels == 1 { "mono" } else { "stereo" },
        fmt_ms(ms(a, n)),
        if a.dirty { "  (changed)" } else { "" }
    );
    set_text(item(a.client, ID_INFO), &t);
    let title = alloc::format!("Digital Audio - {}", if a.file.is_empty() { "(untitled)" } else { base_name(&a.file) });
    set_text(a.frame, &title);
    unsafe { WinInvalidateRect(a.wave, core::ptr::null(), 0) };
}

fn stop_playing() {
    let a = app();
    if a.playing {
        mci("close wepm wait");
        a.playing = false;
        a.play_pos = None;
        let p = zs(&alloc::format!("{}\\WEPMPLAY.WAV", mmbase()));
        unsafe { os2::DosDelete(p.as_ptr()) };
    }
}

fn play() {
    stop_playing();
    let a = app();
    let (mut s, mut e) = selection(a);
    if s == e {
        s = if s >= frames(a) { 0 } else { s };
        e = frames(a);
    }
    if s >= e {
        return;
    }
    let al = a.fmt.align();
    let path = alloc::format!("{}\\WEPMPLAY.WAV", mmbase());
    if !save_as(&path, a.fmt, &a.data[s * al..e * al]) {
        message_box(a.frame, "Digital Audio", "The sound cannot be played (no temporary file).", true);
        return;
    }
    let (rc, _) = mci(&alloc::format!("open \"{path}\" alias wepm wait"));
    if rc != 0 {
        message_box(a.frame, "Digital Audio", &mci_error(rc), true);
        return;
    }
    mci("set wepm time format milliseconds wait");
    mci_to("play wepm notify", a.client);
    a.playing = true;
    a.play_from = s;
}

fn record() {
    let a = app();
    if a.recording {
        mci("stop wepmrec wait");
        let path = alloc::format!("{}\\WEPMREC.WAV", mmbase());
        mci(&alloc::format!("save wepmrec \"{path}\" wait"));
        mci("close wepmrec wait");
        a.recording = false;
        set_text(item(a.client, ID_RECORD), "~Record");
        if let Ok((f, d)) = load(&path) {
            if f == a.fmt {
                let (s, e) = selection(a);
                let al = a.fmt.align();
                a.data.splice(s * al..e * al, d.iter().copied());
                let n = d.len() / al;
                a.sel = (s, s + n);
                a.dirty = true;
            }
        }
        let p = zs(&path);
        unsafe { os2::DosDelete(p.as_ptr()) };
        info();
        return;
    }
    stop_playing();
    let (rc, _) = mci("open new type waveaudio alias wepmrec wait");
    if rc != 0 {
        message_box(a.frame, "Digital Audio", &alloc::format!("Recording is not possible: {}", mci_error(rc)), true);
        return;
    }
    mci(&alloc::format!("set wepmrec samplespersec {} bitspersample {} channels {} wait", a.fmt.rate, a.fmt.bits, a.fmt.channels));
    let (rc, _) = mci("record wepmrec");
    if rc != 0 {
        mci("close wepmrec wait");
        message_box(a.frame, "Digital Audio", &mci_error(rc), true);
        return;
    }
    a.recording = true;
    set_text(item(a.client, ID_RECORD), "Stop ~Rec");
    info();
}

fn command(id: u32) {
    let a = app();
    let al = a.fmt.align();
    let (s, e) = selection(a);
    match id {
        ID_PLAY => play(),
        ID_STOP => {
            stop_playing();
            if a.recording {
                record();
            }
        }
        ID_RECORD => record(),
        ID_COPY | ID_CUT if s != e => {
            a.clip = a.data[s * al..e * al].to_vec();
            if id == ID_CUT {
                a.data.drain(s * al..e * al);
                a.sel = (s, s);
                a.dirty = true;
            }
        }
        ID_DELETE if s != e => {
            a.data.drain(s * al..e * al);
            a.sel = (s, s);
            a.dirty = true;
        }
        ID_PASTE if !a.clip.is_empty() => {
            let n = a.clip.len() / al;
            a.data.splice(s * al..e * al, a.clip.iter().copied());
            a.sel = (s, s + n);
            a.dirty = true;
        }
        ID_NEW => {
            stop_playing();
            a.file.clear();
            a.data.clear();
            a.sel = (0, 0);
            a.dirty = false;
        }
        ID_OPEN => {
            if let Some(f) = file_dialog(a.frame, "Digital Audio - Open", "*.*", false) {
                open(&f);
            }
        }
        ID_SAVE | ID_SAVEAS => {
            let name = if id == ID_SAVE && !a.file.is_empty() { Some(a.file.clone()) } else { file_dialog(a.frame, "Digital Audio - Save as", "*.WAV", true) };
            if let Some(n) = name {
                if save_as(&n, a.fmt, &a.data) {
                    a.file = n;
                    a.dirty = false;
                } else {
                    message_box(a.frame, "Digital Audio", &alloc::format!("{n} cannot be written."), true);
                }
            }
        }
        _ => {}
    }
    info();
}

fn open(path: &str) {
    stop_playing();
    let a = app();
    match load(path) {
        Ok((f, d)) => {
            a.fmt = f;
            a.data = d;
            a.file = String::from(path);
            a.sel = (0, 0);
            a.dirty = false;
        }
        // An empty file (a new one from the Wave Audio template) is a new
        // sound to save under its name.
        Err(_) if read_file(path).is_some_and(|d| d.is_empty()) => {
            a.data.clear();
            a.file = String::from(path);
            a.sel = (0, 0);
            a.dirty = false;
        }
        Err(e) => message_box(a.frame, "Digital Audio", &e, true),
    }
    info();
}

/// The frame under x in the wave window.
fn frame_at(x: i32) -> usize {
    let a = app();
    let w = window_rect(a.wave).r.max(1);
    let n = frames(a);
    ((x.clamp(0, w) as u64 * n as u64) / w as u64) as usize
}

fn x_of(a: &App, f: usize, w: i32) -> i32 {
    let n = frames(a).max(1);
    (f as u64 * w as u64 / n as u64) as i32
}

extern "C" fn wave_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if unsafe { (*core::ptr::addr_of!(APP)).is_none() } {
        return unsafe { WinDefWindowProc(h, msg, mp1, mp2) };
    }
    let a = app();
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            let wr = window_rect(h);
            let (w, ht) = (wr.r, wr.t);
            fill(hps, &wr, CLR_BLACK);
            let (s, e) = selection(a);
            if s != e {
                rect(hps, x_of(a, s, w), 0, x_of(a, e, w), ht - 1, CLR_DARKBLUE, true);
            }
            let mid = ht / 2;
            line(hps, 0, mid, w, mid, CLR_DARKGRAY);
            let n = frames(a);
            if n > 0 {
                // Each column: the lowest and highest sample under it.
                for x in 0..w {
                    let f0 = (x as u64 * n as u64 / w as u64) as usize;
                    let f1 = (((x + 1) as u64 * n as u64 / w as u64) as usize).max(f0 + 1).min(n);
                    let step = ((f1 - f0) / 64).max(1);
                    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                    let mut f = f0;
                    while f < f1 {
                        let v = sample(a, f);
                        lo = lo.min(v);
                        hi = hi.max(v);
                        f += step;
                    }
                    let y0 = mid + lo * (ht / 2 - 2) / 32768;
                    let y1 = mid + hi * (ht / 2 - 2) / 32768;
                    line(hps, x, y0, x, y1.max(y0 + 1), CLR_GREEN);
                }
            }
            let cx = x_of(a, a.sel.1, w);
            line(hps, cx, 0, cx, ht, CLR_YELLOW);
            if let Some(p) = a.play_pos {
                let px = x_of(a, p, w);
                line(hps, px, 0, px, ht, CLR_RED);
            }
            unsafe { WinEndPaint(hps) };
            0
        }
        WM_BUTTON1DOWN => {
            let x = (mp1 & 0xffff) as i16 as i32;
            let f = frame_at(x);
            a.sel = (f, f);
            a.dragging = true;
            unsafe { WinSetCapture(HWND_DESKTOP, h) };
            info();
            1
        }
        WM_MOUSEMOVE if a.dragging => {
            let x = (mp1 & 0xffff) as i16 as i32;
            a.sel.1 = frame_at(x);
            info();
            unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
        }
        WM_BUTTON1UP => {
            a.dragging = false;
            unsafe { WinSetCapture(HWND_DESKTOP, 0) };
            1
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_plain(h, CLR_PALEGRAY);
            0
        }
        WM_SIZE => {
            if unsafe { (*core::ptr::addr_of!(APP)).is_some() } {
                let a = app();
                let r = window_rect(h);
                unsafe { WinSetWindowPos(a.wave, HWND_TOP, 6, 80, r.r - 12, r.t - 86, SWP_SIZE | SWP_MOVE) };
            }
            0
        }
        WM_COMMAND => {
            command(mp1 & 0xffff);
            0
        }
        WM_TIMER => {
            let a = app();
            if a.playing {
                let p = mci_num("status wepm position wait");
                a.play_pos = Some(a.play_from + (p as u64 * a.fmt.rate as u64 / 1000) as usize);
                if mci("status wepm mode wait").1 != "playing" {
                    stop_playing();
                }
                unsafe { WinInvalidateRect(a.wave, core::ptr::null(), 0) };
            }
            0
        }
        MM_MCINOTIFY => 0,
        WM_CLOSE => {
            stop_playing();
            unsafe { WinPostMsg(h, WM_QUIT, 0, 0) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn main(s: &os2::Startup) -> u32 {
    let args = args(s);
    let Some(pm) = pm_start() else {
        os2::print("WEPM needs Presentation Manager.\r\n");
        return 1;
    };
    unsafe { WinRegisterClass(pm.hab, WAVE_CLASS.as_ptr(), wave_proc, 0, 0) };
    let (cw, ch) = (700, 330);
    let (frame, client) = main_window(&pm, CLASS, client_proc, "Digital Audio", cw, ch, true);
    let wave = unsafe { WinCreateWindow(client, WAVE_CLASS.as_ptr(), b"\0".as_ptr(), WS_VISIBLE, 6, 80, cw - 12, ch - 86, client, HWND_TOP, ID_WAVE, core::ptr::null(), core::ptr::null()) };
    let fmt = Format { channels: 1, rate: 22050, bits: 16 };
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App { frame, client, wave, file: String::new(), fmt, data: Vec::new(), sel: (0, 0), clip: Vec::new(), dragging: false, playing: false, recording: false, play_from: 0, play_pos: None, dirty: false })
    };
    let buttons = [
        ("~Play", ID_PLAY),
        ("~Stop", ID_STOP),
        ("~Record", ID_RECORD),
        ("Cu~t", ID_CUT),
        ("~Copy", ID_COPY),
        ("P~aste", ID_PASTE),
        ("~Delete", ID_DELETE),
        ("~New", ID_NEW),
        ("~Open...", ID_OPEN),
        ("Sa~ve", ID_SAVE),
        ("Save ~as...", ID_SAVEAS),
    ];
    for (i, (t, id)) in buttons.iter().enumerate() {
        button(client, t, *id, 6 + i as i32 * 62, 40, 60, 30);
    }
    label(client, "", ID_INFO, 8, 6, cw - 16, 28, DT_LEFT);
    show(frame);
    match args.first() {
        Some(f) => open(f),
        None => info(),
    }
    unsafe { WinStartTimer(pm.hab, client, 1, 200) };
    pm.run();
    stop_playing();
    unsafe { WinDestroyWindow(frame) };
    0
}
