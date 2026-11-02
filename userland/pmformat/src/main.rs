//! PMFORMAT.EXE: rs2's Format window, a re-implementation of OS/2's (the
//! program the drives' "Format disk" menu item starts).
//!
//! As OS/2's (studied in its binary and its resource DLL): `PMFORMAT X:`
//! looks at the drive (DSK_GETDEVICEPARAMS) and shows one of OS/2's
//! dialogs, titled "Format Disk X:": for a hard disk the volume label
//! and the file system (HPFS, JFS or FAT, the drive's current one
//! selected); for a diskette the label and the capacity (the sizes the
//! drive takes); for an optical disk the label and "Long format"; for
//! other removable disks the label only. Format runs `FORMAT.COM X: /P
//! /V:"label"` with `/FS:HPFS|JFS|FAT` (hard disks) or `/ONCE` and
//! `/F:size` or `/L` (removable media), its standard input and output on
//! pipes, and shows "Disk X: - Format Progress": a percent-complete
//! slider, the total and available space, OK (once done), Stop and Help.
//! FORMAT's /P sends `MSG_<number>,<params>` lines (OSO001.MSG numbers):
//! 538 and 1312 percent done, 635 and 1294 finished, 568 and 1283 the
//! total space, 537 and 1286 the space available (formatting done), 1271
//! the "proceed?" question (answered Y), 1288 "press Enter" (answered),
//! 1309 "may take up to 20 minutes" (OK or Cancel), and errors, shown in
//! a message box. Stop ends FORMAT. rs2 adds one question OS/2's does
//! not ask: before a hard disk is formatted, that its data will be lost.
//! Dialog and help ids are OS/2's; the help is PMFORMAT.HLP.
//!
//! `PMFORMAT /SELFTEST` checks the command lines and the handling of
//! FORMAT's lines (PASS/FAIL on the serial port).

#![no_std]
#![no_main]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../pmchkdsk/src/child.rs"]
mod child;

use alloc::vec::Vec;

use pmkit::help::{Help, HelpWindow};
use pmkit::*;

os2::entry!(main);

// Dialogs (OS/2's ids).
const DLG_DISKETTE: u16 = 100;
const DLG_REMOVABLE: u16 = 902;
const DLG_OPTICAL: u16 = 903;
const DLG_DISK: u16 = 5001;
const DLG_PROGRESS: u16 = 900;
// Controls.
const ID_LABEL: u32 = 111;
const ID_CAPACITY: u32 = 258;
const ID_FORMAT: u32 = 222;
const ID_CANCEL: u32 = 333;
const ID_HELP: u32 = 444;
const ID_LONG: u32 = 904;
const ID_FAT: u32 = 5002;
const ID_HPFS: u32 = 5003;
const ID_JFS: u32 = 5004;
const ID_SLIDER: u32 = 901;
const ID_TOTAL: u32 = 1002;
const ID_AVAIL: u32 = 1003;
const ID_OK: u32 = 3001;
const ID_STOP: u32 = 3002;
const ID_PHELP: u32 = 3003;

const WM_LINE: u32 = WM_USER + 1;
const WM_ENDED: u32 = WM_USER + 2;
const SLM_SETSCALETEXT: u32 = 0x0370;
const SLM_SETTICKSIZE: u32 = 0x0372;

const HELP_FILE: &[u8] = b"PMFORMAT.HLP";

/// The capacities offered for each kind of diskette drive.
const CAPACITIES: [&[&[u8]]; 6] = [
    &[b"360KB"],
    &[b"1.2MB", b"360KB"],
    &[b"720KB"],
    &[b"1.44MB", b"720KB"],
    &[b"2.88MB", b"1.44MB", b"720KB"],
    &[b"2.88MB", b"1.44MB", b"720KB", b"1.2MB", b"360KB"],
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Disk,
    Diskette(usize),
    Optical,
    Removable,
}

struct App {
    hab: u32,
    drive: u8,
    kind: Kind,
    help: Option<&'static mut Help>,
    main: u32,
    progress: u32,
    child: Option<child::Child>,
    /// Formatting finished (the space available came).
    done: bool,
    /// An error was shown.
    failed: bool,
    /// The command line for FORMAT.
    args: Vec<u8>,
    /// The capacities in the spin button (kept for its array).
    cap_ptrs: Vec<u32>,
    cap_strs: Vec<Vec<u8>>,
}

static mut APP: App = App {
    hab: 0,
    drive: b'A',
    kind: Kind::Disk,
    help: None,
    main: 0,
    progress: 0,
    child: None,
    done: false,
    failed: false,
    args: Vec::new(),
    cap_ptrs: Vec::new(),
    cap_strs: Vec::new(),
};

fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

fn trace(s: &[u8]) {
    pmkit::trace("pmformat", s);
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
        _ => b'A',
    };
    a.kind = drive_kind(a.drive);
    trace(alloc::format!("drive {}: {:?}", a.drive as char, a.kind).as_bytes());
    let tpl = main_template(a.kind, a.drive);
    run_dialog(0, main_proc, &tpl, 0);
    if let Some(h) = a.help.as_mut() {
        h.destroy(a.main);
    }
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

/// What kind of drive it is, from its device parameters (OS/2's
/// BIOSPARAMETERBLOCK: sectors per track at 13, cylinders at 31, the
/// device type at 33, its attributes at 34; attribute bit 0: not
/// removable).
fn drive_kind(d: u8) -> Kind {
    let name = [d, b':', 0];
    let (mut h, mut action) = (0u32, 0u32);
    // OPEN_FLAGS_DASD | OPEN_SHARE_DENYNONE.
    if unsafe { os2::DosOpen(name.as_ptr(), &mut h, &mut action, 0, 0, 1, 0x8040, core::ptr::null_mut()) } != 0 {
        return Kind::Disk;
    }
    let mut param = [0u8; 2];
    let mut bpb = [0u8; 40];
    let (mut pl, mut dl) = (param.len() as u32, bpb.len() as u32);
    let rc = unsafe { os2::DosDevIOCtl(h, 8, 0x63, param.as_mut_ptr(), pl, &mut pl, bpb.as_mut_ptr(), dl, &mut dl) };
    unsafe { os2::DosClose(h) };
    if rc != 0 {
        return Kind::Disk;
    }
    kind_of(&bpb)
}

pub fn kind_of(bpb: &[u8]) -> Kind {
    let spt = rd16(bpb, 13);
    let cyl = rd16(bpb, 31);
    let ty = bpb[33];
    let attr = rd16(bpb, 34);
    if attr & 1 != 0 {
        return Kind::Disk;
    }
    match ty {
        0 => Kind::Diskette(0),
        1 => Kind::Diskette(1),
        8 => Kind::Optical,
        2 | 7 | 9 if cyl == 80 => Kind::Diskette(match spt {
            9 => 2,
            18 => 3,
            36 => 4,
            _ => 5,
        }),
        2 | 7 | 9 => Kind::Diskette(5),
        _ => Kind::Removable,
    }
}

/// The drive's volume label and file system.
fn current(d: u8) -> (Vec<u8>, Vec<u8>) {
    let mut info = [0u8; 64];
    let label = if unsafe { os2::DosQueryFSInfo((d - b'@') as u32, 2, info.as_mut_ptr(), info.len() as u32) } == 0 {
        let n = (info[4] as usize).min(11);
        info[5..5 + n].to_vec()
    } else {
        Vec::new()
    };
    let name = [d, b':', 0];
    let mut buf = [0u8; 128];
    let mut cb = buf.len() as u32;
    let fs = if unsafe { os2::DosQueryFSAttach(name.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } == 0 {
        let cb_name = rd16(&buf, 2) as usize;
        let cb_fsd = rd16(&buf, 4) as usize;
        let at = 8 + cb_name + 1;
        buf[at..(at + cb_fsd).min(buf.len())].to_vec()
    } else {
        Vec::new()
    };
    (label, fs)
}

fn main_template(kind: Kind, d: u8) -> Vec<u8> {
    let title = [&b"Format Disk "[..], &[d, b':']].concat();
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON.
    let ctl = 0x13u32.to_le_bytes().to_vec();
    let label_row = |y: i16| alloc::vec![label(b"Volume Label:", (9, y + 1, 62, 8), DT_LEFT), entry((81, y, 70, 8), ID_LABEL, 0).group()];
    let (mut frame, mut v) = match kind {
        Kind::Disk => {
            let mut v = label_row(87);
            v.push(group_box(b"File System Type", (5, 31, 190, 48)));
            v.push(radio(b"~HPFS (High Performance File System)", (15, 57, 175, 10), ID_HPFS).group());
            v.push(radio(b"~JFS (Journaled File System)", (15, 47, 163, 10), ID_JFS));
            v.push(radio(b"F~AT (File Allocation Table)", (15, 35, 172, 10), ID_FAT));
            let mut f = dialog_frame(&title, DLG_DISK, 200, 109);
            f.x = 36;
            f.y = 78;
            (f, v)
        }
        Kind::Diskette(_) => {
            let mut v = label_row(50);
            v.push(label(b"Capacity:", (9, 30, 42, 8), DT_LEFT));
            v.push(tpl::item(WC_SPINBUTTON, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | SPBS_READONLY | SPBS_MASTER | 0x8, (79, 27, 75, 13), ID_CAPACITY as u16));
            let mut f = dialog_frame(&title, DLG_DISKETTE, 166, 73);
            f.x = 45;
            f.y = 36;
            (f, v)
        }
        Kind::Optical => {
            let mut v = label_row(46);
            v.push(check_box(b"Long format", (9, 26, 143, 10), ID_LONG).group());
            let mut f = dialog_frame(&title, DLG_OPTICAL, 165, 64);
            f.x = 91;
            f.y = 85;
            (f, v)
        }
        Kind::Removable => {
            let v = label_row(27);
            let mut f = dialog_frame(&title, DLG_REMOVABLE, 165, 45);
            f.x = 36;
            f.y = 95;
            (f, v)
        }
    };
    frame.ctl = ctl;
    let bx: [i16; 3] = if matches!(kind, Kind::Disk | Kind::Optical) { [6, 47, 88] } else { [9, 59, 109] };
    v.push(button(b"~Format", (bx[0], 4, 38), ID_FORMAT, BS_DEFAULT).group());
    v.push(button(b"Cancel", (bx[1], 4, 38), ID_CANCEL, 0));
    v.push(help_button((bx[2], 4, 38), ID_HELP));
    template(frame, v)
}

/// Put the drive's current state in the dialog.
fn fill_main(h: u32) {
    let a = app();
    let (lab, fs) = current(a.drive);
    dlg_msg(h, ID_LABEL, EM_SETTEXTLIMIT, 11, 0);
    set_item_text(h, ID_LABEL, &lab);
    match a.kind {
        Kind::Disk => {
            let id = match fs.as_slice() {
                b"FAT" | b"FAT32" => ID_FAT,
                b"JFS" => ID_JFS,
                _ => ID_HPFS,
            };
            set_check(h, id, true);
        }
        Kind::Diskette(t) => {
            a.cap_strs = CAPACITIES[t].iter().map(|c| z(c)).collect();
            a.cap_ptrs = a.cap_strs.iter().map(|c| c.as_ptr() as u32).collect();
            dlg_msg(h, ID_CAPACITY, SPBM_SETARRAY, a.cap_ptrs.as_ptr() as u32, a.cap_ptrs.len() as u32);
            dlg_msg(h, ID_CAPACITY, SPBM_SETCURRENTVALUE, 0, 0);
        }
        _ => {}
    }
}

fn add_switch_entry(h: u32) {
    let (mut pid, mut tid) = (0, 0);
    unsafe { WinQueryWindowProcess(h, &mut pid, &mut tid) };
    let mut sw = SwCntrl { hwnd: h, icon: 0, program: 0, pid, session: 0, visibility: 0, jump: 2, title: [0; 60], prog_type: 0 };
    let t = window_text(h);
    let n = t.len().min(59);
    sw.title[..n].copy_from_slice(&t[..n]);
    unsafe { WinAddSwitchEntry(&sw) };
}

extern "C" fn main_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            a.main = h;
            let items: &[(u16, u16)] = &[(ID_LABEL as u16, 0), (ID_FORMAT as u16, 0), (ID_CANCEL as u16, 0), (ID_HPFS as u16, 0), (ID_JFS as u16, 0), (ID_FAT as u16, 0), (ID_CAPACITY as u16, 0), (ID_LONG as u16, 0)];
            // Every control of a dialog shows the dialog's panel.
            let with = |p: u16| items.iter().map(|&(i, _)| (i, p)).collect::<Vec<_>>();
            let (d1, d2, d3, d4, d5) = (with(8069), with(8677), with(8678), with(8063), alloc::vec![(ID_OK as u16, 8072), (ID_STOP as u16, 8072)]);
            a.help = Help::create(
                a.hab,
                h,
                HELP_FILE,
                b"Format Help",
                &[
                    HelpWindow { id: DLG_DISKETTE, ext: 8069, items: &d1 },
                    HelpWindow { id: DLG_REMOVABLE, ext: 8677, items: &d2 },
                    HelpWindow { id: DLG_OPTICAL, ext: 8678, items: &d3 },
                    HelpWindow { id: DLG_DISK, ext: 8063, items: &d4 },
                    HelpWindow { id: DLG_PROGRESS, ext: 8072, items: &d5 },
                ],
            );
            fill_main(h);
            add_switch_entry(h);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_LABEL)) };
            1
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_FORMAT => format(h),
                ID_CANCEL | DID_CANCEL => unsafe {
                    WinDismissDlg(h, ID_CANCEL);
                },
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            unsafe { WinDismissDlg(h, ID_CANCEL) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// A volume label FORMAT accepts: up to 11 characters, none of the
/// characters a file name may not have.
pub fn label_ok(l: &[u8]) -> bool {
    l.len() <= 11 && l.iter().all(|&c| c >= 0x20 && !b"*?/\\|.,;:+=<>[]&^\"".contains(&c))
}

/// FORMAT's command line (after the program's name).
pub fn command_line(drive: u8, kind: Kind, label: &[u8], fs: &[u8], capacity: &[u8], long: bool) -> Vec<u8> {
    let mut c = alloc::vec![drive, b':'];
    c.extend_from_slice(b" /P /V:\"");
    c.extend_from_slice(label);
    c.push(b'"');
    match kind {
        Kind::Disk => {
            c.extend_from_slice(b" /FS:");
            c.extend_from_slice(fs);
        }
        Kind::Diskette(_) => {
            c.extend_from_slice(b" /ONCE /F:");
            c.extend_from_slice(capacity);
        }
        Kind::Optical => {
            c.extend_from_slice(b" /ONCE");
            if long {
                c.extend_from_slice(b" /L");
            }
        }
        Kind::Removable => c.extend_from_slice(b" /ONCE"),
    }
    c
}

fn format(h: u32) {
    let a = app();
    let lab = item_text(h, ID_LABEL);
    if !label_ok(&lab) {
        message(h, b"The volume label has a character that cannot be used in it.", b"Format Disk", MB_OK | MB_ERROR);
        return;
    }
    let fs: &[u8] = if checked(h, ID_FAT) {
        b"FAT"
    } else if checked(h, ID_JFS) {
        b"JFS"
    } else {
        b"HPFS"
    };
    let mut cap = alloc::vec![0u8; 16];
    if let Kind::Diskette(_) = a.kind {
        dlg_msg(h, ID_CAPACITY, SPBM_QUERYVALUE, cap.as_mut_ptr() as u32, mp(cap.len() as u32, 0));
    }
    let cap = until_nul(&cap).to_vec();
    if a.kind == Kind::Disk {
        let t = alloc::format!("All the data on drive {}: will be lost. Do you want to format it?", a.drive as char);
        if message(h, t.as_bytes(), b"Warning!", MB_YESNO | MB_WARNING | MB_DEFBUTTON2) != MBID_YES {
            return;
        }
    }
    a.args = command_line(a.drive, a.kind, &lab, fs, &cap, checked(h, ID_LONG));
    run_progress(h);
    // Show the drive as it is now.
    fill_main(h);
}

// ---- Progress ----

fn run_progress(owner: u32) {
    let a = app();
    let title = [&b"Disk "[..], &[a.drive, b':'], b" - Format Progress"].concat();
    let mut frame = dialog_frame(&title, DLG_PROGRESS, 200, 97);
    frame.x = 46;
    frame.y = 68;
    // SLDCDATA: cbSize, 101 increments 2 apart on both scales.
    let mut sld = Vec::new();
    for v in [12u16, 0, 101, 2, 101, 2] {
        sld.extend_from_slice(&v.to_le_bytes());
    }
    let slider = tpl::item(WC_SLIDER, b"Percent Complete", WS_VISIBLE | 0x0001_0000 | 0x182, (10, 55, 180, 22), ID_SLIDER as u16).ctl(&sld[..12]);
    let mut ok = button(b"~OK", (6, 4, 40), ID_OK, 0);
    ok.style |= WS_DISABLED;
    ok.style &= !WS_TABSTOP;
    let tpl = template(
        frame,
        alloc::vec![
            label(b"Percent Complete", (61, 82, 77, 8), DT_LEFT),
            slider,
            label(b"Total space on disk:", (11, 38, 84, 8), DT_LEFT),
            label(b"Space available:", (11, 26, 68, 8), DT_LEFT),
            text_id(b"", (101, 38, 72, 8), ID_TOTAL, DT_RIGHT),
            text_id(b"", (87, 26, 86, 8), ID_AVAIL, DT_RIGHT),
            ok.group(),
            button(b"~Stop", (49, 4, 38), ID_STOP, BS_DEFAULT),
            help_button((90, 4, 38), ID_PHELP),
        ],
    );
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, progress_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        unsafe {
            WinProcessDlg(dlg);
            WinDestroyWindow(dlg);
        }
    }
    a.progress = 0;
}

fn set_percent(h: u32, p: u32) {
    // SMA_SLIDERARMPOSITION in SMA_INCREMENTVALUE.
    dlg_msg(h, ID_SLIDER, SLM_SETSLIDERINFO, mp(SMA_SLIDERARMPOSITION, SMA_INCREMENTVALUE), p.min(100));
}

/// Formatting is done: OK becomes the default, Stop is disabled.
fn finished(h: u32) {
    let a = app();
    a.done = true;
    enable(h, ID_OK, true);
    enable(h, ID_STOP, false);
    dlg_msg(h, ID_OK, BM_SETDEFAULT, 1, 0);
    dlg_msg(h, ID_STOP, BM_SETDEFAULT, 0, 0);
    unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, ID_OK)) };
}

fn stop(h: u32) {
    let a = app();
    if let Some(mut c) = a.child.take() {
        c.kill();
        c.close();
    }
    unsafe { WinDismissDlg(h, ID_STOP) };
}

/// rs2's texts for FORMAT's error messages.
pub fn error_text(n: u32) -> Option<&'static str> {
    Some(match n {
        19 => "The disk is write-protected.",
        123 => "The volume label has a character that cannot be used in it.",
        525 => "This disk cannot be formatted.",
        526 | 532 | 540 => "The disk cannot be formatted for HPFS.",
        527 => "HPFS data cannot be copied to this disk.",
        528 => "Formatting did not finish.",
        529 => "There is not enough memory for FORMAT.",
        1210 => "This version of the operating system cannot run FORMAT.",
        1274 => "The partition is too large for this file system.",
        1276 => "The capacity or the volume label is not right.",
        1279 => "The format was unsuccessful.",
        1281 => "Track 0 cannot be formatted: the disk is not usable.",
        1296 => "The file system cannot be written to this disk.",
        1303 => "The system cannot format this drive now (it is in use, or it is the drive the system was started from).",
        1313 => "A part of the disk could not be read or written.",
        1319 => "The volume label entered is not the drive's.",
        1509 => "The system cannot run FORMAT.",
        _ => return None,
    })
}

/// What a line from FORMAT does to the progress window.
#[derive(PartialEq, Debug)]
pub enum Action {
    Percent(u32),
    Total(Vec<u8>),
    Available(Vec<u8>),
    Answer(&'static [u8]),
    AskLong,
    Error(&'static str),
    Ignore,
}

pub fn action(n: u32, p: &[u8]) -> Action {
    let first = p.split(|&c| c == b',').next().unwrap_or(b"").to_vec();
    let kb = |v: Vec<u8>| [&v[..], b" KB"].concat();
    match n {
        538 | 1312 => Action::Percent(parse_num(&first).unwrap_or(0).clamp(0, 100) as u32),
        635 | 1294 => Action::Percent(100),
        1283 => Action::Total(first),
        568 => Action::Total(kb(first)),
        1286 => Action::Available(first),
        537 => Action::Available(kb(first)),
        1271 => Action::Answer(b"Y"),
        1288 => Action::Answer(b""),
        1309 => Action::AskLong,
        _ => match error_text(n) {
            Some(t) => Action::Error(t),
            None => Action::Ignore,
        },
    }
}

extern "C" fn progress_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            a.progress = h;
            a.done = false;
            a.failed = false;
            for (i, t) in [&b"0\0"[..], b"50\0", b"100\0"].iter().enumerate() {
                let tick = i as u32 * 50;
                dlg_msg(h, ID_SLIDER, SLM_SETTICKSIZE, mp(tick, 5), 0);
                dlg_msg(h, ID_SLIDER, SLM_SETSCALETEXT, tick, t.as_ptr() as u32);
            }
            let Some(prog) = child::find_program(&[b"FORMAT.COM", b"FORMAT.EXE"]) else {
                message(h, b"Format cannot find the program FORMAT.COM, which formats the disk.", b"Format Disk", MB_OK | MB_ERROR);
                unsafe { WinDismissDlg(h, ID_STOP) };
                return 0;
            };
            // A test aid: RS2FORMATTEST=script.cmd runs the script (with
            // FORMAT's arguments) in CMD.EXE instead of FORMAT.
            let mut test: *const u8 = core::ptr::null();
            let (prog, args) = if unsafe { os2::DosScanEnv(b"RS2FORMATTEST\0".as_ptr(), &mut test) } == 0 && !test.is_null() {
                let cmd = [&[boot_drive()][..], b":\\OS2\\CMD.EXE"].concat();
                (cmd, [&b"/C "[..], unsafe { cstr(test) }, b" ", &a.args].concat())
            } else {
                (prog, a.args.clone())
            };
            trace(&[&prog[..], b" ", &args].concat());
            match child::start(&prog, &args, h, WM_LINE, WM_ENDED) {
                Ok(c) => a.child = Some(c),
                Err(rc) => {
                    let t = alloc::format!("Format cannot start FORMAT (error {rc}).");
                    message(h, t.as_bytes(), b"Format Disk", MB_OK | MB_ERROR);
                    unsafe { WinDismissDlg(h, ID_STOP) };
                }
            }
            1
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_OK => unsafe {
                    WinDismissDlg(h, ID_OK);
                },
                ID_STOP | DID_CANCEL => {
                    if a.done || a.child.is_none() {
                        unsafe { WinDismissDlg(h, ID_OK) };
                    } else {
                        stop(h);
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            if a.child.is_some() {
                stop(h);
            } else {
                unsafe { WinDismissDlg(h, ID_OK) };
            }
            0
        }
        WM_LINE => {
            let rest = child::take_line(mp2);
            trace(alloc::format!("MSG_{mp1}").as_bytes());
            match action(mp1, &rest) {
                Action::Percent(p) => set_percent(h, p),
                Action::Total(t) => set_item_text(h, ID_TOTAL, &t),
                Action::Available(t) => {
                    set_item_text(h, ID_AVAIL, &t);
                    set_percent(h, 100);
                    finished(h);
                }
                Action::Answer(t) => {
                    if let Some(c) = a.child.as_ref() {
                        c.answer(t);
                    }
                }
                Action::AskLong => {
                    let r = message(h, b"Formatting this disk can take up to 20 minutes.", b"Warning!", MB_OKCANCEL | MB_WARNING);
                    if r == MBID_CANCEL {
                        stop(h);
                    } else if let Some(c) = a.child.as_ref() {
                        c.answer(b"Y");
                    }
                }
                Action::Error(t) => {
                    a.failed = true;
                    message(h, t.as_bytes(), b"Format Disk", MB_OK | MB_ERROR);
                }
                Action::Ignore => {}
            }
            0
        }
        WM_ENDED => {
            if let Some(mut c) = a.child.take() {
                c.close();
            }
            trace(alloc::format!("format ended ({mp1})").as_bytes());
            if !a.done && !a.failed && mp1 != 0 {
                let t = alloc::format!("The format was unsuccessful (FORMAT ended with code {mp1}).");
                message(h, t.as_bytes(), b"Format Disk", MB_OK | MB_ERROR);
            }
            if !a.done {
                unsafe { WinDismissDlg(h, ID_STOP) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

// ---- Self-test ----

fn selftest() -> u32 {
    let mut fails = 0;
    let mut check = |name: &str, ok: bool| {
        pmkit::trace("pmformat", alloc::format!("{} {name}", if ok { "PASS" } else { "FAIL" }).as_bytes());
        if !ok {
            fails += 1;
        }
    };
    check("hard disk command", command_line(b'D', Kind::Disk, b"DATA", b"HPFS", b"", false) == b"D: /P /V:\"DATA\" /FS:HPFS");
    check("diskette command", command_line(b'A', Kind::Diskette(3), b"", b"", b"1.44MB", false) == b"A: /P /V:\"\" /ONCE /F:1.44MB");
    check("optical command", command_line(b'E', Kind::Optical, b"X", b"", b"", true) == b"E: /P /V:\"X\" /ONCE /L");
    check("label", label_ok(b"RS2 DISK") && !label_ok(b"A*B") && !label_ok(b"TWELVECHARSS"));
    check("percent", action(538, b"42") == Action::Percent(42));
    check("total", action(568, b"20480") == Action::Total(b"20480 KB".to_vec()));
    check("available", action(1286, b"1234") == Action::Available(b"1234".to_vec()) && action(537, b"100") == Action::Available(b"100 KB".to_vec()));
    check("proceed answered", action(1271, b"D:") == Action::Answer(b"Y"));
    check("error", matches!(action(1303, b""), Action::Error(_)));
    let mut bpb = [0u8; 40];
    bpb[34] = 1;
    check("fixed disk", kind_of(&bpb) == Kind::Disk);
    bpb[34] = 0;
    bpb[33] = 7;
    bpb[31] = 80;
    bpb[13] = 18;
    check("1.44 MB diskette", kind_of(&bpb) == Kind::Diskette(3));
    bpb[33] = 8;
    check("optical", kind_of(&bpb) == Kind::Optical);
    pmkit::trace("pmformat", alloc::format!("pmformat selftest done: {fails} failure(s)").as_bytes());
    fails
}
