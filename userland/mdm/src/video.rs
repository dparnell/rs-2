//! The digital video device: FLI/FLC animations (the opening movies of
//! OS/2's Solitaire, Mahjongg and Chess). Frames are drawn into the
//! application's window (WINDOW ... HANDLE) or into MDM's own video window,
//! which a thread of MDM's runs with its own message queue.

use alloc::string::String;
use alloc::vec::Vec;
use mmfmt::flc::Decoder;

use crate::dev::{self, *};
use crate::req::*;
use crate::sys::*;
use mci::*;

pub struct Video {
    dec: Option<Decoder<Vec<u8>>>,
    /// The frame shown (0-based), and how many there are.
    pub frame: u32,
    pub frames: u32,
    pub width: i32,
    pub height: i32,
    delay_ms: u32,
    /// The image shown: 8-bit pixels, top row first, and its palette.
    image: Vec<u8>,
    palette: [[u8; 3]; 256],
    /// The application's window (WINDOW HANDLE), or 0.
    pub app_hwnd: u32,
    /// MDM's own video window (frame, client), when it has one.
    own: (u32, u32),
    /// PUT DESTINATION (x, y, cx, cy in the window), or the whole window.
    dest: Option<[i32; 4]>,
    title: Option<String>,
    /// Where PLAY stops (frame), and FAST.
    end: u32,
    fast: bool,
}

impl Video {
    pub fn new(element: Option<&str>, parent: u32) -> Result<Video, u32> {
        let mut v = Video {
            dec: None,
            frame: 0,
            frames: 0,
            width: 0,
            height: 0,
            delay_ms: 70,
            image: Vec::new(),
            palette: [[0; 3]; 256],
            app_hwnd: 0,
            own: (0, 0),
            dest: None,
            title: None,
            end: u32::MAX,
            fast: false,
        };
        let _ = parent;
        if let Some(e) = element {
            v.load(e)?;
        }
        Ok(v)
    }

    pub fn load(&mut self, name: &str) -> Result<(), u32> {
        let data = read_file(name).ok_or(MCIERR_FILE_NOT_FOUND)?;
        let mut d = Decoder::new(data).map_err(|_| MCIERR_INVALID_MEDIA_TYPE)?;
        self.frames = d.frame_count() as u32;
        self.width = d.width() as i32;
        self.height = d.height() as i32;
        self.delay_ms = d.header().delay_ms().max(10);
        // The first frame, ready to show (CUE, or a window painted before
        // PLAY).
        if d.next_frame().is_ok() {
            self.image = d.frame().to_vec();
            self.palette = *d.palette();
        }
        self.frame = 0;
        self.dec = Some(d);
        let base = name.rsplit(['\\', '/', ':']).next().unwrap_or(name);
        if self.title.is_none() {
            self.title = Some(String::from(base));
        }
        Ok(())
    }

    /// Back to the first frame.
    fn rewind(&mut self) {
        if let Some(d) = &mut self.dec {
            d.rewind();
            if d.next_frame().is_ok() {
                self.image = d.frame().to_vec();
                self.palette = *d.palette();
            }
        }
        self.frame = 0;
    }

    /// Decode the next frame (the animation loops after its last).
    fn advance(&mut self) -> bool {
        let Some(d) = &mut self.dec else { return false };
        if d.next_frame().is_err() {
            return false;
        }
        self.image = d.frame().to_vec();
        self.palette = *d.palette();
        self.frame = (self.frame + 1) % self.frames.max(1);
        true
    }

    /// The window frames go to.
    fn target(&self) -> u32 {
        if self.app_hwnd != 0 { self.app_hwnd } else { self.own.1 }
    }

    pub fn close(&mut self) {
        if self.own.0 != 0 {
            unsafe { WinPostMsg(self.own.1, WM_USER_DESTROY, 0, 0) };
            self.own = (0, 0);
        }
    }

    fn to_time(&self, frame: u32, f: u32) -> u32 {
        let ms = frame as u64 * self.delay_ms as u64;
        (match f {
            MCI_FORMAT_FRAMES => frame as u64,
            MCI_FORMAT_MILLISECONDS => ms,
            _ => ms * 3,
        }) as u32
    }

    fn from_time(&self, t: u32, f: u32) -> u32 {
        let d = self.delay_ms.max(1) as u64;
        (match f {
            MCI_FORMAT_FRAMES => t as u64,
            MCI_FORMAT_MILLISECONDS => t as u64 / d,
            _ => t as u64 / 3 / d,
        }) as u32
    }
}

const WM_USER_DESTROY: u32 = WM_USER + 0x60;
const WM_USER_SHOW: u32 = WM_USER + 0x61;

fn video(d: &mut Device) -> &mut Video {
    match &mut d.kind {
        Kind::Video(v) => v,
        _ => unreachable!(),
    }
}

/// Draw the device's current image into its window.
fn draw(hps: u32, win: u32, v: &Video) {
    if v.image.is_empty() || v.width <= 0 || v.height <= 0 {
        return;
    }
    let (w, h) = (v.width as usize, v.height as usize);
    let stride = (w + 3) & !3;
    // PM bitmaps are bottom-up.
    let mut bits = alloc::vec![0u8; stride * h];
    for y in 0..h {
        let src = &v.image[y * w..y * w + w];
        bits[(h - 1 - y) * stride..(h - 1 - y) * stride + w].copy_from_slice(src);
    }
    // BITMAPINFO2 (16-byte header) with 256 RGB2 entries.
    let mut bmi = alloc::vec![0u8; 16 + 256 * 4];
    bmi[0..4].copy_from_slice(&16u32.to_le_bytes());
    bmi[4..8].copy_from_slice(&(w as u32).to_le_bytes());
    bmi[8..12].copy_from_slice(&(h as u32).to_le_bytes());
    bmi[12..14].copy_from_slice(&1u16.to_le_bytes());
    bmi[14..16].copy_from_slice(&8u16.to_le_bytes());
    for (i, c) in v.palette.iter().enumerate() {
        bmi[16 + i * 4] = c[2];
        bmi[17 + i * 4] = c[1];
        bmi[18 + i * 4] = c[0];
    }
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(win, &mut r) };
    let [x, y, cx, cy] = v.dest.unwrap_or([0, 0, r.r - r.l, r.t - r.b]);
    let (cx, cy) = (if cx <= 0 { v.width } else { cx }, if cy <= 0 { v.height } else { cy });
    let pts = [PointL { x, y }, PointL { x: x + cx - 1, y: y + cy - 1 }, PointL { x: 0, y: 0 }, PointL { x: v.width, y: v.height }];
    // ROP_SRCCOPY, BBO_IGNORE.
    unsafe { GpiDrawBits(hps, bits.as_ptr(), bmi.as_ptr(), 4, pts.as_ptr(), 0xcc, 2) };
}

fn show_frame(id: u16) {
    let _g = LOCK.hold();
    let Some(d) = table().iter_mut().find(|d| d.id == id) else { return };
    let v = video(d);
    let win = v.target();
    if win == 0 {
        return;
    }
    let hps = unsafe { WinGetPS(win) };
    if hps != 0 {
        draw(hps, win, v);
        unsafe { WinReleasePS(hps) };
    }
}

// ---- MDM's own video window ----

static mut REGISTERED: bool = false;

extern "C" fn video_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe { WinFillRect(hps, &r, 0) };
            {
                let _g = LOCK.hold();
                if let Some(d) = table().iter_mut().find(|d| matches!(&d.kind, Kind::Video(v) if v.own.1 == h)) {
                    draw(hps, h, video(d));
                }
            }
            unsafe { WinEndPaint(hps) };
            0
        }
        // The user closed the window: the movie stops there.
        WM_CLOSE => {
            let id = {
                let _g = LOCK.hold();
                table().iter().find(|d| matches!(&d.kind, Kind::Video(v) if v.own.1 == h)).map(|d| d.id)
            };
            if let Some(id) = id {
                stop(id, true);
            }
            unsafe { WinShowWindow(WinQueryWindowParent(h), 0) };
            0
        }
        WM_USER_SHOW => {
            unsafe { WinShowWindow(WinQueryWindowParent(h), mp1) };
            0
        }
        WM_USER_DESTROY => {
            unsafe { WinDestroyWindow(WinQueryWindowParent(h)) };
            0
        }
        WM_DESTROY => {
            unsafe { WinPostMsg(0, 0x2a, 0, 0) }; // WM_QUIT for this thread
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

unsafe extern "C" {
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(h: u32, cmd: i32) -> u32;
}

#[allow(non_snake_case)]
unsafe fn WinQueryWindowParent(h: u32) -> u32 {
    // QW_PARENT
    unsafe { WinQueryWindow(h, 5) }
}

/// (frame, client, size) handed from the window thread.
static mut NEW_WINDOW: (u32, u32) = (0, 0);

/// Start a thread with a message queue that owns a new video window of
/// `cx` x `cy` (client size) titled `title`; returns (frame, client).
fn create_window(cx: i32, cy: i32, title: &str) -> (u32, u32) {
    let mut sem = 0;
    unsafe { DosCreateEventSem(core::ptr::null(), &mut sem, 0, 0) };
    let mut t: Vec<u8> = title.chars().map(|c| c as u32 as u8).collect();
    t.push(0);
    let args = alloc::boxed::Box::new((cx, cy, t, sem));
    let p = alloc::boxed::Box::into_raw(args) as u32;
    let mut tid = 0;
    if unsafe { DosCreateThread(&mut tid, window_thread, p, 0, 65536) } != 0 {
        unsafe { DosCloseEventSem(sem) };
        return (0, 0);
    }
    unsafe {
        DosWaitEventSem(sem, 10_000);
        DosCloseEventSem(sem);
        core::ptr::read(core::ptr::addr_of!(NEW_WINDOW))
    }
}

unsafe extern "C" fn window_thread(p: u32) {
    let args = unsafe { alloc::boxed::Box::from_raw(p as *mut (i32, i32, Vec<u8>, u32)) };
    let (cx, cy, title, sem) = *args;
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    unsafe {
        if !core::ptr::read(core::ptr::addr_of!(REGISTERED)) {
            WinRegisterClass(hab, b"MDMVideo\0".as_ptr(), video_proc, 0, 0);
            *core::ptr::addr_of_mut!(REGISTERED) = true;
        }
    }
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_BORDER | FCF_TASKLIST
    let mut fcf = 0x1 | 0x2 | 0x10 | 0x200 | 0x800;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"MDMVideo\0".as_ptr(), title.as_ptr(), 0, 0, 0, &mut client) };
    if frame != 0 {
        let mut r = RectL { l: 0, b: 0, r: cx, t: cy };
        unsafe { WinCalcFrameRect(frame, &mut r, 0) };
        let (fw, fh) = (r.r - r.l, r.t - r.b);
        let (sw, sh) = unsafe { (WinQuerySysValue(HWND_DESKTOP, 20), WinQuerySysValue(HWND_DESKTOP, 21)) };
        // SWP_SIZE | SWP_MOVE | SWP_ZORDER | SWP_SHOW | SWP_ACTIVATE
        unsafe { WinSetWindowPos(frame, HWND_TOP, (sw - fw) / 2, (sh - fh) / 2, fw, fh, 0x1 | 0x2 | 0x4 | 0x8 | 0x80) };
    }
    unsafe {
        *core::ptr::addr_of_mut!(NEW_WINDOW) = (frame, client);
        DosPostEventSem(sem);
    }
    if frame != 0 {
        let mut q = QMsg::default();
        while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
            unsafe { WinDispatchMsg(hab, &q) };
        }
    }
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

/// MDM's window for a device that has no application window.
fn ensure_window(id: u16) {
    let need = with(id, |d| {
        let v = video(d);
        (v.app_hwnd == 0 && v.own.0 == 0).then(|| (v.width.max(64), v.height.max(48), v.title.clone().unwrap_or_default()))
    })
    .flatten();
    if let Some((cx, cy, title)) = need {
        let w = create_window(cx, cy, &title);
        with(id, |d| video(d).own = w);
    }
}

// ---- Commands ----

pub fn command(id: u16, r: &Req) -> Result<Reply, u32> {
    match r.msg {
        MCI_PLAY => play(id, r),
        MCI_STOP | MCI_PAUSE => {
            stop(id, r.msg == MCI_STOP);
            if r.msg == MCI_PAUSE {
                with(id, |d| d.mode = MCI_MODE_PAUSE);
            }
            Ok(Reply::None)
        }
        MCI_RESUME => {
            let paused = with(id, |d| d.mode == MCI_MODE_PAUSE).unwrap_or(false);
            if paused {
                let mut p = r.clone();
                p.flags &= !(MCI_FROM | MCI_TO | MCI_WAIT);
                return play(id, &p);
            }
            Ok(Reply::None)
        }
        MCI_SEEK => {
            stop(id, true);
            with(id, |d| {
                let tf = d.time_format;
                let v = video(d);
                if r.flags & MCI_TO_END != 0 {
                    while v.frame + 1 < v.frames && v.advance() {}
                } else {
                    let to = if r.flags & MCI_TO != 0 { v.from_time(r.to, tf) } else { 0 };
                    if to >= v.frames.max(1) {
                        return Err(MCIERR_OUTOFRANGE);
                    }
                    v.rewind();
                    while v.frame < to && v.advance() {}
                }
                Ok(())
            })
            .unwrap()?;
            show_frame(id);
            Ok(Reply::None)
        }
        MCI_LOAD => {
            stop(id, true);
            let name = r.element.clone().ok_or(MCIERR_MISSING_PARAMETER)?;
            with(id, |d| {
                d.element = Some(name.clone());
                video(d).load(&name)
            })
            .unwrap()?;
            Ok(Reply::None)
        }
        MCI_CUE => {
            if r.flags & MCI_CUE_OUTPUT != 0 || r.flags & MCI_CUE_INPUT == 0 {
                ensure_window(id);
                return Ok(Reply::None);
            }
            Err(MCIERR_UNSUPPORTED_FLAG)
        }
        MCI_WINDOW => {
            if r.flags & MCI_VID_WINDOW_HWND != 0 {
                let own = with(id, |d| {
                    let v = video(d);
                    v.app_hwnd = r.hwnd;
                    core::mem::replace(&mut v.own, (0, 0))
                })
                .unwrap();
                if own.0 != 0 {
                    unsafe { WinPostMsg(own.1, WM_USER_DESTROY, 0, 0) };
                }
            }
            if r.flags & MCI_VID_WINDOW_DEFAULT != 0 {
                with(id, |d| video(d).app_hwnd = 0);
            }
            if r.flags & MCI_VID_WINDOW_TEXT != 0 {
                let t = r.text.clone().unwrap_or_default();
                let own = with(id, |d| {
                    let v = video(d);
                    v.title = Some(t.clone());
                    v.own.0
                })
                .unwrap();
                if own != 0 {
                    let mut z: Vec<u8> = t.chars().map(|c| c as u32 as u8).collect();
                    z.push(0);
                    unsafe { WinSetWindowText(own, z.as_ptr()) };
                }
            }
            if r.flags & MCI_VID_WINDOW_STATE != 0 {
                let show = r.value & SWP_HIDE == 0;
                if show {
                    ensure_window(id);
                }
                let own = with(id, |d| video(d).own.1).unwrap();
                if own != 0 {
                    unsafe { WinPostMsg(own, WM_USER_SHOW, show as u32, 0) };
                }
            }
            Ok(Reply::None)
        }
        MCI_PUT => {
            if r.flags & (MCI_VID_PUT_DESTINATION | MCI_VID_PUT_RECT) != 0 {
                with(id, |d| video(d).dest = if r.flags & MCI_VID_PUT_RECT != 0 { Some(r.rect) } else { None });
            }
            Ok(Reply::None)
        }
        MCI_WHERE => with(id, |d| {
            let v = video(d);
            if r.flags & MCI_VID_WHERE_SOURCE != 0 {
                return Ok(Reply::Rect([0, 0, v.width, v.height]));
            }
            let win = v.target();
            let mut wr = RectL::default();
            if win != 0 {
                unsafe { WinQueryWindowRect(win, &mut wr) };
            }
            Ok(Reply::Rect(v.dest.unwrap_or([0, 0, wr.r - wr.l, wr.t - wr.b])))
        })
        .unwrap(),
        MCI_STATUS => with(id, |d| {
            let tf = d.time_format;
            let v = video(d);
            Ok(match r.item {
                MCI_STATUS_LENGTH => Reply::Num(v.to_time(v.frames, tf)),
                MCI_STATUS_POSITION => Reply::Num(v.to_time(v.frame, tf)),
                MCI_VID_STATUS_HWND => Reply::Num(v.target()),
                MCI_VID_STATUS_VIDEO_X_EXTENT => Reply::Num(v.width as u32),
                MCI_VID_STATUS_VIDEO_Y_EXTENT => Reply::Num(v.height as u32),
                MCI_VID_STATUS_NORMAL_RATE => Reply::Num(1000 / v.delay_ms.max(1)),
                MCI_STATUS_VOLUME => Reply::Num(0),
                _ => return Err(MCIERR_INVALID_ITEM_FLAG),
            })
        })
        .unwrap(),
        MCI_GETDEVCAPS => with(id, |d| {
            let v = video(d);
            Ok(match r.item {
                MCI_VID_GETDEVCAPS_VIDEO_X_EXTENT => Reply::Num(v.width as u32),
                MCI_VID_GETDEVCAPS_VIDEO_Y_EXTENT => Reply::Num(v.height as u32),
                MCI_VID_GETDEVCAPS_NORMAL_RATE => Reply::Num(1000 / v.delay_ms.max(1)),
                _ => return Err(MCIERR_INVALID_ITEM_FLAG),
            })
        })
        .unwrap(),
        MCI_SET => Ok(Reply::None),
        _ => Err(MCIERR_UNSUPPORTED_FUNCTION),
    }
}

fn stop(id: u16, abort: bool) {
    with(id, |d| {
        d.epoch = d.epoch.wrapping_add(1);
        if abort {
            d.finish(MCI_NOTIFY_ABORTED);
        }
        d.mode = MCI_MODE_STOP;
    });
}

fn play(id: u16, r: &Req) -> Result<Reply, u32> {
    let has = with(id, |d| video(d).dec.is_some()).ok_or(MCIERR_INVALID_DEVICE_ID)?;
    if !has {
        return Err(MCIERR_MISSING_PARAMETER);
    }
    ensure_window(id);
    let epoch = with(id, |d| -> Result<u32, u32> {
        let tf = d.time_format;
        d.epoch = d.epoch.wrapping_add(1);
        d.finish(MCI_NOTIFY_SUPERSEDED);
        let v = video(d);
        if r.flags & MCI_FROM != 0 {
            let to = v.from_time(r.from, tf);
            v.rewind();
            while v.frame < to && v.advance() {}
        } else if v.frame + 1 >= v.frames {
            v.rewind();
        }
        v.end = if r.flags & MCI_TO != 0 { v.from_time(r.to, tf) } else { v.frames.saturating_sub(1) };
        v.fast = r.flags & MCI_DGV_PLAY_FAST != 0;
        d.mode = MCI_MODE_PLAY;
        d.take_notify(r);
        Ok(d.epoch)
    })
    .unwrap()?;
    let param = id as u32 | (epoch & 0xffff) << 16;
    if r.flags & MCI_WAIT != 0 {
        unsafe { player(param) };
    } else {
        let mut tid = 0;
        if unsafe { DosCreateThread(&mut tid, player, param, 0, 65536) } != 0 {
            return Err(MCIERR_OUT_OF_MEMORY);
        }
    }
    Ok(Reply::None)
}

unsafe extern "C" fn player(param: u32) {
    let id = param as u16;
    let epoch = param >> 16;
    show_frame(id);
    let mut finished = false;
    loop {
        let step = with(id, |d| {
            if d.epoch & 0xffff != epoch {
                return None;
            }
            let v = video(d);
            if v.frame >= v.end || v.frame + 1 >= v.frames {
                return Some(None);
            }
            let delay = if v.fast { v.delay_ms / 2 } else { v.delay_ms };
            Some(Some(delay))
        })
        .flatten();
        let Some(step) = step else { break };
        let Some(delay) = step else {
            finished = true;
            break;
        };
        unsafe { DosSleep(delay) };
        let ok = with(id, |d| d.epoch & 0xffff == epoch && video(d).advance()).unwrap_or(false);
        if !ok {
            break;
        }
        show_frame(id);
    }
    let auto_close = with(id, |d| {
        if d.epoch & 0xffff != epoch || !finished {
            return false;
        }
        d.mode = MCI_MODE_STOP;
        d.finish(MCI_NOTIFY_SUCCESSFUL);
        d.auto_close
    })
    .unwrap_or(false);
    if auto_close {
        let _ = dev::close(id);
    }
}
