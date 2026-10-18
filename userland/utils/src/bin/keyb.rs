//! KEYB: show or change the keyboard layout. KEYB [layout [codepage]]
//!
//! The layout is switched with DosDevIOCtl on KBD$, category 4, rs2
//! function 0x5F (parameters: char id[4], USHORT code page); 0x7C
//! queries it. Layouts: US UK GR FR BE NL IT SP LA PO SG SF SV SU DK NO
//! IS CF TR PL.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use os2::*;
use utils::*;
os2::entry!(main);

const LAYOUTS: &[(&str, &str)] = &[
    ("US", "United States"),
    ("UK", "United Kingdom"),
    ("GR", "Germany"),
    ("FR", "France"),
    ("BE", "Belgium"),
    ("NL", "Netherlands"),
    ("IT", "Italy"),
    ("SP", "Spain"),
    ("LA", "Latin America"),
    ("PO", "Portugal"),
    ("SG", "Swiss German"),
    ("SF", "Swiss French"),
    ("SV", "Sweden"),
    ("SU", "Finland"),
    ("DK", "Denmark"),
    ("NO", "Norway"),
    ("IS", "Iceland"),
    ("CF", "Canadian French"),
    ("TR", "Turkey"),
    ("PL", "Poland"),
];

fn kbd_handle() -> (u32, bool) {
    let mut hf = 0;
    let mut action = 0;
    let rc = unsafe { DosOpen(b"KBD$\0".as_ptr(), &mut hf, &mut action, 0, 0, 1, 0x40, core::ptr::null_mut()) };
    if rc == 0 { (hf, true) } else { (0, false) }
}

fn main(s: &Startup) -> u32 {
    let a = args(s);
    let (hf, opened) = kbd_handle();
    let rc = if a.is_empty() {
        query(hf)
    } else if a[0] == "/?" || a[0].eq_ignore_ascii_case("/L") {
        outln("KEYB [layout [codepage]]   Layouts:");
        for (id, name) in LAYOUTS {
            outln(&alloc::format!("  {id}  {name}"));
        }
        0
    } else {
        set(hf, &a[0], a.get(1).map(String::as_str))
    };
    if opened {
        unsafe { DosClose(hf) };
    }
    rc
}

fn query(hf: u32) -> u32 {
    let mut data = [0u8; 6];
    let (mut pl, mut dl) = (0, 6);
    let rc = unsafe { DosDevIOCtl(hf, 4, 0x7c, core::ptr::null_mut(), 0, &mut pl, data.as_mut_ptr(), 6, &mut dl) };
    if rc != 0 {
        error(rc);
        return rc;
    }
    let id: String = data[..4].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    let cp = u16::from_le_bytes([data[4], data[5]]);
    let name = LAYOUTS.iter().find(|l| l.0 == id).map_or("", |l| l.1);
    outln(&alloc::format!("Current keyboard layout: {id} ({name}), code page {cp}"));
    0
}

fn set(hf: u32, id: &str, cp: Option<&str>) -> u32 {
    let id = id.to_ascii_uppercase();
    if id.len() > 4 || !LAYOUTS.iter().any(|l| l.0 == id) {
        eprint(&alloc::format!("KEYB: unknown keyboard layout {id}; KEYB /L lists them.\r\n"));
        return 87;
    }
    let cp: u16 = match cp.map(|c| c.parse()) {
        None => 0,
        Some(Ok(v)) => v,
        Some(Err(_)) => {
            eprint("KEYB: the code page must be a number.\r\n");
            return 87;
        }
    };
    let mut params = [0u8; 6];
    params[..id.len()].copy_from_slice(id.as_bytes());
    params[4..].copy_from_slice(&cp.to_le_bytes());
    let (mut pl, mut dl) = (6, 0);
    let rc = unsafe { DosDevIOCtl(hf, 4, 0x5f, params.as_mut_ptr(), 6, &mut pl, core::ptr::null_mut(), 0, &mut dl) };
    if rc != 0 {
        error(rc);
        return rc;
    }
    query(hf)
}
