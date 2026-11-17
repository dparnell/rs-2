//! WPCONFIG's Power object (WPPower, `<WP_POWER>`), rs2's own
//! implementation of the Toolkit's interface (wppower.idl: its methods in
//! IBM's release order and its instance data first, as IBM lays it out).
//!
//! IBM's drives APM (APM.SYS, `\DEV\APM$`); rs2 has ACPI instead, and
//! what it can do is turn the computer off (ACPI S5, through the shell's
//! shutdown) — it cannot suspend. So:
//!
//! - the Power page turns power management on or off and says whether
//!   power state changes are confirmed (wpSet/QueryPowerManagement,
//!   wpSet/QueryPowerConfirmation; kept with the object and in
//!   `PM_ControlPanel` `APM_BIOS_Enabled`, IBM's key);
//! - the View page chooses whether the status view refreshes itself and
//!   how often (wpSet/QueryAutoRefresh, wpSet/QueryRefreshRate);
//! - the Plan page (rs2's own) chooses the processor power plan
//!   (Performance, Balanced, Power saver: the kernel's cpupower.rs through
//!   `Rs2PowerControl`); a choice takes effect at once and is written to
//!   CONFIG.SYS (`POWERPLAN=`) for the next boot, as POWERCFG /SET /SAVE;
//! - opening the object shows its status view (OPEN_STATUS): the power
//!   source, the battery (none: rs2 does not read ACPI batteries) and
//!   what power management can do;
//! - with power management on, its pop-up menu has Power off (wpPowerOff:
//!   asks when confirmations are on, then shuts down and turns the
//!   computer off) and Suspend, which says the computer cannot suspend
//!   (wpSuspend returns FALSE); wpChangePowerState with APM's states does
//!   the same (3 off, 2 suspend).

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio, spin, spin_init, spin_value};
use crate::som;
use crate::syssetup::{checkbox, group, text, undo_default, ID_DEFAULT, ID_UNDO, SPBN_CHANGE};
use crate::util::{prf_write, HINI_USER};
use crate::wps::{self, Impl};

pub const OPEN_STATUS: u32 = 131;
pub const OPEN_BATTERY: u32 = 132;
pub const APMLEVEL_NONE: u32 = 0;

/// WPPower's instance data: IBM's variables (wppower.idl), then rs2's.
#[repr(C)]
pub struct PowerData {
    apm_level: u32,
    bios_pm: u32,
    os2_pm: u32,
    timer_requests: u32,
    resume_on_ring: u32,
    resume_on_timer: u32,
    resume: [u32; 6],
    battery_max: u32,
    capabilities: u32,
    dialog_revision: u32,
    // rs2's.
    confirm: u32,
    auto_refresh: u32,
    refresh_secs: u32,
    def_status_view: u32,
}

fn pd(o: usize) -> &'static mut PowerData {
    som::data(wps::ccd("WPPower"), o)
}

const KEY_PM: u32 = 1;
const KEY_CONFIRM: u32 = 2;
const KEY_AUTOREFRESH: u32 = 3;
const KEY_REFRESH: u32 = 4;
const KEY_DEFSTATUS: u32 = 5;

// ---- Methods ----

extern "C" fn wpSetPowerManagement(o: usize, on: u32) -> u32 {
    pd(o).os2_pm = (on != 0) as u32;
    pd(o).bios_pm = pd(o).os2_pm;
    prf_write(HINI_USER, b"PM_ControlPanel", b"APM_BIOS_Enabled", Some(if on != 0 { b"1\0" } else { b"0\0" }));
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
    refresh_views(o);
    1
}

extern "C" fn wpQueryPowerManagement(o: usize) -> u32 {
    pd(o).os2_pm
}

extern "C" fn wpSetPowerConfirmation(o: usize, on: u32) {
    pd(o).confirm = (on != 0) as u32;
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
}

extern "C" fn wpQueryPowerConfirmation(o: usize) -> u32 {
    pd(o).confirm
}

extern "C" fn wpSetAutoRefresh(o: usize, on: u32) {
    pd(o).auto_refresh = (on != 0) as u32;
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
    restart_timers(o);
}

extern "C" fn wpQueryAutoRefresh(o: usize) -> u32 {
    pd(o).auto_refresh
}

extern "C" fn wpQueryRefreshRate(o: usize) -> u32 {
    pd(o).refresh_secs
}

extern "C" fn wpSetRefreshRate(o: usize, secs: u32) -> u32 {
    if !(1..=3600).contains(&secs) {
        return 0;
    }
    pd(o).refresh_secs = secs;
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
    restart_timers(o);
    1
}

extern "C" fn wpQueryDefStatusView(o: usize) -> u32 {
    pd(o).def_status_view
}

extern "C" fn wpSetDefStatusView(o: usize, v: u32) {
    if v == OPEN_STATUS || v == OPEN_BATTERY {
        pd(o).def_status_view = v;
        obj::call(m::WPObject::wpSaveDeferred, &[o]);
    }
}

extern "C" fn wpQueryAPMLevel(_o: usize) -> u32 {
    APMLEVEL_NONE
}

/// No battery, and nothing APM's capabilities name.
extern "C" fn wpQueryCapabilities(_o: usize, battery_max: *mut u32, caps: *mut u32) -> u32 {
    unsafe {
        if !battery_max.is_null() {
            *battery_max = 0;
        }
        if !caps.is_null() {
            *caps = 0;
        }
    }
    1
}

extern "C" fn wpQueryBIOSPowerManagementSupport(o: usize, on: *mut u32) -> u32 {
    if !on.is_null() {
        unsafe { *on = pd(o).bios_pm };
    }
    1
}

extern "C" fn wpSetBIOSPowerManagementSupport(o: usize, on: u32) -> u32 {
    wpSetPowerManagement(o, on)
}

extern "C" fn wpQueryOS2PowerManagementSupport(o: usize, on: *mut u32) -> u32 {
    if !on.is_null() {
        unsafe { *on = pd(o).os2_pm };
    }
    1
}

extern "C" fn wpSetOS2PowerManagementSupport(o: usize, on: u32) -> u32 {
    wpSetPowerManagement(o, on)
}

/// Resuming needs a suspend rs2 does not have: the settings are only
/// kept.
extern "C" fn wpQueryTimerBasedRequests(o: usize, on: *mut u32) -> u32 {
    if !on.is_null() {
        unsafe { *on = pd(o).timer_requests };
    }
    1
}

extern "C" fn wpSetTimerBasedRequests(o: usize, on: u32) -> u32 {
    pd(o).timer_requests = (on != 0) as u32;
    1
}

extern "C" fn wpQueryResumeOnRing(o: usize, on: *mut u32) -> u32 {
    if !on.is_null() {
        unsafe { *on = pd(o).resume_on_ring };
    }
    1
}

extern "C" fn wpSetResumeOnRing(o: usize, on: u32) -> u32 {
    pd(o).resume_on_ring = (on != 0) as u32;
    1
}

#[allow(clippy::too_many_arguments)]
extern "C" fn wpQueryResumeOnTimer(o: usize, on: *mut u32, y: *mut u32, mo: *mut u32, d: *mut u32, h: *mut u32, mi: *mut u32, s: *mut u32) -> u32 {
    let p = pd(o);
    unsafe {
        for (ptr, v) in [(on, p.resume_on_timer), (y, p.resume[0]), (mo, p.resume[1]), (d, p.resume[2]), (h, p.resume[3]), (mi, p.resume[4]), (s, p.resume[5])] {
            if !ptr.is_null() {
                *ptr = v;
            }
        }
    }
    1
}

#[allow(clippy::too_many_arguments)]
extern "C" fn wpSetResumeOnTimer(o: usize, on: u32, y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> u32 {
    let p = pd(o);
    p.resume_on_timer = (on != 0) as u32;
    p.resume = [y, mo, d, h, mi, s];
    1
}

fn message(text: &[u8], style: u32) -> u32 {
    let (t, c) = (z(text), z(b"Power"));
    unsafe { WinMessageBox(HWND_DESKTOP, HWND_DESKTOP, t.as_ptr(), c.as_ptr(), 0, style | MB_MOVEABLE) }
}

/// Turn the computer off: the shell's shutdown (programs closed, then
/// ACPI S5).
extern "C" fn wpPowerOff(o: usize, confirm: u32) -> u32 {
    if pd(o).os2_pm == 0 {
        message(b"Power management is off. Turn it on in the Power object's settings first.", MB_OK | MB_INFORMATION);
        return 0;
    }
    // MBID_YES
    if confirm != 0 && pd(o).confirm != 0 && message(b"Close all programs and turn the computer off?", MB_YESNO | MB_ICONQUESTION) != 6 {
        return 0;
    }
    crate::shutdown::begin(false);
    1
}

extern "C" fn wpSuspend(_o: usize, _confirm: u32) -> u32 {
    message(b"This computer cannot be suspended: rs2's power management can only turn it off.", MB_OK | MB_INFORMATION);
    0
}

/// APM's power states: 3 off, 2 suspend (1 standby: as suspend).
extern "C" fn wpChangePowerState(o: usize, p1: u32, p2: u32) -> u32 {
    match p2 & 0xffff {
        3 => wpPowerOff(o, (p1 >> 16 != 0) as u32),
        1 | 2 => wpSuspend(o, 1),
        _ => 0,
    }
}

extern "C" fn wpRefreshNotebookPages(o: usize) {
    refresh_views(o);
}

// ---- Life cycle ----

extern "C" fn power_init(o: usize) {
    som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpInitData, &[o]);
    let p = pd(o);
    p.os2_pm = 1;
    p.bios_pm = 1;
    p.confirm = 1;
    p.auto_refresh = 1;
    p.refresh_secs = 10;
    p.def_status_view = OPEN_STATUS;
    p.dialog_revision = 1;
}

extern "C" fn power_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    let p = pd(o);
    for (k, v) in [(KEY_PM, p.os2_pm), (KEY_CONFIRM, p.confirm), (KEY_AUTOREFRESH, p.auto_refresh), (KEY_REFRESH, p.refresh_secs), (KEY_DEFSTATUS, p.def_status_view)] {
        obj::wpSaveLong(o, b"WPPower\0".as_ptr(), k, v);
    }
    1
}

extern "C" fn power_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    let get = |k| obj::restored_long(o, b"WPPower", k);
    let p = pd(o);
    if let Some(v) = get(KEY_PM) {
        p.os2_pm = v;
        p.bios_pm = v;
    }
    if let Some(v) = get(KEY_CONFIRM) {
        p.confirm = v;
    }
    if let Some(v) = get(KEY_AUTOREFRESH) {
        p.auto_refresh = v;
    }
    if let Some(v) = get(KEY_REFRESH).filter(|v| (1..=3600).contains(v)) {
        p.refresh_secs = v;
    }
    if let Some(v) = get(KEY_DEFSTATUS) {
        p.def_status_view = v;
    }
    1
}

/// POWER=YES|NO, CONFIRM=YES|NO (rs2's setup strings for the pages'
/// settings).
extern "C" fn power_setup(o: usize, s: *const u8) -> u32 {
    let r = som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpSetup, &[o, s as usize]) as u32;
    let setup = crate::util::cstr(s).to_vec();
    for part in setup.split(|&c| c == b';') {
        let Some(eq) = part.iter().position(|&c| c == b'=') else { continue };
        let (k, v) = (part[..eq].trim_ascii(), part[eq + 1..].trim_ascii());
        let yes = v.eq_ignore_ascii_case(b"YES");
        if k.eq_ignore_ascii_case(b"POWER") {
            wpSetPowerManagement(o, yes as u32);
        } else if k.eq_ignore_ascii_case(b"CONFIRM") {
            wpSetPowerConfirmation(o, yes as u32);
        }
    }
    r
}

// ---- The status view ----

struct View {
    obj: usize,
    dlg: u32,
}

static mut VIEWS: Vec<View> = Vec::new();
const TID_POWER: u32 = 0x5720;
const ID_SOURCE: u16 = 1301;
const ID_BATTERY: u16 = 1302;
const ID_STATE: u16 = 1303;
const ID_CAN: u16 = 1304;
const ID_PLAN: u16 = 1305;
const ID_DRAW: u16 = 1306;

fn hab() -> u32 {
    unsafe extern "C" {
        #[link_name = "PMWIN.800"]
        fn WinQueryAnchorBlock(h: u32) -> u32;
    }
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn fill_view(h: u32, o: usize) {
    set_item_text(h, ID_SOURCE as u32, b"Power source: AC line");
    set_item_text(h, ID_BATTERY as u32, b"Battery: none found");
    set_item_text(h, ID_STATE as u32, if pd(o).os2_pm != 0 { b"Power management: on" } else { b"Power management: off" });
    set_item_text(h, ID_CAN as u32, b"Can turn the computer off (ACPI); cannot suspend");
    let (plan, draw) = match os2::power::query() {
        Some(i) => {
            let name = os2::power::NAMES.get(i.plan as usize).copied().unwrap_or("?");
            let draw = if i.flags & os2::power::F_RAPL_PKG != 0 {
                alloc::format!("Processor power: {}.{:01} W", i.pkg_mw / 1000, i.pkg_mw % 1000 / 100)
            } else {
                alloc::string::String::from("Processor power: not measured")
            };
            (alloc::format!("Power plan: {name}"), draw)
        }
        None => (alloc::string::String::from("Power plan: unknown"), alloc::string::String::new()),
    };
    set_item_text(h, ID_PLAN as u32, plan.as_bytes());
    set_item_text(h, ID_DRAW as u32, draw.as_bytes());
}

fn refresh_views(o: usize) {
    for v in unsafe { VIEWS.iter() } {
        if v.obj == o {
            fill_view(v.dlg, o);
        }
    }
}

fn restart_timers(o: usize) {
    for v in unsafe { VIEWS.iter() } {
        if v.obj == o {
            unsafe {
                WinStopTimer(hab(), v.dlg, TID_POWER);
                if pd(o).auto_refresh != 0 {
                    WinStartTimer(hab(), v.dlg, TID_POWER, pd(o).refresh_secs * 1000);
                }
            }
        }
    }
}

extern "C" fn view_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let o = unsafe { VIEWS.iter().find(|v| v.dlg == h).map(|v| v.obj) };
    match msg {
        WM_INITDLG => 0,
        WM_TIMER if a & 0xffff == TID_POWER => {
            if let Some(o) = o {
                fill_view(h, o);
            }
            0
        }
        WM_COMMAND => {
            unsafe { WinDestroyWindow(h) };
            0
        }
        WM_CLOSE => {
            unsafe { WinDestroyWindow(h) };
            0
        }
        WM_DESTROY => {
            unsafe {
                WinStopTimer(hab(), h, TID_POWER);
                VIEWS.retain(|v| v.dlg != h);
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn open_status(o: usize) -> u32 {
    if let Some(v) = unsafe { VIEWS.iter().find(|v| v.obj == o) } {
        crate::view::activate(v.dlg);
        return v.dlg;
    }
    let mut title: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
    title.extend_from_slice(b" - Status");
    let mut f = dialog_frame(&title, 220, 108);
    f.ctl = (FCF_TITLEBAR | FCF_SYSMENU | FCF_TASKLIST).to_le_bytes().to_vec();
    let tpl = template(f, alloc::vec![
        text(b"", (8, 90, 204), ID_SOURCE),
        text(b"", (8, 78, 204), ID_BATTERY),
        text(b"", (8, 66, 204), ID_PLAN),
        text(b"", (8, 54, 204), ID_DRAW),
        text(b"", (8, 42, 204), ID_STATE),
        text(b"", (8, 30, 204), ID_CAN),
        item(WC_BUTTON, b"Close", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (8, 6, 50, 14), 1),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, 0, view_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return 0;
    }
    unsafe { VIEWS.push(View { obj: o, dlg }) };
    send(dlg, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[o]) as u32, 0);
    fill_view(dlg, o);
    restart_timers(o);
    let (sw, sh) = (unsafe { WinQuerySysValue(HWND_DESKTOP, 20) }, unsafe { WinQuerySysValue(HWND_DESKTOP, 21) });
    let mut r = RectL::default();
    unsafe {
        WinQueryWindowRect(dlg, &mut r);
        WinSetWindowPos(dlg, HWND_TOP, (sw - r.r) / 2, (sh - r.t) / 2, 0, 0, SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
    }
    dlg
}

extern "C" fn power_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    match view {
        OPEN_STATUS | OPEN_BATTERY | obj::OPEN_DEFAULT => open_status(o),
        _ => som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32,
    }
}

extern "C" fn power_default_view(_cls: usize) -> u32 {
    OPEN_STATUS
}

// ---- The pop-up menu ----

/// rs2's ids for the items (IBM's come from WPCONFIG's resources).
const IDM_STATUS: u32 = 0x7f30;
const IDM_POWEROFF: u32 = 0x7f31;
const IDM_SUSPEND: u32 = 0x7f32;

fn add_item(menu: u32, id: u32, t: &[u8]) {
    let mi = MenuItem { pos: -1, style: 0x0001, attr: 0, id: id as u16, sub: 0, handle: 0 };
    let t = z(t);
    send(menu, 0x0180, &mi as *const MenuItem as u32, t.as_ptr() as u32);
}

extern "C" fn power_modify_menu(o: usize, menu: u32, cnr: u32, pos: u32) -> u32 {
    som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpModifyPopupMenu, &[o, menu as usize, cnr as usize, pos as usize]);
    add_item(menu, IDM_STATUS, b"~Status");
    if pd(o).os2_pm != 0 {
        add_item(menu, IDM_SUSPEND, b"S~uspend");
        add_item(menu, IDM_POWEROFF, b"~Power off");
    }
    1
}

extern "C" fn power_menu_selected(o: usize, frame: u32, id: u32) -> u32 {
    match id {
        IDM_STATUS => {
            open_status(o);
        }
        IDM_POWEROFF => {
            wpPowerOff(o, 1);
        }
        IDM_SUSPEND => {
            wpSuspend(o, 1);
        }
        _ => return som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpMenuItemSelected, &[o, frame as usize, id as usize]) as u32,
    }
    1
}

// ---- The pages ----

const ID_PM_ON: u16 = 1310;
const ID_PM_OFF: u16 = 1311;
const ID_CONFIRM: u16 = 1312;
const ID_AUTOREFRESH: u16 = 1320;
const ID_RATE: u16 = 1321;

static mut UNDO: (u32, u32, u32, u32) = (1, 1, 1, 10);

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

fn power_page_fill(h: u32) {
    let o = page_obj(h);
    check(h, if pd(o).os2_pm != 0 { ID_PM_ON } else { ID_PM_OFF }, true);
    check(h, ID_CONFIRM, pd(o).confirm != 0);
}

extern "C" fn power_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let p = pd(b as usize);
            unsafe { UNDO = (p.os2_pm, p.confirm, p.auto_refresh, p.refresh_secs) };
            power_page_fill(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = page_obj(h);
            match (a & 0xffff) as u16 {
                ID_PM_ON | ID_PM_OFF => {
                    wpSetPowerManagement(o, checked(h, ID_PM_ON) as u32);
                }
                ID_CONFIRM => wpSetPowerConfirmation(o, checked(h, ID_CONFIRM) as u32),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            let (pm, c) = match (a & 0xffff) as u16 {
                ID_UNDO => unsafe { (UNDO.0, UNDO.1) },
                ID_DEFAULT => (1, 1),
                _ => return 0,
            };
            wpSetPowerManagement(o, pm);
            wpSetPowerConfirmation(o, c);
            power_page_fill(h);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddPowerPage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        group(b"Power management", (4, 72, 228, 44)),
        radio(b"~On: the computer can be turned off from the desktop", (10, 94, 216), ID_PM_ON, true),
        radio(b"O~ff", (10, 80, 216), ID_PM_OFF, false),
        checkbox(b"~Confirm power state changes", (6, 56, 200), ID_CONFIRM),
        book::label(b"rs2 manages power through ACPI: it can turn the", (6, 40, 226)),
        book::label(b"computer off, but cannot suspend it.", (6, 30, 226)),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, power_page_proc, items, b"~Power\0")
}

fn view_page_fill(h: u32) {
    let o = page_obj(h);
    check(h, ID_AUTOREFRESH, pd(o).auto_refresh != 0);
    spin_init(h, ID_RATE, 1, 3600, pd(o).refresh_secs as i32);
    crate::syssetup::enable(h, ID_RATE, pd(o).auto_refresh != 0);
}

extern "C" fn view_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            view_page_fill(h);
            0
        }
        WM_CONTROL => {
            let o = page_obj(h);
            match ((a & 0xffff) as u16, a >> 16) {
                (ID_AUTOREFRESH, 1) => {
                    wpSetAutoRefresh(o, checked(h, ID_AUTOREFRESH) as u32);
                    crate::syssetup::enable(h, ID_RATE, pd(o).auto_refresh != 0);
                }
                (ID_RATE, SPBN_CHANGE) => {
                    if let Some(v) = spin_value(h, ID_RATE).filter(|v| (1..=3600).contains(v)) {
                        wpSetRefreshRate(o, v as u32);
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            let (ar, rate) = match (a & 0xffff) as u16 {
                ID_UNDO => unsafe { (UNDO.2, UNDO.3) },
                ID_DEFAULT => (1, 10),
                _ => return 0,
            };
            wpSetAutoRefresh(o, ar);
            wpSetRefreshRate(o, rate);
            view_page_fill(h);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddPowerViewPage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        group(b"Status view", (4, 60, 228, 56)),
        checkbox(b"~Refresh automatically", (10, 94, 200), ID_AUTOREFRESH),
        book::label(b"Refresh every (seconds):", (10, 76, 110)),
        spin((124, 74, 50), ID_RATE),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, view_page_proc, items, b"~View\0")
}

// ---- The Plan page (rs2's own) ----

const ID_PLAN_PERF: u16 = 1330;
const ID_PLAN_BAL: u16 = 1331;
const ID_PLAN_SAVE: u16 = 1332;
const ID_PLAN_ABOUT: u16 = 1333;
const ID_PLAN_HW: u16 = 1334;
const ID_PLAN_HW2: u16 = 1335;

static mut PLAN_UNDO: u32 = 1;

const ABOUT: [&[u8]; 3] = [
    b"Full speed; idle processors wait in the lightest state.",
    b"Speed follows the load; idle processors sleep deeper.",
    b"No turbo; idle processors sleep as deep as they can.",
];

fn plan_page_fill(h: u32) {
    let info = os2::power::query();
    let plan = info.map_or(1, |i| i.plan.min(2));
    for (i, id) in [ID_PLAN_PERF, ID_PLAN_BAL, ID_PLAN_SAVE].into_iter().enumerate() {
        check(h, id, i as u32 == plan);
    }
    set_item_text(h, ID_PLAN_ABOUT as u32, ABOUT[plan as usize]);
    let (l1, l2) = match info {
        Some(i) => {
            let f = i.flags;
            let speed = if f & os2::power::F_HWP != 0 {
                alloc::format!("Speed: hardware P-states, {}-{} MHz", i.min_mhz, i.max_mhz)
            } else if f & os2::power::F_SPEEDSTEP != 0 {
                alloc::format!("Speed: SpeedStep, {}-{} MHz", i.min_mhz, i.max_mhz)
            } else {
                alloc::string::String::from("Speed: not controlled on this processor")
            };
            let idle = if f & os2::power::F_MWAIT != 0 { alloc::format!("Idle: down to C{}", i.cstate) } else { alloc::string::String::from("Idle: C1 (HLT)") };
            let tick = if f & os2::power::F_TICKLESS != 0 { ", tickless" } else { "" };
            (speed, alloc::format!("{idle}{tick}"))
        }
        None => (alloc::string::String::from("The kernel does not report its power management."), alloc::string::String::new()),
    };
    set_item_text(h, ID_PLAN_HW as u32, l1.as_bytes());
    set_item_text(h, ID_PLAN_HW2 as u32, l2.as_bytes());
}

/// Use `plan` now and from the next boot.
fn choose_plan(h: u32, plan: u32) {
    if os2::power::set_plan(plan) == 0 {
        os2::power::save(plan);
    }
    plan_page_fill(h);
}

extern "C" fn plan_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            unsafe { PLAN_UNDO = os2::power::query().map_or(1, |i| i.plan) };
            plan_page_fill(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let id = (a & 0xffff) as u16;
            if (ID_PLAN_PERF..=ID_PLAN_SAVE).contains(&id) && checked(h, id) {
                let plan = (id - ID_PLAN_PERF) as u32;
                if os2::power::query().is_none_or(|i| i.plan != plan) {
                    choose_plan(h, plan);
                }
            }
            0
        }
        WM_COMMAND => {
            match (a & 0xffff) as u16 {
                ID_UNDO => choose_plan(h, unsafe { PLAN_UNDO }),
                ID_DEFAULT => choose_plan(h, os2::power::BALANCED),
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn add_plan_page(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        group(b"Power plan", (4, 50, 228, 66)),
        radio(b"~Performance", (10, 94, 100), ID_PLAN_PERF, true),
        radio(b"~Balanced", (10, 82, 100), ID_PLAN_BAL, false),
        radio(b"Power ~saver", (10, 70, 100), ID_PLAN_SAVE, false),
        text(b"", (10, 56, 218), ID_PLAN_ABOUT),
        text(b"", (6, 38, 226), ID_PLAN_HW),
        text(b"", (6, 28, 226), ID_PLAN_HW2),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, plan_page_proc, items, b"P~lan\0")
}

/// IBM's Resume page sets APM's wake-up events, which rs2 does not have;
/// the Geyserville page is SpeedStep's.
extern "C" fn no_page(_o: usize, _nb: u32) -> u32 {
    1
}

/// Plan, Power, then View (each inserted in front of those before it).
extern "C" fn power_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPPower"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPPower", m::WPPower::wpAddPowerViewPage, &[o, nb as usize]);
    wps::call("WPPower", m::WPPower::wpAddPowerPage, &[o, nb as usize]);
    add_plan_page(o, nb);
    1
}

methods! {
    __m_wpChangePowerState => wpChangePowerState, 3;
    __m_wpSetPowerManagement => wpSetPowerManagement, 2;
    __m_wpQueryPowerManagement => wpQueryPowerManagement, 1;
    __m_wpSetPowerConfirmation => wpSetPowerConfirmation, 2;
    __m_wpQueryPowerConfirmation => wpQueryPowerConfirmation, 1;
    __m_wpAddPowerPage => wpAddPowerPage, 2;
    __m_wpAddPowerViewPage => wpAddPowerViewPage, 2;
    __m_wpSetAutoRefresh => wpSetAutoRefresh, 2;
    __m_wpQueryAutoRefresh => wpQueryAutoRefresh, 1;
    __m_wpQueryRefreshRate => wpQueryRefreshRate, 1;
    __m_wpSetRefreshRate => wpSetRefreshRate, 2;
    __m_wpQueryDefStatusView => wpQueryDefStatusView, 1;
    __m_wpSetDefStatusView => wpSetDefStatusView, 2;
    __m_wpQueryAPMLevel => wpQueryAPMLevel, 1;
    __m_wpQueryBIOSPowerManagementSupport => wpQueryBIOSPowerManagementSupport, 2;
    __m_wpSetBIOSPowerManagementSupport => wpSetBIOSPowerManagementSupport, 2;
    __m_wpQueryOS2PowerManagementSupport => wpQueryOS2PowerManagementSupport, 2;
    __m_wpSetOS2PowerManagementSupport => wpSetOS2PowerManagementSupport, 2;
    __m_wpQueryTimerBasedRequests => wpQueryTimerBasedRequests, 2;
    __m_wpSetTimerBasedRequests => wpSetTimerBasedRequests, 2;
    __m_wpQueryResumeOnRing => wpQueryResumeOnRing, 2;
    __m_wpSetResumeOnRing => wpSetResumeOnRing, 2;
    __m_wpQueryResumeOnTimer => wpQueryResumeOnTimer, 8;
    __m_wpSetResumeOnTimer => wpSetResumeOnTimer, 8;
    __m_wpQueryCapabilities => wpQueryCapabilities, 3;
    __m_wpPowerOff => wpPowerOff, 2;
    __m_wpSuspend => wpSuspend, 2;
    __m_wpRefreshNotebookPages => wpRefreshNotebookPages, 1;
    __m_power_no_page => no_page, 2;
    __m_power_add_pages => power_add_pages, 2;
    __m_power_init => power_init, 1;
    __m_power_save_state => power_save_state, 1;
    __m_power_restore_state => power_restore_state, 2;
    __m_power_setup => power_setup, 2;
    __m_power_open => power_open, 4;
    __m_power_modify_menu => power_modify_menu, 4;
    __m_power_menu_selected => power_menu_selected, 3;
    __m_power_default_view => power_default_view, 1;
}

pub fn impl_power() -> Impl {
    Impl {
        data: core::mem::size_of::<PowerData>() as u32,
        methods: alloc::vec![
            ("wpChangePowerState", __m_wpChangePowerState as usize),
            ("wpSetPowerManagement", __m_wpSetPowerManagement as usize),
            ("wpQueryPowerManagement", __m_wpQueryPowerManagement as usize),
            ("wpSetPowerConfirmation", __m_wpSetPowerConfirmation as usize),
            ("wpQueryPowerConfirmation", __m_wpQueryPowerConfirmation as usize),
            ("wpAddPowerPage", __m_wpAddPowerPage as usize),
            ("wpAddPowerViewPage", __m_wpAddPowerViewPage as usize),
            ("wpSetAutoRefresh", __m_wpSetAutoRefresh as usize),
            ("wpQueryAutoRefresh", __m_wpQueryAutoRefresh as usize),
            ("wpQueryRefreshRate", __m_wpQueryRefreshRate as usize),
            ("wpSetRefreshRate", __m_wpSetRefreshRate as usize),
            ("wpQueryDefStatusView", __m_wpQueryDefStatusView as usize),
            ("wpSetDefStatusView", __m_wpSetDefStatusView as usize),
            ("wpAddPowerResumePage", __m_power_no_page as usize),
            ("wpQueryAPMLevel", __m_wpQueryAPMLevel as usize),
            ("wpQueryBIOSPowerManagementSupport", __m_wpQueryBIOSPowerManagementSupport as usize),
            ("wpSetBIOSPowerManagementSupport", __m_wpSetBIOSPowerManagementSupport as usize),
            ("wpQueryOS2PowerManagementSupport", __m_wpQueryOS2PowerManagementSupport as usize),
            ("wpSetOS2PowerManagementSupport", __m_wpSetOS2PowerManagementSupport as usize),
            ("wpQueryTimerBasedRequests", __m_wpQueryTimerBasedRequests as usize),
            ("wpSetTimerBasedRequests", __m_wpSetTimerBasedRequests as usize),
            ("wpQueryResumeOnRing", __m_wpQueryResumeOnRing as usize),
            ("wpSetResumeOnRing", __m_wpSetResumeOnRing as usize),
            ("wpQueryResumeOnTimer", __m_wpQueryResumeOnTimer as usize),
            ("wpSetResumeOnTimer", __m_wpSetResumeOnTimer as usize),
            ("wpQueryCapabilities", __m_wpQueryCapabilities as usize),
            ("wpPowerOff", __m_wpPowerOff as usize),
            ("wpSuspend", __m_wpSuspend as usize),
            ("wpRefreshNotebookPages", __m_wpRefreshNotebookPages as usize),
            ("wpAddGeyservillePage", __m_power_no_page as usize),
        ],
        overrides: alloc::vec![
            ("wpAddSettingsPages", __m_power_add_pages as usize),
            ("wpInitData", __m_power_init as usize),
            ("wpSaveState", __m_power_save_state as usize),
            ("wpRestoreState", __m_power_restore_state as usize),
            ("wpSetup", __m_power_setup as usize),
            ("wpOpen", __m_power_open as usize),
            ("wpModifyPopupMenu", __m_power_modify_menu as usize),
            ("wpMenuItemSelected", __m_power_menu_selected as usize),
            crate::pages::no_window_page_override(),
        ],
    }
}

pub fn impl_m_power() -> Impl {
    let mut ov = crate::syssetup::meta_overrides();
    ov.push(("wpclsQueryDefaultView", __m_power_default_view as usize));
    Impl { data: 0, methods: Vec::new(), overrides: ov }
}
