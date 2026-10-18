//! Hybrid ISO 9660 image: bootable as a CD (El Torito, UEFI) and, written
//! to a USB stick, as a disk (an MBR partition covers the same EFI system
//! partition image).
//!
//! Layout (2048-byte sectors):
//! ```text
//! 0-15   system area: MBR with one EFI system partition entry
//! 16     primary volume descriptor
//! 17     El Torito boot record
//! 18     volume descriptor set terminator
//! 19     boot catalog
//! 20/21  path tables (little/big endian)
//! 22     root directory
//! 23..   files (the ESP image first)
//! ```

use std::path::Path;

const SECTOR: usize = 2048;

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
    let total = next;

    // Root directory.
    let mut root = Vec::new();
    root.extend(dir_record(&[0], 22, SECTOR as u32, true));
    root.extend(dir_record(&[1], 22, SECTOR as u32, true));
    for (name, lba, data) in &placed {
        root.extend(dir_record(name, *lba, data.len() as u32, false));
    }
    assert!(root.len() <= SECTOR, "root directory too large");
    root.resize(SECTOR, 0);

    let mut img = vec![0u8; total as usize * SECTOR];

    // MBR: partition 1 = EFI system partition over the ESP image.
    let start512 = esp_lba * 4;
    let count512 = (esp.len() as u32).div_ceil(512);
    let mut part = [0u8; 16];
    part[0] = 0x00;
    part[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    part[4] = 0xef;
    part[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    part[8..12].copy_from_slice(&start512.to_le_bytes());
    part[12..16].copy_from_slice(&count512.to_le_bytes());
    img[446..462].copy_from_slice(&part);
    img[440..444].copy_from_slice(&0x5253_3200u32.to_le_bytes()); // disk signature "RS2"
    img[510] = 0x55;
    img[511] = 0xaa;

    // Primary volume descriptor.
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
    img[16 * SECTOR..17 * SECTOR].copy_from_slice(&pvd);

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

    // Terminator.
    let mut term = vec![0u8; SECTOR];
    term[0] = 255;
    term[1..6].copy_from_slice(b"CD001");
    term[6] = 1;
    img[18 * SECTOR..19 * SECTOR].copy_from_slice(&term);

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

    // Path tables: just the root.
    let mut lpt = vec![0u8; SECTOR];
    lpt[0] = 1;
    lpt[2..6].copy_from_slice(&22u32.to_le_bytes());
    lpt[6..8].copy_from_slice(&1u16.to_le_bytes());
    img[20 * SECTOR..21 * SECTOR].copy_from_slice(&lpt);
    let mut mpt = vec![0u8; SECTOR];
    mpt[0] = 1;
    mpt[2..6].copy_from_slice(&22u32.to_be_bytes());
    mpt[6..8].copy_from_slice(&1u16.to_be_bytes());
    img[21 * SECTOR..22 * SECTOR].copy_from_slice(&mpt);

    img[22 * SECTOR..23 * SECTOR].copy_from_slice(&root);
    for (_, lba, data) in &placed {
        let at = *lba as usize * SECTOR;
        img[at..at + data.len()].copy_from_slice(data);
    }
    std::fs::write(out, img).expect("write iso");
}
