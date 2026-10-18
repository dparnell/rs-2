//! Build HPFS disk images with the shared `hpfs` crate.

use std::cell::RefCell;

use crate::image::FileList;

pub struct MemDisk(pub RefCell<Vec<u8>>);

impl hpfs::BlockDevice for MemDisk {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> hpfs::Result<()> {
        let o = lba as usize * 512;
        buf.copy_from_slice(&self.0.borrow()[o..o + buf.len()]);
        Ok(())
    }
    fn write(&self, lba: u64, buf: &[u8]) -> hpfs::Result<()> {
        let o = lba as usize * 512;
        self.0.borrow_mut()[o..o + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

fn now() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

/// An MBR-partitioned disk image with one HPFS partition (type 0x07,
/// starting at sector 2048) holding `files`. Names keep their case.
pub fn disk(files: &FileList, label: &str, size_mb: u64) -> Vec<u8> {
    let total = (size_mb << 20) as usize;
    let start = 2048usize;
    let part_bytes = total - start * 512;
    let ts = now();
    let part = MemDisk(RefCell::new(vec![0u8; part_bytes]));
    let p = hpfs::FormatParams { label, serial: 0x2026_1018, hidden_sectors: start as u32, code_page: 850, now: ts };
    hpfs::format(&part, &p).expect("format");
    let mut vol = hpfs::Volume::mount(part).expect("mount");
    for (src, dest) in files {
        let data = std::fs::read(src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
        let dest = dest.replace('/', "\\");
        let parts: Vec<&str> = dest.split('\\').filter(|s| !s.is_empty()).collect();
        let mut p = String::new();
        for d in &parts[..parts.len() - 1] {
            if !p.is_empty() {
                p.push('\\');
            }
            p.push_str(d);
            if vol.lookup(&p).is_err() {
                vol.mkdir(&p, ts).unwrap_or_else(|e| panic!("mkdir {p}: {e:?}"));
            }
        }
        if vol.lookup(&dest).is_ok() {
            vol.delete(&dest).ok();
        }
        let mut e = vol.create_file(&dest, 0, ts).unwrap_or_else(|e| panic!("create {dest}: {e:?}"));
        vol.write(&mut e, 0, &data, ts).unwrap_or_else(|e| panic!("write {dest}: {e:?}"));
    }
    let part = vol.dev.0.into_inner();
    let mut img = vec![0u8; total];
    img[start * 512..].copy_from_slice(&part);
    let e = 446;
    img[e + 1..e + 4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    img[e + 4] = 0x07;
    img[e + 5..e + 8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    img[e + 8..e + 12].copy_from_slice(&(start as u32).to_le_bytes());
    img[e + 12..e + 16].copy_from_slice(&((part_bytes / 512) as u32).to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xaa;
    img
}
