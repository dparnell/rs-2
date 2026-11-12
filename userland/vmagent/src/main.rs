//! VMAGENT.EXE: rs2's guest agent (docs/AGENT.md). It shares the
//! clipboard's text with the host of the virtual machine, through
//! whichever channel the hypervisor has:
//!
//! - the SPICE agent's virtio-serial port (`\DEV\VDAGENT$`): QEMU's own
//!   window (qemu-vdagent), SPICE clients (virt-viewer, Proxmox, libvirt),
//!   UTM ([`vdagent`]).
//!
//! The Workplace Shell starts it once per PM session; a second copy, or
//! one in a machine without such a channel, ends at once. A thread reads
//! the channel and posts what arrives to the agent's object window, which
//! does the clipboard work. The agent follows the clipboard through
//! PMWIN's change count (Rs2QueryClipbrdSeq), not as its viewer: there
//! is one viewer, the Clipboard Viewer wants it.

#![no_std]
#![no_main]

extern crate alloc;

mod vdagent;

use alloc::boxed::Box;
use alloc::vec::Vec;

use pmkit::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.9034"]
    fn Rs2QueryClipbrdSeq() -> u32;
}

const CF_TEXT: u32 = 1;
const CFI_POINTER: u32 = 0x0400;

/// From the reader thread: the host's clipboard text (mp1: a
/// `Box<Vec<u8>>` of CF_TEXT bytes, without NUL).
const WM_HOST_TEXT: u32 = WM_USER + 1;
/// From the reader thread: the host wants the guest's text.
const WM_HOST_REQUEST: u32 = WM_USER + 2;
/// From the reader thread: the host side is there (tell it what the
/// guest's clipboard has).
const WM_HOST_READY: u32 = WM_USER + 3;

const TIMER: u32 = 1;
const POLL_MS: u32 = 250;

/// The channel to the host.
pub enum Backend {
    Vdagent(vdagent::Vdagent),
}

impl Backend {
    /// The guest's clipboard changed: its text, if any (CF_TEXT bytes).
    fn local_changed(&self, text: Option<&[u8]>) {
        match self {
            Backend::Vdagent(v) => v.local_changed(text),
        }
    }

    /// Answer the host's request for the guest's text.
    fn reply(&self, text: Option<&[u8]>) {
        match self {
            Backend::Vdagent(v) => v.reply(text),
        }
    }

    /// The reader thread's work (does not return).
    fn read_loop(&self) {
        match self {
            Backend::Vdagent(v) => v.read_loop(),
        }
    }
}

struct State {
    /// `/TEST`: say what arrives, put a test text on the clipboard.
    test: bool,
    hab: u32,
    win: u32,
    backend: Backend,
    /// The code page of CF_TEXT.
    cp: u16,
    /// The clipboard change count last seen.
    seq: u32,
}

static mut STATE: Option<State> = None;

#[allow(static_mut_refs)]
fn st() -> &'static mut State {
    unsafe { STATE.as_mut().unwrap() }
}

/// Tell the agent's window (any thread).
pub fn post_host(msg: u32, mp1: u32) {
    let s = st();
    if s.win != 0 {
        post(s.win, msg, mp1, 0);
    }
}

/// The host's text arrived (as UTF-8 or UTF-16 decoded by the backend
/// into CF_TEXT bytes).
pub fn host_text(text: Vec<u8>) {
    let b = Box::into_raw(Box::new(text));
    post_host(WM_HOST_TEXT, b as u32);
}

pub fn code_page() -> u16 {
    st().cp
}

fn query_cp() -> u16 {
    let mut cps = [0u32; 4];
    let mut cb = 0;
    if unsafe { os2::DosQueryCp(16, cps.as_mut_ptr(), &mut cb) } == 0 && cb >= 4 && cps[0] != 0 {
        cps[0] as u16
    } else {
        850
    }
}

/// CF_TEXT on the clipboard, without its NUL.
fn clipboard_text(hab: u32) -> Option<Vec<u8>> {
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        return None;
    }
    let p = unsafe { WinQueryClipbrdData(hab, CF_TEXT) } as *mut u8;
    let mut v = None;
    if !p.is_null() {
        let (mut cb, mut fl) = (0u32, 0u32);
        let max = if unsafe { os2::DosQueryMem(p, &mut cb, &mut fl) } == 0 { cb as usize } else { 0 };
        let mut t = Vec::new();
        for i in 0..max {
            let b = unsafe { *p.add(i) };
            if b == 0 {
                break;
            }
            t.push(b);
        }
        v = Some(t);
    }
    unsafe { WinCloseClipbrd(hab) };
    v
}

/// Put text on the clipboard, replacing what is there.
fn set_clipboard_text(hab: u32, t: &[u8]) -> bool {
    let mut p: *mut u8 = core::ptr::null_mut();
    let flags = os2::PAG_COMMIT | os2::PAG_READ | os2::PAG_WRITE | os2::OBJ_GIVEABLE;
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), t.len() as u32 + 1, flags) } != 0 {
        return false;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), p, t.len());
        *p.add(t.len()) = 0;
    }
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        unsafe { os2::DosFreeMem(p) };
        return false;
    }
    let ok = unsafe {
        WinEmptyClipbrd(hab);
        let ok = WinSetClipbrdData(hab, p as u32, CF_TEXT, CFI_POINTER) != 0;
        WinCloseClipbrd(hab);
        ok
    };
    ok
}

extern "C" fn wnd_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let s = st();
    match m {
        WM_TIMER if mp1 == TIMER => {
            let seq = unsafe { Rs2QueryClipbrdSeq() };
            if seq != s.seq {
                s.seq = seq;
                let t = clipboard_text(s.hab);
                if s.test {
                    os2::println!("VMAGENT: guest clipboard changed ({} bytes of text)", t.as_ref().map_or(0, |t| t.len()));
                }
                s.backend.local_changed(t.as_deref());
            }
            0
        }
        WM_HOST_TEXT => {
            let t = unsafe { Box::from_raw(mp1 as *mut Vec<u8>) };
            if s.test {
                let mut show = alloc::string::String::new();
                for &b in t.iter() {
                    if (0x20..0x7f).contains(&b) {
                        show.push(b as char);
                    } else {
                        show.push_str(&alloc::format!("\\x{b:02x}"));
                    }
                }
                os2::println!("VMAGENT: host text \"{show}\" (code page {})", s.cp);
            }
            if clipboard_text(s.hab).as_deref() != Some(&t[..]) {
                set_clipboard_text(s.hab, &t);
            }
            // The change is the host's: not given back to it.
            s.seq = unsafe { Rs2QueryClipbrdSeq() };
            if s.test {
                // An answer, which the timer offers to the host.
                let mut a = b"rs2 got ".to_vec();
                a.extend_from_slice(&pmkit::num(t.len() as i64));
                a.extend_from_slice(b" bytes: caf\x82");
                set_clipboard_text(s.hab, &a);
            }
            0
        }
        WM_HOST_REQUEST => {
            let t = clipboard_text(s.hab);
            s.backend.reply(t.as_deref());
            0
        }
        WM_HOST_READY => {
            if s.test {
                os2::println!("VMAGENT: host ready");
                // "rs2 to host: café" in code page 850.
                if !set_clipboard_text(s.hab, b"rs2 to host: caf\x82") {
                    os2::println!("VMAGENT: cannot set the clipboard");
                }
                // The timer offers it to the host.
                return 0;
            }
            if let Some(t) = clipboard_text(s.hab).filter(|t| !t.is_empty()) {
                s.backend.local_changed(Some(&t));
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, m, mp1, mp2) },
    }
}

unsafe extern "C" fn reader(_: u32) {
    st().backend.read_loop();
}

fn main(s: &os2::Startup) -> u32 {
    let test = arg(s).windows(5).any(|w| w.eq_ignore_ascii_case(b"/TEST"));
    // One agent.
    let mut mtx = 0;
    if unsafe { os2::DosCreateMutexSem(b"\\SEM32\\RS2\\VMAGENT\0".as_ptr(), &mut mtx, 0, 0) } != 0 {
        return 0;
    }
    let Some(backend) = vdagent::Vdagent::open().map(Backend::Vdagent) else {
        // No hypervisor channel.
        if test {
            os2::println!("VMAGENT: no channel to a host");
        }
        return 1;
    };
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    if hmq == 0 {
        return 2;
    }
    unsafe { STATE = Some(State { test, hab, win: 0, backend, cp: query_cp(), seq: Rs2QueryClipbrdSeq() }) };
    let class = b"RS2VMAgent\0";
    unsafe { WinRegisterClass(hab, class.as_ptr(), wnd_proc, 0, 0) };
    let win = unsafe {
        WinCreateWindow(HWND_OBJECT, class.as_ptr() as u32, b"\0".as_ptr(), 0, 0, 0, 0, 0, 0, 0, 0, core::ptr::null(), core::ptr::null())
    };
    if win == 0 {
        return 3;
    }
    st().win = win;
    unsafe { WinStartTimer(hab, win, TIMER, POLL_MS) };
    let mut tid = 0;
    // CREATE_READY, a committed stack.
    unsafe { os2::DosCreateThread(&mut tid, reader, 0, 2, 64 * 1024) };
    if test {
        os2::println!("VMAGENT: started");
    }
    st().backend.start();
    message_loop(hab);
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

impl Backend {
    /// The first messages to the host (after the reader runs).
    fn start(&self) {
        match self {
            Backend::Vdagent(v) => v.start(),
        }
    }
}

/// Ask the main thread for the guest's text (any thread).
pub fn host_request() {
    post_host(WM_HOST_REQUEST, 0);
}

/// The host is there (any thread).
pub fn host_ready() {
    post_host(WM_HOST_READY, 0);
}
