extern crate std;

use super::*;
use core::cell::RefCell;
use std::format;
use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

pub(crate) struct Mem(pub RefCell<Vec<u8>>);

impl BlockDevice for Mem {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
        let o = lba as usize * 512;
        let d = self.0.borrow();
        if o + buf.len() > d.len() {
            return Err(Error::Io);
        }
        buf.copy_from_slice(&d[o..o + buf.len()]);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> Result<()> {
        let o = lba as usize * 512;
        let mut d = self.0.borrow_mut();
        if o + buf.len() > d.len() {
            return Err(Error::Io);
        }
        d[o..o + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

const NOW: u32 = 1_760_000_000;

fn fresh_bs(mb: usize, bs: u32) -> Volume<Mem> {
    let dev = Mem(RefCell::new(std::vec![0u8; mb << 20]));
    format(&dev, &FormatParams { label: "TEST", serial: 0x1234_5678, now: NOW, block_size: bs, ..Default::default() }).unwrap();
    Volume::mount(dev).unwrap()
}

fn fresh(mb: usize) -> Volume<Mem> {
    fresh_bs(mb, 0)
}

fn remount(v: Volume<Mem>) -> Volume<Mem> {
    Volume::mount(v.unmount().unwrap()).unwrap()
}

fn ok(v: &mut Volume<Mem>) {
    let r = v.check().unwrap();
    assert!(r.ok(), "{:#?}", r.problems);
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

fn pattern(n: usize, seed: u64) -> Vec<u8> {
    let mut r = Lcg(seed);
    (0..n).map(|_| r.next() as u8).collect()
}

#[test]
fn format_and_mount() {
    let mut v = fresh(32);
    assert_eq!(v.label, "TEST");
    assert_eq!(v.serial, 0x1234_5678);
    assert!(!v.is_readonly());
    let root = v.root_entry().unwrap();
    assert!(root.is_dir());
    assert!(v.read_dir(&root).unwrap().is_empty());
    ok(&mut v);
    let i = v.info();
    assert!(i.free_blocks > i.total_blocks * 9 / 10);
}

#[test]
fn files_roundtrip() {
    let mut v = fresh(32);
    let mut e = v.create_file("hello.txt", 0, NOW).unwrap();
    v.write(&mut e, 0, b"Hello, JFS!", NOW).unwrap();
    let mut v = remount(v);
    let e = v.lookup("HELLO.TXT").unwrap();
    assert_eq!(e.name, "hello.txt");
    assert_eq!(e.size, 11);
    let mut b = [0u8; 32];
    assert_eq!(v.read(&e, 0, &mut b).unwrap(), 11);
    assert_eq!(&b[..11], b"Hello, JFS!");
    ok(&mut v);
}

/// Fill a volume with a bit of everything (used by the tests and by
/// `make_image`, whose output is checked with Linux's jfs_fsck).
fn populate(v: &mut Volume<Mem>) {
    let mut e = v.create_file("README.TXT", 0, NOW).unwrap();
    v.write(&mut e, 0, b"Hello from rs2\r\n", NOW).unwrap();
    v.create_file("EMPTY.DAT", 0, NOW).unwrap();
    v.mkdir("OS2", NOW).unwrap();
    v.mkdir("OS2\\SYSTEM", NOW).unwrap();
    let mut e = v.create_file("OS2\\CONFIG.SYS", 0, NOW).unwrap();
    v.write(&mut e, 0, b"IFS=JFS.IFS /AUTOCHECK:*\r\n", NOW).unwrap();
    v.mkdir("Long Directory Name With Spaces", NOW).unwrap();
    let mut e = v.create_file("Long Directory Name With Spaces\\MixedCase File.Txt", 0, NOW).unwrap();
    v.write(&mut e, 0, &pattern(3000, 7), NOW).unwrap();
    v.mkdir("BIGDIR", NOW).unwrap();
    for k in 0..2000 {
        v.create_file(&format!("BIGDIR\\FILE{k:04}.TXT"), 0, NOW).unwrap();
    }
    v.mkdir("DATA", NOW).unwrap();
    let mut e = v.create_file("DATA\\BIG.BIN", 0, NOW).unwrap();
    let data: Vec<u8> = (0..327680u32).flat_map(|k| format!("REC {k:011}\n").into_bytes()).collect();
    v.write(&mut e, 0, &data, NOW).unwrap();
    let e = v.lookup("README.TXT").unwrap();
    v.write_eas(&e, &[Ea { flags: 0, name: b".TYPE".to_vec(), value: b"\xfd\xff\x0a\x00Plain Text".to_vec() }]).unwrap();
    let e = v.lookup("OS2").unwrap();
    v.write_eas(&e, &[Ea { flags: 0x80, name: b".LONGNAME".to_vec(), value: pattern(600, 3) }]).unwrap();
}

#[test]
fn populated_is_consistent() {
    let mut v = fresh(64);
    populate(&mut v);
    ok(&mut v);
    let mut v = remount(v);
    ok(&mut v);
    let d = v.lookup("bigdir").unwrap();
    assert_eq!(v.read_dir(&d).unwrap().len(), 2000);
    let e = v.lookup("data\\big.bin").unwrap();
    assert_eq!(e.size, 327680 * 16);
}

/// JFS_IMAGE=path cargo test -p jfs make_image -- --ignored
#[test]
#[ignore]
fn make_image() {
    let path = std::env::var("JFS_IMAGE").unwrap();
    let dev = Mem(RefCell::new(std::vec![0u8; 64 << 20]));
    let label = std::env::var("JFS_LABEL").unwrap_or("TEST".into());
    format(&dev, &FormatParams { label: &label, serial: 0x1234_5678, now: NOW, ..Default::default() }).unwrap();
    let mut v = Volume::mount(dev).unwrap();
    populate(&mut v);
    let dev = v.unmount().unwrap();
    std::fs::write(path, dev.0.into_inner()).unwrap();
}

/// Read a volume made elsewhere, change it, write it back:
/// JFS_IN=img JFS_OUT=img2 cargo test -p jfs foreign_image -- --ignored --nocapture
#[test]
#[ignore]
fn foreign_image() {
    let data = std::fs::read(std::env::var("JFS_IN").unwrap()).unwrap();
    let mut v = Volume::mount(Mem(RefCell::new(data))).unwrap();
    std::println!("readonly {} flags {:#x} ci {}", v.is_readonly(), v.flags(), v.ci);
    let r = v.check().unwrap();
    std::println!("check: {} files {} dirs, problems {:#?}", r.files, r.dirs, r.problems);
    let root = v.root_entry().unwrap();
    for e in v.read_dir(&root).unwrap() {
        std::println!("{:30} {:#04x} {:8} ino {}", e.name, e.attr, e.size, e.ino);
    }
    let d = v.lookup("bigdir").unwrap();
    let list = v.read_dir(&d).unwrap();
    std::println!("BIGDIR: {} entries, first {} last {}", list.len(), list[0].name, list[list.len() - 1].name);
    let e = v.lookup("DATA\\big.bin").unwrap();
    let mut b = std::vec![0u8; e.size as usize];
    v.read(&e, 0, &mut b).unwrap();
    let want: Vec<u8> = (0..327680u32).flat_map(|k| format!("REC {k:011}\n").into_bytes()).collect();
    assert!(b == want, "BIG.BIN differs");
    let e = v.lookup("a\\b\\c\\d\\deep.txt").unwrap();
    std::println!("deep: {}", e.size);
    if v.is_readonly() {
        return;
    }
    // Changes: a new file, a big directory shrunk, a rename, EAs.
    v.mkdir("RS2", NOW).unwrap();
    let mut f = v.create_file("RS2\\NEW.BIN", 0, NOW).unwrap();
    v.write(&mut f, 0, &pattern(200_000, 9), NOW).unwrap();
    for k in (1..=2000).step_by(3) {
        v.delete(&format!("BIGDIR\\FILE{k:04}.TXT")).unwrap();
    }
    v.rename("README.TXT", "RS2\\README.TXT").unwrap();
    let f = v.lookup("RS2\\NEW.BIN").unwrap();
    v.write_eas(&f, &[Ea { flags: 0, name: b"RS2.TEST".to_vec(), value: b"value".to_vec() }]).unwrap();
    let r = v.check().unwrap();
    std::println!("after: problems {:#?}", r.problems);
    let dev = v.unmount().unwrap();
    std::fs::write(std::env::var("JFS_OUT").unwrap(), dev.0.into_inner()).unwrap();
}

#[test]
fn names_case_and_validity() {
    let mut v = fresh(32);
    v.create_file("Mixed Case Name.Text", 0, NOW).unwrap();
    assert_eq!(v.lookup("MIXED CASE NAME.TEXT").unwrap().name, "Mixed Case Name.Text");
    assert_eq!(v.create_file("mixed case name.text", 0, NOW).unwrap_err(), Error::Exists);
    // Code page 850 characters, upper-cased through Unicode.
    let n: String = [b'\x84', b'b', b'c'].iter().map(|&c| c as char).collect(); // "äbc"
    let up: String = [b'\x8e', b'B', b'C'].iter().map(|&c| c as char).collect(); // "ÄBC"
    v.create_file(&n, 0, NOW).unwrap();
    assert_eq!(v.lookup(&up).unwrap().name, n);
    for bad in ["a*b", "a?b", "x|y", "q\"", "<", ">"] {
        assert_eq!(v.create_file(bad, 0, NOW).unwrap_err(), Error::InvalidName);
    }
    let long: String = core::iter::repeat('n').take(254).collect();
    v.create_file(&long, 0, NOW).unwrap();
    let too: String = core::iter::repeat('n').take(255).collect();
    assert_eq!(v.create_file(&too, 0, NOW).unwrap_err(), Error::InvalidName);
    let e = v.lookup(&long).unwrap();
    assert_ne!(e.attr & attr::LONG_NAME, 0);
    assert_eq!(v.lookup("EMPTY").unwrap_err(), Error::NotFound);
    assert_eq!(v.lookup("NODIR\\X").unwrap_err(), Error::PathNotFound);
    ok(&mut v);
}

#[test]
fn directory_churn_against_model() {
    let mut v = fresh(64);
    v.mkdir("D", NOW).unwrap();
    let mut model = std::collections::BTreeSet::new();
    let mut r = Lcg(42);
    for step in 0..6000 {
        let k = r.next() % 3000;
        // Names of varying length (1 to 4 continuation slots).
        let name = format!("{}{k:05}", "x".repeat((k % 50) as usize));
        if model.contains(&k) {
            v.delete(&format!("D\\{name}")).unwrap();
            model.remove(&k);
        } else {
            v.create_file(&format!("D\\{name}"), 0, NOW).unwrap();
            model.insert(k);
        }
        if step % 1500 == 0 {
            ok(&mut v);
        }
    }
    ok(&mut v);
    let mut v = remount(v);
    let d = v.lookup("D").unwrap();
    let list = v.read_dir(&d).unwrap();
    assert_eq!(list.len(), model.len());
    for k in &model {
        let name = format!("{}{k:05}", "x".repeat((k % 50) as usize));
        assert!(v.lookup(&format!("d\\{}", name.to_uppercase())).is_ok(), "{name}");
    }
    // Empty it again: the tree collapses back into the inode.
    for k in model {
        let name = format!("{}{k:05}", "x".repeat((k % 50) as usize));
        v.delete(&format!("D\\{name}")).unwrap();
    }
    let d = v.lookup("D").unwrap();
    assert!(v.read_dir(&d).unwrap().is_empty());
    ok(&mut v);
    v.rmdir("D").unwrap();
    ok(&mut v);
}

#[test]
fn large_and_fragmented_files() {
    let mut v = fresh(64);
    let big = pattern(9 << 20, 1);
    let mut e = v.create_file("BIG", 0, NOW).unwrap();
    // Odd-sized writes at odd offsets.
    let mut off = 0;
    let mut r = Lcg(5);
    while off < big.len() {
        let n = ((r.next() % 300_000) as usize + 1).min(big.len() - off);
        v.write(&mut e, off as u64, &big[off..off + n], NOW).unwrap();
        off += n;
    }
    // Interleave two files so both get many extents (xtree pages).
    let mut a = v.create_file("A", 0, NOW).unwrap();
    let mut b = v.create_file("B", 0, NOW).unwrap();
    let pa = pattern(600 * 4096, 2);
    let pb = pattern(600 * 4096, 3);
    for k in 0..600 {
        let s = k * 4096;
        v.write(&mut a, s as u64, &pa[s..s + 4096], NOW).unwrap();
        v.write(&mut b, s as u64, &pb[s..s + 4096], NOW).unwrap();
    }
    ok(&mut v);
    let mut v = remount(v);
    for (n, p) in [("BIG", &big), ("A", &pa), ("B", &pb)] {
        let e = v.lookup(n).unwrap();
        assert_eq!(e.size, p.len() as u64);
        let mut got = std::vec![0u8; p.len()];
        assert_eq!(v.read(&e, 0, &mut got).unwrap(), p.len());
        assert!(&got == p, "{n}");
        // Unaligned read in the middle.
        let mut x = [0u8; 5000];
        v.read(&e, 12345, &mut x).unwrap();
        assert_eq!(&x[..], &p[12345..17345]);
    }
    // Shrink, grow (zeros), overwrite.
    let mut e = v.lookup("A").unwrap();
    v.set_size(&mut e, 100_001, NOW).unwrap();
    v.set_size(&mut e, 300_000, NOW).unwrap();
    let mut got = std::vec![0u8; 300_000];
    v.read(&e, 0, &mut got).unwrap();
    assert_eq!(&got[..100_001], &pa[..100_001]);
    assert!(got[100_001..].iter().all(|&c| c == 0));
    // A write past the end leaves a zeroed gap.
    v.write(&mut e, 1_000_000, b"tail", NOW).unwrap();
    let mut g = std::vec![0u8; 700_000];
    v.read(&e, 300_000, &mut g).unwrap();
    assert!(g.iter().all(|&c| c == 0));
    v.set_size(&mut e, 0, NOW).unwrap();
    assert_eq!(v.allocated(&e).unwrap(), 0);
    ok(&mut v);
    v.delete("B").unwrap();
    v.delete("BIG").unwrap();
    ok(&mut v);
}

#[test]
fn extended_attributes() {
    let mut v = fresh(32);
    let e = v.create_file("F", 0, NOW).unwrap();
    let small = std::vec![Ea { flags: 0, name: b".TYPE".to_vec(), value: b"\xfd\xff\x04\x00Text".to_vec() }];
    v.write_eas(&e, &small).unwrap();
    let i = v.read_inode(e.ino).unwrap();
    assert_eq!(i.ea().flag, layout::dxd::INLINE);
    assert_eq!(v.read_eas(&e).unwrap(), small);
    // Too big for the inode: an extent.
    let big = std::vec![
        Ea { flags: 0x80, name: b".LONGNAME".to_vec(), value: pattern(5000, 4) },
        Ea { flags: 0, name: b"RS2.X".to_vec(), value: pattern(30_000, 5) },
    ];
    v.write_eas(&e, &big).unwrap();
    assert_eq!(v.read_inode(e.ino).unwrap().ea().flag, layout::dxd::EXTENT);
    ok(&mut v);
    let mut v = remount(v);
    let e = v.lookup("F").unwrap();
    assert_eq!(v.read_eas(&e).unwrap(), big);
    assert_eq!(e.ea_size, v.ea_info(e.ino).unwrap());
    // Back inline, then none.
    v.write_eas(&e, &small).unwrap();
    assert_eq!(v.read_inode(e.ino).unwrap().ea().flag, layout::dxd::INLINE);
    ok(&mut v);
    v.write_eas(&e, &[]).unwrap();
    assert!(v.read_eas(&e).unwrap().is_empty());
    // A file with inline EAs that grows past 8 extents splits its xtree.
    v.write_eas(&e, &small).unwrap();
    let mut e = v.lookup("F").unwrap();
    let mut o = v.create_file("O", 0, NOW).unwrap();
    for k in 0..40u64 {
        v.write(&mut e, k * 4096, &[k as u8; 4096], NOW).unwrap();
        v.write(&mut o, k * 4096, &[1; 4096], NOW).unwrap();
    }
    assert_eq!(v.read_eas(&e).unwrap(), small);
    // Directories: always in an extent.
    v.mkdir("DIR", NOW).unwrap();
    let d = v.lookup("DIR").unwrap();
    v.write_eas(&d, &small).unwrap();
    assert_eq!(v.read_eas(&d).unwrap(), small);
    let too = std::vec![Ea { flags: 0, name: b"X".to_vec(), value: std::vec![0; 65530] }];
    assert_eq!(v.write_eas(&e, &too).unwrap_err(), Error::EaTooLong);
    ok(&mut v);
    v.delete("F").unwrap();
    v.rmdir("DIR").unwrap();
    ok(&mut v);
}

#[test]
fn rename_and_info() {
    let mut v = fresh(32);
    v.mkdir("A", NOW).unwrap();
    v.mkdir("A\\SUB", NOW).unwrap();
    v.mkdir("B", NOW).unwrap();
    let mut f = v.create_file("A\\SUB\\file.txt", attr::HIDDEN, NOW).unwrap();
    v.write(&mut f, 0, b"data", NOW).unwrap();
    v.rename("A\\SUB", "B\\MOVED").unwrap();
    assert_eq!(v.lookup("B\\MOVED\\FILE.TXT").unwrap().size, 4);
    assert_eq!(v.lookup("A\\SUB").unwrap_err(), Error::NotFound);
    assert_eq!(v.rename("B", "B\\MOVED\\X").unwrap_err(), Error::AccessDenied);
    v.rename("B\\MOVED\\file.txt", "B\\MOVED\\FILE.TXT").unwrap();
    assert_eq!(v.lookup("b\\moved\\file.txt").unwrap().name, "FILE.TXT");
    v.create_file("B\\other", 0, NOW).unwrap();
    assert_eq!(v.rename("B\\other", "B\\MOVED").unwrap_err(), Error::Exists);
    let mut e = v.lookup("B\\MOVED\\FILE.TXT").unwrap();
    assert_eq!(e.attr & attr::HIDDEN, attr::HIDDEN);
    e.attr = attr::READONLY;
    e.created = 1000;
    e.modified = 2000;
    e.accessed = 3000;
    v.update_entry(&e).unwrap();
    let mut v = remount(v);
    let e = v.lookup("B\\MOVED\\FILE.TXT").unwrap();
    assert_eq!(e.attr & attr::CHANGEABLE, attr::READONLY);
    assert_eq!((e.created, e.modified, e.accessed), (1000, 2000, 3000));
    assert_eq!(v.delete("B\\MOVED\\FILE.TXT").unwrap_err(), Error::AccessDenied);
    assert_eq!(v.rmdir("B").unwrap_err(), Error::DirectoryNotEmpty);
    ok(&mut v);
    v.set_label("NEWLABEL").unwrap();
    let v = remount(v);
    assert_eq!(v.label, "NEWLABEL");
}

#[test]
fn fill_the_disk() {
    let mut v = fresh(20);
    let chunk = pattern(1 << 20, 6);
    let mut n = 0;
    loop {
        let mut e = v.create_file(&format!("F{n}"), 0, NOW).unwrap();
        match v.write(&mut e, 0, &chunk, NOW) {
            Ok(_) => n += 1,
            Err(Error::DiskFull) => break,
            Err(e) => panic!("{e:?}"),
        }
    }
    assert!(n >= 12, "{n}");
    // Many small files until the inodes or blocks run out.
    let mut k = 0;
    loop {
        match v.create_file(&format!("S{k}"), 0, NOW) {
            Ok(mut e) => {
                if v.write(&mut e, 0, b"x", NOW).is_err() {
                    break;
                }
                k += 1;
            }
            Err(Error::DiskFull) => break,
            Err(e) => panic!("{e:?}"),
        }
    }
    ok(&mut v);
    assert!(v.info().free_blocks < 64);
    let mut v = remount(v);
    for i in 0..n {
        v.delete(&format!("F{i}")).unwrap();
    }
    ok(&mut v);
    assert!(v.info().free_blocks > 12 * 256);
}

#[test]
fn small_blocks() {
    let mut v = fresh_bs(32, 512);
    populate(&mut v);
    ok(&mut v);
    let mut v = remount(v);
    ok(&mut v);
    let e = v.lookup("data\\big.bin").unwrap();
    let mut b = std::vec![0u8; 16];
    v.read(&e, 16 * 1000, &mut b).unwrap();
    assert_eq!(&b, b"REC 00000001000\n");
}

#[test]
fn dirty_volume_is_read_only() {
    let mut v = fresh(32);
    v.create_file("X", 0, NOW).unwrap();
    // Crash: mounted, never marked clean. rs2's own state: still writable.
    let dev = v.dev;
    let mut v = Volume::mount(dev).unwrap();
    assert!(!v.is_readonly());
    v.create_file("Y", 0, NOW).unwrap();
    let dev = v.dev;
    // Another system's log left in use: read-only.
    let lp = layout::Pxd::read(&dev.0.borrow()[0x8000..0x8100], layout::sb::LOGPXD);
    let o = (lp.addr * 4096 + 4096) as usize + layout::logsup::STATE;
    dev.0.borrow_mut()[o..o + 4].copy_from_slice(&layout::LOGMOUNT.to_le_bytes());
    let mut v = Volume::mount(dev).unwrap();
    assert!(v.is_readonly());
    assert_eq!(v.create_file("Z", 0, NOW).unwrap_err(), Error::ReadOnly);
    assert!(v.lookup("Y").is_ok());
}

/// JFS_IMAGE=path cargo test -p jfs make_stress_image -- --ignored
#[test]
#[ignore]
fn make_stress_image() {
    let path = std::env::var("JFS_IMAGE").unwrap();
    let bs: u32 = std::env::var("JFS_BS").ok().and_then(|s| s.parse().ok()).unwrap_or(4096);
    let mut v = fresh_bs(64, bs);
    populate(&mut v);
    let mut r = Lcg(77);
    let mut live: BTreeMap<u32, usize> = BTreeMap::new();
    v.mkdir("CHURN", NOW).unwrap();
    for _ in 0..4000 {
        let k = r.next() % 1500;
        let p = format!("CHURN\\{}{k}", "long name ".repeat((k % 9) as usize));
        if live.remove(&k).is_some() {
            v.delete(&p).unwrap();
        } else {
            let mut e = v.create_file(&p, 0, NOW).unwrap();
            let n = (r.next() % 20000) as usize;
            v.write(&mut e, 0, &pattern(n, k as u64), NOW).unwrap();
            if k % 5 == 0 {
                v.write_eas(&e, &[Ea { flags: 0, name: b"N".to_vec(), value: pattern((k % 300) as usize, 1) }]).unwrap();
            }
            live.insert(k, n);
        }
    }
    let mut a = v.create_file("FRAG", 0, NOW).unwrap();
    for k in 0..300u64 {
        v.write(&mut a, k * 8192, &[7; 4096], NOW).unwrap();
        let mut t = v.create_file(&format!("T{k}"), 0, NOW).unwrap();
        v.write(&mut t, 0, &[1; 4096], NOW).unwrap();
    }
    ok(&mut v);
    std::fs::write(path, v.unmount().unwrap().0.into_inner()).unwrap();
}

/// One change to a volume made elsewhere (JFS_IN, JFS_OUT, JFS_OP).
#[test]
#[ignore]
fn foreign_op() {
    let data = std::fs::read(std::env::var("JFS_IN").unwrap()).unwrap();
    let mut v = Volume::mount(Mem(RefCell::new(data))).unwrap();
    match std::env::var("JFS_OP").unwrap().as_str() {
        "delbig" => v.delete("BIGDIR\\FILE0500.TXT").unwrap(),
        "dellast" => v.delete("BIGDIR\\FILE2000.TXT").unwrap(),
        "addA" => drop(v.create_file("A\\NEW.TXT", 0, NOW).unwrap()),
        "addroot" => drop(v.create_file("NEW.TXT", 0, NOW).unwrap()),
        "addbig" => drop(v.create_file("BIGDIR\\FILE0500X.TXT", 0, NOW).unwrap()),
        o => panic!("{o}"),
    }
    assert!(v.check().unwrap().ok());
    std::fs::write(std::env::var("JFS_OUT").unwrap(), v.unmount().unwrap().0.into_inner()).unwrap();
}

/// A volume with Linux's directory index tables (case-sensitive names).
fn fresh_indexed(mb: usize) -> Volume<Mem> {
    let dev = Mem(RefCell::new(std::vec![0u8; mb << 20]));
    format(&dev, &FormatParams { label: "LNX", now: NOW, ..Default::default() }).unwrap();
    {
        let mut d = dev.0.borrow_mut();
        for sbo in [0x8000usize, 0xf000] {
            let f = (layout::flag::DIR_INDEX | layout::flag::LINUX | layout::flag::GROUPCOMMIT | layout::flag::INLINELOG).to_le_bytes();
            d[sbo + layout::sb::FLAG..sbo + layout::sb::FLAG + 4].copy_from_slice(&f);
        }
    }
    let mut v = Volume::mount(dev).unwrap();
    let mut root = v.read_inode(layout::ROOT_I).unwrap();
    layout::wr32(&mut root.b, layout::di::NEXT_INDEX, 2);
    root.set_size(1);
    v.write_inode(&root).unwrap();
    v.flush().unwrap();
    v
}

#[test]
fn directory_index_tables() {
    let mut v = fresh_indexed(64);
    assert!(!v.ci);
    v.mkdir("D", NOW).unwrap();
    let mut r = Lcg(3);
    let mut live = std::collections::BTreeSet::new();
    for step in 0..5000 {
        let k = r.next() % 1500;
        let n = format!("D\\{}{k}", "Name ".repeat((k % 6) as usize));
        if live.remove(&k) {
            v.delete(&n).unwrap();
        } else {
            v.create_file(&n, 0, NOW).unwrap();
            live.insert(k);
        }
        if step % 1000 == 999 {
            ok(&mut v);
        }
    }
    // Names in any case find the file (OS/2 semantics), so another case
    // of an existing name cannot be created.
    v.create_file("Case", 0, NOW).unwrap();
    assert_eq!(v.create_file("CASE", 0, NOW).unwrap_err(), Error::Exists);
    assert_eq!(v.lookup("case").unwrap().name, "Case");
    let mut v = remount(v);
    ok(&mut v);
    let d = v.lookup("D").unwrap();
    assert_eq!(v.read_dir(&d).unwrap().len(), live.len());
}
