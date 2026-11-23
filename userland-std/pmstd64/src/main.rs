//! PMSTD64.EXE: a 64-bit Presentation Manager program in Rust with the
//! standard library (docs/STD64.md).
//!
//! A worker thread walks a directory (argument, default C:\) and sends what
//! it finds through a channel; the window shows the running totals by file
//! extension. With `/SELFTEST` it checks that std and PM work together
//! (thread-locals in window procedures called back from the 32-bit PMWIN,
//! String data passed to PM, std threads posting messages, panics caught
//! in a window procedure), closes itself and prints PASS/FAIL lines.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use os2::*;
// (os2 has a println of its own.)
use std::println;
use rs2libc as _;

os2::app_type!(PM);

const CLASS: &CStr = c"PmStd64";
const WM_FOUND: ULONG = WM_USER + 1;

#[derive(Default)]
struct State {
    files: u64,
    bytes: u64,
    dirs: u64,
    kinds: BTreeMap<String, u64>,
    done: bool,
}

enum Found {
    File { ext: String, len: u64 },
    Dir,
    Done,
}

// PM calls a window's procedure on the thread that created it: its data can
// be thread-local. (PMWIN is 32-bit code; FS is the thread pointer again in
// the procedure.)
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static RX: RefCell<Option<Receiver<Found>>> = const { RefCell::new(None) };
    static PAINTS: Cell<u32> = const { Cell::new(0) };
    static CAUGHT: Cell<bool> = const { Cell::new(false) };
    static SELFTEST: Cell<bool> = const { Cell::new(false) };
}

fn walk(dir: &Path, depth: u32, tx: &Sender<Found>, notify: &dyn Fn()) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(m) = e.metadata() else { continue };
        let found = if m.is_dir() {
            Found::Dir
        } else {
            let ext = e.path().extension().map(|x| x.to_string_lossy().to_uppercase()).unwrap_or_default();
            Found::File { ext, len: m.len() }
        };
        let dir = m.is_dir();
        if tx.send(found).is_err() {
            return;
        }
        notify();
        if dir && depth > 0 {
            walk(&e.path(), depth - 1, tx, notify);
        }
    }
}

fn text_at(hps: HPS, x: i32, y: i32, s: &str) {
    let mut p = POINTL { x, y };
    unsafe { GpiCharStringAt(hps, &mut p, s.len() as i32, s.as_ptr() as PCH) };
}

fn paint(hwnd: HWND) {
    let mut r = RECTL::default();
    unsafe {
        let hps = WinBeginPaint(hwnd, 0, std::ptr::null_mut());
        WinQueryWindowRect(hwnd, &mut r);
        WinFillRect(hps, &mut r, CLR_WHITE);
        GpiSetColor(hps, CLR_DARKBLUE);
        STATE.with_borrow(|s| {
            let mut y = r.yTop - 30;
            let what = if s.done { "done" } else { "looking" };
            text_at(hps, 20, y, &format!("{} files in {} directories, {:.1} MB ({what})", s.files, s.dirs, s.bytes as f64 / 1e6));
            GpiSetColor(hps, CLR_BLACK);
            let mut kinds: Vec<_> = s.kinds.iter().collect();
            kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (k, n) in kinds.iter().take(10) {
                y -= 22;
                text_at(hps, 20, y, &format!("{:>6}  .{k}", n));
            }
        });
        WinEndPaint(hps);
    }
    PAINTS.set(PAINTS.get() + 1);
}

unsafe extern "C" fn client_proc(hwnd: HWND, msg: ULONG, mp1: MPARAM, mp2: MPARAM) -> MRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            MRVOID
        }
        WM_FOUND => {
            let done = RX.with_borrow(|rx| {
                let Some(rx) = rx else { return false };
                STATE.with_borrow_mut(|s| {
                    for f in rx.try_iter() {
                        match f {
                            Found::File { ext, len } => {
                                s.files += 1;
                                s.bytes += len;
                                *s.kinds.entry(ext).or_default() += 1;
                            }
                            Found::Dir => s.dirs += 1,
                            Found::Done => s.done = true,
                        }
                    }
                    s.done
                })
            });
            unsafe { WinInvalidateRect(hwnd, std::ptr::null_mut(), 0) };
            if done && SELFTEST.get() {
                let r = std::panic::catch_unwind(|| {
                    let v: Vec<u32> = Vec::new();
                    v[std::hint::black_box(1)]
                });
                CAUGHT.set(r.is_err());
                unsafe { WinPostMsg(hwnd, WM_TIMER, MPVOID, MPVOID) };
            }
            MRVOID
        }
        WM_TIMER => {
            unsafe {
                WinUpdateWindow(hwnd);
                WinPostMsg(hwnd, WM_CLOSE, MPVOID, MPVOID);
            }
            MRVOID
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

fn main() {
    let mut dir = PathBuf::from("C:\\");
    for a in std::env::args().skip(1) {
        if a.eq_ignore_ascii_case("/SELFTEST") {
            SELFTEST.set(true);
        } else {
            dir = a.into();
        }
    }
    let selftest = SELFTEST.get();
    if selftest {
        std::panic::set_hook(Box::new(|_| {}));
    }
    let mut failed = 0;
    let mut check = |name: &str, ok: bool| {
        failed += !ok as i32;
        if selftest {
            println!("{}: {name}", if ok { "PASS" } else { "FAIL" });
        }
    };
    unsafe {
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        check("WinInitialize and WinCreateMsgQueue", hab != 0 && hmq != 0);
        check("WinRegisterClass", WinRegisterClass(hab, CLASS.as_ptr().cast(), Some(client_proc), CS_SIZEREDRAW, 0) != 0);
        // A title from the heap.
        let title = CString::new(format!("Files of {}", dir.display())).unwrap();
        let mut fl = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST;
        let mut client = 0;
        let frame =
            WinCreateStdWindow(HWND_DESKTOP, 0, &mut fl, CLASS.as_ptr().cast(), title.as_ptr().cast(), 0, 0, 1, &mut client);
        check("WinCreateStdWindow", frame != 0 && client != 0);
        let mut text = [0u8; 64];
        let n = WinQueryWindowText(frame, 64, text.as_mut_ptr().cast()) as usize;
        check("a String as the title", &text[..n] == title.to_bytes());
        WinSetWindowPos(frame, HWND_TOP, 80, 80, 460, 320, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);

        let (tx, rx) = channel();
        RX.set(Some(rx));
        let worker = std::thread::spawn(move || {
            walk(&dir, if selftest { 1 } else { 16 }, &tx, &|| {
                WinPostMsg(client, WM_FOUND, MPVOID, MPVOID);
            });
            let _ = tx.send(Found::Done);
            // The queue may be full: until the message is in.
            while WinPostMsg(client, WM_FOUND, MPVOID, MPVOID) == 0 && WinIsWindow(hab, client) != 0 {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let mut q = QMSG::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &mut q);
        }
        RX.set(None); // the worker stops at its next send
        check("the worker thread ended", worker.join().is_ok());
        check("thread-locals in the window procedure", STATE.with_borrow(|s| s.done && s.files > 10 && s.dirs > 2));
        check("WM_PAINT drew", PAINTS.get() > 0);
        check("a panic caught in the window procedure", CAUGHT.get() || !selftest);
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    if selftest {
        println!("pmstd64 selftest done: {failed} failure(s)");
    }
    std::process::exit(failed);
}
