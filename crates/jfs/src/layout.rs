//! On-disk layout of a JFS aggregate (all little-endian).
//!
//! An aggregate is addressed in blocks of `s_bsize` bytes (512..4096).
//! Fixed metadata sits in the first 64 KB: the primary superblock at
//! 32 KB, the aggregate inode map at 36 KB (control page and IAG 0), the
//! aggregate inode table at 44 KB (one 16 KB inode extent) and the
//! secondary superblock at 60 KB. Everything else is found through the
//! aggregate inodes: the block allocation map (BMAP_I), the fileset inode
//! map (FILESYSTEM_I, whose IAGs locate every file's inode), the inline
//! log and the fsck work area (superblock extents at the end of the
//! volume, past the block map).
//!
//! Metadata pages (map pages, xtree and dtree pages) are 4 KB; inodes
//! are 512 bytes, 32 to an extent. See `docs/formats/jfs.md`.

/// Metadata page size.
pub const PSIZE: usize = 4096;
pub const L2PSIZE: u32 = 12;

pub const SUPER1_OFF: u64 = 0x8000;
pub const AIMAP_OFF: u64 = 0x9000;
pub const AITBL_OFF: u64 = 0xb000;
pub const SUPER2_OFF: u64 = 0xf000;
/// Bytes before the first block the format hands out.
pub const AGGR_RSVD_BYTES: u64 = 0x10000;

pub const DISIZE: usize = 512;
pub const INODE_EXTENT_SIZE: usize = 16384;
pub const INOSPEREXT: u32 = 32;
pub const EXTSPERIAG: u32 = 128;
pub const INOSPERIAG: u32 = 4096;
pub const L2INOSPERIAG: u32 = 12;
pub const MAXAG: usize = 128;

/// Aggregate inodes.
pub const AGGREGATE_I: u32 = 1;
pub const BMAP_I: u32 = 2;
pub const LOG_I: u32 = 3;
pub const BADBLOCK_I: u32 = 4;
pub const FILESYSTEM_I: u32 = 16;
/// Fileset inodes: 0, 1 (fileset extension) and 3 (ACL) are reserved.
pub const ROOT_I: u32 = 2;
pub const FIRST_FREE_I: u32 = 4;

pub const MAGIC: &[u8; 4] = b"JFS1";
/// OS/2 JFS accepts version 1 only (Linux writes 2).
pub const VERSION: u32 = 1;

/// Superblock field offsets.
pub mod sb {
    pub const MAGIC: usize = 0;
    pub const VERSION: usize = 4;
    /// Aggregate size (without the fsck area and the inline log) in
    /// device blocks of `PBSIZE` bytes.
    pub const SIZE: usize = 8;
    pub const BSIZE: usize = 16;
    pub const L2BSIZE: usize = 20;
    pub const L2BFACTOR: usize = 22;
    pub const PBSIZE: usize = 24;
    pub const L2PBSIZE: usize = 28;
    pub const AGSIZE: usize = 32;
    pub const FLAG: usize = 36;
    pub const STATE: usize = 40;
    pub const COMPRESS: usize = 44;
    pub const AIT2: usize = 48;
    pub const AIM2: usize = 56;
    pub const LOGDEV: usize = 64;
    pub const LOGSERIAL: usize = 68;
    pub const LOGPXD: usize = 72;
    pub const FSCKPXD: usize = 80;
    pub const TIME: usize = 88;
    pub const FSCKLOGLEN: usize = 96;
    pub const FSCKLOG: usize = 100;
    /// Volume label (11 bytes, the OS/2 label).
    pub const FPACK: usize = 101;
    pub const XSIZE: usize = 112;
    pub const XFSCKPXD: usize = 120;
    pub const XLOGPXD: usize = 128;
    pub const UUID: usize = 136;
    pub const LABEL: usize = 152;
    pub const LOGUUID: usize = 168;
}

/// Superblock flags.
pub mod flag {
    pub const GROUPCOMMIT: u32 = 0x0000_0100;
    pub const INLINELOG: u32 = 0x0000_0800;
    pub const BAD_SAIT: u32 = 0x0001_0000;
    pub const SPARSE: u32 = 0x0002_0000;
    pub const DASD_ENABLED: u32 = 0x0004_0000;
    pub const DASD_PRIME: u32 = 0x0008_0000;
    /// Directory index tables (Linux).
    pub const DIR_INDEX: u32 = 0x0020_0000;
    pub const LINUX: u32 = 0x1000_0000;
    /// Case-insensitive names.
    pub const OS2: u32 = 0x4000_0000;
}

/// Superblock states.
pub mod state {
    pub const CLEAN: u32 = 0;
    pub const MOUNT: u32 = 1;
    pub const DIRTY: u32 = 2;
    pub const LOGREDO: u32 = 3;
}

/// Inode field offsets.
pub mod di {
    pub const INOSTAMP: usize = 0;
    pub const FILESET: usize = 4;
    pub const NUMBER: usize = 8;
    pub const GEN: usize = 12;
    pub const IXPXD: usize = 16;
    pub const SIZE: usize = 24;
    pub const NBLOCKS: usize = 32;
    pub const NLINK: usize = 40;
    pub const UID: usize = 44;
    pub const GID: usize = 48;
    pub const MODE: usize = 52;
    pub const ATIME: usize = 56;
    pub const CTIME: usize = 64;
    pub const MTIME: usize = 72;
    /// Creation time.
    pub const OTIME: usize = 80;
    pub const ACL: usize = 88;
    pub const EA: usize = 104;
    pub const NEXT_INDEX: usize = 120;
    pub const ACLTYPE: usize = 124;
    /// Directory index slots (directories with DIR_INDEX), or the root
    /// of the index table's xtree once it moved out.
    pub const DIRTABLE: usize = 128;
    /// Next generation number (fileset inode map inode).
    pub const GENGEN: usize = 132;
    /// dtree root (directories) or xtree root (everything else).
    pub const ROOT: usize = 224;
    /// Inline EA area: the last 128 bytes, free when the xtree root
    /// holds at most 8 extents.
    pub const INLINEEA: usize = 384;
    pub const INLINEEA_SIZE: usize = 128;
}

/// di_mode bits: Unix type in the low half, OS/2 attributes above.
pub mod mode {
    pub const IFMT: u32 = 0o170000;
    pub const IFDIR: u32 = 0o040000;
    pub const IFREG: u32 = 0o100000;
    pub const IFLNK: u32 = 0o120000;
    pub const IFJOURNAL: u32 = 0x0001_0000;
    pub const ISPARSE: u32 = 0x0002_0000;
    /// The inline EA area is free.
    pub const INLINEEA: u32 = 0x0004_0000;
    /// OS/2 attribute byte shifted to bit 25 (read-only 0x02000000 ...
    /// directory 0x20000000, archive 0x40000000, long name 0x80000000).
    pub const ATTRSHIFT: u32 = 25;
    pub const IDIRECTORY: u32 = 0x2000_0000;
}

/// B+-tree page flags (xtree and dtree).
pub mod bt {
    pub const ROOT: u8 = 0x01;
    pub const LEAF: u8 = 0x02;
    pub const INTERNAL: u8 = 0x04;
    pub const TYPE: u8 = 0x07;
    /// Set in tree roots: the root doubles as a data descriptor.
    pub const DXD_INDEX: u8 = 0x80;
}

/// dxd flags.
pub mod dxd {
    pub const INLINE: u8 = 0x40;
    pub const EXTENT: u8 = 0x20;
}

/// xad flags.
pub mod xad {
    /// Allocated but never written: reads as zeros.
    pub const NOTRECORDED: u8 = 0x08;
}

/// xtree layout.
pub const XTENTRYSTART: usize = 2;
pub const XTROOTINITSLOT: u16 = 10;
pub const XTROOTMAXSLOT: u16 = 18;
pub const XTPAGEMAXSLOT: u16 = 256;
/// Directory index table xtree root in DIRTABLE: 4 extents.
pub const XTROOTINITSLOT_DIR: u16 = 6;

/// dtree layout.
pub const DTSLOTSIZE: usize = 32;
pub const DTROOTMAXSLOT: usize = 9;
/// Name characters in a leaf entry's first slot (13 without a directory
/// index, 11 with one), in an internal entry's (11) and in a
/// continuation slot (15).
pub const DTLHDRDATALEN_LEGACY: usize = 13;
pub const DTLHDRDATALEN: usize = 11;
pub const DTIHDRDATALEN: usize = 11;
pub const DTSLOTDATALEN: usize = 15;
/// Root directory size while the tree fits in the inode.
pub const IDATASIZE: u64 = 256;
/// Directory index entries kept in the inode (indices 2..13).
pub const MAX_INLINE_DIRTABLE_ENTRY: u32 = 13;

/// Block allocation map.
pub const BPERDMAP: u64 = 8192;
pub const L2BPERDMAP: u32 = 13;
pub const LPERDMAP: usize = 256;
pub const LPERCTL: usize = 1024;
pub const L2LPERCTL: u32 = 10;
pub const MAXL0SIZE: u64 = 1 << 23;
pub const MAXL1SIZE: u64 = 1 << 33;
pub const NOFREE: i8 = -1;

/// dmap page offsets.
pub mod dm {
    pub const NBLOCKS: usize = 0;
    pub const NFREE: usize = 4;
    pub const START: usize = 8;
    pub const TREE: usize = 16;
    pub const WMAP: usize = 2048;
    pub const PMAP: usize = 3072;
}

/// Tree headers of dmap and dmapctl pages (relative to the tree).
pub mod tree {
    pub const NLEAFS: usize = 0;
    pub const L2NLEAFS: usize = 4;
    pub const LEAFIDX: usize = 8;
    pub const HEIGHT: usize = 12;
    pub const BUDMIN: usize = 16;
    pub const STREE: usize = 17;
}
pub const DMAP_TREESIZE: usize = 341;
pub const DMAP_LEAFIDX: usize = 85;
pub const CTL_TREESIZE: usize = 1365;
pub const CTL_LEAFIDX: usize = 341;

/// Block map control page (dbmap) offsets.
pub mod bm {
    pub const MAPSIZE: usize = 0;
    pub const NFREE: usize = 8;
    pub const L2NBPERPAGE: usize = 16;
    pub const NUMAG: usize = 20;
    pub const MAXLEVEL: usize = 24;
    pub const MAXAG: usize = 28;
    pub const AGPREF: usize = 32;
    pub const AGLEVEL: usize = 36;
    pub const AGHEIGHT: usize = 40;
    pub const AGWIDTH: usize = 44;
    pub const AGSTART: usize = 48;
    pub const AGL2SIZE: usize = 52;
    pub const AGFREE: usize = 56;
    pub const AGSIZE: usize = 1080;
    pub const MAXFREEBUD: usize = 1088;
}

/// Inode map control page (dinomap) offsets.
pub mod im {
    pub const FREEIAG: usize = 0;
    pub const NEXTIAG: usize = 4;
    pub const NUMINOS: usize = 8;
    pub const NUMFREE: usize = 12;
    pub const NBPERIEXT: usize = 16;
    pub const L2NBPERIEXT: usize = 20;
    pub const DISKBLOCK: usize = 24;
    pub const MAXAG: usize = 28;
    /// Per-AG control: inofree, extfree, numinos, numfree (16 bytes each).
    pub const AGCTL: usize = 2048;
}

/// IAG offsets.
pub mod iag {
    pub const AGSTART: usize = 0;
    pub const IAGNUM: usize = 8;
    pub const INOFREEFWD: usize = 12;
    pub const INOFREEBACK: usize = 16;
    pub const EXTFREEFWD: usize = 20;
    pub const EXTFREEBACK: usize = 24;
    pub const IAGFREE: usize = 28;
    /// Bit set: the extent has no free inode (or is not allocated).
    pub const INOSMAP: usize = 32;
    /// Bit set: the extent is allocated.
    pub const EXTSMAP: usize = 48;
    pub const NFREEINOS: usize = 64;
    pub const NFREEEXTS: usize = 68;
    pub const WMAP: usize = 2048;
    pub const PMAP: usize = 2560;
    pub const INOEXT: usize = 3072;
}

/// Log.
pub const LOGPSIZE: usize = 4096;
pub const LOGMAGIC: u32 = 0x8765_4321;
pub const LOGVERSION: u32 = 1;
pub const LOGPHDRSIZE: usize = 8;
pub const LOGRDSIZE: usize = 36;
pub const LOG_SYNCPT: u16 = 0x4000;
pub mod logsup {
    pub const MAGIC: usize = 0;
    pub const VERSION: usize = 4;
    pub const SERIAL: usize = 8;
    pub const SIZE: usize = 12;
    pub const BSIZE: usize = 16;
    pub const L2BSIZE: usize = 20;
    pub const FLAG: usize = 24;
    pub const STATE: usize = 28;
    pub const END: usize = 32;
    pub const UUID: usize = 36;
    pub const LABEL: usize = 52;
}
/// Log states.
pub const LOGMOUNT: u32 = 0;
pub const LOGREDONE: u32 = 1;

pub fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub fn rd64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

pub fn wr16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn wr32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn wr64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// A physical extent: `len` blocks from block `addr` (len 24 bits,
/// address 40 bits).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pxd {
    pub addr: u64,
    pub len: u32,
}

impl Pxd {
    pub fn new(addr: u64, len: u32) -> Self {
        Pxd { addr, len }
    }

    pub fn read(b: &[u8], o: usize) -> Self {
        let w = rd32(b, o);
        Pxd { len: w & 0xff_ffff, addr: ((w >> 24) as u64) << 32 | rd32(b, o + 4) as u64 }
    }

    pub fn write(&self, b: &mut [u8], o: usize) {
        wr32(b, o, self.len & 0xff_ffff | ((self.addr >> 32) as u32 & 0xff) << 24);
        wr32(b, o + 4, self.addr as u32);
    }

    pub fn end(&self) -> u64 {
        self.addr + self.len as u64
    }
}

/// An xtree entry: file blocks `off..off+len` are at `addr..`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Xad {
    pub flag: u8,
    pub off: u64,
    pub len: u32,
    pub addr: u64,
}

impl Xad {
    pub fn read(b: &[u8], o: usize) -> Self {
        let p = Pxd::read(b, o + 8);
        Xad { flag: b[o], off: (b[o + 3] as u64) << 32 | rd32(b, o + 4) as u64, len: p.len, addr: p.addr }
    }

    pub fn write(&self, b: &mut [u8], o: usize) {
        b[o] = self.flag;
        b[o + 1] = 0;
        b[o + 2] = 0;
        b[o + 3] = (self.off >> 32) as u8;
        wr32(b, o + 4, self.off as u32);
        Pxd::new(self.addr, self.len).write(b, o + 8);
    }

    pub fn end(&self) -> u64 {
        self.off + self.len as u64
    }
}

/// A data descriptor (EAs, ACLs): `size` bytes inline or in an extent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dxd {
    pub flag: u8,
    pub size: u32,
    pub loc: Pxd,
}

impl Dxd {
    pub fn read(b: &[u8], o: usize) -> Self {
        Dxd { flag: b[o], size: rd32(b, o + 4), loc: Pxd::read(b, o + 8) }
    }

    pub fn write(&self, b: &mut [u8], o: usize) {
        b[o] = self.flag;
        b[o + 1..o + 4].fill(0);
        wr32(b, o + 4, self.size);
        self.loc.write(b, o + 8);
    }
}

/// Page index in the block map file of dmap `d`.
pub fn dmap_page(d: u64) -> u64 {
    d + (d >> 10) + (d >> 20) + 4
}

/// Page index of the summary (dmapctl) page `n` of `level` (0..=2).
pub fn ctl_page(level: u32, n: u64) -> u64 {
    match level {
        0 => n * 1024 + n + (n >> 10) + 3,
        1 => (n << 20) + (n << 10) + n + 2,
        _ => 1,
    }
}

/// Map level needed for `blocks` blocks.
pub fn map_level(blocks: u64) -> u32 {
    if blocks <= MAXL0SIZE {
        0
    } else if blocks <= MAXL1SIZE {
        1
    } else {
        2
    }
}

pub fn log2(mut v: u64) -> u32 {
    let mut n = 0;
    while v > 1 {
        v >>= 1;
        n += 1;
    }
    n
}
