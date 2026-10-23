//! WPUrl: Internet objects (IBM's WPINET.DLL, whose classes rs2's PMWP
//! provides). A WPUrl is a data file whose content is the URL (setup
//! `URL=`, the URL settings page). Opening it starts the web browser set
//! up in OS2.INI (`WPURLDEFAULTSETTINGS`: `DefaultBrowserExe`,
//! `DefaultParameters`, `DefaultWorkingDir`, as IBM's shell keeps it)
//! with the URL; without one it says so and shows the URL. Its icon is
//! a globe (rs2's own). WPUrlFolder is a folder of them.

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::som;
use crate::util::*;
use crate::util::{cstr, z};
use crate::wps::{self, Impl};

const APP: &[u8] = b"WPURLDEFAULTSETTINGS";

/// The object's URL (its file's first line).
pub fn url_of(obj: usize) -> Vec<u8> {
    let Some(p) = wps::path_of(obj) else { return Vec::new() };
    let c = z(&p);
    let (mut hf, mut act) = (0u32, 0u32);
    if unsafe { os2::DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; 2048];
    let mut n = 0u32;
    unsafe {
        os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n);
        os2::DosClose(hf);
    }
    buf.truncate(n as usize);
    let end = buf.iter().position(|&c| c == b'\r' || c == b'\n' || c == 0).unwrap_or(buf.len());
    buf[..end].trim_ascii().to_vec()
}

/// Write the URL into the object's file.
pub fn set_url(obj: usize, url: &[u8]) -> bool {
    let Some(p) = wps::path_of(obj) else { return false };
    let c = z(&p);
    let (mut hf, mut act) = (0u32, 0u32);
    // Replace or create; write only, deny write.
    if unsafe { os2::DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut n = 0u32;
    let ok = unsafe { os2::DosWrite(hf, url.as_ptr(), url.len() as u32, &mut n) } == 0;
    unsafe { os2::DosClose(hf) };
    ok
}

fn setting(key: &[u8]) -> Vec<u8> {
    prf_read(HINI_USER, APP, key).map(|v| v.strip_suffix(&[0]).unwrap_or(&v).to_vec()).unwrap_or_default()
}

extern "C" fn url_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPUrl"), wps::cd("WPDataFile"), m::WPObject::wpSetup, &[obj, setup as usize]);
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        if k == b"URL" || k == b"LOCATOR" {
            set_url(obj, &v);
        }
    }
    1
}

extern "C" fn url_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPUrl"), wps::cd("WPDataFile"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize]) as u32;
    }
    let url = url_of(obj);
    let exe = setting(b"DefaultBrowserExe");
    if exe.is_empty() {
        let mut t = b"No web browser is set up (OS2.INI's WPURLDEFAULTSETTINGS DefaultBrowserExe).\n\nThe URL is:\n".to_vec();
        t.extend_from_slice(&url);
        let owner = crate::view::wins().iter().find(|w| w.desktop).map_or(HWND_DESKTOP, |w| w.frame);
        message(owner, &t, &obj::title(obj), MB_OK | MB_INFORMATION);
        return 0;
    }
    let mut params = setting(b"DefaultParameters");
    if !params.is_empty() {
        params.push(b' ');
    }
    params.extend_from_slice(&url);
    crate::launch::start(&exe, &params, &setting(b"DefaultWorkingDir"), &obj::title(obj), 0)
}

/// The globe, unless the object has an icon of its own.
extern "C" fn url_icon(obj: usize) -> u32 {
    if obj::d(obj).icon == 0 && ea_read(&wps::path_of(obj).unwrap_or_default(), b".ICON").is_none() {
        return crate::icons::shell(crate::icons::INTERNET);
    }
    som::call_parent(wps::ccd("WPUrl"), wps::cd("WPDataFile"), m::WPObject::wpQueryIcon, &[obj]) as u32
}

// ---- The URL page ----

const ID_URL: u16 = 470;

extern "C" fn url_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, b);
                WinSendDlgItemMsg(h, ID_URL as u32, EM_SETTEXTLIMIT, 1000, 0);
            }
            set_item_text(h, ID_URL as u32, &url_of(b as usize));
            0
        }
        WM_DESTROY => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let t = item_text(h, ID_URL as u32);
            if o != 0 && t != url_of(o) {
                set_url(o, t.trim_ascii());
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn url_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPUrl"), wps::cd("WPDataFile"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    let tpl = template(book::page_frame(), alloc::vec![
        book::label(b"URL (the Internet address it opens):", (6, 104, 220)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 92, 216, 8), ID_URL),
    ]);
    let page = book::make_page(nb, url_page_proc, &tpl, o);
    if page != 0 {
        book::insert(o, nb, page, b"~URL\0");
    }
    1
}

methods! {
    __m_url_setup => url_setup, 2;
    __m_url_open => url_open, 4;
    __m_url_icon => url_icon, 1;
    __m_url_add_pages => url_add_pages, 2;
}

pub fn impl_url() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpSetup", __m_url_setup as usize),
            ("wpOpen", __m_url_open as usize),
            ("wpQueryIcon", __m_url_icon as usize),
            ("wpAddSettingsPages", __m_url_add_pages as usize),
        ],
    }
}
