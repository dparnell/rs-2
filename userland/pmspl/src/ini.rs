//! The spooler's configuration in OS2SYS.INI, in IBM's layout:
//!
//! | Application | Key | Value |
//! |-------------|-----|-------|
//! | PM_SPOOLER | DIR, QUEUE, PRINTER | `C:\SPOOL;`, the default queue and printer (`NAME;`) |
//! | PM_SPOOLER_QUEUE | queue | `QPROC;SEPARATOR;QPROC PARAMETERS;` |
//! | PM_SPOOLER_QUEUE_DESCR | queue | `description;` |
//! | PM_SPOOLER_QUEUE_DD | queue | `DRIVER.DEVICE,...;` |
//! | PM_SPOOLER_QUEUE_DDDATA | queue | the DRIVDATA (binary) |
//! | PM_SPOOLER_QUEUE_STATUS | queue | `priority,start,until,held,` |
//! | PM_SPOOLER_QUEUE_TYPE | queue | `APPLDEFAULT;` and other flags |
//! | PM_SPOOLER_PRINTER | printer | `PORT;DRIVER.DEVICE,...;QUEUE,...;HELD;TIMEOUT;` |
//! | PM_SPOOLER_PRINTER_DESCR | printer | `description;` |
//! | PM_SPOOLER_PORT | port | the port's settings (`host;queue;port;` for LPR) |
//! | PM_SPOOLER_QP | queue processor | `C:\OS2\DLL\PMPRINT.QPR;;` |
//! | PM_SPOOLER_DD | driver | `PSCRIPT.DRV;;;` |
//!
//! A system without these entries gets them on first use: the ports
//! (LPT1-3, COM1-4, FILE, LPR1, IPP1), PMPRINT, the PostScript driver,
//! and one printer, "PostScript Printer" (queue PSPRINT on printer
//! PRINTER1, port FILE).

use alloc::vec;
use alloc::vec::Vec;

use crate::util::{cat, dec, fields, join, list, parse_dec, trim, upper, z};

unsafe extern "C" {
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.101"]
    fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, size: *mut u32) -> u32;
}

const HINI_SYSTEM: u32 = 0xffff_fffe;

pub const SPOOLER: &[u8] = b"PM_SPOOLER";
pub const QUEUE: &[u8] = b"PM_SPOOLER_QUEUE";
pub const QUEUE_DESCR: &[u8] = b"PM_SPOOLER_QUEUE_DESCR";
pub const QUEUE_DD: &[u8] = b"PM_SPOOLER_QUEUE_DD";
pub const QUEUE_DDDATA: &[u8] = b"PM_SPOOLER_QUEUE_DDDATA";
pub const QUEUE_STATUS: &[u8] = b"PM_SPOOLER_QUEUE_STATUS";
pub const QUEUE_TYPE: &[u8] = b"PM_SPOOLER_QUEUE_TYPE";
pub const PRINTER: &[u8] = b"PM_SPOOLER_PRINTER";
pub const PRINTER_DESCR: &[u8] = b"PM_SPOOLER_PRINTER_DESCR";
pub const PORT: &[u8] = b"PM_SPOOLER_PORT";
pub const QP: &[u8] = b"PM_SPOOLER_QP";
pub const DD: &[u8] = b"PM_SPOOLER_DD";

/// Raw profile data (None when missing).
pub fn get(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let (a, k) = (z(app), z(key));
    let mut size = 0;
    if unsafe { PrfQueryProfileSize(HINI_SYSTEM, a.as_ptr(), k.as_ptr(), &mut size) } == 0 {
        return None;
    }
    let mut v = vec![0u8; size as usize];
    let mut n = size;
    if size > 0 && unsafe { PrfQueryProfileData(HINI_SYSTEM, a.as_ptr(), k.as_ptr(), v.as_mut_ptr(), &mut n) } == 0 {
        return None;
    }
    v.truncate(n as usize);
    Some(v)
}

/// A string value without its NUL.
pub fn get_str(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let mut v = get(app, key)?;
    while v.last() == Some(&0) {
        v.pop();
    }
    Some(v)
}

pub fn set_str(app: &[u8], key: &[u8], v: Option<&[u8]>) {
    let (a, k) = (z(app), z(key));
    match v {
        Some(v) => {
            let d = z(v);
            unsafe { PrfWriteProfileString(HINI_SYSTEM, a.as_ptr(), k.as_ptr(), d.as_ptr()) };
        }
        None => unsafe {
            PrfWriteProfileString(HINI_SYSTEM, a.as_ptr(), k.as_ptr(), core::ptr::null());
        },
    }
}

pub fn set_data(app: &[u8], key: &[u8], d: &[u8]) {
    let (a, k) = (z(app), z(key));
    unsafe { PrfWriteProfileData(HINI_SYSTEM, a.as_ptr(), k.as_ptr(), d.as_ptr(), d.len() as u32) };
}

/// The keys of an application.
pub fn keys(app: &[u8]) -> Vec<Vec<u8>> {
    let a = z(app);
    let mut buf = vec![0u8; 8192];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEM, a.as_ptr(), core::ptr::null(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    buf[..n.min(buf.len())].split(|&c| c == 0).filter(|k| !k.is_empty()).map(|k| k.to_vec()).collect()
}

/// The first field of a value (`DIR` = `C:\SPOOL;` -> `C:\SPOOL`).
pub fn first(app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let v = get_str(app, key)?;
    Some(fields(&v).first().map(|f| f.to_vec()).unwrap_or_default())
}

// ---- Model ----

#[derive(Clone, Default)]
pub struct Queue {
    pub name: Vec<u8>,
    pub descr: Vec<u8>,
    pub qproc: Vec<u8>,
    pub sepfile: Vec<u8>,
    pub params: Vec<u8>,
    /// DRIVER.DEVICE names; the first is the default.
    pub drivers: Vec<Vec<u8>>,
    pub drivdata: Vec<u8>,
    pub priority: u16,
    pub start: u16,
    pub until: u16,
    pub held: bool,
    /// PRQ3_TYPE_* bits.
    pub flags: u16,
}

#[derive(Clone, Default)]
pub struct Printer {
    pub name: Vec<u8>,
    pub descr: Vec<u8>,
    pub port: Vec<u8>,
    pub drivers: Vec<Vec<u8>>,
    pub queues: Vec<Vec<u8>>,
    pub held: bool,
    pub timeout: u16,
}

pub const PRQ3_TYPE_RAW: u16 = 1;
pub const PRQ3_TYPE_BYPASS: u16 = 2;
pub const PRQ3_TYPE_APPDEFAULT: u16 = 4;

impl Queue {
    pub fn load(name: &[u8]) -> Option<Queue> {
        let name = upper(name);
        let v = get_str(QUEUE, &name)?;
        let f = fields(&v);
        let g = |i: usize| f.get(i).map(|x| x.to_vec()).unwrap_or_default();
        let descr = get_str(QUEUE_DESCR, &name).map(|d| fields(&d).first().map(|x| x.to_vec()).unwrap_or_default()).unwrap_or_default();
        let drivers = get_str(QUEUE_DD, &name).map(|d| list(fields(&d).first().copied().unwrap_or(b""))).unwrap_or_default();
        let mut drivdata = get(QUEUE_DDDATA, &name).unwrap_or_default();
        // The DRIVDATA's own length (IBM writes it with ";\0" after it).
        if drivdata.len() >= 4 {
            let cb = u32::from_le_bytes([drivdata[0], drivdata[1], drivdata[2], drivdata[3]]) as usize;
            if cb >= 40 && cb <= drivdata.len() {
                drivdata.truncate(cb);
            } else {
                drivdata.clear();
            }
        } else {
            drivdata.clear();
        }
        let st = get_str(QUEUE_STATUS, &name).unwrap_or_default();
        let nums: Vec<u32> = st.split(|&c| c == b',').filter_map(parse_dec).collect();
        let ty = get_str(QUEUE_TYPE, &name).unwrap_or_default();
        let mut flags = 0;
        for t in fields(&ty) {
            match trim(t) {
                b"APPLDEFAULT" => flags |= PRQ3_TYPE_APPDEFAULT,
                b"RAW" => flags |= PRQ3_TYPE_RAW,
                b"BYPASS" => flags |= PRQ3_TYPE_BYPASS,
                _ => {}
            }
        }
        if default_queue().is_some_and(|d| d == name) {
            flags |= PRQ3_TYPE_APPDEFAULT;
        }
        Some(Queue {
            qproc: if g(0).is_empty() { b"PMPRINT".to_vec() } else { g(0) },
            sepfile: g(1),
            params: g(2),
            name,
            descr,
            drivers,
            drivdata,
            priority: nums.first().map_or(5, |&p| p as u16),
            start: nums.get(1).map_or(0, |&p| p as u16),
            until: nums.get(2).map_or(0, |&p| p as u16),
            held: nums.get(3).is_some_and(|&h| h != 0),
            flags,
        })
    }

    pub fn save(&self) {
        let n = &self.name;
        set_str(QUEUE, n, Some(&cat(&[&self.qproc, b";", &self.sepfile, b";", &self.params, b";"])));
        set_str(QUEUE_DESCR, n, Some(&cat(&[&self.descr, b";"])));
        set_str(QUEUE_DD, n, Some(&cat(&[&join(&self.drivers, b','), b";"])));
        let mut dd = self.drivdata.clone();
        dd.extend_from_slice(b";\0");
        set_data(QUEUE_DDDATA, n, &dd);
        let st = cat(&[&dec(self.priority as u32, 1), b",", &dec(self.start as u32, 1), b",", &dec(self.until as u32, 1), b",", &dec(self.held as u32, 1), b","]);
        set_str(QUEUE_STATUS, n, Some(&st));
        let mut ty = Vec::new();
        if self.flags & PRQ3_TYPE_RAW != 0 {
            ty.extend_from_slice(b"RAW;");
        }
        if self.flags & PRQ3_TYPE_BYPASS != 0 {
            ty.extend_from_slice(b"BYPASS;");
        }
        set_str(QUEUE_TYPE, n, (!ty.is_empty()).then_some(&ty[..]));
        if self.flags & PRQ3_TYPE_APPDEFAULT != 0 {
            set_str(SPOOLER, b"QUEUE", Some(&cat(&[n, b";"])));
        }
    }

    pub fn remove(name: &[u8]) {
        for app in [QUEUE, QUEUE_DESCR, QUEUE_DD, QUEUE_DDDATA, QUEUE_STATUS, QUEUE_TYPE] {
            set_str(app, name, None);
        }
    }

    /// The printers that print this queue's jobs.
    pub fn printers(&self) -> Vec<Printer> {
        printers().into_iter().filter(|p| p.queues.iter().any(|q| q.eq_ignore_ascii_case(&self.name))).collect()
    }
}

impl Printer {
    pub fn load(name: &[u8]) -> Option<Printer> {
        let name = upper(name);
        let v = get_str(PRINTER, &name)?;
        let f = fields(&v);
        let g = |i: usize| f.get(i).copied().unwrap_or(b"");
        let descr = get_str(PRINTER_DESCR, &name).map(|d| fields(&d).first().map(|x| x.to_vec()).unwrap_or_default()).unwrap_or_default();
        Some(Printer {
            port: g(0).to_vec(),
            drivers: list(g(1)),
            queues: list(g(2)).into_iter().map(|q| upper(&q)).collect(),
            held: parse_dec(g(3)).is_some_and(|h| h != 0),
            timeout: parse_dec(g(4)).unwrap_or(45) as u16,
            name,
            descr,
        })
    }

    pub fn save(&self) {
        let v = cat(&[&self.port, b";", &join(&self.drivers, b','), b";", &join(&self.queues, b','), b";", if self.held { b"1" } else { b"" }, b";", &dec(self.timeout as u32, 1), b";"]);
        set_str(PRINTER, &self.name, Some(&v));
        set_str(PRINTER_DESCR, &self.name, Some(&cat(&[&self.descr, b";"])));
    }

    pub fn remove(name: &[u8]) {
        set_str(PRINTER, name, None);
        set_str(PRINTER_DESCR, name, None);
    }
}

pub fn queues() -> Vec<Queue> {
    keys(QUEUE).iter().filter_map(|k| Queue::load(k)).collect()
}

pub fn printers() -> Vec<Printer> {
    keys(PRINTER).iter().filter_map(|k| Printer::load(k)).collect()
}

pub fn default_queue() -> Option<Vec<u8>> {
    first(SPOOLER, b"QUEUE").map(|q| upper(&q)).filter(|q| !q.is_empty())
}

pub fn spool_dir() -> Vec<u8> {
    match first(SPOOLER, b"DIR") {
        Some(d) if !d.is_empty() => d,
        _ => cat(&[&[crate::util::boot_drive()], b":\\SPOOL"]),
    }
}

/// The port's settings (the INI value without its NUL), None if there
/// is no such port.
pub fn port_settings(port: &[u8]) -> Option<Vec<u8>> {
    get_str(PORT, &upper(port))
}

pub fn ports() -> Vec<Vec<u8>> {
    keys(PORT)
}

/// The queue processor's DLL path.
pub fn qproc_path(name: &[u8]) -> Option<Vec<u8>> {
    let v = get_str(QP, &upper(name))?;
    fields(&v).first().map(|f| f.to_vec()).filter(|p| !p.is_empty())
}

pub fn qprocs() -> Vec<Vec<u8>> {
    keys(QP)
}

pub fn drivers() -> Vec<Vec<u8>> {
    keys(DD)
}

/// The default job properties of rs2's PostScript driver for a device:
/// DRIVDATA { cb, lVersion, szDeviceName[32], abGeneralData } with rs2's
/// own general data (see PMWIN's psdev: magic "RSPS", form, orientation,
/// copies).
pub fn default_drivdata(device: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&56u32.to_le_bytes());
    d.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    let mut name = [0u8; 32];
    let k = device.len().min(31);
    name[..k].copy_from_slice(&device[..k]);
    d.extend_from_slice(&name);
    d.extend_from_slice(b"RSPS");
    d.extend_from_slice(&0u32.to_le_bytes()); // form: Letter
    d.extend_from_slice(&0u32.to_le_bytes()); // portrait
    d.extend_from_slice(&1u32.to_le_bytes()); // copies
    d
}

pub const PS_DEVICE: &[u8] = b"Generic PostScript Printer";

/// Write the default configuration if the spooler has never been set up.
pub fn ensure_defaults() {
    if get_str(SPOOLER, b"DIR").is_some() && get_str(PORT, b"FILE").is_some() {
        return;
    }
    let dir = cat(&[&[crate::util::boot_drive()], b":\\SPOOL"]);
    if get_str(SPOOLER, b"DIR").is_none() {
        set_str(SPOOLER, b"DIR", Some(&cat(&[&dir, b";"])));
        set_str(SPOOLER, b"SPOOL", Some(b"1;"));
    }
    let boot = [crate::util::boot_drive()];
    set_str(QP, b"PMPRINT", Some(&cat(&[&boot, b":\\OS2\\DLL\\PMPRINT.QPR;;"])));
    set_str(DD, b"PSCRIPT", Some(b"PSCRIPT.DRV;;;"));
    for p in [&b"LPT1"[..], b"LPT2", b"LPT3", b"FILE", b"LPR1", b"IPP1"] {
        if get_str(PORT, p).is_none() {
            set_str(PORT, p, Some(b";"));
        }
    }
    for p in [&b"COM1"[..], b"COM2", b"COM3", b"COM4"] {
        if get_str(PORT, p).is_none() {
            set_str(PORT, p, Some(b"9600;0;8;1;1;"));
        }
    }
    if keys(QUEUE).is_empty() && keys(PRINTER).is_empty() {
        let drv = cat(&[b"PSCRIPT.", PS_DEVICE]);
        let q = Queue {
            name: b"PSPRINT".to_vec(),
            descr: b"PostScript Printer".to_vec(),
            qproc: b"PMPRINT".to_vec(),
            drivers: vec![drv.clone()],
            drivdata: default_drivdata(PS_DEVICE),
            priority: 5,
            flags: PRQ3_TYPE_APPDEFAULT,
            ..Queue::default()
        };
        q.save();
        let p = Printer { name: b"PRINTER1".to_vec(), descr: b"PostScript Printer".to_vec(), port: b"FILE".to_vec(), drivers: vec![drv], queues: vec![b"PSPRINT".to_vec()], held: false, timeout: 45 };
        p.save();
        set_str(SPOOLER, b"PRINTER", Some(b"PRINTER1;"));
    }
}
