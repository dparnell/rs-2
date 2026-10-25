//! Printer device contexts: rs2's PostScript printer driver
//! (docs/PRINTING.md), driver name `PSCRIPT`, device "Generic PostScript
//! Printer", 300 dpi.
//!
//! - OD_DIRECT: presentation spaces draw through crates/gpi's PostScript
//!   writer; the document goes to the printer's port through PMSPL's
//!   PrtOpen/PrtWrite (PMPRINT prints jobs this way). Opened with
//!   `DATATYPE=PM_Q_RAW` the DC takes DEVESC_RAWDATA as it is.
//! - OD_QUEUED with PM_Q_RAW: the same PostScript, spooled with SplQmOpen
//!   and SplQmWrite.
//! - OD_QUEUED with PM_Q_STD (the default): the drawing is recorded as a
//!   metafile (as on a metafile DC), spooled at DEVESC_ENDDOC; PMPRINT
//!   plays it on an OD_DIRECT DC. DEVESC_NEWFRAME is recorded as a device
//!   escape (rs2's escape order 0x80).
//! - OD_INFO: answers queries (DevQueryCaps, fonts, the page) and draws
//!   nothing.
//!
//! The job properties are a DRIVDATA (szDeviceName, then rs2's data:
//! "RSPS", form, orientation, copies), from the DEVOPENSTRUC or the
//! queue; DevPostDeviceModes shows and changes them.
//!
//! PMSPL is loaded when first needed (it does not load PMWIN either), and
//! its calls run without the PM lock (writing to a network printer can
//! take a while).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use gpi::psdev::{PsWriter, Sink, FORMS};
use gpi::Rect;

pub const DPI: i32 = 300;
pub const DRIVER: &[u8] = b"PSCRIPT";
pub const DEVICE: &[u8] = b"Generic PostScript Printer";

const OD_QUEUED: i32 = 2;
const OD_DIRECT: i32 = 5;

pub const DEVESC_QUERYESCSUPPORT: i32 = 0;
pub const DEVESC_GETCP: i32 = 8000;
pub const DEVESC_STARTDOC: i32 = 8150;
pub const DEVESC_ENDDOC: i32 = 8151;
pub const DEVESC_ABORTDOC: i32 = 8153;
pub const DEVESC_GETJOBID: i32 = 8160;
pub const DEVESC_QUERYJOBPROPERTIES: i32 = 8163;
pub const DEVESC_SETJOBPROPERTIES: i32 = 8164;
pub const DEVESC_DEFAULTJOBPROPERTIES: i32 = 8165;
pub const DEVESC_NEWFRAME: i32 = 16300;
pub const DEVESC_DRAFTMODE: i32 = 16301;
pub const DEVESC_FLUSHOUTPUT: i32 = 16302;
pub const DEVESC_RAWDATA: i32 = 16303;
pub const DEVESC_SETMODE: i32 = 16304;
const DEVESC_ERROR: i32 = -1;
const DEVESC_NOTIMPLEMENTED: i32 = 0;
const DEV_OK: i32 = 1;

/// The metafile escape order a recorded DevEscape becomes (rs2's own
/// identifier): the escape code (LONG) and its input.
pub const ESC_DEVESCAPE: u8 = 0x80;

// ---- PMSPL, loaded on demand ----

const SPL_ORDS: [u32; 13] = [301, 302, 304, 303, 305, 306, 308, 370, 371, 373, 375, 397, 381];

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Spl {
    QmOpen,
    QmStartDoc,
    QmWrite,
    QmEndDoc,
    QmAbort,
    QmClose,
    QmAbortDoc,
    PrtOpen,
    PrtWrite,
    PrtClose,
    PrtAbort,
    QueryQueue,
    QueryDevice,
}

static mut SPL: [usize; 13] = [0; 13];

fn spl(f: Spl) -> usize {
    unsafe {
        let t = &mut *core::ptr::addr_of_mut!(SPL);
        if t[0] == 0 {
            let mut hmod = 0;
            let mut fail = [0u8; 16];
            if os2::DosLoadModule(fail.as_mut_ptr(), 16, b"PMSPL\0".as_ptr(), &mut hmod) != 0 {
                return 0;
            }
            for (i, &o) in SPL_ORDS.iter().enumerate() {
                let mut p = 0usize;
                os2::DosQueryProcAddr(hmod, o, core::ptr::null(), &mut p);
                t[i] = p;
            }
        }
        t[f as usize]
    }
}

/// Call a PMSPL function (by its index) with the PM lock released.
macro_rules! spl_call {
    ($f:expr, fn($($t:ty),*) -> $r:ty, $($a:expr),*) => {{
        let p = spl($f);
        if p == 0 {
            None
        } else {
            let f: unsafe extern "C" fn($($t),*) -> $r = unsafe { core::mem::transmute(p) };
            Some(crate::lock::unlocked(|| unsafe { f($($a),*) }))
        }
    }};
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// A DRIVDATA's bytes.
fn drivdata(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let cb = unsafe { (p as *const u32).read_unaligned() } as usize;
    if !(40..=0x10000).contains(&cb) {
        return Vec::new();
    }
    unsafe { core::slice::from_raw_parts(p, cb) }.to_vec()
}

/// A queue's driver and job properties (PRQINFO3 pszDriverName,
/// pDriverData), None when there is no such queue.
fn queue_driver(q: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let name = z(q);
    let mut buf = vec![0u8; 4096];
    let mut need = 0u32;
    let rc = spl_call!(Spl::QueryQueue, fn(*const u8, *const u8, u32, *mut u8, u32, *mut u32) -> u32, core::ptr::null(), name.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need)?;
    if rc != 0 {
        return None;
    }
    let rd = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap()) as *const u8;
    Some((cstr(rd(36)).to_vec(), drivdata(rd(40))))
}

// ---- Job properties ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Props {
    /// Index in FORMS.
    pub form: u32,
    pub landscape: bool,
    pub copies: u32,
}

impl Default for Props {
    fn default() -> Props {
        Props { form: 0, landscape: false, copies: 1 }
    }
}

impl Props {
    pub fn from_drivdata(d: &[u8]) -> Props {
        let mut p = Props::default();
        if d.len() >= 56 && &d[40..44] == b"RSPS" {
            let rd = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
            p.form = rd(44).min(FORMS.len() as u32 - 1);
            p.landscape = rd(48) != 0;
            p.copies = rd(52).clamp(1, 999);
        }
        p
    }

    pub fn drivdata(&self, device: &[u8]) -> Vec<u8> {
        let mut d = Vec::with_capacity(56);
        d.extend_from_slice(&56u32.to_le_bytes());
        d.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        let mut name = [0u8; 32];
        let k = device.len().min(31);
        name[..k].copy_from_slice(&device[..k]);
        d.extend_from_slice(&name);
        d.extend_from_slice(b"RSPS");
        for v in [self.form, self.landscape as u32, self.copies] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d
    }

    /// Paper size in points (portrait).
    pub fn paper(&self) -> (i32, i32) {
        let f = FORMS[self.form as usize];
        (f.1, f.2)
    }

    /// The page in device pels, as drawn (landscape: wide).
    pub fn pels(&self) -> (i32, i32) {
        let (w, h) = self.paper();
        let (w, h) = (w * DPI / 72, h * DPI / 72);
        if self.landscape { (h, w) } else { (w, h) }
    }
}

// ---- The device context ----

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Direct,
    Raw,
    Std,
    Info,
}

enum Out {
    None,
    Prt(u32),
    Spl(u32),
}

pub struct PrinterDc {
    pub kind: Kind,
    /// The queue (queued DCs) or printer (OD_DIRECT) name.
    logaddr: Vec<u8>,
    driver: Vec<u8>,
    device: Vec<u8>,
    pub props: Props,
    /// DEVOPENSTRUC strings for SplQmOpen: comment, queue processor
    /// name and parameters, spooler and network parameters.
    comment: Vec<u8>,
    qproc: Vec<u8>,
    qproc_params: Vec<u8>,
    spl_params: Vec<u8>,
    net_params: Vec<u8>,
    /// The PostScript writer (OD_DIRECT, PM_Q_RAW).
    pub writer: Option<Box<dyn Sink>>,
    /// The metafile being recorded (PM_Q_STD).
    pub meta: Option<Box<crate::gpiseg::MetaRec>>,
    doc: bool,
    doc_name: Vec<u8>,
    out: Out,
    pub job: u32,
    /// OD_DIRECT opened for raw data (DATATYPE=PM_Q_RAW).
    raw_only: bool,
    failed: bool,
}

/// Whether DevOpenDC asks for a printer: OD_QUEUED and OD_DIRECT always,
/// OD_INFO when the DEVOPENSTRUC names a queue, printer or driver other
/// than the display.
pub fn wants_printer(ty: i32, n: i32, dop: *const u32) -> bool {
    match ty {
        OD_QUEUED | OD_DIRECT => true,
        6 => {
            if n < 2 || dop.is_null() {
                return false;
            }
            let s = |i: usize| cstr((unsafe { dop.add(i).read_unaligned() }) as *const u8);
            let (addr, drv) = (s(0), s(1));
            !(addr.is_empty() && drv.is_empty()) && !drv.eq_ignore_ascii_case(b"DISPLAY")
        }
        _ => false,
    }
}

/// DevOpenDC for a printer.
pub fn open(ty: i32, n: i32, dop: *const u32) -> Option<PrinterDc> {
    let item = |i: i32| -> u32 { if i < n && !dop.is_null() { unsafe { dop.add(i as usize).read_unaligned() } } else { 0 } };
    let s = |i: i32| cstr(item(i) as *const u8).to_vec();
    let logaddr = s(0);
    let mut driver = s(1);
    let mut dd = drivdata(item(2) as *const u8);
    let datatype = s(3);
    let qparams = s(6);
    // What the queue says when the caller leaves it out.
    if (driver.is_empty() || dd.is_empty())
        && !logaddr.is_empty()
        && let Some((qd, qdd)) = queue_driver(&logaddr)
    {
        if driver.is_empty() {
            driver = qd;
        }
        if dd.is_empty() {
            dd = qdd;
        }
    }
    // "PSCRIPT.Generic PostScript Printer" names the device too.
    let mut device = DEVICE.to_vec();
    if let Some(dot) = driver.iter().position(|&c| c == b'.') {
        device = driver[dot + 1..].to_vec();
        driver.truncate(dot);
    }
    if dd.len() >= 40 {
        let name = &dd[8..40];
        let k = name.iter().position(|&c| c == 0).unwrap_or(32);
        if k > 0 {
            device = name[..k].to_vec();
        }
    }
    let props = Props::from_drivdata(&dd);
    let raw_only = ty == OD_DIRECT && qparams.windows(8).any(|w| w.eq_ignore_ascii_case(b"PM_Q_RAW"));
    let kind = match ty {
        OD_DIRECT => Kind::Direct,
        OD_QUEUED if datatype.eq_ignore_ascii_case(b"PM_Q_RAW") => Kind::Raw,
        OD_QUEUED => Kind::Std,
        _ => Kind::Info,
    };
    let writer: Option<Box<dyn Sink>> = match kind {
        Kind::Direct | Kind::Raw => {
            let mut w = PsWriter::new(DPI, props.paper(), props.landscape);
            w.copies = props.copies;
            Some(Box::new(w))
        }
        _ => None,
    };
    let meta = (kind == Kind::Std).then(|| Box::new(crate::gpiseg::MetaRec::new()));
    Some(PrinterDc {
        kind,
        logaddr,
        driver: if driver.is_empty() { DRIVER.to_vec() } else { driver },
        device,
        props,
        comment: s(4),
        qproc: s(5),
        qproc_params: qparams,
        spl_params: s(7),
        net_params: s(8),
        writer,
        meta,
        doc: false,
        doc_name: Vec::new(),
        out: Out::None,
        job: 0,
        raw_only,
        failed: false,
    })
}

impl PrinterDc {
    /// The page (device rectangle).
    pub fn page(&self) -> Rect {
        let (w, h) = self.props.pels();
        Rect::new(0, 0, w, h)
    }

    fn ps(&mut self) -> Option<&mut PsWriter> {
        self.writer.as_mut()?.as_any().downcast_mut::<PsWriter>()
    }

    /// Open the job's output (the port, or a spool job).
    fn open_out(&mut self) -> bool {
        if !matches!(self.out, Out::None) {
            return true;
        }
        match self.kind {
            Kind::Direct => {
                let name = z(&self.logaddr);
                let (mut h, mut act) = (0u32, 0u32);
                let rc = spl_call!(Spl::PrtOpen, fn(*const u8, *mut u32, *mut u32, u32, u32, u32, u32, u32) -> u32, name.as_ptr(), &mut h, &mut act, 0, 0, 0x11, 0x41, 0);
                if rc != Some(0) {
                    return false;
                }
                self.out = Out::Prt(h);
                true
            }
            Kind::Raw | Kind::Std => {
                let strs = [z(&self.logaddr), z(&self.driver), z(if self.kind == Kind::Raw { b"PM_Q_RAW" } else { b"PM_Q_STD" }), z(&self.comment), z(&self.qproc), z(&self.qproc_params), z(&self.spl_params), z(&self.net_params)];
                let dd = self.props.drivdata(&self.device);
                let dop = [strs[0].as_ptr() as u32, strs[1].as_ptr() as u32, dd.as_ptr() as u32, strs[2].as_ptr() as u32, strs[3].as_ptr() as u32, strs[4].as_ptr() as u32, strs[5].as_ptr() as u32, strs[6].as_ptr() as u32, strs[7].as_ptr() as u32];
                let h = spl_call!(Spl::QmOpen, fn(*const u8, i32, *const u32) -> u32, b"*\0".as_ptr(), 9, dop.as_ptr()).unwrap_or(0);
                if h == 0 {
                    return false;
                }
                let name = z(&self.doc_name);
                spl_call!(Spl::QmStartDoc, fn(u32, *const u8) -> u32, h, name.as_ptr());
                self.out = Out::Spl(h);
                true
            }
            Kind::Info => false,
        }
    }

    fn write(&mut self, d: &[u8]) -> bool {
        if d.is_empty() {
            return true;
        }
        if !self.open_out() {
            return false;
        }
        match self.out {
            Out::Prt(h) => {
                let mut n = 0u32;
                spl_call!(Spl::PrtWrite, fn(u32, *const u8, u32, *mut u32) -> u32, h, d.as_ptr(), d.len() as u32, &mut n) == Some(0)
            }
            Out::Spl(h) => spl_call!(Spl::QmWrite, fn(u32, i32, *const u8) -> u32, h, d.len() as i32, d.as_ptr()) == Some(1),
            Out::None => false,
        }
    }

    /// Send what the writer has finished.
    fn flush(&mut self) -> bool {
        let Some(w) = self.ps() else { return true };
        let d = w.take();
        let ok = self.write(&d);
        if !ok {
            self.failed = true;
        }
        ok
    }

    fn start_doc(&mut self, name: &[u8]) -> bool {
        if self.doc {
            return false;
        }
        self.doc = true;
        self.failed = false;
        self.doc_name = name.to_vec();
        self.job = 0;
        match self.kind {
            Kind::Direct | Kind::Raw => {
                let raw_only = self.raw_only;
                if let Some(w) = self.ps() {
                    w.title = name.to_vec();
                    if !raw_only {
                        w.begin_doc();
                    }
                }
                // Raw DCs open their output with the document (a printer
                // that cannot be reached fails STARTDOC).
                self.open_out()
            }
            Kind::Std => {
                if let Some(m) = self.meta.as_mut() {
                    m.segs = vec![(0, Vec::new())];
                }
                true
            }
            Kind::Info => false,
        }
    }

    fn close_out(&mut self, abort: bool) -> u32 {
        let mut job = 0;
        match core::mem::replace(&mut self.out, Out::None) {
            Out::Prt(h) => {
                if abort {
                    spl_call!(Spl::PrtAbort, fn(u32) -> u32, h);
                } else if spl_call!(Spl::PrtClose, fn(u32) -> u32, h) != Some(0) {
                    self.failed = true;
                }
            }
            Out::Spl(h) => {
                if abort {
                    spl_call!(Spl::QmAbort, fn(u32) -> u32, h);
                } else {
                    job = spl_call!(Spl::QmEndDoc, fn(u32) -> u32, h).unwrap_or(0);
                    spl_call!(Spl::QmClose, fn(u32) -> u32, h);
                    if job == 0 {
                        self.failed = true;
                    }
                }
            }
            Out::None => {}
        }
        job
    }

    fn end_doc(&mut self) -> bool {
        if !self.doc {
            return false;
        }
        self.doc = false;
        match self.kind {
            Kind::Direct | Kind::Raw => {
                if let Some(w) = self.ps() {
                    if w.in_doc() || w.pages() > 0 {
                        w.end_doc();
                    }
                }
                self.flush();
            }
            Kind::Std => {
                let bits = self.meta.as_ref().map(|m| m.build()).unwrap_or_default();
                if !self.write(&bits) {
                    self.failed = true;
                }
            }
            Kind::Info => return false,
        }
        self.job = self.close_out(false);
        !self.failed
    }

    fn abort_doc(&mut self) {
        self.doc = false;
        if let Some(w) = self.ps() {
            let _ = w.take();
            *w = {
                let (p, l, c) = (w.paper, w.landscape, w.copies);
                let mut n = PsWriter::new(DPI, p, l);
                n.copies = c;
                n
            };
        }
        self.close_out(true);
    }

    /// DevCloseDC: a document still open is ended.
    pub fn close(&mut self) {
        if self.doc {
            self.end_doc();
        } else if !matches!(self.out, Out::None) {
            // Drawing without STARTDOC: finish what there is.
            if let Some(w) = self.ps() {
                w.end_doc();
            }
            self.flush();
            self.close_out(false);
        }
    }

    /// Drawing happened: OD_DIRECT/raw DCs without STARTDOC start the
    /// document implicitly when a page is sent.
    fn ensure_doc(&mut self) {
        if !self.doc && matches!(self.kind, Kind::Direct | Kind::Raw) {
            if let Some(w) = self.ps() {
                w.begin_doc();
            }
            self.open_out();
        }
    }

    pub fn escape(&mut self, code: i32, input: &[u8], nout: *mut i32, pout: *mut u8) -> i32 {
        let out_len = if nout.is_null() { 0 } else { unsafe { nout.read_unaligned() }.max(0) as usize };
        let put = |d: &[u8]| {
            if !pout.is_null() && !nout.is_null() {
                let k = d.len().min(out_len);
                unsafe {
                    core::ptr::copy_nonoverlapping(d.as_ptr(), pout, k);
                    nout.write_unaligned(k as i32);
                }
            }
        };
        match code {
            DEVESC_QUERYESCSUPPORT => {
                let c = if input.len() >= 4 { i32::from_le_bytes(input[..4].try_into().unwrap()) } else { -1 };
                let ok = matches!(c, DEVESC_QUERYESCSUPPORT | DEVESC_GETCP | DEVESC_STARTDOC | DEVESC_ENDDOC | DEVESC_ABORTDOC | DEVESC_GETJOBID | DEVESC_QUERYJOBPROPERTIES | DEVESC_SETJOBPROPERTIES | DEVESC_DEFAULTJOBPROPERTIES | DEVESC_NEWFRAME | DEVESC_DRAFTMODE | DEVESC_FLUSHOUTPUT | DEVESC_RAWDATA | DEVESC_SETMODE);
                if ok { DEV_OK } else { DEVESC_NOTIMPLEMENTED }
            }
            DEVESC_GETCP => {
                put(&850u16.to_le_bytes());
                DEV_OK
            }
            DEVESC_STARTDOC => {
                let name = &input[..input.iter().position(|&c| c == 0).unwrap_or(input.len())];
                if self.start_doc(name) { DEV_OK } else { DEVESC_ERROR }
            }
            DEVESC_ENDDOC => {
                let ok = self.end_doc();
                put(&(self.job as u16).to_le_bytes());
                if ok { DEV_OK } else { DEVESC_ERROR }
            }
            DEVESC_ABORTDOC => {
                self.abort_doc();
                DEV_OK
            }
            DEVESC_GETJOBID => {
                put(&self.job.to_le_bytes());
                DEV_OK
            }
            DEVESC_NEWFRAME => match self.kind {
                Kind::Std => {
                    if let Some(m) = self.meta.as_mut() {
                        m.escape(ESC_DEVESCAPE, &DEVESC_NEWFRAME.to_le_bytes());
                    }
                    DEV_OK
                }
                Kind::Direct | Kind::Raw => {
                    self.ensure_doc();
                    if let Some(w) = self.ps() {
                        w.new_frame();
                    }
                    if self.flush() { DEV_OK } else { DEVESC_ERROR }
                }
                Kind::Info => DEVESC_ERROR,
            },
            DEVESC_RAWDATA => match self.kind {
                Kind::Direct | Kind::Raw => {
                    self.ensure_doc();
                    if let Some(w) = self.ps() {
                        w.raw(input);
                    }
                    if self.flush() { DEV_OK } else { DEVESC_ERROR }
                }
                _ => DEVESC_ERROR,
            },
            DEVESC_FLUSHOUTPUT => {
                if self.flush() { DEV_OK } else { DEVESC_ERROR }
            }
            DEVESC_DRAFTMODE | DEVESC_SETMODE => DEV_OK,
            DEVESC_QUERYJOBPROPERTIES | DEVESC_DEFAULTJOBPROPERTIES => {
                let p = if code == DEVESC_DEFAULTJOBPROPERTIES { Props::default() } else { self.props };
                let d = p.drivdata(&self.device);
                if !nout.is_null() && out_len == 0 {
                    unsafe { nout.write_unaligned(d.len() as i32) };
                    return DEV_OK;
                }
                put(&d);
                DEV_OK
            }
            DEVESC_SETJOBPROPERTIES => {
                if input.len() < 56 || self.doc {
                    return DEVESC_ERROR;
                }
                self.props = Props::from_drivdata(input);
                let p = self.props;
                if let Some(w) = self.ps() {
                    w.paper = p.paper();
                    w.landscape = p.landscape;
                    w.copies = p.copies;
                }
                DEV_OK
            }
            _ => DEVESC_NOTIMPLEMENTED,
        }
    }

    /// DevQueryCaps values.
    pub fn caps(&self, i: i32, kind: i32) -> i32 {
        let (w, h) = self.props.pels();
        let ppm = DPI * 10000 / 254;
        let em = DPI * 10 / 72;
        match i {
            0 => kind,
            1 => 2,                          // CAPS_IO_SUPPORTS_OP
            2 => 5,                          // CAPS_TECH_POSTSCRIPT
            3 => 0x0300,
            4 => w,
            5 => h,
            6 => w / (em * 6 / 10).max(1),
            7 => h / em,
            8 | 9 => ppm,
            10 | 35 => em * 6 / 10,
            11 | 36 => em,
            12 => em / 2,
            13 => em * 8 / 10,
            14 | 33 => 1 << 24,
            15 => 1,
            16 => 24,
            17 => 1 | 4,                     // RGB_8, TRUE_MIX
            19 => 2 | 16,                    // overpaint, leave alone
            20 => 2 | 16,
            22 => 2,
            23 => 4,
            24 => 1 | 4 | 16,                // bitblt, scaling, set pel
            25 | 26 => DPI / 30,
            27 => 0,
            29 => 0x200,
            31 => 1,
            32 => 4 | 512 | 4096,            // outline default font, wide lines, clip fills
            34 => 255,
            37 | 38 => DPI,
            39 => 1 | 2 | 4,                 // simulated bold, italic, underscore
            40 => DPI / 50,
            _ => 0,
        }
    }

    /// DevQueryHardcopyCaps: HCINFO records (68 bytes: szFormname[32],
    /// cx, cy, xLeftClip, yBottomClip, xRightClip, yTopClip in mm, xPels,
    /// yPels, flAttributes).
    pub fn hardcopy(&self, start: i32, n: i32, out: *mut u8) -> i32 {
        let total = FORMS.len() as i32;
        if n == 0 {
            return total;
        }
        if start < 0 || out.is_null() {
            return -1;
        }
        let mut k = 0;
        for (i, f) in FORMS.iter().enumerate().skip(start as usize).take(n.max(0) as usize) {
            let mut r = [0u8; 68];
            r[..f.0.len()].copy_from_slice(f.0.as_bytes());
            let mm = |pt: i32| pt * 254 / 720;
            let (cx, cy) = if self.props.landscape { (mm(f.2), mm(f.1)) } else { (mm(f.1), mm(f.2)) };
            let (px, py) = if self.props.landscape { (f.2 * DPI / 72, f.1 * DPI / 72) } else { (f.1 * DPI / 72, f.2 * DPI / 72) };
            let attr = 2 | if i as u32 == self.props.form { 1 } else { 0 };
            for (j, v) in [cx, cy, 0, 0, cx, cy, px, py, attr].iter().enumerate() {
                r[32 + 4 * j..36 + 4 * j].copy_from_slice(&v.to_le_bytes());
            }
            unsafe { core::ptr::copy_nonoverlapping(r.as_ptr(), out.add(68 * k), 68) };
            k += 1;
        }
        k as i32
    }
}

/// The device default font: Courier, 10 points at the printer's
/// resolution (an outline font).
pub fn default_font() -> Option<&'static gpi::font::Font> {
    let i = crate::fonts::face_index(b"Courier")?;
    crate::fonts::face(i, DPI * 10 / 72)
}

/// DevQueryDeviceNames(hab, pszDriverName, pldn, aDeviceName,
/// aDeviceDesc, pldt, aDataType): PSCRIPT has one device.
pub fn device_names(driver: &[u8], ndev: *mut i32, names: *mut u8, descs: *mut u8, ntypes: *mut i32, types: *mut u8) -> bool {
    let d = &driver[..driver.iter().position(|&c| c == b'.').unwrap_or(driver.len())];
    if !d.eq_ignore_ascii_case(DRIVER) {
        return false;
    }
    let room_d = if ndev.is_null() { 0 } else { unsafe { ndev.read_unaligned() } };
    let room_t = if ntypes.is_null() { 0 } else { unsafe { ntypes.read_unaligned() } };
    if room_d >= 1 {
        for (p, len) in [(names, 32usize), (descs, 64)] {
            if !p.is_null() {
                unsafe {
                    core::ptr::write_bytes(p, 0, len);
                    core::ptr::copy_nonoverlapping(DEVICE.as_ptr(), p, DEVICE.len());
                }
            }
        }
    }
    if room_t >= 2 && !types.is_null() {
        unsafe {
            core::ptr::write_bytes(types, 0, 32);
            core::ptr::copy_nonoverlapping(b"PM_Q_STD".as_ptr(), types, 8);
            core::ptr::copy_nonoverlapping(b"PM_Q_RAW".as_ptr(), types.add(16), 8);
        }
    }
    if !ndev.is_null() {
        unsafe { ndev.write_unaligned(1) };
    }
    if !ntypes.is_null() {
        unsafe { ntypes.write_unaligned(2) };
    }
    true
}

// ---- Job properties dialog (DevPostDeviceModes) ----

pub const DPDM_ERROR: i32 = -1;
pub const DPDM_NONE: i32 = 0;
const DPDM_QUERYJOBPROP: u32 = 2;

const ID_FORM: u32 = 101;
const ID_PORTRAIT: u32 = 102;
const ID_LANDSCAPE: u32 = 103;
const ID_COPIES: u32 = 104;

static mut DLG_PROPS: Props = Props { form: 0, landscape: false, copies: 1 };

fn item(class: &[u8], text: &[u8], style: u32, x: i32, y: i32, cx: i32, cy: i32, id: u32) -> crate::res::DlgItemT {
    crate::res::DlgItemT { class: class.to_vec(), text: text.to_vec(), style, x, y, cx, cy, id, ctl: core::ptr::null(), pres: core::ptr::null(), children: Vec::new() }
}

extern "C" fn props_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    const WM_INITDLG: u32 = 0x3b;
    const WM_COMMAND: u32 = 0x20;
    const LM_INSERTITEM: u32 = 0x161;
    const LM_SELECTITEM: u32 = 0x164;
    const LM_QUERYSELECTION: u32 = 0x165;
    const BM_SETCHECK: u32 = 0x125;
    const BM_QUERYCHECK: u32 = 0x124;
    let send = |id: u32, m: u32, a: u32, b: u32| crate::dlg::WinSendDlgItemMsg(h, id, m, a, b);
    match msg {
        WM_INITDLG => {
            let p = unsafe { DLG_PROPS };
            for f in FORMS {
                let t = z(f.0.as_bytes());
                send(ID_FORM, LM_INSERTITEM, 0xffff, t.as_ptr() as u32);
            }
            send(ID_FORM, LM_SELECTITEM, p.form, 1);
            send(if p.landscape { ID_LANDSCAPE } else { ID_PORTRAIT }, BM_SETCHECK, 1, 0);
            crate::dlg::WinSetDlgItemShort(h, ID_COPIES, p.copies, 0);
            0
        }
        WM_COMMAND => {
            let id = mp1 & 0xffff;
            if id == 1 {
                let mut p = unsafe { DLG_PROPS };
                let sel = send(ID_FORM, LM_QUERYSELECTION, 0xffff, 0);
                if (sel as usize) < FORMS.len() {
                    p.form = sel;
                }
                p.landscape = send(ID_LANDSCAPE, BM_QUERYCHECK, 0, 0) != 0;
                let mut c = 0i16;
                if crate::dlg::WinQueryDlgItemShort(h, ID_COPIES, &mut c, 0) != 0 && c > 0 {
                    p.copies = c as u32;
                }
                unsafe { DLG_PROPS = p };
            }
            crate::dlg::WinDismissDlg(h, id);
            0
        }
        _ => crate::dlg::WinDefDlgProc(h, msg, mp1, mp2),
    }
}

/// The job properties dialog: true when OK was pressed.
fn props_dialog(title: &[u8], p: &mut Props) -> bool {
    const WS_VISIBLE: u32 = 0x8000_0000;
    const WS_GROUP: u32 = 0x0001_0000;
    const WS_TABSTOP: u32 = 0x0002_0000;
    const FS_DLGBORDER: u32 = 0x80;
    const BS_AUTORADIOBUTTON: u32 = 4;
    const BS_DEFAULT: u32 = 0x400;
    const SS_TEXT: u32 = 1;
    const ES_MARGIN: u32 = 8;
    const LS_NOADJUSTPOS: u32 = 4;
    /// The frame's control data: FCF_TITLEBAR | FCF_SYSMENU.
    static FCF: u32 = 3;
    unsafe { DLG_PROPS = *p };
    let mut dlg = item(b"#1", title, FS_DLGBORDER | WS_VISIBLE, 40, 40, 200, 120, 0);
    dlg.ctl = &FCF as *const u32 as *const u8;
    dlg.children = vec![
        item(b"#5", b"Form", SS_TEXT | WS_VISIBLE, 8, 102, 60, 8, 0xffff),
        item(b"#7", b"", LS_NOADJUSTPOS | WS_VISIBLE | WS_TABSTOP | WS_GROUP, 8, 46, 90, 54, ID_FORM),
        item(b"#5", b"Orientation", SS_TEXT | WS_VISIBLE, 108, 102, 80, 8, 0xffff),
        item(b"#3", b"~Portrait", BS_AUTORADIOBUTTON | WS_VISIBLE | WS_TABSTOP | WS_GROUP, 108, 88, 80, 10, ID_PORTRAIT),
        item(b"#3", b"~Landscape", BS_AUTORADIOBUTTON | WS_VISIBLE, 108, 76, 80, 10, ID_LANDSCAPE),
        item(b"#5", b"Copies", SS_TEXT | WS_VISIBLE, 108, 58, 40, 8, 0xffff),
        item(b"#6", b"1", ES_MARGIN | WS_VISIBLE | WS_TABSTOP | WS_GROUP, 150, 58, 30, 8, ID_COPIES),
        item(b"#3", b"OK", BS_DEFAULT | WS_VISIBLE | WS_TABSTOP | WS_GROUP, 8, 8, 50, 14, 1),
        item(b"#3", b"Cancel", WS_VISIBLE | WS_TABSTOP, 64, 8, 50, 14, 2),
    ];
    let h = crate::dlg::create_from(1, 1, props_proc as *const () as u32, &dlg, 0);
    if h == 0 {
        return false;
    }
    let r = crate::dlg::WinProcessDlg(h);
    crate::win::WinDestroyWindow(h);
    if r == 1 {
        *p = unsafe { DLG_PROPS };
        true
    } else {
        false
    }
}

/// DevPostDeviceModes(hab, pdrivDriverData, pszDriverName, pszDeviceName,
/// pszName, flOptions): with no DRIVDATA, its size; DPDM_QUERYJOBPROP
/// fills it in; DPDM_POSTJOBPROP and DPDM_CHANGEPROP show the job
/// properties dialog (DPDM_NONE when cancelled).
pub fn post_device_modes(drv: *mut u8, driver: &[u8], device: &[u8], name: &[u8], fl: u32) -> i32 {
    let d = &driver[..driver.iter().position(|&c| c == b'.').unwrap_or(driver.len())];
    if !d.is_empty() && !d.eq_ignore_ascii_case(DRIVER) {
        return DPDM_ERROR;
    }
    let device = if device.is_empty() { DEVICE } else { device };
    if drv.is_null() {
        return 56;
    }
    let given = drivdata(drv);
    let mut p = if given.len() >= 56 { Props::from_drivdata(&given) } else { Props::default() };
    if fl != DPDM_QUERYJOBPROP {
        let mut title = b"Job properties".to_vec();
        if !name.is_empty() {
            title = [name, b" - Job properties"].concat();
        }
        if !props_dialog(&title, &mut p) {
            return DPDM_NONE;
        }
    }
    let out = p.drivdata(device);
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), drv, out.len()) };
    1
}

/// Is a queue or printer known to the spooler? (DevOpenDC refuses
/// printers that do not exist.)
pub fn known(name: &[u8]) -> bool {
    if name.is_empty() {
        return true;
    }
    if queue_driver(name).is_some() {
        return true;
    }
    let n = z(name);
    let mut buf = [0u8; 16];
    let mut need = 0u32;
    // NERR_BufTooSmall (2123) means it exists.
    matches!(spl_call!(Spl::QueryDevice, fn(*const u8, *const u8, u32, *mut u8, u32, *mut u32) -> u32, core::ptr::null(), n.as_ptr(), 3, buf.as_mut_ptr(), 0, &mut need), Some(0) | Some(2123))
}
