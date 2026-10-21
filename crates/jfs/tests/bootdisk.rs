//! A GPT test disk booting rs2 with C: on JFS: an EFI system partition
//! (a FAT system disk from `cargo xtask disk --system`, its CMDLINE given
//! `root=` the JFS partition's GUID) and a JFS partition holding a copy of
//! its files.
//!
//! SYS_IMG=sys.img BOOT_IMG=out.img cargo test -p jfs --test bootdisk -- --ignored

use std::cell::RefCell;

struct Mem(RefCell<Vec<u8>>);

impl fat::BlockDevice for &Mem {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> fat::Result<()> {
        let o = lba as usize * 512;
        buf.copy_from_slice(&self.0.borrow()[o..o + buf.len()]);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> fat::Result<()> {
        let o = lba as usize * 512;
        self.0.borrow_mut()[o..o + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

impl jfs::BlockDevice for &Mem {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> jfs::Result<()> {
        let o = lba as usize * 512;
        buf.copy_from_slice(&self.0.borrow()[o..o + buf.len()]);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> jfs::Result<()> {
        let o = lba as usize * 512;
        self.0.borrow_mut()[o..o + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// GUID text (mixed-endian, as the kernel prints it) of 16 bytes.
fn guid_text(g: &[u8; 16]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
        u32::from_le_bytes(g[0..4].try_into().unwrap()),
        u16::from_le_bytes(g[4..6].try_into().unwrap()),
        u16::from_le_bytes(g[6..8].try_into().unwrap()),
        g[8],
        g[9],
        g[10..].iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn copy_tree(f: &fat::Volume<&Mem>, dir: &fat::Entry, path: &str, j: &mut jfs::Volume<&Mem>, now: u32) -> usize {
    let mut n = 0;
    let d = if dir.cluster == 0 { f.root_dir() } else { fat::Dir::Cluster(dir.cluster) };
    for e in f.read_dir(d).unwrap() {
        if e.name == "." || e.name == ".." {
            continue;
        }
        let p = if path.is_empty() { e.name.clone() } else { format!("{path}\\{}", e.name) };
        if p.eq_ignore_ascii_case("EFI") {
            continue;
        }
        if e.is_dir() {
            j.mkdir(&p, now).unwrap();
            n += copy_tree(f, &e, &p, j, now);
        } else {
            let mut data = vec![0u8; e.size as usize];
            f.read(&e, 0, &mut data).unwrap();
            let mut je = j.create_file(&p, e.attr & 0x27, now).unwrap();
            j.write(&mut je, 0, &data, now).unwrap();
            n += 1;
        }
    }
    n
}

#[test]
#[ignore]
fn make_boot_disk() {
    let sys = std::fs::read(std::env::var("SYS_IMG").unwrap()).unwrap();
    let start = u32::from_le_bytes(sys[446 + 8..446 + 12].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(sys[446 + 12..446 + 16].try_into().unwrap()) as usize;
    let esp = Mem(RefCell::new(sys[start * 512..(start + count) * 512].to_vec()));
    let jfs_bytes = 192usize << 20;
    let part = Mem(RefCell::new(vec![0u8; jfs_bytes]));
    let now = 1_760_000_000;
    let esp_lba = 2048u64;
    let jfs_lba = esp_lba + count as u64;
    jfs::format(&&part, &jfs::FormatParams { label: "RS2JFS", serial: 0x0a0b_0c0d, hidden_sectors: jfs_lba as u32, now, ..Default::default() }).unwrap();
    let mut guid = [0u8; 16];
    guid.copy_from_slice(b"rs2-jfs-root-par");
    guid[7] = 0x40 | (guid[7] & 0x0f);
    guid[8] = 0x80 | (guid[8] & 0x3f);
    {
        let mut f = fat::Volume::mount(&esp).unwrap();
        let mut j = jfs::Volume::mount(&part).unwrap();
        let n = copy_tree(&f, &f.root_entry(), "", &mut j, now);
        let r = j.check().unwrap();
        assert!(r.ok(), "{:?}", r.problems);
        j.unmount().unwrap();
        eprintln!("{n} files copied to JFS");
        // root= on the command line.
        let ts = fat::Timestamp::new(2026, 10, 22, 12, 0, 0);
        let mut e = match f.lookup("RS2\\CMDLINE") {
            Ok(e) => e,
            Err(_) => f.create_file("RS2\\CMDLINE", 0, ts).unwrap(),
        };
        let mut cmd = vec![0u8; e.size as usize];
        f.read(&e, 0, &mut cmd).unwrap();
        let mut s = String::from_utf8(cmd).unwrap().trim().to_string();
        s = format!("{s} root={}", guid_text(&guid));
        eprintln!("CMDLINE: {s}");
        f.set_size(&mut e, 0, ts).unwrap();
        f.write(&mut e, 0, s.as_bytes(), ts).unwrap();
    }
    // The GPT disk.
    let total = jfs_lba as usize + jfs_bytes / 512 + 2048;
    let mut d = vec![0u8; total * 512];
    d[esp_lba as usize * 512..(esp_lba as usize + count) * 512].copy_from_slice(&esp.0.borrow());
    d[jfs_lba as usize * 512..jfs_lba as usize * 512 + jfs_bytes].copy_from_slice(&part.0.borrow());
    // Protective MBR.
    d[446 + 4] = 0xee;
    d[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    d[446 + 12..446 + 16].copy_from_slice(&((total - 1) as u32).to_le_bytes());
    d[510] = 0x55;
    d[511] = 0xaa;
    let mut entries = vec![0u8; 128 * 128];
    let esp_type: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];
    let data_type: [u8; 16] = [0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7];
    let mut put = |k: usize, ty: &[u8; 16], id: &[u8; 16], first: u64, last: u64, name: &str| {
        let e = &mut entries[k * 128..(k + 1) * 128];
        e[0..16].copy_from_slice(ty);
        e[16..32].copy_from_slice(id);
        e[32..40].copy_from_slice(&first.to_le_bytes());
        e[40..48].copy_from_slice(&last.to_le_bytes());
        for (i, c) in name.encode_utf16().enumerate() {
            e[56 + 2 * i..58 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
    };
    put(0, &esp_type, b"rs2-esp-part-uid", esp_lba, esp_lba + count as u64 - 1, "EFI system");
    put(1, &data_type, &guid, jfs_lba, jfs_lba + (jfs_bytes / 512) as u64 - 1, "RS2 JFS");
    let ecrc = crc32(&entries);
    for (lba, alt) in [(1u64, (total - 1) as u64), ((total - 1) as u64, 1u64)] {
        let elba = if lba == 1 { 2 } else { (total - 33) as u64 };
        let mut h = vec![0u8; 92];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&lba.to_le_bytes());
        h[32..40].copy_from_slice(&alt.to_le_bytes());
        h[40..48].copy_from_slice(&34u64.to_le_bytes());
        h[48..56].copy_from_slice(&((total - 34) as u64).to_le_bytes());
        h[56..72].copy_from_slice(b"rs2-jfs-test-dsk");
        h[72..80].copy_from_slice(&elba.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&ecrc.to_le_bytes());
        let c = crc32(&h);
        h[16..20].copy_from_slice(&c.to_le_bytes());
        d[lba as usize * 512..lba as usize * 512 + 92].copy_from_slice(&h);
        d[elba as usize * 512..elba as usize * 512 + entries.len()].copy_from_slice(&entries);
    }
    std::fs::write(std::env::var("BOOT_IMG").unwrap(), d).unwrap();
}
