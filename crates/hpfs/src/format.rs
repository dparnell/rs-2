//! Creating an empty HPFS volume.

use alloc::vec;
use alloc::vec::Vec;

use crate::dir::{set_dir_root_in, Dirent, Dnode};
use crate::layout::*;
use crate::{attr, BlockDevice, CaseMap, Error, Result, DOTDOT};

/// Parameters for [`format`].
pub struct FormatParams<'a> {
    pub label: &'a str,
    pub serial: u32,
    /// Sectors before the volume (partition offset), recorded in the BPB.
    pub hidden_sectors: u32,
    /// Code page of file names; 437 and 850 have built-in case tables.
    pub code_page: u16,
    /// Time stamp for the root directory (seconds since 1970).
    pub now: u32,
}

/// Smallest volume accepted (1 MB).
const MIN_SECTORS: u32 = 2048;
/// HPFS addresses at most 64 GB.
const MAX_SECTORS: u32 = 1 << 27;
const HOTFIX_SPARES: u32 = 16;

/// Hands out sectors of the fixed area at the start of the volume.
struct Cursor(u32);

impl Cursor {
    fn take(&mut self, n: u32, align: u32) -> u32 {
        self.0 = self.0.next_multiple_of(align);
        let s = self.0;
        self.0 += n;
        s
    }
}

struct Bitmaps {
    total: u32,
    maps: Vec<Vec<u8>>,
}

impl Bitmaps {
    fn new(total: u32) -> Self {
        let bands = total.div_ceil(BAND);
        let maps = (0..bands)
            .map(|b| {
                let n = BAND.min(total - b * BAND);
                let mut m = vec![0u8; BITMAP_BYTES];
                m[..(n / 8) as usize].fill(0xff);
                for s in n & !7..n {
                    m[(s / 8) as usize] |= 1 << (s % 8);
                }
                m
            })
            .collect();
        Bitmaps { total, maps }
    }

    fn mark_used(&mut self, start: u32, n: u32) {
        for s in start..(start + n).min(self.total) {
            self.maps[(s / BAND) as usize][((s % BAND) / 8) as usize] &= !(1 << (s % 8));
        }
    }
}

fn boot_sector(total: u32, p: &FormatParams) -> Vec<u8> {
    let mut bs = vec![0u8; SECTOR];
    // jmp to an endless loop: this volume is not bootable.
    bs[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    bs[0x3e..0x40].copy_from_slice(&[0xeb, 0xfe]);
    bs[3..11].copy_from_slice(b"RS2 4.52");
    wr16(&mut bs, 11, SECTOR as u16);
    bs[13] = 1; // sectors per cluster
    bs[21] = 0xf8; // media
    if total < 0x10000 {
        wr16(&mut bs, 19, total as u16);
    } else {
        wr32(&mut bs, 32, total);
    }
    wr16(&mut bs, 24, 63);
    wr16(&mut bs, 26, 255);
    wr32(&mut bs, 28, p.hidden_sectors);
    bs[36] = 0x80;
    bs[38] = 0x29;
    wr32(&mut bs, 39, p.serial);
    let mut label = [b' '; 11];
    for (d, s) in label.iter_mut().zip(p.label.bytes()) {
        *d = s.to_ascii_uppercase();
    }
    bs[43..54].copy_from_slice(&label);
    bs[54..62].copy_from_slice(b"HPFS    ");
    bs[510] = 0x55;
    bs[511] = 0xaa;
    bs
}

fn code_page_sectors(cp: u16, data_sec: u32) -> (Vec<u8>, Vec<u8>) {
    let mut dir = vec![0u8; SECTOR];
    wr32(&mut dir, 0, CPDIR_MAGIC);
    wr32(&mut dir, 4, 1);
    // Entry 0: index, code page, bounds, data sector, index in data.
    wr16(&mut dir, 16 + 2, cp);
    wr32(&mut dir, 16 + 8, data_sec);
    let mut data = vec![0u8; SECTOR];
    wr32(&mut data, 0, CPDATA_MAGIC);
    wr32(&mut data, 4, 1);
    let off = 26;
    wr16(&mut data, 20, off as u16);
    wr16(&mut data, off + 2, cp);
    data[off + 6..off + 6 + 128].copy_from_slice(&CaseMap::for_code_page(cp).high());
    (dir, data)
}

/// Create an empty HPFS file system on `dev`.
pub fn format<D: BlockDevice>(dev: &D, p: &FormatParams) -> Result<()> {
    if dev.sector_size() != SECTOR {
        return Err(Error::Io);
    }
    let total = dev.sector_count().min(MAX_SECTORS as u64) as u32;
    if total < MIN_SECTORS {
        return Err(Error::DiskFull);
    }
    let bands = total.div_ceil(BAND);
    let mut bm = Bitmaps::new(total);
    let mut c = Cursor(SPARE_LBA + 1);
    let hotfix_list = c.take(4, 1);
    let spares = c.take(HOTFIX_SPARES, 1);
    let list_secs = ((bands * 4).div_ceil(SECTOR as u32)).next_multiple_of(4);
    let bitmap_list = c.take(list_secs, 4);
    let mut bitmap_secs = vec![c.take(4, 4)];
    for b in 1..bands {
        let start = b * BAND;
        // A tiny last band keeps its bitmap in band 0.
        bitmap_secs.push(if total - start >= 64 { start } else { c.take(4, 4) });
    }
    let bad_list = c.take(4, 4);
    let cp_dir = c.take(1, 1);
    let cp_data = c.take(1, 1);
    let n_dspares: u32 = if total < 16384 { 4 } else { 20 };
    let dspares: Vec<u32> = (0..n_dspares).map(|_| c.take(DNODE_SECTORS, DNODE_SECTORS)).collect();
    let fixed_end = c.0;

    // Directory band (with its bitmap and the root F-node in front) in the
    // middle band that has room for it.
    let mut dir_sectors = ((total / 64) & !3).clamp(64, 8000);
    let band_room = |b: u32| {
        let base = if b == 0 { fixed_end.next_multiple_of(4) } else { b * BAND + 4 };
        let end = ((b + 1) * BAND).min(total);
        (base, end.saturating_sub(base + 8))
    };
    let mut mb = bands / 2;
    while mb > 0 && (bitmap_secs[mb as usize] != mb * BAND || band_room(mb).1 < dir_sectors) {
        mb -= 1;
    }
    let (base, room) = band_room(mb);
    dir_sectors = dir_sectors.min(room & !3);
    if dir_sectors < 16 {
        return Err(Error::DiskFull);
    }
    let dir_bitmap = base;
    let root_fnode = base + 4;
    let dir_start = base + 8;
    let root_dnode = dir_start;

    bm.mark_used(0, fixed_end);
    for &s in &bitmap_secs {
        bm.mark_used(s, 4);
    }
    bm.mark_used(base, 8 + dir_sectors);

    let z4 = vec![0u8; 4 * SECTOR];
    dev.write(0, &boot_sector(total, p))?;
    for s in 1..SUPER_LBA {
        dev.write(s as u64, &z4[..SECTOR])?;
    }

    let mut sb = vec![0u8; SECTOR];
    wr32(&mut sb, 0, SUPER_MAGIC.0);
    wr32(&mut sb, 4, SUPER_MAGIC.1);
    sb[sb::VERSION] = 2;
    sb[sb::FUNC_VERSION] = 2;
    wr32(&mut sb, sb::ROOT, root_fnode);
    wr32(&mut sb, sb::SECTORS, total);
    wr32(&mut sb, sb::BITMAPS, bitmap_list);
    wr32(&mut sb, sb::BAD_LIST, bad_list);
    wr32(&mut sb, sb::DIR_BAND_SECTORS, dir_sectors);
    wr32(&mut sb, sb::DIR_BAND_START, dir_start);
    wr32(&mut sb, sb::DIR_BAND_END, dir_start + dir_sectors - 1);
    wr32(&mut sb, sb::DIR_BAND_BITMAP, dir_bitmap);
    dev.write(SUPER_LBA as u64, &sb)?;

    let mut sp = vec![0u8; SECTOR];
    wr32(&mut sp, 0, SPARE_MAGIC.0);
    wr32(&mut sp, 4, SPARE_MAGIC.1);
    wr32(&mut sp, sp::HOTFIX_LIST, hotfix_list);
    wr32(&mut sp, sp::HOTFIX_COUNT, HOTFIX_SPARES);
    wr32(&mut sp, sp::SPARE_DNODES_FREE, n_dspares);
    wr32(&mut sp, sp::SPARE_DNODES, n_dspares);
    wr32(&mut sp, sp::CODE_PAGE_DIR, cp_dir);
    wr32(&mut sp, sp::CODE_PAGES, 1);
    for (i, &d) in dspares.iter().enumerate() {
        wr32(&mut sp, sp::SPARE_DNODE_LIST + i * 4, d);
    }
    dev.write(SPARE_LBA as u64, &sp)?;

    // Hot-fix table: bad sectors, replacements, owning F-nodes.
    let mut hf = z4.clone();
    for i in 0..HOTFIX_SPARES {
        wr32(&mut hf, ((HOTFIX_SPARES + i) * 4) as usize, spares + i);
    }
    dev.write(hotfix_list as u64, &hf)?;
    for s in spares..spares + HOTFIX_SPARES {
        dev.write(s as u64, &z4[..SECTOR])?;
    }

    let mut list = vec![0u8; list_secs as usize * SECTOR];
    for (i, &s) in bitmap_secs.iter().enumerate() {
        wr32(&mut list, i * 4, s);
    }
    dev.write(bitmap_list as u64, &list)?;
    for (s, m) in bitmap_secs.iter().zip(&bm.maps) {
        dev.write(*s as u64, m)?;
    }
    dev.write(bad_list as u64, &z4)?;
    let (cpd, cpx) = code_page_sectors(p.code_page, cp_data);
    dev.write(cp_dir as u64, &cpd)?;
    dev.write(cp_data as u64, &cpx)?;
    for &d in &dspares {
        dev.write(d as u64, &z4)?;
    }

    // Directory band bitmap: all DNODEs free except the root's.
    let mut dbm = vec![0u8; BITMAP_BYTES];
    for k in 1..dir_sectors / DNODE_SECTORS {
        dbm[(k / 8) as usize] |= 1 << (k % 8);
    }
    dev.write(dir_bitmap as u64, &dbm)?;

    let mut f = vec![0u8; SECTOR];
    wr32(&mut f, 0, FNODE_MAGIC);
    wr32(&mut f, fnode::UP, root_fnode);
    wr16(&mut f, fnode::EA_OFFSET, fnode::EA_AREA as u16);
    f[fnode::FLAGS] = fnode::DIR;
    set_dir_root_in(&mut f, root_dnode);
    dev.write(root_fnode as u64, &f)?;

    let dotdot = Dirent {
        flags: de::SPECIAL,
        attr: attr::DIRECTORY,
        fnode: root_fnode,
        mtime: p.now,
        size: 0,
        atime: p.now,
        ctime: p.now,
        ea_size: 0,
        acls: 0,
        code_page: 0,
        name: DOTDOT.to_vec(),
        extra: Vec::new(),
        down: None,
    };
    let top = Dnode::new_root(root_dnode, root_fnode, vec![dotdot]);
    dev.write(root_dnode as u64, &top.serialize())?;
    Ok(())
}
