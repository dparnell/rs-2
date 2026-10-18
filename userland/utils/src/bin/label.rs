//! LABEL: show or change a volume label. LABEL [drive:][label]
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use utils::*;
os2::entry!(main);

/// Characters a FAT volume label cannot contain.
const BAD: &str = "*?/\\|.,;:+=[]<>\"";

fn main(s: &os2::Startup) -> u32 {
    let raw: String = s.args().iter().map(|&b| b as char).collect();
    let raw = raw.trim();
    // "d:" may be followed by the label with or without a space.
    let (drive, label) = match raw.as_bytes() {
        [d, b':', ..] if d.is_ascii_alphabetic() => (drive_arg(Some(&raw[..2])), raw[2..].trim()),
        _ => (drive_arg(None), raw),
    };
    let Some(drive) = drive else {
        error(15);
        return 15;
    };
    let mut label = String::from(label);
    if label.is_empty() {
        let (cur, ser) = match volume_label(drive) {
            Ok(v) => v,
            Err(rc) => {
                error(rc);
                return rc;
            }
        };
        let letter = (b'A' + drive) as char;
        if cur.is_empty() {
            outln("The disk has no volume label.");
        } else {
            outln(&alloc::format!("\r\nThe volume label in drive {letter} is {cur}."));
        }
        outln(&alloc::format!("The Volume Serial Number is {}.", serial(ser)));
        label = ask("Enter a volume label of up to 11 characters\r\nor press Enter for no volume label update.  ");
        if label.is_empty() {
            return 0;
        }
    }
    if label.chars().any(|c| BAD.contains(c) || (c as u32) < 0x20) {
        eprint("SYS0123: A volume label contains a character that is not allowed.\r\n");
        return 123;
    }
    if label.len() > 11 {
        outln("The volume label is longer than 11 characters; only the first 11\r\ncharacters were written to the disk.");
        label.truncate(11);
    }
    let rc = set_volume_label(drive, &label.to_ascii_uppercase());
    if rc != 0 {
        error(rc);
    }
    rc
}
