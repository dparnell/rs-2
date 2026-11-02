//! Host tests. The fixture is a set IBM's BACKUP (OS/2 Warp 4.52) wrote,
//! run in rs2: `BACKUP C:\T\*.* E: /S` of \T\A.TXT (with a .SUBJECT EA),
//! \T\B.TXT and \T\SUB\S.TXT (the directory with a .COMMENTS EA), then
//! `BACKUP C:\T\NEW.TXT E: /A`.

use super::*;
use alloc::vec;

fn hex(s: &str) -> Vec<u8> {
    let d: Vec<u8> = s.bytes().filter(|b| b.is_ascii_hexdigit()).collect();
    d.chunks(2).map(|c| u8::from_str_radix(core::str::from_utf8(c).unwrap(), 16).unwrap()).collect()
}

/// CONTROL.001 after the records: the 139-byte header is "\x8BBKUPJFS ",
/// sequence 1, zeros and 0xFF last.
const IBM_RECORDS: &str = "
2100 0200 2201 0000 0000 0000 d400 0000 0000 0000 0000 0000 0000 0000 0001 0054 00
3b00 0311 0000 0000 0000 0001 001a 0000 0000 0000 0011 0000 0000 0000 0020 0063 1e62
5d00 0000 0000 0000 001a 0000 0000 0000 0005 0041 2e54 5854 00
3b00 030d 0000 0000 0000 0001 002b 0000 0000 0000 000d 0000 0000 0000 0020 0063 1e62
5d00 0000 0000 0000 0000 0000 0000 0000 0005 0042 2e54 5854 00
2500 0100 8201 0000 0000 0000 d438 0000 0000 0000 0019 0000 0000 0000 0005 0054 5c53
5542 00
3b00 030a 0000 0000 0000 0001 0051 0000 0000 0000 000a 0000 0000 0000 0020 0063 1e62
5d00 0000 0000 0000 0000 0000 0000 0000 0005 0053 2e54 5854 00
2100 0100 ffff ffff ffff ffff d400 0000 0000 0000 0000 0000 0000 0000 0001 0054 00
3d00 030a 0000 0000 0000 0001 005b 0000 0000 0000 000a 0000 0000 0000 0020 0063 1e62
5d00 0000 0000 0000 0000 0000 0000 0000 0007 004e 4557 2e54 5854 00";

/// The first 0x38 bytes of BACKUP.001: A.TXT's EAs, its data, B.TXT's.
const IBM_DATA: &str = "
1a00 0000 0008 0900 2e53 5542 4a45 4354 00fd ff05 0068 656c 6c6f
6865 6c6c 6f20 776f 726c 6420 6f6e 650d 0a
7365 636f 6e64 2066 696c 650d 0a";

fn ibm_control() -> Vec<u8> {
    let mut v = vec![0u8; HEADER_LEN];
    v[0] = 0x8B;
    v[1..9].copy_from_slice(b"BKUPJFS ");
    v[9] = 1;
    v[0x8A] = 0xFF;
    v.extend(hex(IBM_RECORDS));
    v
}

#[test]
fn parses_ibm_control_file() {
    let c = Control::parse(&ibm_control()).unwrap();
    assert_eq!(c.header, Header { sequence: 1, last: true });
    let names: Vec<(&str, Vec<&str>)> = c.dirs.iter().map(|(d, f)| (d.path.as_str(), f.iter().map(|f| f.name.as_str()).collect())).collect();
    assert_eq!(names, vec![("T", vec!["A.TXT", "B.TXT"]), ("T\\SUB", vec!["S.TXT"]), ("T", vec!["NEW.TXT"])]);
    let a = &c.dirs[0].1[0];
    assert_eq!((a.size, a.offset, a.part_len, a.ea_offset, a.ea_len), (17, 0x1a, 17, 0, 0x1a));
    assert_eq!((a.flags, a.part, a.attr, a.time, a.date), (3, 1, 0x20, 0x1e63, 0x5d62));
    let sub = &c.dirs[1].0;
    assert_eq!((sub.ea_offset, sub.ea_len, sub.next), (0x38, 0x19, Some(0x182)));
    assert_eq!(c.dirs[2].0.next, None);
}

#[test]
fn writes_what_ibm_wrote() {
    let d = ibm_control();
    let c = Control::parse(&d).unwrap();
    assert_eq!(c.write(), d);
}

#[test]
fn ea_lists() {
    let data = hex(IBM_DATA);
    let eas = parse_fealist(&data[..0x1a]).unwrap();
    assert_eq!(eas, vec![Ea { flags: 0, name: b".SUBJECT".to_vec(), value: b"\xfd\xff\x05\x00hello".to_vec() }]);
    assert_eq!(fealist(&eas), data[..0x1a].to_vec());
    assert_eq!(&data[0x1a..0x1a + 17], b"hello world one\r\n");
    // FEA2LIST round trip, entries aligned to 4 bytes.
    let two = vec![eas[0].clone(), Ea { flags: 0x80, name: b".TYPE".to_vec(), value: b"\xfd\xff\x04\x00Text".to_vec() }];
    let l = fea2list(&two);
    assert_eq!(l.len() % 4, 0);
    assert_eq!(parse_fea2list(&l), two);
    assert!(fealist(&[]).is_empty());
}

#[test]
fn restore_matching() {
    assert!(matches("T", "A.TXT", "T", "*.*", false));
    assert!(!matches("T\\SUB", "S.TXT", "T", "*.*", false));
    assert!(matches("T\\SUB", "S.TXT", "T", "*.*", true));
    assert!(!matches("TX", "S.TXT", "T", "*.*", true));
    assert!(matches("T\\SUB", "S.TXT", "", "S.*", true));
    assert!(wild("*.TXT", "a.txt"));
    assert!(!wild("*.TXT", "a.exe"));
    assert!(wild("A?.*", "A"));
}
