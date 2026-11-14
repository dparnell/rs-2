//! RS2\PMBENCH.EXE: times the screen updates PM makes most often, for
//! measuring how fast the display path is (`dispstat` on the kernel
//! command line adds what the kernel spends presenting). A 640x420
//! window full of text lines is scrolled a line at a time (WinScrollWindow
//! and painting the line that comes in), then moved 8 pixels at a time
//! as a frame drag would (WinSetWindowPos, everything repainted). Each
//! phase's time goes to the serial log (`PMBENCH: ...`). `PMBENCH SCROLL`
//! stops after scrolling and leaves the window up; `PMBENCH HOLD` ends
//! by moving it under another window and leaves both up.

#![no_std]
#![no_main]

os2::entry!(main);

#[repr(C)]
#[derive(Default)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct RectL {
    l: i32,
    b: i32,
    r: i32,
    t: i32,
}

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.913"]
    fn WinDrawText(hps: u32, cch: i32, text: *const u8, r: *mut RectL, fore: i32, back: i32, fl: u32) -> i32;
    #[link_name = "PMWIN.849"]
    fn WinScrollWindow(h: u32, dx: i32, dy: i32, scroll: *const RectL, clip: *const RectL, hrgn: u32, upd: *mut RectL, fl: u32) -> i32;
    #[link_name = "PMWIN.892"]
    fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
}

const WM_PAINT: u32 = 0x23;
const WM_USER: u32 = 0x1000;
const LINE: i32 = 16;
const STEPS: i32 = 150;

/// The first text line shown (scrolling adds one per step).
static mut FIRST: i32 = 0;
/// `PMBENCH SCROLL`: stop after scrolling, leaving the window up (to
/// look at what scrolling drew).
static mut SCROLL_ONLY: bool = false;
/// `PMBENCH HOLD`: at the end, put another window over this one, move
/// this one under it, and leave both up (to look at what moving drew).
static mut HOLD: bool = false;

fn ms() -> u32 {
    let mut v = 0u32;
    unsafe { os2::DosQuerySysInfo(14, 14, &mut v, 4) };
    v
}

fn fmt_line(n: i32, buf: &mut [u8; 96]) -> usize {
    let head = b"Line ";
    buf[..head.len()].copy_from_slice(head);
    let mut i = head.len();
    let mut d = [0u8; 10];
    let (mut v, mut k) = (n as u32, 0);
    loop {
        d[k] = b'0' + (v % 10) as u8;
        k += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while k > 0 {
        k -= 1;
        buf[i] = d[k];
        i += 1;
    }
    let tail = b": The quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJ";
    buf[i..i + tail.len()].copy_from_slice(tail);
    i + tail.len()
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut upd = RectL::default();
                let hps = WinBeginPaint(h, 0, &mut upd);
                let mut r = RectL::default();
                WinQueryWindowRect(h, &mut r);
                WinFillRect(hps, &upd, -2); // CLR_WHITE
                let first = FIRST;
                let mut buf = [0u8; 96];
                for i in 0..(r.t - r.b) / LINE + 1 {
                    let top = r.t - i * LINE;
                    if top - LINE >= upd.t || top <= upd.b {
                        continue;
                    }
                    let n = fmt_line(first + i, &mut buf);
                    let mut lr = RectL { l: 4, b: top - LINE, r: r.r, t: top };
                    // CLR_BLACK on CLR_WHITE / CLR_BLUE every fifth line.
                    let fore = if (first + i) % 5 == 0 { 1 } else { -1 };
                    WinDrawText(hps, n as i32, buf.as_ptr(), &mut lr, fore, -2, 0);
                }
                WinEndPaint(hps);
                0
            }
            _ => WinDefWindowProc(h, msg, mp1, mp2),
        }
    }
}

fn run(frame: u32, client: u32) {
    unsafe {
        let (x0, y0) = (40, 60);
        // SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER
        WinSetWindowPos(frame, 3, x0, y0, 640, 420, 0x1 | 0x2 | 0x8 | 0x80 | 0x4);
        WinUpdateWindow(1);
        DosSleep(2000);

        // Scroll: the contents go up a line, the new line is painted.
        let t = ms();
        let mut in_call = 0;
        for _ in 0..STEPS {
            FIRST += 1;
            let c = ms();
            WinScrollWindow(client, 0, LINE, core::ptr::null(), core::ptr::null(), 0, core::ptr::null_mut(), 2);
            in_call += ms().wrapping_sub(c);
            WinUpdateWindow(client);
        }
        let scroll = ms().wrapping_sub(t);
        os2::println!("PMBENCH: scroll {STEPS} lines: {scroll} ms ({} ms/step; WinScrollWindow {in_call} ms, painting {} ms)", scroll / STEPS as u32, scroll - in_call);
        if SCROLL_ONLY {
            os2::println!("PMBENCH: done");
            loop {
                DosSleep(u32::MAX);
            }
        }
        DosSleep(2000);

        // Move: 8 pixels right (and back), as a drag does.
        let t = ms();
        let mut in_call = 0;
        for i in 0..STEPS {
            let k = i % 40;
            let x = x0 + if k < 20 { k } else { 40 - k } * 8;
            let c = ms();
            WinSetWindowPos(frame, 0, x, y0, 0, 0, 0x2);
            in_call += ms().wrapping_sub(c);
            WinUpdateWindow(1);
        }
        let mv = ms().wrapping_sub(t);
        os2::println!("PMBENCH: move {STEPS} times: {mv} ms ({} ms/step; WinSetWindowPos {in_call} ms, painting {} ms)", mv / STEPS as u32, mv - in_call);
        if HOLD {
            let mut fcf = 0x1 | 0x8;
            let mut c2 = 0;
            let cover = WinCreateStdWindow(1, 0, &mut fcf, b"PmBench\0".as_ptr(), b"Cover\0".as_ptr(), 0, 0, 2, &mut c2);
            WinSetWindowPos(cover, 3, 420, 300, 300, 260, 0x1 | 0x2 | 0x8 | 0x4);
            WinUpdateWindow(1);
            for i in 0..12 {
                WinSetWindowPos(frame, 0, x0 + 8 * i, y0 + 6 * i, 0, 0, 0x2);
                WinUpdateWindow(1);
            }
            os2::println!("PMBENCH: done");
            loop {
                DosSleep(u32::MAX);
            }
        }
        DosSleep(2000);
        os2::println!("PMBENCH: done");
    }
}

fn main(s: &os2::Startup) -> u32 {
    unsafe {
        SCROLL_ONLY = s.args().trim_ascii().eq_ignore_ascii_case(b"scroll");
        HOLD = s.args().trim_ascii().eq_ignore_ascii_case(b"hold");
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        if hab == 0 || hmq == 0 {
            return 1;
        }
        WinRegisterClass(hab, b"PmBench\0".as_ptr(), client_proc, 0, 0);
        // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST
        let mut fcf = 0x1 | 0x2 | 0x8 | 0x30 | 0x800;
        let mut client = 0;
        let frame = WinCreateStdWindow(1, 0, &mut fcf, b"PmBench\0".as_ptr(), b"PM bench\0".as_ptr(), 0, 0, 1, &mut client);
        WinPostMsg(client, WM_USER, 0, 0);
        let mut q = QMsg::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            if q.msg == WM_USER {
                run(frame, client);
                break;
            }
            WinDispatchMsg(hab, &q);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
