//! The desktop's own settings and the lockup (WPDesktop, IBM's `desk_`
//! methods and the lockup code of PMWP, studied with Ghidra):
//!
//! - **Lockup** (three pages): the password (1 to 15 characters, typed
//!   twice, kept upper case) or screen saver mode (no password: any key
//!   unlocks); the lockup's background colour; automatic lockup after a
//!   number of idle minutes, lock on startup, partial or full screen and
//!   auto-dim. Kept as IBM keeps them: OS2.INI `PM_Lockup`
//!   `LockupOptions` (IBM's 46-byte record, the password XORed with a
//!   key byte stored beside it), `LockupScreenSaverMode` and
//!   `LockupBitmap`; the setup strings AUTOLOCKUP, LOCKUPTIMEOUT,
//!   LOCKUPONSTARTUP, LOCKUPFULLSCREEN, LOCKUPAUTODIM,
//!   LOCKUPSCREENSAVERMODE and LOCKUPBACKGROUND, and the wpQuery/Set
//!   methods.
//! - **The lockup** ("Lockup now" on the desktop's menu, WinLockupSystem,
//!   the idle timeout, lock on startup): a system-modal window (PMWIN
//!   gives it all input; Ctrl+Esc does nothing) that covers the screen
//!   (full screen) or most of it (partial), with IBM's text; typing the
//!   password and Enter unlocks (WinUnlockSystem too). Without a
//!   password the password dialog comes first. With auto-dim the screen
//!   goes black after two idle minutes until a key or the mouse moves.
//! - **Archive**: "Create archive at each system startup" copies OS2.INI,
//!   OS2SYS.INI, CONFIG.SYS, STARTUP.CMD and the desktop directory into
//!   the archive location (`\OS2\ARCHIVES\nnnnn`, the last three kept,
//!   with ARCHIVES.LOG listing them) when the shell starts; the Recovery
//!   Choices settings are kept (rs2's loader has no recovery screen).
//! - **Desktop**: "Save Desktop properties" (`PM_Workplace`
//!   `PreserveDTSettings`) and the default drop operation of objects
//!   dropped on the desktop from other folders (create shadow or move;
//!   wpQuery/SetDefaultDropOp, kept with the desktop).

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio, spin, spin_init, spin_value};
use crate::som;
use crate::util::{cstr, parse_dec, prf_read, prf_write, z, HINI_USER};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.872"]
    fn WinSetSysModalWindow(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.9031"]
    fn Rs2QueryIdleTime() -> u32;
    #[link_name = "PMWIN.851"]
    fn WinSetActiveWindow(desk: u32, h: u32) -> u32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMGPI.404"]
    fn GpiMove(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.356"]
    fn GpiBox(hps: u32, ctl: i32, p: *const PointL, h: i32, v: i32) -> i32;
    #[link_name = "PMGPI.393"]
    fn GpiFullArc(hps: u32, control: u32, mult: u32) -> i32;
}

// ---- The lockup options (OS2.INI PM_Lockup, IBM's layout) ----

const APP: &[u8] = b"PM_Lockup";
const OPTIONS: &[u8] = b"LockupOptions";
const SAVER: &[u8] = b"LockupScreenSaverMode";
const BITMAP: &[u8] = b"LockupBitmap";
const OPT_SIZE: usize = 46;

/// IBM's LockupOptions: full screen +0, automatic lockup +2, timeout
/// (minutes) +4, the password's key (key * 3 + 36) +6, auto-dim +8, the
/// password XORed with the key +10 (16 bytes), lock on startup +26, the
/// background colour (RGB, or a negative SYSCLR_*) +36, the image's mode
/// (IBM's control ids: 306 normal, 307 tiled, 308 scaled) +42 and scale
/// +44.
#[derive(Clone, Copy)]
pub struct LockOpt {
    pub full: bool,
    pub auto: bool,
    pub timeout: u16,
    pub dim: bool,
    pub password: [u8; 16],
    pub startup: bool,
    pub color: i32,
    pub mode: u16,
    pub scale: u16,
}

const SYSCLR_BACKGROUND: i32 = -24;

impl LockOpt {
    fn default() -> LockOpt {
        LockOpt { full: true, auto: false, timeout: 3, dim: true, password: [0; 16], startup: false, color: SYSCLR_BACKGROUND, mode: 308, scale: 1 }
    }

    fn decode(b: &[u8]) -> LockOpt {
        let r16 = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let key = (r16(6).wrapping_sub(0x24) / 3) as u8;
        let mut password = [0u8; 16];
        for i in 0..15 {
            if b[10 + i] == 0 {
                break;
            }
            password[i] = b[10 + i] ^ key;
        }
        LockOpt {
            full: r16(0) != 0,
            auto: r16(2) != 0,
            timeout: r16(4).clamp(1, 99),
            dim: r16(8) != 0,
            password,
            startup: r16(26) != 0,
            color: i32::from_le_bytes([b[36], b[37], b[38], b[39]]),
            mode: r16(42),
            scale: r16(44),
        }
    }

    /// The record with the password encoded: the key is the day of the
    /// month, made a byte the password does not contain (as IBM's
    /// SaveLockOpt picks it).
    fn encode(&self) -> [u8; OPT_SIZE] {
        let mut b = [0u8; OPT_SIZE];
        let mut dt = os2::DateTime::default();
        unsafe { os2::DosGetDateTime(&mut dt) };
        let pw = self.pass();
        let mut key = dt.day.max(1);
        while pw.contains(&key) || key == 0 {
            key = key.wrapping_add(1);
        }
        let w16 = |b: &mut [u8; OPT_SIZE], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        w16(&mut b, 0, self.full as u16);
        w16(&mut b, 2, self.auto as u16);
        w16(&mut b, 4, self.timeout);
        w16(&mut b, 6, key as u16 * 3 + 0x24);
        w16(&mut b, 8, self.dim as u16);
        for (i, &c) in pw.iter().enumerate() {
            b[10 + i] = c ^ key;
        }
        w16(&mut b, 26, self.startup as u16);
        b[36..40].copy_from_slice(&self.color.to_le_bytes());
        w16(&mut b, 40, 0x128);
        w16(&mut b, 42, self.mode);
        w16(&mut b, 44, self.scale);
        b
    }

    pub fn pass(&self) -> Vec<u8> {
        self.password.iter().copied().take_while(|&c| c != 0).collect()
    }
}

pub fn options() -> LockOpt {
    match prf_read(HINI_USER, APP, OPTIONS) {
        Some(b) if b.len() == OPT_SIZE => LockOpt::decode(&b),
        _ => LockOpt::default(),
    }
}

fn save_options(o: &LockOpt) {
    prf_write(HINI_USER, APP, OPTIONS, Some(&o.encode()));
    timer_follow(o);
}

fn update(f: impl FnOnce(&mut LockOpt)) {
    let mut o = options();
    f(&mut o);
    save_options(&o);
}

/// Screen saver mode: the lockup asks for no password.
pub fn screen_saver() -> bool {
    prf_read(HINI_USER, APP, SAVER).is_some_and(|v| v.len() >= 4 && u32::from_le_bytes([v[0], v[1], v[2], v[3]]) != 0)
}

fn set_screen_saver(on: bool) {
    prf_write(HINI_USER, APP, SAVER, Some(&(on as u32).to_le_bytes()));
}

fn set_password(p: &[u8]) {
    update(|o| {
        o.password = [0; 16];
        for (i, &c) in p.iter().take(15).enumerate() {
            o.password[i] = c.to_ascii_uppercase();
        }
    });
}

/// The lockup background's image file (LOCKUPBACKGROUND=, IBM's
/// LockupBitmap).
fn bitmap() -> Vec<u8> {
    prf_read(HINI_USER, APP, BITMAP).map(|v| v.into_iter().take_while(|&c| c != 0).collect()).unwrap_or_default()
}

// ---- The background image ----

unsafe extern "C" {
    #[link_name = "PMWIN.730"]
    fn WinDrawBitmap(hps: u32, hbm: u32, src: *const RectL, dst: *const PointL, fore: i32, back: i32, fl: u32) -> u32;
}

/// The lockup's image while locked: (bitmap, width, height).
static mut IMAGE: (u32, i32, i32) = (0, 0, 0);

/// A bitmap from an OS/2 or Windows .BMP file (the first of a bitmap
/// array; uncompressed): its BITMAPINFO(2) and bits as GpiCreateBitmap
/// takes them.
pub fn load_bmp(path: &[u8]) -> Option<(u32, i32, i32)> {
    let d = crate::mindex::read_all(path)?;
    let mut base = 0usize;
    if d.get(0..2) == Some(b"BA") {
        base = 14;
    }
    let f = d.get(base..)?;
    if f.get(0..2) != Some(b"BM") || f.len() < 26 {
        return None;
    }
    let u32_at = |o: usize| f.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let off_bits = u32_at(10)? as usize;
    let cb = u32_at(14)? as usize;
    let (cx, cy, bpp) = if cb == 12 {
        let u16_at = |o: usize| u16::from_le_bytes([f[o], f[o + 1]]) as i32;
        (u16_at(18), u16_at(20), u16_at(24))
    } else {
        // BITMAPINFOHEADER2 / Windows: cx, cy (i32), planes, bit count,
        // compression.
        if cb >= 20 && u32_at(30)? != 0 {
            return None;
        }
        (u32_at(18)? as i32, (u32_at(22)? as i32).abs(), u16::from_le_bytes([f[28], f[29]]) as i32)
    };
    if cx <= 0 || cy <= 0 || !matches!(bpp, 1 | 4 | 8 | 24) {
        return None;
    }
    let rows = (bpp as usize * cx as usize).div_ceil(32) * 4 * cy as usize;
    // The bits' offset is from the file's start (in an array too).
    let bits = d.get(off_bits..off_bits + rows)?;
    // The header and its colour table, as in the file (a Windows
    // BITMAPINFOHEADER is laid out as the start of OS/2's 2.x one).
    let info = d.get(base + 14..off_bits.max(base + 14 + cb))?.to_vec();
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(&16u32.to_le_bytes());
    hdr[4..8].copy_from_slice(&(cx as u32).to_le_bytes());
    hdr[8..12].copy_from_slice(&(cy as u32).to_le_bytes());
    hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
    hdr[14..16].copy_from_slice(&(bpp as u16).to_le_bytes());
    // CBM_INIT
    let hbm = unsafe { GpiCreateBitmap(0, hdr.as_ptr(), 4, bits.as_ptr(), info.as_ptr()) };
    (hbm != 0).then_some((hbm, cx, cy))
}

/// The image drawn as the lockup's mode says: 306 normal (centred), 307
/// tiled, 308 scaled (`scale` images across and down; 1 fills the
/// window).
fn draw_image(hps: u32, all: &RectL, mode: u16, scale: u16) {
    let (hbm, bx, by) = unsafe { IMAGE };
    if hbm == 0 {
        return;
    }
    const DBM_NORMAL: u32 = 0;
    const DBM_STRETCH: u32 = 4;
    let (w, h) = (all.r - all.l, all.t - all.b);
    match mode {
        306 => {
            let p = PointL { x: all.l + (w - bx) / 2, y: all.b + (h - by) / 2 };
            unsafe { WinDrawBitmap(hps, hbm, core::ptr::null(), &p, 0, 0, DBM_NORMAL) };
        }
        307 => {
            let mut y = all.b;
            while y < all.t {
                let mut x = all.l;
                while x < all.r {
                    unsafe { WinDrawBitmap(hps, hbm, core::ptr::null(), &PointL { x, y }, 0, 0, DBM_NORMAL) };
                    x += bx;
                }
                y += by;
            }
        }
        _ => {
            let n = scale.clamp(1, 99) as i32;
            for j in 0..n {
                for i in 0..n {
                    let r = RectL { l: all.l + w * i / n, b: all.b + h * j / n, r: all.l + w * (i + 1) / n, t: all.b + h * (j + 1) / n };
                    unsafe { WinDrawBitmap(hps, hbm, core::ptr::null(), &r as *const RectL as *const PointL, 0, 0, DBM_STRETCH) };
                }
            }
        }
    }
}

fn load_image() {
    free_image();
    let f = bitmap();
    if !f.is_empty() {
        // A .BMP directly, any other picture through MMIO (image.rs).
        if let Some(i) = load_bmp(&f).or_else(|| crate::image::load(&f)) {
            unsafe { IMAGE = i };
        }
    }
}

fn free_image() {
    unsafe {
        if IMAGE.0 != 0 {
            GpiDeleteBitmap(IMAGE.0);
        }
        IMAGE = (0, 0, 0);
    }
}

fn color_rgb(c: i32) -> u32 {
    match c {
        // The desktop's own background.
        SYSCLR_BACKGROUND => crate::setup::desktop_background(),
        _ if c < 0 => crate::setup::sys_color(c),
        _ => c as u32 & 0xff_ffff,
    }
}

// ---- The lockup ----

static LOCK_CLASS: &[u8] = b"RS2Lockup\0";
const TID_LOCK: u32 = 0x5712;
/// The desktop's timer for the automatic lockup (every 20 seconds, as
/// IBM's).
pub const TID_AUTOLOCK: u32 = 0x5711;
const DIM_MS: u32 = 120_000;

struct Lock {
    hwnd: u32,
    typed: Vec<u8>,
    dimmed: bool,
    message: &'static [u8],
}

static mut LOCK: Option<Lock> = None;

pub fn locked() -> bool {
    unsafe { LOCK.is_some() }
}

fn screen() -> (i32, i32) {
    unsafe { (WinQuerySysValue(HWND_DESKTOP, 20), WinQuerySysValue(HWND_DESKTOP, 21)) }
}

extern "C" fn lock_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_lock(h);
            0
        }
        WM_CHAR => {
            // KC_KEYUP: nothing; KC_CHAR: the character.
            if a & 0x40 != 0 {
                return 1;
            }
            let Some(l) = (unsafe { LOCK.as_mut() }) else { return 1 };
            if l.dimmed {
                undim();
                return 1;
            }
            let ch = (b & 0xff) as u8;
            let vk = (b >> 16) & 0xffff;
            if screen_saver() {
                unlock();
                return 1;
            }
            match vk {
                // VK_NEWLINE, VK_ENTER
                0x08 | 0x1e if a & 0x02 != 0 => {
                    let typed = core::mem::take(&mut l.typed);
                    if !try_unlock(&typed) {
                        unsafe { WinAlarm(HWND_DESKTOP, 0) };
                        if let Some(l) = unsafe { LOCK.as_mut() } {
                            l.message = b"The password is not correct. Type your password and press Enter.";
                            unsafe { WinInvalidateRect(l.hwnd, core::ptr::null(), 0) };
                        }
                    }
                }
                // VK_BACKSPACE
                0x05 if a & 0x02 != 0 => {
                    l.typed.pop();
                }
                _ if a & 0x01 != 0 && ch >= 0x20 && l.typed.len() < 15 => l.typed.push(ch.to_ascii_uppercase()),
                _ => {}
            }
            1
        }
        WM_MOUSEMOVE | WM_BUTTON1DOWN | WM_BUTTON2DOWN => {
            if unsafe { LOCK.as_ref() }.is_some_and(|l| l.dimmed) {
                undim();
            }
            if msg == WM_MOUSEMOVE {
                unsafe { WinDefWindowProc(h, msg, a, b) }
            } else {
                1
            }
        }
        WM_TIMER if a == TID_LOCK => {
            // Stay on top and active; dim when nobody has typed for a while.
            unsafe { WinSetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
            if options().dim && !unsafe { LOCK.as_ref() }.is_some_and(|l| l.dimmed) && unsafe { Rs2QueryIdleTime() } >= DIM_MS {
                if let Some(l) = unsafe { LOCK.as_mut() } {
                    l.dimmed = true;
                }
                unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

fn undim() {
    if let Some(l) = unsafe { LOCK.as_mut() } {
        l.dimmed = false;
        unsafe { WinInvalidateRect(l.hwnd, core::ptr::null(), 0) };
    }
}

fn paint_lock(h: u32) {
    let mut r = RectL::default();
    let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
    let mut all = RectL::default();
    unsafe { WinQueryWindowRect(h, &mut all) };
    let o = options();
    let dimmed = unsafe { LOCK.as_ref() }.is_some_and(|l| l.dimmed);
    unsafe {
        // LCOLF_RGB
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        WinFillRect(hps, &all, if dimmed { 0 } else { color_rgb(o.color) as i32 });
    }
    if !dimmed {
        draw_image(hps, &all, o.mode, o.scale);
    }
    if !dimmed {
        let (cx, cy) = (all.r, all.t);
        // A padlock in the middle: the shackle (outlines), the body over
        // its lower half, the keyhole.
        let (mx, my) = (cx / 2, cy / 2 + 20);
        unsafe {
            GpiSetColor(hps, 0xc8c8c8);
            for k in 0..6 {
                GpiMove(hps, &PointL { x: mx, y: my + 6 });
                // DRO_OUTLINE
                GpiFullArc(hps, 2, ((20 - k) as u32) << 16);
            }
            GpiSetColor(hps, 0xe0b040);
            GpiMove(hps, &PointL { x: mx - 30, y: my - 40 });
            // DRO_OUTLINEFILL
            GpiBox(hps, 3, &PointL { x: mx + 30, y: my + 6 }, 10, 10);
            GpiSetColor(hps, 0x302010);
            GpiMove(hps, &PointL { x: mx, y: my - 12 });
            GpiFullArc(hps, 3, 6 << 16);
            GpiMove(hps, &PointL { x: mx - 2, y: my - 28 });
            GpiBox(hps, 3, &PointL { x: mx + 2, y: my - 12 }, 0, 0);
        }
        // IBM's message line across the bottom.
        let bar = RectL { l: 0, b: 0, r: cx, t: 40 };
        unsafe { WinFillRect(hps, &bar, 0xc0c0c0) };
        let msg: &[u8] = unsafe { LOCK.as_ref() }.map_or(b"", |l| l.message);
        let t = z(msg);
        let mut tr = bar;
        unsafe { WinDrawText(hps, -1, t.as_ptr(), &mut tr, 0, 0xc0c0c0, DT_CENTER | DT_VCENTER) };
        let title = z(b"Keyboard / Mouse Lock Up");
        let mut tr = RectL { l: 0, b: my - 80, r: cx, t: my - 56 };
        unsafe { WinDrawText(hps, -1, title.as_ptr(), &mut tr, 0xffffff, color_rgb(o.color) as i32, DT_CENTER | DT_VCENTER) };
    }
    unsafe { WinEndPaint(hps) };
}

const DT_CENTER: u32 = 0x0100;
const DT_VCENTER: u32 = 0x0400;
const HWND_TOP: u32 = 3;
const SWP_ZORDER: u32 = 0x0004;

/// Lock the system now ("Lockup now", WinLockupSystem, the timeout).
/// Without a password (and not in screen saver mode) the password dialog
/// comes first; cancelled, nothing is locked.
pub fn lock_now() -> bool {
    if locked() {
        return true;
    }
    if !screen_saver() && options().pass().is_empty() && !ask_password() {
        return false;
    }
    let hab = unsafe { WinQueryAnchorBlock(HWND_DESKTOP) };
    unsafe { WinRegisterClass(hab, LOCK_CLASS.as_ptr(), lock_proc, 0, 4) };
    let (sw, sh) = screen();
    let o = options();
    // Partial screen: most of the screen, the desktop's edges showing.
    let (x, y, cx, cy) = if o.full { (0, 0, sw, sh) } else { (sw / 8, sh / 8, sw * 3 / 4, sh * 3 / 4) };
    load_image();
    let h = unsafe { WinCreateWindow(HWND_DESKTOP, LOCK_CLASS.as_ptr() as u32, b"\0".as_ptr(), 0, x, y, cx, cy, 0, HWND_TOP, 0, core::ptr::null(), core::ptr::null()) };
    if h == 0 {
        return false;
    }
    unsafe {
        LOCK = Some(Lock { hwnd: h, typed: Vec::new(), dimmed: false, message: b"Type your password and press Enter to unlock the keyboard and mouse." });
        if screen_saver() {
            LOCK.as_mut().unwrap().message = b"Press a key to unlock the keyboard and mouse.";
        }
        WinShowWindow(h, 1);
        WinSetActiveWindow(HWND_DESKTOP, h);
        WinSetFocus(HWND_DESKTOP, h);
        WinSetSysModalWindow(HWND_DESKTOP, h);
        WinStartTimer(hab, h, TID_LOCK, 2000);
        WinAlarm(HWND_DESKTOP, 0);
    }
    true
}

fn try_unlock(typed: &[u8]) -> bool {
    let p = options().pass();
    if screen_saver() || (!p.is_empty() && typed.eq_ignore_ascii_case(&p)) {
        unlock();
        return true;
    }
    false
}

fn unlock() {
    let Some(l) = (unsafe { LOCK.take() }) else { return };
    free_image();
    unsafe {
        WinSetSysModalWindow(HWND_DESKTOP, 0);
        WinDestroyWindow(l.hwnd);
    }
}

/// WinLockupSystem(hab) (PMWP.283): the lockup, in the shell (from other
/// processes through ipc.rs).
#[unsafe(no_mangle)]
pub extern "C" fn WinLockupSystem(_hab: u32) -> u32 {
    if wps::active() {
        return lock_now() as u32;
    }
    crate::ipc::call(crate::ipc::OP_LOCKUP, &[], &[]).unwrap_or(0)
}

/// WinUnlockSystem(hab, pszPassword) (PMWP.282): unlock when the
/// password is the lockup's.
#[unsafe(no_mangle)]
pub extern "C" fn WinUnlockSystem(_hab: u32, pw: *const u8) -> u32 {
    let p = if pw.is_null() { Vec::new() } else { cstr(pw).to_vec() };
    if wps::active() {
        return (locked() && try_unlock(&p)) as u32;
    }
    crate::ipc::call(crate::ipc::OP_UNLOCK, &[], &[&p]).unwrap_or(0)
}

/// The desktop's timer: lock after the idle timeout.
pub fn auto_tick() {
    let o = options();
    if !o.auto || locked() {
        return;
    }
    if unsafe { Rs2QueryIdleTime() } >= o.timeout as u32 * 60_000 {
        lock_now();
    }
}

/// The automatic lockup's timer runs while it is on.
fn timer_follow(o: &LockOpt) {
    let client = crate::view::desktop_client();
    if client == 0 {
        return;
    }
    let hab = unsafe { WinQueryAnchorBlock(HWND_DESKTOP) };
    unsafe {
        if o.auto {
            WinStartTimer(hab, client, TID_AUTOLOCK, 20_000);
        } else {
            WinStopTimer(hab, client, TID_AUTOLOCK);
        }
    }
}

/// When the shell has started: the automatic lockup's timer, and the
/// lockup itself with "Lock on startup".
pub fn started() {
    let o = options();
    timer_follow(&o);
    if o.startup && (screen_saver() || !o.pass().is_empty()) {
        lock_now();
    }
}

// ---- The password (Lockup page 1, and before locking without one) ----

const ID_PW1: u16 = 254;
const ID_PW2: u16 = 255;
const ID_SAVER: u16 = 1267;
const ID_PWHINT: u16 = 259;

fn pw_items(y0: i16) -> Vec<Item> {
    // ES_AUTOSCROLL | ES_MARGIN | ES_UNREADABLE
    let entry = |y: i16, id: u16| item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008 | 0x0080, (80, y, 100, 8), id);
    alloc::vec![
        item(WC_STATIC, b"Type your password (1 to 15 characters) in both entry fields.  Then press Enter (characters are not displayed).", WS_VISIBLE | 0x0001 | 0x4000, (8, y0 + 34, 210, 20), ID_PWHINT),
        book::label(b"Password", (8, y0 + 20, 60)),
        entry(y0 + 20, ID_PW1),
        book::label(b"Password", (8, y0 + 4, 60)),
        book::label(b"(for verification)", (8, y0 - 4, 70)),
        entry(y0 + 2, ID_PW2),
    ]
}

/// The two entry fields agree: the new password (upper case).
fn take_password(h: u32) -> Option<Vec<u8>> {
    let (a, b) = (item_text(h, ID_PW1 as u32), item_text(h, ID_PW2 as u32));
    set_item_text(h, ID_PW1 as u32, b"");
    set_item_text(h, ID_PW2 as u32, b"");
    if a.is_empty() || a.len() > 15 || !a.eq_ignore_ascii_case(&b) {
        unsafe { WinAlarm(HWND_DESKTOP, 2) };
        set_item_text(h, ID_PWHINT as u32, b"The passwords do not match (or are empty). Type the password in both entry fields again.");
        unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_PW1 as u32)) };
        return None;
    }
    Some(a.to_ascii_uppercase())
}

extern "C" fn pw_dlg_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            for id in [ID_PW1, ID_PW2] {
                unsafe { WinSendDlgItemMsg(h, id as u32, EM_SETTEXTLIMIT, 15, 0) };
            }
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_PW1 as u32)) };
            1
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            if let Some(p) = take_password(h) {
                set_password(&p);
                unsafe { WinDismissDlg(h, DID_OK) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn ask_password() -> bool {
    let mut items = pw_items(26);
    items.push(item(WC_BUTTON, b"~OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (8, 4, 50, 14), DID_OK as u16));
    items.push(item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 50, 14), DID_CANCEL as u16));
    let tpl = template(dialog_frame(b"Lockup Password", 230, 96), items);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, crate::view::desktop_client(), pw_dlg_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return false;
    }
    // In the middle of the screen.
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(dlg, &mut r) };
    let (sw, sh) = screen();
    unsafe { WinSetWindowPos(dlg, HWND_TOP, (sw - r.r) / 2, (sh - r.t) / 2, 0, 0, 0x0002 | SWP_ZORDER | 0x0008 | 0x0080) };
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    r == DID_OK && !options().pass().is_empty()
}

const ID_SETPW: u16 = 1;

extern "C" fn lockup1_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            for id in [ID_PW1, ID_PW2] {
                unsafe { WinSendDlgItemMsg(h, id as u32, EM_SETTEXTLIMIT, 15, 0) };
            }
            let saver = screen_saver();
            check(h, ID_SAVER, saver);
            enable_pw(h, !saver);
            0
        }
        WM_CONTROL if a & 0xffff == ID_SAVER as u32 && a >> 16 == 1 => {
            let saver = checked(h, ID_SAVER);
            set_screen_saver(saver);
            enable_pw(h, !saver);
            0
        }
        WM_COMMAND if a & 0xffff == ID_SETPW as u32 => {
            if let Some(p) = take_password(h) {
                set_password(&p);
                set_item_text(h, ID_PWHINT as u32, b"The password is set. Type your password (1 to 15 characters) in both entry fields to change it.");
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn enable_pw(h: u32, on: bool) {
    for id in [ID_PW1, ID_PW2, ID_SETPW] {
        unsafe { WinEnableWindow(WinWindowFromID(h, id as u32), on as u32) };
    }
}

extern "C" fn wpAddDesktopLockup1Page(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"~Screen saver mode (no password)", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (8, 104, 200, 10), ID_SAVER),
        group(b"Password", (4, 20, 226, 80)),
    ];
    items.extend(pw_items(40));
    items.push(item(WC_BUTTON, b"~OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (186, 40, 40, 14), ID_SETPW));
    add_page(o, nb, lockup1_proc, items, b"~Lockup\0")
}

fn group(t: &[u8], (x, y, cx, cy): (i16, i16, i16, i16)) -> Item {
    // SS_GROUPBOX
    item(WC_STATIC, t, WS_VISIBLE | 0x0002, (x, y, cx, cy), 0xffff)
}

// ---- Lockup page 2: the background ----

const LOCK_COLORS: [(&[u8], i32); 6] = [
    (b"~Desktop colour", SYSCLR_BACKGROUND),
    (b"~Black", 0x000000),
    (b"Dark ~blue", 0x1c2c5c),
    (b"~Teal", 0x2e6e8e),
    (b"Dark ~green", 0x1f4f3f),
    (b"G~rey", 0x606060),
];
const ID_LCOLOR: u16 = 295;
const ID_LFILE: u16 = 302;
/// IBM's ids for the image's mode (LockupOptions keeps them) and scale.
const ID_LNORMAL: u16 = 306;
const ID_LTILED: u16 = 307;
const ID_LSCALED: u16 = 308;
const ID_LSCALE: u16 = 309;

fn enable_item(h: u32, id: u16, on: bool) {
    unsafe { WinEnableWindow(WinWindowFromID(h, id as u32), on as u32) };
}

extern "C" fn lockup2_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let c = options().color;
            let i = LOCK_COLORS.iter().position(|&(_, v)| v == c).unwrap_or(0);
            check(h, ID_LCOLOR + i as u16, true);
            set_item_text(h, ID_LFILE as u32, &bitmap());
            let o = options();
            check(h, o.mode.clamp(ID_LNORMAL, ID_LSCALED), true);
            spin_init(h, ID_LSCALE, 1, 99, o.scale.max(1) as i32);
            enable_item(h, ID_LSCALE, o.mode == ID_LSCALED);
            0
        }
        WM_CONTROL if a & 0xffff == ID_LSCALE as u32 && a >> 16 == SPBN_CHANGE => {
            if let Some(v) = spin_value(h, ID_LSCALE).filter(|v| (1..=99).contains(v)) {
                update(|o| o.scale = v as u16);
            }
            0
        }
        WM_CONTROL if a >> 16 == 1 => {
            let id = (a & 0xffff) as u16;
            if (ID_LCOLOR..ID_LCOLOR + LOCK_COLORS.len() as u16).contains(&id) {
                update(|o| o.color = LOCK_COLORS[(id - ID_LCOLOR) as usize].1);
            }
            if (ID_LNORMAL..=ID_LSCALED).contains(&id) {
                update(|o| o.mode = id);
                enable_item(h, ID_LSCALE, id == ID_LSCALED);
            }
            0
        }
        // EN_KILLFOCUS on the file name.
        WM_CONTROL if a & 0xffff == ID_LFILE as u32 && a >> 16 == 2 => {
            let f = item_text(h, ID_LFILE as u32);
            prf_write(HINI_USER, APP, BITMAP, (!f.is_empty()).then(|| z(&f)).as_deref());
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddDesktopLockup2Page(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![group(b"Color", (4, 60, 226, 56))];
    for (i, &(t, _)) in LOCK_COLORS.iter().enumerate() {
        let (col, row) = (i as i16 % 2, i as i16 / 2);
        items.push(radio(t, (10 + col * 110, 94 - row * 12, 104), ID_LCOLOR + i as u16, i == 0));
    }
    items.push(group(b"Image", (4, 2, 226, 56)));
    items.push(book::label(b"File (an image):", (10, 40, 70)));
    // ES_AUTOSCROLL | ES_MARGIN
    items.push(item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (82, 40, 140, 8), ID_LFILE));
    items.push(radio(b"~Normal", (10, 22, 50), ID_LNORMAL, true));
    items.push(radio(b"T~iled", (64, 22, 50), ID_LTILED, false));
    items.push(radio(b"~Scaled", (118, 22, 50), ID_LSCALED, false));
    items.push(crate::setup::spin((172, 20, 36), ID_LSCALE));
    items.push(book::label(b"Images across and down", (118, 8, 110)));
    let page = {
        let tpl = template(book::page_frame(), items);
        book::make_page(nb, lockup2_proc, &tpl, o)
    };
    if page == 0 {
        return 0;
    }
    book::insert_more(o, nb, page, b"Lockup - Page 2 of 3\0")
}

// ---- Lockup page 3: timeout and display ----

const ID_NOAUTO: u16 = 271;
const ID_AUTO: u16 = 272;
const ID_DELAY: u16 = 274;
const ID_STARTUP: u16 = 276;
const ID_PARTIAL: u16 = 256;
const ID_FULL: u16 = 257;
const ID_DIM: u16 = 258;
const ID_DEFAULT: u16 = 287;
const SPBN_CHANGE: u32 = 0x20d;

fn lockup3_init(h: u32) {
    let o = options();
    check(h, if o.auto { ID_AUTO } else { ID_NOAUTO }, true);
    spin_init(h, ID_DELAY, 1, 99, o.timeout as i32);
    check(h, ID_STARTUP, o.startup);
    check(h, if o.full { ID_FULL } else { ID_PARTIAL }, true);
    check(h, ID_DIM, o.dim);
}

extern "C" fn lockup3_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            lockup3_init(h);
            0
        }
        WM_CONTROL => {
            let id = (a & 0xffff) as u16;
            match (id, a >> 16) {
                (ID_NOAUTO, 1) => update(|o| o.auto = false),
                (ID_AUTO, 1) => update(|o| o.auto = true),
                (ID_DELAY, SPBN_CHANGE) => {
                    if let Some(v) = spin_value(h, ID_DELAY).filter(|v| (1..=99).contains(v)) {
                        update(|o| o.timeout = v as u16);
                    }
                }
                (ID_STARTUP, 1) => {
                    let on = checked(h, ID_STARTUP);
                    update(|o| o.startup = on);
                }
                (ID_PARTIAL, 1) => update(|o| o.full = false),
                (ID_FULL, 1) => update(|o| o.full = true),
                (ID_DIM, 1) => {
                    let on = checked(h, ID_DIM);
                    update(|o| o.dim = on);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if a & 0xffff == ID_DEFAULT as u32 {
                update(|o| {
                    let d = LockOpt::default();
                    (o.auto, o.timeout, o.startup, o.full, o.dim) = (d.auto, d.timeout, d.startup, d.full, d.dim);
                });
                lockup3_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddDesktopLockup3Page(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let cb = |t: &[u8], (x, y, cx): (i16, i16, i16), id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (x, y, cx, 10), id);
    let items = alloc::vec![
        group(b"Timeout", (4, 30, 114, 86)),
        radio(b"~No automatic lockup", (8, 96, 106), ID_NOAUTO, true),
        radio(b"~Automatic lockup", (8, 84, 106), ID_AUTO, false),
        book::label(b"Delay", (18, 68, 30)),
        spin((48, 66, 34), ID_DELAY),
        book::label(b"minutes", (84, 68, 32)),
        cb(b"~Lock on startup", (8, 40, 106), ID_STARTUP),
        group(b"Display", (122, 30, 110, 86)),
        radio(b"~Partial screen", (126, 96, 100), ID_PARTIAL, true),
        radio(b"~Full screen", (126, 84, 100), ID_FULL, false),
        cb(b"Auto-~dim", (126, 60, 100), ID_DIM),
        item(WC_BUTTON, b"~Default", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (4, 4, 40, 14), ID_DEFAULT),
    ];
    let tpl = template(book::page_frame(), items);
    let page = book::make_page(nb, lockup3_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_more(o, nb, page, b"Lockup - Page 3 of 3\0")
}

// ---- Archive ----

const ARC: &[u8] = b"PM_Workplace:Archive";
const ID_ARC_ON: u16 = 704;
const ID_ARC_LOC: u16 = 703;
const ID_REC_ON: u16 = 705;
const ID_REC_TIME: u16 = 706;

fn arc_flag(key: &[u8]) -> bool {
    prf_read(HINI_USER, ARC, key).is_some_and(|v| v.first() == Some(&b'1'))
}

fn set_arc_flag(key: &[u8], on: bool) {
    prf_write(HINI_USER, ARC, key, Some(if on { b"1\0" } else { b"0\0" }));
}

/// Where archives go (IBM's default: \OS2\ARCHIVES on the boot drive).
pub fn archive_location() -> Vec<u8> {
    match prf_read(HINI_USER, ARC, b"Location") {
        Some(v) if v.len() > 1 => v.into_iter().take_while(|&c| c != 0).collect(),
        _ => alloc::format!("{}:\\OS2\\ARCHIVES", boot_drive() as char).into_bytes(),
    }
}

pub fn boot_drive() -> u8 {
    let mut d = 3u32;
    // QSV_BOOT_DRIVE
    unsafe { os2::DosQuerySysInfo(5, 5, &mut d, 4) };
    b'A' + (d as u8).saturating_sub(1)
}

fn recovery_timeout() -> i32 {
    prf_read(HINI_USER, ARC, b"RecoveryTimeout").and_then(|v| parse_dec(&v.into_iter().take_while(|&c| c != 0).collect::<Vec<u8>>())).map_or(3, |n| n as i32)
}

extern "C" fn archive_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            check(h, ID_ARC_ON, arc_flag(b"Enabled"));
            set_item_text(h, ID_ARC_LOC as u32, &archive_location());
            check(h, ID_REC_ON, arc_flag(b"Recovery"));
            spin_init(h, ID_REC_TIME, 0, 999, recovery_timeout());
            0
        }
        WM_CONTROL => {
            let id = (a & 0xffff) as u16;
            match (id, a >> 16) {
                (ID_ARC_ON, 1) => set_arc_flag(b"Enabled", checked(h, ID_ARC_ON)),
                (ID_REC_ON, 1) => set_arc_flag(b"Recovery", checked(h, ID_REC_ON)),
                // EN_KILLFOCUS
                (ID_ARC_LOC, 2) => {
                    let l = item_text(h, ID_ARC_LOC as u32);
                    prf_write(HINI_USER, ARC, b"Location", (!l.is_empty()).then(|| z(&l)).as_deref());
                }
                (ID_REC_TIME, SPBN_CHANGE) => {
                    if let Some(v) = spin_value(h, ID_REC_TIME).filter(|v| (0..=999).contains(v)) {
                        prf_write(HINI_USER, ARC, b"RecoveryTimeout", Some(&z(alloc::format!("{v}").as_bytes())));
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddDesktopArcRest1Page(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let cb = |t: &[u8], (x, y, cx): (i16, i16, i16), id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (x, y, cx, 10), id);
    add_page(o, nb, archive_proc, alloc::vec![
        group(b"Archive system files", (4, 62, 226, 54)),
        cb(b"Create archive at each system startup", (8, 96, 216), ID_ARC_ON),
        book::label(b"Archive location", (10, 84, 100)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (12, 70, 200, 8), ID_ARC_LOC),
        group(b"Recovery Choices screen", (4, 6, 226, 54)),
        cb(b"Display recovery choices at each system startup", (8, 40, 216), ID_REC_ON),
        book::label(b"Timeout for Recovery Choices screen", (8, 26, 140)),
        spin((150, 24, 34), ID_REC_TIME),
        book::label(b"seconds", (188, 26, 36)),
        book::label(b"(0 seconds indicates no timeout)", (8, 14, 140)),
    ], b"~Archive\0")
}

/// When the shell starts: an archive, if the Archive page asks for one.
/// It goes into the next of three numbered directories under the archive
/// location; ARCHIVES.LOG there lists them with their dates.
pub fn archive_at_startup() {
    if !arc_flag(b"Enabled") {
        return;
    }
    let loc = archive_location();
    make_dirs(&loc);
    let next = prf_read(HINI_USER, ARC, b"Next").and_then(|v| parse_dec(&v.into_iter().take_while(|&c| c != 0).collect::<Vec<u8>>())).unwrap_or(0) % 3;
    let dir = [&loc[..], alloc::format!("\\{:05}", next).as_bytes()].concat();
    if crate::fileops::exists(&dir) {
        crate::fileops::delete_tree(&dir);
        crate::fileops::delete_path(&dir);
    }
    make_dirs(&dir);
    let drive = boot_drive() as char;
    let mut copied = 0;
    for f in ["OS2\\OS2.INI", "OS2\\OS2SYS.INI", "CONFIG.SYS", "STARTUP.CMD"] {
        let src = alloc::format!("{drive}:\\{f}").into_bytes();
        if crate::fileops::exists(&src) {
            let name = crate::fileops::name_of(&src).to_vec();
            let (s, d) = (z(&src), z(&crate::fileops::join(&dir, &name)));
            // DCPY_EXISTING
            if unsafe { os2::DosCopy(s.as_ptr(), d.as_ptr(), 1) } == 0 {
                copied += 1;
            }
        }
    }
    let desk = alloc::format!("{drive}:\\DESKTOP").into_bytes();
    if crate::fileops::exists(&desk) {
        let mut nothing = |_: &[u8], _: u32| true;
        crate::fileops::copy_tree(&desk, &crate::fileops::join(&dir, b"DESKTOP"), true, &mut nothing);
    }
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    let line = alloc::format!("{:05} {:04}-{:02}-{:02} {:02}:{:02} {} files and the desktop\r\n", next, dt.year, dt.month, dt.day, dt.hours, dt.minutes, copied);
    append(&crate::fileops::join(&loc, b"ARCHIVES.LOG"), line.as_bytes());
    prf_write(HINI_USER, ARC, b"Next", Some(&z(alloc::format!("{}", (next + 1) % 3).as_bytes())));
    prf_write(HINI_USER, ARC, b"Last", Some(&z(&dir)));
}

fn make_dirs(path: &[u8]) -> u32 {
    let mut rc = 0;
    for i in 3..=path.len() {
        if i == path.len() || path[i] == b'\\' {
            let p = z(&path[..i]);
            if !crate::fileops::exists(&path[..i]) {
                rc = unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
            }
        }
    }
    rc
}

fn append(path: &[u8], data: &[u8]) {
    let p = z(path);
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS; write, deny write
    if unsafe { os2::DosOpen(p.as_ptr(), &mut h, &mut act, 0, 0, 0x11, 0x21, core::ptr::null_mut()) } != 0 {
        return;
    }
    let (mut newp, mut n) = (0u32, 0u32);
    unsafe {
        os2::DosSetFilePtr(h, 0, 2, &mut newp);
        os2::DosWrite(h, data.as_ptr(), data.len() as u32, &mut n);
        os2::DosClose(h);
    }
}

// ---- The Desktop page ----

const ID_PRESERVE: u16 = 801;
const ID_DROP_SHADOW: u16 = 887;
const ID_DROP_MOVE: u16 = 888;
pub const DO_LINK: u32 = 0x18;
pub const DO_MOVE: u32 = 0x20;

/// `PM_Workplace` `PreserveDTSettings` (IBM's, a ULONG; on unless set).
pub fn preserve_settings() -> bool {
    prf_read(HINI_USER, b"PM_Workplace", b"PreserveDTSettings").is_none_or(|v| v.len() < 4 || u32::from_le_bytes([v[0], v[1], v[2], v[3]]) != 0)
}

#[repr(C)]
pub struct DeskData {
    /// The default drop operation (DO_LINK or DO_MOVE).
    pub drop_op: u32,
}

fn dd(o: usize) -> &'static mut DeskData {
    som::data(wps::ccd("WPDesktop"), o)
}

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

extern "C" fn desktop_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            check(h, ID_PRESERVE, preserve_settings());
            let op = wps::call("WPDesktop", m::WPDesktop::wpQueryDefaultDropOp, &[b as usize]) as u32;
            check(h, if op == DO_MOVE { ID_DROP_MOVE } else { ID_DROP_SHADOW }, true);
            0
        }
        WM_CONTROL if a >> 16 == 1 => {
            let o = page_obj(h);
            match (a & 0xffff) as u16 {
                ID_PRESERVE => {
                    prf_write(HINI_USER, b"PM_Workplace", b"PreserveDTSettings", Some(&(checked(h, ID_PRESERVE) as u32).to_le_bytes()));
                }
                ID_DROP_SHADOW => {
                    wps::call("WPDesktop", m::WPDesktop::wpSetDefaultDropOp, &[o, DO_LINK as usize]);
                }
                ID_DROP_MOVE => {
                    wps::call("WPDesktop", m::WPDesktop::wpSetDefaultDropOp, &[o, DO_MOVE as usize]);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddDesktopDefDT1Page(o: usize, nb: u32) -> u32 {
    add_page(o, nb, desktop_page_proc, alloc::vec![
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"Save Desktop properties", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (8, 100, 200, 10), ID_PRESERVE),
        group(b"Default drop operation", (4, 50, 200, 44)),
        radio(b"~Create shadow", (8, 74, 180), ID_DROP_SHADOW, true),
        radio(b"~Move", (8, 62, 180), ID_DROP_MOVE, false),
        book::label(b"What objects dragged onto the desktop from other", (8, 36, 220)),
        book::label(b"folders do when no key is held.", (8, 26, 220)),
    ], b"~Desktop\0")
}

// ---- The methods ----

extern "C" fn wpIsCurrentDesktop(o: usize) -> u32 {
    (o == crate::fs::desktop()) as u32
}

extern "C" fn wpQueryDefaultDropOp(o: usize) -> u32 {
    match dd(o).drop_op {
        DO_MOVE => DO_MOVE,
        _ => DO_LINK,
    }
}

extern "C" fn wpSetDefaultDropOp(o: usize, op: u32) -> u32 {
    if dd(o).drop_op != op {
        dd(o).drop_op = op;
        obj::call(m::WPObject::wpSaveDeferred, &[o]);
    }
    1
}

macro_rules! flag_methods {
    ($q:ident, $s:ident, $f:ident) => {
        extern "C" fn $q(_o: usize) -> u32 {
            options().$f as u32
        }
        extern "C" fn $s(_o: usize, on: u32) -> u32 {
            update(|x| x.$f = on != 0);
            1
        }
    };
}

flag_methods!(wpQueryAutoLockup, wpSetAutoLockup, auto);
flag_methods!(wpQueryLockupOnStartup, wpSetLockupOnStartup, startup);
flag_methods!(wpQueryLockupFullScreen, wpSetLockupFullScreen, full);
flag_methods!(wpQueryLockupAutoDim, wpSetLockupAutoDim, dim);

extern "C" fn wpQueryLockupTimeout(_o: usize) -> u32 {
    options().timeout as u32
}

extern "C" fn wpSetLockupTimeout(_o: usize, t: u32) -> u32 {
    if !(1..=99).contains(&t) {
        return 0;
    }
    update(|x| x.timeout = t as u16);
    1
}

static mut BITMAP_NAME: Vec<u8> = Vec::new();

/// wpQueryLockupBackground(ppszImageFileName, pulImageMode,
/// pulScaleFactor, pulBackgroundType, plBackgroundColor).
extern "C" fn wpQueryLockupBackground(_o: usize, name: *mut *const u8, mode: *mut u32, scale: *mut u32, ty: *mut u32, color: *mut i32) -> u32 {
    let o = options();
    unsafe {
        BITMAP_NAME = z(&bitmap());
        if !name.is_null() {
            *name = if BITMAP_NAME.len() > 1 { BITMAP_NAME.as_ptr() } else { core::ptr::null() };
        }
        if !mode.is_null() {
            // BKGND_NORMAL 0, _TILED 1, _SCALED 2.
            *mode = (o.mode as u32).wrapping_sub(306).min(2);
        }
        if !scale.is_null() {
            *scale = o.scale as u32;
        }
        if !ty.is_null() {
            // BKGND_COLOR 1, BKGND_IMAGE 2.
            *ty = if BITMAP_NAME.len() > 1 { 2 } else { 1 };
        }
        if !color.is_null() {
            *color = o.color;
        }
    }
    1
}

extern "C" fn wpSetLockupBackground(_o: usize, name: *const u8, mode: u32, scale: u32, _ty: u32, color: i32) -> u32 {
    let n = if name.is_null() { Vec::new() } else { cstr(name).to_vec() };
    prf_write(HINI_USER, APP, BITMAP, (!n.is_empty()).then(|| z(&n)).as_deref());
    update(|o| {
        o.mode = 306 + mode.min(2) as u16;
        o.scale = scale.clamp(1, 99) as u16;
        o.color = color;
    });
    1
}

/// IBM's tabs after the folder's: Lockup (three pages), Archive, Desktop
/// (each insertion goes in front of those before it).
extern "C" fn desktop_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPDesktop"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    if wpIsCurrentDesktop(o) == 0 {
        return 1;
    }
    for i in [m::WPDesktop::wpAddDesktopDefDT1Page, m::WPDesktop::wpAddDesktopArcRest1Page, m::WPDesktop::wpAddDesktopLockup3Page, m::WPDesktop::wpAddDesktopLockup2Page, m::WPDesktop::wpAddDesktopLockup1Page] {
        wps::call("WPDesktop", i, &[o, nb as usize]);
    }
    1
}

/// The setup strings of the lockup (IBM's desk_wpSetup).
extern "C" fn desktop_setup(o: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPDesktop"), wps::cd("WPObject"), m::WPObject::wpSetup, &[o, setup as usize]);
    let yes = |v: &[u8]| v.eq_ignore_ascii_case(b"YES");
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        match k.as_slice() {
            b"AUTOLOCKUP" => update(|x| x.auto = yes(&v)),
            b"LOCKUPAUTODIM" => update(|x| x.dim = yes(&v)),
            b"LOCKUPFULLSCREEN" => update(|x| x.full = yes(&v)),
            b"LOCKUPONSTARTUP" => update(|x| x.startup = yes(&v)),
            b"LOCKUPSCREENSAVERMODE" => set_screen_saver(yes(&v)),
            b"LOCKUPTIMEOUT" => {
                if let Some(t) = parse_dec(&v).filter(|t| (1..=99).contains(t)) {
                    update(|x| x.timeout = t as u16);
                }
            }
            // file,mode(n/t/s),scale
            b"LOCKUPBACKGROUND" => {
                let parts: Vec<&[u8]> = v.split(|&c| c == b',').collect();
                let file = parts.first().copied().unwrap_or(b"");
                prf_write(HINI_USER, APP, BITMAP, (!file.is_empty()).then(|| z(file)).as_deref());
                let mode = match parts.get(1).and_then(|m| m.first()).map(|c| c.to_ascii_uppercase()) {
                    Some(b'N') => 306,
                    Some(b'T') => 307,
                    _ => 308,
                };
                let scale = parts.get(2).and_then(|s| parse_dec(s)).unwrap_or(1).clamp(1, 99) as u16;
                update(|x| {
                    x.mode = mode;
                    x.scale = scale;
                });
            }
            _ => {}
        }
    }
    1
}

extern "C" fn desktop_init(o: usize) {
    som::call_parent(wps::ccd("WPDesktop"), wps::cd("WPObject"), m::WPObject::wpInitData, &[o]);
    dd(o).drop_op = DO_LINK;
}

extern "C" fn desktop_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPDesktop"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    obj::wpSaveLong(o, b"WPDesktop\0".as_ptr(), 1, dd(o).drop_op);
    1
}

extern "C" fn desktop_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPDesktop"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(v) = obj::restored_long(o, b"WPDesktop", 1) {
        dd(o).drop_op = v;
    }
    1
}

methods! {
    __m_wpIsCurrentDesktop => wpIsCurrentDesktop, 1;
    __m_wpAddDesktopLockup1Page => wpAddDesktopLockup1Page, 2;
    __m_wpAddDesktopLockup2Page => wpAddDesktopLockup2Page, 2;
    __m_wpAddDesktopLockup3Page => wpAddDesktopLockup3Page, 2;
    __m_wpAddDesktopArcRest1Page => wpAddDesktopArcRest1Page, 2;
    __m_wpAddDesktopDefDT1Page => wpAddDesktopDefDT1Page, 2;
    __m_wpQueryDefaultDropOp => wpQueryDefaultDropOp, 1;
    __m_wpSetDefaultDropOp => wpSetDefaultDropOp, 2;
    __m_wpQueryAutoLockup => wpQueryAutoLockup, 1;
    __m_wpSetAutoLockup => wpSetAutoLockup, 2;
    __m_wpQueryLockupTimeout => wpQueryLockupTimeout, 1;
    __m_wpSetLockupTimeout => wpSetLockupTimeout, 2;
    __m_wpQueryLockupOnStartup => wpQueryLockupOnStartup, 1;
    __m_wpSetLockupOnStartup => wpSetLockupOnStartup, 2;
    __m_wpQueryLockupFullScreen => wpQueryLockupFullScreen, 1;
    __m_wpSetLockupFullScreen => wpSetLockupFullScreen, 2;
    __m_wpQueryLockupAutoDim => wpQueryLockupAutoDim, 1;
    __m_wpSetLockupAutoDim => wpSetLockupAutoDim, 2;
    __m_wpQueryLockupBackground => wpQueryLockupBackground, 6;
    __m_wpSetLockupBackground => wpSetLockupBackground, 6;
    __m_desktop_add_pages => desktop_add_pages, 2;
    __m_desktop_setup => desktop_setup, 2;
    __m_desktop_init => desktop_init, 1;
    __m_desktop_save_state => desktop_save_state, 1;
    __m_desktop_restore_state => desktop_restore_state, 2;
}

pub fn impl_desktop() -> Impl {
    Impl {
        data: core::mem::size_of::<DeskData>() as u32,
        methods: alloc::vec![
            ("wpIsCurrentDesktop", __m_wpIsCurrentDesktop as usize),
            ("wpAddDesktopLockup1Page", __m_wpAddDesktopLockup1Page as usize),
            ("wpAddDesktopLockup2Page", __m_wpAddDesktopLockup2Page as usize),
            ("wpAddDesktopLockup3Page", __m_wpAddDesktopLockup3Page as usize),
            ("wpAddDesktopArcRest1Page", __m_wpAddDesktopArcRest1Page as usize),
            ("wpAddDesktopDefDT1Page", __m_wpAddDesktopDefDT1Page as usize),
            ("wpQueryDefaultDropOp", __m_wpQueryDefaultDropOp as usize),
            ("wpSetDefaultDropOp", __m_wpSetDefaultDropOp as usize),
            ("wpQueryAutoLockup", __m_wpQueryAutoLockup as usize),
            ("wpSetAutoLockup", __m_wpSetAutoLockup as usize),
            ("wpQueryLockupTimeout", __m_wpQueryLockupTimeout as usize),
            ("wpSetLockupTimeout", __m_wpSetLockupTimeout as usize),
            ("wpQueryLockupOnStartup", __m_wpQueryLockupOnStartup as usize),
            ("wpSetLockupOnStartup", __m_wpSetLockupOnStartup as usize),
            ("wpQueryLockupFullScreen", __m_wpQueryLockupFullScreen as usize),
            ("wpSetLockupFullScreen", __m_wpSetLockupFullScreen as usize),
            ("wpQueryLockupAutoDim", __m_wpQueryLockupAutoDim as usize),
            ("wpSetLockupAutoDim", __m_wpSetLockupAutoDim as usize),
            ("wpQueryLockupBackground", __m_wpQueryLockupBackground as usize),
            ("wpSetLockupBackground", __m_wpSetLockupBackground as usize),
        ],
        overrides: alloc::vec![
            ("wpAddSettingsPages", __m_desktop_add_pages as usize),
            ("wpSetup", __m_desktop_setup as usize),
            ("wpInitData", __m_desktop_init as usize),
            ("wpSaveState", __m_desktop_save_state as usize),
            ("wpRestoreState", __m_desktop_restore_state as usize),
        ],
    }
}
