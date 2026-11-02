//! Mahjongg's dialogs (OS/2's ids, so its help table applies; layouts
//! and texts rs2's own).

use alloc::vec::Vec;

use crate::{app, DLG_EDIT, DLG_GAMEINFO, DLG_LEVEL, DLG_NOMOVES, DLG_PRODUCT, DLG_SELECT, DLG_SETTINGS, IDM_EXIT, TITLE};
use pmkit::*;

const ID_HELP: u32 = 0x3f5;

extern "C" fn plain_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m == WM_COMMAND {
        unsafe { WinDismissDlg(h, lo(a)) };
        return 0;
    }
    unsafe { WinDefDlgProc(h, m, a, b) }
}

fn spin_set(h: u32, id: u32, lo_: i32, hi_: i32, v: i32) {
    dlg_msg(h, id, SPBM_SETLIMITS, hi_ as u32, lo_ as u32);
    dlg_msg(h, id, SPBM_SETCURRENTVALUE, v as u32, 0);
}

fn spin_get(h: u32, id: u32) -> i32 {
    let mut v: i32 = 0;
    dlg_msg(h, id, SPBM_QUERYVALUE, &mut v as *mut i32 as u32, mp(0, SPBQ_ALWAYSUPDATE));
    v
}

// ---- Select Initial Level (first start) ----

extern "C" fn level_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            set_check(h, 3901 + app().settings.hint_mode, true);
            0
        }
        WM_COMMAND => {
            let s = &mut app().settings;
            s.hint_mode = if checked(h, 3903) { 2 } else if checked(h, 3902) { 1 } else { 0 };
            unsafe { WinDismissDlg(h, lo(a)) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// How much help the player wants: free hints, subtle hints or none.
pub fn initial_level() {
    let tpl = template(
        dialog_frame(b"Choose Your Hints", DLG_LEVEL, 170, 76),
        alloc::vec![
            label(b"How much should Mahjongg help you?", (6, 60, 160, 9), DT_LEFT),
            radio(b"Show me any pair I ask for", (6, 46, 160, 10), 3901).group(),
            radio(b"Show me just one tile of a pair", (6, 35, 160, 10), 3902),
            radio(b"No hints, ever", (6, 24, 160, 10), 3903),
            button(b"~OK", (6, 4, 44), DID_OK, BS_DEFAULT).group(),
            help_button((120, 4, 44), 3904),
        ],
    );
    run_dialog(app().frame, level_proc, &tpl, 0);
}

// ---- No more moves ----

pub enum NoMoves {
    New,
    Retry,
    Exit,
    Cancel,
}

pub fn no_more_moves() -> NoMoves {
    let tpl = template(
        dialog_frame(TITLE, DLG_NOMOVES, 200, 60),
        alloc::vec![
            sys_icon(SPTR_ICONINFORMATION, (6, 30)),
            label(b"There are no pairs left to take. Deal again, start this deal over, or take moves back?", (34, 22, 160, 32), DT_LEFT | DT_WORDBREAK),
            button(b"~New", (4, 4, 44), 3541, BS_DEFAULT),
            button(b"~Retry", (52, 4, 44), 3542, 0),
            button(b"e~Xit", (100, 4, 44), IDM_EXIT as u32, 0),
            button(b"~Cancel", (148, 4, 44), DID_CANCEL, 0),
        ],
    );
    match run_dialog(app().frame, plain_proc, &tpl, 0) {
        3541 => NoMoves::New,
        3542 => NoMoves::Retry,
        x if x == IDM_EXIT as u32 => NoMoves::Exit,
        _ => NoMoves::Cancel,
    }
}

// ---- Select Game ----

static mut SELECT: u32 = 1;

extern "C" fn select_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            spin_set(h, 3911, 1, 30000, unsafe { SELECT } as i32);
            0
        }
        WM_COMMAND => {
            if lo(a) == DID_OK {
                unsafe { SELECT = spin_get(h, 3911).clamp(1, 30000) as u32 };
            }
            unsafe { WinDismissDlg(h, lo(a)) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// Choose a game number.
pub fn select_game(current: u32) -> Option<u32> {
    unsafe { SELECT = current.clamp(1, 30000) };
    let tpl = template(
        dialog_frame(b"Select Game", DLG_SELECT, 140, 46),
        alloc::vec![
            label(b"Game number:", (4, 28, 50, 8), DT_LEFT),
            spin((56, 26, 78, 12), 3911),
            button(b"~OK", (4, 4, 40), DID_OK, BS_DEFAULT),
            button(b"~Cancel", (48, 4, 40), DID_CANCEL, 0),
            help_button((92, 4, 44), 3912),
        ],
    );
    (run_dialog(app().frame, select_proc, &tpl, 0) == DID_OK).then(|| unsafe { SELECT })
}

// ---- Settings ----

const ID_BEEP: u32 = 3612;
const ID_SOLVABLE: u32 = 3613;
const ID_AUTOHINTS: u32 = 3614;
const ID_VIEW_RIGHT: u32 = 3618;
const ID_VIEW_LEFT: u32 = 3619;
const ID_DEFAULT: u32 = 3621;
const ID_SPEED: u32 = 3611;
const ID_HINT0: u32 = 3901;
const ID_USECOLOR: u32 = 3641;
const ID_SAMPLE: u32 = 3642;
const ID_R: u32 = 3643;
const ID_G: u32 = 3644;
const ID_B: u32 = 3645;

const SPEEDS: [&[u8]; 3] = [b"Slow", b"Medium", b"Fast"];

fn settings_to_dialog(h: u32, s: &crate::Settings) {
    set_check(h, ID_BEEP, s.beep);
    set_check(h, ID_SOLVABLE, s.solvable);
    set_check(h, ID_AUTOHINTS, s.auto_hints);
    set_check(h, ID_VIEW_RIGHT, s.view3d == 0);
    set_check(h, ID_VIEW_LEFT, s.view3d == 1);
    for k in 0..3 {
        set_check(h, ID_HINT0 + k, s.hint_mode == k);
    }
    dlg_msg(h, ID_SPEED, LM_SELECTITEM, s.speed, 1);
    set_check(h, ID_USECOLOR, s.use_back);
    spin_set(h, ID_R, 0, 255, (s.back >> 16) as i32);
    spin_set(h, ID_G, 0, 255, ((s.back >> 8) & 0xff) as i32);
    spin_set(h, ID_B, 0, 255, (s.back & 0xff) as i32);
}

fn dialog_to_settings(h: u32, s: &mut crate::Settings) {
    s.beep = checked(h, ID_BEEP);
    s.solvable = checked(h, ID_SOLVABLE);
    s.auto_hints = checked(h, ID_AUTOHINTS);
    s.view3d = checked(h, ID_VIEW_LEFT) as u32;
    s.hint_mode = (0..3).find(|&k| checked(h, ID_HINT0 + k)).unwrap_or(0);
    let sp = dlg_msg(h, ID_SPEED, LM_QUERYSELECTION, LIT_FIRST, 0);
    if sp < 3 {
        s.speed = sp;
    }
    s.use_back = checked(h, ID_USECOLOR);
    s.back = (spin_get(h, ID_R).clamp(0, 255) as u32) << 16 | (spin_get(h, ID_G).clamp(0, 255) as u32) << 8 | spin_get(h, ID_B).clamp(0, 255) as u32;
}

extern "C" fn sample_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m == WM_PAINT {
        let mut r = RectL::default();
        let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
        pmkit::gpi::rgb_mode(hps);
        let rr = window_rect(h);
        let c = unsafe { WinQueryWindowULong(h, QWL_USER) };
        pmkit::gpi::fill(hps, rr, c as i32);
        pmkit::gpi::frame(hps, rr, 0);
        unsafe { WinEndPaint(hps) };
        return 0;
    }
    unsafe { WinDefWindowProc(h, m, a, b) }
}

fn update_sample(h: u32) {
    let mut s = app().settings;
    dialog_to_settings(h, &mut s);
    let w = item(h, ID_SAMPLE);
    unsafe { WinSetWindowULong(w, QWL_USER, s.back) };
    invalidate(w);
}

extern "C" fn settings_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            for s in SPEEDS {
                lb_add(h, ID_SPEED, s);
            }
            settings_to_dialog(h, &app().settings);
            update_sample(h);
            0
        }
        WM_CONTROL => {
            if matches!(lo(a), ID_R | ID_G | ID_B) && hi(a) == SPBN_CHANGE {
                update_sample(h);
            }
            0
        }
        WM_COMMAND => {
            match lo(a) {
                ID_DEFAULT => {
                    settings_to_dialog(h, &crate::Settings::DEFAULT);
                    update_sample(h);
                }
                DID_OK => {
                    let mut s = app().settings;
                    dialog_to_settings(h, &mut s);
                    app().settings = s;
                    unsafe { WinDismissDlg(h, DID_OK) };
                }
                id => unsafe {
                    WinDismissDlg(h, id);
                },
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// Options > Settings; true when changed.
pub fn settings() -> bool {
    let a = app();
    unsafe { WinRegisterClass(a.hab, b"MahjonggSample\0".as_ptr(), sample_proc, 0, 4) };
    let before = a.settings;
    let tpl = template(
        dialog_frame(b"Mahjongg Settings", DLG_SETTINGS, 280, 186),
        alloc::vec![
            group_box(b"Play", (4, 92, 150, 90)),
            check_box(b"Warning beeps", (10, 158, 140, 10), ID_BEEP),
            check_box(b"Deal only solvable games", (10, 146, 140, 10), ID_SOLVABLE),
            check_box(b"Show matching tiles", (10, 134, 140, 10), ID_AUTOHINTS),
            label(b"Hints:", (10, 122, 40, 9), DT_LEFT),
            radio(b"Pairs", (14, 110, 60, 10), ID_HINT0).group(),
            radio(b"One tile", (14, 100, 60, 10), ID_HINT0 + 1),
            radio(b"None", (80, 110, 60, 10), ID_HINT0 + 2),
            group_box(b"Tiles seen from", (160, 132, 116, 50)),
            radio(b"Lower right", (166, 158, 100, 10), ID_VIEW_RIGHT).group(),
            radio(b"Lower left", (166, 146, 100, 10), ID_VIEW_LEFT),
            label(b"Auto play speed:", (166, 120, 100, 9), DT_LEFT),
            list_box((166, 92, 100, 26), ID_SPEED, 0).group(),
            group_box(b"Background", (4, 22, 272, 66)),
            check_box(b"Use this color for every game", (10, 66, 180, 10), ID_USECOLOR),
            label(b"Red:", (10, 50, 26, 8), DT_LEFT),
            spin((38, 48, 40, 12), ID_R),
            label(b"Green:", (84, 50, 30, 8), DT_LEFT),
            spin((116, 48, 40, 12), ID_G),
            label(b"Blue:", (162, 50, 26, 8), DT_LEFT),
            spin((190, 48, 40, 12), ID_B),
            custom(b"MahjonggSample", (236, 30, 34, 44), ID_SAMPLE, 0),
            label(b"(each game file has its own color)", (10, 30, 200, 9), DT_LEFT),
            button(b"~OK", (4, 4, 44), DID_OK, BS_DEFAULT).group(),
            button(b"~Cancel", (52, 4, 44), DID_CANCEL, 0),
            button(b"~Default", (100, 4, 44), ID_DEFAULT, 0),
            help_button((232, 4, 44), ID_HELP),
        ],
    );
    run_dialog(a.frame, settings_proc, &tpl, 0);
    a.settings != before
}

// ---- Game information ----

extern "C" fn info_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    let l = &mut app().layout;
    match m {
        WM_INITDLG => {
            for (id, v) in [(3721, &l.name), (3722, &l.author), (3723, &l.about), (3724, &l.help)] {
                dlg_msg(h, id, EM_SETTEXTLIMIT, 80, 0);
                set_item_text(h, id, v);
            }
            0
        }
        WM_COMMAND => {
            if lo(a) == DID_OK {
                let v: Vec<Vec<u8>> = [3721, 3722, 3723, 3724].iter().map(|&id| item_text(h, id)).collect();
                if v[0] != l.name || v[1] != l.author || v[2] != l.about || v[3] != l.help {
                    l.name = v[0].clone();
                    l.author = v[1].clone();
                    l.about = v[2].clone();
                    l.help = v[3].clone();
                    app().layout_changed = true;
                    crate::set_title();
                }
            }
            unsafe { WinDismissDlg(h, lo(a)) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// Help > Game information: the game file's title, author, comment and
/// help file, which can be changed (and saved with the game).
pub fn game_info() {
    let tpl = template(
        dialog_frame(b"Game Information", DLG_GAMEINFO, 212, 124),
        alloc::vec![
            label(b"Game title:", (8, 110, 80, 8), DT_LEFT),
            entry((10, 100, 194, 8), 3721, 0),
            label(b"Written by:", (8, 88, 80, 8), DT_LEFT),
            entry((10, 78, 194, 8), 3722, 0),
            label(b"Comment:", (8, 66, 80, 8), DT_LEFT),
            entry((10, 56, 194, 8), 3723, 0),
            label(b"Help file:", (8, 44, 80, 8), DT_LEFT),
            entry((10, 34, 194, 8), 3724, 0),
            button(b"~OK", (4, 4, 44), DID_OK, BS_DEFAULT),
            button(b"~Cancel", (52, 4, 44), DID_CANCEL, 0),
            help_button((162, 4, 44), ID_HELP),
        ],
    );
    run_dialog(app().frame, info_proc, &tpl, 0);
}

// ---- Product information ----

pub fn product_info() {
    let l = &app().layout;
    let name = if l.name.is_empty() { b"(Untitled)".to_vec() } else { l.name.clone() };
    let game = [&b"Current game: "[..], &name].concat();
    let by = [&b"Written by: "[..], &l.author].concat();
    let tpl = template(
        dialog_frame(b"Product Information", DLG_PRODUCT, 240, 100),
        alloc::vec![
            sys_icon(SPTR_APPICON, (8, 70)),
            label(b"Mahjongg Solitaire", (40, 82, 190, 10), DT_LEFT),
            label(b"rs2's own version, for OS/2 Warp 4.52 compatible systems.", (40, 70, 196, 9), DT_LEFT),
            label(&game, (8, 50, 226, 9), DT_LEFT),
            label(&by, (8, 40, 226, 9), DT_LEFT),
            label(&l.about, (8, 30, 226, 9), DT_LEFT),
            button(b"~OK", (98, 6, 44), DID_OK, BS_DEFAULT),
        ],
    );
    run_dialog(app().frame, plain_proc, &tpl, 0);
}

// ---- Open and Save as ----

const FILEDLG_SIZE: usize = 328;
const FDS_CENTER: u32 = 0x0001;
const FDS_OPEN_DIALOG: u32 = 0x0100;
const FDS_SAVEAS_DIALOG: u32 = 0x0200;
const FDS_ENABLEFILELB: u32 = 0x0800;

/// The standard file dialog for game files.
pub fn file_dialog(save: bool) -> Option<Vec<u8>> {
    let a = app();
    let mut fd = alloc::vec![0u8; FILEDLG_SIZE];
    wr32(&mut fd, 0, FILEDLG_SIZE as u32);
    wr32(&mut fd, 4, FDS_CENTER | if save { FDS_SAVEAS_DIALOG | FDS_ENABLEFILELB } else { FDS_OPEN_DIALOG });
    let (title, ok): (&[u8], &[u8]) = if save { (b"Save a Mahjongg Game File\0", b"~Save\0") } else { (b"Open a Mahjongg Game File\0", b"~Open\0") };
    wr32(&mut fd, 20, title.as_ptr() as u32);
    wr32(&mut fd, 24, ok.as_ptr() as u32);
    let initial: Vec<u8> = match (&a.path, save) {
        (Some(p), true) => p.clone(),
        _ => b"*.MAH".to_vec(),
    };
    let n = initial.len().min(259);
    fd[52..52 + n].copy_from_slice(&initial[..n]);
    fd[326..328].copy_from_slice(&(-1i16).to_le_bytes());
    let ok = unsafe { WinFileDlg(HWND_DESKTOP, a.frame, fd.as_mut_ptr()) } != 0;
    if !ok || rd32(&fd, 12) != DID_OK {
        return None;
    }
    let name = until_nul(&fd[52..312]).to_vec();
    (!name.is_empty()).then_some(name)
}

// ---- Edit mode ----

const ID_DX: u32 = 3701;
const ID_DY: u32 = 3702;
const ID_LEVEL: u32 = 3703;
const ID_RESET: u32 = 3705;
const ID_EDITHELP: u32 = 3706;

extern "C" fn edit_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    let app_ = app();
    match m {
        WM_INITDLG => {
            spin_set(h, ID_DX, 2, 30, app_.layout.dx);
            spin_set(h, ID_DY, 2, 20, app_.layout.dy);
            spin_set(h, ID_LEVEL, 0, 9, app_.edit_level);
            0
        }
        WM_CONTROL => {
            if hi(a) == SPBN_CHANGE {
                let l = &mut app_.layout;
                match lo(a) {
                    ID_DX => l.dx = spin_get(h, ID_DX).clamp(2, 30),
                    ID_DY => l.dy = spin_get(h, ID_DY).clamp(2, 20),
                    ID_LEVEL => app_.edit_level = spin_get(h, ID_LEVEL).clamp(0, 9),
                    _ => return 0,
                }
                app_.back = None;
                invalidate(app_.client);
            }
            0
        }
        WM_COMMAND => {
            match lo(a) {
                ID_RESET => {
                    app_.layout.pos.clear();
                    invalidate(app_.client);
                }
                // Ended from the client (this window goes away then).
                DID_OK => {
                    post(app_.client, crate::WM_LEAVE_EDIT, 1, 0);
                }
                DID_CANCEL => {
                    post(app_.client, crate::WM_LEAVE_EDIT, 0, 0);
                }
                _ => {}
            }
            0
        }
        WM_HELP => {
            crate::help(HM_DISPLAY_HELP, DLG_EDIT as u32, HM_RESOURCEID);
            0
        }
        WM_CLOSE => {
            post(app_.client, crate::WM_LEAVE_EDIT, 0, 0);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// The modeless Edit Mode window beside the board.
pub fn edit_dialog() -> u32 {
    let a = app();
    let tpl = template(
        dialog_frame(b"Edit Mode", DLG_EDIT, 80, 150),
        alloc::vec![
            group_box(b"Board size", (2, 100, 76, 40)),
            label(b"Across:", (6, 120, 30, 8), DT_LEFT),
            spin((38, 118, 34, 12), ID_DX),
            label(b"Down:", (6, 106, 30, 8), DT_LEFT),
            spin((38, 104, 34, 12), ID_DY),
            label(b"Level:", (6, 88, 30, 8), DT_LEFT),
            spin((38, 86, 34, 12), ID_LEVEL),
            button(b"~Reset", (6, 68, 68), ID_RESET, 0),
            pmkit::tpl::item(WC_BUTTON, b"~Help", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON | BS_HELP | BS_NOPOINTERFOCUS, (6, 36, 68, 14), ID_EDITHELP as u16),
            button(b"~Cancel", (6, 20, 68), DID_CANCEL, 0),
            button(b"~OK", (6, 4, 68), DID_OK, BS_DEFAULT),
        ],
    );
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, a.frame, edit_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        let f = window_pos(a.frame);
        let d = window_pos(dlg);
        unsafe {
            WinSetWindowPos(dlg, HWND_TOP, (f.x - d.cx).max(0), f.y + (f.cy - d.cy) / 2, 0, 0, SWP_MOVE | SWP_SHOW | SWP_ZORDER);
        }
    }
    dlg
}
