//! The standard font dialog (WinFontDlg): the fonts rs2 has (the system
//! font and the outline faces drawn from it) with their styles and
//! sizes, emphasis, and a preview; the choice comes back in the
//! FONTDLG (fAttrs, fxPointSize, family name, style). The logic is
//! WinDefFontDlgProc; FONTDLG.pfnDlgProc may subclass it. The FONTDLG
//! is the dialog's QWL_USER window word.

use alloc::vec::Vec;

use crate::pm::*;

// FONTDLG offsets.
const N_TITLE: usize = 12;
const N_PREVIEW: usize = 16;
const N_DLGPROC: usize = 24;
const N_FAMILY: usize = 28;
const N_POINTSIZE: usize = 32;
const N_FL: usize = 36;
const N_STYLE: usize = 52;
const N_RETURN: usize = 72;
const N_SRC: usize = 76;
const N_EMHEIGHT: usize = 80;
const N_XHEIGHT: usize = 84;
const N_LEADING: usize = 88;
const N_FATTRS: usize = 96;
const N_NOMINAL: usize = 152;
const N_WEIGHT: usize = 154;
const N_WIDTH: usize = 156;
const N_FAMILYLEN: usize = 164;

const FNTS_HELPBUTTON: u32 = 0x0008;
const FNTS_APPLYBUTTON: u32 = 0x0010;
const FNTS_RESETBUTTON: u32 = 0x0020;
const FNTS_MODELESS: u32 = 0x0040;
const FNTS_INITFROMFATTRS: u32 = 0x0080;

const FATTR_SEL_UNDERSCORE: u32 = 0x0002;
const FATTR_SEL_OUTLINE: u32 = 0x0008;
const FATTR_SEL_STRIKEOUT: u32 = 0x0010;
const FATTR_SEL_BOLD: u32 = 0x0020;
const FATTR_FONTUSE_OUTLINE: u16 = 0x0004;

const DID_NAME: u16 = 301;
const DID_STYLE: u16 = 302;
const DID_SIZE: u16 = 305;
const DID_SAMPLE: u16 = 306;
const DID_OUTLINE: u16 = 307;
const DID_UNDERSCORE: u16 = 308;
const DID_STRIKEOUT: u16 = 309;
const DID_HELP_BUTTON: u16 = 310;
const DID_APPLY_BUTTON: u16 = 311;
const DID_RESET_BUTTON: u16 = 312;
const DID_NAME_PREFIX: u16 = 313;
const DID_STYLE_PREFIX: u16 = 314;
const DID_SIZE_PREFIX: u16 = 315;
const DID_SAMPLE_GROUPBOX: u16 = 316;
const DID_EMPHASIS_GROUPBOX: u16 = 317;

/// Families: (name, outline).
const FAMILIES: &[(&[u8], bool)] = &[(b"System VIO", false), (b"Helvetica", true), (b"Times New Roman", true), (b"Courier", true)];
const STYLES: &[&[u8]] = &[b"Normal", b"Bold"];
const SIZES: &[&[u8]] = &[b"8", b"9", b"10", b"12", b"14", b"18", b"24"];

#[derive(Clone)]
struct Choice {
    family: usize,
    bold: bool,
    points: i32,
    underscore: bool,
    strikeout: bool,
    outline: bool,
}

struct State {
    hwnd: u32,
    fntd: *mut u8,
    choice: Choice,
    initial: Choice,
    preview: Vec<u8>,
    sample_proc: u32,
}

static mut STATES: Vec<State> = Vec::new();

fn states() -> &'static mut Vec<State> {
    unsafe { &mut *core::ptr::addr_of_mut!(STATES) }
}

fn state(h: u32) -> Option<&'static mut State> {
    states().iter_mut().find(|s| s.hwnd == h)
}

fn rd32(p: *const u8, off: usize) -> u32 {
    unsafe { (p.add(off) as *const u32).read_unaligned() }
}

fn rd16(p: *const u8, off: usize) -> u16 {
    unsafe { (p.add(off) as *const u16).read_unaligned() }
}

fn wr32(p: *mut u8, off: usize, v: u32) {
    unsafe { (p.add(off) as *mut u32).write_unaligned(v) }
}

fn wr16(p: *mut u8, off: usize, v: u16) {
    unsafe { (p.add(off) as *mut u16).write_unaligned(v) }
}

/// Pels of a point size at 96 dpi.
fn pels(points: i32) -> i32 {
    (points * 96 + 36) / 72
}

fn face_of(c: &Choice) -> Vec<u8> {
    let mut f = FAMILIES[c.family].0.to_vec();
    if c.bold && FAMILIES[c.family].1 {
        f.extend_from_slice(b" Bold");
    }
    f
}

/// The FATTRS of a choice.
fn fattrs(c: &Choice) -> [u8; 56] {
    let mut a = [0u8; 56];
    a[0..2].copy_from_slice(&56u16.to_le_bytes());
    let mut sel = 0u32;
    if c.underscore {
        sel |= FATTR_SEL_UNDERSCORE;
    }
    if c.strikeout {
        sel |= FATTR_SEL_STRIKEOUT;
    }
    if c.outline {
        sel |= FATTR_SEL_OUTLINE;
    }
    if c.bold && !FAMILIES[c.family].1 {
        sel |= FATTR_SEL_BOLD;
    }
    a[2..4].copy_from_slice(&(sel as u16).to_le_bytes());
    let face = face_of(c);
    a[8..8 + face.len()].copy_from_slice(&face);
    a[42..44].copy_from_slice(&850u16.to_le_bytes());
    let h = pels(c.points);
    a[44..48].copy_from_slice(&h.to_le_bytes());
    a[48..52].copy_from_slice(&(h / 2).max(1).to_le_bytes());
    if FAMILIES[c.family].1 {
        a[54..56].copy_from_slice(&FATTR_FONTUSE_OUTLINE.to_le_bytes());
    }
    a
}

/// The choice an application's FONTDLG starts from.
fn initial(fntd: *const u8) -> Choice {
    let fl = rd32(fntd, N_FL);
    let fa = unsafe { core::slice::from_raw_parts(fntd.add(N_FATTRS), 56) };
    let face_from = |f: &[u8]| -> (usize, bool) {
        let bold = f.ends_with(b" Bold") || f.ends_with(b" bold");
        let base = if bold { &f[..f.len() - 5] } else { f };
        let i = FAMILIES.iter().position(|(n, _)| n.eq_ignore_ascii_case(base)).or_else(|| FAMILIES.iter().position(|(n, _)| base.len() >= 4 && n.len() >= 4 && n[..4].eq_ignore_ascii_case(&base[..4]))).unwrap_or(0);
        (i, bold)
    };
    let fx = rd32(fntd, N_POINTSIZE);
    let mut points = if fx >> 16 != 0 { (fx >> 16) as i32 } else { 12 };
    let mut c = Choice { family: 0, bold: false, points, underscore: false, strikeout: false, outline: false };
    if fl & FNTS_INITFROMFATTRS != 0 && fa[8] != 0 {
        let face = &fa[8..40];
        let face = &face[..face.iter().position(|&b| b == 0).unwrap_or(32)];
        let (i, bold) = face_from(face);
        let sel = u16::from_le_bytes([fa[2], fa[3]]) as u32;
        let h = i32::from_le_bytes([fa[44], fa[45], fa[46], fa[47]]);
        if fx >> 16 == 0 && h > 0 {
            points = (h * 72 + 48) / 96;
        }
        c = Choice { family: i, bold: bold || sel & FATTR_SEL_BOLD != 0, points, underscore: sel & FATTR_SEL_UNDERSCORE != 0, strikeout: sel & FATTR_SEL_STRIKEOUT != 0, outline: sel & FATTR_SEL_OUTLINE != 0 };
    } else {
        let fam = rd32(fntd, N_FAMILY);
        if fam != 0 {
            let (i, bold) = face_from(&cstr(fam as *const u8));
            c.family = i;
            c.bold = bold || rd32(fntd, N_STYLE) & FATTR_SEL_BOLD != 0;
        }
    }
    c.points = c.points.clamp(4, 72);
    c
}

/// Show a choice in the controls.
fn show(dlg: u32, c: &Choice) {
    list_select(dlg, DID_NAME as u32, c.family as u32);
    list_select(dlg, DID_STYLE as u32, c.bold as u32);
    set_item_text(dlg, DID_NAME as u32, FAMILIES[c.family].0);
    set_item_text(dlg, DID_STYLE as u32, STYLES[c.bold as usize]);
    let size = alloc::format!("{}", c.points);
    if let Some(i) = SIZES.iter().position(|s| *s == size.as_bytes()) {
        list_select(dlg, DID_SIZE as u32, i as u32);
    }
    set_item_text(dlg, DID_SIZE as u32, size.as_bytes());
    unsafe {
        WinSendDlgItemMsg(dlg, DID_UNDERSCORE as u32, BM_SETCHECK, c.underscore as u32, 0);
        WinSendDlgItemMsg(dlg, DID_STRIKEOUT as u32, BM_SETCHECK, c.strikeout as u32, 0);
        WinSendDlgItemMsg(dlg, DID_OUTLINE as u32, BM_SETCHECK, c.outline as u32, 0);
        WinInvalidateRect(WinWindowFromID(dlg, DID_SAMPLE as u32), core::ptr::null(), 0);
    }
}

/// Read the controls into the dialog's choice.
fn take(dlg: u32) {
    let name = item_text(dlg, DID_NAME as u32);
    let style = item_text(dlg, DID_STYLE as u32);
    let size = item_text(dlg, DID_SIZE as u32);
    let check = |id: u16| unsafe { WinSendDlgItemMsg(dlg, id as u32, BM_QUERYCHECK, 0, 0) } != 0;
    let (u, s, o) = (check(DID_UNDERSCORE), check(DID_STRIKEOUT), check(DID_OUTLINE));
    let Some(st) = state(dlg) else { return };
    if let Some(i) = FAMILIES.iter().position(|(n, _)| n.eq_ignore_ascii_case(&name)) {
        st.choice.family = i;
    }
    st.choice.bold = style.eq_ignore_ascii_case(b"Bold");
    let digits: Vec<u8> = size.iter().copied().take_while(u8::is_ascii_digit).collect();
    if !digits.is_empty() {
        let v = digits.iter().fold(0i32, |a, &d| a * 10 + (d - b'0') as i32);
        st.choice.points = v.clamp(4, 72);
    }
    (st.choice.underscore, st.choice.strikeout, st.choice.outline) = (u, s, o);
    // The FONTDLG follows the choice as it changes (an application's
    // dialog procedure reads it when OK or Apply is pressed).
    store(st.fntd, &st.choice);
    unsafe { WinInvalidateRect(WinWindowFromID(dlg, DID_SAMPLE as u32), core::ptr::null(), 0) };
}

/// Write the choice into the FONTDLG.
fn store(fntd: *mut u8, c: &Choice) {
    let a = fattrs(c);
    unsafe { core::ptr::copy_nonoverlapping(a.as_ptr(), fntd.add(N_FATTRS), 56) };
    wr32(fntd, N_POINTSIZE, (c.points as u32) << 16);
    let fam = rd32(fntd, N_FAMILY);
    let len = rd16(fntd, N_FAMILYLEN) as usize;
    if fam != 0 && len > 0 {
        let name = FAMILIES[c.family].0;
        let n = name.len().min(len - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(name.as_ptr(), fam as *mut u8, n);
            *(fam as *mut u8).add(n) = 0;
        }
    }
    let mut style = 0;
    if c.bold {
        style |= FATTR_SEL_BOLD;
    }
    if c.underscore {
        style |= FATTR_SEL_UNDERSCORE;
    }
    if c.strikeout {
        style |= FATTR_SEL_STRIKEOUT;
    }
    if c.outline {
        style |= FATTR_SEL_OUTLINE;
    }
    wr32(fntd, N_STYLE, style);
    let h = pels(c.points);
    wr32(fntd, N_EMHEIGHT, h as u32);
    wr32(fntd, N_XHEIGHT, (h / 2) as u32);
    wr32(fntd, N_LEADING, 0);
    wr16(fntd, N_NOMINAL, (c.points * 10) as u16);
    wr16(fntd, N_WEIGHT, if c.bold { 7 } else { 5 });
    wr16(fntd, N_WIDTH, 5);
}

/// The preview: the sample text in the chosen font (the sample static's
/// subclass procedure; exported for its stack-aligning entry).
#[unsafe(no_mangle)]
pub extern "C" fn Rs2FontSampleProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let dlg = states().iter().find(|s| unsafe { WinWindowFromID(s.hwnd, DID_SAMPLE as u32) } == hwnd).map(|s| s.hwnd);
    if msg != WM_PAINT || dlg.is_none() {
        let old = dlg.and_then(state).map_or(0, |s| s.sample_proc);
        if old != 0 {
            let f: extern "C" fn(u32, u32, u32, u32) -> u32 = unsafe { core::mem::transmute(old as usize) };
            return f(hwnd, msg, mp1, mp2);
        }
        return unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) };
    }
    let Some(s) = dlg.and_then(state) else { return 0 };
    let c = s.choice.clone();
    let text = s.preview.clone();
    let mut r = [0i32; 4];
    unsafe {
        let hps = WinBeginPaint(hwnd, 0, core::ptr::null_mut());
        WinQueryWindowRect(hwnd, r.as_mut_ptr());
        // CLR_WHITE; a black frame.
        WinFillRect(hps, r.as_ptr(), -2);
        let a = fattrs(&c);
        GpiCreateLogFont(hps, core::ptr::null(), 1, a.as_ptr());
        GpiSetCharSet(hps, 1);
        let h = pels(c.points);
        let bx = [((h / 2).max(1)) << 16, h << 16];
        GpiSetCharBox(hps, bx.as_ptr());
        // CLR_BLACK
        GpiSetColor(hps, -1);
        let p = [4, ((r[3] - h) / 2 + h / 4).max(2)];
        GpiCharStringAt(hps, p.as_ptr(), text.len() as i32, text.as_ptr());
        GpiSetCharSet(hps, 0);
        GpiDeleteSetId(hps, 1);
        WinEndPaint(hps);
    }
    0
}

/// The dialog's window procedure: the FONTDLG is the QWL_USER window word
/// from WM_INITDLG on; messages go to FONTDLG.pfnDlgProc if there is one,
/// else to WinDefFontDlgProc.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2FontDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_INITDLG {
        unsafe { WinSetWindowPtr(hwnd, 0, mp2) };
    }
    let fntd = unsafe { WinQueryWindowPtr(hwnd, 0) } as *const u8;
    let user = if fntd.is_null() { 0 } else { rd32(fntd, N_DLGPROC) };
    if user != 0 {
        let f: extern "C" fn(u32, u32, u32, u32) -> u32 = unsafe { core::mem::transmute(user as usize) };
        return f(hwnd, msg, mp1, mp2);
    }
    WinDefFontDlgProc(hwnd, msg, mp1, mp2)
}

/// WinDefFontDlgProc(hwnd, msg, mp1, mp2)
#[unsafe(no_mangle)]
pub extern "C" fn WinDefFontDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let fntd = mp2 as *mut u8;
            unsafe { WinSetWindowPtr(hwnd, 0, fntd as u32) };
            let c = initial(fntd);
            let pv = rd32(fntd, N_PREVIEW);
            let preview = if pv != 0 { cstr(pv as *const u8) } else { b"abcdABCD".to_vec() };
            states().retain(|s| s.hwnd != hwnd);
            states().push(State { hwnd, fntd, choice: c.clone(), initial: c.clone(), preview, sample_proc: 0 });
            let title = rd32(fntd, N_TITLE);
            if title != 0 {
                unsafe { WinSetWindowText(hwnd, title as *const u8) };
            }
            for (n, _) in FAMILIES {
                list_add(hwnd, DID_NAME as u32, n);
            }
            for s in STYLES {
                list_add(hwnd, DID_STYLE as u32, s);
            }
            for s in SIZES {
                list_add(hwnd, DID_SIZE as u32, s);
            }
            let sample = unsafe { WinWindowFromID(hwnd, DID_SAMPLE as u32) };
            let old = unsafe { WinSubclassWindow(sample, crate::exports::__a_Rs2FontSampleProc as *const () as u32) };
            if let Some(s) = state(hwnd) {
                s.sample_proc = old;
            }
            show(hwnd, &c);
            store(fntd, &c);
            0
        }
        WM_CONTROL => {
            let (id, code) = ((mp1 & 0xffff) as u16, mp1 >> 16);
            match (id, code) {
                (DID_NAME | DID_STYLE | DID_SIZE, CBN_LBSELECT | CBN_ENTER | CBN_EFCHANGE) => take(hwnd),
                // BN_CLICKED of the emphasis boxes.
                (DID_UNDERSCORE | DID_STRIKEOUT | DID_OUTLINE, 1) => take(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let id = mp1 & 0xffff;
            let Some(s) = state(hwnd) else { return 0 };
            let fntd = s.fntd;
            match id {
                DID_OK => {
                    take(hwnd);
                    if let Some(s) = state(hwnd) {
                        store(fntd, &s.choice);
                    }
                    wr32(fntd, N_RETURN, DID_OK);
                    unsafe { WinDismissDlg(hwnd, DID_OK) };
                }
                DID_CANCEL => {
                    wr32(fntd, N_RETURN, DID_CANCEL);
                    unsafe { WinDismissDlg(hwnd, DID_CANCEL) };
                }
                x if x == DID_APPLY_BUTTON as u32 => {
                    take(hwnd);
                    if let Some(s) = state(hwnd) {
                        store(fntd, &s.choice);
                    }
                }
                x if x == DID_RESET_BUTTON as u32 => {
                    let c = s.initial.clone();
                    s.choice = c.clone();
                    show(hwnd, &c);
                }
                x if x == DID_HELP_BUTTON as u32 => {
                    unsafe { WinSendMsg(hwnd, WM_HELP, mp1, mp2) };
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(hwnd, msg, mp1, mp2) },
    }
}

fn template_for(fl: u32) -> Vec<u8> {
    let st = WS_VISIBLE;
    let tab = WS_VISIBLE | WS_TABSTOP;
    let mut frame = item(WC_FRAME, b"Font", 0x1400_0000 | 0x0000_0080 | 0x0000_0010, (20, 20, 250, 170), 300);
    frame.ctl = (0x0000_0001u32 | 0x0000_0002).to_le_bytes().to_vec();
    let mut v = Vec::new();
    v.push(item(WC_STATIC, b"~Name:", st | 0x0000_2001, (6, 156, 120, 8), DID_NAME_PREFIX));
    // CBS_DROPDOWNLIST / CBS_DROPDOWN.
    v.push(item(WC_COMBOBOX, b"", tab | WS_GROUP | 0x0004, (6, 90, 120, 64), DID_NAME));
    v.push(item(WC_STATIC, b"~Style:", st | 0x0000_2001, (132, 156, 70, 8), DID_STYLE_PREFIX));
    v.push(item(WC_COMBOBOX, b"", tab | WS_GROUP | 0x0004, (132, 110, 70, 44), DID_STYLE));
    v.push(item(WC_STATIC, b"Si~ze:", st | 0x0000_2001, (208, 156, 36, 8), DID_SIZE_PREFIX));
    v.push(item(WC_COMBOBOX, b"", tab | WS_GROUP | 0x0002, (208, 90, 36, 64), DID_SIZE));
    // SS_GROUPBOX.
    v.push(item(WC_STATIC, b"Emphasis", st | 0x0002, (6, 100, 238, 34), DID_EMPHASIS_GROUPBOX));
    // BS_AUTOCHECKBOX.
    v.push(item(WC_BUTTON, b"~Outline", tab | WS_GROUP | 0x0002, (12, 106, 70, 10), DID_OUTLINE));
    v.push(item(WC_BUTTON, b"~Underline", tab | 0x0002, (88, 106, 70, 10), DID_UNDERSCORE));
    v.push(item(WC_BUTTON, b"Stri~keout", tab | 0x0002, (164, 106, 74, 10), DID_STRIKEOUT));
    v.push(item(WC_STATIC, b"Sample", st | 0x0002, (6, 26, 238, 70), DID_SAMPLE_GROUPBOX));
    // SS_TEXT painted by the dialog.
    v.push(item(WC_STATIC, b"", st | 0x0001, (12, 32, 226, 54), DID_SAMPLE));
    let mut x = 6;
    v.push(item(WC_BUTTON, b"OK", tab | WS_GROUP | 0x0400, (x, 6, 44, 14), DID_OK as u16));
    x += 48;
    v.push(item(WC_BUTTON, b"Cancel", tab, (x, 6, 44, 14), DID_CANCEL as u16));
    x += 48;
    if fl & FNTS_APPLYBUTTON != 0 {
        v.push(item(WC_BUTTON, b"~Apply", tab, (x, 6, 44, 14), DID_APPLY_BUTTON));
        x += 48;
    }
    if fl & FNTS_RESETBUTTON != 0 {
        v.push(item(WC_BUTTON, b"~Reset", tab, (x, 6, 44, 14), DID_RESET_BUTTON));
        x += 48;
    }
    if fl & FNTS_HELPBUTTON != 0 {
        v.push(item(WC_BUTTON, b"Help", tab | 0x0100 | 0x0800, (x, 6, 44, 14), DID_HELP_BUTTON));
    }
    template(frame, v)
}

/// WinFontDlg(hwndParent, hwndOwner, pfntd): the dialog (FNTS_MODELESS)
/// or TRUE after a modal one; lReturn has DID_OK or DID_CANCEL.
#[unsafe(no_mangle)]
pub extern "C" fn WinFontDlg(parent: u32, owner: u32, fntd: *mut u8) -> u32 {
    if fntd.is_null() {
        return 0;
    }
    let fl = rd32(fntd, N_FL);
    wr32(fntd, N_RETURN, 0);
    wr32(fntd, N_SRC, 0);
    let t = template_for(fl);
    let proc_ = crate::exports::__a_Rs2FontDlgProc as *const () as u32;
    let parent = if parent == 0 { 1 } else { parent };
    let dlg = unsafe { WinCreateDlg(parent, owner, proc_, t.as_ptr(), fntd as u32) };
    if dlg == 0 {
        wr32(fntd, N_SRC, 3);
        return 0;
    }
    center(dlg);
    if fl & FNTS_MODELESS != 0 {
        unsafe { WinShowWindow(dlg, 1) };
        return dlg;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    if rd32(fntd, N_RETURN) == 0 {
        wr32(fntd, N_RETURN, r);
    }
    states().retain(|s| s.hwnd != dlg);
    unsafe { WinDestroyWindow(dlg) };
    1
}
