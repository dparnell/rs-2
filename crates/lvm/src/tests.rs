//! Host tests on in-memory disks.

extern crate std;

use super::*;
use core::cell::RefCell;

struct Mem(RefCell<Vec<u8>>);

impl Mem {
    fn new(mb: u64) -> Mem {
        Mem(RefCell::new(vec![0u8; (mb << 20) as usize]))
    }
}

impl Disk for Mem {
    fn sectors(&self) -> u64 {
        (self.0.borrow().len() / SECTOR) as u64
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> core::result::Result<(), u32> {
        let o = lba as usize * SECTOR;
        let d = self.0.borrow();
        buf.copy_from_slice(d.get(o..o + buf.len()).ok_or(27u32)?);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> core::result::Result<(), u32> {
        let o = lba as usize * SECTOR;
        let mut d = self.0.borrow_mut();
        d.get_mut(o..o + buf.len()).ok_or(27u32)?.copy_from_slice(buf);
        Ok(())
    }
}

#[test]
fn crc_is_the_reflected_crc32_without_final_xor() {
    assert_eq!(lvm_crc(b"123456789"), !0xCBF4_3926);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn dla_round_trip_and_crc_check() {
    let h = DlaHeader { disk_serial: 0x1234, boot_disk_serial: 0x1234, install_flags: 0, disk_name: "[ D1 ]".into(), reboot: false };
    let e = DlaEntry { volume_serial: 7, partition_serial: 8, size: 16002, start: 63, on_boot_menu: false, installable: true, letter: b'C', volume_name: "SYSTEM".into(), partition_name: "OS2".into() };
    let mut s = make_dla(&h, &[e.clone()]);
    seal_dla(&mut s, 100);
    let (h2, es) = parse_dla(&s).unwrap();
    assert_eq!(h2, h);
    assert_eq!(es[0], e);
    assert_eq!(es[1], DlaEntry::default());
    s[0x50] ^= 1;
    assert!(parse_dla(&s).is_none());
}

#[test]
fn creates_primary_and_logical_partitions_with_dlats() {
    let d = Mem::new(512);
    let mut seed = 99;
    let mut l = Layout::read(&d).unwrap();
    assert_eq!(l.scheme, Scheme::Empty);
    let f = l.free(false)[0];
    assert_eq!(f.start, 63);
    let a = l.create(f, 100, false, false, 0x06, "Boot", &mut seed).unwrap();
    l.set_volume(a, "SYSTEM", b'C', &mut seed).unwrap();
    l.set_active(a).unwrap();
    let f = l.free(true)[0];
    let b = l.create(f, 50, true, false, 0x07, "Data", &mut seed).unwrap();
    l.set_volume(b, "DATA", b'E', &mut seed).unwrap();
    let f = l.free(true)[0];
    l.create(f, 0, true, false, 0x06, "Rest", &mut seed).unwrap();
    l.write(&d, &mut seed).unwrap();

    let r = Layout::read(&d).unwrap();
    assert_eq!(r.scheme, Scheme::Mbr);
    assert_eq!(r.parts.len(), 3);
    let p = &r.parts[0];
    assert_eq!((p.kind, p.start, p.active, p.mbr_type), (Kind::Primary, 63, true, 0x06));
    assert_eq!((p.start + p.count) % CYLINDER, 0);
    assert!(p.mb() >= 100 && p.mb() < 100 + 8);
    let v = p.volume().unwrap();
    assert_eq!((v.letter, v.volume_name.as_str(), v.partition_name.as_str()), (b'C', "SYSTEM", "Boot"));
    let q = &r.parts[1];
    assert_eq!(q.kind, Kind::Logical);
    assert_eq!((q.start - SPT) % CYLINDER, 0);
    assert_eq!(q.volume().unwrap().letter, b'E');
    assert_eq!(r.parts[2].kind, Kind::Logical);
    assert!(r.parts[2].volume().is_none());
    assert_eq!(r.parts[2].name, "Rest");
    // Disk full now.
    assert!(r.free(false).is_empty() && r.free(true).is_empty());
    assert_eq!(r.dla.disk_name, "[ D1 ]");
    assert_eq!(r, Layout::read(&d).unwrap());
}

#[test]
fn deletes_and_reuses_space() {
    let d = Mem::new(256);
    let mut seed = 5;
    let mut l = Layout::read(&d).unwrap();
    for i in 0..3 {
        let f = l.free(true)[0];
        l.create(f, 40, true, false, 0x06, &alloc::format!("L{i}"), &mut seed).unwrap();
    }
    l.write(&d, &mut seed).unwrap();
    let mut l = Layout::read(&d).unwrap();
    assert_eq!(l.parts.len(), 3);
    let i = l.partition_index("L1").unwrap();
    l.delete(i).unwrap();
    l.write(&d, &mut seed).unwrap();
    let l = Layout::read(&d).unwrap();
    let names: Vec<&str> = l.parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["L0", "L2"]);
    // The hole between L0 and L2 can take a logical partition again, and
    // a primary only outside the extended partition.
    let fl = l.free(true);
    assert!(fl.iter().any(|f| f.start < l.parts[1].start));
    let fp = l.free(false);
    assert!(fp.iter().all(|f| f.start > l.parts[1].start || f.start + f.count <= l.parts[0].start - SPT));
}

#[test]
fn letters_and_names_are_unique() {
    let d = Mem::new(128);
    let mut seed = 1;
    let mut l = Layout::read(&d).unwrap();
    let a = l.create(l.free(false)[0], 30, false, false, 6, "A", &mut seed).unwrap();
    let b = l.create(l.free(false)[0], 30, false, false, 6, "B", &mut seed).unwrap();
    l.set_volume(a, "ONE", b'D', &mut seed).unwrap();
    assert_eq!(l.set_volume(b, "TWO", b'D', &mut seed), Err(Error::InUse));
    assert_eq!(l.set_volume(b, "ONE", b'F', &mut seed), Err(Error::InUse));
    l.set_volume(b, "TWO", b'*', &mut seed).unwrap();
    l.delete_volume(a).unwrap();
    assert!(l.parts[a].volume().is_none());
    assert_eq!(l.volume_index("TWO"), Some(b));
}

#[test]
fn gpt_create_and_delete() {
    let d = Mem::new(64);
    // A protective MBR and an empty GPT (as rs2's installer lays it out).
    let mut mbr = vec![0u8; SECTOR];
    mbr[446 + 4] = 0xee;
    put32(&mut mbr, 446 + 8, 1);
    put32(&mut mbr, 446 + 12, (d.sectors() - 1) as u32);
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    d.write(0, &mbr).unwrap();
    let mut h = vec![0u8; SECTOR];
    h[0..8].copy_from_slice(b"EFI PART");
    put32(&mut h, 8, 0x0001_0000);
    put32(&mut h, 12, 92);
    put64(&mut h, 40, 34);
    put64(&mut h, 48, d.sectors() - 34);
    put64(&mut h, 72, 2);
    put32(&mut h, 80, 128);
    put32(&mut h, 84, 128);
    d.write(1, &h).unwrap();
    let mut seed = 3;
    let mut l = Layout::read(&d).unwrap();
    assert_eq!(l.scheme, Scheme::Gpt);
    let f = l.free(false)[0];
    assert_eq!(f.start, 2048);
    l.create(f, 10, false, false, 0, "rs2 data", &mut seed).unwrap();
    l.create(l.free(false)[0], 0, false, false, 0, "rest", &mut seed).unwrap();
    assert_eq!(l.set_volume(0, "X", b'D', &mut seed), Err(Error::NotSupported));
    l.write(&d, &mut seed).unwrap();
    let r = Layout::read(&d).unwrap();
    assert_eq!(r.parts.len(), 2, "{:?}", r.parts.iter().map(|p| (p.start, p.count, p.name.clone())).collect::<Vec<_>>());
    assert_eq!((r.parts[0].start, r.parts[0].count, r.parts[0].name.as_str()), (2048, 10 * 2048, "rs2 data"));
    // Header and entry CRCs hold, in both copies.
    for lba in [1, d.sectors() - 1] {
        let mut x = vec![0u8; SECTOR];
        d.read(lba, &mut x).unwrap();
        let want = le32(&x, 16);
        put32(&mut x, 16, 0);
        assert_eq!(crc32(&x[..92]), want);
        let at = le64(&x, 72);
        let mut e = vec![0u8; 128 * 128];
        d.read(at, &mut e).unwrap();
        assert_eq!(crc32(&e), le32(&x, 88));
    }
    let mut r = r;
    r.delete(0).unwrap();
    r.write(&d, &mut seed).unwrap();
    assert_eq!(Layout::read(&d).unwrap().parts.len(), 1);
}
