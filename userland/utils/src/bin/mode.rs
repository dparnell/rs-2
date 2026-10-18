//! MODE: display and set device modes.
//!
//! ```text
//! MODE [CON]                  show the screen mode
//! MODE display[,rows]         display = 40, 80, 132, BW40, BW80, CO40, CO80, MONO
//! MODE CON RATE=r DELAY=d     keyboard typematic rate
//! MODE COMn[:]                show the serial port settings
//! MODE COMn[:]baud[,parity[,databits[,stopbits]]] [TO=ON|OFF] [XON=ON|OFF]
//!      [IDSR=ON|OFF] [ODSR=ON|OFF] [OCTS=ON|OFF] [DTR=ON|OFF|HS] [RTS=ON|OFF|HS|TOG]
//! ```
//! The screen mode goes through VioGetMode/VioSetMode (16-bit, thunked),
//! the keyboard rate through KBD$ and the serial ports through the
//! category 1 (ASYNC) IOCtls.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::null_mut;
use os2::vio;
use os2::{DosClose, DosDevIOCtl, DosOpen};
use utils::*;
os2::entry!(main);

fn bad(p: &str) -> u32 {
    eprint(&alloc::format!("SYS1613: The MODE parameter {p} is incorrect.\r\n"));
    87
}

fn main(s: &os2::Startup) -> u32 {
    let raw: String = s.args().iter().map(|&b| b as char).collect();
    // Parameters are separated by blanks or commas ("CO80,25" or "CON RATE=30").
    let words: Vec<String> = raw.split([' ', '\t']).filter(|w| !w.is_empty()).map(|w| w.to_ascii_uppercase()).collect();
    let Some(first) = words.first() else { return status() };
    let dev = first.split(':').next().unwrap_or("");
    if dev == "CON" {
        if words.len() == 1 {
            return status();
        }
        return con(&words[1..]);
    }
    if let Some(n) = dev.strip_prefix("COM").and_then(|n| n.parse::<u8>().ok()) {
        // "COM1:9600,N,8,1 XON=ON": settings after the colon or a blank.
        let rest: String = raw.trim_start().splitn(2, [' ', ':', '\t']).nth(1).unwrap_or("").into();
        return com(n, &rest.to_ascii_uppercase());
    }
    if dev.starts_with("COM") || dev.starts_with("LPT") || dev == "DSKT" {
        eprint(&alloc::format!("SYS1601: The MODE parameters are incorrect.\r\n(The device {dev} is not available in this release.)\r\n"));
        return 1;
    }
    display(&words.join(","))
}

fn status() -> u32 {
    match vio::vio_get_mode() {
        Ok(m) => {
            let (cols, rows) = (m.col, m.row);
            outln("\r\nStatus for device CON:");
            outln("----------------------");
            outln(&alloc::format!("Columns={cols}"));
            outln(&alloc::format!("Rows={rows}"));
            let kind = if m.fb_type & 1 == 0 { "monochrome" } else { "color" };
            outln(&alloc::format!("Display={kind}, {} colors", 1u32 << m.color));
            0
        }
        Err(rc) => {
            error(rc as u32);
            rc as u32
        }
    }
}

/// MODE CON RATE=r DELAY=d (and COLS=c LINES=l).
fn con(params: &[String]) -> u32 {
    let (mut cols, mut rows) = (None, None);
    let (mut rate, mut delay) = (None, None);
    for p in params.iter().flat_map(|p| p.split(',')).filter(|p| !p.is_empty()) {
        let Some((k, v)) = p.split_once('=') else { return bad(p) };
        let Ok(n) = v.parse::<u16>() else { return bad(p) };
        match k {
            // Typematic rate 1-32 characters per second, delay 1-4 quarter
            // seconds: accepted; the keyboard driver keeps its own rate.
            "RATE" if (1..=32).contains(&n) => rate = Some(n),
            "DELAY" if (1..=4).contains(&n) => delay = Some(n),
            "COLS" => cols = Some(n),
            "LINES" | "ROWS" => rows = Some(n),
            _ => return bad(p),
        }
    }
    if rate.is_some() || delay.is_some() {
        // RATEDELAY { usDelay ms, usRate characters per second }.
        let mut rd = [delay.unwrap_or(2) * 250, rate.unwrap_or(30).min(30)];
        let rc = match open_dev("KBD$") {
            Ok(h) => {
                let mut cb = 4;
                let rc = unsafe { DosDevIOCtl(h, 4, 0x54, rd.as_mut_ptr() as *mut u8, 4, &mut cb, null_mut(), 0, null_mut()) };
                unsafe { DosClose(h) };
                rc
            }
            Err(rc) => rc,
        };
        if rc != 0 {
            error(rc);
            return rc;
        }
    }
    if cols.is_some() || rows.is_some() {
        return set(cols, rows, None);
    }
    0
}

fn open_dev(name: &str) -> Result<u32, u32> {
    let mut z = String::from(name);
    z.push('\0');
    let (mut h, mut act) = (0, 0);
    let rc = unsafe { DosOpen(z.as_ptr(), &mut h, &mut act, 0, 0, 1, 0x42, null_mut()) };
    if rc == 0 { Ok(h) } else { Err(rc) }
}

/// Category 1 IOCtl with parameters; returns the data.
fn async_ioctl(h: u32, f: u32, params: &mut [u8], data: &mut [u8]) -> u32 {
    let (mut cbp, mut cbd) = (params.len() as u32, data.len() as u32);
    let pp = if params.is_empty() { null_mut() } else { params.as_mut_ptr() };
    let dp = if data.is_empty() { null_mut() } else { data.as_mut_ptr() };
    unsafe { DosDevIOCtl(h, 1, f, pp, params.len() as u32, &mut cbp, dp, data.len() as u32, &mut cbd) }
}

fn on_off(v: bool) -> &'static str {
    if v { "ON" } else { "OFF" }
}

/// MODE COMn[:]baud,parity,databits,stopbits [KEY=value ...]
fn com(n: u8, spec: &str) -> u32 {
    let name = alloc::format!("COM{n}");
    let h = match open_dev(&name) {
        Ok(h) => h,
        Err(rc) => {
            eprint(&alloc::format!("SYS1620: The COM{n} device is not available.\r\n"));
            return rc;
        }
    };
    let rc = com_set(h, spec).and_then(|shown| if shown { Ok(()) } else { com_status(h, n) });
    unsafe { DosClose(h) };
    match rc {
        Ok(()) => 0,
        Err(rc) => rc,
    }
}

/// Apply the settings; Ok(false) when there were none (show the status).
fn com_set(h: u32, spec: &str) -> Result<bool, u32> {
    let mut words = spec.split([' ', '\t']).filter(|w| !w.is_empty());
    let mut keys: Vec<&str> = Vec::new();
    let mut line: Option<&str> = None;
    for w in words.by_ref() {
        if w.contains('=') {
            keys.push(w);
        } else if line.is_none() {
            line = Some(w);
        } else {
            return Err(bad(w));
        }
    }
    if line.is_none() && keys.is_empty() {
        return Ok(false);
    }
    // Current line settings, changed field by field.
    let mut lc = [0u8; 4];
    let rc = async_ioctl(h, 0x62, &mut [], &mut lc);
    if rc != 0 {
        error(rc);
        return Err(rc);
    }
    let (mut data, mut parity, mut stop) = (lc[0], lc[1], lc[2]);
    let mut baud: Option<u32> = None;
    if let Some(l) = line {
        let mut f = l.split(',');
        if let Some(b) = f.next().filter(|b| !b.is_empty()) {
            // Two-digit abbreviations as on OS/2 (11 = 110, 96 = 9600, 19 = 19200).
            let v = match b {
                "11" => 110,
                "15" => 150,
                "30" => 300,
                "60" => 600,
                "12" => 1200,
                "24" => 2400,
                "48" => 4800,
                "96" => 9600,
                "19" => 19200,
                _ => b.parse::<u32>().map_err(|_| bad(b))?,
            };
            baud = Some(v);
        }
        if let Some(p) = f.next().filter(|p| !p.is_empty()) {
            parity = match p {
                "N" => 0,
                "O" => 1,
                "E" => 2,
                "M" => 3,
                "S" => 4,
                _ => return Err(bad(p)),
            };
        }
        if let Some(d) = f.next().filter(|d| !d.is_empty()) {
            data = d.parse::<u8>().ok().filter(|d| (5..=8).contains(d)).ok_or_else(|| bad(d))?;
        }
        if let Some(st) = f.next().filter(|s| !s.is_empty()) {
            stop = match st {
                "1" => 0,
                "1.5" => 1,
                "2" => 2,
                _ => return Err(bad(st)),
            };
        }
        if let Some(x) = f.next().filter(|x| *x != "P") {
            return Err(bad(x));
        }
    }
    if let Some(b) = baud {
        let rc = if b <= 0xffff {
            let mut p = (b as u16).to_le_bytes();
            async_ioctl(h, 0x41, &mut p, &mut [])
        } else {
            let mut p = [0u8; 5];
            p[..4].copy_from_slice(&b.to_le_bytes());
            async_ioctl(h, 0x43, &mut p, &mut [])
        };
        if rc != 0 {
            eprint(&alloc::format!("SYS1621: The baud rate {b} is not valid.\r\n"));
            return Err(rc);
        }
    }
    let mut p = [data, parity, stop];
    let rc = async_ioctl(h, 0x42, &mut p, &mut []);
    if rc != 0 {
        eprint("SYS1622: The combination of data bits, parity and stop bits is not valid.\r\n");
        return Err(rc);
    }
    if !keys.is_empty() {
        let mut dcb = [0u8; 11];
        let rc = async_ioctl(h, 0x73, &mut [], &mut dcb);
        if rc != 0 {
            error(rc);
            return Err(rc);
        }
        for k in keys {
            let (key, v) = k.split_once('=').unwrap_or((k, ""));
            let on = match v {
                "ON" => true,
                "OFF" => false,
                "HS" | "TOG" => true,
                _ => return Err(bad(k)),
            };
            let flag = |b: &mut u8, bit: u8, set: bool| if set { *b |= bit } else { *b &= !bit };
            match (key, v) {
                ("TO", _) => flag(&mut dcb[6], 0x01, on),
                ("XON", _) => flag(&mut dcb[5], 0x03, on),
                ("IDSR", _) => flag(&mut dcb[4], 0x40, on),
                ("ODSR", _) => flag(&mut dcb[4], 0x10, on),
                ("OCTS", _) => flag(&mut dcb[4], 0x08, on),
                ("DTR", "HS") => dcb[4] = (dcb[4] & !0x03) | 0x02,
                ("DTR", _) => dcb[4] = (dcb[4] & !0x03) | on as u8,
                ("RTS", "HS") => dcb[5] = (dcb[5] & !0xc0) | 0x80,
                ("RTS", "TOG") => dcb[5] |= 0xc0,
                ("RTS", _) => dcb[5] = (dcb[5] & !0xc0) | if on { 0x40 } else { 0 },
                _ => return Err(bad(k)),
            }
        }
        let rc = async_ioctl(h, 0x53, &mut dcb, &mut []);
        if rc != 0 {
            error(rc);
            return Err(rc);
        }
    }
    Ok(true)
}

fn com_status(h: u32, n: u8) -> Result<(), u32> {
    let mut eb = [0u8; 15];
    let mut lc = [0u8; 4];
    let mut dcb = [0u8; 11];
    for (f, d) in [(0x63, &mut eb[..]), (0x62, &mut lc[..]), (0x73, &mut dcb[..])] {
        let rc = async_ioctl(h, f, &mut [], d);
        if rc != 0 {
            error(rc);
            return Err(rc);
        }
    }
    let baud = u32::from_le_bytes([eb[0], eb[1], eb[2], eb[3]]);
    let parity = ["NONE", "ODD", "EVEN", "MARK", "SPACE"].get(lc[1] as usize).copied().unwrap_or("?");
    let stop = ["1", "1.5", "2"].get(lc[2] as usize).copied().unwrap_or("?");
    let dtr = match dcb[4] & 3 {
        0 => "OFF",
        2 => "HS",
        _ => "ON",
    };
    let rts = match dcb[5] & 0xc0 {
        0 => "OFF",
        0x80 => "HS",
        0xc0 => "TOG",
        _ => "ON",
    };
    outln(&alloc::format!("\r\nStatus for device COM{n}:"));
    outln("-------------------------");
    outln(&alloc::format!("BAUD = {baud}"));
    outln(&alloc::format!("PARITY = {parity}"));
    outln(&alloc::format!("DATABITS = {}", lc[0]));
    outln(&alloc::format!("STOPBITS = {stop}"));
    outln(&alloc::format!("TO = {}", on_off(dcb[6] & 1 != 0)));
    outln(&alloc::format!("XON = {}", on_off(dcb[5] & 3 != 0)));
    outln(&alloc::format!("IDSR = {}", on_off(dcb[4] & 0x40 != 0)));
    outln(&alloc::format!("ODSR = {}", on_off(dcb[4] & 0x10 != 0)));
    outln(&alloc::format!("OCTS = {}", on_off(dcb[4] & 0x08 != 0)));
    outln(&alloc::format!("DTR = {dtr}"));
    outln(&alloc::format!("RTS = {rts}"));
    Ok(())
}

/// MODE display[,rows]
fn display(spec: &str) -> u32 {
    let mut parts = spec.split(',').filter(|p| !p.is_empty());
    let d = parts.next().unwrap_or("");
    let (cols, mono): (Option<u16>, Option<bool>) = match d {
        "40" => (Some(40), None),
        "80" => (Some(80), None),
        "132" => (Some(132), None),
        "BW40" | "CO40" => (Some(40), Some(false)),
        "BW80" | "CO80" => (Some(80), Some(false)),
        "MONO" => (Some(80), Some(true)),
        "" => (None, None),
        _ => {
            eprint(&alloc::format!("SYS1623: The MODE parameter display {d} is incorrect.\r\n"));
            return 87;
        }
    };
    let rows = match parts.next() {
        Some(r) => match r.parse::<u16>() {
            Ok(n) if matches!(n, 25 | 43 | 50 | 60) => Some(n),
            _ => {
                eprint(&alloc::format!("SYS1622: The MODE parameter rows {r} is incorrect.\r\n"));
                return 87;
            }
        },
        None => None,
    };
    if let Some(p) = parts.next() {
        return bad(p);
    }
    set(cols, rows, mono)
}

fn set(cols: Option<u16>, rows: Option<u16>, mono: Option<bool>) -> u32 {
    let mut m = match vio::vio_get_mode() {
        Ok(m) => m,
        Err(rc) => {
            error(rc as u32);
            return rc as u32;
        }
    };
    if let Some(c) = cols {
        m.col = c;
    }
    if let Some(r) = rows {
        m.row = r;
    }
    if let Some(mono) = mono {
        m.fb_type = if mono { 0 } else { 1 };
    }
    // Type, colors, columns and rows; the system picks the rest.
    m.cb = 8;
    match vio::vio_set_mode(&m) {
        Ok(()) => 0,
        Err(rc) => {
            eprint(&alloc::format!("SYS{rc:04}: The display cannot be set to {} columns and {} rows.\r\n", { m.col }, { m.row }));
            rc as u32
        }
    }
}
