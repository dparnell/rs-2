//! PMREXXIO.DLL: a Presentation Manager window for the standard input,
//! output and error of REXX procedures (and of the commands they run),
//! as IBM's. PMREXX.EXE runs procedures in it; other programs can too.
//!
//! RexxCreateIOWindow(owner, &client) makes the window (hidden until
//! there is output or input is wanted) and points handles 0, 1 and 2 of
//! the process at pipes: what is written to 1 and 2 appears in the
//! window's read-only MLE, lines typed in its entry field are written to
//! 0. A REXX I/O exit "PMREXXIO" (RXSIO, registered with
//! RexxRegisterExitExe) answers interactive trace pauses from the entry
//! field and the Trace menu, and shows the window when the procedure
//! reads. The program runs RexxStart on a thread of its own after posting
//! REXX_STARTPROC (WM_USER+1, the thread id) to the client, posts
//! REXX_ENDPROC (WM_USER+2) when it returns, and calls
//! RexxDestroyIOWindow(frame) at the end. Halt and the trace items call
//! REXX's RexxSetHalt/RexxSetTrace/RexxResetTrace for that thread.

#![no_std]
#![no_main]

extern crate alloc;
extern crate os2;

mod api;
mod ids;
mod tpl;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::SeqCst};

use api::*;
use ids::*;

const CLASS: &[u8] = b"PMREXXIO\0";
const EXIT_NAME: &[u8] = b"PMREXXIO\0";
const PROFILE_APP: &[u8] = b"PMREXX Interface\0";
const BORDER: i32 = 3;
/// Longest input line.
const ANSLEN: u32 = 128;
/// Output is gathered in a buffer of this size (less IBM's 12-byte
/// header) and added to the MLE when it is full or 100 ms after the last
/// line.
const IMPORT_SIZE: usize = 0x2000;
const IMPORT_HDR: usize = 12;
/// Lines of output are read up to this length.
const MAXWIDTH: usize = 200;
const PIPE_SIZE: u32 = 0x2000;
const TIMER_ID: u32 = 1;
const TIMER_DELAY: u32 = 100;
const STACK: u32 = 0x10000;
/// OPEN_FLAGS_NOINHERIT, which IBM's PMREXXIO sets on the handles it
/// means to be inherited (and clears on the others): kept as IBM's.
const NOINHERIT: u32 = 0x80;

/// The DLL's entry point (_DLL_InitTerm): nothing to set up.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// Entry points called from other modules (whose code may keep only
/// 4-byte stack alignment) realign the stack for Rust code and copy the
/// arguments.
macro_rules! aligned {
    ($($(#[$m:meta])* $tramp:ident => $target:path, $copy:literal;)*) => {$(
        $(#[$m])*
        #[unsafe(naked)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, {n}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const ($copy * 4 + 15) / 16 * 16, n = const $copy, f = sym $target,
            );
        }
    )*};
}

aligned! {
    #[unsafe(no_mangle)] RexxCreateIOWindow => create_io_window, 2;
    #[unsafe(no_mangle)] RexxDestroyIOWindow => destroy_io_window, 1;
    a_client_proc => client_proc, 4;
    a_entry_proc => entry_proc, 4;
    a_search_proc => search_proc, 4;
    a_io_exit => io_exit, 3;
    a_stdin_thread => stdin_thread, 1;
    a_stdout_thread => stdout_thread, 1;
}

fn proc_ptr(f: extern "C" fn()) -> WndProc {
    unsafe { core::mem::transmute(f) }
}

fn thread_ptr(f: extern "C" fn()) -> os2::ThreadFn {
    unsafe { core::mem::transmute(f) }
}

// ---- Strings ----

/// The window's texts by IBM's string numbers (message boxes posted with
/// RXIOB_MSGBOX name them).
fn string(id: u32) -> &'static [u8] {
    match id {
        1 => b"REXX Input and Output\0",
        2 => b"The output window could not be made.\0",
        3 => b"The output could not be added to the window.\0",
        4 => b"The output buffer could not be given to the window.\0",
        5 => b"Input:\0",
        6 => b"PMREXX Error\0",
        7 => b"Help\0",
        8 => b"Help is not available for this window.\0",
        9 => b"PMREXXIO.HLP\0",
        10 => b"PMREXX Help\0",
        11 => b"The help could not be attached to the window.\0",
        12 => b"The help could not be started.\0",
        13 => b"The input and output pipes could not be set up.\0",
        16 => b"Clipboard\0",
        18 => b"The text could not be cut.\0",
        19 => b"The text could not be deleted.\0",
        21 => b"The text could not be copied.\0",
        22 => b"There is not enough memory.\0",
        23 => b"The quick brown fox 0123456789\0",
        274 => b"PMREXX Font\0",
        9002 => b"PMREXX Find\0",
        9003 => b"The text was not found.\0",
        _ => b"\0",
    }
}

fn error_box(owner: u32, id: u32, style: u32) {
    unsafe { WinMessageBox(HWND_DESKTOP, owner, string(id).as_ptr(), core::ptr::null(), 0, style) };
}

// ---- Semaphore-guarded queues of input lines ----

struct Mutex(u32);

impl Mutex {
    fn new() -> Self {
        let mut h = 0;
        unsafe { os2::DosCreateMutexSem(core::ptr::null(), &mut h, 0, 0) };
        Mutex(h)
    }
    fn lock(&self) -> bool {
        unsafe { os2::DosRequestMutexSem(self.0, 0xffff_ffff) == 0 }
    }
    fn unlock(&self) {
        unsafe { os2::DosReleaseMutexSem(self.0) };
    }
}

/// Lines for a reader: the stdin pipe's writer thread, or the trace exit.
struct Queue {
    mtx: Mutex,
    ev: u32,
    items: UnsafeCell<VecDeque<Vec<u8>>>,
}

impl Queue {
    fn new() -> Self {
        let mut ev = 0;
        unsafe { os2::DosCreateEventSem(core::ptr::null(), &mut ev, 0, 0) };
        Queue { mtx: Mutex::new(), ev, items: UnsafeCell::new(VecDeque::new()) }
    }

    fn add(&self, line: Vec<u8>) {
        if self.mtx.lock() {
            unsafe { (*self.items.get()).push_back(line) };
            unsafe { os2::DosPostEventSem(self.ev) };
            self.mtx.unlock();
        }
    }

    /// The next line, waiting for one.
    fn remove(&self) -> Vec<u8> {
        loop {
            if unsafe { os2::DosWaitEventSem(self.ev, 0xffff_ffff) } == 0 && self.mtx.lock() {
                let items = unsafe { &mut *self.items.get() };
                let line = items.pop_front();
                if items.is_empty() {
                    let mut n = 0;
                    unsafe { os2::DosResetEventSem(self.ev, &mut n) };
                }
                self.mtx.unlock();
                if let Some(l) = line {
                    return l;
                }
            }
        }
    }

    fn clear(&self) {
        if self.mtx.lock() {
            unsafe { (*self.items.get()).clear() };
            let mut n = 0;
            unsafe { os2::DosResetEventSem(self.ev, &mut n) };
            self.mtx.unlock();
        }
    }

    fn close(&self) {
        unsafe {
            os2::DosCloseMutexSem(self.mtx.0);
            os2::DosCloseEventSem(self.ev);
        }
    }
}

// ---- The window's data ----

struct Io {
    hab: u32,
    frame: u32,
    client: u32,
    mle: u32,
    entry: u32,
    help: u32,
    hmod: u32,
    stdin_r: u32,
    stdin_w: u32,
    stdout_r: u32,
    stdout_w: u32,
    stderr_w: u32,
    /// Lines typed for the procedure's standard input.
    pipe_in: Queue,
    /// Lines typed (or chosen from the Trace menu) at a trace pause.
    trace_in: Queue,
    /// Output waiting for the MLE.
    out: Mutex,
    out_buf: UnsafeCell<Vec<u8>>,
    out_queued: AtomicBool,
    stop_out: AtomicBool,
    visible: AtomicBool,
    /// The entry field has the focus.
    infocus: AtomicBool,
    /// The trace exit waits for input (lines go to `trace_in`).
    trace_inp: AtomicBool,
    /// 0 normal input, 1 trace input (each keeps its last line for Up).
    in_type: AtomicUsize,
    tracebit: AtomicBool,
    halted: AtomicBool,
    results: AtomicBool,
    pid: u32,
    rexx_tid: AtomicU32,
    in_hgt: i32,
    /// Where the entry field starts (after "Input:").
    wtitle: AtomicU32,
    codepage: u16,
    /// FONTDLG (its FATTRS at 96 is the MLE's font) and its family name.
    fontdlg: [u8; 168],
    family: [u8; MAXWIDTH],
    last: [Option<Vec<u8>>; 2],
    help_table: Vec<u8>,
}

fn fattrs(io: &mut Io) -> &mut [u8] {
    &mut io.fontdlg[96..152]
}

fn data(client: u32) -> Option<&'static mut Io> {
    let p = unsafe { WinQueryWindowPtr(client, 0) } as *mut Io;
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

/// The window data the I/O exit registration carries (its user area).
fn exit_data() -> Option<&'static mut Io> {
    let mut user = [0u32; 2];
    let mut flag = 0u16;
    if unsafe { RexxQueryExit(EXIT_NAME.as_ptr(), core::ptr::null(), &mut flag, user.as_mut_ptr() as *mut u8) } != 0 {
        return None;
    }
    let p = user[0] as *mut Io;
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

// ---- Exports ----

/// RexxCreateIOWindow(owner, &client): the I/O frame window (hidden), or
/// 0.
extern "C" fn create_io_window(owner: u32, client: *mut u32) -> u32 {
    let hab = unsafe { WinQueryAnchorBlock(owner) };
    if hab == 0 {
        return 0;
    }
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(CLASS.as_ptr(), &mut hmod) } != 0 {
        return 0;
    }
    if unsafe { WinRegisterClass(hab, CLASS.as_ptr(), proc_ptr(a_client_proc), CS_SIZEREDRAW, 4) } == 0 {
        return 0;
    }
    // IBM's: title bar, system menu, menu, sizing border, minimize and
    // maximize, icon, shell position, task list; rs2's modules have no
    // resources, so the menu is made here and there is no icon.
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINBUTTON | FCF_MAXBUTTON | FCF_SHELLPOSITION | FCF_TASKLIST;
    let mut hclient = 0;
    let frame = unsafe { WinCreateStdWindow(owner, 0, &mut fcf, CLASS.as_ptr(), core::ptr::null(), WS_VISIBLE, hmod, RXHOST_WIN as u32, &mut hclient) };
    if frame == 0 {
        return 0;
    }
    let menu = tpl::action_bar();
    unsafe { WinCreateMenu(frame, menu.as_ptr()) };
    send(frame, WM_UPDATEFRAME, FCF_MENU, 0);
    unsafe { WinSetWindowText(WinWindowFromID(frame, FID_TITLEBAR), string(1).as_ptr()) };
    if !client.is_null() {
        unsafe { *client = hclient };
    }
    frame
}

/// RexxDestroyIOWindow(frame): end the I/O window and its exit.
extern "C" fn destroy_io_window(frame: u32) {
    let client = unsafe { WinWindowFromID(frame, FID_CLIENT) };
    let Some(io) = data(client) else { return };
    if io.help != 0 {
        unsafe {
            WinDestroyHelpInstance(io.help);
            WinAssociateHelpInstance(0, io.frame);
        }
    }
    io.pipe_in.close();
    io.trace_in.close();
    unsafe {
        RexxDeregisterExit(EXIT_NAME.as_ptr(), core::ptr::null());
        WinSetWindowPtr(client, 0, 0);
        WinDestroyWindow(io.frame);
    }
    // The pipe threads still reference the data: it is left allocated, as
    // IBM's leaves its threads (the process is about to end).
    io.stop_out.store(true, SeqCst);
}

// ---- Pipes ----

/// A pipe whose read end is handle `rd` and write end `wr`.
fn make_pipe(rd: u32, wr: u32) -> (u32, u32) {
    let (mut r, mut w) = (0, 0);
    unsafe { os2::DosCreatePipe(&mut r, &mut w, PIPE_SIZE) };
    if r != rd {
        if w == rd {
            let mut t = if r != wr { wr } else { 0xffff_ffff };
            unsafe { os2::DosDupHandle(w, &mut t) };
            w = t;
        }
        let mut t = rd;
        unsafe {
            os2::DosDupHandle(r, &mut t);
            os2::DosClose(r);
        }
        r = rd;
    }
    if w != wr {
        let mut t = wr;
        unsafe {
            os2::DosDupHandle(w, &mut t);
            os2::DosClose(w);
        }
        w = wr;
    }
    (r, w)
}

fn set_inherit(h: u32, flag: bool) {
    let mut st = 0;
    unsafe { os2::DosQueryFHState(h, &mut st) };
    let st = if flag { st | NOINHERIT } else { st & !NOINHERIT };
    unsafe { os2::DosSetFHState(h, st) };
}

fn stdin_pipe(io: &mut Io) {
    (io.stdin_r, io.stdin_w) = make_pipe(0, 4);
    set_inherit(io.stdin_r, true);
    set_inherit(io.stdin_w, false);
}

/// Pipes for handles 0, 1 and 2, the I/O exit and the pipe threads.
fn initialize(io: &mut Io) -> u32 {
    unsafe {
        os2::DosClose(0);
        os2::DosClose(1);
        os2::DosClose(2);
    }
    stdin_pipe(io);
    (io.stdout_r, io.stdout_w) = make_pipe(5, 1);
    set_inherit(io.stdout_w, true);
    set_inherit(io.stdout_r, false);
    io.stderr_w = 2;
    let rc = unsafe { os2::DosDupHandle(io.stdout_w, &mut io.stderr_w) };
    if rc != 0 {
        return rc;
    }
    let user = [io as *mut Io as u32, 0];
    unsafe { RexxRegisterExitExe(EXIT_NAME.as_ptr(), a_io_exit as *const () as usize, user.as_ptr() as *const u8) };
    let mut tid = 0;
    let p = io as *mut Io as u32;
    let rc = unsafe { os2::DosCreateThread(&mut tid, thread_ptr(a_stdin_thread), p, 0, STACK) };
    if rc != 0 {
        return rc;
    }
    unsafe { os2::DosCreateThread(&mut tid, thread_ptr(a_stdout_thread), p, 0, STACK) }
}

/// Writes the lines typed to the procedure's standard input.
unsafe extern "C" fn stdin_thread(p: u32) {
    let io = unsafe { &*(p as *const Io) };
    loop {
        let mut line = io.pipe_in.remove();
        line.extend_from_slice(b"\r\n");
        let mut n = 0;
        unsafe { os2::DosWrite(io.stdin_w, line.as_ptr(), line.len() as u32, &mut n) };
    }
}

/// Reads standard output and error (a line at a time, CR LF and lone CRs
/// made LFs) into the output buffer, and has the window add it.
unsafe extern "C" fn stdout_thread(p: u32) {
    let io = unsafe { &*(p as *const Io) };
    let read = |c: &mut u8| -> bool {
        let mut n = 0;
        unsafe { os2::DosRead(io.stdout_r, c, 1, &mut n) == 0 && n != 0 }
    };
    let mut c = 0u8;
    while !io.stop_out.load(SeqCst) {
        let mut line = Vec::with_capacity(MAXWIDTH);
        loop {
            if read(&mut c) {
                if c == b'\r' && read(&mut c) && c != b'\n' {
                    line.push(b'\n');
                }
                line.push(c);
            } else {
                unsafe { os2::DosSleep(10) };
            }
            if line.len() >= MAXWIDTH - 1 || c == b'\n' {
                break;
            }
        }
        if io.stop_out.load(SeqCst) {
            break;
        }
        io.out.lock();
        let buf = unsafe { &mut *io.out_buf.get() };
        if IMPORT_HDR + buf.len() + line.len() + 2 < IMPORT_SIZE {
            buf.extend_from_slice(&line);
            io.out.unlock();
            if !io.out_queued.swap(true, SeqCst) {
                post_hard(io.frame, START_TIMER, 0, 0, TIMER_DELAY);
            }
        } else {
            io.out.unlock();
            add_line(io, line);
            unsafe { os2::DosSleep(TIMER_DELAY) };
        }
    }
}

/// Have the window add a line of output (ADD_STDOUTWIN).
fn add_line(io: &Io, line: Vec<u8>) {
    let p = Box::into_raw(Box::new(line)) as u32;
    post_hard(io.client, ADD_STDOUTWIN, p, 0, TIMER_DELAY);
}

// ---- The REXX I/O exit ----

/// RXSIO: trace input (RXSIODTR) from the entry field or the Trace menu;
/// at a read (RXSIOTRD) the window is shown and REXX reads standard
/// input itself. Everything else is left to REXX.
extern "C" fn io_exit(exitno: i32, sub: i32, parm: *mut RxString) -> i32 {
    let Some(io) = exit_data() else { return RXEXIT_RAISE_ERROR };
    if exitno != RXSIO {
        return RXEXIT_NOT_HANDLED;
    }
    match sub {
        RXSIODTR => {
            for id in [RXIOA_TRCSTEP, RXIOA_TRCLAST, RXIOA_TRCOFF] {
                set_options(io.frame, id, true);
            }
            if !io.tracebit.load(SeqCst) {
                set_check(io.frame, RXIOA_TRACE, true);
            }
            io.trace_inp.store(true, SeqCst);
            io.in_type.store(1, SeqCst);
            let line = io.trace_in.remove();
            io.in_type.store(0, SeqCst);
            io.trace_inp.store(false, SeqCst);
            for id in [RXIOA_TRCSTEP, RXIOA_TRCLAST, RXIOA_TRCOFF] {
                set_options(io.frame, id, false);
            }
            if !io.tracebit.load(SeqCst) {
                set_check(io.frame, RXIOA_TRACE, false);
            }
            let s = unsafe { &mut *parm };
            if !s.ptr.is_null() {
                unsafe { core::ptr::copy_nonoverlapping(line.as_ptr(), s.ptr, line.len()) };
                unsafe { *s.ptr.add(line.len()) = 0 };
            }
            s.len = line.len() as u32;
            RXEXIT_HANDLED
        }
        RXSIOTRD => {
            if !io.visible.load(SeqCst) {
                post_hard(io.client, REXX_MAKE_VISIBLE, 0, 0, TIMER_DELAY);
            }
            RXEXIT_NOT_HANDLED
        }
        _ => RXEXIT_NOT_HANDLED,
    }
}

// ---- Menu items ----

/// Enable or disable a menu item (posted: the exit runs on REXX's thread).
fn set_options(frame: u32, id: u16, on: bool) {
    let menu = unsafe { WinWindowFromID(frame, FID_MENU) };
    post_hard(menu, MM_SETITEMATTR, mp(id as u32, 1), mp(MIA_DISABLED, if on { 0 } else { MIA_DISABLED }), 0);
}

fn set_check(frame: u32, id: u16, on: bool) {
    let menu = unsafe { WinWindowFromID(frame, FID_MENU) };
    post_hard(menu, MM_SETITEMATTR, mp(id as u32, 1), mp(MIA_CHECKED, if on { MIA_CHECKED } else { 0 }), 0);
}

// ---- The client window ----

extern "C" fn client_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m == WM_CREATE {
        return create(h);
    }
    let Some(io) = data(h) else { return unsafe { WinDefWindowProc(h, m, mp1, mp2) } };
    match m {
        WM_TIMER => {
            unsafe { WinStopTimer(io.hab, h, TIMER_ID) };
            import(io);
        }
        WM_ERASEBACKGROUND => return 1,
        WM_SIZE => size_window(io),
        WM_PAINT => {
            let hps = unsafe { WinGetPS(h) };
            if hps != 0 {
                let mut wp = Swp::default();
                unsafe { WinQueryWindowPos(h, &mut wp) };
                let text = string(5);
                let mut p = PointL { x: BORDER * 2, y: wp.cy - io.in_hgt - BORDER * 2 };
                unsafe {
                    GpiCharStringAt(hps, &p, text.len() as i32 - 1, text.as_ptr());
                    GpiQueryCurrentPosition(hps, &mut p);
                    WinReleasePS(hps);
                }
                io.wtitle.store(p.x as u32, SeqCst);
            }
        }
        WM_COMMAND => return command(h, mp1 & 0xffff, io),
        WM_CHAR => {
            if mp1 & KC_KEYUP == 0 && mp1 & KC_VIRTUALKEY != 0 {
                match mp2 >> 16 {
                    VK_TAB | VK_BACKTAB => {
                        let to = if io.infocus.load(SeqCst) { io.mle } else { io.entry };
                        return unsafe { WinSetFocus(HWND_DESKTOP, to) };
                    }
                    VK_NEWLINE | VK_ENTER => {
                        send(io.entry, WM_CHAR, KC_VIRTUALKEY, VK_NEWLINE << 16);
                    }
                    VK_DELETE => {
                        send(h, WM_COMMAND, RXIOA_DELETE as u32, 0);
                    }
                    _ => {}
                }
            }
        }
        REXX_STARTPROC => io.rexx_tid.store(mp1, SeqCst),
        REXX_ENDPROC => {
            // The procedure ended: unread input is dropped and standard
            // input gets a new pipe.
            io.pipe_in.clear();
            unsafe {
                os2::DosClose(io.stdin_r);
                os2::DosClose(io.stdin_w);
            }
            stdin_pipe(io);
            unsafe { WinSetFocus(HWND_DESKTOP, io.entry) };
        }
        REXX_MAKE_INVISIBLE => {
            if io.visible.swap(false, SeqCst) {
                unsafe { WinShowWindow(io.frame, 0) };
            }
        }
        REXX_MAKE_VISIBLE => make_visible(h, io),
        RXIOB_MSGBOX => {
            let (text, title) = (string(mp1 & 0xffff), string(mp1 >> 16));
            let r = unsafe { WinMessageBox(HWND_DESKTOP, h, text.as_ptr(), title.as_ptr(), mp1 & 0xffff, mp2 & 0xffff) };
            return r & 0xffff;
        }
        START_TIMER => {
            if !io.visible.load(SeqCst) {
                send(h, REXX_MAKE_VISIBLE, 0, 0);
            }
            unsafe { WinStartTimer(io.hab, h, TIMER_ID, TIMER_DELAY) };
            io.out_queued.store(false, SeqCst);
        }
        ADD_STDOUTWIN => {
            if !io.visible.load(SeqCst) {
                send(h, REXX_MAKE_VISIBLE, 0, 0);
            }
            let line = unsafe { Box::from_raw(mp1 as *mut Vec<u8>) };
            add_output(io, &line);
            unsafe { WinStartTimer(io.hab, h, TIMER_ID, TIMER_DELAY) };
            io.out_queued.store(false, SeqCst);
            return 1;
        }
        HM_QUERY_KEYS_HELP => return RX_KEYSHELP,
        WM_SAVEAPPLICATION => save_window(io),
        _ => {}
    }
    unsafe { WinDefWindowProc(h, m, mp1, mp2) }
}

/// WM_CREATE: the window data, pipes and exit, the MLE and entry field.
fn create(h: u32) -> u32 {
    let hab = unsafe { WinQueryAnchorBlock(h) };
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(CLASS.as_ptr(), &mut hmod) } != 0 {
        return 0;
    }
    let io = Box::leak(Box::new(Io {
        hab,
        frame: unsafe { WinQueryWindow(h, QW_PARENT) },
        client: h,
        mle: 0,
        entry: 0,
        help: 0,
        hmod,
        stdin_r: 0,
        stdin_w: 0,
        stdout_r: 0,
        stdout_w: 0,
        stderr_w: 0,
        pipe_in: Queue::new(),
        trace_in: Queue::new(),
        out: Mutex::new(),
        out_buf: UnsafeCell::new(Vec::with_capacity(IMPORT_SIZE)),
        out_queued: AtomicBool::new(false),
        stop_out: AtomicBool::new(false),
        visible: AtomicBool::new(false),
        infocus: AtomicBool::new(false),
        trace_inp: AtomicBool::new(false),
        in_type: AtomicUsize::new(0),
        tracebit: AtomicBool::new(false),
        halted: AtomicBool::new(false),
        results: AtomicBool::new(true),
        pid: 0,
        rexx_tid: AtomicU32::new(0),
        in_hgt: 0,
        wtitle: AtomicU32::new(0),
        codepage: 0,
        fontdlg: [0; 168],
        family: [0; MAXWIDTH],
        last: [None, None],
        help_table: tpl::help_table(),
    }));
    let mut tib: *mut os2::Tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    io.pid = unsafe { (*pib).pid };
    let cc = os2::CountryCode::default();
    let mut ci = os2::CountryInfo::default();
    let mut n = 0;
    unsafe { os2::DosQueryCtryInfo(core::mem::size_of::<os2::CountryInfo>() as u32, &cc, &mut ci, &mut n) };
    io.codepage = ci.codepage as u16;
    fixed_font(h, io);
    unsafe { WinSetWindowPtr(h, 0, io as *mut Io as usize) };
    if initialize(io) != 0 {
        unsafe {
            WinAlarm(HWND_DESKTOP, WA_ERROR);
            WinMessageBox(HWND_DESKTOP, 0, string(13).as_ptr(), string(6).as_ptr(), 1, MB_CANCEL | MB_CUACRITICAL | MB_MOVEABLE);
        }
    }
    let mut fm = [0u8; 228];
    let hps = unsafe { WinGetPS(h) };
    unsafe {
        GpiQueryFontMetrics(hps, fm.len() as i32, fm.as_mut_ptr());
        WinReleasePS(hps);
    }
    let rd = |o: usize| i32::from_le_bytes([fm[o], fm[o + 1], fm[o + 2], fm[o + 3]]);
    io.in_hgt = rd(112) + 2;
    let ave = rd(100) as u16 as i32;
    io.mle = unsafe { WinCreateWindow(h, WC_MLE, core::ptr::null(), MLS_HSCROLL | MLS_VSCROLL | MLS_BORDER, 0, 0, 0, 0, h, HWND_TOP, MLE_WNDW as u32, core::ptr::null(), core::ptr::null()) };
    if io.mle == 0 {
        error_box(h, 2, MB_OK | MB_CUAWARNING);
    }
    unsafe { WinEnableWindowUpdate(h, 0) };
    send(io.mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
    send(io.mle, MLM_SETREADONLY, 1, 0);
    send(io.mle, MLM_SETFONT, fattrs(io).as_ptr() as u32, 0);
    unsafe { WinShowWindow(io.mle, 1) };
    io.wtitle.store(((string(5).len() as i32 - 1 + 5) * ave) as u32, SeqCst);
    // IBM's loads a dialog whose window is the entry field; this is the
    // same field, subclassed.
    io.entry = unsafe { WinCreateWindow(h, WC_ENTRYFIELD, b"\0".as_ptr(), WS_TABSTOP | ES_MARGIN | ES_AUTOSCROLL, 0, 0, 0, 0, h, HWND_TOP, ID_EF1 as u32, core::ptr::null(), core::ptr::null()) };
    let old = unsafe { WinSubclassWindow(io.entry, proc_ptr(a_entry_proc)) };
    ENTRY_PROC.store(old, SeqCst);
    send(io.entry, EM_SETTEXTLIMIT, ANSLEN, 0);
    send(io.entry, EM_SETFIRSTCHAR, 0, 0);
    send(io.entry, EM_SETSEL, 0, 0);
    unsafe {
        WinShowWindow(io.entry, 1);
        WinEnableWindowUpdate(io.entry, 1);
        WinEnableWindowUpdate(h, 1);
        WinSetFocus(HWND_DESKTOP, io.mle);
    }
    0
}

/// Show the window (at its saved place) and start its help.
fn make_visible(h: u32, io: &mut Io) {
    if io.visible.swap(true, SeqCst) {
        return;
    }
    restore_window(io);
    unsafe { WinShowWindow(io.frame, 1) };
    tpl::fix_help_table(&mut io.help_table);
    let mut hi = HelpInit {
        cb: core::mem::size_of::<HelpInit>() as u32,
        ret: 0,
        tutorial: core::ptr::null(),
        table: io.help_table.as_ptr() as *const u16,
        table_module: io.hmod,
        accel_module: io.hmod,
        accel_id: 0,
        action_bar_id: 0,
        title: string(10).as_ptr(),
        show_panel_id: CMIC_HIDE_PANEL_ID,
        library: string(9).as_ptr(),
    };
    io.help = unsafe { WinCreateHelpInstance(io.hab, &mut hi) };
    if io.help == 0 {
        error_box(h, 12, MB_OK | MB_CUAWARNING);
    } else if unsafe { WinAssociateHelpInstance(io.help, io.frame) } == 0 {
        error_box(h, 11, MB_OK | MB_CUAWARNING);
        unsafe { WinDestroyHelpInstance(io.help) };
        io.help = 0;
    }
    unsafe { WinSetFocus(HWND_DESKTOP, io.entry) };
}

/// The MLE below, the entry field to the right of "Input:" above.
fn size_window(io: &Io) {
    if !io.visible.load(SeqCst) || io.mle == 0 || io.entry == 0 {
        return;
    }
    let mut wp = Swp::default();
    unsafe { WinQueryWindowPos(io.client, &mut wp) };
    let (cx, cy, ih) = (wp.cx as i16 as i32, wp.cy as i16 as i32, io.in_hgt);
    let wt = io.wtitle.load(SeqCst) as i32;
    let fl = SWP_SIZE | SWP_MOVE | SWP_ZORDER | SWP_SHOW;
    unsafe {
        WinSetWindowPos(io.mle, HWND_TOP, BORDER, BORDER - 1, cx - BORDER * 2 + 1, cy - ih - BORDER * 6, fl);
        WinSetWindowPos(io.entry, HWND_TOP, BORDER * 3 + wt, cy - ih - BORDER * 3, cx - BORDER * 5 - wt, ih, fl);
    }
}

/// Add the buffered output at the end of the MLE.
fn import(io: &Io) {
    io.out.lock();
    let buf = unsafe { &mut *io.out_buf.get() };
    let mut text = core::mem::replace(buf, Vec::with_capacity(IMPORT_SIZE));
    io.out.unlock();
    if text.is_empty() {
        return;
    }
    let mle = io.mle;
    let mut ipt = send(mle, MLM_QUERYTEXTLENGTH, 0, 0);
    let n = text.len() as u32;
    text.push(0);
    if send(mle, MLM_SETIMPORTEXPORT, text.as_ptr() as u32, n + 1) == 0 {
        error_box(mle, 4, MB_OK | MB_CUACRITICAL);
        unsafe { WinPostMsg(WinQueryWindow(mle, QW_PARENT), WM_CLOSE, 0, 0) };
        return;
    }
    unsafe { WinEnableWindowUpdate(mle, 0) };
    send(mle, MLM_FORMAT, MLFIE_NOTRANS, 0);
    send(mle, MLM_SETREADONLY, 0, 0);
    if send(mle, MLM_IMPORT, &mut ipt as *mut u32 as u32, n) == 0 {
        error_box(mle, 3, MB_OK | MB_CUACRITICAL);
        send(mle, MLM_SETSEL, ipt, ipt);
        send(mle, MLM_INSERT, b"\r\0".as_ptr() as u32, 0);
        ipt = send(mle, MLM_QUERYTEXTLENGTH, 0, 0);
        if send(mle, MLM_IMPORT, &mut ipt as *mut u32 as u32, n) == 0 {
            unsafe { WinPostMsg(WinQueryWindow(mle, QW_PARENT), WM_CLOSE, 0, 0) };
        }
    }
    send(mle, MLM_SETSEL, ipt, ipt);
    unsafe { WinEnableWindowUpdate(mle, 1) };
    send(mle, MLM_FORMAT, MLFIE_CFTEXT, 0);
    send(mle, MLM_SETREADONLY, 1, 0);
}

/// ADD_STDOUTWIN: a line too long for the buffer goes in pieces, the
/// buffer being added to the MLE when it is full.
fn add_output(io: &Io, line: &[u8]) {
    let room = IMPORT_SIZE - IMPORT_HDR - 1;
    for piece in line.chunks(room) {
        loop {
            io.out.lock();
            let buf = unsafe { &mut *io.out_buf.get() };
            if buf.len() + piece.len() < room {
                buf.extend_from_slice(piece);
                io.out.unlock();
                break;
            }
            io.out.unlock();
            unsafe { WinStopTimer(io.hab, io.client, TIMER_ID) };
            import(io);
        }
    }
}

// ---- Commands ----

fn queue_line(q: &Queue, s: &[u8]) {
    q.add(s.to_vec());
}

fn help_msg(io: &Io, msg: u32) {
    if io.help != 0 {
        send(io.help, msg, 0, 0);
    } else {
        send(io.client, RXIOB_MSGBOX, mp(8, 7), MB_ENTER | MB_CUACRITICAL | MB_MOVEABLE);
    }
}

fn clipboard(io: &Io, msg: u32, err: u32) {
    if send(io.mle, msg, 0, 0) == 0 {
        send(io.client, RXIOB_MSGBOX, mp(err, 16), MB_ENTER | MB_CUACRITICAL | MB_MOVEABLE);
    }
}

fn select(h: u32, io: &Io, end: u32) {
    let mle = unsafe { WinWindowFromID(h, MLE_WNDW as u32) };
    unsafe { WinEnableWindowUpdate(h, 0) };
    send(mle, MLM_SETSEL, 0, end & 0xffff);
    unsafe {
        WinInvalidateRect(h, core::ptr::null(), 1);
        WinEnableWindowUpdate(h, 1);
    }
    send(mle, MLM_RESETUNDO, 0, 0);
    unsafe { WinSetFocus(HWND_DESKTOP, io.mle) };
}

fn command(h: u32, id: u32, io: &mut Io) -> u32 {
    let tid = io.rexx_tid.load(SeqCst);
    match id as u16 {
        RXIOA_RESULTS => {
            let on = !io.results.load(SeqCst);
            io.results.store(on, SeqCst);
            set_check(io.frame, RXIOA_RESULTS, on);
            send(io.client, WM_COMMAND, PMRXA_RESULTCHANGE, 0);
        }
        RXIOA_TRCSTEP => queue_line(&io.trace_in, b""),
        RXIOA_TRCLAST => queue_line(&io.trace_in, b"="),
        RXIOA_TRCOFF => {
            unsafe { RexxResetTrace(io.pid, tid) };
            io.tracebit.store(false, SeqCst);
            queue_line(&io.trace_in, b"trace off");
            set_check(io.frame, RXIOA_TRACE, false);
            send(io.client, WM_COMMAND, PMRXA_FLAGCHANGE, 0);
        }
        RXIOA_TRACE => {
            let on = !io.tracebit.load(SeqCst);
            io.tracebit.store(on, SeqCst);
            unsafe {
                if on {
                    RexxSetTrace(io.pid, tid);
                } else {
                    RexxResetTrace(io.pid, tid);
                }
            }
            set_check(io.frame, RXIOA_TRACE, on);
            send(io.client, WM_COMMAND, PMRXA_FLAGCHANGE, 1);
        }
        RXIOA_HALT => {
            unsafe { RexxSetHalt(io.pid, tid) };
            // An empty line ends a read the procedure may be waiting in.
            if io.trace_inp.load(SeqCst) {
                queue_line(&io.trace_in, b"");
            } else {
                queue_line(&io.pipe_in, b"");
            }
            if !io.halted.swap(true, SeqCst) {
                set_check(io.frame, RXIOA_HALT, true);
            }
        }
        RXIOA_CUT => {
            if io.infocus.load(SeqCst) {
                send(io.entry, EM_CUT, 0, 0);
            } else {
                clipboard(io, MLM_CUT, 18);
            }
        }
        RXIOA_DELETE => {
            if io.infocus.load(SeqCst) {
                send(io.entry, EM_CLEAR, 0, 0);
            } else {
                clipboard(io, MLM_CLEAR, 19);
            }
        }
        RXIOA_PASTE => {
            send(io.entry, EM_PASTE, 0, 0);
        }
        RXIOA_COPY => {
            if io.infocus.load(SeqCst) {
                send(io.entry, EM_COPY, 0, 0);
            } else {
                clipboard(io, MLM_COPY, 21);
            }
        }
        RXIOA_ALL => {
            let mle = unsafe { WinWindowFromID(h, MLE_WNDW as u32) };
            let n = send(mle, MLM_QUERYTEXTLENGTH, 0, 0);
            select(h, io, n);
        }
        RXIOA_DALL => select(h, io, 0),
        RXIOA_FONTS => {
            send(unsafe { WinWindowFromID(h, MLE_WNDW as u32) }, MLM_RESETUNDO, 0, 0);
            select_font(io);
        }
        RXIOA_SEARCH => search(h, io),
        RXIOM_HELPHELP => help_msg(io, HM_DISPLAY_HELP),
        RXIOM_EXTHELP => help_msg(io, HM_EXT_HELP),
        RXIOM_KEYSHELP => help_msg(io, HM_KEYS_HELP),
        RXIOM_INDEXHELP => help_msg(io, HM_HELP_INDEX),
        _ => {
            unsafe { WinAlarm(HWND_DESKTOP, WA_ERROR) };
        }
    }
    0
}

// ---- Find ----

/// SEARCHINFO: size, case sensitive, the text.
#[repr(C)]
struct SearchInfo {
    size: u32,
    case: u32,
    text: [u8; 100],
}

extern "C" fn search_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_CLOSE => {
            send(h, WM_COMMAND, PB_CANCEL as u32, mp(1, 1));
            return 0;
        }
        WM_INITDLG => {
            SEARCH.store(mp2 as usize, SeqCst);
            return 0;
        }
        WM_COMMAND => match (mp1 & 0xffff) as u16 {
            PB_OK => {
                let si = SEARCH.load(SeqCst) as *mut SearchInfo;
                if !si.is_null() {
                    let si = unsafe { &mut *si };
                    unsafe { WinQueryWindowText(WinWindowFromID(h, RX_SEARCH_FIELD as u32), 100, si.text.as_mut_ptr()) };
                    si.case = unsafe { WinSendDlgItemMsg(h, RX_CASEBOX as u32, BM_QUERYCHECK, 0, 0) != 0 } as u32;
                }
                unsafe { WinDismissDlg(h, PB_OK as u32) };
                return 0;
            }
            PB_CANCEL => {
                unsafe { WinDismissDlg(h, PB_CANCEL as u32) };
            }
            _ => {}
        },
        _ => {}
    }
    unsafe { WinDefDlgProc(h, m, mp1, mp2) }
}

static SEARCH: AtomicUsize = AtomicUsize::new(0);

fn search(h: u32, io: &Io) {
    let mut si = SearchInfo { size: core::mem::size_of::<SearchInfo>() as u32, case: 0, text: [0; 100] };
    let t = tpl::search_dialog();
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, h, proc_ptr(a_search_proc), t.as_ptr(), &mut si as *mut SearchInfo as usize) };
    if dlg == 0 {
        return;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    SEARCH.store(0, SeqCst);
    if r != PB_OK as u32 {
        return;
    }
    let mut sd = MleSearch { cb: 0x1c, find: si.text.as_ptr(), replace: core::ptr::null(), cch_find: 0, cch_replace: 0, start: -1, stop: -1, cch_found: 0 };
    let fl = MLFSEARCH_SELECTMATCH | if si.case != 0 { MLFSEARCH_CASESENSITIVE } else { 0 };
    send(unsafe { WinWindowFromID(h, MLE_WNDW as u32) }, MLM_SEARCH, fl, &mut sd as *mut MleSearch as u32);
    if sd.cch_found == 0 {
        unsafe { WinMessageBox(HWND_DESKTOP, 0, string(9003).as_ptr(), string(9002).as_ptr(), 1, MB_OK | MB_INFORMATION | MB_MOVEABLE) };
    } else if io.infocus.swap(false, SeqCst) {
        unsafe { WinSetFocus(HWND_DESKTOP, io.mle) };
    }
}

// ---- The entry field ----

static ENTRY_PROC: AtomicUsize = AtomicUsize::new(0);

fn entry_default(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let p = ENTRY_PROC.load(SeqCst);
    if p == 0 {
        return unsafe { WinDefWindowProc(h, m, mp1, mp2) };
    }
    let f: WndProc = unsafe { core::mem::transmute(p) };
    f(h, m, mp1, mp2)
}

fn set_entry_text(h: u32, text: &[u8]) {
    let mut t = text.to_vec();
    t.push(0);
    unsafe { WinSetWindowText(h, t.as_ptr()) };
}

extern "C" fn entry_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let client = unsafe { WinQueryWindow(h, QW_PARENT) };
    let Some(io) = data(client) else { return entry_default(h, m, mp1, mp2) };
    match m {
        WM_SETFOCUS => {
            io.infocus.store(mp2 != 0, SeqCst);
        }
        WM_BUTTON1DBLCLK => {
            // The first line of the MLE's selection becomes the input.
            let start = send(io.mle, MLM_QUERYSEL, 1, 0);
            let end = send(io.mle, MLM_QUERYSEL, 2, 0);
            if (start as i32) < end as i32 {
                let mut len = end - start;
                let mut buf = alloc::vec![0u8; len as usize + 2];
                let mut from = start;
                if send(io.mle, MLM_SETIMPORTEXPORT, buf.as_mut_ptr() as u32, len + 1) != 0 {
                    let got = send(io.mle, MLM_EXPORT, &mut from as *mut u32 as u32, &mut len as *mut u32 as u32) as usize;
                    if got != 0 {
                        let got = got.min(buf.len());
                        let s = buf[..got].iter().position(|&c| c != 0).unwrap_or(got);
                        let e = buf[s..got].iter().position(|&c| c == 0 || c == b'\n' || c == b'\r').map_or(got, |i| s + i);
                        set_entry_text(h, &buf[s..e]);
                    }
                }
            }
        }
        WM_BUTTON2DBLCLK => {
            send(h, WM_CHAR, KC_VIRTUALKEY, VK_NEWLINE << 16);
        }
        WM_CHAR if mp1 & KC_KEYUP == 0 && mp1 & KC_VIRTUALKEY != 0 => match mp2 >> 16 {
            VK_BREAK => {
                send(io.client, WM_COMMAND, RXIOA_HALT as u32, 0);
            }
            VK_NEWLINE | VK_ENTER => {
                let mut buf = [0u8; ANSLEN as usize + 1];
                let n = unsafe { WinQueryWindowText(h, ANSLEN as i32, buf.as_mut_ptr()) }.max(0) as usize;
                let line = buf[..n.min(ANSLEN as usize)].to_vec();
                let mut echo = line.clone();
                echo.push(b'\n');
                let t = io.in_type.load(SeqCst).min(1);
                io.last[t] = Some(echo.clone());
                add_line(io, echo);
                if io.trace_inp.load(SeqCst) {
                    io.trace_in.add(line);
                } else {
                    io.pipe_in.add(line);
                }
                send(h, EM_SETSEL, mp(0, n as u32), 0);
                send(h, EM_CLEAR, 0, 0);
                return 1;
            }
            VK_UP => {
                let t = io.in_type.load(SeqCst).min(1);
                if let Some(last) = io.last[t].clone() {
                    send(h, EM_SETSEL, mp(0, 0xffff), 0);
                    send(h, EM_CLEAR, 0, 0);
                    set_entry_text(h, &last[..last.len() - 1]);
                    return 1;
                }
            }
            VK_TAB | VK_BACKTAB => {
                let to = if io.infocus.load(SeqCst) { io.mle } else { io.entry };
                return unsafe { WinSetFocus(HWND_DESKTOP, to) };
            }
            _ => {}
        },
        _ => {}
    }
    entry_default(h, m, mp1, mp2)
}

// ---- Fonts and the saved window ----

/// The MLE's font: the one saved in OS2.INI, else the 8-point Courier
/// bitmap font for the display's resolution.
fn fixed_font(h: u32, io: &mut Io) {
    let hps = unsafe { WinGetPS(h) };
    let hdc = unsafe { GpiQueryDevice(hps) };
    let (mut xres, mut yres) = (0i32, 0i32);
    unsafe {
        DevQueryCaps(hdc, CAPS_HORIZONTAL_FONT_RES, 1, &mut xres);
        DevQueryCaps(hdc, CAPS_VERTICAL_FONT_RES, 1, &mut yres);
    }
    let mut cb = 56u32;
    let f = fattrs(io);
    let ok = unsafe { PrfQueryProfileData(HINI_USERPROFILE, PROFILE_APP.as_ptr(), b"UserFont\0".as_ptr(), f.as_mut_ptr(), &mut cb) } != 0;
    if ok && cb == 56 {
        unsafe { GpiCreateLogFont(hps, core::ptr::null(), 1, f.as_ptr()) };
    } else {
        f.fill(0);
        let face = b"Courier\0";
        let mut n = 0i32;
        let total = unsafe { GpiQueryFonts(hps, QF_PUBLIC, face.as_ptr(), &mut n, 0, core::ptr::null_mut()) };
        if total > 0 && total < 0x10000 / 228 {
            let mut fms = alloc::vec![0u8; total as usize * 228];
            let mut n = total;
            unsafe { GpiQueryFonts(hps, QF_PUBLIC, face.as_ptr(), &mut n, 228, fms.as_mut_ptr()) };
            let mut lmatch = 0i32;
            for fm in fms.chunks(228).take(n.max(0) as usize) {
                let r16 = |o: usize| i16::from_le_bytes([fm[o], fm[o + 1]]) as i32;
                if r16(126) == xres as i16 as i32 && r16(128) == yres as i16 as i32 && fm[146] & 1 == 0 && r16(138) == 80 {
                    lmatch = i32::from_le_bytes([fm[204], fm[205], fm[206], fm[207]]);
                }
            }
            if lmatch != 0 {
                f[0..2].copy_from_slice(&56u16.to_le_bytes());
                f[2..4].copy_from_slice(&0x20u16.to_le_bytes());
                f[4..8].copy_from_slice(&lmatch.to_le_bytes());
                f[8..8 + face.len()].copy_from_slice(face);
                unsafe { GpiCreateLogFont(hps, core::ptr::null(), 1, f.as_ptr()) };
            }
        }
    }
    unsafe { WinReleasePS(hps) };
}

/// The font dialog for the MLE.
fn select_font(io: &mut Io) {
    let wr = |fd: &mut [u8; 168], o: usize, v: u32| fd[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let rd = |fd: &[u8; 168], o: usize| u32::from_le_bytes([fd[o], fd[o + 1], fd[o + 2], fd[o + 3]]);
    let mut fd = io.fontdlg;
    wr(&mut fd, 0, 168);
    if rd(&fd, 28) == 0 {
        let face = &fd[104..136];
        let n = face.iter().position(|&c| c == 0).unwrap_or(32);
        io.family[..n].copy_from_slice(&face[..n]);
        io.family[n] = 0;
        wr(&mut fd, 32, 8 << 16);
    }
    wr(&mut fd, 28, io.family.as_mut_ptr() as u32);
    wr(&mut fd, 16, string(23).as_ptr() as u32);
    wr(&mut fd, 12, string(274).as_ptr() as u32);
    wr(&mut fd, 36, FNTS_CENTER | FNTS_HELPBUTTON);
    fd[154..156].copy_from_slice(&5u16.to_le_bytes());
    fd[156..158].copy_from_slice(&5u16.to_le_bytes());
    let hps = unsafe { WinGetPS(io.mle) };
    wr(&mut fd, 4, hps);
    fd[138..140].copy_from_slice(&io.codepage.to_le_bytes());
    wr(&mut fd, 60, (-1i32) as u32);
    wr(&mut fd, 64, (-2i32) as u32);
    fd[164..166].copy_from_slice(&(MAXWIDTH as u16).to_le_bytes());
    wr(&mut fd, 20, 0);
    unsafe { WinEnableWindowUpdate(io.mle, 0) };
    let mut ok = unsafe { WinFontDlg(HWND_DESKTOP, io.frame, fd.as_mut_ptr()) } != 0 && rd(&fd, 72) == DID_OK;
    if ok && u16::from_le_bytes([fd[150], fd[151]]) == FATTR_FONTUSE_OUTLINE {
        let hdc = unsafe { GpiQueryDevice(hps) };
        let (mut xres, mut yres) = (0i32, 0i32);
        unsafe {
            DevQueryCaps(hdc, CAPS_HORIZONTAL_FONT_RES, 1, &mut xres);
            DevQueryCaps(hdc, CAPS_VERTICAL_FONT_RES, 1, &mut yres);
        }
        let pts = rd(&fd, 32) as i32 / 72;
        wr(&mut fd, 140, ((pts.wrapping_mul(yres)) as u32) >> 16);
        wr(&mut fd, 144, ((pts.wrapping_mul(xres)) as u32) >> 16);
    }
    unsafe { WinReleasePS(hps) };
    if rd(&fd, 72) != DID_OK {
        ok = false;
    }
    io.fontdlg = fd;
    if ok {
        send(io.mle, MLM_SETFONT, fattrs(io).as_ptr() as u32, 0);
    }
    unsafe { WinEnableWindowUpdate(io.mle, 1) };
}

/// The window's place from OS2.INI (read once: the entry is removed),
/// then the controls laid out.
fn restore_window(io: &Io) {
    let mut st = [0u8; 20];
    let mut cb = 20u32;
    let ok = unsafe { PrfQueryProfileData(HINI_USERPROFILE, PROFILE_APP.as_ptr(), b"WindowState\0".as_ptr(), st.as_mut_ptr(), &mut cb) } != 0;
    let fl = u32::from_le_bytes([st[0], st[1], st[2], st[3]]);
    if ok && cb == 20 {
        let s = |o: usize| i16::from_le_bytes([st[o], st[o + 1]]);
        unsafe {
            PrfWriteProfileData(HINI_USERPROFILE, PROFILE_APP.as_ptr(), b"WindowState\0".as_ptr(), core::ptr::null(), 0);
            WinSetWindowPos(io.frame, HWND_TOP, s(10) as i32, s(8) as i32, s(6) as i32, s(4) as i32, fl);
        }
        if fl & (SWP_MINIMIZE | SWP_MAXIMIZE) != 0 {
            for (i, o) in [(QWS_CYRESTORE, 12), (QWS_CXRESTORE, 14), (QWS_YRESTORE, 16), (QWS_XRESTORE, 18)] {
                unsafe { WinSetWindowUShort(io.frame, i, s(o) as u16) };
            }
        }
    }
    if !ok || cb != 20 || fl & SWP_MINIMIZE == 0 {
        size_window(io);
    }
}

/// WM_SAVEAPPLICATION: the font and the window's place into OS2.INI.
fn save_window(io: &mut Io) {
    if !io.visible.load(SeqCst) {
        return;
    }
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, PROFILE_APP.as_ptr(), b"UserFont\0".as_ptr(), fattrs(io).as_ptr(), 56) };
    let mut swp = Swp::default();
    unsafe { WinQueryWindowPos(io.frame, &mut swp) };
    let mut st = Vec::with_capacity(20);
    st.extend_from_slice(&swp.fl.to_le_bytes());
    for v in [swp.cy, swp.cx, swp.y, swp.x] {
        st.extend_from_slice(&(v as i16).to_le_bytes());
    }
    for i in [QWS_CYRESTORE, QWS_CXRESTORE, QWS_YRESTORE, QWS_XRESTORE] {
        st.extend_from_slice(&unsafe { WinQueryWindowUShort(io.frame, i) }.to_le_bytes());
    }
    unsafe { PrfWriteProfileData(HINI_USERPROFILE, PROFILE_APP.as_ptr(), b"WindowState\0".as_ptr(), st.as_ptr(), 20) };
}
