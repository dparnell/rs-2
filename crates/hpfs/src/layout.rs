//! On-disk constants and field offsets (see docs/formats/hpfs.md).

pub const SECTOR: usize = 512;

pub const SUPER_LBA: u32 = 16;
pub const SPARE_LBA: u32 = 17;

pub const SUPER_MAGIC: (u32, u32) = (0xf995_e849, 0xfa53_e9c5);
pub const SPARE_MAGIC: (u32, u32) = (0xf991_1849, 0xfa52_29c5);
pub const FNODE_MAGIC: u32 = 0xf7e4_0aae;
pub const ALSEC_MAGIC: u32 = 0x37e4_0aae;
pub const DNODE_MAGIC: u32 = 0x77e4_0aae;
pub const CPDIR_MAGIC: u32 = 0x4945_21f7;
pub const CPDATA_MAGIC: u32 = 0x8945_21f7;

/// Sectors per bitmap band (one 2 KB bitmap each).
pub const BAND: u32 = 0x4000;
/// Size of a band bitmap in bytes (and the directory band bitmap).
pub const BITMAP_BYTES: usize = 2048;
pub const DNODE_SECTORS: u32 = 4;
pub const DNODE_BYTES: usize = 2048;

/// Superblock fields.
pub mod sb {
    pub const VERSION: usize = 0x08;
    pub const FUNC_VERSION: usize = 0x09;
    pub const ROOT: usize = 0x0c;
    pub const SECTORS: usize = 0x10;
    pub const BAD_SECTORS: usize = 0x14;
    pub const BITMAPS: usize = 0x18;
    pub const BAD_LIST: usize = 0x20;
    pub const CHKDSK_TIME: usize = 0x28;
    pub const OPTIMIZE_TIME: usize = 0x2c;
    pub const DIR_BAND_SECTORS: usize = 0x30;
    pub const DIR_BAND_START: usize = 0x34;
    pub const DIR_BAND_END: usize = 0x38;
    pub const DIR_BAND_BITMAP: usize = 0x3c;
}

/// Spareblock fields.
pub mod sp {
    pub const FLAGS: usize = 0x08;
    pub const HOTFIX_LIST: usize = 0x0c;
    pub const HOTFIX_USED: usize = 0x10;
    pub const HOTFIX_COUNT: usize = 0x14;
    pub const SPARE_DNODES_FREE: usize = 0x18;
    pub const SPARE_DNODES: usize = 0x1c;
    pub const CODE_PAGE_DIR: usize = 0x20;
    pub const CODE_PAGES: usize = 0x24;
    pub const SPARE_DNODE_LIST: usize = 0x6c;
    /// Room for spare DNODE pointers in the spareblock.
    pub const MAX_SPARE_DNODES: usize = 100;
}

/// F-node fields.
pub mod fnode {
    pub const NAME_LEN: usize = 0x0c;
    pub const NAME: usize = 0x0d;
    pub const NAME_MAX: usize = 15;
    pub const UP: usize = 0x1c;
    pub const ACL_EXT_SIZE: usize = 0x20;
    pub const ACL_SECTOR: usize = 0x24;
    pub const ACL_INT_SIZE: usize = 0x28;
    pub const ACL_IN_ANODE: usize = 0x2a;
    pub const EA_EXT_SIZE: usize = 0x2c;
    pub const EA_SECTOR: usize = 0x30;
    pub const EA_INT_SIZE: usize = 0x34;
    pub const EA_IN_ANODE: usize = 0x36;
    pub const FLAGS: usize = 0x37;
    pub const BTREE: usize = 0x38;
    pub const FILE_SIZE: usize = 0xa0;
    pub const NEED_EAS: usize = 0xa4;
    pub const EA_OFFSET: usize = 0xb8;
    /// Start of the F-node's internal EA/ACL area.
    pub const EA_AREA: usize = 0xc4;
    /// F-node flag: this is a directory.
    pub const DIR: u8 = 0x01;
}

/// Allocation sector (ALSEC) fields.
pub mod alsec {
    pub const SELF: usize = 0x04;
    pub const UP: usize = 0x08;
    pub const BTREE: usize = 0x0c;
}

/// B+ tree header (in F-nodes and ALSECs).
pub mod bt {
    pub const FLAGS: usize = 0;
    pub const FREE: usize = 4;
    pub const USED: usize = 5;
    pub const FIRST_FREE: usize = 6;
    pub const ENTRIES: usize = 8;
    /// Entries are internal nodes (file sector bound, child ALSEC).
    pub const INTERNAL: u8 = 0x80;
    /// The parent of this ALSEC is an F-node.
    pub const FNODE_PARENT: u8 = 0x20;
    pub const LEAF_SIZE: usize = 12;
    pub const NODE_SIZE: usize = 8;
    pub const FNODE_LEAVES: usize = 8;
    pub const FNODE_NODES: usize = 12;
    pub const ALSEC_LEAVES: usize = 40;
    pub const ALSEC_NODES: usize = 60;
}

/// DNODE header.
pub mod dn {
    pub const FIRST_FREE: usize = 0x04;
    pub const FLAGS: usize = 0x08;
    pub const UP: usize = 0x0c;
    pub const SELF: usize = 0x10;
    pub const ENTRIES: usize = 0x14;
    /// Flag: top DNODE of a directory (its `up` is the F-node).
    pub const ROOT: u8 = 0x01;
}

/// Directory entry (DIRENT) fields and flags.
pub mod de {
    pub const LEN: usize = 0x00;
    pub const FLAGS: usize = 0x02;
    pub const ATTR: usize = 0x03;
    pub const FNODE: usize = 0x04;
    pub const MTIME: usize = 0x08;
    pub const SIZE: usize = 0x0c;
    pub const ATIME: usize = 0x10;
    pub const CTIME: usize = 0x14;
    pub const EA_SIZE: usize = 0x18;
    pub const ACLS: usize = 0x1c;
    pub const CODE_PAGE: usize = 0x1d;
    pub const NAME_LEN: usize = 0x1e;
    pub const NAME: usize = 0x1f;

    /// The special `..` entry that starts every directory.
    pub const SPECIAL: u8 = 0x01;
    pub const ACL: u8 = 0x02;
    /// A B-tree down pointer ends the entry.
    pub const DOWN: u8 = 0x04;
    /// The end entry of a DNODE.
    pub const END: u8 = 0x08;
    pub const HAS_EA: u8 = 0x10;
    pub const NEED_EA: u8 = 0x80;
}

pub fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn wr16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn wr32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn align4(n: usize) -> usize {
    (n + 3) & !3
}
