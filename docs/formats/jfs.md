# JFS (Journaled File System)

JFS is IBM's journaled file system, shipped with OS/2 Warp Server for
e-business and Warp 4.5x (`JFS.IFS`, `UJFS.DLL` for FORMAT and CHKDSK).
This document describes the on-disk layout as implemented by `crates/jfs`
(shared by the kernel driver `kernel/src/fs/jfsfs.rs`, FORMAT, CHKDSK and
the installer).

Sources: IBM's `JFS.IFS` and `UJFS.DLL` studied in Ghidra with their debug
symbols (format routines, the mount checks, CHKDSK's `verify_*` and
`validate_super`, the case table), the published layout of the open-source
JFS for Linux, and the jfsutils 1.1.15 sources for jfs_fsck's directory
index rules. The crate's output is checked with jfs_fsck 1.1.15 and the
Linux kernel driver (see "Verification" below).

All integers are little-endian. Addresses are in aggregate blocks of
`s_bsize` bytes (512, 1024, 2048 or 4096; rs2 formats 4096) from the start
of the partition. Metadata pages are 4 KB (`PSIZE`); inodes are 512 bytes.

## Extents

| Name | Size | Layout |
|------|-----:|--------|
| pxd | 8 | bits 0-23 length, bits 24-31 address bits 32-39; then address bits 0-31 |
| xad | 16 | flag (0x08 = allocated, not written: reads zeros), 2 reserved, offset bits 32-39, offset bits 0-31 (in blocks), pxd |
| dxd | 16 | flag (0x40 inline, 0x20 extent, 0x80 B+-tree root), 3 reserved, size in bytes, pxd |

## Overall layout

| Offset | Contents |
|-------:|----------|
| 0 | boot sector: a BPB, serial at 0x27, label at 0x2B, `JFS     ` at 0x36 |
| 32 KB | primary superblock |
| 36 KB | aggregate inode map: control page, IAG 0 |
| 44 KB | aggregate inode table: one 16 KB extent of 32 inodes |
| 60 KB | secondary superblock |
| 64 KB | (rs2's and IBM's format) the block map file, then the secondary aggregate inode map and table (8 KB apart, as IBM's mount checks), the fileset inode map (control page, IAG 0) and the fileset's first inode extent |
| `mapsize` | fsck work area: `((mapsize + len + 32767) / 32768 + 51)` pages, `len` its own length (IBM's CHKDSK checks this formula), the last 50 pages the fsck service log |
| after it | inline log, to the end of the partition |

The block map covers blocks 0 to `mapsize - 1`; the fsck area and the
log lie past it. The superblock's size is `mapsize` in device sectors.

### Superblock

| Offset | Field |
|-------:|-------|
| 0 | `JFS1` |
| 4 | version: 1 (OS/2 accepts only 1; Linux writes 2 unless asked) |
| 8 | size of the mapped aggregate in device sectors |
| 16 | block size; 20 log2; 22 log2(block / sector) |
| 24 | device sector size; 28 log2 |
| 32 | allocation group size in blocks |
| 36 | flags: 0x100 group commit, 0x800 inline log, 0x10000 bad secondary table, 0x20000 sparse files, 0x40000 directory limits (DASD), 0x200000 directory index tables, 0x10000000 Linux, 0x40000000 OS/2 (case-insensitive names). OS/2 requires 0x40000900 |
| 40 | state: 0 clean, 1 mounted, 2 dirty, 3 log replay failed |
| 48 | secondary aggregate inode table (pxd), 56 its map (pxd) |
| 64 | log device, 68 log serial |
| 72 | inline log (pxd), 80 fsck work area (pxd) |
| 88 | time written (seconds, nanoseconds) |
| 96 | fsck service log length (blocks), 100 which half is current |
| 101 | volume label, 11 bytes (what OS/2 shows) |
| 112 | extended size, 120 and 128 new fsck area and log (extendfs) |
| 136 | UUID, 152 16-byte label, 168 log UUID |

### Inodes

| Offset | Field |
|-------:|-------|
| 0 | stamp (equal in every inode of the aggregate) |
| 4 | fileset: 1 for aggregate inodes, 16 for files and directories |
| 8 | number, 12 generation |
| 16 | the inode's extent (pxd) |
| 24 | size, 32 blocks used (data, tree pages, EA and ACL extents) |
| 40 | links: 1 for files, 2 + subdirectories for directories |
| 44 | uid, 48 gid |
| 52 | mode: Unix type and permissions in the low 16 bits; 0x10000 journaled, 0x20000 sparse, 0x40000 inline EA area free; the OS/2 attributes shifted left by 25 (read-only 0x02000000, hidden, system, 0x20000000 directory, archive, 0x80000000 long name) |
| 56 | access, 64 change, 72 modification, 80 creation time (seconds, nanoseconds) |
| 88 | ACL (dxd), 104 EAs (dxd) |
| 120 | next directory index, 124 ACL type |
| 128 | directories: index slots (12 × 8 bytes) or the index table's xtree root; the fileset inode map's inode: 132 next generation number |
| 224 | directories: dtree root (288 bytes); others: xtree root (288 bytes) |
| 384 | inline EAs (128 bytes) while the xtree root holds at most 8 extents |

OS/2 creates files as `IFREG | INLINEEA` plus attributes (no permission
bits) and directories as `IFDIR | IFJOURNAL | IDIRECTORY`; the root
directory is `0x200141C0` (IBM's CHKDSK checks that value).

Aggregate inodes: 0 reserved, 1 the aggregate inode map, 2 the block map,
3 the log (empty xtree), 4 bad blocks (empty, `ISPARSE`), 16 the fileset
inode map; each `IFREG | IFJOURNAL`, generation 1, one link, xtree root
with 18 slots. Fileset inodes 0, 1 and 3 are reserved (same shape), 2 is the
root directory, files start at 4.

## Maps

### Inode maps

A map file of 4 KB pages: a control page (free IAG list, number of IAGs,
inodes backed and free, blocks per inode extent, and from 2048 per
allocation group the heads of the lists of IAGs with free inodes and with
free extent slots plus the group's counts), then one IAG per 4096 inodes:

| Offset | Field |
|-------:|-------|
| 0 | start of the allocation group the IAG serves |
| 8 | IAG number |
| 12, 16 | free-inode list forward and back (−1 ends) |
| 20, 24 | free-extent list forward and back |
| 28 | free IAG list |
| 32 | per extent, a bit set when it has no free inode (or is not allocated) |
| 48 | per extent, a bit set when allocated |
| 64 | free inodes, 68 free extent slots |
| 2048 | working map, 2560 persistent map (a bit per inode, first in the high bit) |
| 3072 | 128 inode extent addresses (pxd) |

Inode *n* is IAG `n / 4096`, extent `(n / 32) % 128`, slot `n % 32`. An IAG
is on its group's free-inode list while it has free inodes, on the
free-extent list while it has between 1 and 127 free extent slots, and on
the free IAG list while it has no extents. rs2 adds extents as needed and
never gives them back.

### Block map

The BMAP_I file: page 0 the control page, then summary pages of levels 2,
1 and 0 and the dmaps, interleaved (dmap *d* is page
`d + d/1024 + d/2^20 + 4`; level-0 page *n* is `1025n + n/1024 + 3`;
level-1 page *n* `2^20 n + 1024n + n + 2`; level 2 page 1). Only levels up
to `maxlevel` (0 below 2^23 blocks, 1 below 2^33) are kept up to date.

A dmap covers 8192 blocks: count, free count, first block, a summary tree,
then at 2048 the working and at 3072 the persistent bitmap (one bit per
block, the first block in the high bit of each 32-bit word). The summary
tree has 256 leaves (the largest free buddy run in each word, log2: 5 for an
empty word, −1 for a full one) combined as buddies (two equal neighbours of
value *v* become *v+1* and −1) and 4-way maxima above. A summary page does
the same over 1024 children's roots, with the minimum buddy size
13 + 10 × level. The stored leaves are the combined ones; rs2 expands them
before changing one.

Control page: map size, free blocks, log2 blocks per page, number of
allocation groups, `maxlevel`, highest active group (jfs_fsck: the last with
a block in use), preferred group, the group's level, height, width and start
in the summary tree, log2 group size, free blocks per group (128 × 8 bytes
from 56), group size, and the top tree's root. The group size is 8192
blocks below 2^20 blocks, else the volume rounded up to a power of two
divided by 128.

## xtrees

A file's data, and every map file, is a B+-tree of xads keyed by file
block. The root in the inode has a 32-byte header (next, previous, flag,
next free slot, maximum slots) and 16-byte slots; entries start at slot 2.
A regular file's root starts with 10 slots (8 extents), leaving the inline
EA area free; it grows to 18 while that area is unused. Other pages are
4 KB with 254 entries; an internal entry points at the page mapping from its
offset on. rs2 rewrites a file's tree packed after each change (reusing its
pages).

## dtrees

A directory is a B+-tree of names. The root in the inode is 9 slots of 32
bytes, slot 0 the header (16 bytes of directory limits, flag, number of
entries, free slots, first free slot, the parent's inode, 8 bytes of sorted
index); 4 KB pages have a 32-byte header (next, previous, flag, entries,
free slots, first free slot, slot count, index table slot, own pxd), the
sorted index (`stbl`, 4 slots) and 123 slots for entries.

- Leaf entry: inode, next slot, name length, then 13 UCS-2 characters
  (11 and a 4-byte index number on volumes with index tables).
- Internal entry: child pxd, next slot, name length, 11 characters of the
  router key.
- Continuation slot: next slot, a count byte of 1, 15 characters.

Names are compared case-insensitively on OS/2 volumes: each UCS-2
character upper-cased with IBM's table (a delta table for U+0000-U+01FF,
ranges for Greek, Cyrillic, U+1E00-U+1FFC and full-width Latin; in
`crates/jfs/src/upcase.rs`), then by length. A router key is the shortest
prefix of the right page's first name that sorts after the left page's last
one, upper-cased; the first entry of the root and of the leftmost page of
each internal level has an empty key. A directory's size is 256 while the
tree fits the inode, else 4096 per page.

### Directory index tables (Linux)

With superblock flag 0x200000 every leaf entry has an index number
(from 2) and the directory keeps a slot per number: reserved byte, flag
(1 valid), the entry's position in its page's sorted index, page address
(0 for the root). Indices 2-13 sit in the inode at 128; then the table moves
to pages mapped by an xtree whose root (4 extents) replaces them, index *i*
at byte `(i − 2) × 8`. A freed slot keeps in its address the index of the
entry that followed (readdir resumes there), −1 at the end. The size is
`(next − 2) × 8` while inline (1 for a new directory), else the table's
bytes. OS/2 volumes have none of this.

## Extended attributes

An OS/2 FEA list: total size (4 bytes, included), then per EA flags (0x80
NEEDEA), name length, value length (2 bytes), name, NUL, value, unpadded.
Inline in the inode's last 128 bytes (dxd flag 0x40) when it fits and the
xtree root leaves room (regular files only), else in one extent of blocks
(0x20). The EAs of a file are at most 64 KB.

## Log

The inline log is pages of 4 KB. Page 1 holds the log superblock: magic
0x87654321, version 1, serial, size in pages, page size and log2, flags,
state (0 in use, 1 redone/clean), end of the last record, UUID, label and
the UUIDs of the file systems using it. Each log page starts with its
sequence number and end-of-records offset and ends with the same.
Formatting writes a sync point record in page 2 (numbered `pages − 3`) and
numbers the following pages 0, 1, ... so the log's end is found as after a
wrap.

## What rs2 does with the log

rs2 writes metadata in place, synchronously at the end of each change, and
writes no log records. It marks the superblock mounted (state 1) at the
first change and clean again at DosShutdown (the kernel's
`FileSystem::shutdown`) or `Volume::unmount`. On mount:

- clean volumes, and volumes marked mounted whose log is clean (rs2's own,
  after a power cut), are opened read/write;
- volumes whose log is still in use (another system stopped without
  shutting the log down) or that are marked dirty are opened read-only:
  rs2 does not replay logs. Run CHKDSK on OS/2 (JFS autocheck) or jfs_fsck
  on Linux first.

## Verification

Checked with jfs_fsck 1.1.15 (`-n`, read only, no messages) and the Linux
6.18 kernel driver, in a Linux VM:

- volumes formatted and filled by the crate (4 KB and 512-byte blocks,
  64 MB to 40 GB, 2000-entry directories, 4000 random creates and deletes,
  fragmented files with xtree pages, inline and extent EAs);
- volumes made by Linux `mkfs.jfs -O` (OS/2 style) and `mkfs.jfs` (with
  index tables), changed by the crate, then changed again by Linux;
- disks written by rs2 itself in QEMU (CMD COPY/XCOPY/REN, apitest's EA
  tests, FORMAT /FS:JFS, a system booted from a JFS partition).

Linux mounts only 4 KB-block volumes. Volumes formatted by IBM's own
FORMAT have not been compared byte for byte: IBM's FORMAT.COM and CHKDSK do
not run under rs2 yet (UJFS.DLL's 16-bit entry crashes in the 16-to-32-bit
thunk), so IBM's layout rules come from its code instead.
