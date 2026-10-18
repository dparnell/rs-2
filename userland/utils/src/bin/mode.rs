//! MODE: display and set device modes.
//!
//! ```text
//! MODE [CON]                  show the screen mode
//! MODE display[,rows]         display = 40, 80, 132, BW40, BW80, CO40, CO80, MONO
//! MODE CON RATE=r DELAY=d     keyboard typematic rate (accepted)
//! ```
//! The screen mode goes through VioGetMode/VioSetMode (16-bit, thunked).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::vio;
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
    let dev = first.trim_end_matches(':');
    if dev == "CON" {
        if words.len() == 1 {
            return status();
        }
        return con(&words[1..]);
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
    for p in params.iter().flat_map(|p| p.split(',')).filter(|p| !p.is_empty()) {
        let Some((k, v)) = p.split_once('=') else { return bad(p) };
        let Ok(n) = v.parse::<u16>() else { return bad(p) };
        match k {
            // Typematic rate 1-32 characters per second, delay 1-4 quarter
            // seconds: accepted; the keyboard driver keeps its own rate.
            "RATE" if (1..=32).contains(&n) => {}
            "DELAY" if (1..=4).contains(&n) => {}
            "COLS" => cols = Some(n),
            "LINES" | "ROWS" => rows = Some(n),
            _ => return bad(p),
        }
    }
    if cols.is_some() || rows.is_some() {
        return set(cols, rows, None);
    }
    0
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
