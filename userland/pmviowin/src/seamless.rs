//! Seamless Win-OS/2 (docs/WINOS2.md): a DOS session whose surface says
//! it is seamless shows no window of its own. Its Windows desktop is the
//! size of the PM screen and is never shown; each visible top-level
//! Windows window (menus, drop-downs and dialogs too) gets a borderless
//! PM frame at the same place that paints its part of the surface.
//! Frames of windows with a caption are in the window list.
//!
//! The VDM keeps the list of Windows' windows in the surface (VWIN's
//! messages from WINSHELD, crate `dos::vwin`); requests go back with
//! RS2API 407 (activate, close, place, minimise, restore, run). The
//! mouse goes to the session as positions on its screen, captured while
//! a button is down; keys go straight to the session while one of its
//! frames has the focus (Rs2PmVioWindow). A timer keeps activation and
//! minimised windows in step with what the user does on the PM side.
//!
//! While a mouse button is down, what Windows draws on its desktop
//! outside its windows (the outline of a window being moved or sized)
//! is shown inverted on the PM screen, a see-through overlay, and taken
//! away again as Windows erases it.

use alloc::vec::Vec;

use win16drv::surface::{self as sf, Surface};

use crate::surface::View;
use crate::*;

unsafe extern "C" {
    #[link_name = "RS2API.407"]
    fn Rs2VdmRequest(sid: u32, code: u32, a: i32, b: i32) -> u32;
    #[link_name = "PMWIN.799"]
    fn WinQueryActiveWindow(desktop: u32) -> u32;
    #[link_name = "PMWIN.775"]
    fn WinIsWindowVisible(h: u32) -> u32;
    #[link_name = "PMWIN.759"]
    fn WinGetScreenPS(desktop: u32) -> u32;
    #[link_name = "PMWIN.767"]
    fn WinInvertRect(hps: u32, r: *const RectL) -> u32;
    #[link_name = "PMWIN.848"]
    fn WinReleasePS(hps: u32) -> u32;
}

/// The window class of the frames' clients.
pub const CLASS: &[u8] = b"Rs2Seamless\0";
const TID_SEAMLESS: u32 = 0x5e;

/// A Windows window shown on the PM desktop.
pub struct SWin {
    pub hwnd: u16,
    pub frame: u32,
    pub client: u32,
    /// Its rectangle on the Windows screen.
    pub rect: (i32, i32, i32, i32),
    pub flags: u16,
    pub title: Vec<u8>,
}

/// A seamless session's windows.
pub struct Seamless {
    pub sid: u32,
    pub wins: Vec<SWin>,
    win_gen: u32,
    /// Windows' active window as last seen, and PM's.
    active_win: u16,
    active_pm: u32,
    pub buttons: u32,
    timer: bool,
    /// The overlay: a bit per surface pixel inverted on the PM screen.
    overlay: Vec<u8>,
    overlay_on: bool,
}

impl Seamless {
    pub fn new(sid: u32) -> Seamless {
        Seamless { sid, wins: Vec::new(), win_gen: u32::MAX, active_win: 0, active_pm: 0, buttons: 0, timer: false, overlay: Vec::new(), overlay_on: false }
    }

    pub fn by_client(&self, h: u32) -> Option<usize> {
        self.wins.iter().position(|w| w.client == h)
    }

    fn request(&self, code: u32, hwnd: u16, b: i32) {
        unsafe { Rs2VdmRequest(self.sid, code, hwnd as i32, b) };
    }
}

fn screen_h() -> i32 {
    unsafe { WinQuerySysValue(HWND_DESKTOP, 21) }
}

/// PM position of a Windows rectangle.
fn pm_rect(r: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    (r.0, screen_h() - (r.1 + r.3), r.2, r.3)
}

fn read_windows(s: Surface) -> Vec<(u16, u16, (i32, i32, i32, i32), Vec<u8>)> {
    let n = (unsafe { s.get(sf::WIN_COUNT) } as usize).min(sf::MAX_WINDOWS);
    (0..n)
        .map(|i| {
            let e = sf::WINDOWS + i * sf::WINDOW_SIZE;
            unsafe {
                let hf = s.get(e + sf::WIN_HWND);
                let r = (s.get(e + sf::WIN_X) as i32, s.get(e + sf::WIN_Y) as i32, s.get(e + sf::WIN_W) as i32, s.get(e + sf::WIN_H) as i32);
                let t = s.bytes(e + sf::WIN_TITLE, sf::WIN_TITLE_LEN);
                let title = t[..t.iter().position(|&c| c == 0).unwrap_or(t.len())].to_vec();
                (hf as u16, (hf >> 16) as u16, r, title)
            }
        })
        .collect()
}

/// Bring the frames in step with Windows' window list (`view` is the
/// session's surface).
pub fn sync(sm: &mut Seamless, view: &View, object: u32) {
    if !sm.timer {
        sm.timer = true;
        unsafe { WinStartTimer(HAB.load(Ordering::Relaxed), object, TID_SEAMLESS, 150) };
    }
    let g = unsafe { view.s.get(sf::WIN_GEN) };
    if g == sm.win_gen {
        return;
    }
    sm.win_gen = g;
    let list = read_windows(view.s);
    // Frames of windows that went away or are hidden.
    let shown = |f: u16| f & sf::WF_VISIBLE != 0 && f & sf::WF_MINIMIZED == 0;
    sm.wins.retain(|w| {
        let keep = list.iter().any(|(h, f, r, _)| *h == w.hwnd && shown(*f) && r.2 > 0 && r.3 > 0);
        if !keep {
            unsafe {
                Rs2PmVioWindow(w.client, 0, 0);
                WinDestroyWindow(w.frame);
            }
        }
        keep
    });
    let ours_active = sm.wins.iter().any(|w| w.frame == unsafe { WinQueryActiveWindow(HWND_DESKTOP) });
    let mut active = 0u16;
    for (hwnd, flags, r, title) in &list {
        if flags & sf::WF_ACTIVE != 0 {
            active = *hwnd;
        }
        if !shown(*flags) || r.2 <= 0 || r.3 <= 0 {
            continue;
        }
        let (x, y, w, h) = pm_rect(*r);
        match sm.wins.iter().position(|o| o.hwnd == *hwnd) {
            Some(i) => {
                let o = &mut sm.wins[i];
                if o.rect != *r {
                    o.rect = *r;
                    unsafe {
                        WinSetWindowPos(o.frame, 0, x, y, w, h, SWP_SIZE | SWP_MOVE);
                        WinInvalidateRect(o.client, null(), 0);
                    }
                }
                if o.title != *title {
                    o.title = title.clone();
                    let mut t = title.clone();
                    t.push(0);
                    unsafe { WinSetWindowText(o.frame, t.as_ptr()) };
                }
                o.flags = *flags;
            }
            None => {
                // A caption (WS_CAPTION) puts it in the window list.
                let e = list.iter().position(|l| l.0 == *hwnd).unwrap();
                let style = unsafe { view.s.get(sf::WINDOWS + e * sf::WINDOW_SIZE + sf::WIN_STYLE) };
                let mut fcf: u32 = if style & 0x00c0_0000 == 0x00c0_0000 { 0x800 } else { 0 };
                let mut t = title.clone();
                t.push(0);
                let mut client = 0;
                let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), t.as_ptr(), 0, 0, *hwnd as u32, &mut client) };
                if frame == 0 {
                    continue;
                }
                unsafe {
                    WinSetWindowPos(frame, HWND_TOP, x, y, w, h, SWP_SIZE | SWP_MOVE | SWP_ZORDER | SWP_SHOW | SWP_NOACTIVATE);
                    Rs2PmVioWindow(client, sm.sid, VW_DIRECT);
                }
                sm.wins.push(SWin { hwnd: *hwnd, frame, client, rect: *r, flags: *flags, title: title.clone() });
            }
        }
    }
    // Z-order: Windows' order among our frames, when the user works in
    // Windows (one of ours is active, or Windows activated a window).
    if ours_active || active != sm.active_win {
        for (hwnd, ..) in &list {
            if let Some(o) = sm.wins.iter().find(|o| o.hwnd == *hwnd) {
                unsafe { WinSetWindowPos(o.frame, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
            }
        }
    }
    // Windows activated a window: PM follows.
    if active != sm.active_win {
        sm.active_win = active;
        if let Some(o) = sm.wins.iter().find(|o| o.hwnd == active) {
            unsafe {
                WinSetActiveWindow(HWND_DESKTOP, o.frame);
                WinSetFocus(HWND_DESKTOP, o.client);
            }
            sm.active_pm = o.frame;
        }
    }
}

/// The timer: what the user did on the PM side, to Windows.
pub fn tick(sm: &mut Seamless) {
    let a = unsafe { WinQueryActiveWindow(HWND_DESKTOP) };
    if a != sm.active_pm {
        sm.active_pm = a;
        if let Some(o) = sm.wins.iter().find(|o| o.frame == a) {
            if o.hwnd != sm.active_win {
                sm.active_win = o.hwnd;
                sm.request(sf::REQ_ACTIVATE, o.hwnd, 0);
            }
        }
    }
    // A frame the window list showed again.
    for o in &sm.wins {
        if o.flags & sf::WF_MINIMIZED != 0 && unsafe { WinIsWindowVisible(o.frame) } != 0 {
            sm.request(sf::REQ_RESTORE, o.hwnd, 0);
        }
    }
}

/// The overlay over surface rectangle `r`: pixels outside the frames
/// that are not the desktop's (0) are inverted on the screen; while no
/// button is down only pixels inverted before are taken back.
fn overlay(sm: &mut Seamless, view: &View, r: (i32, i32, i32, i32)) {
    let (w, h) = (view.w as i32, view.h as i32);
    if sm.overlay.len() != (w * h) as usize / 8 + 1 {
        sm.overlay = alloc::vec![0; (w * h) as usize / 8 + 1];
    }
    if sm.buttons == 0 && !sm.overlay_on {
        return;
    }
    let (x0, y0, x1, y1) = (r.0.max(0), r.1.max(0), r.2.min(w), r.3.min(h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let px = unsafe { view.s.bytes(sf::PIXELS, view.w * view.h) };
    let inside = |x: i32, y: i32| sm.wins.iter().any(|o| x >= o.rect.0 && x < o.rect.0 + o.rect.2 && y >= o.rect.1 && y < o.rect.1 + o.rect.3);
    let sh = screen_h();
    let mut hps = 0;
    let mut any = false;
    for y in y0..y1 {
        let mut run: Option<i32> = None;
        for x in x0..=x1 {
            let flip = x < x1 && {
                let i = (y * w + x) as usize;
                let want = sm.buttons != 0 && px[i] != 0 && !inside(x, y);
                let had = sm.overlay[i / 8] & (1 << (i % 8)) != 0;
                if want != had {
                    sm.overlay[i / 8] ^= 1 << (i % 8);
                }
                want != had
            };
            match (flip, run) {
                (true, None) => run = Some(x),
                (false, Some(s)) => {
                    if hps == 0 {
                        hps = unsafe { WinGetScreenPS(HWND_DESKTOP) };
                    }
                    let rc = RectL { l: s, b: sh - 1 - y, r: x, t: sh - y };
                    unsafe { WinInvertRect(hps, &rc) };
                    run = None;
                }
                _ => {}
            }
        }
        any |= sm.overlay[(y * w + x0) as usize / 8..=((y * w + x1 - 1) as usize / 8)].iter().any(|&b| b != 0);
    }
    if hps != 0 {
        unsafe { WinReleasePS(hps) };
    }
    if any {
        sm.overlay_on = true;
    } else if sm.overlay_on && sm.overlay.iter().all(|&b| b == 0) {
        sm.overlay_on = false;
    }
}

/// Damage on the surface: the frames over it repaint.
pub fn damage(sm: &mut Seamless, view: &View, rects: Option<Vec<(i32, i32, i32, i32)>>) {
    if let Some(rs) = &rects {
        for &r in rs {
            overlay(sm, view, r);
        }
    }
    for o in &sm.wins {
        let (wx, wy, ww, wh) = o.rect;
        match &rects {
            None => unsafe {
                WinInvalidateRect(o.client, null(), 0);
            },
            Some(rs) => {
                for &(x0, y0, x1, y1) in rs {
                    let (l, t, r, b) = (x0.max(wx), y0.max(wy), x1.min(wx + ww), y1.min(wy + wh));
                    if l < r && t < b {
                        // Client coordinates: bottom up from the frame's
                        // bottom left.
                        let rc = RectL { l: l - wx, b: wy + wh - b, r: r - wx, t: wy + wh - t };
                        unsafe { WinInvalidateRect(o.client, &rc, 0) };
                    }
                }
            }
        }
    }
}

/// WM_PAINT of a frame's client.
pub fn paint(o: &SWin, view: &View) {
    let mut r = RectL::default();
    let hps = unsafe { WinBeginPaint(o.client, 0, &mut r) };
    view.paint_at(hps, &r, o.rect.3, o.rect.0, o.rect.1);
    unsafe { WinEndPaint(hps) };
}

/// The mouse over a frame's client: to the session as a position on
/// its screen.
pub fn mouse(sm: &mut Seamless, i: usize, view: &mut View, msg: u32, mp1: u32) {
    let (x, y) = (mp1 as i16 as i32, (mp1 >> 16) as i16 as i32);
    match msg {
        WM_BUTTON1DOWN => sm.buttons |= 1,
        WM_BUTTON1UP => sm.buttons &= !1,
        WM_BUTTON2DOWN => sm.buttons |= 2,
        WM_BUTTON2UP => sm.buttons &= !2,
        WM_BUTTON3DOWN => sm.buttons |= 4,
        WM_BUTTON3UP => sm.buttons &= !4,
        _ => {}
    }
    let o = &sm.wins[i];
    let sx = (o.rect.0 + x).clamp(0, view.w as i32 - 1);
    let sy = (o.rect.1 + o.rect.3 - 1 - y).clamp(0, view.h as i32 - 1);
    let fx = (sx as u32 * 65536 / view.w.max(1) as u32).min(65535);
    let fy = (sy as u32 * 65536 / view.h.max(1) as u32).min(65535);
    unsafe {
        if msg == WM_BUTTON1DOWN || msg == WM_BUTTON2DOWN {
            WinSetFocus(HWND_DESKTOP, o.client);
        }
        WinSetCapture(HWND_DESKTOP, if sm.buttons != 0 { o.client } else { 0 });
        Rs2VdmMouse(sm.sid, fx, fy, sm.buttons);
    }
    view.set_pointer(o.client);
}

/// WM_CLOSE (the window list's Close): Windows closes the window.
pub fn close(sm: &Seamless, i: usize) {
    sm.request(sf::REQ_CLOSE, sm.wins[i].hwnd, 0);
}

/// Start a program in the session (`cmd` without the NUL).
pub fn run(sm: &Seamless, view: &View, cmd: &[u8]) {
    let n = cmd.len().min(sf::HOST_CMD_LEN - 1);
    unsafe {
        let b = view.s.bytes(sf::HOST_CMD, sf::HOST_CMD_LEN);
        b[..n].copy_from_slice(&cmd[..n]);
        b[n] = 0;
    }
    sm.request(sf::REQ_RUN, n as u16, 0);
}

pub fn is_timer(id: u32) -> bool {
    id == TID_SEAMLESS
}

/// The session's frames go away.
pub fn destroy(sm: &mut Seamless) {
    for w in sm.wins.drain(..) {
        unsafe {
            Rs2PmVioWindow(w.client, 0, 0);
            WinDestroyWindow(w.frame);
        }
    }
}
