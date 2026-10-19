extern crate std;

use super::*;
use alloc::vec;
use std::path::PathBuf;
use std::println;

fn sample() -> Profile {
    let mut p = Profile::default();
    p.set(b"PM_Workplace:Location", b"<WP_DESKTOP>", b"\x2a\x00\x00\x00").unwrap();
    p.set(b"PM_Workplace:Location", b"<WP_OS2SYS>", b"\x2b\x00\x00\x00").unwrap();
    p.set(b"MYAPP", b"Text", b"hello\0").unwrap();
    p.set(b"MYAPP", b"Empty", b"").unwrap();
    p.set(b"myapp", b"Text", b"other case\0").unwrap();
    p
}

/// IBM INI files from the extracted media (work/ in the repository, or
/// RS2_WORK); None when they are not there.
fn media(rel: &str) -> Option<Vec<u8>> {
    let mut dirs = vec![PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../work"))];
    if let Ok(w) = std::env::var("RS2_WORK") {
        dirs.insert(0, PathBuf::from(w));
    }
    let r = dirs.iter().find_map(|d| std::fs::read(d.join(rel)).ok());
    if r.is_none() {
        println!("skipping {rel}: not found");
    }
    r
}

#[test]
fn round_trip() {
    let p = sample();
    let b = p.to_bytes();
    assert_eq!(&b[..4], &[0xff; 4]);
    assert_eq!(u32_at(&b, 4), Some(0x14));
    assert_eq!(u32_at(&b, 8), Some(b.len()));
    let q = parse(&b);
    assert!(!q.damaged && !q.foreign);
    assert_eq!(q.profile, p);
    assert_eq!(q.profile.to_bytes(), b);
}

#[test]
fn case_sensitive_names() {
    let p = sample();
    assert_eq!(p.get(b"MYAPP", b"Text"), Some(&b"hello\0"[..]));
    assert_eq!(p.get(b"myapp", b"Text"), Some(&b"other case\0"[..]));
    assert_eq!(p.get(b"MyApp", b"Text"), None);
    assert_eq!(p.get(b"MYAPP", b"TEXT"), None);
    assert_eq!(p.get(b"MYAPP", b"Empty"), Some(&b""[..]));
}

#[test]
fn order_and_updates() {
    let mut p = sample();
    p.set(b"MYAPP", b"Text", b"changed").unwrap();
    p.set(b"MYAPP", b"New", b"n").unwrap();
    p.set(b"Last", b"k", b"v").unwrap();
    let apps: Vec<&[u8]> = p.apps.iter().map(|a| a.name.as_slice()).collect();
    assert_eq!(apps, [&b"PM_Workplace:Location"[..], b"MYAPP", b"myapp", b"Last"]);
    let keys: Vec<&[u8]> = p.app(b"MYAPP").unwrap().keys.iter().map(|k| k.name.as_slice()).collect();
    assert_eq!(keys, [&b"Text"[..], b"Empty", b"New"]);
    assert!(p.remove_key(b"MYAPP", b"Empty"));
    assert!(!p.remove_key(b"MYAPP", b"Empty"));
    assert!(p.remove_key(b"Last", b"k"));
    assert!(p.app(b"Last").is_none(), "the last key takes the application with it");
    assert!(p.remove_app(b"myapp"));
    assert!(!p.remove_app(b"myapp"));
    assert_eq!(parse(&p.to_bytes()).profile, p);
    assert_eq!(p.set(b"", b"k", b"v"), Err(Error::BadName));
    assert_eq!(p.set(b"a", b"k\0x", b"v"), Err(Error::BadName));
    assert_eq!(p.set(b"a", b"k", &vec![0; MAX_DATA + 1]), Err(Error::TooLarge));
    p.set(b"a", b"k", &vec![7; MAX_DATA]).unwrap();
    assert_eq!(parse(&p.to_bytes()).profile.get(b"a", b"k").unwrap().len(), MAX_DATA);
}

#[test]
fn empty_and_foreign() {
    let e = parse(&[]);
    assert!(e.profile.apps.is_empty() && !e.foreign && !e.damaged);
    let b = Profile::default().to_bytes();
    assert_eq!(b, [0xff, 0xff, 0xff, 0xff, 0x14, 0, 0, 0, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let e = parse(&b);
    assert!(e.profile.apps.is_empty() && !e.foreign && !e.damaged);
    let mut z = b.clone();
    z[4] = 0;
    assert!(parse(&z).profile.apps.is_empty() && !parse(&z).damaged);
    assert!(parse(b"[MYAPP]\r\nkey=value\r\n").foreign);
    assert!(parse(&[0xff; 10]).foreign);
}

#[test]
fn corrupt_input() {
    let good = sample().to_bytes();
    // Every truncation: no panic, only whole records, flagged when cut.
    for n in 0..good.len() {
        let p = parse(&good[..n]);
        for a in &p.profile.apps {
            assert!(!a.name.is_empty());
        }
        if n > HEADER {
            assert!(p.damaged, "cut at {n}");
        }
    }
    // Every single byte changed to a few values: no panic.
    for i in 0..good.len() {
        for v in [0u8, 1, 0x13, 0x14, 0x7f, 0xff] {
            let mut b = good.clone();
            b[i] = v;
            let _ = parse(&b);
        }
    }
    // An application chain that loops back to itself.
    let mut b = good.clone();
    b[0x14..0x18].copy_from_slice(&0x14u32.to_le_bytes());
    let p = parse(&b);
    assert!(p.damaged);
    assert!(p.profile.apps.len() <= b.len() / APP_REC);
    // A key chain that loops.
    let mut b = good.clone();
    let first_key = u32_at(&b, 0x18).unwrap();
    b[first_key..first_key + 4].copy_from_slice(&(first_key as u32).to_le_bytes());
    let p = parse(&b);
    assert!(p.damaged);
    // A name offset beyond the end drops that key only.
    let mut b = good.clone();
    b[first_key + 12..first_key + 16].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    let p = parse(&b);
    assert!(p.damaged);
    assert_eq!(p.profile.apps.len(), sample().apps.len());
}

fn dump(name: &str, p: &Parsed) {
    let keys: usize = p.profile.apps.iter().map(|a| a.keys.len()).sum();
    println!("{name}: {} applications, {keys} keys, damaged {}", p.profile.apps.len(), p.damaged);
}

/// IBM files: they parse cleanly, and those IBM wrote in its canonical
/// order come out byte for byte the same from `to_bytes`.
#[test]
fn ibm_files() {
    for (rel, canonical) in [
        ("media/client/cid/server/ffst/epw/epw.ini", true),
        ("unpacked/disk_21/MMTEMP/MMPMCONN.INI", true),
        ("media/client/os2image/FI/PRTUTILS/MARKVIS/MODULES.INI", true),
        ("media/client/cid/server/ffst/epw/vpd.ini", false),
    ] {
        let Some(b) = media(rel) else { continue };
        let p = parse(&b);
        dump(rel, &p);
        assert!(!p.foreign && !p.damaged, "{rel}");
        assert!(!p.profile.apps.is_empty());
        let out = p.profile.to_bytes();
        if canonical {
            assert_eq!(out, b, "{rel} re-serialises byte for byte");
        } else {
            assert_eq!(parse(&out).profile, p.profile);
        }
    }
}

#[test]
fn ibm_epw_contents() {
    let Some(b) = media("media/client/cid/server/ffst/epw/epw.ini") else { return };
    let p = parse(&b).profile;
    assert_eq!(p.apps[0].name, b"CFG");
    assert_eq!(p.get(b"CFG", b"MSG"), Some(&b"ON\0"[..]));
    assert_eq!(p.get(b"cfg", b"MSG"), None);
}

/// vpd.ini: non-zero reserved header words and stale records after the
/// live data, which the reader ignores.
#[test]
fn ibm_vpd_stale_records() {
    let Some(b) = media("media/client/cid/server/ffst/epw/vpd.ini") else { return };
    assert_ne!(u32_at(&b, 12), Some(0));
    let p = parse(&b).profile;
    assert_eq!(p.apps[0].name, b"HVPD");
    assert!(p.profile_size() < b.len());
}

impl Profile {
    fn profile_size(&self) -> usize {
        self.to_bytes().len()
    }
}

#[test]
fn names() {
    assert_eq!(name_list([&b"A"[..], b"BC"]), b"A\0BC\0\0");
    assert_eq!(name_list(core::iter::empty()), b"\0");
}

#[test]
fn program_list_entries() {
    use proglist::*;
    let mut e = Entry { group: 3, progc: 2, visible: 0, ..Entry::default() };
    e.swp = [0x8f, 300, 400, 10, 20, 3, 0, 0, 0];
    e.fields[TITLE] = Some(b"Command prompt".to_vec());
    e.fields[EXECUTABLE] = Some(b"C:\\OS2\\CMD.EXE".to_vec());
    e.fields[2] = Some(b"".to_vec());
    e.fields[ENVIRONMENT] = Some(b"A=1\0B=2\0".to_vec());
    let b = e.to_bytes();
    assert_eq!(Entry::parse(&b), Some(e.clone()));
    assert!(!e.is_group());
    assert_eq!(Entry::parse(&b[..b.len() - 1]), None);
    assert_eq!(&key(0x1c), b"0000001C");
    assert_eq!(handle(b"0000001C"), Some(0x1c));
    assert_eq!(handle(NEXT), None);
    assert_eq!(handle(b"0000001G"), None);
}
