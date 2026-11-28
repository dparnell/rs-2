//! COMDLG32: the common dialogs (GetOpenFileName, GetSaveFileName,
//! ChooseColor, ChooseFont, FindText, ReplaceText, CommDlgExtendedError,
//! GetFileTitle), A and W. WIN16's COMMDLG (userland/win16/src/commdlg.rs)
//! with Win32's structures.
//!
//! The dialogs are made from templates built here (DLGTEMPLATEs in dialog
//! units of MS Sans Serif 8) with Windows' control IDs (dlgs.h: edt1,
//! lst1, cmb1, chx1, rad1, psh1...), so a program's own template (the
//! *_ENABLETEMPLATE flags) works with the dialog procedure here, as on
//! Windows. The dialog procedure is the built-in `Rs2CommDlgProc`. A
//! program's hook procedure gets every message first (WM_INITDIALOG with
//! the structure as lParam), in the A or W form of the call, and keeps
//! the default handling from happening by returning nonzero.
//!
//! The structures are read and written in the program's memory; their
//! strings are A or W as the call was. Explorer-style dialogs
//! (OFN_EXPLORER) are the old style here: their hooks and templates,
//! which expect a child dialog and CDN_ notifications, are left out.
//! There are no printers: PrintDlg and PageSetupDlg fail with
//! PDERR_NODEFAULTPRN.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::dialog::get_item;
use crate::res;
use crate::wnd::*;
use crate::{gdi, Global};

// Control IDs (dlgs.h).
const IDOK: u16 = 1;
const IDCANCEL: u16 = 2;
const PSH1: u16 = 0x400;
const PSH2: u16 = 0x401;
const PSH3: u16 = 0x402;
const PSH_HELP: u16 = 0x40e;
const CHX1: u16 = 0x410;
const CHX2: u16 = 0x411;
const RAD1: u16 = 0x420;
const RAD2: u16 = 0x421;
const RAD3: u16 = 0x422;
const RAD4: u16 = 0x423;
const GRP1: u16 = 0x430;
const GRP2: u16 = 0x431;
const GRP3: u16 = 0x432;
const STC1: u16 = 0x440;
const STC2: u16 = 0x441;
const STC3: u16 = 0x442;
const STC4: u16 = 0x443;
const STC5: u16 = 0x444;
const LST1: u16 = 0x460;
const LST2: u16 = 0x461;
const CMB1: u16 = 0x470;
const CMB2: u16 = 0x471;
const CMB3: u16 = 0x472;
const CMB4: u16 = 0x473;
const EDT1: u16 = 0x480;
const EDT2: u16 = 0x481;
const EDT3: u16 = 0x482;
// The colour dialog's (color.dlg).
const COLOR_HUE: u16 = 703;
const COLOR_SAT: u16 = 704;
const COLOR_LUM: u16 = 705;
const COLOR_RED: u16 = 706;
const COLOR_GREEN: u16 = 707;
const COLOR_BLUE: u16 = 708;
const COLOR_ADD: u16 = 712;
const COLOR_MIX: u16 = 719;
/// The basic colours' and custom colours' buttons (rs2's own IDs).
const COLOR_BASIC: u16 = 800;
const COLOR_CUST: u16 = 860;

// Messages (Win32's numbers).
const LB_ADDSTRING: u16 = 0x0180;
const LB_RESETCONTENT: u16 = 0x0184;
const LB_SETCURSEL: u16 = 0x0186;
const LB_GETCURSEL: u16 = 0x0188;
const CB_ADDSTRING: u16 = 0x0143;
const CB_GETCURSEL: u16 = 0x0147;
const CB_GETLBTEXT: u16 = 0x0148;
const CB_FINDSTRINGEXACT: u16 = 0x0158;
const CB_SETCURSEL: u16 = 0x014e;
const BM_GETCHECK: u16 = 0x00f0;
const BM_SETCHECK: u16 = 0x00f1;
const EM_LIMITTEXT: u16 = 0x00c5;
const WM_CHOOSEFONT_GETLOGFONT: u16 = W_USER + 1;
const LBN_SELCHANGE: u16 = 1;
const LBN_DBLCLK: u16 = 2;
const CBN_SELCHANGE: u16 = 1;
const CBN_EDITCHANGE: u16 = 5;
const EN_CHANGE: u16 = 0x0300;
const BN_CLICKED: u16 = 0;

// Styles.
const CHILD: u32 = WS_CHILD | WS_VISIBLE;
const TAB: u32 = WS_TABSTOP;
const GROUP: u32 = WS_GROUP;
const DS_MODALFRAME: u32 = 0x80;
const DS_SETFONT: u32 = 0x40;
const DLG: u32 = WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME;
const SS_RIGHT: u32 = 2;
const SS_NOPREFIX: u32 = 0x80;
const BS_DEFPUSHBUTTON: u32 = 1;
const BS_AUTOCHECKBOX: u32 = 3;
const BS_GROUPBOX: u32 = 7;
const BS_AUTORADIOBUTTON: u32 = 9;
const BS_OWNERDRAW: u32 = 0xb;
const ES_AUTOHSCROLL: u32 = 0x80;
const LBS_NOTIFY: u32 = 1;
const LBS_SORT: u32 = 2;
const CBS_SIMPLE: u32 = 1;
const CBS_DROPDOWNLIST: u32 = 3;
const CBS_AUTOHSCROLL: u32 = 0x40;
const CBS_SORT: u32 = 0x100;

// CommDlgExtendedError's codes.
const CDERR_STRUCTSIZE: u32 = 0x0001;
const CDERR_NOTEMPLATE: u32 = 0x0003;
const CDERR_FINDRESFAILURE: u32 = 0x0006;
const PDERR_NODEFAULTPRN: u32 = 0x1008;
const PDERR_RETDEFFAILURE: u32 = 0x1009;
const PDERR_CREATEICFAILURE: u32 = 0x100a;
const FNERR_INVALIDFILENAME: u32 = 0x3002;
const FNERR_BUFFERTOOSMALL: u32 = 0x3003;
const FRERR_BUFFERLENGTHZERO: u32 = 0x4001;

static ERROR: Global<u32> = Global::new(0);

/// A dialog template being built (DLGTEMPLATE, MS Sans Serif 8, and
/// DLGITEMTEMPLATEs, DWORD-aligned), in dialog units.
struct Tpl {
    b: Vec<u8>,
    n: u16,
}

fn put_w(b: &mut Vec<u8>, s: &[u8]) {
    for c in crate::to_wide(s) {
        b.extend_from_slice(&c.to_le_bytes());
    }
    b.extend_from_slice(&[0, 0]);
}

impl Tpl {
    fn new(style: u32, x: i16, y: i16, cx: i16, cy: i16, caption: &[u8]) -> Tpl {
        let mut b = Vec::with_capacity(1024);
        b.extend_from_slice(&(style | DS_SETFONT).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        for v in [x, y, cx, cy] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        // No menu, the dialog class, the caption, the font.
        b.extend_from_slice(&[0, 0, 0, 0]);
        put_w(&mut b, caption);
        b.extend_from_slice(&8u16.to_le_bytes());
        put_w(&mut b, b"MS Sans Serif");
        Tpl { b, n: 0 }
    }
    /// A control of a predefined class (0x80 BUTTON, 0x81 EDIT, 0x82
    /// STATIC, 0x83 LISTBOX, 0x85 COMBOBOX).
    #[allow(clippy::too_many_arguments)]
    fn item(&mut self, class: u8, text: &[u8], id: u16, style: u32, x: i16, y: i16, cx: i16, cy: i16) -> &mut Tpl {
        while self.b.len() % 4 != 0 {
            self.b.push(0);
        }
        self.b.extend_from_slice(&(style | CHILD).to_le_bytes());
        self.b.extend_from_slice(&0u32.to_le_bytes());
        for v in [x, y, cx, cy] {
            self.b.extend_from_slice(&v.to_le_bytes());
        }
        self.b.extend_from_slice(&id.to_le_bytes());
        self.b.extend_from_slice(&[0xff, 0xff, class, 0]);
        put_w(&mut self.b, text);
        self.b.extend_from_slice(&[0, 0]);
        self.n += 1;
        self
    }
    fn label(&mut self, text: &[u8], id: u16, x: i16, y: i16, cx: i16, cy: i16) -> &mut Tpl {
        self.item(0x82, text, id, 0, x, y, cx, cy)
    }
    fn button(&mut self, text: &[u8], id: u16, style: u32, x: i16, y: i16, cx: i16, cy: i16) -> &mut Tpl {
        self.item(0x80, text, id, style | TAB, x, y, cx, cy)
    }
    fn edit(&mut self, id: u16, x: i16, y: i16, cx: i16, cy: i16) -> &mut Tpl {
        self.item(0x81, b"", id, WS_BORDER | ES_AUTOHSCROLL | TAB, x, y, cx, cy)
    }
    fn done(mut self) -> Vec<u8> {
        self.b[8..10].copy_from_slice(&self.n.to_le_bytes());
        self.b
    }
}

/// The kinds of dialog and their state.
enum Kind {
    File(File),
    Color(Color),
    Font(Font),
    Find(Find),
    Print(Print),
}

struct Cd {
    kind: Kind,
    /// The program's structure.
    st: u32,
    hook: u32,
    /// The call was the W one (its strings and hook are W).
    wide: bool,
}

static PENDING: Global<Option<Cd>> = Global::new(None);
static STATES: Global<BTreeMap<u16, Box<Cd>>> = Global::new(BTreeMap::new());

fn state(h: u16) -> Option<&'static mut Cd> {
    STATES.get().get_mut(&h).map(|b| &mut **b)
}

fn wide(h: u16) -> bool {
    state(h).is_some_and(|s| s.wide)
}

// ---- Reading and writing the program's structures ----

fn rd16(st: u32, off: usize) -> u16 {
    crate::rd16(st, off as u32)
}
fn rd32(st: u32, off: usize) -> u32 {
    crate::rd32(st, off as u32)
}
fn wr16(st: u32, off: usize, v: u16) {
    unsafe { ((st + off as u32) as *mut u16).write_unaligned(v) }
}
fn wr32(st: u32, off: usize, v: u32) {
    crate::wr32(st, off as u32, v)
}

/// The string at a pointer (A or W) in the process code page; None for
/// a null pointer.
fn get_str(p: u32, wide: bool) -> Option<Vec<u8>> {
    if p == 0 {
        return None;
    }
    Some(if wide { crate::narrow(unsafe { crate::wstr(p as *const u16) }) } else { unsafe { crate::cstr(p as *const u8) }.to_vec() })
}

/// A string into a buffer of `max` characters (A or W).
fn put_str(p: u32, max: u32, s: &[u8], wide: bool) {
    if p == 0 {
        return;
    }
    if wide {
        crate::copy_out_w(&crate::to_wide(s), p as *mut u16, max as i32);
    } else {
        crate::copy_out(s, p as *mut u8, max as i32);
    }
}

/// The length of a string as the program counts it (A bytes, W UTF-16
/// units).
fn units(s: &[u8], wide: bool) -> usize {
    if wide { crate::to_wide(s).len() } else { s.len() }
}

/// A list of strings ending with an empty one (the filters).
fn str_list(mut p: u32, wide: bool) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    if p == 0 {
        return v;
    }
    for _ in 0..256 {
        let Some(s) = get_str(p, wide) else { break };
        if s.is_empty() {
            break;
        }
        p += (units(&s, wide) as u32 + 1) * if wide { 2 } else { 1 };
        v.push(s);
    }
    v
}

// ---- Controls ----

fn set_text(h: u16, id: u16, t: &[u8]) {
    let c = get_item(h, id);
    if c != 0 {
        let z = crate::z(t);
        send(c, W_SETTEXT, 0, z.as_ptr() as u32);
    }
}

fn text(h: u16, id: u16) -> Vec<u8> {
    crate::ctl::text_of(get_item(h, id))
}

/// Send a message with a string as lParam.
fn send_str(c: u16, m: u16, wp: u32, s: &[u8]) -> u32 {
    let z = crate::z(s);
    send(c, m, wp, z.as_ptr() as u32)
}

/// The text of a list box's (LB_GETTEXT) or combo box's (CB_GETLBTEXT)
/// item.
fn item_text(c: u16, m: u16, i: u32) -> Vec<u8> {
    let mut b = vec![0u8; 512];
    if send(c, m, i, b.as_mut_ptr() as u32) as i32 == -1 {
        return Vec::new();
    }
    let n = b.iter().position(|&x| x == 0).unwrap_or(0);
    b.truncate(n);
    b
}

fn checked(h: u16, id: u16) -> bool {
    send(get_item(h, id), BM_GETCHECK, 0, 0) != 0
}

fn check(h: u16, id: u16, on: bool) {
    send(get_item(h, id), BM_SETCHECK, on as u32, 0);
}

fn show(h: u16, id: u16, on: bool) {
    let c = get_item(h, id);
    if c != 0 {
        show_window(c, if on { 5 } else { 0 });
    }
}

fn enable(h: u16, id: u16, on: bool) {
    let c = get_item(h, id);
    if c != 0 {
        enable_window(c, on);
    }
}

/// A message box owned by the dialog (MB_* style): the button pressed.
fn message(h: u16, t: &[u8], style: u32) -> u32 {
    let title = window_text(h);
    message_box(h, t, &title, style)
}

/// The program's template (from its resources or memory) when
/// `use_res`/`use_handle`, else ours.
fn template(use_res: bool, use_handle: bool, hinst: u32, name: u32, wide: bool, ours: Vec<u8>) -> Option<Vec<u8>> {
    if use_handle {
        if hinst == 0 {
            *ERROR.get() = CDERR_NOTEMPLATE;
            return None;
        }
        return Some(unsafe { core::slice::from_raw_parts(hinst as *const u8, 0x8000) }.to_vec());
    }
    if use_res {
        let t = unsafe { res::Name::of(name, wide) }.and_then(|n| res::find(hinst, res::RT_DIALOG, &n));
        if t.is_none() {
            *ERROR.get() = CDERR_FINDRESFAILURE;
        }
        return t.map(|t| t.to_vec());
    }
    Some(ours)
}

unsafe extern "C" {
    fn __w32t_Rs2CommDlgProc();
}

/// The address of the dialog procedure here.
pub fn proc_addr() -> u32 {
    crate::user32::thunk(__w32t_Rs2CommDlgProc)
}

/// The common dialogs' dialog procedure (built in).
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2CommDlgProc(h: u32, m: u32, wp: u32, lp: u32) -> u32 {
    dlg_proc(hw(h), m as u16, wp, lp)
}

/// Run a modal common dialog: its EndDialog result.
fn run(t: &[u8], owner: u32, hinst: u32, cd: Cd) -> i32 {
    *PENDING.get() = Some(cd);
    let r = crate::dialog::dialog_box(hinst, t, hw(owner), proc_addr(), 0, false);
    *PENDING.get() = None;
    r
}

/// The program's hook procedure, in the form of its call.
fn call_hook(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    let Some((hook, wide)) = state(h).map(|s| (s.hook, s.wide)) else { return 0 };
    if hook == 0 {
        return 0;
    }
    let r = call_conv(hook, wide, h, m, wp, lp, false);
    // A 16-bit hook's UINT is in AX alone.
    if far16(hook).is_some() { r & 0xffff } else { r }
}

fn end(h: u16, r: i32) {
    crate::dialog::EndDialog(h as u32, r);
}

// ---- The dialog procedure ----

pub fn dlg_proc(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    if m == W_INITDIALOG {
        let Some(cd) = PENDING.get().take() else { return 1 };
        let st = cd.st;
        STATES.get().insert(h, Box::new(cd));
        init(h);
        if state(h).is_some_and(|s| s.hook != 0) {
            return call_hook(h, m, wp, st);
        }
        return 1;
    }
    if state(h).is_none() {
        return 0;
    }
    if call_hook(h, m, wp, lp) != 0 {
        return 1;
    }
    if m == W_NCDESTROY || m == W_DESTROY {
        if m == W_NCDESTROY {
            STATES.get().remove(&h);
        }
        if m == W_DESTROY {
            font_destroy(h);
        }
        return 0;
    }
    let Some(cd) = state(h) else { return 0 };
    match cd.kind {
        Kind::File(_) => file_msg(h, m, wp, lp),
        Kind::Color(_) => color_msg(h, m, wp, lp),
        Kind::Font(_) => font_msg(h, m, wp, lp),
        Kind::Find(_) => find_msg(h, m, wp, lp),
        Kind::Print(_) => print_msg(h, m, wp, lp),
    }
}

fn init(h: u16) {
    let Some(cd) = state(h) else { return };
    match cd.kind {
        Kind::File(_) => file_init(h),
        Kind::Color(_) => color_init(h),
        Kind::Font(_) => font_init(h),
        Kind::Find(_) => find_init(h),
        Kind::Print(_) => print_init(h),
    }
}

// ---- Open and Save As ----

#[derive(Default)]
struct File {
    save: bool,
    /// The directory shown ("c:\dir").
    dir: Vec<u8>,
    /// The patterns of the file list ("*.txt;*.doc").
    pattern: Vec<u8>,
    /// The directories of the directory list's items, and how they are
    /// shown (indented).
    dirs: Vec<Vec<u8>>,
    labels: Vec<Vec<u8>>,
    /// The file list's names (its items, for owner-drawn lists).
    files: Vec<Vec<u8>>,
    /// The filter patterns (cmb1's items).
    filters: Vec<Vec<u8>>,
    /// The current directory when the dialog started.
    start: Vec<u8>,
}

// OPENFILENAME offsets.
const OFN_HWNDOWNER: usize = 4;
const OFN_HINSTANCE: usize = 8;
const OFN_FILTER: usize = 12;
const OFN_FILTERINDEX: usize = 24;
const OFN_FILE: usize = 28;
const OFN_MAXFILE: usize = 32;
const OFN_FILETITLE: usize = 36;
const OFN_MAXFILETITLE: usize = 40;
const OFN_INITIALDIR: usize = 44;
const OFN_TITLE: usize = 48;
const OFN_FLAGS: usize = 52;
const OFN_FILEOFFSET: usize = 56;
const OFN_FILEEXTENSION: usize = 58;
const OFN_DEFEXT: usize = 60;
const OFN_HOOK: usize = 68;
const OFN_TEMPLATENAME: usize = 72;

const OFN_ALLOWMULTISELECT: u32 = 0x200;
const OFN_EXPLORER: u32 = 0x80000;
const OFN_READONLY: u32 = 0x1;
const OFN_OVERWRITEPROMPT: u32 = 0x2;
const OFN_HIDEREADONLY: u32 = 0x4;
const OFN_NOCHANGEDIR: u32 = 0x8;
const OFN_SHOWHELP: u32 = 0x10;
const OFN_ENABLEHOOK: u32 = 0x20;
const OFN_ENABLETEMPLATE: u32 = 0x40;
const OFN_ENABLETEMPLATEHANDLE: u32 = 0x80;
const OFN_EXTENSIONDIFFERENT: u32 = 0x400;
const OFN_PATHMUSTEXIST: u32 = 0x800;
const OFN_FILEMUSTEXIST: u32 = 0x1000;
const OFN_CREATEPROMPT: u32 = 0x2000;
const OFN_NOREADONLYRETURN: u32 = 0x8000;

fn file_template(save: bool) -> Vec<u8> {
    let mut t = Tpl::new(DLG, 36, 24, 264, 134, if save { b"Save As" } else { b"Open" });
    t.label(b"File &Name:", 0x442, 6, 6, 76, 9)
        .edit(EDT1, 6, 16, 90, 12)
        .item(0x83, b"", LST1, LBS_NOTIFY | LBS_SORT | WS_VSCROLL | WS_BORDER | TAB, 6, 32, 90, 68)
        .label(b"&Directories:", 0xffff, 110, 6, 92, 9)
        .item(0x82, b"", STC1, SS_NOPREFIX, 110, 18, 92, 9)
        .item(0x83, b"", LST2, LBS_NOTIFY | WS_VSCROLL | WS_BORDER | TAB, 110, 32, 92, 68)
        .label(b"List Files of &Type:", STC2, 6, 104, 90, 9)
        .item(0x85, b"", CMB1, CBS_DROPDOWNLIST | CBS_AUTOHSCROLL | WS_VSCROLL | TAB, 6, 114, 90, 36)
        .label(b"Dri&ves:", STC4, 110, 104, 92, 9)
        .item(0x85, b"", CMB2, CBS_DROPDOWNLIST | CBS_AUTOHSCROLL | WS_VSCROLL | TAB, 110, 114, 92, 68)
        .button(b"OK", IDOK, BS_DEFPUSHBUTTON | GROUP, 208, 6, 50, 14)
        .button(b"Cancel", IDCANCEL, GROUP, 208, 24, 50, 14)
        .button(b"&Help", PSH_HELP, GROUP, 208, 46, 50, 14)
        .button(b"&Read Only", CHX1, BS_AUTOCHECKBOX | GROUP, 208, 68, 50, 12);
    t.done()
}

/// GetOpenFileName, GetSaveFileName.
fn get_file_name(st: u32, save: bool, wide: bool) -> bool {
    *ERROR.get() = 0;
    if st == 0 || rd32(st, 0) < 76 {
        *ERROR.get() = CDERR_STRUCTSIZE;
        return false;
    }
    let fl = rd32(st, OFN_FLAGS);
    // An Explorer-style dialog's template and hook are for a child
    // dialog of the Explorer one: not used.
    let explorer = fl & OFN_EXPLORER != 0;
    let hinst = rd32(st, OFN_HINSTANCE);
    let Some(t) = template(fl & OFN_ENABLETEMPLATE != 0 && !explorer, fl & OFN_ENABLETEMPLATEHANDLE != 0 && !explorer, hinst, rd32(st, OFN_TEMPLATENAME), wide, file_template(save)) else { return false };
    let hook = if fl & OFN_ENABLEHOOK != 0 && !explorer { rd32(st, OFN_HOOK) } else { 0 };
    let start = cur_dir();
    let cd = Cd { kind: Kind::File(File { save, start: start.clone(), ..Default::default() }), st, hook, wide };
    let r = run(&t, rd32(st, OFN_HWNDOWNER), hinst, cd);
    if fl & OFN_NOCHANGEDIR != 0 {
        set_dir(&start);
    }
    r == 1
}

fn file(h: u16) -> Option<&'static mut File> {
    match &mut state(h)?.kind {
        Kind::File(f) => Some(f),
        _ => None,
    }
}

/// The process's current directory ("c:\dir").
pub(crate) fn cur_dir() -> Vec<u8> {
    let (mut d, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
    let mut buf = [0u8; 260];
    let mut cb = 260u32;
    unsafe { os2::DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut cb) };
    let mut p = vec![b'a' + d.max(1) as u8 - 1, b':', b'\\'];
    p.extend_from_slice(unsafe { crate::cstr(buf.as_ptr()) });
    p.make_ascii_lowercase();
    p
}

/// Make `dir` the current drive and directory.
pub(crate) fn set_dir(dir: &[u8]) -> bool {
    if dir.len() < 2 || dir[1] != b':' {
        return false;
    }
    let drive = dir[0].to_ascii_lowercase().wrapping_sub(b'a') as u32 + 1;
    if unsafe { os2::DosSetDefaultDisk(drive) } != 0 {
        return false;
    }
    let z = crate::z(dir);
    unsafe { os2::DosSetCurrentDir(z.as_ptr()) == 0 }
}

/// `dir` and `name` joined.
fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(name);
    p
}

/// The file's attributes, if it exists.
fn attrs(path: &[u8]) -> Option<u32> {
    let z = crate::z(path);
    let mut buf = [0u8; 24];
    // FIL_STANDARD: attrFile at 20.
    (unsafe { os2::DosQueryPathInfo(z.as_ptr(), 1, buf.as_mut_ptr(), 24) } == 0).then(|| u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]))
}

pub(crate) fn is_dir(path: &[u8]) -> bool {
    path.len() == 3 && path[1] == b':' || attrs(path).is_some_and(|a| a & 0x10 != 0)
}

/// The names in `dir` matching `spec` (directories when `dirs`).
fn list(dir: &[u8], spec: &[u8], dirs: bool) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let z = crate::z(&join(dir, spec));
    let mut hdir = 0xffff_ffffu32;
    let mut buf = vec![0u8; 2048];
    let mut n = 64u32;
    // FILE_ARCHIVED | FILE_READONLY (| FILE_DIRECTORY).
    let attr = 0x21 | if dirs { 0x10 } else { 0 };
    let mut rc = unsafe { os2::DosFindFirst(z.as_ptr(), &mut hdir, attr, buf.as_mut_ptr(), buf.len() as u32, &mut n, 1) };
    while rc == 0 && n > 0 {
        let mut off = 0usize;
        for _ in 0..n {
            let rd = |o: usize| u32::from_le_bytes(buf[off + o..off + o + 4].try_into().unwrap());
            let (next, a, len) = (rd(0), rd(24), buf[off + 28] as usize);
            let name = &buf[off + 29..off + 29 + len];
            if (a & 0x10 != 0) == dirs && name != b"." && name != b".." {
                out.push(name.to_ascii_lowercase());
            }
            if next == 0 {
                break;
            }
            off += next as usize;
        }
        n = 64;
        rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    }
    unsafe { os2::DosFindClose(hdir) };
    out.sort();
    out.dedup();
    out
}

fn has_wild(s: &[u8]) -> bool {
    s.iter().any(|&c| c == b'*' || c == b'?')
}

/// The file name part of a path.
fn title_of(p: &[u8]) -> Vec<u8> {
    p.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(p).to_vec()
}

fn file_init(h: u16) {
    let Some(st) = state(h).map(|s| s.st) else { return };
    let w = wide(h);
    let fl = rd32(st, OFN_FLAGS);
    if let Some(t) = get_str(rd32(st, OFN_TITLE), w).filter(|t| !t.is_empty()) {
        send_str(h, W_SETTEXT, 0, &t);
    }
    if fl & OFN_HIDEREADONLY != 0 {
        show(h, CHX1, false);
    }
    check(h, CHX1, fl & OFN_READONLY != 0);
    if fl & OFN_SHOWHELP == 0 {
        show(h, PSH_HELP, false);
    }
    send(get_item(h, EDT1), EM_LIMITTEXT, 255, 0);
    // The filters: pairs of strings, the list ending with an empty one.
    let mut filters = Vec::new();
    let cmb = get_item(h, CMB1);
    let list = str_list(rd32(st, OFN_FILTER), w);
    for pair in list.chunks(2) {
        send_str(cmb, CB_ADDSTRING, 0, &pair[0]);
        filters.push(pair.get(1).cloned().unwrap_or_else(|| b"*.*".to_vec()));
    }
    let idx = (rd32(st, OFN_FILTERINDEX).max(1) - 1) as usize;
    let idx = if idx < filters.len() { idx } else { 0 };
    send(cmb, CB_SETCURSEL, idx as u32, 0);
    let pattern = filters.get(idx).cloned().unwrap_or_else(|| b"*.*".to_vec());
    // The drives.
    let (mut d, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut d, &mut map) };
    let drives = get_item(h, CMB2);
    for i in 0..26 {
        if map & (1 << i) != 0 {
            let mut s = vec![b'a' + i as u8, b':'];
            if i >= 2 {
                if let Some(l) = volume_label(i + 1) {
                    s.extend_from_slice(b" [");
                    s.extend_from_slice(&l.to_ascii_lowercase());
                    s.push(b']');
                }
            }
            send_str(drives, CB_ADDSTRING, 0, &s);
        }
    }
    let initial = get_str(rd32(st, OFN_INITIALDIR), w).filter(|d| !d.is_empty()).map(|d| d.to_ascii_lowercase());
    let file_given = get_str(rd32(st, OFN_FILE), w).unwrap_or_default();
    let dir = match initial {
        Some(d) if is_dir(&d) => d,
        _ => file(h).map_or_else(cur_dir, |f| f.start.clone()),
    };
    if let Some(f) = file(h) {
        f.filters = filters;
        f.pattern = pattern.clone();
    }
    change_dir(h, &dir);
    set_text(h, EDT1, if file_given.is_empty() { &pattern } else { &file_given });
}

fn volume_label(drive: u32) -> Option<Vec<u8>> {
    let mut buf = [0u8; 64];
    // FSIL_VOLSER: ulVSN, then the label (a length and the characters).
    if unsafe { os2::DosQueryFSInfo(drive, 2, buf.as_mut_ptr(), 64) } != 0 {
        return None;
    }
    let n = (buf[4] as usize).min(32);
    (n > 0).then(|| buf[5..5 + n].to_vec())
}

/// Show directory `dir`: the current directory, the lists.
fn change_dir(h: u16, dir: &[u8]) -> bool {
    let mut d = dir.to_ascii_lowercase();
    if d.len() > 3 && d.ends_with(b"\\") {
        d.pop();
    }
    if !is_dir(&d) {
        return false;
    }
    set_dir(&d);
    let Some(f) = file(h) else { return false };
    f.dir = d.clone();
    let pattern = f.pattern.clone();
    set_text(h, STC1, &d);
    // The directory list: the path's parts, then the subdirectories,
    // each level indented.
    let lst = get_item(h, LST2);
    send(lst, LB_RESETCONTENT, 0, 0);
    let mut dirs = Vec::new();
    let mut labels = Vec::new();
    let parts: Vec<&[u8]> = d[3..].split(|&c| c == b'\\').filter(|p| !p.is_empty()).collect();
    let mut path = d[..3].to_vec();
    send_str(lst, LB_ADDSTRING, 0, &path);
    dirs.push(path.clone());
    labels.push(path.clone());
    for (i, p) in parts.iter().enumerate() {
        path = join(&path, p);
        let mut s = vec![b' '; 2 * (i + 1)];
        s.extend_from_slice(p);
        send_str(lst, LB_ADDSTRING, 0, &s);
        dirs.push(path.clone());
        labels.push(s);
    }
    let cur = dirs.len() - 1;
    for sub in list(&d, b"*", true) {
        let mut s = vec![b' '; 2 * (parts.len() + 1)];
        s.extend_from_slice(&sub);
        send_str(lst, LB_ADDSTRING, 0, &s);
        dirs.push(join(&d, &sub));
        labels.push(s);
    }
    send(lst, LB_SETCURSEL, cur as u32, 0);
    if let Some(f) = file(h) {
        f.dirs = dirs;
        f.labels = labels;
    }
    // The drive.
    let drives = get_item(h, CMB2);
    for i in 0..26u32 {
        let t = item_text(drives, CB_GETLBTEXT, i);
        if t.is_empty() {
            break;
        }
        if t[0] == d[0] {
            send(drives, CB_SETCURSEL, i, 0);
        }
    }
    fill_files(h, &d, &pattern);
    true
}

fn fill_files(h: u16, dir: &[u8], pattern: &[u8]) {
    let lst = get_item(h, LST1);
    send(lst, LB_RESETCONTENT, 0, 0);
    let mut names = Vec::new();
    for spec in pattern.split(|&c| c == b';') {
        let spec = spec.trim_ascii();
        if !spec.is_empty() {
            names.extend(list(dir, spec, false));
        }
    }
    names.sort();
    names.dedup();
    for n in &names {
        send_str(lst, LB_ADDSTRING, 0, n);
    }
    if let Some(f) = file(h) {
        f.files = names;
    }
}

/// WM_DRAWITEM for an owner-drawn file or directory list (a program's
/// template, as Windows 3.1's own): the name, a folder for directories.
fn draw_file_item(h: u16, p: u32) {
    let rd = |i: u32| crate::rd32(p, 4 * i);
    let (id, item, action, state, hdc) = (rd(1) as u16, rd(2), rd(3), rd(4), rd(6) as u16);
    let r = [rd(7) as i32, rd(8) as i32, rd(9) as i32, rd(10) as i32];
    // ODA_FOCUS alone: the focus rectangle.
    if action == 4 {
        gdi::focus_rect(hdc, r);
        return;
    }
    let Some(f) = file(h) else { return };
    let text = match id {
        LST1 => f.files.get(item as usize).cloned(),
        _ => f.labels.get(item as usize).cloned(),
    };
    let sel = state & 1 != 0;
    let (bg, fg) = if sel { (gdi::COLOR_HIGHLIGHT, gdi::COLOR_HIGHLIGHTTEXT) } else { (gdi::COLOR_WINDOW, gdi::COLOR_WINDOWTEXT) };
    gdi::fill(hdc, r, gdi::sys_color(bg));
    let Some(t) = text else { return };
    let indent = t.iter().take_while(|&&c| c == b' ').count() as i32;
    let name = t.trim_ascii();
    let mut x = r[0] + 2;
    let mid = (r[1] + r[3]) / 2;
    if id == LST2 {
        // A small folder: open for the path, closed for subdirectories.
        x += indent * 4;
        let (fx, fy) = (x, mid - 5);
        gdi::fill(hdc, [fx, fy, fx + 6, fy + 2], 0x00c0c0);
        gdi::fill(hdc, [fx, fy + 2, fx + 14, fy + 10], 0x00ffff);
        gdi::frame(hdc, [fx, fy + 2, fx + 14, fy + 10], 0x008080);
        x += 18;
    }
    gdi::set_text_color(hdc, gdi::sys_color(fg));
    gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
    gdi::text_at(hdc, x, mid - gdi::line_height(hdc) / 2, name);
    if state & 0x10 != 0 {
        gdi::focus_rect(hdc, r);
    }
}

fn file_msg(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    if m == W_DRAWITEM && (wp == LST1 as u32 || wp == LST2 as u32) {
        draw_file_item(h, lp);
        return 1;
    }
    if m != W_COMMAND {
        return 0;
    }
    let code = (wp >> 16) as u16;
    match wp as u16 {
        IDOK => {
            file_ok(h);
            1
        }
        IDCANCEL => {
            end(h, 0);
            1
        }
        LST1 if code == LBN_SELCHANGE || code == LBN_DBLCLK => {
            let c = get_item(h, LST1);
            let i = send(c, LB_GETCURSEL, 0, 0);
            let t = file(h).and_then(|f| f.files.get(i as usize).cloned()).unwrap_or_default();
            if !t.is_empty() {
                set_text(h, EDT1, &t);
                if code == LBN_DBLCLK {
                    file_ok(h);
                }
            }
            1
        }
        LST2 if code == LBN_DBLCLK => {
            open_dir_item(h);
            1
        }
        CMB1 if code == CBN_SELCHANGE => {
            let i = send(get_item(h, CMB1), CB_GETCURSEL, 0, 0) as usize;
            let Some(f) = file(h) else { return 1 };
            if let Some(p) = f.filters.get(i).cloned() {
                f.pattern = p.clone();
                let d = f.dir.clone();
                let typed = text(h, EDT1);
                if typed.is_empty() || has_wild(&typed) {
                    set_text(h, EDT1, &p);
                }
                fill_files(h, &d, &p);
            }
            1
        }
        CMB2 if code == CBN_SELCHANGE => {
            let c = get_item(h, CMB2);
            let i = send(c, CB_GETCURSEL, 0, 0);
            let t = item_text(c, CB_GETLBTEXT, i);
            if let Some(&l) = t.first() {
                // The drive's current directory.
                let mut buf = [0u8; 260];
                let mut cb = 260u32;
                let drive = (l - b'a') as u32 + 1;
                let mut d = vec![l, b':', b'\\'];
                if unsafe { os2::DosQueryCurrentDir(drive, buf.as_mut_ptr(), &mut cb) } == 0 {
                    d.extend_from_slice(unsafe { crate::cstr(buf.as_ptr()) });
                }
                if !change_dir(h, &d) {
                    message(h, b"The drive is not ready.", 0x30);
                }
            }
            1
        }
        _ => 0,
    }
}

/// Open the directory list's selected item.
fn open_dir_item(h: u16) {
    let i = send(get_item(h, LST2), LB_GETCURSEL, 0, 0) as usize;
    if let Some(d) = file(h).and_then(|f| f.dirs.get(i).cloned()) {
        change_dir(h, &d);
    }
}

fn file_ok(h: u16) {
    let Some(cd) = state(h) else { return };
    let (st, w) = (cd.st, cd.wide);
    let Some(f) = file(h) else { return };
    let (save, dir) = (f.save, f.dir.clone());
    // Enter in the directory list opens its directory.
    if focus() == get_item(h, LST2) {
        open_dir_item(h);
        return;
    }
    let typed = text(h, EDT1).trim_ascii().to_ascii_lowercase();
    if typed.is_empty() {
        return;
    }
    // A full path, or one relative to the directory shown.
    let path = if typed.len() >= 2 && typed[1] == b':' {
        if typed.len() == 2 { join(&typed, b"") } else { typed.clone() }
    } else if typed.starts_with(b"\\") {
        let mut p = dir[..2].to_vec();
        p.extend_from_slice(&typed);
        p
    } else {
        join(&dir, &typed)
    };
    let path = normalize(&path);
    if has_wild(&path) {
        // A new pattern (in another directory, perhaps).
        let (d, spec) = split_path(&path);
        if let Some(f) = file(h) {
            f.pattern = spec.clone();
        }
        if !change_dir(h, &d) {
            message(h, b"Path not found.\nPlease verify the correct path was given.", 0x30);
        }
        set_text(h, EDT1, &spec);
        return;
    }
    if is_dir(&path) {
        let p = file(h).map_or_else(|| b"*.*".to_vec(), |f| f.pattern.clone());
        change_dir(h, &path);
        set_text(h, EDT1, &p);
        return;
    }
    let fl = rd32(st, OFN_FLAGS);
    let mut path = path;
    let title = title_of(&path);
    if !title.contains(&b'.') {
        if let Some(ext) = get_str(rd32(st, OFN_DEFEXT), w).filter(|e| !e.is_empty()) {
            path.push(b'.');
            path.extend_from_slice(&ext.to_ascii_lowercase());
        }
    }
    let title = title_of(&path);
    if title.is_empty() || title.len() > 255 || title.iter().any(|&c| b"\"<>|".contains(&c) || c < b' ') {
        let mut t = path.clone();
        t.extend_from_slice(b"\nThe filename is not valid.");
        message(h, &t, 0x30);
        return;
    }
    let (d, _) = split_path(&path);
    let exists = attrs(&path).is_some();
    if (fl & (OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST) != 0 || save) && !is_dir(&d) {
        let mut t = path.clone();
        t.extend_from_slice(b"\nPath does not exist.\nPlease verify the correct path was given.");
        message(h, &t, 0x30);
        return;
    }
    if !exists && fl & OFN_FILEMUSTEXIST != 0 {
        let mut t = path.clone();
        t.extend_from_slice(b"\nFile not found.\nPlease verify the correct filename was given.");
        message(h, &t, 0x30);
        return;
    }
    if !exists && fl & OFN_CREATEPROMPT != 0 {
        let mut t = path.clone();
        t.extend_from_slice(b"\nThis file does not exist.\nCreate the file?");
        // MB_YESNO | MB_ICONQUESTION: IDYES 6.
        if message(h, &t, 0x24) != 6 {
            return;
        }
    }
    if exists && save && fl & OFN_OVERWRITEPROMPT != 0 {
        let mut t = path.clone();
        t.extend_from_slice(b" already exists.\nDo you want to replace it?");
        if message(h, &t, 0x34) != 6 {
            return;
        }
    }
    let ro = checked(h, CHX1);
    if exists && fl & OFN_NOREADONLYRETURN != 0 && attrs(&path).is_some_and(|a| a & 1 != 0) {
        let mut t = path.clone();
        t.extend_from_slice(b"\nThis file is read-only.");
        message(h, &t, 0x30);
        return;
    }
    // The results: offsets in the program's characters.
    let buf = rd32(st, OFN_FILE);
    let max = rd32(st, OFN_MAXFILE) as usize;
    let plen = units(&path, w);
    if buf != 0 && max < plen + 1 {
        *ERROR.get() = FNERR_BUFFERTOOSMALL;
        if max >= 2 {
            wr16(buf, 0, plen as u16 + 1);
        }
        end(h, 0);
        return;
    }
    if buf == 0 {
        *ERROR.get() = FNERR_INVALIDFILENAME;
        end(h, 0);
        return;
    }
    put_str(buf, max as u32, &path, w);
    // A multiple selection's list ends with an empty string.
    if fl & OFN_ALLOWMULTISELECT != 0 && max > plen + 1 {
        if w {
            wr16(buf, 2 * (plen + 1), 0);
        } else {
            unsafe { *((buf as usize + plen + 1) as *mut u8) = 0 };
        }
    }
    let off = path.len() - title.len();
    wr16(st, OFN_FILEOFFSET, units(&path[..off], w) as u16);
    let ext = title.iter().rposition(|&c| c == b'.').map_or(path.len(), |i| off + i + 1);
    wr16(st, OFN_FILEEXTENSION, units(&path[..ext.min(path.len())], w) as u16);
    let mut fl = fl & !(OFN_READONLY | OFN_EXTENSIONDIFFERENT);
    if ro {
        fl |= OFN_READONLY;
    }
    if let Some(defext) = get_str(rd32(st, OFN_DEFEXT), w) {
        if !path[ext.min(path.len())..].eq_ignore_ascii_case(&defext) {
            fl |= OFN_EXTENSIONDIFFERENT;
        }
    }
    wr32(st, OFN_FLAGS, fl);
    let i = send(get_item(h, CMB1), CB_GETCURSEL, 0, 0);
    if i as i32 >= 0 {
        wr32(st, OFN_FILTERINDEX, i + 1);
    }
    let ft = rd32(st, OFN_FILETITLE);
    if ft != 0 {
        put_str(ft, rd32(st, OFN_MAXFILETITLE), &title, w);
    }
    // The hook can refuse the name (FILEOKSTRING).
    if state(h).is_some_and(|s| s.hook != 0) {
        let m = crate::user32::register_message(b"commdlg_FileNameOK") as u16;
        if call_hook(h, m, 0, st) != 0 {
            return;
        }
    }
    end(h, 1);
}

/// "a\b\..\c" -> "a\c".
fn normalize(p: &[u8]) -> Vec<u8> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut parts: Vec<&[u8]> = Vec::new();
    for s in p[3..].split(|&c| c == b'\\') {
        match s {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            _ => parts.push(s),
        }
    }
    let mut out = p[..3].to_vec();
    for (i, s) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'\\');
        }
        out.extend_from_slice(s);
    }
    out
}

/// (directory, name) of a path.
fn split_path(p: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match p.iter().rposition(|&c| c == b'\\') {
        Some(i) if i <= 2 => (p[..i + 1].to_vec(), p[i + 1..].to_vec()),
        Some(i) => (p[..i].to_vec(), p[i + 1..].to_vec()),
        None => (p.to_vec(), Vec::new()),
    }
}

// ---- Color ----

struct Color {
    rgb: u32,
    cust: [u32; 16],
    /// The custom colour "Add to Custom Colors" replaces next.
    next_cust: usize,
    full: bool,
    /// Edits being filled (their EN_CHANGE is ours).
    filling: bool,
}

// CHOOSECOLOR offsets.
// CHOOSECOLOR offsets.
const CC_HWNDOWNER: usize = 4;
const CC_HINSTANCE: usize = 8;
const CC_RGBRESULT: usize = 12;
const CC_CUSTCOLORS: usize = 16;
const CC_FLAGS: usize = 20;
const CC_HOOK: usize = 28;
const CC_TEMPLATENAME: usize = 32;

const CC_RGBINIT: u32 = 0x1;
const CC_FULLOPEN: u32 = 0x2;
const CC_PREVENTFULLOPEN: u32 = 0x4;
const CC_SHOWHELP: u32 = 0x8;
const CC_ENABLEHOOK: u32 = 0x10;
const CC_ENABLETEMPLATE: u32 = 0x20;
const CC_ENABLETEMPLATEHANDLE: u32 = 0x40;

/// Windows 3.1's basic colours (COLORREFs).
const BASIC: [u32; 48] = [
    0x8080ff, 0x80ffff, 0x80ff80, 0x80ff00, 0xffff80, 0xff8000, 0xc080ff, 0xff80ff,
    0x0000ff, 0x00ffff, 0x00ff80, 0x40ff00, 0xffff00, 0xc08000, 0xc08080, 0xff00ff,
    0x404080, 0x4080ff, 0x00ff00, 0x808000, 0x804000, 0xff8080, 0x400080, 0x8000ff,
    0x000080, 0x0080ff, 0x008000, 0x408000, 0xff0000, 0xa00000, 0x800080, 0xff0080,
    0x000040, 0x004080, 0x004000, 0x404000, 0x800000, 0x400000, 0x400040, 0x800040,
    0x000000, 0x008080, 0x408080, 0x808080, 0x808040, 0xc0c0c0, 0x400040, 0xffffff,
];

/// The spectrum (hue across, saturation up) and the luminosity bar, in
/// dialog units of the full dialog.
const RAINBOW: [i32; 4] = [152, 4, 270, 120];
const LUMBAR: [i32; 4] = [280, 4, 288, 120];
const CURRENT: [i32; 4] = [152, 124, 192, 150];

fn color_template(full: bool) -> Vec<u8> {
    let mut t = Tpl::new(DLG, 2, 0, if full { 298 } else { 147 }, 186, b"Color");
    t.label(b"&Basic Colors:", 0xffff, 4, 4, 140, 9);
    for i in 0..48u16 {
        let (x, y) = (4 + (i % 8) as i16 * 17, 14 + (i / 8) as i16 * 13);
        t.item(0x80, b"", COLOR_BASIC + i, BS_OWNERDRAW | if i == 0 { GROUP | TAB } else { 0 }, x, y, 15, 11);
    }
    t.label(b"&Custom Colors:", 0xffff, 4, 96, 140, 9);
    for i in 0..16u16 {
        let (x, y) = (4 + (i % 8) as i16 * 17, 106 + (i / 8) as i16 * 13);
        t.item(0x80, b"", COLOR_CUST + i, BS_OWNERDRAW | if i == 0 { GROUP | TAB } else { 0 }, x, y, 15, 11);
    }
    t.button(b"&Define Custom Colors...", COLOR_MIX, GROUP, 4, 136, 138, 14)
        .button(b"OK", IDOK, BS_DEFPUSHBUTTON | GROUP, 4, 166, 44, 14)
        .button(b"Cancel", IDCANCEL, GROUP, 52, 166, 44, 14)
        .button(b"&Help", PSH_HELP, GROUP, 100, 166, 44, 14);
    t.label(b"Color|Sol&id", 0xffff, 152, 152, 40, 9);
    for (i, (name, id)) in [(b"Hu&e:" as &[u8], COLOR_HUE), (b"&Sat:", COLOR_SAT), (b"&Lum:", COLOR_LUM)].into_iter().enumerate() {
        let y = 126 + i as i16 * 14;
        t.item(0x82, name, 0xffff, SS_RIGHT, 194, y + 2, 20, 9).edit(id, 216, y, 18, 12);
    }
    for (i, (name, id)) in [(b"&Red:" as &[u8], COLOR_RED), (b"&Green:", COLOR_GREEN), (b"Bl&ue:", COLOR_BLUE)].into_iter().enumerate() {
        let y = 126 + i as i16 * 14;
        t.item(0x82, name, 0xffff, SS_RIGHT, 238, y + 2, 32, 9).edit(id, 272, y, 18, 12);
    }
    t.button(b"&Add to Custom Colors", COLOR_ADD, GROUP, 152, 168, 142, 14);
    t.done()
}

/// ChooseColor.
fn choose_color(st: u32, wide: bool) -> bool {
    *ERROR.get() = 0;
    if st == 0 || rd32(st, 0) < 36 {
        *ERROR.get() = CDERR_STRUCTSIZE;
        return false;
    }
    let fl = rd32(st, CC_FLAGS);
    let full = fl & CC_FULLOPEN != 0 && fl & CC_PREVENTFULLOPEN == 0;
    let Some(t) = template(fl & CC_ENABLETEMPLATE != 0, fl & CC_ENABLETEMPLATEHANDLE != 0, rd32(st, CC_HINSTANCE), rd32(st, CC_TEMPLATENAME), wide, color_template(full)) else { return false };
    let mut cust = [0xffffffu32; 16];
    let cp = rd32(st, CC_CUSTCOLORS);
    if cp != 0 {
        for (i, c) in cust.iter_mut().enumerate() {
            *c = rd32(cp, i * 4) & 0xffffff;
        }
    }
    let rgb = if fl & CC_RGBINIT != 0 { rd32(st, CC_RGBRESULT) & 0xffffff } else { 0 };
    let hook = if fl & CC_ENABLEHOOK != 0 { rd32(st, CC_HOOK) } else { 0 };
    let cd = Cd { kind: Kind::Color(Color { rgb, cust, next_cust: 0, full, filling: false }), st, hook, wide };
    run(&t, rd32(st, CC_HWNDOWNER), rd32(st, CC_HINSTANCE), cd) == 1
}

fn color(h: u16) -> Option<&'static mut Color> {
    match &mut state(h)?.kind {
        Kind::Color(c) => Some(c),
        _ => None,
    }
}

fn color_init(h: u16) {
    let Some(st) = state(h).map(|s| s.st) else { return };
    let fl = rd32(st, CC_FLAGS);
    if fl & CC_SHOWHELP == 0 {
        show(h, PSH_HELP, false);
    }
    if fl & CC_PREVENTFULLOPEN != 0 || color(h).is_some_and(|c| c.full) {
        enable(h, COLOR_MIX, false);
    }
    for id in [COLOR_HUE, COLOR_SAT, COLOR_LUM, COLOR_RED, COLOR_GREEN, COLOR_BLUE] {
        send(get_item(h, id), EM_LIMITTEXT, 3, 0);
    }
    let rgb = color(h).map_or(0, |c| c.rgb);
    // The basic colour of the initial colour gets the focus.
    if let Some(i) = BASIC.iter().position(|&b| b == rgb) {
        crate::dialog::focus_control(h, get_item(h, COLOR_BASIC + i as u16));
    }
    fill_color_edits(h);
}

/// Windows' hue, saturation and luminosity (0-240) of a COLORREF.
fn rgb_to_hls(c: u32) -> (i32, i32, i32) {
    let (r, g, b) = ((c & 0xff) as i32, (c >> 8 & 0xff) as i32, (c >> 16 & 0xff) as i32);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = ((max + min) * 240 + 255) / 510;
    if max == min {
        return (160, 0, l);
    }
    let d = max - min;
    let s = if l <= 120 { (d * 240 + (max + min) / 2) / (max + min) } else { (d * 240 + (510 - max - min) / 2) / (510 - max - min) };
    let h6 = if r == max {
        (g - b) * 40 / d
    } else if g == max {
        80 + (b - r) * 40 / d
    } else {
        160 + (r - g) * 40 / d
    };
    (h6.rem_euclid(240), s.min(240), l.min(240))
}

fn hls_to_rgb(h: i32, l: i32, s: i32) -> u32 {
    if s == 0 {
        let v = (l * 255 / 240) as u32;
        return v | v << 8 | v << 16;
    }
    let m2 = if l <= 120 { l * (240 + s) / 240 } else { l + s - l * s / 240 };
    let m1 = 2 * l - m2;
    let part = |h: i32| {
        let h = h.rem_euclid(240);
        let v = if h < 40 {
            m1 + (m2 - m1) * h / 40
        } else if h < 120 {
            m2
        } else if h < 160 {
            m1 + (m2 - m1) * (160 - h) / 40
        } else {
            m1
        };
        ((v * 255 + 120) / 240).clamp(0, 255) as u32
    };
    part(h + 80) | part(h) << 8 | part(h - 80) << 16
}

fn fill_color_edits(h: u16) {
    let Some(c) = color(h) else { return };
    let rgb = c.rgb;
    c.filling = true;
    let (hu, s, l) = rgb_to_hls(rgb);
    for (id, v) in [(COLOR_HUE, hu), (COLOR_SAT, s), (COLOR_LUM, l), (COLOR_RED, (rgb & 0xff) as i32), (COLOR_GREEN, (rgb >> 8 & 0xff) as i32), (COLOR_BLUE, (rgb >> 16 & 0xff) as i32)] {
        set_text(h, id, alloc::format!("{v}").as_bytes());
    }
    if let Some(c) = color(h) {
        c.filling = false;
    }
    invalidate(h, None);
}

fn edit_value(h: u16, id: u16) -> i32 {
    let t = text(h, id);
    t.iter().take_while(|c| c.is_ascii_digit()).fold(0i32, |v, &c| v * 10 + (c - b'0') as i32)
}

/// Pixels of a rectangle in dialog units.
fn dlu_rect(h: u16, r: [i32; 4]) -> [i32; 4] {
    let (bx, by) = crate::dialog::base_units_of(h);
    [r[0] * bx / 4, r[1] * by / 8, r[2] * bx / 4, r[3] * by / 8]
}

fn color_msg(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    match m {
        W_DRAWITEM => {
            // DRAWITEMSTRUCT: CtlType, CtlID, itemID, itemAction,
            // itemState, hwndItem, hDC, rcItem.
            let rd = |i: u32| crate::rd32(lp, 4 * i);
            let (id, state, hdc) = (rd(1) as u16, rd(4), rd(6) as u16);
            let r = [rd(7) as i32, rd(8) as i32, rd(9) as i32, rd(10) as i32];
            let Some(c) = color(h) else { return 0 };
            let col = if (COLOR_BASIC..COLOR_BASIC + 48).contains(&id) { BASIC[(id - COLOR_BASIC) as usize] } else { c.cust[((id - COLOR_CUST) & 15) as usize] };
            let sel = col == c.rgb;
            gdi::fill(hdc, r, gdi::sys_color(gdi::COLOR_BTNFACE));
            // A swatch; the selected one framed in black, the
            // focused one with a focus rectangle.
            gdi::fill(hdc, [r[0] + 2, r[1] + 2, r[2] - 2, r[3] - 2], col);
            gdi::frame(hdc, [r[0] + 1, r[1] + 1, r[2] - 1, r[3] - 1], 0x808080);
            if sel {
                gdi::frame(hdc, r, 0);
            }
            // ODS_FOCUS
            if state & 0x10 != 0 {
                gdi::focus_rect(hdc, r);
            }
            1
        }
        W_PAINT => {
            let full = color(h).is_some_and(|c| c.full);
            if !full {
                return 0;
            }
            let hdc = begin_paint_rs(h);
            let (cx, cy) = client_size(h);
            gdi::fill(hdc, [0, 0, cx, cy], gdi::sys_color(gdi::COLOR_BTNFACE));
            paint_spectrum(h, hdc);
            gdi::release_dc(hdc);
            1
        }
        W_LBUTTONDOWN => {
            let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
            let Some(c) = color(h) else { return 0 };
            if !c.full {
                return 0;
            }
            let rb = dlu_rect(h, RAINBOW);
            let lb = dlu_rect(h, LUMBAR);
            let (hu, s, l) = rgb_to_hls(c.rgb);
            if x >= rb[0] && x < rb[2] && y >= rb[1] && y < rb[3] {
                let nh = (x - rb[0]) * 239 / (rb[2] - rb[0] - 1).max(1);
                let ns = 240 - (y - rb[1]) * 240 / (rb[3] - rb[1] - 1).max(1);
                c.rgb = hls_to_rgb(nh, if l == 0 || l == 240 { 120 } else { l }, ns.clamp(0, 240));
            } else if x >= lb[0] && x < lb[2] + 6 && y >= lb[1] && y < lb[3] {
                let nl = 240 - (y - lb[1]) * 240 / (lb[3] - lb[1] - 1).max(1);
                c.rgb = hls_to_rgb(hu, nl.clamp(0, 240), s);
            } else {
                return 0;
            }
            fill_color_edits(h);
            1
        }
        W_COMMAND => {
            let code = (wp >> 16) as u16;
            let wp = wp as u16;
            match wp {
                IDOK => {
                    let Some((st, rgb, cust)) = state(h).and_then(|s| color(h).map(|c| (s.st, c.rgb, c.cust))) else { return 1 };
                    wr32(st, CC_RGBRESULT, rgb);
                    let cp = rd32(st, CC_CUSTCOLORS);
                    if cp != 0 {
                        for (i, c) in cust.iter().enumerate() {
                            wr32(cp, i * 4, *c);
                        }
                    }
                    if call_hook(h, crate::user32::register_message(b"commdlg_ColorOK") as u16, 0, st) != 0 {
                        return 1;
                    }
                    end(h, 1);
                    1
                }
                IDCANCEL => {
                    end(h, 0);
                    1
                }
                COLOR_MIX => {
                    // Open the custom colour part.
                    if let Some(c) = color(h) {
                        c.full = true;
                    }
                    let r = window_rect(h);
                    let (bx, _) = crate::dialog::base_units_of(h);
                    move_to(h, [r[0], r[1], r[2] + 151 * bx / 4, r[3]]);
                    enable(h, COLOR_MIX, false);
                    crate::dialog::focus_control(h, get_item(h, COLOR_HUE));
                    invalidate(h, None);
                    1
                }
                COLOR_ADD => {
                    if let Some(c) = color(h) {
                        let i = c.next_cust;
                        c.cust[i] = c.rgb;
                        c.next_cust = (i + 1) % 16;
                        invalidate(get_item(h, COLOR_CUST + i as u16), None);
                    }
                    1
                }
                COLOR_HUE..=COLOR_BLUE if code == EN_CHANGE => {
                    let Some(c) = color(h) else { return 1 };
                    if c.filling {
                        return 1;
                    }
                    let v = edit_value(h, wp);
                    let (mut hu, mut s, mut l) = rgb_to_hls(c.rgb);
                    let mut rgb = c.rgb;
                    match wp {
                        COLOR_HUE => hu = v.min(239),
                        COLOR_SAT => s = v.min(240),
                        COLOR_LUM => l = v.min(240),
                        COLOR_RED => rgb = rgb & !0xff | v.min(255) as u32,
                        COLOR_GREEN => rgb = rgb & !0xff00 | (v.min(255) as u32) << 8,
                        _ => rgb = rgb & !0xff0000 | (v.min(255) as u32) << 16,
                    }
                    c.rgb = if wp <= COLOR_LUM { hls_to_rgb(hu, l, s) } else { rgb };
                    // Update the other edits (not the one being typed in).
                    c.filling = true;
                    let rgb = c.rgb;
                    let (hu, s, l) = rgb_to_hls(rgb);
                    for (id, v) in [(COLOR_HUE, hu), (COLOR_SAT, s), (COLOR_LUM, l), (COLOR_RED, (rgb & 0xff) as i32), (COLOR_GREEN, (rgb >> 8 & 0xff) as i32), (COLOR_BLUE, (rgb >> 16 & 0xff) as i32)] {
                        if id != wp && (id <= COLOR_LUM) != (wp <= COLOR_LUM) {
                            set_text(h, id, alloc::format!("{v}").as_bytes());
                        }
                    }
                    if let Some(c) = color(h) {
                        c.filling = false;
                    }
                    let r = dlu_rect(h, [RAINBOW[0], RAINBOW[1], LUMBAR[2] + 8, CURRENT[3]]);
                    invalidate(h, Some(r));
                    redraw_swatches(h);
                    1
                }
                id if code == BN_CLICKED && (COLOR_BASIC..COLOR_CUST + 16).contains(&id) => {
                    let Some(c) = color(h) else { return 1 };
                    c.rgb = if id < COLOR_BASIC + 48 { BASIC[(id - COLOR_BASIC) as usize] } else { c.cust[(id - COLOR_CUST) as usize] };
                    if id >= COLOR_CUST {
                        c.next_cust = (id - COLOR_CUST) as usize;
                    }
                    fill_color_edits(h);
                    redraw_swatches(h);
                    1
                }
                _ => 0,
            }
        }
        _ => 0,
    }
}

fn redraw_swatches(h: u16) {
    for id in (COLOR_BASIC..COLOR_BASIC + 48).chain(COLOR_CUST..COLOR_CUST + 16) {
        invalidate(get_item(h, id), None);
    }
}

fn paint_spectrum(h: u16, hdc: u16) {
    let Some(c) = color(h) else { return };
    let rgb = c.rgb;
    let rb = dlu_rect(h, RAINBOW);
    let (hu, s, l) = rgb_to_hls(rgb);
    // The spectrum in blocks: hue across, saturation down.
    let (nx, ny) = (24, 16);
    let (w, hh) = (rb[2] - rb[0], rb[3] - rb[1]);
    for j in 0..ny {
        for i in 0..nx {
            let col = hls_to_rgb(i * 240 / nx, 120, 240 - j * 240 / ny);
            gdi::fill(hdc, [rb[0] + i * w / nx, rb[1] + j * hh / ny, rb[0] + (i + 1) * w / nx, rb[1] + (j + 1) * hh / ny], col);
        }
    }
    gdi::frame(hdc, [rb[0] - 1, rb[1] - 1, rb[2] + 1, rb[3] + 1], 0);
    // The cross hair at the colour's hue and saturation.
    let (cx, cy) = (rb[0] + hu * (w - 1) / 239, rb[1] + (240 - s) * (hh - 1) / 240);
    gdi::fill(hdc, [cx - 6, cy, cx - 2, cy + 1], 0);
    gdi::fill(hdc, [cx + 3, cy, cx + 7, cy + 1], 0);
    gdi::fill(hdc, [cx, cy - 6, cx + 1, cy - 2], 0);
    gdi::fill(hdc, [cx, cy + 3, cx + 1, cy + 7], 0);
    // The luminosity bar of the hue and saturation, and its arrow.
    let lb = dlu_rect(h, LUMBAR);
    let n = 32;
    let lh = lb[3] - lb[1];
    for j in 0..n {
        let col = hls_to_rgb(hu, 240 - j * 240 / (n - 1), s);
        gdi::fill(hdc, [lb[0], lb[1] + j * lh / n, lb[2], lb[1] + (j + 1) * lh / n], col);
    }
    gdi::frame(hdc, [lb[0] - 1, lb[1] - 1, lb[2] + 1, lb[3] + 1], 0);
    let ay = lb[1] + (240 - l) * (lh - 1) / 240;
    for k in 0..4 {
        gdi::fill(hdc, [lb[2] + 2 + k, ay - 3 + k, lb[2] + 3 + k, ay + 4 - k], 0);
    }
    // The colour.
    let cr = dlu_rect(h, CURRENT);
    gdi::fill(hdc, cr, rgb);
    gdi::frame(hdc, [cr[0] - 1, cr[1] - 1, cr[2] + 1, cr[3] + 1], 0);
}

// ---- Font ----

struct Font {
    face: Vec<u8>,
    bold: bool,
    italic: bool,
    size: i32,
    underline: bool,
    strike: bool,
    rgb: u32,
    /// The sample's font.
    sample: u16,
}

// CHOOSEFONT offsets.
// CHOOSEFONT offsets.
const CF_HWNDOWNER: usize = 4;
const CF_LOGFONT: usize = 12;
const CF_POINTSIZE: usize = 16;
const CF_FLAGS: usize = 20;
const CF_RGB: usize = 24;
const CF_HOOK: usize = 32;
const CF_TEMPLATENAME: usize = 36;
const CF_HINSTANCE: usize = 40;
const CF_STYLE: usize = 44;
const CF_FONTTYPE: usize = 48;
const CF_SIZEMIN: usize = 52;
const CF_SIZEMAX: usize = 56;

const CF_SHOWHELP: u32 = 0x4;
const CF_ENABLEHOOK: u32 = 0x8;
const CF_ENABLETEMPLATE: u32 = 0x10;
const CF_ENABLETEMPLATEHANDLE: u32 = 0x20;
const CF_INITTOLOGFONTSTRUCT: u32 = 0x40;
const CF_USESTYLE: u32 = 0x80;
const CF_EFFECTS: u32 = 0x100;
const CF_APPLY: u32 = 0x200;
const CF_LIMITSIZE: u32 = 0x2000;
const CF_FIXEDPITCHONLY: u32 = 0x4000;
const CF_NOFACESEL: u32 = 0x80000;
const CF_NOSTYLESEL: u32 = 0x100000;
const CF_NOSIZESEL: u32 = 0x200000;

/// The faces offered (Windows names CreateFont maps to rs2's fonts) and
/// their LOGFONT pitch and family.
const FACES: [(&[u8], u8); 6] = [(b"Arial", 0x22), (b"Courier New", 0x31), (b"Times New Roman", 0x12), (b"MS Sans Serif", 0x22), (b"System", 0x22), (b"Fixedsys", 0x31)];
const STYLES: [&[u8]; 4] = [b"Regular", b"Italic", b"Bold", b"Bold Italic"];
const SIZES: [i32; 16] = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];
/// The colours of the effects (Windows' 16 colours).
const COLORS: [(&[u8], u32); 16] = [
    (b"Black", 0x000000), (b"Maroon", 0x000080), (b"Green", 0x008000), (b"Olive", 0x008080),
    (b"Navy", 0x800000), (b"Purple", 0x800080), (b"Teal", 0x808000), (b"Gray", 0x808080),
    (b"Silver", 0xc0c0c0), (b"Red", 0x0000ff), (b"Lime", 0x00ff00), (b"Yellow", 0x00ffff),
    (b"Blue", 0xff0000), (b"Fuchsia", 0xff00ff), (b"Aqua", 0xffff00), (b"White", 0xffffff),
];

fn font_template() -> Vec<u8> {
    let simple = CBS_SIMPLE | CBS_AUTOHSCROLL | WS_VSCROLL | TAB;
    let mut t = Tpl::new(DLG, 13, 54, 264, 147, b"Font");
    t.label(b"&Font:", STC1, 6, 3, 40, 9)
        .item(0x85, b"", CMB1, simple | CBS_SORT, 6, 13, 94, 54)
        .label(b"Font St&yle:", STC2, 108, 3, 44, 9)
        .item(0x85, b"", CMB2, simple, 108, 13, 64, 54)
        .label(b"&Size:", STC3, 179, 3, 30, 9)
        .item(0x85, b"", CMB3, simple, 179, 13, 32, 54)
        .button(b"OK", IDOK, BS_DEFPUSHBUTTON | GROUP, 218, 6, 40, 14)
        .button(b"Cancel", IDCANCEL, GROUP, 218, 23, 40, 14)
        .button(b"&Apply", PSH3, GROUP, 218, 40, 40, 14)
        .button(b"&Help", PSH_HELP, GROUP, 218, 57, 40, 14)
        .button(b"Effects", GRP1, BS_GROUPBOX, 6, 72, 84, 34)
        .button(b"Stri&keout", CHX1, BS_AUTOCHECKBOX | GROUP, 10, 82, 49, 10)
        .button(b"&Underline", CHX2, BS_AUTOCHECKBOX, 10, 94, 51, 10)
        .label(b"&Color:", STC4, 6, 110, 30, 9)
        .item(0x85, b"", CMB4, CBS_DROPDOWNLIST | WS_VSCROLL | TAB, 6, 120, 84, 100)
        .button(b"Sample", GRP2, BS_GROUPBOX, 98, 72, 160, 49)
        .item(0x82, b"AaBbYyZz", STC5, 1 | SS_NOPREFIX, 104, 81, 149, 37);
    t.done()
}

/// ChooseFont: LOGFONT (A: the face at 28, 32 bytes; W: 32 UTF-16
/// characters).
fn choose_font(st: u32, wide: bool) -> bool {
    *ERROR.get() = 0;
    if st == 0 || rd32(st, 0) < 60 {
        *ERROR.get() = CDERR_STRUCTSIZE;
        return false;
    }
    let fl = rd32(st, CF_FLAGS);
    let Some(t) = template(fl & CF_ENABLETEMPLATE != 0, fl & CF_ENABLETEMPLATEHANDLE != 0, rd32(st, CF_HINSTANCE), rd32(st, CF_TEMPLATENAME), wide, font_template()) else { return false };
    let mut f = Font { face: b"Arial".to_vec(), bold: false, italic: false, size: 10, underline: false, strike: false, rgb: rd32(st, CF_RGB) & 0xffffff, sample: 0 };
    let lf = rd32(st, CF_LOGFONT);
    if fl & CF_INITTOLOGFONTSTRUCT != 0 && lf != 0 {
        let face = get_str(lf + 28, wide).unwrap_or_default();
        if !face.is_empty() {
            f.face = face;
        }
        let height = rd32(lf, 0) as i32;
        if height != 0 {
            let px = if height < 0 { -height } else { height * 5 / 6 };
            f.size = ((px * 72 + 48) / 96).max(1);
        }
        f.bold = rd32(lf, 16) >= 600;
        let b = |o: u32| unsafe { *((lf + o) as *const u8) } != 0;
        f.italic = b(20);
        f.underline = b(21);
        f.strike = b(22);
    }
    if fl & CF_FIXEDPITCHONLY != 0 && !FACES.iter().any(|x| x.1 & 3 == 1 && x.0.eq_ignore_ascii_case(&f.face)) {
        f.face = b"Courier New".to_vec();
    }
    let hook = if fl & CF_ENABLEHOOK != 0 { rd32(st, CF_HOOK) } else { 0 };
    let cd = Cd { kind: Kind::Font(f), st, hook, wide };
    run(&t, rd32(st, CF_HWNDOWNER), rd32(st, CF_HINSTANCE), cd) == 1
}

/// A LOGFONT of the choices (A: 60 bytes, W: 92).
fn logfont(f: &Font, wide: bool) -> Vec<u8> {
    let mut b = alloc::vec![0u8; if wide { 92 } else { 60 }];
    let height = -((f.size * 96 + 36) / 72);
    b[0..4].copy_from_slice(&height.to_le_bytes());
    b[16..20].copy_from_slice(&(if f.bold { 700u32 } else { 400 }).to_le_bytes());
    b[20] = f.italic as u8;
    b[21] = f.underline as u8;
    b[22] = f.strike as u8;
    // ANSI_CHARSET, OUT_STROKE_PRECIS, CLIP_STROKE_PRECIS, DRAFT_QUALITY.
    b[23] = 0;
    b[24] = 3;
    b[25] = 2;
    b[26] = 1;
    b[27] = FACES.iter().find(|x| x.0.eq_ignore_ascii_case(&f.face)).map_or(0x22, |x| x.1);
    if wide {
        for (i, c) in crate::to_wide(&f.face).iter().take(31).enumerate() {
            b[28 + 2 * i..30 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
    } else {
        let n = f.face.len().min(31);
        b[28..28 + n].copy_from_slice(&f.face[..n]);
    }
    b
}

fn update_sample(h: u16) {
    read_font(h);
    let Some(f) = font(h) else { return };
    let lf = logfont(f, false);
    let old = f.sample;
    let hf = crate::gdi32::CreateFontIndirectA(lf.as_ptr() as u32) as u16;
    if let Some(f) = font(h) {
        f.sample = hf;
    }
    let c = get_item(h, STC5);
    send(c, W_SETFONT, hf as u32, 1);
    invalidate(c, None);
    if old != 0 {
        gdi::delete_object(old);
    }
}

fn write_font(h: u16) {
    let Some((st, w)) = state(h).map(|s| (s.st, s.wide)) else { return };
    read_font(h);
    let Some(f) = font(h) else { return };
    let fl = rd32(st, CF_FLAGS);
    let lf = rd32(st, CF_LOGFONT);
    if lf != 0 {
        let b = logfont(f, w);
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), lf as *mut u8, b.len()) };
    }
    wr32(st, CF_POINTSIZE, (f.size * 10) as u32);
    if fl & CF_EFFECTS != 0 {
        wr32(st, CF_RGB, f.rgb);
    }
    // SCREEN_FONTTYPE | BOLD/REGULAR | ITALIC.
    let ty = 0x2000 | if f.bold { 0x100 } else { 0x400 } | if f.italic { 0x200 } else { 0 };
    wr16(st, CF_FONTTYPE, ty);
    if fl & CF_USESTYLE != 0 {
        put_str(rd32(st, CF_STYLE), 32, STYLES[(f.italic as usize) | (f.bold as usize) << 1], w);
    }
}

/// The sample's font goes with the dialog.
fn font_destroy(h: u16) {
    if let Some(hf) = font(h).map(|f| f.sample).filter(|&f| f != 0) {
        send(get_item(h, STC5), W_SETFONT, 0, 0);
        gdi::delete_object(hf);
    }
}

fn font(h: u16) -> Option<&'static mut Font> {
    match &mut state(h)?.kind {
        Kind::Font(f) => Some(f),
        _ => None,
    }
}

fn font_init(h: u16) {
    let Some(st) = state(h).map(|s| s.st) else { return };
    let fl = rd32(st, CF_FLAGS);
    if fl & CF_SHOWHELP == 0 {
        show(h, PSH_HELP, false);
    }
    if fl & CF_APPLY == 0 {
        show(h, PSH3, false);
    }
    if fl & CF_EFFECTS == 0 {
        for id in [GRP1, CHX1, CHX2, STC4, CMB4] {
            show(h, id, false);
        }
    }
    let Some(f) = font(h) else { return };
    let (face, bold, italic, size, ul, so, rgb) = (f.face.clone(), f.bold, f.italic, f.size, f.underline, f.strike, f.rgb);
    let cmb = get_item(h, CMB1);
    for (name, pf) in FACES {
        if fl & CF_FIXEDPITCHONLY == 0 || pf & 3 == 1 {
            send_str(cmb, CB_ADDSTRING, 0, name);
        }
    }
    let styles = get_item(h, CMB2);
    for s in STYLES {
        send_str(styles, CB_ADDSTRING, 0, s);
    }
    let sizes = get_item(h, CMB3);
    let (lo, hi) = if fl & CF_LIMITSIZE != 0 { (rd32(st, CF_SIZEMIN) as i32, rd32(st, CF_SIZEMAX) as i32) } else { (1, 999) };
    for s in SIZES {
        if (lo..=hi).contains(&s) {
            send_str(sizes, CB_ADDSTRING, 0, alloc::format!("{s}").as_bytes());
        }
    }
    let colors = get_item(h, CMB4);
    for (name, _) in COLORS {
        send_str(colors, CB_ADDSTRING, 0, name);
    }
    select_exact(cmb, &face);
    set_text(h, CMB1, &face);
    let style = (italic as usize) | (bold as usize) << 1;
    let given = if fl & CF_USESTYLE != 0 { get_str(rd32(st, CF_STYLE), wide(h)).filter(|s| !s.is_empty()) } else { None };
    let style_name: Vec<u8> = given.unwrap_or_else(|| STYLES[style].to_vec());
    select_exact(styles, &style_name);
    set_text(h, CMB2, &style_name);
    let st_size = alloc::format!("{size}");
    select_exact(sizes, st_size.as_bytes());
    set_text(h, CMB3, st_size.as_bytes());
    check(h, CHX1, so);
    check(h, CHX2, ul);
    let ci = COLORS.iter().position(|c| c.1 == rgb).unwrap_or(0);
    send(colors, CB_SETCURSEL, ci as u32, 0);
    for (id, on) in [(CMB1, fl & CF_NOFACESEL == 0), (CMB2, fl & CF_NOSTYLESEL == 0), (CMB3, fl & CF_NOSIZESEL == 0)] {
        if !on {
            set_text(h, id, b"");
        }
    }
    update_sample(h);
}

/// Select the item with this text (CB_FINDSTRINGEXACT).
fn select_exact(c: u16, s: &[u8]) {
    let i = send_str(c, CB_FINDSTRINGEXACT, 0xffff_ffff, s);
    if i as i32 >= 0 {
        send(c, CB_SETCURSEL, i, 0);
    }
}

/// Read the dialog's choices into the state.
fn read_font(h: u16) {
    let face = text(h, CMB1);
    let style = text(h, CMB2).to_ascii_lowercase();
    let size = text(h, CMB3);
    let ci = send(get_item(h, CMB4), CB_GETCURSEL, 0, 0) as usize;
    let (so, ul) = (checked(h, CHX1), checked(h, CHX2));
    let Some(f) = font(h) else { return };
    if !face.is_empty() {
        f.face = face;
    }
    if !style.is_empty() {
        f.bold = style.windows(4).any(|w| w == b"bold");
        f.italic = style.windows(6).any(|w| w == b"italic");
    }
    let n = size.iter().take_while(|c| c.is_ascii_digit()).fold(0, |v, &c| v * 10 + (c - b'0') as i32);
    if n > 0 {
        f.size = n.min(999);
    }
    f.strike = so;
    f.underline = ul;
    if let Some(c) = COLORS.get(ci) {
        f.rgb = c.1;
    }
}

fn font_msg(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    match m {
        WM_CHOOSEFONT_GETLOGFONT => {
            read_font(h);
            let w = wide(h);
            if let (Some(f), false) = (font(h), lp == 0) {
                let b = logfont(f, w);
                unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), lp as *mut u8, b.len()) };
            }
            1
        }
        W_CTLCOLORSTATIC => {
            // The sample in the chosen colour.
            if lp as u16 == get_item(h, STC5) {
                if let Some(f) = font(h) {
                    gdi::set_text_color(wp as u16, f.rgb);
                    gdi::set_bk_color(wp as u16, gdi::sys_color(gdi::COLOR_BTNFACE));
                    return gdi::sys_brush(gdi::COLOR_BTNFACE) as u32;
                }
            }
            0
        }
        W_COMMAND => {
            let code = (wp >> 16) as u16;
            let wp = wp as u16;
            match wp {
                IDOK => {
                    let size = text(h, CMB3);
                    if !size.is_empty() && !size.iter().all(|c| c.is_ascii_digit()) {
                        message(h, b"Size must be a number.", 0x30);
                        return 1;
                    }
                    write_font(h);
                    end(h, 1);
                    1
                }
                IDCANCEL => {
                    end(h, 0);
                    1
                }
                CMB1 | CMB2 | CMB3 | CMB4 if code == CBN_SELCHANGE || code == CBN_EDITCHANGE => {
                    if code == CBN_SELCHANGE {
                        // The list's selection becomes the edit's text.
                        let c = get_item(h, wp);
                        let i = send(c, CB_GETCURSEL, 0, 0);
                        if wp != CMB4 && i as i32 >= 0 {
                            let t = item_text(c, CB_GETLBTEXT, i);
                            set_text(h, wp, &t);
                        }
                    }
                    update_sample(h);
                    1
                }
                CHX1 | CHX2 => {
                    update_sample(h);
                    1
                }
                _ => 0,
            }
        }
        _ => 0,
    }
}

// ---- Print and Print Setup ----

struct Print {
    setup: bool,
    printers: Vec<crate::print::Printer>,
    /// The printer chosen (index in `printers`).
    sel: usize,
    /// The job properties: orientation (1 portrait, 2 landscape), paper
    /// (DMPAPER_*), copies.
    orient: u16,
    paper: u16,
    copies: u16,
}

// PRINTDLG offsets (Win32's is packed: 66 bytes).
const PD_HWNDOWNER: usize = 4;
const PD_HDEVMODE: usize = 8;
const PD_HDEVNAMES: usize = 12;
const PD_HDC: usize = 16;
const PD_FLAGS: usize = 20;
const PD_FROMPAGE: usize = 24;
const PD_TOPAGE: usize = 26;
const PD_MINPAGE: usize = 28;
const PD_MAXPAGE: usize = 30;
const PD_COPIES: usize = 32;
const PD_HINSTANCE: usize = 34;
const PD_PRINTHOOK: usize = 42;
const PD_SETUPHOOK: usize = 46;
const PD_PRINTTEMPLATENAME: usize = 50;
const PD_SETUPTEMPLATENAME: usize = 54;
const PD_HPRINTTEMPLATE: usize = 58;
const PD_HSETUPTEMPLATE: usize = 62;

const PD_SELECTION: u32 = 0x1;
const PD_PAGENUMS: u32 = 0x2;
const PD_NOSELECTION: u32 = 0x4;
const PD_NOPAGENUMS: u32 = 0x8;
const PD_COLLATE: u32 = 0x10;
const PD_PRINTTOFILE: u32 = 0x20;
const PD_PRINTSETUP: u32 = 0x40;
const PD_RETURNDC: u32 = 0x100;
const PD_RETURNIC: u32 = 0x200;
const PD_RETURNDEFAULT: u32 = 0x400;
const PD_SHOWHELP: u32 = 0x800;
const PD_ENABLEPRINTHOOK: u32 = 0x1000;
const PD_ENABLESETUPHOOK: u32 = 0x2000;
const PD_ENABLEPRINTTEMPLATE: u32 = 0x4000;
const PD_ENABLESETUPTEMPLATE: u32 = 0x8000;
const PD_ENABLEPRINTTEMPLATEHANDLE: u32 = 0x10000;
const PD_ENABLESETUPTEMPLATEHANDLE: u32 = 0x20000;
const PD_DISABLEPRINTTOFILE: u32 = 0x80000;
const PD_HIDEPRINTTOFILE: u32 = 0x100000;

/// The paper sizes (DMPAPER_*) in rs2's PostScript driver's forms order.
const PAPERS: [(&[u8], u16); 6] = [(b"Letter 8 1/2 x 11 in", 1), (b"A4 210 x 297 mm", 9), (b"Legal 8 1/2 x 14 in", 5), (b"A3 297 x 420 mm", 8), (b"A5 148 x 210 mm", 11), (b"Executive 7 1/4 x 10 1/2 in", 7)];

fn print_template() -> Vec<u8> {
    let mut t = Tpl::new(DLG, 36, 52, 225, 130, b"Print");
    t.label(b"Printer:", 0xffff, 4, 4, 40, 8)
        .item(0x82, b"", STC1, SS_NOPREFIX, 44, 4, 120, 18)
        .button(b"Print Range", GRP1, BS_GROUPBOX, 4, 27, 132, 67)
        .button(b"&All", RAD1, BS_AUTORADIOBUTTON | GROUP, 10, 39, 76, 12)
        .item(0x80, b"S&election", RAD2, BS_AUTORADIOBUTTON, 10, 52, 76, 12)
        .item(0x80, b"&Pages", RAD3, BS_AUTORADIOBUTTON, 10, 65, 76, 12)
        .item(0x82, b"&From:", STC2, SS_RIGHT, 20, 80, 28, 9)
        .edit(EDT1, 52, 78, 26, 12)
        .item(0x82, b"&To:", STC3, SS_RIGHT, 80, 80, 18, 9)
        .edit(EDT2, 102, 78, 26, 12)
        .label(b"Print &Quality:", STC4, 4, 100, 50, 9)
        .item(0x85, b"", CMB1, CBS_DROPDOWNLIST | WS_VSCROLL | TAB, 56, 98, 80, 36)
        .label(b"&Copies:", STC5, 146, 100, 30, 9)
        .edit(EDT3, 180, 98, 22, 12)
        .button(b"Print to Fi&le", CHX1, BS_AUTOCHECKBOX | GROUP, 4, 114, 120, 12)
        .button(b"Collate Cop&ies", CHX2, BS_AUTOCHECKBOX, 146, 114, 72, 12)
        .button(b"OK", IDOK, BS_DEFPUSHBUTTON | GROUP, 170, 4, 50, 14)
        .button(b"Cancel", IDCANCEL, GROUP, 170, 21, 50, 14)
        .button(b"&Setup...", PSH1, GROUP, 170, 41, 50, 14)
        .button(b"&Help", PSH_HELP, GROUP, 170, 58, 50, 14);
    t.done()
}

fn setup_template() -> Vec<u8> {
    let mut t = Tpl::new(DLG, 30, 23, 287, 122, b"Print Setup");
    t.button(b"Printer", GRP3, BS_GROUPBOX, 4, 6, 224, 66)
        .button(b"&Default Printer", RAD3, BS_AUTORADIOBUTTON | GROUP, 8, 16, 212, 12)
        .item(0x82, b"", STC1, SS_NOPREFIX, 20, 30, 204, 9)
        .item(0x80, b"Specific &Printer:", RAD4, BS_AUTORADIOBUTTON, 8, 42, 212, 12)
        .item(0x85, b"", CMB1, CBS_DROPDOWNLIST | WS_VSCROLL | TAB | GROUP, 20, 55, 204, 80)
        .button(b"Orientation", GRP1, BS_GROUPBOX, 4, 74, 91, 46)
        .button(b"Po&rtrait", RAD1, BS_AUTORADIOBUTTON | GROUP, 14, 86, 70, 12)
        .item(0x80, b"&Landscape", RAD2, BS_AUTORADIOBUTTON, 14, 102, 70, 12)
        .button(b"Paper", GRP2, BS_GROUPBOX, 100, 74, 128, 46)
        .label(b"Si&ze:", STC2, 104, 88, 28, 9)
        .item(0x85, b"", CMB2, CBS_DROPDOWNLIST | WS_VSCROLL | TAB | GROUP, 134, 86, 90, 80)
        .label(b"&Source:", STC3, 104, 104, 28, 9)
        .item(0x85, b"", CMB3, CBS_DROPDOWNLIST | WS_VSCROLL | TAB | GROUP, 134, 102, 90, 48)
        .button(b"OK", IDOK, BS_DEFPUSHBUTTON | GROUP, 232, 4, 50, 14)
        .button(b"Cancel", IDCANCEL, GROUP, 232, 21, 50, 14)
        .button(b"&Options...", PSH1, GROUP, 232, 40, 50, 14)
        .button(b"&Help", PSH_HELP, GROUP, 232, 57, 50, 14);
    t.done()
}

fn print_dlg(st: u32, wide: bool) -> bool {
    *ERROR.get() = 0;
    if st == 0 || rd32(st, 0) < 66 {
        *ERROR.get() = CDERR_STRUCTSIZE;
        return false;
    }
    let fl = rd32(st, PD_FLAGS);
    let printers = crate::print::printers();
    if printers.is_empty() {
        *ERROR.get() = PDERR_NODEFAULTPRN;
        return false;
    }
    // The printer and properties of hDevNames and hDevMode, else the
    // default printer's.
    let dflt = printers.iter().position(|p| p.default).unwrap_or(0);
    let mut p = Print { setup: fl & PD_PRINTSETUP != 0, sel: dflt, orient: 1, paper: 1, copies: rd16(st, PD_COPIES).max(1), printers };
    let hdn = rd32(st, PD_HDEVNAMES);
    let dn = crate::kern::global_lock(hdn);
    if dn != 0 {
        // DEVNAMES: offsets in characters.
        let u = if wide { 2 } else { 1 };
        let dev = get_str(dn + u * rd16(dn, 2) as u32, wide).unwrap_or_default();
        let out = get_str(dn + u * rd16(dn, 4) as u32, wide).unwrap_or_default();
        if let Some(i) = crate::print::find(&p.printers, &dev, &out) {
            p.sel = i;
        }
        crate::kern::global_unlock(hdn);
    }
    let hdm = rd32(st, PD_HDEVMODE);
    let dm = crate::kern::global_lock(hdm);
    if dm != 0 {
        // (DEVMODEW's fields are 32 bytes on: its name is UTF-16.)
        let dm = if wide { dm + 32 } else { dm };
        let fields = rd32(dm, 40);
        if fields & 0x1 != 0 {
            p.orient = rd16(dm, 44).clamp(1, 2);
        }
        if fields & 0x2 != 0 {
            p.paper = rd16(dm, 46);
        }
        if fields & 0x100 != 0 {
            p.copies = rd16(dm, 54).max(1);
        }
        crate::kern::global_unlock(hdm);
    } else {
        let props = crate::print::props(&p.printers[p.sel]);
        p.orient = if props.1 { 2 } else { 1 };
        p.paper = PAPERS.get(props.0 as usize).map_or(1, |x| x.1);
    }
    if fl & PD_RETURNDEFAULT != 0 {
        if hdn != 0 || hdm != 0 {
            *ERROR.get() = PDERR_RETDEFFAILURE;
            return false;
        }
        p.sel = dflt;
        return print_result(st, &p, fl, wide);
    }
    let setup = p.setup;
    let (use_res, use_handle, name, htpl) = if setup {
        (fl & PD_ENABLESETUPTEMPLATE != 0, fl & PD_ENABLESETUPTEMPLATEHANDLE != 0, rd32(st, PD_SETUPTEMPLATENAME), rd32(st, PD_HSETUPTEMPLATE))
    } else {
        (fl & PD_ENABLEPRINTTEMPLATE != 0, fl & PD_ENABLEPRINTTEMPLATEHANDLE != 0, rd32(st, PD_PRINTTEMPLATENAME), rd32(st, PD_HPRINTTEMPLATE))
    };
    let ours = if setup { setup_template() } else { print_template() };
    let Some(t) = template(use_res, use_handle, if use_handle { crate::kern::global_lock(htpl).max(htpl) } else { rd32(st, PD_HINSTANCE) }, name, wide, ours) else { return false };
    let hook = match (setup, fl & PD_ENABLESETUPHOOK != 0, fl & PD_ENABLEPRINTHOOK != 0) {
        (true, true, _) => rd32(st, PD_SETUPHOOK),
        (false, _, true) => rd32(st, PD_PRINTHOOK),
        _ => 0,
    };
    let cd = Cd { kind: Kind::Print(p), st, hook, wide };
    *PRINT_RESULT.get() = None;
    let r = run(&t, rd32(st, PD_HWNDOWNER), rd32(st, PD_HINSTANCE), cd);
    match PRINT_RESULT.get().take() {
        Some(p) if r == 1 => print_result(st, &p, rd32(st, PD_FLAGS), wide),
        _ => false,
    }
}

/// The choices of a Print or Print Setup dialog closed with OK.
static PRINT_RESULT: Global<Option<Print>> = Global::new(None);

/// Fill hDevMode, hDevNames (and hDC) from the choices: a DEVMODEA
/// (DEVMODEW) and DEVNAMES in global memory.
fn print_result(st: u32, p: &Print, fl: u32, wide: bool) -> bool {
    let pr = &p.printers[p.sel];
    let dev = crate::print::device_name(pr);
    let out = crate::print::port_name(pr);
    // DEVMODE: the name (32 characters), then the fields (dmSize 156,
    // 220 for DEVMODEW).
    let (size, name_len) = if wide { (220u32, 64usize) } else { (156u32, 32usize) };
    let mut hdm = rd32(st, PD_HDEVMODE);
    if hdm != 0 && crate::kern::global_size(hdm) < size {
        hdm = crate::kern::global_realloc(hdm, size, 0x0042);
    }
    if hdm == 0 {
        hdm = crate::kern::global_alloc(0x0042, size);
    }
    let dm = crate::kern::global_lock(hdm);
    if dm != 0 {
        let mut b = vec![0u8; size as usize];
        let n = dev.len().min(31);
        if wide {
            for (i, c) in crate::to_wide(&dev[..n]).iter().enumerate() {
                b[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
            }
        } else {
            b[..n].copy_from_slice(&dev[..n]);
        }
        let f = &mut b[name_len - 32..];
        f[32..34].copy_from_slice(&0x0401u16.to_le_bytes());
        f[34..36].copy_from_slice(&0x0100u16.to_le_bytes());
        f[36..38].copy_from_slice(&(size as u16).to_le_bytes());
        // DM_ORIENTATION | DM_PAPERSIZE | DM_COPIES
        f[40..44].copy_from_slice(&0x0000_0103u32.to_le_bytes());
        f[44..46].copy_from_slice(&p.orient.to_le_bytes());
        f[46..48].copy_from_slice(&p.paper.to_le_bytes());
        f[52..54].copy_from_slice(&100u16.to_le_bytes());
        f[54..56].copy_from_slice(&p.copies.to_le_bytes());
        f[58..60].copy_from_slice(&300u16.to_le_bytes());
        f[60..62].copy_from_slice(&2u16.to_le_bytes());
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), dm as *mut u8, b.len()) };
        crate::kern::global_unlock(hdm);
        wr32(st, PD_HDEVMODE, hdm);
    }
    // DEVNAMES: the offsets (in characters), then the driver, device and
    // port.
    let driver: &[u8] = b"PSCRIPT";
    let (o1, o2) = (4 + driver.len() + 1, 4 + driver.len() + 1 + dev.len() + 1);
    let dflt = (p.sel == p.printers.iter().position(|x| x.default).unwrap_or(usize::MAX)) as u16;
    let mut chars: Vec<u16> = alloc::vec![4, o1 as u16, o2 as u16, dflt];
    for s in [driver, &dev, &out] {
        chars.extend(crate::to_wide(s));
        chars.push(0);
    }
    let d: Vec<u8> = if wide {
        chars.iter().flat_map(|c| c.to_le_bytes()).collect()
    } else {
        // Offsets in bytes: as the characters' (each string is as many
        // bytes as characters in the code page), the header 8 bytes.
        let mut v: Vec<u8> = Vec::new();
        for (i, c) in chars[..4].iter().enumerate() {
            let o = if i < 3 { *c + 4 } else { *c };
            v.extend_from_slice(&o.to_le_bytes());
        }
        for s in [driver, &dev, &out] {
            v.extend_from_slice(s);
            v.push(0);
        }
        v
    };
    let mut hdn = rd32(st, PD_HDEVNAMES);
    if hdn != 0 {
        hdn = crate::kern::global_realloc(hdn, d.len() as u32, 0x0042);
    }
    if hdn == 0 {
        hdn = crate::kern::global_alloc(0x0042, d.len() as u32);
    }
    let q = crate::kern::global_lock(hdn);
    if q != 0 {
        unsafe { core::ptr::copy_nonoverlapping(d.as_ptr(), q as *mut u8, d.len()) };
        crate::kern::global_unlock(hdn);
        wr32(st, PD_HDEVNAMES, hdn);
    }
    wr16(st, PD_COPIES, p.copies);
    if fl & (PD_RETURNDC | PD_RETURNIC) != 0 {
        let dm = crate::kern::global_lock(hdm);
        let dmf = if dm == 0 { None } else { Some(if wide { dm + 32 } else { dm }) };
        let hdc = crate::print::create_dc(pr, dmf, fl & PD_RETURNDC != 0);
        crate::kern::global_unlock(hdm);
        if hdc == 0 {
            *ERROR.get() = PDERR_CREATEICFAILURE;
            return false;
        }
        wr32(st, PD_HDC, hdc as u32);
    }
    true
}

fn print(h: u16) -> Option<&'static mut Print> {
    match &mut state(h)?.kind {
        Kind::Print(p) => Some(p),
        _ => None,
    }
}

fn printer_label(p: &crate::print::Printer) -> Vec<u8> {
    let mut s = crate::print::device_name(p);
    s.extend_from_slice(b" on ");
    s.extend_from_slice(&crate::print::port_name(p));
    s
}

fn print_init(h: u16) {
    let Some(st) = state(h).map(|s| s.st) else { return };
    let fl = rd32(st, PD_FLAGS);
    if fl & PD_SHOWHELP == 0 {
        show(h, PSH_HELP, false);
    }
    let Some(p) = print(h) else { return };
    let label = printer_label(&p.printers[p.sel]);
    if p.setup {
        let dflt = p.printers.iter().position(|x| x.default).unwrap_or(0);
        let mut d = b"(".to_vec();
        d.extend_from_slice(&printer_label(&p.printers[dflt]));
        d.push(b')');
        let cmb = get_item(h, CMB1);
        for x in &p.printers {
            send_str(cmb, CB_ADDSTRING, 0, &printer_label(x));
        }
        let (sel, orient, paper) = (p.sel, p.orient, p.paper);
        send(cmb, CB_SETCURSEL, sel as u32, 0);
        set_text(h, STC1, &d);
        check(h, if sel == dflt { RAD3 } else { RAD4 }, true);
        check(h, if orient == 2 { RAD2 } else { RAD1 }, true);
        let papers = get_item(h, CMB2);
        for (name, _) in PAPERS {
            send_str(papers, CB_ADDSTRING, 0, name);
        }
        send(papers, CB_SETCURSEL, PAPERS.iter().position(|x| x.1 == paper).unwrap_or(0) as u32, 0);
        let src = get_item(h, CMB3);
        send_str(src, CB_ADDSTRING, 0, b"Automatic");
        send(src, CB_SETCURSEL, 0, 0);
        return;
    }
    let copies = p.copies;
    set_text(h, STC1, &label);
    let q = get_item(h, CMB1);
    send_str(q, CB_ADDSTRING, 0, b"300 dpi");
    send(q, CB_SETCURSEL, 0, 0);
    set_text(h, EDT3, alloc::format!("{copies}").as_bytes());
    if fl & PD_NOSELECTION != 0 {
        enable(h, RAD2, false);
    }
    if fl & PD_NOPAGENUMS != 0 {
        for id in [RAD3, STC2, EDT1, STC3, EDT2] {
            enable(h, id, false);
        }
    } else {
        set_text(h, EDT1, alloc::format!("{}", rd16(st, PD_FROMPAGE)).as_bytes());
        set_text(h, EDT2, alloc::format!("{}", rd16(st, PD_TOPAGE)).as_bytes());
    }
    check(h, if fl & PD_SELECTION != 0 { RAD2 } else if fl & PD_PAGENUMS != 0 { RAD3 } else { RAD1 }, true);
    check(h, CHX1, fl & PD_PRINTTOFILE != 0);
    check(h, CHX2, fl & PD_COLLATE != 0);
    if fl & PD_DISABLEPRINTTOFILE != 0 {
        enable(h, CHX1, false);
    }
    if fl & PD_HIDEPRINTTOFILE != 0 {
        show(h, CHX1, false);
    }
}

fn print_msg(h: u16, m: u16, wp: u32, _lp: u32) -> u32 {
    if m != W_COMMAND {
        return 0;
    }
    let code = (wp >> 16) as u16;
    let wp = wp as u16;
    let Some(st) = state(h).map(|s| s.st) else { return 0 };
    let setup = print(h).is_some_and(|p| p.setup);
    match wp {
        IDOK if setup => {
            let specific = checked(h, RAD4);
            let sel = send(get_item(h, CMB1), CB_GETCURSEL, 0, 0) as usize;
            let paper = send(get_item(h, CMB2), CB_GETCURSEL, 0, 0) as usize;
            let landscape = checked(h, RAD2);
            let Some(p) = print(h) else { return 1 };
            let dflt = p.printers.iter().position(|x| x.default).unwrap_or(0);
            p.sel = if specific && sel < p.printers.len() { sel } else { dflt };
            p.orient = if landscape { 2 } else { 1 };
            p.paper = PAPERS.get(paper).map_or(1, |x| x.1);
            finish_print(h, p);
            1
        }
        IDOK => {
            let mut fl = rd32(st, PD_FLAGS) & !(PD_SELECTION | PD_PAGENUMS | PD_PRINTTOFILE | PD_COLLATE);
            if checked(h, RAD2) {
                fl |= PD_SELECTION;
            }
            if checked(h, RAD3) {
                let (from, to) = (edit_value(h, EDT1), edit_value(h, EDT2));
                let (lo, hi) = (rd16(st, PD_MINPAGE) as i32, rd16(st, PD_MAXPAGE) as i32);
                if hi > 0 && (from < lo || to > hi || from > to) {
                    let t = alloc::format!("The page numbers must be between {lo} and {hi}.");
                    message(h, t.as_bytes(), 0x30);
                    return 1;
                }
                wr16(st, PD_FROMPAGE, from as u16);
                wr16(st, PD_TOPAGE, to as u16);
                fl |= PD_PAGENUMS;
            }
            if checked(h, CHX1) {
                fl |= PD_PRINTTOFILE;
            }
            if checked(h, CHX2) {
                fl |= PD_COLLATE;
            }
            wr32(st, PD_FLAGS, fl);
            let copies = edit_value(h, EDT3).clamp(1, 999) as u16;
            let Some(p) = print(h) else { return 1 };
            p.copies = copies;
            finish_print(h, p);
            1
        }
        IDCANCEL => {
            end(h, 0);
            1
        }
        PSH1 if !setup => {
            // Print Setup, on the choices so far.
            let Some(p) = print(h) else { return 1 };
            let fl = rd32(st, PD_FLAGS);
            let printers = p.printers.clone();
            let sub = Print { setup: true, printers, sel: p.sel, orient: p.orient, paper: p.paper, copies: p.copies };
            let (use_res, use_handle) = (fl & PD_ENABLESETUPTEMPLATE != 0, fl & PD_ENABLESETUPTEMPLATEHANDLE != 0);
            let htpl = if use_handle { crate::kern::global_lock(rd32(st, PD_HSETUPTEMPLATE)).max(rd32(st, PD_HSETUPTEMPLATE)) } else { rd32(st, PD_HINSTANCE) };
            let w = wide(h);
            let Some(t) = template(use_res, use_handle, htpl, rd32(st, PD_SETUPTEMPLATENAME), w, setup_template()) else { return 1 };
            let hook = if fl & PD_ENABLESETUPHOOK != 0 { rd32(st, PD_SETUPHOOK) } else { 0 };
            *PRINT_RESULT.get() = None;
            if run(&t, h as u32, rd32(st, PD_HINSTANCE), Cd { kind: Kind::Print(sub), st, hook, wide: w }) == 1 {
                if let (Some(r), Some(p)) = (PRINT_RESULT.get().take(), print(h)) {
                    p.sel = r.sel;
                    p.orient = r.orient;
                    p.paper = r.paper;
                    let label = printer_label(&p.printers[p.sel]);
                    set_text(h, STC1, &label);
                }
            }
            1
        }
        PSH1 if setup => {
            // Options: the driver's job properties.
            let specific = checked(h, RAD4);
            let sel = send(get_item(h, CMB1), CB_GETCURSEL, 0, 0) as usize;
            let paper = PAPERS.get(send(get_item(h, CMB2), CB_GETCURSEL, 0, 0) as usize).map_or(1, |x| x.1);
            let orient = if checked(h, RAD2) { 2 } else { 1 };
            let Some(p) = print(h) else { return 1 };
            let dflt = p.printers.iter().position(|x| x.default).unwrap_or(0);
            let i = if specific && sel < p.printers.len() { sel } else { dflt };
            let pr = p.printers[i].clone();
            if let Some((o, pa)) = crate::print::job_properties(&pr, orient, paper) {
                check(h, RAD1, o == 1);
                check(h, RAD2, o == 2);
                let k = PAPERS.iter().position(|x| x.1 == pa).unwrap_or(0);
                send(get_item(h, CMB2), CB_SETCURSEL, k as u32, 0);
            }
            1
        }
        RAD3 | RAD4 if setup => 0,
        CMB1 if setup && code == CBN_SELCHANGE => {
            check(h, RAD3, false);
            check(h, RAD4, true);
            1
        }
        EDT1 | EDT2 if !setup && code == EN_CHANGE => {
            // Typing page numbers chooses Pages.
            if focus() == get_item(h, wp) {
                check(h, RAD1, false);
                check(h, RAD2, false);
                check(h, RAD3, true);
            }
            1
        }
        _ => 0,
    }
}

/// End a Print or Setup dialog with OK: its choices for print_dlg.
fn finish_print(h: u16, p: &Print) {
    *PRINT_RESULT.get() = Some(Print { setup: p.setup, printers: p.printers.clone(), sel: p.sel, orient: p.orient, paper: p.paper, copies: p.copies });
    end(h, 1);
}

// ---- Find and Replace ----

struct Find {
    replace: bool,
    msg: u16,
    owner: u16,
}

// FINDREPLACE offsets.
// FINDREPLACE offsets.
const FR_HWNDOWNER: usize = 4;
const FR_HINSTANCE: usize = 8;
const FR_FLAGS: usize = 12;
const FR_FINDWHAT: usize = 16;
const FR_REPLACEWITH: usize = 20;
const FR_FINDWHATLEN: usize = 24;
const FR_REPLACEWITHLEN: usize = 26;
const FR_HOOK: usize = 32;
const FR_TEMPLATENAME: usize = 36;

const FR_DOWN: u32 = 0x1;
const FR_WHOLEWORD: u32 = 0x2;
const FR_MATCHCASE: u32 = 0x4;
const FR_FINDNEXT: u32 = 0x8;
const FR_REPLACE: u32 = 0x10;
const FR_REPLACEALL: u32 = 0x20;
const FR_DIALOGTERM: u32 = 0x40;
const FR_SHOWHELP: u32 = 0x80;
const FR_ENABLEHOOK: u32 = 0x100;
const FR_ENABLETEMPLATE: u32 = 0x200;
const FR_NOUPDOWN: u32 = 0x400;
const FR_NOMATCHCASE: u32 = 0x800;
const FR_NOWHOLEWORD: u32 = 0x1000;
const FR_ENABLETEMPLATEHANDLE: u32 = 0x2000;
const FR_HIDEUPDOWN: u32 = 0x4000;
const FR_HIDEMATCHCASE: u32 = 0x8000;
const FR_HIDEWHOLEWORD: u32 = 0x10000;

fn find_template(replace: bool) -> Vec<u8> {
    let vis = WS_VISIBLE;
    if replace {
        let mut t = Tpl::new(DLG | vis, 36, 44, 230, 94, b"Replace");
        t.label(b"Fi&nd What:", 0xffff, 4, 9, 48, 8)
            .edit(EDT1, 54, 7, 114, 12)
            .label(b"Re&place With:", 0xffff, 4, 26, 48, 8)
            .edit(EDT2, 54, 24, 114, 12)
            .button(b"Match &Whole Word Only", CHX1, BS_AUTOCHECKBOX | GROUP, 5, 46, 104, 12)
            .button(b"Match &Case", CHX2, BS_AUTOCHECKBOX, 5, 62, 59, 12)
            .button(b"&Find Next", IDOK, BS_DEFPUSHBUTTON | GROUP, 174, 4, 50, 14)
            .button(b"&Replace", PSH1, GROUP, 174, 21, 50, 14)
            .button(b"Replace &All", PSH2, GROUP, 174, 38, 50, 14)
            .button(b"Cancel", IDCANCEL, GROUP, 174, 55, 50, 14)
            .button(b"&Help", PSH_HELP, GROUP, 174, 75, 50, 14);
        t.done()
    } else {
        let mut t = Tpl::new(DLG | vis, 30, 73, 236, 62, b"Find");
        t.label(b"Fi&nd What:", 0xffff, 4, 8, 42, 8)
            .edit(EDT1, 47, 7, 128, 12)
            .button(b"Match &Whole Word Only", CHX1, BS_AUTOCHECKBOX | GROUP, 4, 26, 100, 12)
            .button(b"Match &Case", CHX2, BS_AUTOCHECKBOX, 4, 42, 64, 12)
            .button(b"Direction", GRP1, BS_GROUPBOX, 107, 26, 68, 28)
            .button(b"&Up", RAD1, BS_AUTORADIOBUTTON | GROUP, 111, 38, 26, 12)
            .item(0x80, b"&Down", RAD2, BS_AUTORADIOBUTTON, 138, 38, 34, 12)
            .button(b"&Find Next", IDOK, BS_DEFPUSHBUTTON | GROUP, 182, 5, 50, 14)
            .button(b"Cancel", IDCANCEL, GROUP, 182, 23, 50, 14)
            .button(b"&Help", PSH_HELP, GROUP, 182, 45, 50, 14);
        t.done()
    }
}

/// FindText, ReplaceText: a modeless dialog (its window).
fn find_text(st: u32, replace: bool, wide: bool) -> u32 {
    *ERROR.get() = 0;
    if st == 0 || rd32(st, 0) < 40 {
        *ERROR.get() = CDERR_STRUCTSIZE;
        return 0;
    }
    if rd16(st, FR_FINDWHATLEN) == 0 || rd32(st, FR_FINDWHAT) == 0 || (replace && (rd16(st, FR_REPLACEWITHLEN) == 0 || rd32(st, FR_REPLACEWITH) == 0)) {
        *ERROR.get() = FRERR_BUFFERLENGTHZERO;
        return 0;
    }
    let fl = rd32(st, FR_FLAGS);
    let hinst = rd32(st, FR_HINSTANCE);
    let Some(t) = template(fl & FR_ENABLETEMPLATE != 0, fl & FR_ENABLETEMPLATEHANDLE != 0, hinst, rd32(st, FR_TEMPLATENAME), wide, find_template(replace)) else { return 0 };
    let hook = if fl & FR_ENABLEHOOK != 0 { rd32(st, FR_HOOK) } else { 0 };
    let owner = hw(rd32(st, FR_HWNDOWNER));
    let msg = crate::user32::register_message(b"commdlg_FindReplace") as u16;
    *PENDING.get() = Some(Cd { kind: Kind::Find(Find { replace, msg, owner }), st, hook, wide });
    let h = crate::dialog::create_dialog(hinst, &t, owner, proc_addr(), 0, false);
    *PENDING.get() = None;
    h as u32
}

fn find_init(h: u16) {
    let Some((st, w)) = state(h).map(|s| (s.st, s.wide)) else { return };
    let fl = rd32(st, FR_FLAGS);
    let what = get_str(rd32(st, FR_FINDWHAT), w).unwrap_or_default();
    set_text(h, EDT1, &what);
    send(get_item(h, EDT1), EM_LIMITTEXT, rd16(st, FR_FINDWHATLEN).saturating_sub(1) as u32, 0);
    enable(h, IDOK, !what.is_empty());
    if find(h).is_some_and(|f| f.replace) {
        set_text(h, EDT2, &get_str(rd32(st, FR_REPLACEWITH), w).unwrap_or_default());
        send(get_item(h, EDT2), EM_LIMITTEXT, rd16(st, FR_REPLACEWITHLEN).saturating_sub(1) as u32, 0);
        enable(h, PSH1, !what.is_empty());
        enable(h, PSH2, !what.is_empty());
    }
    check(h, CHX1, fl & FR_WHOLEWORD != 0);
    check(h, CHX2, fl & FR_MATCHCASE != 0);
    check(h, if fl & FR_DOWN != 0 { RAD2 } else { RAD1 }, true);
    for (flag, hide, ids) in [(FR_NOWHOLEWORD, FR_HIDEWHOLEWORD, &[CHX1][..]), (FR_NOMATCHCASE, FR_HIDEMATCHCASE, &[CHX2]), (FR_NOUPDOWN, FR_HIDEUPDOWN, &[GRP1, RAD1, RAD2])] {
        for &id in ids {
            if fl & hide != 0 {
                show(h, id, false);
            } else if fl & flag != 0 {
                enable(h, id, false);
            }
        }
    }
    if fl & FR_SHOWHELP == 0 {
        show(h, PSH_HELP, false);
    }
}

fn find(h: u16) -> Option<&'static mut Find> {
    match &mut state(h)?.kind {
        Kind::Find(f) => Some(f),
        _ => None,
    }
}

/// Tell the owner (FINDMSGSTRING) with these flags.
fn find_notify(h: u16, what: u32) {
    let Some((st, w)) = state(h).map(|s| (s.st, s.wide)) else { return };
    let Some((msg, owner, replace)) = find(h).map(|f| (f.msg, f.owner, f.replace)) else { return };
    let mut fl = rd32(st, FR_FLAGS) & !(FR_FINDNEXT | FR_REPLACE | FR_REPLACEALL | FR_DIALOGTERM | FR_WHOLEWORD | FR_MATCHCASE | FR_DOWN);
    fl |= what;
    if what != FR_DIALOGTERM {
        let t = text(h, EDT1);
        put_str(rd32(st, FR_FINDWHAT), rd16(st, FR_FINDWHATLEN) as u32, &t, w);
        if replace {
            let t = text(h, EDT2);
            put_str(rd32(st, FR_REPLACEWITH), rd16(st, FR_REPLACEWITHLEN) as u32, &t, w);
        }
        if checked(h, CHX1) {
            fl |= FR_WHOLEWORD;
        }
        if checked(h, CHX2) {
            fl |= FR_MATCHCASE;
        }
        if replace || checked(h, RAD2) {
            fl |= FR_DOWN;
        }
    }
    wr32(st, FR_FLAGS, fl);
    send(owner, msg, 0, st);
}

fn find_msg(h: u16, m: u16, wp: u32, lp: u32) -> u32 {
    let _ = lp;
    if m != W_COMMAND {
        return 0;
    }
    let code = (wp >> 16) as u16;
    match wp as u16 {
        IDOK => {
            find_notify(h, FR_FINDNEXT);
            1
        }
        PSH1 => {
            find_notify(h, FR_REPLACE);
            1
        }
        PSH2 => {
            find_notify(h, FR_REPLACEALL);
            1
        }
        IDCANCEL => {
            find_notify(h, FR_DIALOGTERM);
            destroy_window(h);
            1
        }
        EDT1 if code == EN_CHANGE => {
            let some = !text(h, EDT1).is_empty();
            for id in [IDOK, PSH1, PSH2] {
                enable(h, id, some);
            }
            1
        }
        _ => 0,
    }
}

// ---- The entry points ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetOpenFileNameA(st: u32) -> i32 {
    get_file_name(st, false, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetOpenFileNameW(st: u32) -> i32 {
    get_file_name(st, false, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSaveFileNameA(st: u32) -> i32 {
    get_file_name(st, true, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSaveFileNameW(st: u32) -> i32 {
    get_file_name(st, true, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChooseColorA(st: u32) -> i32 {
    choose_color(st, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChooseColorW(st: u32) -> i32 {
    choose_color(st, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChooseFontA(st: u32) -> i32 {
    choose_font(st, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ChooseFontW(st: u32) -> i32 {
    choose_font(st, true) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindTextA(st: u32) -> u32 {
    find_text(st, false, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FindTextW(st: u32) -> u32 {
    find_text(st, false, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReplaceTextA(st: u32) -> u32 {
    find_text(st, true, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ReplaceTextW(st: u32) -> u32 {
    find_text(st, true, true)
}

/// PrintDlg(lppd): Print, or Print Setup (PD_PRINTSETUP), on the
/// spooler's printers (print.rs).
#[unsafe(no_mangle)]
pub extern "stdcall" fn PrintDlgA(st: u32) -> i32 {
    print_dlg(st, false) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PrintDlgW(st: u32) -> i32 {
    print_dlg(st, true) as i32
}

/// PageSetupDlg: not yet (CDERR_DIALOGFAILURE would stop programs that
/// only want a printer; they get none).
#[unsafe(no_mangle)]
pub extern "stdcall" fn PageSetupDlgA(_st: u32) -> i32 {
    *ERROR.get() = PDERR_NODEFAULTPRN;
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PageSetupDlgW(_st: u32) -> i32 {
    *ERROR.get() = PDERR_NODEFAULTPRN;
    0
}

/// PrintDlgEx: E_FAIL.
#[unsafe(no_mangle)]
pub extern "stdcall" fn PrintDlgExA(_st: u32) -> u32 {
    *ERROR.get() = PDERR_NODEFAULTPRN;
    0x8000_4005
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PrintDlgExW(_st: u32) -> u32 {
    *ERROR.get() = PDERR_NODEFAULTPRN;
    0x8000_4005
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CommDlgExtendedError() -> u32 {
    *ERROR.get()
}

/// GetFileTitle(path, buf, cb): 0, the size needed (buffer too small),
/// or -1 (not a file name).
fn file_title(path: &[u8], buf: u32, cb: u32, wide: bool) -> i16 {
    let t = title_of(path);
    if t.is_empty() || t.iter().any(|&c| b"*?\\:".contains(&c)) {
        return -1;
    }
    let n = units(&t, wide);
    if (cb as usize) < n + 1 {
        return n as i16 + 1;
    }
    put_str(buf, cb, &t, wide);
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetFileTitleA(p: u32, buf: u32, cb: u32) -> i16 {
    file_title(&get_str(p, false).unwrap_or_default(), buf, cb, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetFileTitleW(p: u32, buf: u32, cb: u32) -> i16 {
    file_title(&get_str(p, true).unwrap_or_default(), buf, cb, true)
}
