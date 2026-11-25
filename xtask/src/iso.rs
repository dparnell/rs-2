//! Hybrid ISO 9660 image: bootable as a CD (El Torito, UEFI) and, written
//! to a USB stick, as a disk: a GPT and an MBR partition both cover the
//! same EFI system partition image.
//!
//! The GPT matters on real machines: firmware looks at a stick's GPT
//! first, then El Torito, then the MBR (EDK2's order). El Torito's boot
//! entry counts 512-byte sectors in 16 bits (32 MB), less than the ESP
//! image, so firmware that took the El Torito view of a stick found a
//! truncated FAT and did not boot it (a Surface Pro 4, 2026-11-15; QEMU's
//! OVMF and VMware's CD boot did not mind). On a CD (2048-byte sectors)
//! the GPT is not where firmware looks for one, and El Torito is used.
//!
//! Layout (2048-byte sectors):
//! ```text
//! 0-15   system area: MBR (protective GPT entry + EFI system partition
//!        entry), GPT header at 512-byte LBA 1, its entries at LBA 2-33
//! 16     primary volume descriptor
//! 17     El Torito boot record
//! 18     volume descriptor set terminator
//! 19     boot catalog
//! 20/21  path tables (little/big endian)
//! 22     root directory
//! 23..   files (the ESP image first)
//! last 17  backup GPT (entries, then the header in the last 512 bytes)
//! ```

use std::path::Path;

const SECTOR: usize = 2048;
/// 2048-byte sectors at the end for the backup GPT (33 512-byte LBAs).
const GPT_TAIL_SECTORS: u32 = 17;
/// GPT entries: 128 of 128 bytes, 32 LBAs.
const GPT_ENTRIES: usize = 128;
const GPT_ENTRY_SIZE: usize = 128;

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// GPT with one EFI system partition at 512-byte LBA `start`, `count`
/// LBAs long, on a disk of `total512` LBAs: primary at LBA 1-33, backup
/// in the last 33 LBAs. The GUIDs are fixed (the image is reproducible).
fn write_gpt(img: &mut [u8], total512: u64, start: u64, count: u64) {
    const ESP: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];
    const DISK: [u8; 16] = *b"RS2 LIVE DISK\x00\x00\x01";
    const PART: [u8; 16] = *b"RS2 LIVE ESP\x00\x00\x00\x01";
    let mut entries = vec![0u8; GPT_ENTRIES * GPT_ENTRY_SIZE];
    entries[0..16].copy_from_slice(&ESP);
    entries[16..32].copy_from_slice(&PART);
    entries[32..40].copy_from_slice(&start.to_le_bytes());
    entries[40..48].copy_from_slice(&(start + count - 1).to_le_bytes());
    for (i, c) in "RS2BOOT".encode_utf16().enumerate() {
        entries[56 + 2 * i..58 + 2 * i].copy_from_slice(&c.to_le_bytes());
    }
    let entries_crc = crc32(&entries);
    let last = total512 - 1;
    let tables = (GPT_ENTRIES * GPT_ENTRY_SIZE / 512) as u64;
    let header = |me: u64, other: u64, table: u64| {
        let mut h = [0u8; 92];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&me.to_le_bytes());
        h[32..40].copy_from_slice(&other.to_le_bytes());
        h[40..48].copy_from_slice(&(2 + tables).to_le_bytes());
        h[48..56].copy_from_slice(&(last - 1 - tables).to_le_bytes());
        h[56..72].copy_from_slice(&DISK);
        h[72..80].copy_from_slice(&table.to_le_bytes());
        h[80..84].copy_from_slice(&(GPT_ENTRIES as u32).to_le_bytes());
        h[84..88].copy_from_slice(&(GPT_ENTRY_SIZE as u32).to_le_bytes());
        h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let crc = crc32(&h);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    };
    let at = |lba: u64| lba as usize * 512;
    img[at(1)..at(1) + 92].copy_from_slice(&header(1, last, 2));
    img[at(2)..at(2) + entries.len()].copy_from_slice(&entries);
    img[at(last)..at(last) + 92].copy_from_slice(&header(last, 1, last - tables));
    img[at(last - tables)..at(last - tables) + entries.len()].copy_from_slice(&entries);
}

fn both16(v: u16) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[0..2].copy_from_slice(&v.to_le_bytes());
    b[2..4].copy_from_slice(&v.to_be_bytes());
    b
}

fn both32(v: u32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0..4].copy_from_slice(&v.to_le_bytes());
    b[4..8].copy_from_slice(&v.to_be_bytes());
    b
}

fn pad(s: &str, n: usize) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().take(n).collect();
    v.resize(n, b' ');
    v
}

/// ISO 9660 date/time (7-byte directory form).
fn dir_date() -> [u8; 7] {
    [126, 10, 18, 0, 0, 0, 0] // 2026-10-18 00:00:00 GMT
}

fn dir_record(name: &[u8], lba: u32, size: u32, dir: bool) -> Vec<u8> {
    let len = 33 + name.len() + (name.len() + 1) % 2;
    let mut r = vec![0u8; len];
    r[0] = len as u8;
    r[2..10].copy_from_slice(&both32(lba));
    r[10..18].copy_from_slice(&both32(size));
    r[18..25].copy_from_slice(&dir_date());
    r[25] = if dir { 2 } else { 0 };
    r[28..32].copy_from_slice(&both16(1));
    r[32] = name.len() as u8;
    r[33..33 + name.len()].copy_from_slice(name);
    r
}

/// The root directory (one sector, at 22) of files: name, sector, size.
fn root_dir<'a>(files: impl Iterator<Item = (&'a [u8], u32, u32)>) -> Vec<u8> {
    let mut root = Vec::new();
    root.extend(dir_record(&[0], 22, SECTOR as u32, true));
    root.extend(dir_record(&[1], 22, SECTOR as u32, true));
    for (name, lba, size) in files {
        root.extend(dir_record(name, lba, size, false));
    }
    assert!(root.len() <= SECTOR, "root directory too large");
    root.resize(SECTOR, 0);
    root
}

/// The primary volume descriptor.
fn volume(label: &str, total: u32) -> Vec<u8> {
    let mut pvd = vec![0u8; SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    pvd[8..40].copy_from_slice(&pad("RS2", 32));
    pvd[40..72].copy_from_slice(&pad(label, 32));
    pvd[80..88].copy_from_slice(&both32(total));
    pvd[120..124].copy_from_slice(&both16(1));
    pvd[124..128].copy_from_slice(&both16(1));
    pvd[128..132].copy_from_slice(&both16(SECTOR as u16));
    pvd[132..140].copy_from_slice(&both32(10)); // path table size
    pvd[140..144].copy_from_slice(&20u32.to_le_bytes());
    pvd[148..152].copy_from_slice(&21u32.to_be_bytes());
    let rootrec = dir_record(&[0], 22, SECTOR as u32, true);
    pvd[156..156 + 34].copy_from_slice(&rootrec[..34]);
    pvd[190..318].copy_from_slice(&pad("RS2", 128));
    pvd[318..446].copy_from_slice(&pad("RS2 PROJECT", 128));
    pvd[446..574].copy_from_slice(&pad("CARGO XTASK ISO", 128));
    pvd[574..702].copy_from_slice(&pad("", 128));
    let date = b"2026101800000000\0";
    for at in [813usize, 830, 847, 864] {
        pvd[at..at + 17].copy_from_slice(date);
    }
    pvd[881] = 1;
    pvd
}

fn terminator() -> Vec<u8> {
    let mut term = vec![0u8; SECTOR];
    term[0] = 255;
    term[1..6].copy_from_slice(b"CD001");
    term[6] = 1;
    term
}

/// Path tables (sectors 20 and 21): just the root.
fn path_tables(img: &mut [u8]) {
    for (at, lba, one) in [(20, 22u32.to_le_bytes(), 1u16.to_le_bytes()), (21, 22u32.to_be_bytes(), 1u16.to_be_bytes())] {
        let t = &mut img[at * SECTOR..];
        t[0] = 1;
        t[2..6].copy_from_slice(&lba);
        t[6..8].copy_from_slice(&one);
    }
}

/// A disc that does not boot: `files` (8.3 names) in its root directory.
pub fn write_data(out: &Path, label: &str, files: &[(String, Vec<u8>)]) {
    let mut next = 23u32;
    let mut placed = Vec::new();
    for (name, data) in files {
        placed.push((format!("{};1", name.to_ascii_uppercase()).into_bytes(), next, data));
        next += data.len().div_ceil(SECTOR).max(1) as u32;
    }
    placed.sort();
    let mut img = vec![0u8; next as usize * SECTOR];
    img[16 * SECTOR..17 * SECTOR].copy_from_slice(&volume(label, next));
    img[17 * SECTOR..18 * SECTOR].copy_from_slice(&terminator());
    path_tables(&mut img);
    let root = root_dir(placed.iter().map(|(n, l, d)| (&n[..], *l, d.len() as u32)));
    img[22 * SECTOR..23 * SECTOR].copy_from_slice(&root);
    for (_, lba, data) in &placed {
        let at = *lba as usize * SECTOR;
        img[at..at + data.len()].copy_from_slice(data);
    }
    std::fs::write(out, img).expect("write iso");
}

/// Write the ISO. `esp` is the FAT image used for UEFI boot; `files` are
/// extra (name, contents) pairs placed in the root directory.
pub fn write(out: &Path, label: &str, esp: &[u8], files: &[(String, Vec<u8>)]) {
    let esp_lba = 23u32;
    let esp_sectors = esp.len().div_ceil(SECTOR) as u32;
    let mut next = esp_lba + esp_sectors;
    let mut placed: Vec<(Vec<u8>, u32, &[u8])> = Vec::new();
    placed.push((b"EFIBOOT.IMG;1".to_vec(), esp_lba, esp));
    for (name, data) in files {
        placed.push((format!("{};1", name.to_ascii_uppercase()).into_bytes(), next, data));
        next += data.len().div_ceil(SECTOR).max(1) as u32;
    }
    // The backup GPT: 32 512-byte LBAs of entries and the header.
    let total = next + GPT_TAIL_SECTORS;

    // Root directory.
    let root = root_dir(placed.iter().map(|(n, l, d)| (&n[..], *l, d.len() as u32)));

    let mut img = vec![0u8; total as usize * SECTOR];

    // MBR: entry 1 protects the GPT (firmware that reads the GPT wants an
    // 0xEE entry starting at LBA 1), entry 2 is the EFI system partition
    // over the ESP image for firmware that only reads the MBR.
    let start512 = esp_lba * 4;
    let count512 = (esp.len() as u32).div_ceil(512);
    let total512 = total as u64 * (SECTOR / 512) as u64;
    let mbr_entry = |boot: u8, ty: u8, start: u32, count: u32| {
        let mut e = [0u8; 16];
        e[0] = boot;
        e[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
        e[4] = ty;
        e[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
        e[8..12].copy_from_slice(&start.to_le_bytes());
        e[12..16].copy_from_slice(&count.to_le_bytes());
        e
    };
    img[446..462].copy_from_slice(&mbr_entry(0, 0xee, 1, (total512 - 1).min(u32::MAX as u64) as u32));
    img[462..478].copy_from_slice(&mbr_entry(0x80, 0xef, start512, count512));
    img[440..444].copy_from_slice(&0x5253_3200u32.to_le_bytes()); // disk signature "RS2"
    img[510] = 0x55;
    img[511] = 0xaa;
    write_gpt(&mut img, total512, start512 as u64, count512 as u64);

    img[16 * SECTOR..17 * SECTOR].copy_from_slice(&volume(label, total));

    // El Torito boot record.
    let mut br = vec![0u8; SECTOR];
    br[0] = 0;
    br[1..6].copy_from_slice(b"CD001");
    br[6] = 1;
    br[7..39].copy_from_slice(&{
        let mut v = b"EL TORITO SPECIFICATION".to_vec();
        v.resize(32, 0);
        v
    });
    br[71..75].copy_from_slice(&19u32.to_le_bytes());
    img[17 * SECTOR..18 * SECTOR].copy_from_slice(&br);

    img[18 * SECTOR..19 * SECTOR].copy_from_slice(&terminator());

    // Boot catalog: validation entry + default entry (UEFI, no emulation).
    let mut cat = vec![0u8; SECTOR];
    cat[0] = 1;
    cat[1] = 0xef; // platform: EFI
    cat[4..28].copy_from_slice(&pad("RS2", 24));
    cat[30] = 0x55;
    cat[31] = 0xaa;
    let mut sum: u16 = 0;
    for i in (0..32).step_by(2) {
        sum = sum.wrapping_add(u16::from_le_bytes([cat[i], cat[i + 1]]));
    }
    cat[28..30].copy_from_slice(&(0u16.wrapping_sub(sum)).to_le_bytes());
    cat[32] = 0x88; // bootable
    cat[33] = 0; // no emulation
    let vsectors = count512.min(0xffff) as u16;
    cat[38..40].copy_from_slice(&vsectors.to_le_bytes());
    cat[40..44].copy_from_slice(&esp_lba.to_le_bytes());
    img[19 * SECTOR..20 * SECTOR].copy_from_slice(&cat);

    path_tables(&mut img);

    img[22 * SECTOR..23 * SECTOR].copy_from_slice(&root);
    for (_, lba, data) in &placed {
        let at = *lba as usize * SECTOR;
        img[at..at + data.len()].copy_from_slice(data);
    }
    std::fs::write(out, img).expect("write iso");
}
