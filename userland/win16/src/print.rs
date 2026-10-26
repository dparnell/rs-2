//! Printing: printer DCs (CreateDC, CreateIC, DeleteDC), Escape and
//! the Windows 3.1 printing calls (StartDoc, StartPage, EndPage, EndDoc,
//! AbortDoc, SetAbortProc, QueryAbort), GetDeviceCaps of printers, and
//! the WIN.INI entries programs read to find the printers.
//!
//! The printers are rs2's spooler queues (docs/PRINTING.md): a Windows
//! printer is a queue, its device name the queue's description, its
//! port the queue's name with a colon ("PSPRINT:"), its driver the
//! queue's (PSCRIPT). A printer DC is a queued PM DC (OD_QUEUED,
//! PM_Q_STD: the drawing is recorded and the spooler plays it on rs2's
//! PostScript driver) with a presentation space in device pels; GDI
//! draws on it as on a window's (gdi.rs), with y flipped by the page
//! height. CreateIC gives an OD_INFO DC: it answers queries and draws
//! nothing.
//!
//! WIN.INI is not a file here: GetProfileString answers [windows]
//! device=, [devices] and [PrinterPorts] from the queues.

use alloc::vec;
use alloc::vec::Vec;
use thunk16::{flat, Pascal, PascalCall};

use crate::{far_str, gdi, Global};

unsafe extern "C" {
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.605"]
    fn DevEscape(hdc: u32, code: i32, nin: i32, pin: *const u8, nout: *mut i32, pout: *mut u8) -> i32;
    #[link_name = "PMGPI.606"]
    fn DevQueryCaps(hdc: u32, start: i32, n: i32, out: *mut i32) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
}

const OD_QUEUED: i32 = 2;
const OD_INFO: i32 = 6;
const PU_PELS: u32 = 0x0008;
const GPIA_ASSOC: u32 = 0x4000;

const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const DEVESC_ABORTDOC: i32 = 8153;
const DEVESC_NEWFRAME: i32 = 16300;
const DEVESC_DRAFTMODE: i32 = 16301;
const DEVESC_FLUSHOUTPUT: i32 = 16302;

/// SP_ERROR, SP_APPABORT.
const SP_ERROR: u32 = 0xffff;
const SP_APPABORT: u32 = 0xfffe;

/// A printer: one of the spooler's queues.
#[derive(Clone)]
pub struct Printer {
    pub queue: Vec<u8>,
    pub desc: Vec<u8>,
    /// The queue's driver ("PSCRIPT").
    pub driver: Vec<u8>,
    /// The driver's device ("Generic PostScript Printer").
    pub device: Vec<u8>,
    pub default: bool,
    /// The queue's job properties (DRIVDATA).
    pub drivdata: Vec<u8>,
}

/// A PMSPL entry point (PMSPL is loaded when first needed).
fn spl(ord: u32) -> usize {
    static MODULE: Global<u32> = Global::new(0);
    let m = MODULE.get();
    if *m == 0 {
        let mut fail = [0u8; 32];
        if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 32, b"PMSPL\0".as_ptr(), m) } != 0 {
            return 0;
        }
    }
    let mut f = 0usize;
    if unsafe { os2::DosQueryProcAddr(*m, ord, core::ptr::null(), &mut f) } != 0 {
        return 0;
    }
    f
}

/// The printers (SplEnumQueue level 3).
pub fn printers() -> Vec<Printer> {
    let f = spl(399);
    if f == 0 {
        return Vec::new();
    }
    let enum_queue: extern "C" fn(*const u8, u32, *mut u8, u32, *mut u32, *mut u32, *mut u32, u32) -> u32 = unsafe { core::mem::transmute(f) };
    let mut buf = vec![0u8; 16384];
    let (mut ret, mut total, mut need) = (0u32, 0u32, 0u32);
    if enum_queue(core::ptr::null(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) != 0 {
        return Vec::new();
    }
    let s = |p: u32| unsafe { crate::cstr(p as *const u8) }.to_vec();
    let mut out = Vec::new();
    for i in 0..ret as usize {
        let rd = |o: usize| u32::from_le_bytes(buf[i * 44 + o..i * 44 + o + 4].try_into().unwrap());
        let ty = u16::from_le_bytes([buf[i * 44 + 10], buf[i * 44 + 11]]);
        let driver = s(rd(36));
        let dd = rd(40) as *const u8;
        let drivdata = if dd.is_null() {
            Vec::new()
        } else {
            let cb = unsafe { (dd as *const u32).read_unaligned() } as usize;
            if (40..0x10000).contains(&cb) { unsafe { core::slice::from_raw_parts(dd, cb) }.to_vec() } else { Vec::new() }
        };
        let device = if drivdata.len() >= 40 { unsafe { crate::cstr(drivdata.as_ptr().add(8)) }.to_vec() } else { driver.split(|&c| c == b'.').nth(1).unwrap_or(b"").to_vec() };
        let driver = driver.split(|&c| c == b'.').next().unwrap_or(b"").to_vec();
        // PRQ3_TYPE_APPDEFAULT
        out.push(Printer { queue: s(rd(0)), desc: s(rd(24)), driver, device, default: ty & 4 != 0, drivdata });
    }
    if !out.iter().any(|p| p.default) {
        if let Some(p) = out.first_mut() {
            p.default = true;
        }
    }
    out
}

/// The Windows device name of a printer: the queue's description.
pub fn device_name(p: &Printer) -> Vec<u8> {
    let d = if p.desc.is_empty() { &p.queue } else { &p.desc };
    // No commas (WIN.INI's lists use them) and 31 characters at most.
    d.iter().map(|&c| if c == b',' || c < b' ' { b' ' } else { c }).take(31).collect()
}

/// The Windows port of a printer: "QUEUE:".
pub fn port_name(p: &Printer) -> Vec<u8> {
    let mut s = p.queue.clone();
    s.push(b':');
    s
}

/// The printer named by a device name and a port (either may be empty).
pub fn find(list: &[Printer], dev: &[u8], port: &[u8]) -> Option<usize> {
    let port = port.strip_suffix(b":").unwrap_or(port);
    if !port.is_empty() {
        if let Some(i) = list.iter().position(|p| p.queue.eq_ignore_ascii_case(port)) {
            return Some(i);
        }
    }
    if !dev.is_empty() {
        let i = list.iter().position(|p| device_name(p).eq_ignore_ascii_case(dev) || p.queue.eq_ignore_ascii_case(dev) || p.device.eq_ignore_ascii_case(dev));
        if i.is_some() {
            return i;
        }
    }
    None
}

/// rs2's PostScript driver's job properties of a printer: the form
/// (index in its forms) and landscape. (The DRIVDATA after the device
/// name: "RSPS", form, orientation, copies.)
pub fn props(p: &Printer) -> (u32, bool) {
    let d = &p.drivdata;
    if d.len() >= 56 && &d[40..44] == b"RSPS" {
        let rd = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        return (rd(44), rd(48) != 0);
    }
    (0, false)
}

/// A printer's job properties with a DEVMODE's changes applied.
fn drivdata_with(p: &Printer, dm: Option<u32>) -> Vec<u8> {
    let mut d = p.drivdata.clone();
    let Some(dm) = dm.filter(|&dm| dm != 0) else { return d };
    if d.len() < 56 || &d[40..44] != b"RSPS" {
        return d;
    }
    let rd16 = |o: usize| unsafe { (flat(dm).add(o) as *const u16).read_unaligned() };
    let fields = rd16(40) as u32 | (rd16(42) as u32) << 16;
    let mut put = |o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    // DM_PAPERSIZE: DMPAPER_LETTER 1, LEGAL 5, EXECUTIVE 7, A3 8, A4 9,
    // A5 11 -> the driver's forms.
    if fields & 0x2 != 0 {
        let form = match rd16(46) {
            9 => 1,
            5 => 2,
            8 => 3,
            11 => 4,
            7 => 5,
            _ => 0,
        };
        put(44, form);
    }
    // DM_ORIENTATION
    if fields & 0x1 != 0 {
        put(48, (rd16(44) == 2) as u32);
    }
    // DM_COPIES
    if fields & 0x100 != 0 {
        put(52, rd16(54).clamp(1, 999) as u32);
    }
    d
}

/// A printer DC's state.
struct Pdc {
    hdc: u16,
    /// The PM DC.
    pm: u32,
    hps: u32,
    info: bool,
    abort: u32,
    in_doc: bool,
    band: bool,
    page: (i32, i32),
}

static PDCS: Global<Vec<Pdc>> = Global::new(Vec::new());

fn pdc(hdc: u16) -> Option<&'static mut Pdc> {
    PDCS.get().iter_mut().find(|p| p.hdc == hdc)
}

/// A DC on a printer (CreateDC, or CreateIC when not `output`).
pub fn create_dc(p: &Printer, dm: Option<u32>, output: bool) -> u16 {
    let mut q = p.queue.clone();
    q.push(0);
    let mut drv = if p.driver.is_empty() { b"PSCRIPT".to_vec() } else { p.driver.clone() };
    drv.push(0);
    let dd = drivdata_with(p, dm);
    let dop: [u32; 9] = [
        q.as_ptr() as u32,
        drv.as_ptr() as u32,
        if dd.is_empty() { 0 } else { dd.as_ptr() as u32 },
        b"PM_Q_RAW\0".as_ptr() as u32,
        b"Windows program\0".as_ptr() as u32,
        0,
        0,
        0,
        0,
    ];
    let hab = crate::kernel::TASK.get().hab;
    let pm = unsafe { DevOpenDC(hab, if output { OD_QUEUED } else { OD_INFO }, b"*\0".as_ptr(), 9, dop.as_ptr(), 0) };
    if pm == 0 {
        return 0;
    }
    let size = [0i32, 0];
    let hps = unsafe { GpiCreatePS(hab, pm, size.as_ptr(), PU_PELS | GPIA_ASSOC) };
    if hps == 0 {
        unsafe { DevCloseDC(pm) };
        return 0;
    }
    // CAPS_WIDTH, CAPS_HEIGHT
    let mut caps = [0i32; 2];
    unsafe { DevQueryCaps(pm, 4, 2, caps.as_mut_ptr()) };
    let hdc = gdi::new_dc(hps, 0, caps[1], gdi::DcKind::Printer);
    if hdc == 0 {
        unsafe {
            GpiAssociate(hps, 0);
            GpiDestroyPS(hps);
            DevCloseDC(pm);
        }
        return 0;
    }
    PDCS.get().push(Pdc { hdc, pm, hps, info: !output, abort: 0, in_doc: false, band: false, page: (caps[0], caps[1]) });
    hdc
}

/// A printer DC is going (DeleteDC): its PS and PM DC.
pub fn close(hdc: u16) {
    let list = PDCS.get();
    let Some(i) = list.iter().position(|p| p.hdc == hdc) else { return };
    let p = list.remove(i);
    unsafe {
        if p.in_doc {
            DevEscape(p.pm, DEVESC_ABORTDOC, 0, core::ptr::null(), core::ptr::null_mut(), core::ptr::null_mut());
        }
        GpiAssociate(p.hps, 0);
        GpiDestroyPS(p.hps);
        DevCloseDC(p.pm);
    }
}

fn escape(p: &Pdc, code: i32, data: &[u8]) -> bool {
    unsafe { DevEscape(p.pm, code, data.len() as i32, data.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut()) == 1 }
}

/// The program's abort procedure: go on printing?
fn keep_going(hdc: u16) -> bool {
    let Some(proc_) = pdc(hdc).map(|p| p.abort).filter(|&a| a != 0) else { return true };
    PascalCall::new().w(hdc).w(0).call(proc_) as u16 != 0
}

fn start_doc(hdc: u16, name: &[u8]) -> u32 {
    let Some(p) = pdc(hdc) else { return SP_ERROR };
    if p.info {
        return SP_ERROR;
    }
    if p.in_doc {
        return 1;
    }
    let name = if name.is_empty() { b"Windows document" as &[u8] } else { name };
    if !escape(p, DEVESC_STARTDOC, name) {
        return SP_ERROR;
    }
    p.in_doc = true;
    p.band = false;
    1
}

fn new_frame(hdc: u16) -> u32 {
    if !keep_going(hdc) {
        abort_doc(hdc);
        return SP_APPABORT;
    }
    let Some(p) = pdc(hdc) else { return SP_ERROR };
    if !p.in_doc || !escape(p, DEVESC_NEWFRAME, &[]) {
        return SP_ERROR;
    }
    gdi::page_done(hdc);
    1
}

fn end_doc(hdc: u16) -> u32 {
    let Some(p) = pdc(hdc) else { return SP_ERROR };
    if !p.in_doc {
        return SP_ERROR;
    }
    p.in_doc = false;
    let mut id = [0u8; 2];
    let mut n = 2;
    let ok = unsafe { DevEscape(p.pm, DEVESC_ENDDOC, 0, core::ptr::null(), &mut n, id.as_mut_ptr()) } == 1;
    if ok { 1 } else { SP_ERROR }
}

fn abort_doc(hdc: u16) -> u32 {
    let Some(p) = pdc(hdc) else { return SP_ERROR };
    if p.in_doc {
        p.in_doc = false;
        escape(p, DEVESC_ABORTDOC, &[]);
    }
    1
}

/// The GDI calls of printing (WIN16 ordinals), None for others (and for
/// CreateDC/CreateIC of the display: bitmap.rs's memory DCs).
pub fn call(idx: u32, a: &mut Pascal) -> Option<u32> {
    Some(match idx {
        // Escape(hdc, escape, cbIn, lpIn, lpOut)
        3038 => {
            let (hdc, esc, n, pin, pout) = (a.w(), a.i(), a.i(), a.l(), a.l());
            esc_call(hdc, esc, n, pin, pout)
        }
        // CreateDC(driver, device, output, lpInitData), CreateIC
        3053 | 3153 => {
            let (drv, dev, out, init) = (a.l(), a.l(), a.l(), a.l());
            let (drv, dev, out) = (far_str(drv).unwrap_or(b""), far_str(dev).unwrap_or(b""), far_str(out).unwrap_or(b""));
            if drv.is_empty() || drv.eq_ignore_ascii_case(b"DISPLAY") || drv.eq_ignore_ascii_case(b"DISPLAY.DRV") {
                return None;
            }
            let list = printers();
            let i = find(&list, dev, out).or_else(|| (dev.is_empty() && out.is_empty()).then(|| list.iter().position(|p| p.default)).flatten());
            let Some(i) = i else { return Some(0) };
            create_dc(&list[i], Some(init), idx == 3053) as u32
        }
        // QueryAbort(hdc, reserved)
        3155 => {
            let (hdc, _) = (a.w(), a.w());
            keep_going(hdc) as u32
        }
        // StartDoc(hdc, lpDocInfo): DOCINFO (cbSize, lpszDocName,
        // lpszOutput).
        3377 => {
            let (hdc, di) = (a.w(), a.l());
            let name = if di == 0 { None } else { far_str(unsafe { (flat(di).add(2) as *const u32).read_unaligned() }) };
            start_doc(hdc, name.unwrap_or(b""))
        }
        // EndDoc(hdc)
        3378 => end_doc(a.w()),
        // StartPage(hdc)
        3379 => {
            let hdc = a.w();
            if pdc(hdc).is_some_and(|p| p.in_doc) { 1 } else { 0 }
        }
        // EndPage(hdc)
        3380 => new_frame(a.w()),
        // SetAbortProc(hdc, proc)
        3381 => {
            let (hdc, p) = (a.w(), a.l());
            match pdc(hdc) {
                Some(d) => {
                    d.abort = p;
                    1
                }
                None => SP_ERROR,
            }
        }
        // AbortDoc(hdc)
        3382 => abort_doc(a.w()),
        _ => return None,
    })
}

fn esc_call(hdc: u16, esc: i32, n: i32, pin: u32, pout: u32) -> u32 {
    let in_word = || if pin == 0 { 0 } else { unsafe { (flat(pin) as *const u16).read_unaligned() } };
    let out_point = |x: i32, y: i32| {
        let p = flat(pout) as *mut i16;
        if !p.is_null() {
            unsafe {
                p.write_unaligned(x as i16);
                p.add(1).write_unaligned(y as i16);
            }
        }
    };
    let Some(page) = pdc(hdc).map(|p| p.page) else {
        // Escapes on the screen: QUERYESCSUPPORT says none.
        return 0;
    };
    match esc {
        // NEWFRAME
        1 => new_frame(hdc),
        // ABORTDOC
        2 => abort_doc(hdc),
        // NEXTBAND: the whole page, then an empty band (the page is done).
        3 => {
            let Some(p) = pdc(hdc) else { return SP_ERROR };
            let r = if p.band { [0, 0, 0, 0] } else { [0, 0, page.0, page.1] };
            let done = p.band;
            p.band = !p.band;
            crate::write_rect(pout, r);
            if done { new_frame(hdc) } else { 1 }
        }
        // QUERYESCSUPPORT(escape)
        8 => matches!(in_word(), 1..=3 | 6..=13 | 17 | 20..=23 | 0x100 | 4096 | 4097) as u32,
        // SETABORTPROC(proc)
        9 => {
            if let (Some(p), false) = (pdc(hdc), pin == 0) {
                p.abort = pin;
            }
            1
        }
        // STARTDOC(name)
        10 => {
            let name = if pin == 0 || n <= 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(flat(pin), n as usize) }.iter().copied().take_while(|&c| c != 0).collect() };
            start_doc(hdc, &name)
        }
        // ENDDOC
        11 => end_doc(hdc),
        // GETPHYSPAGESIZE
        12 => {
            out_point(page.0, page.1);
            1
        }
        // GETPRINTINGOFFSET, GETSCALINGFACTOR
        13 | 14 => {
            out_point(0, 0);
            1
        }
        // FLUSHOUTPUT, DRAFTMODE
        6 | 7 => {
            if let Some(p) = pdc(hdc) {
                let code = if esc == 6 { DEVESC_FLUSHOUTPUT } else { DEVESC_DRAFTMODE };
                if p.in_doc {
                    escape(p, code, &(in_word() as u32).to_le_bytes());
                }
            }
            1
        }
        // SETCOPYCOUNT(n): copies are the job properties' (one here).
        17 => {
            let p = flat(pout) as *mut u16;
            if !p.is_null() {
                unsafe { p.write_unaligned(1) };
            }
            1
        }
        // GETTECHNOLOGY
        20 => {
            crate::user::copy_out(b"PostScript", flat(pout), 32);
            1
        }
        // SETLINECAP, SETLINEJOIN, SETMITERLIMIT
        21..=23 => 1,
        _ => 0,
    }
}

/// GetDeviceCaps of a printer DC.
pub fn caps(hdc: u16, i: u16) -> Option<u32> {
    let p = pdc(hdc)?;
    let (w, h) = p.page;
    Some(match i {
        0 => 0x300,
        2 => 2,                            // TECHNOLOGY: DT_RASPRINTER
        4 => (w * 254 / 3000) as u32,      // HORZSIZE (mm)
        6 => (h * 254 / 3000) as u32,      // VERTSIZE
        8 => w as u32,                     // HORZRES
        10 => h as u32,                    // VERTRES
        12 => 1,                           // BITSPIXEL
        14 => 1,                           // PLANES
        16 => 0xffff,                      // NUMBRUSHES
        18 => 0xffff,                      // NUMPENS
        22 => 0,                           // NUMFONTS
        24 => 8,                           // NUMCOLORS
        26 => 0,                           // PDEVICESIZE
        28 => 0x01ff,                      // CURVECAPS
        30 => 0x00fe,                      // LINECAPS
        32 => 0x00ff,                      // POLYGONALCAPS
        34 => 0x7807,                      // TEXTCAPS: OP_CHARACTER, CP_STROKE, CR_90, SA_*, UA_ABLE...
        36 => 1,                           // CLIPCAPS: CP_RECTANGLE
        38 => 0x0001 | 0x0008 | 0x0080 | 0x0800 | 0x2000, // RASTERCAPS: BITBLT, BITMAP64, DI_BITMAP, STRETCHBLT, STRETCHDIB
        40 | 42 => 300,                    // ASPECTX, ASPECTY
        44 => 424,                         // ASPECTXY
        88 | 90 => 300,                    // LOGPIXELSX, LOGPIXELSY
        104..=108 => 0,
        _ => 0,
    })
}

/// WIN.INI's printer entries: [windows] device=, [devices] and
/// [PrinterPorts] (key None: the list of keys).
pub fn profile(app: &[u8], key: Option<&[u8]>) -> Option<Vec<u8>> {
    let app = app.to_ascii_lowercase();
    let list = || printers();
    let entry = |p: &Printer, ports: bool| {
        let mut s = if p.driver.is_empty() { b"PSCRIPT".to_vec() } else { p.driver.clone() };
        s.push(b',');
        s.extend_from_slice(&port_name(p));
        if ports {
            s.extend_from_slice(b",15,45");
        }
        s
    };
    match (&app[..], key.map(|k| k.to_ascii_lowercase())) {
        (b"windows", Some(k)) if k == b"device" => {
            let l = list();
            let p = l.iter().find(|p| p.default)?;
            let mut s = device_name(p);
            s.push(b',');
            s.extend_from_slice(&entry(p, false));
            Some(s)
        }
        (b"devices" | b"printerports", None) => {
            let mut s = Vec::new();
            for p in list() {
                s.extend_from_slice(&device_name(&p));
                s.push(0);
            }
            Some(s)
        }
        (b"devices" | b"printerports", Some(k)) => {
            let l = list();
            let p = l.iter().find(|p| device_name(p).eq_ignore_ascii_case(&k))?;
            Some(entry(p, app == b"printerports"))
        }
        _ => None,
    }
}
