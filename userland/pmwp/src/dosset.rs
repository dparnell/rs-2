//! WPProgram's DOS settings: the "DOS properties" dialog of the Session
//! page (laid out after IBM's: the settings in a list, the selected one's
//! value as an entry field, On/Off buttons, a choice list or a multi-line
//! field by its kind, and its description; Save, Cancel and Default).
//!
//! The settings are kept as IBM's shell keeps them: `NAME=value` strings
//! in the program's environment (PROGDETAILS pszEnvironment, the `SET
//! NAME=value` setup strings), only those that differ from the default.
//! DosStartSession hands them to VDM.EXE in the session's environment,
//! where `dos::Config::set` applies them (docs/DOS.md).

use alloc::vec::Vec;

use crate::pm::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Num(u32, u32),
    Choice(&'static [&'static [u8]]),
    Text,
    /// Several lines (DOS_DEVICE, DOS_VERSION).
    Lines,
}

pub struct Setting {
    pub name: &'static [u8],
    pub kind: Kind,
    pub default: &'static [u8],
    pub text: &'static [u8],
}

const fn s(name: &'static [u8], kind: Kind, default: &'static [u8], text: &'static [u8]) -> Setting {
    Setting { name, kind, default, text }
}

/// Settings rs2's DOS sessions keep but do not act on yet say so.
const KEPT: &[u8] = b" (Kept for compatibility; rs2 does not use it yet.)";

/// OS/2's DOS settings, in IBM's order (alphabetical).
pub static SETTINGS: &[Setting] = &[
    s(b"DOS_BACKGROUND_EXECUTION", Kind::Bool, b"1", b"Whether the program keeps running while its session is in the background."),
    s(b"DOS_BREAK", Kind::Bool, b"0", b"On: DOS checks for Ctrl+Break at every DOS call, not only during input and output."),
    s(b"DOS_DEVICE", Kind::Lines, b"", b"DOS device drivers loaded into the session, one per line, as CONFIG.SYS DEVICE= lines."),
    s(b"DOS_FCBS", Kind::Num(0, 255), b"16", b"File control blocks DOS keeps open for programs that use them."),
    s(b"DOS_FCBS_KEEP", Kind::Num(0, 255), b"8", b"File control blocks that are never closed to make room for others."),
    s(b"DOS_FILES", Kind::Num(20, 255), b"20", b"Files the session can have open at the same time."),
    s(b"DOS_HIGH", Kind::Bool, b"1", b"On: DOS is loaded into the high memory area, leaving more conventional memory for programs."),
    s(b"DOS_LASTDRIVE", Kind::Text, b"Z", b"The last drive letter DOS programs can use."),
    s(b"DOS_RMSIZE", Kind::Num(128, 640), b"640", b"Conventional memory of the session in KB (128 to 640)."),
    s(b"DOS_SHELL", Kind::Text, b"C:\\OS2\\MDOS\\COMMAND.COM", b"The command processor of the session and its parameters."),
    s(b"DOS_STARTUP_DRIVE", Kind::Text, b"", b"A drive or diskette image to start a specific DOS version from; empty for OS/2's DOS."),
    s(b"DOS_UMB", Kind::Bool, b"1", b"On: DOS manages the upper memory blocks, so DEVICEHIGH and LOADHIGH can use them."),
    s(b"DOS_VERSION", Kind::Lines, b"", b"The DOS version reported to particular programs, one per line: NAME.EXE,major,minor,count."),
    s(b"DPMI_DOS_API", Kind::Choice(&[b"AUTO", b"ENABLED", b"DISABLED"]), b"AUTO", b"Whether protected-mode (DPMI) programs get the DOS calls translated for them."),
    s(b"DPMI_MEMORY_LIMIT", Kind::Num(0, 512), b"3", b"Protected-mode (DPMI) memory for the session in MB."),
    s(b"DPMI_NETWORK_BUFF_SIZE", Kind::Num(1, 64), b"8", b"Size of the network buffer for protected-mode programs in KB."),
    s(b"EMS_FRAME_LOCATION", Kind::Choice(&[b"AUTO", b"NONE", b"C000", b"C400", b"C800", b"CC00", b"D000", b"D400", b"D800", b"DC00", b"E000"]), b"AUTO", b"Where the expanded memory page frame is."),
    s(b"EMS_HIGH_OS_MAP_REGION", Kind::Num(0, 96), b"32", b"Expanded memory that can be mapped above 640 KB, in KB."),
    s(b"EMS_LOW_OS_MAP_REGION", Kind::Num(0, 576), b"384", b"Expanded memory that can be mapped below 640 KB, in KB."),
    s(b"EMS_MEMORY_LIMIT", Kind::Num(0, 32768), b"2048", b"Expanded (EMS) memory for the session in KB; 0 for none."),
    s(b"HW_NOSOUND", Kind::Bool, b"0", b"On: the session makes no sound (PC speaker, Sound Blaster and FM music are silent)."),
    s(b"HW_ROM_TO_RAM", Kind::Bool, b"0", b"On: the BIOS ROM is copied to RAM."),
    s(b"HW_TIMER", Kind::Bool, b"0", b"On: the program may program the timer directly for more exact timing."),
    s(b"IDLE_SECONDS", Kind::Num(0, 60), b"0", b"Seconds to wait before a program that polls the keyboard is treated as idle."),
    s(b"IDLE_SENSITIVITY", Kind::Num(1, 100), b"75", b"How readily a program polling for input is treated as idle (higher: sooner), so it does not use the processor while it waits."),
    s(b"INT_DURING_IO", Kind::Bool, b"0", b"On: interrupts reach the program during disk input and output."),
    s(b"KBD_ALTHOME_BYPASS", Kind::Bool, b"0", b"On: Alt+Home goes to the program instead of switching between window and full screen."),
    s(b"KBD_BUFFER_EXTEND", Kind::Bool, b"1", b"On: the keyboard buffer holds more keys than the BIOS's 16."),
    s(b"KBD_CTRL_BYPASS", Kind::Choice(&[b"NONE", b"ALT_ESC", b"CTRL_ESC"]), b"NONE", b"A system key combination that goes to the program instead."),
    s(b"KBD_RATE_LOCK", Kind::Bool, b"0", b"On: the program cannot change the keyboard repeat rate."),
    s(b"MEM_EXCLUDE_REGIONS", Kind::Text, b"", b"Upper memory ranges (C0000-C7FFF, ...) that EMS and XMS must not use."),
    s(b"MEM_INCLUDE_REGIONS", Kind::Text, b"", b"Upper memory ranges EMS and XMS may use although they seem taken."),
    s(b"MOUSE_EXCLUSIVE_ACCESS", Kind::Bool, b"0", b"On: in a window, the mouse belongs to the program until Alt, Esc or Ctrl is pressed."),
    s(b"PRINT_TIMEOUT", Kind::Num(0, 3600), b"15", b"Seconds after the program's last output to the printer that the print job ends."),
    s(b"VIDEO_FASTPASTE", Kind::Bool, b"0", b"On: text pasted into the session is typed faster."),
    s(b"VIDEO_MODE_RESTRICTION", Kind::Choice(&[b"NONE", b"CGA", b"MONO"]), b"NONE", b"Restricts the video modes to CGA or monochrome, for more conventional memory."),
    s(b"VIDEO_ONDEMAND_MEMORY", Kind::Bool, b"1", b"On: memory to save the video memory is allocated when the session switches."),
    s(b"VIDEO_RETRACE_EMULATION", Kind::Bool, b"1", b"On: the video status port's retrace bit is emulated."),
    s(b"VIDEO_ROM_EMULATION", Kind::Bool, b"1", b"On: the video BIOS's text output is emulated for speed."),
    s(b"VIDEO_SWITCH_NOTIFICATION", Kind::Bool, b"0", b"On: the program is told when its session is switched, to redraw its screen."),
    s(b"VIDEO_WINDOW_REFRESH", Kind::Num(1, 600), b"1", b"How often a windowed session's picture is redrawn, in tenths of a second."),
    s(b"XMS_HANDLES", Kind::Num(0, 128), b"32", b"Extended memory blocks a program can have at the same time."),
    s(b"XMS_MEMORY_LIMIT", Kind::Num(0, 65535), b"3072", b"Extended (XMS) memory for the session in KB; 0 for none."),
    s(b"XMS_MINIMUM_HMA", Kind::Num(0, 63), b"0", b"The smallest request in KB that gets the high memory area."),
];

/// Settings rs2's DOS machine applies (crate dos's Config::set and
/// VDM.EXE); the others are kept for compatibility.
const APPLIED: &[&[u8]] = &[
    b"DOS_BREAK", b"DOS_HIGH", b"DOS_RMSIZE", b"DOS_SHELL", b"DOS_UMB", b"DOS_VERSION", b"DPMI_DOS_API", b"DPMI_MEMORY_LIMIT",
    b"EMS_MEMORY_LIMIT", b"HW_NOSOUND", b"IDLE_SENSITIVITY", b"XMS_MEMORY_LIMIT",
];

pub fn find(name: &[u8]) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// Whether an environment string ("NAME=value") is a DOS setting.
pub fn is_setting(entry: &[u8]) -> bool {
    let n = entry.split(|&c| c == b'=').next().unwrap_or(b"");
    find(n).is_some()
}

/// A value as the setting stores it (booleans as 1/0, choices upper
/// case, numbers within range); None when it is not valid.
fn normalize(s: &Setting, v: &[u8]) -> Option<Vec<u8>> {
    let t = v.trim_ascii();
    match s.kind {
        Kind::Bool => match t.to_ascii_uppercase().as_slice() {
            b"1" | b"ON" | b"YES" => Some(b"1".to_vec()),
            b"0" | b"OFF" | b"NO" => Some(b"0".to_vec()),
            _ => None,
        },
        Kind::Num(lo, hi) => {
            let n = crate::util::parse_dec(t)?;
            Some(alloc::format!("{}", n.clamp(lo, hi)).into_bytes())
        }
        Kind::Choice(list) => list.iter().find(|c| c.eq_ignore_ascii_case(t)).map(|c| c.to_vec()),
        Kind::Text => Some(t.to_vec()),
        Kind::Lines => {
            // Lines as the environment carries them: separated by LF.
            let lines: Vec<&[u8]> = v.split(|&c| c == b'\n' || c == b'\r').map(|l| l.trim_ascii()).filter(|l| !l.is_empty()).collect();
            Some(lines.join(&b'\n'))
        }
    }
}

// ---- The dialog ----

const ID_LIST: u16 = 880;
const ID_VALUE: u16 = 881;
const ID_ON: u16 = 882;
const ID_OFF: u16 = 883;
const ID_CHOICES: u16 = 884;
const ID_LINES: u16 = 885;
const ID_DESC: u16 = 886;
const ID_DEFAULT: u16 = 887;
const ID_RANGE: u16 = 888;
const WC_MLE: u16 = 10;

/// The values being edited (one per setting) and the selected one.
struct Edit {
    values: Vec<Vec<u8>>,
    sel: usize,
}

static mut EDIT: Option<Edit> = None;

fn edit() -> &'static mut Edit {
    unsafe { (*&raw mut EDIT).as_mut().unwrap() }
}

fn show(h: u32, id: u16, on: bool) {
    unsafe { WinShowWindow(WinWindowFromID(h, id as u32), on as u32) };
}

fn send_item(h: u32, id: u16, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendDlgItemMsg(h, id as u32, m, a, b) }
}

/// Take the shown value into the edit state.
fn take(h: u32) {
    let e = edit();
    let st = &SETTINGS[e.sel];
    let v = match st.kind {
        Kind::Bool => {
            if send_item(h, ID_ON, BM_QUERYCHECK, 0, 0) != 0 { b"1".to_vec() } else { b"0".to_vec() }
        }
        Kind::Choice(list) => {
            let i = send_item(h, ID_CHOICES, LM_QUERYSELECTION, 0xffff, 0) as i32;
            if i >= 0 && (i as usize) < list.len() { list[i as usize].to_vec() } else { e.values[e.sel].clone() }
        }
        Kind::Lines => item_text(h, ID_LINES as u32),
        _ => item_text(h, ID_VALUE as u32),
    };
    if let Some(v) = normalize(st, &v) {
        e.values[e.sel] = v;
    }
}

/// Show setting `i`'s value and description.
fn present(h: u32, i: usize) {
    let e = edit();
    e.sel = i;
    let st = &SETTINGS[i];
    let v = e.values[i].clone();
    let k = st.kind;
    show(h, ID_VALUE, matches!(k, Kind::Num(..) | Kind::Text));
    show(h, ID_ON, k == Kind::Bool);
    show(h, ID_OFF, k == Kind::Bool);
    show(h, ID_CHOICES, matches!(k, Kind::Choice(_)));
    show(h, ID_LINES, k == Kind::Lines);
    match k {
        Kind::Bool => {
            send_item(h, ID_ON, BM_SETCHECK, (v == b"1") as u32, 0);
            send_item(h, ID_OFF, BM_SETCHECK, (v != b"1") as u32, 0);
        }
        Kind::Choice(list) => {
            send_item(h, ID_CHOICES, LM_DELETEALL, 0, 0);
            for c in list {
                let t = z(c);
                send_item(h, ID_CHOICES, LM_INSERTITEM, LIT_END, t.as_ptr() as u32);
            }
            let at = list.iter().position(|c| c.eq_ignore_ascii_case(&v)).unwrap_or(0);
            send_item(h, ID_CHOICES, LM_SELECTITEM, at as u32, 1);
        }
        Kind::Lines => {
            // The MLE shows lines ending in CR LF.
            let t: Vec<u8> = v.split(|&c| c == b'\n').collect::<Vec<_>>().join(&b"\r\n"[..]);
            set_item_text(h, ID_LINES as u32, &t);
        }
        _ => set_item_text(h, ID_VALUE as u32, &v),
    }
    let range = match k {
        Kind::Num(lo, hi) => alloc::format!("{lo} to {hi} (default {})", core::str::from_utf8(st.default).unwrap_or("")).into_bytes(),
        Kind::Bool => alloc::format!("Default: {}", if st.default == b"1" { "On" } else { "Off" }).into_bytes(),
        _ if st.default.is_empty() => Vec::new(),
        _ => alloc::format!("Default: {}", core::str::from_utf8(st.default).unwrap_or("")).into_bytes(),
    };
    set_item_text(h, ID_RANGE as u32, &range);
    let mut d = st.text.to_vec();
    if !APPLIED.iter().any(|a| *a == st.name) {
        d.extend_from_slice(KEPT);
    }
    set_item_text(h, ID_DESC as u32, &d);
}

extern "C" fn dlg_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            center(h);
            for st in SETTINGS {
                let t = z(st.name);
                send_item(h, ID_LIST, LM_INSERTITEM, LIT_END, t.as_ptr() as u32);
            }
            send_item(h, ID_VALUE, EM_SETTEXTLIMIT, 254, 0);
            send_item(h, ID_LIST, LM_SELECTITEM, 0, 1);
            present(h, 0);
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_LIST as u32)) };
            1
        }
        WM_CONTROL if a & 0xffff == ID_LIST as u32 && a >> 16 == LN_SELECT => {
            let i = send_item(h, ID_LIST, LM_QUERYSELECTION, 0xffff, 0) as i32;
            if i >= 0 && (i as usize) < SETTINGS.len() && i as usize != edit().sel {
                take(h);
                present(h, i as usize);
            }
            0
        }
        WM_COMMAND => {
            match (a & 0xffff) as u16 {
                ID_DEFAULT => {
                    let e = edit();
                    e.values[e.sel] = SETTINGS[e.sel].default.to_vec();
                    present(h, e.sel);
                }
                id if id as u32 == DID_OK => {
                    take(h);
                    unsafe { WinDismissDlg(h, DID_OK) };
                }
                _ => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn label(text: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // SS_TEXT | DT_LEFT
    item(WC_STATIC, text, WS_VISIBLE | 0x0001, (x, y, cx, 8), id)
}

/// Edit `settings` ("NAME=value" strings that differ from the defaults):
/// the new list when the user saved.
pub fn run(owner: u32, title: &[u8], settings: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    let mut values: Vec<Vec<u8>> = SETTINGS.iter().map(|s| s.default.to_vec()).collect();
    for e in settings {
        let Some(eq) = e.iter().position(|&c| c == b'=') else { continue };
        if let Some(i) = SETTINGS.iter().position(|s| s.name.eq_ignore_ascii_case(&e[..eq])) {
            if let Some(v) = normalize(&SETTINGS[i], &e[eq + 1..]) {
                values[i] = v;
            }
        }
    }
    unsafe { EDIT = Some(Edit { values, sel: 0 }) };
    let mut t = b"DOS Properties - ".to_vec();
    t.extend(title.iter().map(|&c| if c == b'\n' || c == b'\r' { b' ' } else { c }));
    // WS_GROUP on the radio buttons' first; BS_AUTORADIOBUTTON.
    let radio = |text: &[u8], x: i16, id: u16, first: bool| item(WC_BUTTON, text, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (x, 134, 44, 10), id);
    let tpl = template(dialog_frame(&t, 316, 180), alloc::vec![
        label(b"Settings", (6, 166, 140), 0xffff),
        // LS_NOADJUSTPOS
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004, (6, 24, 146, 140), ID_LIST),
        label(b"Value", (160, 166, 150), 0xffff),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (162, 150, 146, 8), ID_VALUE),
        radio(b"~On", 162, ID_ON, true),
        radio(b"O~ff", 212, ID_OFF, false),
        item(WC_LISTBOX, b"", WS_TABSTOP | WS_GROUP, (160, 98, 150, 62), ID_CHOICES),
        // MLS_BORDER | MLS_VSCROLL | MLS_HSCROLL
        item(WC_MLE, b"", WS_TABSTOP | WS_GROUP | 0x0002 | 0x0004 | 0x0008, (160, 98, 150, 62), ID_LINES),
        label(b"", (160, 86, 150), ID_RANGE),
        label(b"Description", (160, 72, 150), 0xffff),
        // SS_TEXT | DT_LEFT | DT_TOP | DT_WORDBREAK
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001 | 0x4000, (160, 24, 150, 46), ID_DESC),
        item(WC_BUTTON, b"~Save", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 54, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 54, 14), DID_CANCEL as u16),
        item(WC_BUTTON, b"~Default", WS_VISIBLE | WS_TABSTOP, (122, 4, 54, 14), ID_DEFAULT),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, dlg_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return None;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    let e = unsafe { (*&raw mut EDIT).take()? };
    if r != DID_OK {
        return None;
    }
    let mut out = Vec::new();
    for (st, v) in SETTINGS.iter().zip(e.values) {
        if v.as_slice() != st.default {
            let mut s = st.name.to_vec();
            s.push(b'=');
            s.extend_from_slice(&v);
            out.push(s);
        }
    }
    Some(out)
}
