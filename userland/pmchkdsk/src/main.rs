//! PMCHKDSK.EXE: rs2's Check Disk window, a re-implementation of OS/2's
//! (the program the drives' "Check disk" menu item starts).
//!
//! As OS/2's (studied in its binary and its resource DLL, and by running
//! it on rs2): `PMCHKDSK X:` shows "Check Disk - X:" with "Write
//! corrections to disk", Check, Cancel and Help. Check runs `CHKDSK.COM
//! X: [/F] /P` with its standard input and output on pipes; CHKDSK's /P
//! sends each result or question as a `MSG_<number>,<params>` line (the
//! numbers of OSO001.MSG). Questions (lost clusters, lost extended
//! attributes, a damaged directory) are asked in a message box and
//! answered with a line starting Y or N; errors are shown as they come.
//! When CHKDSK ends, "Check Disk - Results - X:" shows the file system,
//! the total and available space and a pie chart of the disk's use
//! (directories, user files, bad blocks, extended attributes, reserved
//! space; free space is the slice left out) with a legend in the slices'
//! colours, naming only the parts there are. Dialog and help ids are
//! OS/2's; the help is PMCHKDSK.HLP.
//!
//! `PMCHKDSK /SELFTEST` checks the parsing of CHKDSK's lines and the
//! results (PASS/FAIL lines on the serial port).

#![no_std]
#![no_main]

extern crate alloc;

mod child;

use alloc::vec::Vec;

use pmkit::gpi;
use pmkit::help::{Help, HelpWindow};
use pmkit::*;

os2::entry!(main);

// Dialog and control ids (OS/2's).
const DLG_CHECK: u16 = 100;
const DLG_RESULTS: u16 = 900;
const ID_WRITE: u32 = 5001;
const ID_CHECK: u32 = 222;
const ID_CANCEL: u32 = 333;
const ID_HELP: u32 = 444;
const ID_FSTYPE: u32 = 5002;
const ID_TOTAL: u32 = 5003;
const ID_AVAIL: u32 = 5004;
const ID_USAGE: u32 = 938;
const ID_PIE: u32 = 939;
const ID_LEG_DIRS: u32 = 5005;
const ID_RCANCEL: u32 = 4000;
const ID_RHELP: u32 = 5000;

const WM_LINE: u32 = WM_USER + 1;
const WM_ENDED: u32 = WM_USER + 2;

const HELP_FILE: &[u8] = b"PMCHKDSK.HLP";
const TITLE: &[u8] = b"Check Disk";

/// What CHKDSK reported (bytes).
#[derive(Default, Clone)]
pub struct Results {
    pub fs: Vec<u8>,
    pub total: u64,
    pub free: u64,
    /// Directories, user files, bad blocks, extended attributes,
    /// reserved.
    pub parts: [u64; 5],
}

/// The slices' colours (RGB) and the legend's names.
const PART_COLORS: [i32; 5] = [0xe0c000, 0x2040d0, 0xd02020, 0x20a040, 0xa040a0];

struct App {
    hab: u32,
    drive: u8,
    dlg: u32,
    help: Option<&'static mut Help>,
    child: Option<child::Child>,
    results: Results,
    /// An error was shown (no results then).
    failed: bool,
    /// Corrections CHKDSK reported, shown together at the end.
    notes: Vec<Vec<u8>>,
}

static mut APP: App = App { hab: 0, drive: b'C', dlg: 0, help: None, child: None, results: Results { fs: Vec::new(), total: 0, free: 0, parts: [0; 5] }, failed: false, notes: Vec::new() };

fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

fn trace(s: &[u8]) {
    pmkit::trace("pmchkdsk", s);
}

unsafe extern "C" {
    #[link_name = "PMSHAPI.120"]
    fn WinAddSwitchEntry(ctl: *const SwCntrl) -> u32;
}

fn main(s: &os2::Startup) -> u32 {
    let arg = pmkit::arg(s);
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    if arg.eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    a.drive = match arg.first() {
        Some(&c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase(),
        _ => {
            let (mut d, mut map) = (0u32, 0u32);
            unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
            b'@' + d as u8
        }
    };
    unsafe { WinRegisterClass(a.hab, b"PieChart\0".as_ptr(), pie_proc, CS_SIZEREDRAW, 0) };
    let title = [TITLE, b" - ", &[a.drive, b':']].concat();
    let mut frame = dialog_frame(&title, DLG_CHECK, 159, 48);
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON.
    frame.ctl = 0x13u32.to_le_bytes().to_vec();
    frame.x = 30;
    frame.y = 134;
    let tpl = template(
        frame,
        alloc::vec![
            check_box(b"~Write corrections to disk", (13, 28, 140, 10), ID_WRITE).group(),
            button(b"~Check", (9, 6, 38), ID_CHECK, BS_DEFAULT).group(),
            button(b"Cancel", (59, 6, 38), ID_CANCEL, 0),
            help_button((109, 6, 38), ID_HELP),
        ],
    );
    run_dialog(0, check_proc, &tpl, 0);
    if let Some(h) = a.help.as_mut() {
        h.destroy(a.dlg);
    }
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

extern "C" fn check_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            a.dlg = h;
            a.help = Help::create(
                a.hab,
                h,
                HELP_FILE,
                b"Check Disk Help",
                &[
                    HelpWindow { id: DLG_CHECK, ext: 8061, items: &[(ID_WRITE as u16, 8061), (ID_CHECK as u16, 8061), (ID_CANCEL as u16, 8061)] },
                    HelpWindow { id: DLG_RESULTS, ext: 8068, items: &[(ID_RCANCEL as u16, 8068)] },
                ],
            );
            add_switch_entry(h);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_CHECK)) };
            1
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_CHECK => start_check(h),
                ID_CANCEL | DID_CANCEL => {
                    if let Some(c) = a.child.as_ref() {
                        c.kill();
                    }
                    unsafe { WinDismissDlg(h, ID_CANCEL) };
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            if let Some(c) = a.child.as_ref() {
                c.kill();
            }
            unsafe { WinDismissDlg(h, ID_CANCEL) };
            0
        }
        WM_LINE => {
            let rest = child::take_line(mp2);
            on_message(h, mp1, &rest);
            0
        }
        WM_ENDED => {
            on_end(h, mp1);
            0
        }
        WM_CONTROLPOINTER if a.child.is_some() => unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_WAIT, 0) },
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn add_switch_entry(h: u32) {
    let mut pid = 0;
    let mut tid = 0;
    unsafe { WinQueryWindowProcess(h, &mut pid, &mut tid) };
    let mut sw = SwCntrl { hwnd: h, icon: 0, program: 0, pid, session: 0, visibility: 0, jump: 2, title: [0; 60], prog_type: 0 };
    let t = window_text(h);
    let n = t.len().min(59);
    sw.title[..n].copy_from_slice(&t[..n]);
    unsafe { WinAddSwitchEntry(&sw) };
}

fn busy(h: u32, on: bool) {
    enable(h, ID_CHECK, !on);
    enable(h, ID_WRITE, !on);
    let p = unsafe { WinQuerySysPointer(HWND_DESKTOP, if on { SPTR_WAIT } else { SPTR_ARROW }, 0) };
    unsafe { WinSetPointer(HWND_DESKTOP, p) };
}

fn start_check(h: u32) {
    let a = app();
    if a.child.is_some() {
        return;
    }
    a.results = Results::default();
    a.failed = false;
    a.notes.clear();
    let Some(prog) = child::find_program(&[b"CHKDSK.COM", b"CHKDSK.EXE"]) else {
        message(h, b"Check Disk cannot find the program CHKDSK.COM, which checks the disk.", TITLE, MB_OK | MB_ERROR);
        return;
    };
    let mut args = alloc::vec![a.drive, b':'];
    if checked(h, ID_WRITE) {
        args.extend_from_slice(b" /F");
    }
    args.extend_from_slice(b" /P");
    trace(&[&prog[..], b" ", &args].concat());
    match child::start(&prog, &args, h, WM_LINE, WM_ENDED) {
        Ok(c) => {
            a.child = Some(c);
            busy(h, true);
        }
        Err(rc) => {
            let t = alloc::format!("Check Disk cannot start CHKDSK (error {rc}).");
            message(h, t.as_bytes(), TITLE, MB_OK | MB_ERROR);
        }
    }
}

fn on_end(h: u32, code: u32) {
    let a = app();
    if let Some(mut c) = a.child.take() {
        c.close();
    }
    busy(h, false);
    trace(alloc::format!("chkdsk ended ({code}), {} total, {} free", a.results.total, a.results.free).as_bytes());
    if !a.notes.is_empty() {
        let mut t = Vec::new();
        for n in &a.notes {
            t.extend_from_slice(n);
            t.extend_from_slice(b"\n");
        }
        message(h, &t, TITLE, MB_OK | MB_INFORMATION);
    }
    if a.failed {
        return;
    }
    if a.results.total == 0 {
        let t = alloc::format!("Check Disk could not check drive {}: (CHKDSK ended with code {code}).", a.drive as char);
        message(h, t.as_bytes(), TITLE, MB_OK | MB_ERROR);
        return;
    }
    show_results(h);
}

/// The value of a parameter (digits; separators are skipped).
fn value(p: &[u8]) -> u64 {
    let mut v: u64 = 0;
    for &c in p {
        if c.is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((c - b'0') as u64);
        }
    }
    v
}

/// Fill the results from one of CHKDSK's messages; returns true when it
/// was a result.
pub fn take_result(r: &mut Results, n: u32, p: &[Vec<u8>]) -> bool {
    let v0 = p.first().map(|x| value(x)).unwrap_or(0);
    match n {
        1507 => r.fs = p.first().cloned().unwrap_or_default(),
        // In bytes (FAT).
        1361 => r.total = v0,
        1362 => r.parts[2] = v0,
        1363 => r.parts[4] = v0,
        1364 => r.parts[0] = v0,
        1819 => r.parts[3] = v0,
        1365 => r.parts[1] = v0,
        1368 => r.free = v0,
        // In kilobytes (HPFS, JFS).
        568 => r.total = v0 * 1024,
        569 => r.parts[0] = v0 * 1024,
        570 => r.parts[1] = v0 * 1024,
        633 => r.parts[3] = v0 * 1024,
        632 => r.parts[4] = v0 * 1024,
        571 => r.free = v0 * 1024,
        _ => return false,
    }
    true
}

/// rs2's texts for the messages CHKDSK can send (%1 is the first
/// parameter), and whether each is a question (answered Y or N), an
/// error or a note about something corrected.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Kind {
    Question,
    Error,
    Note,
}

pub fn text_of(n: u32) -> Option<(Kind, &'static str)> {
    use Kind::*;
    Some(match n {
        26 => (Error, "The disk cannot be read. Check that it is in the drive and try again."),
        108 => (Error, "The disk is being used or is locked by another program. Close the programs using it and try again, or check without writing corrections."),
        544 => (Error, "CHKDSK cannot reach drive %1."),
        545 => (Error, "Error %1 kept CHKDSK from checking the disk."),
        546 | 548 => (Error, "There is not enough memory for CHKDSK. Close some programs and try again."),
        547 => (Error, "CHKDSK cannot write the data it recovered to the disk."),
        549 => (Error, "There is not enough free space on the disk for CHKDSK."),
        550 => (Note, "No data was recovered."),
        551 => (Note, "The root directory could not be found; CHKDSK is rebuilding the file system."),
        552 => (Error, "The disk has file system errors. Check it again with \"Write corrections to disk\" selected."),
        553 => (Note, "A small file system error was corrected."),
        554 => (Note, "An error in a directory's organization was corrected."),
        555 => (Note, "Data was moved from a bad block to another place on the disk."),
        556 => (Note, "A damaged directory block was removed."),
        557 => (Note, "The damaged file %1 was removed."),
        558 => (Note, "An error in the allocation of disk space was corrected."),
        560 => (Note, "An allocation error in the file %1 was corrected."),
        562 => (Note, "Lost data was found on the disk."),
        565 => (Error, "Important data on the disk cannot be read. Back up your files now."),
        566 => (Error, "Another program is using the disk."),
        567 => (Note, "Data that had been moved for a while was recovered."),
        572 => (Note, "Recovered files are put in %1."),
        573 => (Note, "A lost directory was recovered as %1."),
        574 => (Note, "Recovered data was put in the file %1."),
        575..=588 | 590..=592 => (Note, "A problem with extended attributes, access lists or replacement sectors was found and dealt with."),
        589 => (Error, "The file system's checking module is not the right version."),
        1210 => (Error, "This version of the operating system cannot run CHKDSK."),
        1314 | 1353 | 1821 => (Note, "Lost data was found on the disk."),
        1315 => (Question, "Lost extended attribute data was found. Turn it into files?"),
        1341 => (Error, "The current directory cannot be read."),
        1346 => (Question, "A directory has errors. Turn it into a file?"),
        1356 => (Question, "Lost clusters were found. Turn them into files?"),
        1820 => (Question, "Extended attribute chains have errors. Turn them into files?"),
        _ => return None,
    })
}

fn fill(t: &str, p: &[Vec<u8>]) -> Vec<u8> {
    let first = p.first().cloned().unwrap_or_default();
    let mut out = Vec::new();
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && b.get(i + 1) == Some(&b'1') {
            out.extend_from_slice(&first);
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn on_message(h: u32, n: u32, rest: &[u8]) {
    let a = app();
    let p = child::params(rest);
    if take_result(&mut a.results, n, &p) {
        return;
    }
    trace(alloc::format!("MSG_{n}").as_bytes());
    let Some((kind, t)) = text_of(n) else { return };
    let text = fill(t, &p);
    match kind {
        Kind::Question => {
            let r = message(h, &text, b"Warning!", MB_YESNO | MB_WARNING);
            if let Some(c) = a.child.as_ref() {
                c.answer(if r == MBID_YES { b"Y" } else { b"N" });
            }
        }
        Kind::Error => {
            a.failed = true;
            message(h, &text, TITLE, MB_OK | MB_ERROR);
        }
        Kind::Note => {
            if !a.notes.contains(&text) {
                a.notes.push(text);
            }
        }
    }
}

// ---- Results ----

extern "C" fn results_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            let r = &a.results;
            set_item_text(h, ID_FSTYPE, &r.fs);
            set_item_text(h, ID_TOTAL, &num(r.total as i64));
            set_item_text(h, ID_AVAIL, &num(r.free as i64));
            for (i, &c) in PART_COLORS.iter().enumerate() {
                let w = pmkit::item(h, ID_LEG_DIRS + i as u32);
                let rgb = c.to_le_bytes();
                unsafe {
                    WinSetPresParam(w, PP_FOREGROUNDCOLOR, 4, rgb.as_ptr());
                    WinShowWindow(w, (r.parts[i] > 0) as u32);
                }
            }
            1
        }
        WM_COMMAND => {
            unsafe { WinDismissDlg(h, lo(mp1)) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn show_results(owner: u32) {
    let a = app();
    let title = [b"Check Disk - Results - ", &[a.drive, b':'][..]].concat();
    let mut frame = dialog_frame(&title, DLG_RESULTS, 211, 174);
    frame.x = 81;
    frame.y = 20;
    let names: [&[u8]; 5] = [b"Directories", b"User files", b"Bad blocks", b"EAs", b"Reserved"];
    let mut v = alloc::vec![
        label(b"Type of file system:", (7, 157, 100, 8), DT_LEFT),
        label(b"Total disk space:", (7, 145, 79, 8), DT_LEFT),
        label(b"Space available for use:", (7, 133, 106, 8), DT_LEFT),
        text_id(b"", (117, 157, 73, 8), ID_FSTYPE, DT_CENTER),
        text_id(b"", (117, 145, 73, 8), ID_TOTAL, DT_RIGHT),
        text_id(b"", (117, 133, 73, 8), ID_AVAIL, DT_RIGHT),
        tpl::item(WC_STATIC, b"Current Disk Usage", WS_VISIBLE | SS_GROUPBOX, (7, 25, 197, 101), ID_USAGE as u16),
        custom(b"PieChart", (14, 30, 120, 86), ID_PIE, 0),
    ];
    // The legend: only the parts there are (shown in WM_INITDLG).
    let mut y = 91;
    for (i, n) in names.iter().enumerate() {
        let mut l = text_id(n, (145, y, 56, 8), ID_LEG_DIRS + i as u32, DT_LEFT);
        if a.results.parts[i] == 0 {
            l.style &= !WS_VISIBLE;
        } else {
            y -= 12;
        }
        v.push(l);
    }
    v.push(button(b"Cancel", (7, 4, 38), ID_RCANCEL, BS_DEFAULT).group());
    v.push(help_button((57, 4, 38), ID_RHELP));
    let tpl = template(frame, v);
    // At its own place (as OS/2's), not over the Check Disk window.
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, results_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        unsafe {
            WinProcessDlg(dlg);
            WinDestroyWindow(dlg);
        }
    }
}

/// The pie: each part a slice from 0 degrees on, counter-clockwise, in
/// proportion to the disk's size; the free space is the slice left out;
/// a shadow below and to the right.
extern "C" fn pie_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    if m != WM_PAINT {
        return unsafe { WinDefWindowProc(h, m, mp1, mp2) };
    }
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(h, 0, &mut upd) };
    let r = window_rect(h);
    gpi::rgb_mode(hps);
    let bg = unsafe { WinQuerySysColor(HWND_DESKTOP, SYSCLR_DIALOGBACKGROUND, 0) };
    gpi::fill(hps, r, bg);
    let res = &a.results;
    let radius = (r.width().min(r.height()) / 2 - 6).max(4);
    let (cx, cy) = (r.l + radius + 4, (r.b + r.t) / 2 + 2);
    let total = res.total.max(1) as f32;
    let used: u64 = res.parts.iter().sum();
    if used == 0 {
        let t = b"100% of disk space available.";
        gpi::draw_text(hps, r, t, CLR_BLACK, 0, DT_CENTER | DT_VCENTER | DT_WORDBREAK);
        unsafe { WinEndPaint(hps) };
        return 0;
    }
    let p = [radius, radius, 0, 0];
    unsafe { GpiSetArcParams(hps, p.as_ptr()) };
    let slices = |dx: i32, dy: i32, shadow: bool| {
        let mut start = 0.0f32;
        for (i, &part) in res.parts.iter().enumerate() {
            if part == 0 {
                continue;
            }
            let mut sweep = part as f32 / total * 360.0;
            if sweep < 1.0 {
                sweep = 1.0;
            }
            let c = if shadow { 0x606060 } else { PART_COLORS[i] };
            let center = pt(cx + dx, cy + dy);
            gpi::color(hps, c);
            unsafe {
                GpiBeginArea(hps, BA_BOUNDARY | BA_ALTERNATE);
                GpiMove(hps, &center);
                GpiPartialArc(hps, &center, 0x10000, (start * 65536.0) as i32, (sweep * 65536.0) as i32);
                GpiLine(hps, &center);
                GpiEndArea(hps);
            }
            start += sweep;
        }
    };
    slices(4, -4, true);
    slices(0, 0, false);
    unsafe { WinEndPaint(hps) };
    0
}

// ---- Self-test ----

fn selftest() -> u32 {
    let mut fails = 0;
    let mut check = |name: &str, ok: bool| {
        pmkit::trace("pmchkdsk", alloc::format!("{} {name}", if ok { "PASS" } else { "FAIL" }).as_bytes());
        if !ok {
            fails += 1;
        }
    };
    check("parse", child::parse(b"MSG_1361,31377408") == Some((1361, b"31377408".to_vec())));
    check("parse without params", child::parse(b"  MSG_1356") == Some((1356, Vec::new())));
    check("not a message", child::parse(b"The type of file system is FAT.").is_none());
    let mut r = Results::default();
    for l in [&b"MSG_1507,FAT"[..], b"MSG_1361,1000000", b"MSG_1364,4096,2", b"MSG_1365,500000,10", b"MSG_1362,0", b"MSG_1368,495904,2048"] {
        let (n, rest) = child::parse(l).unwrap();
        take_result(&mut r, n, &child::params(&rest));
    }
    check("FAT results", r.fs == b"FAT" && r.total == 1000000 && r.parts[0] == 4096 && r.parts[1] == 500000 && r.free == 495904);
    let mut k = Results::default();
    for l in [&b"MSG_1507,HPFS"[..], b"MSG_568,20480", b"MSG_569,64,3", b"MSG_570,1024,7", b"MSG_632,512", b"MSG_571,18880"] {
        let (n, rest) = child::parse(l).unwrap();
        take_result(&mut k, n, &child::params(&rest));
    }
    check("HPFS results", k.fs == b"HPFS" && k.total == 20480 * 1024 && k.parts[4] == 512 * 1024 && k.free == 18880 * 1024);
    check("question", text_of(1356).map(|t| t.0) == Some(Kind::Question));
    check("parameter", fill("The damaged file %1 was removed.", &[b"C:\\X.TXT".to_vec()]) == b"The damaged file C:\\X.TXT was removed.");
    pmkit::trace("pmchkdsk", alloc::format!("pmchkdsk selftest done: {fails} failure(s)").as_bytes());
    fails
}
