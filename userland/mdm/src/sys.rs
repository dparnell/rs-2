//! The system calls MDM uses: PM (notifications, the video window,
//! drawing), RS2API's audio streams, and a process-wide lock.

#![allow(dead_code)]

pub use os2::dos::*;

pub type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct RectL {
    pub l: i32,
    pub b: i32,
    pub r: i32,
    pub t: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct QMsg {
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u32,
    pub mp2: u32,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub reserved: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.919"]
    pub fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    pub fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.763"]
    pub fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.888"]
    pub fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.716"]
    pub fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    pub fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.915"]
    pub fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, first: u32, last: u32) -> u32;
    #[link_name = "PMWIN.912"]
    pub fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.926"]
    pub fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    pub fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.911"]
    pub fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    pub fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    pub fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.757"]
    pub fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    pub fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.840"]
    pub fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.765"]
    pub fn WinInvalidateRect(h: u32, r: *const RectL, children: u32) -> u32;
    #[link_name = "PMWIN.728"]
    pub fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.875"]
    pub fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.772"]
    pub fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.829"]
    pub fn WinQuerySysValue(desk: u32, i: i32) -> i32;
    #[link_name = "PMWIN.743"]
    pub fn WinFillRect(hps: u32, r: *const RectL, clr: i32) -> u32;
    #[link_name = "PMWIN.704"]
    pub fn WinCalcFrameRect(h: u32, r: *mut RectL, client: u32) -> u32;
    #[link_name = "PMWIN.877"]
    pub fn WinSetWindowText(h: u32, t: *const u8) -> u32;
    #[link_name = "PMWIN.883"]
    pub fn WinShowWindow(h: u32, show: u32) -> u32;
    #[link_name = "PMGPI.603"]
    pub fn GpiDrawBits(hps: u32, bits: *const u8, bmi: *const u8, n: i32, pts: *const PointL, rop: i32, opts: u32) -> i32;

    // RS2API audio streams (the kernel's sound driver and mixer).
    #[link_name = "RS2API.300"]
    pub fn Rs2AudioQuery(devices: *mut u32) -> u32;
    #[link_name = "RS2API.301"]
    pub fn Rs2AudioOpen(rate: u32, channels: u32, bits: u32, h: *mut u32) -> u32;
    #[link_name = "RS2API.302"]
    pub fn Rs2AudioWrite(h: u32, data: *const u8, cb: u32) -> u32;
    #[link_name = "RS2API.303"]
    pub fn Rs2AudioQueryPos(h: u32, bytes: *mut u32) -> u32;
    #[link_name = "RS2API.304"]
    pub fn Rs2AudioDrain(h: u32, timeout: u32) -> u32;
    #[link_name = "RS2API.305"]
    pub fn Rs2AudioStop(h: u32) -> u32;
    #[link_name = "RS2API.306"]
    pub fn Rs2AudioClose(h: u32) -> u32;
    #[link_name = "RS2API.307"]
    pub fn Rs2AudioSetVolume(h: u32, vol: u32) -> u32;
    #[link_name = "RS2API.308"]
    pub fn Rs2AudioPause(h: u32, pause: u32) -> u32;
    #[link_name = "RS2API.309"]
    pub fn Rs2AudioOpenInput(rate: u32, channels: u32, bits: u32, h: *mut u32) -> u32;
    #[link_name = "RS2API.310"]
    pub fn Rs2AudioRead(h: u32, buf: *mut u8, cb: u32, timeout: u32, read: *mut u32) -> u32;
    #[link_name = "RS2API.311"]
    pub fn Rs2AudioQueryLevel(h: u32, level: *mut u32) -> u32;
    #[link_name = "RS2API.312"]
    pub fn Rs2AudioSetInput(source: u32, taken: *mut u32) -> u32;
}

pub const HWND_DESKTOP: u32 = 1;
pub const HWND_TOP: u32 = 3;
pub const SEM_INDEFINITE_WAIT: u32 = 0xffff_ffff;

pub const WM_PAINT: u32 = 0x0023;
pub const WM_CLOSE: u32 = 0x0029;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_USER: u32 = 0x1000;

pub const MM_MCINOTIFY: u32 = 0x0500;
pub const MM_MCIPASSDEVICE: u32 = 0x0501;
pub const MM_MCIPOSITIONCHANGE: u32 = 0x0502;
pub const MCI_GAINING_USE: u32 = 2;
pub const MM_MCICUEPOINT: u32 = 0x0503;
pub const MM_MCIPLAYLISTMESSAGE: u32 = 0x0504;

pub fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

/// A process-wide mutex semaphore, created on first use.
pub struct Lock(core::sync::atomic::AtomicU32);

impl Lock {
    pub const fn new() -> Lock {
        Lock(core::sync::atomic::AtomicU32::new(0))
    }

    fn handle(&self) -> u32 {
        use core::sync::atomic::Ordering;
        let h = self.0.load(Ordering::Acquire);
        if h != 0 {
            return h;
        }
        let mut n = 0;
        unsafe { DosCreateMutexSem(core::ptr::null(), &mut n, 0, 0) };
        match self.0.compare_exchange(0, n, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => n,
            Err(other) => {
                unsafe { DosCloseMutexSem(n) };
                other
            }
        }
    }

    pub fn hold(&self) -> Guard<'_> {
        let h = self.handle();
        unsafe { DosRequestMutexSem(h, SEM_INDEFINITE_WAIT) };
        Guard(self, h)
    }
}

pub struct Guard<'a>(&'a Lock, u32);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        unsafe { DosReleaseMutexSem(self.1) };
    }
}

/// A NUL-terminated string from a caller.
pub fn cstr(p: *const u8) -> alloc::string::String {
    let mut s = alloc::string::String::new();
    if p.is_null() {
        return s;
    }
    let mut i = 0;
    loop {
        let b = unsafe { *p.add(i) };
        if b == 0 {
            return s;
        }
        s.push(b as char);
        i += 1;
    }
}

/// Copy text into a caller's buffer of `cb` bytes, NUL-terminated.
pub fn copy_out(t: &str, buf: *mut u8, cb: u32) -> bool {
    if buf.is_null() || cb == 0 {
        return false;
    }
    let bytes: alloc::vec::Vec<u8> = t.chars().map(|c| c as u32 as u8).collect();
    let n = bytes.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    bytes.len() < cb as usize
}

pub fn rd(p: u32, off: u32) -> u32 {
    unsafe { ((p + off) as *const u32).read_unaligned() }
}

pub fn rd16(p: u32, off: u32) -> u16 {
    unsafe { ((p + off) as *const u16).read_unaligned() }
}

pub fn wr(p: u32, off: u32, v: u32) {
    unsafe { ((p + off) as *mut u32).write_unaligned(v) }
}

pub fn wr16(p: u32, off: u32, v: u16) {
    unsafe { ((p + off) as *mut u16).write_unaligned(v) }
}

/// A line on the serial port (RS2API.4), for debugging.
pub fn trace(args: core::fmt::Arguments) {
    unsafe extern "C" {
        #[link_name = "RS2API.4"]
        fn Rs2DebugTrace(psz: *const u8) -> u32;
    }
    let mut s = alloc::format!("{}", args).into_bytes();
    s.push(0);
    unsafe { Rs2DebugTrace(s.as_ptr()) };
}

/// The whole file, read with DosOpen/DosRead.
unsafe extern "C" {
    #[link_name = "MMIO.27"]
    fn mmioOpen(name: *const u8, info: *mut u8, flags: u32) -> u32;
    #[link_name = "MMIO.28"]
    fn mmioRead(h: u32, buf: *mut u8, n: i32) -> i32;
    #[link_name = "MMIO.32"]
    fn mmioClose(h: u32, flags: u32) -> u32;
}

/// A compound MMIO name ("A.EXT+ELEMENT": an element of a storage system
/// whose I/O procedure the extension names, such as IBM's OpenDoc
/// multimedia parts' ".SU+..." storage unit data), read through MMIO.
fn read_compound(z: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    let plus = z.iter().position(|&b| b == b'+')?;
    if !z[..plus].contains(&b'.') {
        return None;
    }
    // MMIO_READ | MMIO_DENYNONE | MMIO_NOIDENTIFY: the bytes as stored.
    let h = unsafe { mmioOpen(z.as_ptr(), core::ptr::null_mut(), 0x4 | 0x200 | 0x4_0000) };
    if h == 0 {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    let mut buf = alloc::vec![0u8; 32768];
    loop {
        let n = unsafe { mmioRead(h, buf.as_mut_ptr(), buf.len() as i32) };
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { mmioClose(h, 0) };
    Some(out)
}

pub fn read_file(name: &str) -> Option<alloc::vec::Vec<u8>> {
    let mut z: alloc::vec::Vec<u8> = name.chars().map(|c| c as u32 as u8).collect();
    z.push(0);
    if let Some(v) = read_compound(&z) {
        return Some(v);
    }
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, OPEN_SHARE_DENYWRITE | OPEN_ACCESS_READONLY.
    if unsafe { DosOpen(z.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    let mut buf = alloc::vec![0u8; 32768];
    loop {
        let mut n = 0;
        if unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(h) };
    Some(out)
}

/// Write a whole file (created or replaced) with DosOpen/DosWrite.
pub fn write_file(name: &str, data: &[u8]) -> bool {
    let mut z: alloc::vec::Vec<u8> = name.chars().map(|c| c as u32 as u8).collect();
    z.push(0);
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
    // OPEN_SHARE_DENYWRITE | OPEN_ACCESS_WRITEONLY.
    if unsafe { DosOpen(z.as_ptr(), &mut h, &mut act, data.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut ok = true;
    for piece in data.chunks(32768) {
        let mut n = 0;
        if unsafe { DosWrite(h, piece.as_ptr(), piece.len() as u32, &mut n) } != 0 || n as usize != piece.len() {
            ok = false;
            break;
        }
    }
    unsafe { DosClose(h) };
    ok
}

/// The master volume Volume Control keeps in OS2.INI (`MMPM2` `MasterVolume`,
/// `Mute`), given to the kernel mixer once per process: the mixer starts at
/// 100 when rs2 starts.
pub fn master_volume_once() {
    use core::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    unsafe extern "C" {
        #[link_name = "PMSHAPI.115"]
        fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, cb: u32) -> u32;
    }
    let get = |key: &[u8]| {
        let mut b = [0u8; 16];
        let n = unsafe { PrfQueryProfileString(1, b"MMPM2\0".as_ptr(), key.as_ptr(), core::ptr::null(), b.as_mut_ptr(), 16) };
        (n > 0).then(|| b.iter().take_while(|&&c| c != 0).fold(0u32, |v, &c| if c.is_ascii_digit() { v * 10 + (c - b'0') as u32 } else { v }))
    };
    let Some(v) = get(b"MasterVolume\0") else { return };
    let v = if get(b"Mute\0") == Some(1) { 0 } else { v.min(100) };
    unsafe { Rs2AudioSetVolume(0, v) };
}
