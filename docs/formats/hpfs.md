# HPFS (High Performance File System)

HPFS is OS/2's native file system. This document describes the on-disk
layout as implemented by `crates/hpfs` (shared by the kernel driver
`kernel/src/fs/hpfsfs.rs` and `cargo xtask disk --hpfs`). It is written from
published descriptions of the format; no original HPFS volume was available,
so the crate's formatter and unit tests are the reference.

All integers are little-endian. Sectors are 512 bytes and are addressed
relative to the start of the volume (partition). HPFS volumes are at most
64 GB (2^27 sectors). MBR partition type 0x07 is shared with NTFS and exFAT;
an HPFS volume is recognised by the superblock and spareblock signatures.

## Overall layout

| Sectors | Contents |
|--------:|----------|
| 0 | boot sector with a BPB (like FAT; file system name `HPFS    ` at 0x36) |
| 1-15 | boot loader area |
| 16 | superblock |
| 17 | spareblock |
| anywhere | everything else, found through pointers |

Space is managed in *bands* of 0x4000 sectors (8 MB); each band has a 2 KB
free-space bitmap. The rs2 formatter places, after the spareblock: the
hot-fix table (4 sectors) and its spare sectors, the bitmap list, band 0's
bitmap, the bad sector list (4 sectors), the code page directory and data
sectors and the spare DNODEs. The bitmap of every other band sits at the
start of that band (OS/2 itself alternates bitmaps between band ends and
starts so two are adjacent; readers must only follow the bitmap list). The
directory band, preceded by its bitmap and the root F-node, is placed in the
middle band.

## Superblock (sector 16)

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 8 | signature `F995E849`, `FA53E9C5` |
| 0x08 | 1 | version (2) |
| 0x09 | 1 | functional version (2 for volumes up to 4 GB, 3 above) |
| 0x0C | 4 | F-node of the root directory |
| 0x10 | 4 | sectors in the volume |
| 0x14 | 4 | number of bad sectors |
| 0x18 | 4 | first sector of the bitmap list |
| 0x20 | 4 | first sector of the bad sector list |
| 0x28 | 4 | time of last CHKDSK (0 = never) |
| 0x2C | 4 | time of last optimisation |
| 0x30 | 4 | sectors in the directory band |
| 0x34 | 4 | first sector of the directory band |
| 0x38 | 4 | last sector of the directory band |
| 0x3C | 4 | directory band bitmap (4 sectors) |
| 0x40 | 32 | volume name (unused; the label lives in the BPB) |
| 0x60 | 4 | user ID table (8 sectors, HPFS386; 0 = none) |

## Spareblock (sector 17)

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 8 | signature `F9911849`, `FA5229C5` |
| 0x08 | 1 | flags: 0x01 dirty (not cleanly unmounted), 0x02 spare DNODEs used, 0x04 hot fixes used, 0x08 bad sector, 0x10 bad bitmap, 0x20 fast format, 0x40 written by an old version |
| 0x09 | 1 | more flags (HPFS386) |
| 0x0C | 4 | hot-fix table (4 sectors) |
| 0x10 | 4 | hot fixes in use |
| 0x14 | 4 | hot-fix table size (entries) |
| 0x18 | 4 | spare DNODEs still free |
| 0x1C | 4 | spare DNODEs |
| 0x20 | 4 | code page directory sector |
| 0x24 | 4 | number of code pages |
| 0x28 | 8 | checksums of superblock and spareblock (HPFS386) |
| 0x6C | 400 | up to 100 spare DNODE pointers |

The hot-fix table holds three arrays of *size* entries: bad sectors, their
replacements and the owning F-nodes. The first *in use* pairs remap I/O:
rs2 sends every access to a bad sector to its replacement. rs2 never adds hot
fixes and does not change the dirty flag.

## Bitmaps

The bitmap list is an array of sector numbers, one per band, each pointing
to a 4-sector (2048 byte, 16384 bit) bitmap. Bit *n* (byte *n*/8, bit *n*%8
counted from the least significant) describes sector *band* × 0x4000 + *n*;
**1 means free**. Bits past the end of the volume are 0. All file system
structures, including the whole directory band, are marked in use.

The directory band is a region reserved for DNODEs. Its own 2 KB bitmap
has one bit per DNODE (4 sectors), 1 = free. DNODEs are taken from the band
first and from normal space (4-sector aligned) when it is full. Free space
reported to applications is the count of free bits in the band bitmaps.

## Time stamps

Times are 32-bit seconds since 1970-01-01 00:00, in local time (OS/2 keeps
the hardware clock in local time and does no zone conversion).

## F-node (one sector)

Every file and directory has an F-node.

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 4 | signature `F7E40AAE` |
| 0x04 | 8 | access history (unused) |
| 0x0C | 1 | length of the name |
| 0x0D | 15 | first 15 bytes of the name |
| 0x1C | 4 | F-node of the containing directory (the root: itself) |
| 0x20 | 4 | external ACL size |
| 0x24 | 4 | external ACL sector |
| 0x28 | 2 | internal ACL size |
| 0x2A | 1 | external ACL is an ALSEC tree |
| 0x2C | 4 | external EA size |
| 0x30 | 4 | external EA sector (run or ALSEC) |
| 0x34 | 2 | internal EA size |
| 0x36 | 1 | 0x01: external EAs are described by an ALSEC tree |
| 0x37 | 1 | 0x01: directory |
| 0x38 | 8 | allocation B+ tree header |
| 0x40 | 96 | 8 extents or 12 subtree pointers |
| 0xA0 | 4 | file size in bytes |
| 0xA4 | 4 | number of EAs with the NEEDEA flag |
| 0xA8 | 16 | user ID (unused) |
| 0xB8 | 2 | offset of the internal EAs in the F-node (0xC4) |
| 0xBA | 10 | DASD limits (HPFS386) |
| 0xC4 | 316 | internal EAs, then internal ACLs |

For a directory the allocation tree has a single leaf entry whose disk
sector is the directory's top DNODE (file sector field -1, length 0).

## Allocation B+ trees

A file's data is a list of *extents* (file sector, length, disk sector)
kept in a B+ tree rooted in the F-node. Header (8 bytes):

| Offset | Size | Field |
|-------:|-----:|-------|
| 0 | 1 | flags: 0x80 entries are internal nodes, 0x20 parent is an F-node |
| 4 | 1 | free entries |
| 5 | 1 | used entries |
| 6 | 2 | offset of the first free entry from the header start |

Leaf entries are 12 bytes: first file sector, number of sectors, first disk
sector. Internal entries are 8 bytes: a file sector bound and an ALSEC; the
subtree holds the sectors below the bound (and at or above the previous
entry's). The last internal entry's bound is -1.

An ALSEC (allocation sector) continues the tree:

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 4 | signature `37E40AAE` |
| 0x04 | 4 | this sector |
| 0x08 | 4 | parent (ALSEC or F-node) |
| 0x0C | 8 | B+ tree header |
| 0x14 | 480 | 40 extents or 60 subtree pointers |

HPFS has no holes: extents cover file sectors 0 to ceil(size/512) without
gaps. rs2 allocates exactly that much; it rebuilds the tree from the extent
list whenever the list changes (filling F-node leaves, then ALSEC leaves of
40, then levels of 60-way internal ALSECs until at most 12 remain for the
F-node), reusing the old ALSEC sectors.

## Directories

A directory is a B-tree of DNODEs. A DNODE is 4 sectors (2048 bytes):

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 4 | signature `77E40AAE` |
| 0x04 | 4 | offset of the first free byte |
| 0x08 | 1 | 0x01: top DNODE of the directory; other bits: change counters |
| 0x0C | 4 | parent: the directory's F-node (top DNODE) or the parent DNODE |
| 0x10 | 4 | this sector |
| 0x14 | | directory entries |

Directory entry (DIRENT), 4-byte aligned:

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 2 | length of the entry |
| 0x02 | 1 | flags: 0x01 the special `..` entry, 0x02 ACL, 0x04 down pointer present, 0x08 END entry, 0x10 has EAs, 0x20 extended permissions, 0x40 explicit ACL, 0x80 has NEEDEA EAs |
| 0x03 | 1 | attributes: 0x01 read-only, 0x02 hidden, 0x04 system, 0x10 directory, 0x20 archive, 0x40 name is not a valid 8.3 name |
| 0x04 | 4 | F-node |
| 0x08 | 4 | last write time |
| 0x0C | 4 | file size |
| 0x10 | 4 | last access time |
| 0x14 | 4 | creation time |
| 0x18 | 4 | size of the EAs |
| 0x1C | 1 | number of ACLs (low 3 bits) |
| 0x1D | 1 | code page index of the name |
| 0x1E | 1 | name length |
| 0x1F | n | name |
| | 4 | B-tree down pointer (last 4 bytes of the entry, if flag 0x04) |

The length is `align4(31 + name length) + 4 if down`.

Entries in a DNODE are sorted by name; the last entry is the END entry
(name `FF`, flag 0x08). If an entry has a down pointer, the DNODE it points
to holds the names that sort between the previous entry and this one; the
END entry's down pointer covers names after the last entry. Within a DNODE
either all entries (END included) have down pointers or none do, and all
leaves are at the same depth. Reading a directory in order is an in-order
walk.

Every directory starts with a special entry named `01 01` (flag 0x01,
directory attribute) whose F-node is the directory's own. In the root it
also carries the root directory's time stamps.

rs2 maintains the tree as follows:

* **Insert**: descend to the leaf where the name belongs and insert. A DNODE
  that overflows is split around the entry that best balances the bytes on
  both sides (each side keeps at least one entry): the entries before it
  move to a new DNODE whose END entry takes over its down pointer, and it
  moves up into the parent pointing to the new DNODE. When the top DNODE
  splits, a new top DNODE is allocated and the F-node updated.
* **Delete**: an entry in a leaf is removed. An entry in an internal DNODE
  is replaced by its predecessor (the last entry of the rightmost leaf of
  its left subtree). A non-top DNODE left without entries is merged away:
  the separator above it moves down into the right neighbour (or, if it was
  the parent's END entry, the previous separator moves into the left
  neighbour), carrying the empty DNODE's remaining down pointer. A top DNODE
  left with only a down pointer is replaced by its child. Overflows caused
  by these moves are split as on insert.
* `up` pointers are updated whenever entries with down pointers move.

## Names and code pages

Names are up to 254 bytes in the volume's code page, case preserved.
Directories are ordered by comparing names byte by byte after upper-casing
(the end of the shorter name first), so lookups are case-insensitive.
Upper-casing uses ASCII rules below 0x80 and the volume's code page table
above.

The spareblock points to a code page directory sector (`494521F7`, count at
0x04, 16-byte entries from 0x10: index, code page number, DBCS bounds, data
sector, index within the data sector). A code page data sector (`894521F7`,
count at 0x04, DBCS bounds at 0x08, three 16-bit offsets at 0x14) holds up
to three tables at those offsets: index, code page number, 2 unused bytes,
then 128 bytes giving the upper case of characters 0x80-0xFF. rs2 uses the
first table; its formatter writes code page 850 by default (437 also has a
built-in table).

## Extended attributes

A file's EAs are an FEA list: per EA a flags byte (0x80 NEEDEA), name
length, 16-bit value length, the name, a NUL and the value. The list is
split between the F-node (from the offset at 0xB8, internal size at 0x34)
and an external area (size at 0x2C, sector at 0x30: a contiguous run, or
an ALSEC tree if the byte at 0x36 is set). A large value may be stored
outside the list: EA flag 0x01 means the 8-byte value holds the length and
first sector of the real value, flag 0x02 that this sector is an ALSEC
tree. The DIRENT records the total EA size.

rs2 reads EAs (`Volume::read_eas`; the kernel uses them for the FEA2LIST
size reported with file information) and frees all EA and ACL storage when
a file or directory is deleted; it never writes EAs, and keeps F-node EA
areas untouched when it rewrites an F-node.
