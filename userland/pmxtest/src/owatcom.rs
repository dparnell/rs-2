//! What the Open Watcom 1.9 installer (its GUI library) relies on: the
//! CREATESTRUCT layout, NULLHANDLE parents as IBM treats them, a dialog
//! dismissed before WinProcessDlg, WinWaitMsg, menu keys on its frames.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::check;
use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMWIN.774"]
    fn WinIsWindowShowing(h: u32) -> u32;
    #[link_name = "PMWIN.922"]
    fn WinCreateDlg(parent: u32, owner: u32, proc_: WndProc, t: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.796"]
    fn WinProcessDlg(h: u32) -> u32;
    #[link_name = "PMWIN.910"]
    fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.897"]
    fn WinWaitMsg(hab: u32, first: u32, last: u32) -> u32;
    #[link_name = "PMWIN.907"]
    fn WinCreateMenu(parent: u32, t: *const u8) -> u32;
    #[link_name = "PMWIN.929"]
    fn WinSubclassWindow(h: u32, p: WndProc) -> u32;
    #[link_name = "PMWIN.865"]
    fn WinSetParent(h: u32, parent: u32, redraw: u32) -> u32;
    #[link_name = "PMWIN.878"]
    fn WinSetWindowULong(h: u32, i: i32, v: u32) -> u32;
    #[link_name = "PMWIN.879"]
    fn WinSetWindowUShort(h: u32, i: i32, v: u32) -> u32;
    #[link_name = "PMWIN.892"]
    fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMGPI.476"]
    fn GpiQueryPel(hps: u32, p: *const i32) -> i32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(h: u32, r: *mut i32) -> u32;
}

fn pel(h: u32, x: i32, y: i32) -> i32 {
    let hps = unsafe { WinGetPS(h) };
    let p = [x, y];
    let v = unsafe { GpiQueryPel(hps, p.as_ptr()) };
    unsafe { WinReleasePS(hps) };
    v
}

/// EN_KILLFOCUS notifications a client saw.
static KILLFOCUS: AtomicU32 = AtomicU32::new(0);

extern "C" fn edit_owner_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    // WM_CONTROL with EN_KILLFOCUS (2).
    if msg == 0x0030 && mp1 >> 16 == 2 {
        KILLFOCUS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

/// The focus leaves a window before it is destroyed (IBM's
/// XDestroyWindow): an entry field with the focus tells its owner
/// EN_KILLFOCUS, when the field goes and when its frame goes. The GUI
/// library ignored every key after a dialog closed with the focus in a
/// field.
unsafe fn focus_leaves_destroyed(hab: u32) {
    unsafe {
        WinRegisterClass(hab, b"OwEditOwner\0".as_ptr(), edit_owner_proc, 0, 0);
        let mk = || {
            let mut fcf = 0x0001 | 0x0008;
            let mut c = 0;
            let f = WinCreateStdWindow(HWND_DESKTOP, 0x8000_0000, &mut fcf, b"OwEditOwner\0".as_ptr(), b"Dlg\0".as_ptr(), 0, 0, 0, &mut c);
            WinSetWindowPos(f, 3, 200, 200, 200, 120, 0x0003 | 0x0008 | 0x0080);
            // WC_ENTRYFIELD, ES_MARGIN | WS_VISIBLE.
            let e = WinCreateWindow(c, 0xffff_0006 as *const u8, b"x\0".as_ptr(), 0x8000_0008, 10, 10, 100, 20, c, 3, 7, core::ptr::null(), core::ptr::null());
            WinSetFocus(HWND_DESKTOP, e);
            KILLFOCUS.store(0, Ordering::Relaxed);
            (f, e)
        };
        let (f, e) = mk();
        WinDestroyWindow(e);
        check("an entry field destroyed with the focus tells its owner EN_KILLFOCUS", KILLFOCUS.load(Ordering::Relaxed) == 1);
        WinDestroyWindow(f);
        let (f, _) = mk();
        WinDestroyWindow(f);
        check("so does one whose frame is destroyed", KILLFOCUS.load(Ordering::Relaxed) == 1);
    }
}

static CHILD_ACT: AtomicU32 = AtomicU32::new(0);
static OLD_CHILD: AtomicU32 = AtomicU32::new(0);

/// A document frame's subclass: what WM_ACTIVATE said (1 on, 2 off).
extern "C" fn child_frame_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == 0x000d {
        CHILD_ACT.store(if mp1 & 0xffff != 0 { 1 } else { 2 }, Ordering::Relaxed);
    }
    let old: WndProc = unsafe { core::mem::transmute(OLD_CHILD.load(Ordering::Relaxed) as usize) };
    old(h, msg, mp1, mp2)
}

/// A frame inside a client (the GUI library's MDI documents): the focus
/// going into it sends it WM_ACTIVATE(TRUE) and lights its title bar,
/// leaving it WM_ACTIVATE(FALSE), as IBM's WinFrameFocusChange (the IDE
/// makes a document current on that WM_ACTIVATE).
unsafe fn child_frame_activation(client: u32) {
    unsafe {
        let mut fcf = 0x0001 | 0x0008; // FCF_TITLEBAR | FCF_SIZEBORDER
        let mut doc = 0;
        let child = WinCreateStdWindow(client, 0x8000_0000, &mut fcf, b"OwGuiClient\0".as_ptr(), b"Doc\0".as_ptr(), 0, 0, 0, &mut doc);
        WinSetWindowPos(child, 3, 10, 10, 150, 100, 0x0003 | 0x0008);
        OLD_CHILD.store(WinSubclassWindow(child, child_frame_proc), Ordering::Relaxed);
        WinSetFocus(HWND_DESKTOP, client);
        CHILD_ACT.store(0, Ordering::Relaxed);
        WinSetFocus(HWND_DESKTOP, doc);
        let tb = WinWindowFromID(child, 0x8003);
        let lit = |tb: u32| WinSendMsg(tb, 0x01e4 /* TBM_QUERYHILITE */, 0, 0);
        check("the focus into a frame inside a client: WM_ACTIVATE(TRUE), its title lit", CHILD_ACT.load(Ordering::Relaxed) == 1 && lit(tb) == 1);
        WinSetFocus(HWND_DESKTOP, client);
        check("the focus out of it: WM_ACTIVATE(FALSE), its title not lit", CHILD_ACT.load(Ordering::Relaxed) == 2 && lit(tb) == 0);
        WinDestroyWindow(child);
    }
}

/// LM_QUERYSELECTION as IBM's: a single-selection list ignores mp1 (the
/// GUI library's LB_GETCURSEL asks with 0: the IDE's New Target said
/// "Select an image type." with the first one selected); a multiple one
/// looks after mp1.
unsafe fn list_selection(client: u32) {
    unsafe {
        let mk = |style: u32| {
            let l = WinCreateWindow(client, 0xffff_0007 as *const u8, b"\0".as_ptr(), 0x8000_0000 | style, 10, 10, 100, 60, client, 3, 903, core::ptr::null(), core::ptr::null());
            for t in [&b"one\0"[..], b"two\0", b"three\0"] {
                WinSendMsg(l, 0x0161 /* LM_INSERTITEM */, 0xffff /* LIT_END */, t.as_ptr() as u32);
            }
            l
        };
        let l = mk(0);
        WinSendMsg(l, 0x0164 /* LM_SELECTITEM */, 0, 1);
        let q = |l: u32, from: u32| WinSendMsg(l, 0x0165 /* LM_QUERYSELECTION */, from, 0) as i32;
        check("LM_QUERYSELECTION(0) of a single-selection list is its selected item 0", q(l, 0) == 0 && q(l, 0xffff) == 0);
        WinDestroyWindow(l);
        let m = mk(0x0001 /* LS_MULTIPLESEL */);
        WinSendMsg(m, 0x0164, 0, 1);
        WinSendMsg(m, 0x0164, 2, 1);
        check("LM_QUERYSELECTION of a multiple-selection list: LIT_FIRST, then after an item", q(m, 0xffff) == 0 && q(m, 0) == 2 && q(m, 2) == -1);
        WinDestroyWindow(m);
    }
}

/// The IDE's New Target dialog makes its radio buttons, then the group
/// box around them: a group box draws only its outline and title (IBM's
/// StaticPaintGroupbox), so a window below it inside still shows.
unsafe fn group_box_over_sibling(client: u32) {
    unsafe {
        // WC_STATIC: SS_FGNDRECT, then SS_GROUPBOX over it (WS_VISIBLE).
        let rect = WinCreateWindow(client, 0xffff_0005 as *const u8, b"\0".as_ptr(), 0x8000_0000 | 0x05, 20, 20, 40, 30, client, 3, 901, core::ptr::null(), core::ptr::null());
        WinUpdateWindow(client);
        let (inside, outside) = (pel(client, 40, 30), pel(client, 100, 30));
        let gb = WinCreateWindow(client, 0xffff_0005 as *const u8, b"Group\0".as_ptr(), 0x8000_0000 | 0x02, 10, 10, 80, 60, client, 3, 902, core::ptr::null(), core::ptr::null());
        WinUpdateWindow(client);
        let after = pel(client, 40, 30);
        check("a group box made over a sibling shows it inside (only its outline is drawn)", rect != 0 && gb != 0 && inside != outside && after == inside);
        WinDestroyWindow(gb);
        WinDestroyWindow(rect);
    }
}

/// MENUITEM: iPosition, afStyle, afAttribute, id, hwndSubMenu, hItem.
#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

const MM_INSERTITEM: u32 = 0x0180;
const MM_QUERYITEM: u32 = 0x0182;
const MM_QUERYITEMTEXT: u32 = 0x018b;
const MM_ENDMENUMODE: u32 = 0x0186;
const MM_QUERYSELITEMID: u32 = 0x018a;
const WM_TRANSLATEACCEL: u32 = 0x004b;
const WM_SYSCOMMAND: u32 = 0x0021;
const FID_MENU: u32 = 0x8005;

/// The frame's original procedure and the WM_CHARs its subclass took.
static OLD_FRAME: AtomicU32 = AtomicU32::new(0);
static FRAME_CHARS: AtomicU32 = AtomicU32::new(0);

fn is_f10(mp1: u32, mp2: u32) -> bool {
    mp1 & 2 != 0 && mp2 >> 16 == 0x2b
}

/// As the Open Watcom GUI library's GUIFrameProc: the frame's WM_CHARs
/// go to the program's own key handling (never to the frame procedure),
/// and F10 is kept from accelerator translation.
extern "C" fn gui_frame_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CHAR => {
            FRAME_CHARS.fetch_add(1, Ordering::Relaxed);
            1
        }
        WM_TRANSLATEACCEL if mp1 != 0 && {
            let q = unsafe { (mp1 as *const QMsg).read_unaligned() };
            is_f10(q.mp1, q.mp2)
        } => 0,
        _ => {
            let old: WndProc = unsafe { core::mem::transmute(OLD_FRAME.load(Ordering::Relaxed) as usize) };
            old(h, msg, mp1, mp2)
        }
    }
}

extern "C" fn gui_client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

/// The IDE's action bar: WinCreateMenu(HWND_DESKTOP) filled with
/// MM_INSERTITEM, then made the frame's FID_MENU as the GUI library's
/// _wpi_setmenu does. Alt+F must open File although the frame's
/// subclass keeps every WM_CHAR (IBM: the frame's WM_TRANSLATEACCEL asks
/// its menu, which opens the item and empties the message); F10 stays a
/// key when the frame refuses to translate it; Alt+F4 is still SC_CLOSE.
unsafe fn gui_menu_keys(hab: u32) {
    unsafe {
        WinRegisterClass(hab, b"OwGuiClient\0".as_ptr(), gui_client_proc, 0, 0);
        let mut fcf = 0x0001 | 0x0008; // FCF_TITLEBAR | FCF_SIZEBORDER
        let mut client = 0;
        let frame = WinCreateStdWindow(HWND_DESKTOP, 0x8000_0000, &mut fcf, b"OwGuiClient\0".as_ptr(), b"GUI\0".as_ptr(), 0, 0, 0, &mut client);
        WinSetWindowPos(frame, 3, 50, 50, 300, 200, 0x0003 | 0x0008 | 0x0080);
        OLD_FRAME.store(WinSubclassWindow(frame, gui_frame_proc), Ordering::Relaxed);
        let bar = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let sub = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let mut mi = MenuItem { pos: -1, id: 10, ..Default::default() };
        WinSendMsg(sub, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~Open\0".as_ptr() as u32);
        mi.id = 1;
        mi.sub = sub;
        WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~File\0".as_ptr() as u32);
        WinSetWindowUShort(bar, -1 /* QWS_ID */, FID_MENU);
        WinSetWindowULong(bar, -2 /* QWL_STYLE */, 0x8000_0000 | 0x0200_0000 | 0x0001 | 0x0008); // WS_VISIBLE | WS_SYNCPAINT | MS_ACTIONBAR | 8, as _wpi_setmenu
        WinSetParent(bar, frame, 0);
        WinSetOwner(bar, frame);
        WinSendMsg(frame, 0x0042 /* WM_UPDATEFRAME */, 0x0004, 0);
        WinSetFocus(HWND_DESKTOP, client);
        let key = |mp1: u32, mp2: u32| {
            WinPostMsg(client, WM_CHAR, mp1, mp2);
            let mut q = QMsg::default();
            WinGetMsg(hab, &mut q, 0, WM_CHAR, WM_CHAR);
            q
        };
        // Alt+F: KC_ALT | KC_SCANCODE, scan code 0x21, no character.
        let q = key(0x24 | 1 << 16 | 0x21 << 24, 0);
        let sel = WinSendMsg(bar, MM_QUERYSELITEMID, 0, 0) & 0xffff;
        check("Alt+F opens File on a GUI-library frame (WM_TRANSLATEACCEL, the message emptied)", q.msg == 0 && q.hwnd == 0 && sel == 1 && FRAME_CHARS.load(Ordering::Relaxed) == 0);
        WinSendMsg(bar, MM_ENDMENUMODE, 1, 0);
        // F10: KC_VIRTUALKEY | KC_SCANCODE, VK_F10.
        let q = key(0x06 | 1 << 16 | 0x44 << 24, 0x2b << 16);
        check("F10 refused by the frame's WM_TRANSLATEACCEL stays WM_CHAR", q.msg == WM_CHAR && q.hwnd == client);
        // Alt+F4: KC_ALT | KC_VIRTUALKEY | KC_SCANCODE, VK_F4.
        let q = key(0x26 | 1 << 16 | 0x3e << 24, 0x23 << 16);
        check("Alt+F4 is still WM_SYSCOMMAND SC_CLOSE for the frame", q.msg == WM_SYSCOMMAND && q.hwnd == frame && q.mp1 & 0xffff == 0x8004);
        WinDestroyWindow(frame);
    }
}

/// What WM_CREATE brought: mp1, then the CREATESTRUCT's pPresParams,
/// pCtlData, id and hwndParent.
static CREATE: [AtomicU32; 5] = [const { AtomicU32::new(0xdead) }; 5];

extern "C" fn create_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_CREATE && mp2 != 0 {
        let cs = mp2 as *const u32;
        let v = unsafe { [mp1, cs.read_unaligned(), cs.add(1).read_unaligned(), cs.add(2).read_unaligned(), cs.add(12).read_unaligned()] };
        for (a, v) in CREATE.iter().zip(v) {
            a.store(v, Ordering::Relaxed);
        }
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

static OLD_SUB: AtomicU32 = AtomicU32::new(0);

/// A frame subclass that passes everything on (the GUI library's).
extern "C" fn frame_sub(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let old: WndProc = unsafe { core::mem::transmute(OLD_SUB.load(Ordering::Relaxed) as usize) };
    old(h, msg, mp1, mp2)
}

extern "C" fn dlg_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { WinDefDlgProc(h, msg, mp1, mp2) }
}

/// A DLGTEMPLATE with one item, an empty frame (WC_FRAME).
fn frame_template() -> [u8; 46] {
    let mut t = [0u8; 46];
    let mut put = |at: usize, v: u16| t[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put(0, 46);
    put(4, 850);
    put(6, 14);
    put(8, 1);
    put(10, 0xffff);
    put(14 + 6, 1); // WC_FRAME
    put(14 + 8, 1);
    put(14 + 10, 44); // the text, ""
    put(14 + 20, 100);
    put(14 + 22, 60);
    put(14 + 24, 500);
    put(14 + 26, 0xffff);
    put(14 + 28, 0xffff);
    t
}

pub fn owatcom_checks(hab: u32, hmq: u32, client: u32) {
    unsafe {
        WinRegisterClass(hab, b"OwCreate\0".as_ptr(), create_proc, 0, 4);
        let ctl = [0x55u8; 8];
        let h = WinCreateWindow(client, b"OwCreate\0".as_ptr(), b"\0".as_ptr(), 0, 0, 0, 10, 10, client, 3, 77, ctl.as_ptr(), core::ptr::null());
        let got: [u32; 5] = core::array::from_fn(|i| CREATE[i].load(Ordering::Relaxed));
        let p = ctl.as_ptr() as u32;
        check("WM_CREATE CREATESTRUCT: pPresParams, pCtlData, id, hwndParent", h != 0 && got == [p, 0, p, 77, client]);
        WinDestroyWindow(h);

        // NULLHANDLE parents: WinCreateWindow makes a window that is never
        // shown, WinCreateStdWindow and WinGetPS fail.
        let h = WinCreateWindow(0, b"OwCreate\0".as_ptr(), b"\0".as_ptr(), 0x8000_0000, 0, 0, 10, 10, 0, 3, 1, core::ptr::null(), core::ptr::null());
        check("WinCreateWindow(NULLHANDLE) is never showing", h != 0 && WinIsWindowShowing(h) == 0);
        WinDestroyWindow(h);
        let mut fcf = 1u32;
        let mut cl = 0;
        let f = WinCreateStdWindow(0, 0, &mut fcf, core::ptr::null(), b"x\0".as_ptr(), 0, 0, 0, &mut cl);
        check("WinCreateStdWindow(NULLHANDLE) fails", f == 0);
        check("WinGetPS(NULLHANDLE) fails", WinGetPS(0) == 0);

        // A dialog dismissed before WinProcessDlg: it returns the result at
        // once and the dialog stays hidden.
        let t = frame_template();
        let d = WinCreateDlg(HWND_DESKTOP, client, dlg_proc, t.as_ptr(), 0);
        WinDismissDlg(d, 7);
        let r = WinProcessDlg(d);
        check("WinProcessDlg of a dismissed dialog", d != 0 && r == 7 && WinIsWindowShowing(d) == 0);
        WinDestroyWindow(d);

        // WinCreateMenu without a template: an empty menu. Items inserted
        // with style 0 are text items, one with a submenu is MIS_SUBMENU.
        let bar = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let sub = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let mut mi = MenuItem { pos: -1, id: 1, ..Default::default() };
        WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~File\0".as_ptr() as u32);
        mi.id = 2;
        mi.sub = sub;
        WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~Edit\0".as_ptr() as u32);
        let mut buf = [0u8; 16];
        let n = WinSendMsg(bar, MM_QUERYITEMTEXT, 1 | 16 << 16, buf.as_mut_ptr() as u32);
        let mut q = MenuItem::default();
        WinSendMsg(bar, MM_QUERYITEM, 2, &mut q as *mut MenuItem as u32);
        check("WinCreateMenu(NULL) and MM_INSERTITEM with style 0", bar != 0 && n == 5 && &buf[..5] == b"~File" && { q.style } == 0x11);
        WinDestroyWindow(bar);

        // The GUI library's menu bar (wprof, wdw): made on the desktop,
        // given the ID FID_MENU and a visible action bar's style, put in
        // the frame, and WM_UPDATEFRAME(FCF_MENU): the frame formats it
        // between its title bar and the client. As in wprof, the frame is
        // hidden, subclassed and of size 0 then (formatting hid the menu,
        // and rs2 left a hidden menu out of the frame for good).
        WinRegisterClass(hab, b"OwMenuClient\0".as_ptr(), create_proc, 0, 4);
        let mut fcf = 0x0000_0001 | 0x0000_0008 | 0x0000_0002 | 0x0000_0030; // FCF_TITLEBAR | FCF_SIZEBORDER | FCF_SYSMENU | FCF_MINMAX
        let mut cl = 0;
        let frame = WinCreateStdWindow(HWND_DESKTOP, 0x3000_0000, &mut fcf, b"OwMenuClient\0".as_ptr(), b"menu\0".as_ptr(), 0, 0, 1, &mut cl);
        OLD_SUB.store(WinSubclassWindow(frame, frame_sub), Ordering::Relaxed);
        let bar = WinCreateMenu(HWND_DESKTOP, core::ptr::null());
        let mi = MenuItem { pos: -1, id: 1, ..Default::default() };
        WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as u32, b"~File\0".as_ptr() as u32);
        WinSetWindowUShort(bar, -1, 0x8005);
        WinSetWindowULong(bar, -2, 0x8200_0009);
        WinSetParent(bar, frame, 0);
        WinSetOwner(bar, frame);
        WinSendMsg(frame, 0x0042, 4, 0); // WM_UPDATEFRAME, FCF_MENU
        WinSetWindowPos(frame, 3, 20, 20, 300, 200, 0x0003);
        WinShowWindow(frame, 1);
        let mut mr = [0i32; 4];
        let mut mp = [0i32; 9];
        WinQueryWindowPos(bar, mp.as_mut_ptr());
        WinQueryWindowRect(bar, mr.as_mut_ptr());
        let mut cp = [0i32; 9];
        WinQueryWindowPos(cl, cp.as_mut_ptr());
        check("a menu put in a frame as FID_MENU: WM_UPDATEFRAME formats it above the client", WinWindowFromID(frame, 0x8005) == bar && WinIsWindowShowing(bar) != 0 && mr[3] > 0 && mp[3] >= cp[3] + cp[1] /* SWP: fl, cy, cx, y, x: the menu's y at the client's top or above */);
        WinDestroyWindow(frame);

        // WinWaitMsg: returns once a message in the range is queued and
        // leaves it there.
        WinPostQueueMsg(hmq, WM_USER + 5, 1, 2);
        let w = WinWaitMsg(hab, WM_USER + 5, WM_USER + 5);
        let mut q = QMsg::default();
        let left = WinPeekMsg(hab, &mut q, 0, WM_USER + 5, WM_USER + 5, 1);
        check("WinWaitMsg", w != 0 && left != 0 && q.mp1 == 1);
        gui_menu_keys(hab);
        group_box_over_sibling(client);
        list_selection(client);
        child_frame_activation(client);
        focus_leaves_destroyed(hab);
    }
}
