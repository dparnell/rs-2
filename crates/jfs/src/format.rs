//! Creating an empty JFS volume, laid out as OS/2's FORMAT /FS:JFS does:
//!
//! ```text
//! block 0          boot sector (BPB, "JFS     ")
//! 32 KB            primary superblock
//! 36 KB            aggregate inode map (control page, IAG 0)
//! 44 KB            aggregate inode table (inodes 0-31)
//! 60 KB            secondary superblock
//! 64 KB            block allocation map (BMAP_I)
//!                  secondary aggregate inode map, then table (8 KB apart)
//!                  fileset inode map (FILESYSTEM_I: control page, IAG 0)
//!                  fileset inode extent 0 (inodes 0-31; 2 = root)
//!   ...            free space
//! mapsize          fsck work area ((size + 32767) / 32768 + 51 pages)
//!                  inline log
//! ```
//!
//! Aggregate inodes 0-4 and 16 and fileset inodes 0-3 are in use; the
//! special ones are `IFREG | IFJOURNAL` with generation 1 and one link,
//! the root directory `IFDIR | IFJOURNAL | IDIRECTORY | 0700`, as IBM's
//! CHKDSK expects.

use alloc::vec;
use alloc::vec::Vec;

use crate::bmap::{ag_hints, ctl_rebuild, dmap_rebuild, new_dmap};
use crate::layout::*;
use crate::{BlockDevice, Error, Result};

/// Parameters for [`format`].
pub struct FormatParams<'a> {
    /// Volume label (up to 11 characters, code page bytes).
    pub label: &'a str,
    pub serial: u32,
    /// Sectors before the volume (partition offset), recorded in the BPB.
    pub hidden_sectors: u32,
    /// Time stamp for the root directory (seconds since 1970).
    pub now: u32,
    /// Block size: 512, 1024, 2048 or 4096 (0: 4096).
    pub block_size: u32,
    /// Inline log size in bytes (0: about 0.4% of the volume).
    pub log_size: u64,
}

impl Default for FormatParams<'_> {
    fn default() -> Self {
        FormatParams { label: "", serial: 0, hidden_sectors: 0, now: 0, block_size: 0, log_size: 0 }
    }
}

/// Smallest volume accepted (16 MB).
pub const MIN_BYTES: u64 = 16 << 20;

/// What the format decided (for tests and FORMAT's report).
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub bsize: u32,
    pub mapsize: u64,
    pub bmap: Pxd,
    pub aim2: Pxd,
    pub ait2: Pxd,
    pub fsimap: Pxd,
    pub fsiext: Pxd,
    pub fsck: Pxd,
    pub log: Pxd,
    pub agl2size: u32,
}

/// Default inline log size: 0.4% of the volume, 256 KB to 32 MB, in
/// whole 16 KB units.
fn default_log(total: u64) -> u64 {
    (total / 250).clamp(256 << 10, 32 << 20) & !0x3fff
}

/// log2 of the allocation group size for a map of `blocks`.
pub(crate) fn agl2size(blocks: u64) -> u32 {
    if blocks < BPERDMAP * MAXAG as u64 {
        return L2BPERDMAP;
    }
    let mut l2 = 63 - blocks.leading_zeros();
    if 1u64 << l2 < blocks {
        l2 += 1;
    }
    l2 - 7
}

pub(crate) fn geometry(total_bytes: u64, ss: u32, p: &FormatParams) -> Result<Geometry> {
    let bsize = if p.block_size == 0 { 4096 } else { p.block_size };
    if !bsize.is_power_of_two() || !(512..=4096).contains(&bsize) || bsize < ss || total_bytes < MIN_BYTES {
        return Err(Error::InvalidName);
    }
    let l2 = log2(bsize as u64);
    let nb = (PSIZE as u32) >> l2;
    let total = total_bytes >> l2;
    let logb = (if p.log_size == 0 { default_log(total_bytes) } else { p.log_size.next_multiple_of(PSIZE as u64) }) >> l2;
    if logb < 32 * nb as u64 || logb as u64 * 4 > total {
        return Err(Error::InvalidName);
    }
    let log = Pxd::new((total - logb) / nb as u64 * nb as u64, logb as u32);
    // The fsck area ends where the log starts; its length depends on
    // that end (IBM's CHKDSK checks the same formula).
    let fsck_len = ((log.addr + 0x7fff) / 0x8000 + 0x33) * nb as u64;
    let mapsize = log.addr - fsck_len;
    let fsck = Pxd::new(mapsize, fsck_len as u32);
    let ndmaps = mapsize.div_ceil(BPERDMAP);
    let bpages = dmap_page(ndmaps - 1) + 1;
    let bmap = Pxd::new(AGGR_RSVD_BYTES >> l2, (bpages * nb as u64) as u32);
    let ipe = (INODE_EXTENT_SIZE >> l2) as u32;
    let aim2 = Pxd::new(bmap.end(), 2 * nb);
    let ait2 = Pxd::new(aim2.end(), ipe);
    let fsimap = Pxd::new(ait2.end(), 2 * nb);
    let fsiext = Pxd::new(fsimap.end(), ipe);
    if fsiext.end() + 64 > mapsize {
        return Err(Error::DiskFull);
    }
    Ok(Geometry { bsize, mapsize, bmap, aim2, ait2, fsimap, fsiext, fsck, log, agl2size: agl2size(mapsize) })
}

struct Writer<'a, D: BlockDevice> {
    dev: &'a D,
    ss: u64,
}

impl<D: BlockDevice> Writer<'_, D> {
    fn put(&self, off: u64, data: &[u8]) -> Result<()> {
        self.dev.write(off / self.ss, data)
    }
}

/// An aggregate or fileset inode with the common fields.
fn inode(stamp: u32, fileset: u32, ino: u32, ext: Pxd, mode: u32, now: u32) -> Vec<u8> {
    let mut b = vec![0u8; DISIZE];
    wr32(&mut b, di::INOSTAMP, stamp);
    wr32(&mut b, di::FILESET, fileset);
    wr32(&mut b, di::NUMBER, ino);
    ext.write(&mut b, di::IXPXD);
    if mode != 0 {
        wr32(&mut b, di::GEN, 1);
        wr32(&mut b, di::NLINK, 1);
        wr32(&mut b, di::MODE, mode);
        for t in [di::ATIME, di::CTIME, di::MTIME, di::OTIME] {
            wr32(&mut b, t, now);
        }
    }
    b
}

/// Give an inode a leaf xtree root holding `xads`.
fn xtree(b: &mut [u8], xads: &[Xad], maxentry: u16, bsize: u32) {
    let r = di::ROOT;
    b[r + 16] = bt::DXD_INDEX | bt::ROOT | bt::LEAF;
    wr16(b, r + 18, (XTENTRYSTART + xads.len()) as u16);
    wr16(b, r + 20, maxentry);
    let mut blocks = 0u64;
    for (k, x) in xads.iter().enumerate() {
        x.write(b, r + (XTENTRYSTART + k) * 16);
        blocks += x.len as u64;
    }
    wr64(b, di::SIZE, blocks * bsize as u64);
    wr64(b, di::NBLOCKS, blocks);
}

/// An inode map: control page and IAG 0 with one extent at `ext`, the
/// inodes in `used` (bit mask over the extent) allocated.
fn inode_map(ext: Pxd, used: u32, nb_iext: u32, agl2size: u32) -> Vec<u8> {
    let mut m = vec![0u8; 2 * PSIZE];
    let n = used.count_ones();
    {
        let c = &mut m[..PSIZE];
        wr32(c, im::FREEIAG, u32::MAX);
        wr32(c, im::NEXTIAG, 1);
        wr32(c, im::NUMINOS, INOSPEREXT);
        wr32(c, im::NUMFREE, INOSPEREXT - n);
        wr32(c, im::NBPERIEXT, nb_iext);
        wr32(c, im::L2NBPERIEXT, log2(nb_iext as u64));
        let ag = (ext.addr >> agl2size) as usize;
        for a in 0..MAXAG {
            let o = im::AGCTL + 16 * a;
            if a == ag {
                wr32(c, o, 0);
                wr32(c, o + 4, 0);
                wr32(c, o + 8, INOSPEREXT);
                wr32(c, o + 12, INOSPEREXT - n);
            } else {
                wr32(c, o, u32::MAX);
                wr32(c, o + 4, u32::MAX);
            }
        }
    }
    let p = &mut m[PSIZE..];
    wr64(p, iag::AGSTART, (ext.addr >> agl2size) << agl2size);
    wr32(p, iag::IAGNUM, 0);
    for f in [iag::INOFREEFWD, iag::INOFREEBACK, iag::EXTFREEFWD, iag::EXTFREEBACK, iag::IAGFREE] {
        wr32(p, f, u32::MAX);
    }
    // Extent 0 allocated with free inodes; 1-127 not allocated.
    wr32(p, iag::INOSMAP, 0x7fff_ffff);
    for k in 1..4 {
        wr32(p, iag::INOSMAP + 4 * k, u32::MAX);
    }
    wr32(p, iag::EXTSMAP, 0x8000_0000);
    wr32(p, iag::NFREEINOS, INOSPEREXT - n);
    wr32(p, iag::NFREEEXTS, EXTSPERIAG - 1);
    wr32(p, iag::WMAP, used);
    wr32(p, iag::PMAP, used);
    ext.write(p, iag::INOEXT);
    m
}

/// Format `dev` as an empty JFS volume.
pub fn format<D: BlockDevice>(dev: &D, p: &FormatParams) -> Result<Geometry> {
    let ss = dev.sector_size() as u32;
    if !ss.is_power_of_two() || !(512..=4096).contains(&ss) {
        return Err(Error::Io);
    }
    let g = geometry(dev.sector_count() * ss as u64, ss, p)?;
    let w = Writer { dev, ss: ss as u64 };
    let bsize = g.bsize;
    let l2 = log2(bsize as u64);
    let nb = (PSIZE as u32) >> l2;
    let ipe = (INODE_EXTENT_SIZE >> l2) as u32;
    let stamp = p.serial ^ p.now.rotate_left(13) ^ 0x4a46_5331;
    let now = p.now;

    // Clear the fixed area (boot sector to the secondary superblock).
    w.put(0, &vec![0u8; AGGR_RSVD_BYTES as usize])?;

    // ---- Block allocation map ----
    let mapsize = g.mapsize;
    let ndmaps = mapsize.div_ceil(BPERDMAP);
    let level = map_level(mapsize);
    let mut dmaps: Vec<Vec<u8>> = (0..ndmaps)
        .map(|d| new_dmap(d * BPERDMAP, (mapsize - d * BPERDMAP).min(BPERDMAP) as u32))
        .collect();
    let used = |start: u64, n: u64, dm: &mut Vec<Vec<u8>>| {
        for b in start..start + n {
            let pg = &mut dm[(b >> L2BPERDMAP) as usize];
            let o = (b & (BPERDMAP - 1)) as usize;
            let off = dm::WMAP + 4 * (o / 32);
            let v = rd32(pg, off) | 0x8000_0000 >> (o % 32);
            wr32(pg, off, v);
            wr32(pg, dm::PMAP + 4 * (o / 32), v);
        }
    };
    used(0, AGGR_RSVD_BYTES >> l2, &mut dmaps);
    for x in [g.bmap, g.aim2, g.ait2, g.fsimap, g.fsiext] {
        used(x.addr, x.len as u64, &mut dmaps);
    }
    let roots: Vec<i8> = dmaps.iter_mut().map(|d| dmap_rebuild(d)).collect();
    let nfree: u64 = dmaps.iter().map(|d| rd32(d, dm::NFREE) as u64).sum();
    let mut pages: Vec<(u64, Vec<u8>)> = Vec::new();
    // Summary pages, bottom up, for the levels in use.
    let mut values = roots;
    for lv in 0..=level {
        let n = values.len().div_ceil(LPERCTL);
        let mut up = Vec::with_capacity(n);
        for k in 0..n {
            let mut c = vec![0u8; PSIZE];
            for j in 0..LPERCTL {
                c[tree::STREE + CTL_LEAFIDX + j] = values.get(k * LPERCTL + j).copied().unwrap_or(NOFREE) as u8;
            }
            up.push(ctl_rebuild(&mut c, lv));
            pages.push((ctl_page(lv, k as u64), c));
        }
        values = up;
    }
    let maxfreebud = values[0];
    for (d, page) in dmaps.into_iter().enumerate() {
        pages.push((dmap_page(d as u64), page));
    }
    // Control page.
    let agl2 = g.agl2size;
    let agsize = 1u64 << agl2;
    let numag = mapsize.div_ceil(agsize) as u32;
    let mut c = vec![0u8; PSIZE];
    wr64(&mut c, bm::MAPSIZE, mapsize);
    wr64(&mut c, bm::NFREE, nfree);
    wr32(&mut c, bm::L2NBPERPAGE, log2(nb as u64));
    wr32(&mut c, bm::NUMAG, numag);
    wr32(&mut c, bm::MAXLEVEL, level);
    let aglevel = map_level(agsize);
    let l2nl = agl2 - (L2BPERDMAP + aglevel * L2LPERCTL);
    let agheight = l2nl >> 1;
    let agwidth = 1u32 << (l2nl - 2 * agheight);
    let agstart = (0..5u32.saturating_sub(agheight)).fold(0u32, |s, i| s + (1 << (2 * i)));
    wr32(&mut c, bm::AGLEVEL, aglevel);
    wr32(&mut c, bm::AGHEIGHT, agheight);
    wr32(&mut c, bm::AGWIDTH, agwidth);
    wr32(&mut c, bm::AGSTART, agstart);
    wr32(&mut c, bm::AGL2SIZE, agl2);
    let mut agfree = vec![0u64; MAXAG];
    for a in 0..numag as u64 {
        let (s, e) = (a * agsize, ((a + 1) * agsize).min(mapsize));
        for (pg, data) in pages.iter().filter(|(pg, _)| *pg >= 4) {
            let _ = pg;
            let start = rd64(data, dm::START);
            let n = rd32(data, dm::NBLOCKS) as u64;
            if n == 0 || rd32(data, tree::NLEAFS) == LPERCTL as u32 || start + n <= s || start >= e {
                continue;
            }
            for b in start.max(s)..(start + n).min(e) {
                let o = (b - start) as usize;
                if rd32(data, dm::WMAP + 4 * (o / 32)) & (0x8000_0000 >> (o % 32)) == 0 {
                    agfree[a as usize] += 1;
                }
            }
        }
        wr64(&mut c, bm::AGFREE + 8 * a as usize, agfree[a as usize]);
    }
    let (maxag, agpref) = ag_hints(&agfree, numag, agsize, mapsize, nfree, 0);
    wr32(&mut c, bm::MAXAG, maxag);
    wr32(&mut c, bm::AGPREF, agpref);
    wr64(&mut c, bm::AGSIZE, agsize);
    c[bm::MAXFREEBUD] = maxfreebud as u8;
    pages.push((0, c));
    // Write the whole map file (pages for unused levels stay zero).
    let bpages = g.bmap.len as u64 / nb as u64;
    let mut file = vec![0u8; (bpages as usize) * PSIZE];
    for (pg, data) in &pages {
        file[*pg as usize * PSIZE..(*pg as usize + 1) * PSIZE].copy_from_slice(data);
    }
    for (k, chunk) in file.chunks(64 * PSIZE).enumerate() {
        w.put((g.bmap.addr << l2) + (k * 64 * PSIZE) as u64, chunk)?;
    }

    // ---- Aggregate inode table and map (primary and secondary) ----
    let ait1 = Pxd::new(AITBL_OFF >> l2, ipe);
    let aim1 = Pxd::new(AIMAP_OFF >> l2, 2 * nb);
    let special = mode::IFREG | mode::IFJOURNAL;
    for (ait, aim) in [(ait1, aim1), (g.ait2, g.aim2)] {
        let mut t = vec![0u8; INODE_EXTENT_SIZE];
        for k in 0..INOSPEREXT {
            let m = match k {
                AGGREGATE_I | BMAP_I | LOG_I | FILESYSTEM_I => special,
                BADBLOCK_I => special | mode::ISPARSE,
                _ => 0,
            };
            let mut b = inode(stamp, AGGREGATE_I, k, ait, m, now);
            match k {
                AGGREGATE_I => xtree(&mut b, &[Xad { flag: 0, off: 0, len: aim.len, addr: aim.addr }], XTROOTMAXSLOT, bsize),
                BMAP_I => xtree(&mut b, &[Xad { flag: 0, off: 0, len: g.bmap.len, addr: g.bmap.addr }], XTROOTMAXSLOT, bsize),
                LOG_I | BADBLOCK_I => xtree(&mut b, &[], XTROOTMAXSLOT, bsize),
                FILESYSTEM_I => {
                    xtree(&mut b, &[Xad { flag: 0, off: 0, len: g.fsimap.len, addr: g.fsimap.addr }], XTROOTMAXSLOT, bsize);
                    // Next generation number for fileset inodes.
                    wr32(&mut b, di::GENGEN, 1);
                }
                _ => {}
            }
            t[k as usize * DISIZE..(k as usize + 1) * DISIZE].copy_from_slice(&b);
        }
        w.put(ait.addr << l2, &t)?;
        // Inodes 0-4 and 16 in use.
        w.put(aim.addr << l2, &inode_map(ait, 0xf800_8000, ipe, agl2))?;
    }

    // ---- Fileset: inode map and the first inode extent ----
    w.put(g.fsimap.addr << l2, &inode_map(g.fsiext, 0xf000_0000, ipe, agl2))?;
    let mut t = vec![0u8; INODE_EXTENT_SIZE];
    for k in 0..INOSPEREXT {
        let m = match k {
            1 | 3 => special,
            ROOT_I => mode::IFDIR | mode::IFJOURNAL | mode::IDIRECTORY | 0o700,
            _ => 0,
        };
        let mut b = inode(stamp, FILESYSTEM_I, k, g.fsiext, m, now);
        match k {
            1 | 3 => xtree(&mut b, &[], XTROOTMAXSLOT, bsize),
            ROOT_I => {
                wr32(&mut b, di::NLINK, 2);
                let r = di::ROOT;
                b[r + 16] = bt::DXD_INDEX | bt::ROOT | bt::LEAF;
                b[r + 18] = 8;
                b[r + 19] = 1;
                wr32(&mut b, r + 20, ROOT_I);
                for s in 1..DTROOTMAXSLOT {
                    b[r + s * DTSLOTSIZE] = if s + 1 < DTROOTMAXSLOT { (s + 1) as u8 } else { 0xff };
                    b[r + s * DTSLOTSIZE + 1] = 1;
                }
                wr64(&mut b, di::SIZE, IDATASIZE);
            }
            _ => {}
        }
        t[k as usize * DISIZE..(k as usize + 1) * DISIZE].copy_from_slice(&b);
    }
    w.put(g.fsiext.addr << l2, &t)?;

    // ---- fsck work area and inline log ----
    let zero = vec![0u8; 64 * PSIZE];
    let fbytes = (g.fsck.len as u64) << l2;
    let mut o = 0;
    while o < fbytes {
        let n = (fbytes - o).min(zero.len() as u64) as usize;
        w.put((g.fsck.addr << l2) + o, &zero[..n])?;
        o += n as u64;
    }
    write_log(&w, g.log, l2, p.label)?;

    // ---- Superblocks and boot sector ----
    let mut s = vec![0u8; PSIZE];
    s[0..4].copy_from_slice(MAGIC);
    wr32(&mut s, sb::VERSION, VERSION);
    wr64(&mut s, sb::SIZE, (mapsize << l2) / ss as u64);
    wr32(&mut s, sb::BSIZE, bsize);
    wr16(&mut s, sb::L2BSIZE, l2 as u16);
    wr16(&mut s, sb::L2BFACTOR, log2((bsize / ss) as u64) as u16);
    wr32(&mut s, sb::PBSIZE, ss);
    wr16(&mut s, sb::L2PBSIZE, log2(ss as u64) as u16);
    wr32(&mut s, sb::AGSIZE, agsize as u32);
    wr32(&mut s, sb::FLAG, flag::OS2 | flag::GROUPCOMMIT | flag::INLINELOG);
    wr32(&mut s, sb::STATE, state::CLEAN);
    g.ait2.write(&mut s, sb::AIT2);
    g.aim2.write(&mut s, sb::AIM2);
    g.log.write(&mut s, sb::LOGPXD);
    g.fsck.write(&mut s, sb::FSCKPXD);
    wr32(&mut s, sb::TIME, now);
    wr32(&mut s, sb::FSCKLOGLEN, 50 * nb);
    let label: Vec<u8> = p.label.chars().take(11).map(|c| u8::try_from(c as u32).unwrap_or(b'_')).collect();
    s[sb::FPACK..sb::FPACK + label.len()].copy_from_slice(&label);
    s[sb::LABEL..sb::LABEL + label.len()].copy_from_slice(&label);
    // A version 4 UUID from the serial and time.
    let mut u = [0u8; 16];
    let mut x = (p.serial as u64) << 32 | now as u64 | 1;
    for b in u.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    u[6] = u[6] & 0x0f | 0x40;
    u[8] = u[8] & 0x3f | 0x80;
    s[sb::UUID..sb::UUID + 16].copy_from_slice(&u);
    let n = (ss as usize).max(512).min(PSIZE);
    w.put(SUPER1_OFF, &s[..n])?;
    w.put(SUPER2_OFF, &s[..n])?;
    w.put(0, &boot_sector(dev.sector_count(), ss, bsize, p, &label))?;
    Ok(g)
}

/// The inline log: superblock in page 1, page 2 holding a sync point,
/// then empty pages numbered so the log's end is found after a wrap.
fn write_log<D: BlockDevice>(w: &Writer<D>, log: Pxd, l2: u32, label: &str) -> Result<()> {
    let base = log.addr << l2;
    let npages = ((log.len as u64) << l2) / LOGPSIZE as u64;
    let mut sup = vec![0u8; 2 * LOGPSIZE];
    {
        let s = &mut sup[LOGPSIZE..];
        wr32(s, logsup::MAGIC, LOGMAGIC);
        wr32(s, logsup::VERSION, LOGVERSION);
        wr32(s, logsup::SERIAL, 0);
        wr32(s, logsup::SIZE, npages as u32);
        wr32(s, logsup::BSIZE, LOGPSIZE as u32);
        wr32(s, logsup::L2BSIZE, 12);
        wr32(s, logsup::FLAG, flag::OS2 | flag::GROUPCOMMIT | flag::INLINELOG);
        wr32(s, logsup::STATE, LOGREDONE);
        wr32(s, logsup::END, (2 * LOGPSIZE + LOGPHDRSIZE + LOGRDSIZE) as u32);
        for (k, c) in label.bytes().take(16).enumerate() {
            s[logsup::LABEL + k] = c;
        }
    }
    w.put(base, &sup)?;
    let page = |num: u32, eor: u16| {
        let mut b = vec![0u8; LOGPSIZE];
        wr32(&mut b, 0, num);
        wr16(&mut b, 6, eor);
        wr32(&mut b, LOGPSIZE - 4 - 4, num);
        wr16(&mut b, LOGPSIZE - 2, eor);
        b
    };
    // Page 2: the sync point record (lrd after the page header).
    let mut p2 = page(npages as u32 - 3, (LOGPHDRSIZE + LOGRDSIZE) as u16);
    wr16(&mut p2, LOGPHDRSIZE + 8, LOG_SYNCPT);
    w.put(base + 2 * LOGPSIZE as u64, &p2)?;
    let mut k = 3u64;
    while k < npages {
        let n = (npages - k).min(64);
        let mut buf = Vec::with_capacity(n as usize * LOGPSIZE);
        for j in 0..n {
            buf.extend_from_slice(&page((k + j - 3) as u32, LOGPHDRSIZE as u16));
        }
        w.put(base + k * LOGPSIZE as u64, &buf)?;
        k += n;
    }
    Ok(())
}

fn boot_sector(sectors: u64, ss: u32, bsize: u32, p: &FormatParams, label: &[u8]) -> Vec<u8> {
    let mut bs = vec![0u8; ss as usize];
    // jmp to an endless loop: this volume is not bootable.
    bs[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    bs[0x3e..0x40].copy_from_slice(&[0xeb, 0xfe]);
    bs[3..11].copy_from_slice(b"IBM 4.50");
    wr16(&mut bs, 11, ss as u16);
    bs[13] = (bsize / ss) as u8;
    bs[21] = 0xf8;
    if sectors < 0x10000 {
        wr16(&mut bs, 19, sectors as u16);
    } else {
        wr32(&mut bs, 32, sectors.min(u32::MAX as u64) as u32);
    }
    wr16(&mut bs, 24, 63);
    wr16(&mut bs, 26, 255);
    wr32(&mut bs, 28, p.hidden_sectors);
    bs[0x24] = 0x80;
    bs[0x26] = 0x29;
    wr32(&mut bs, 0x27, p.serial);
    bs[0x2b..0x36].fill(b' ');
    bs[0x2b..0x2b + label.len()].copy_from_slice(label);
    bs[0x36..0x3e].copy_from_slice(b"JFS     ");
    bs[510] = 0x55;
    bs[511] = 0xaa;
    bs
}
