extern crate std;

use super::*;
use core::cell::RefCell;
use std::format;
use std::string::ToString;
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
    let mut v = fresh(64);
    populate(&mut v);
    let dev = v.unmount().unwrap();
    std::fs::write(path, dev.0.into_inner()).unwrap();
}
