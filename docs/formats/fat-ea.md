# Extended attributes on FAT

FAT directory entries have no room for OS/2 extended attributes (EAs), so
OS/2 keeps them in a hidden file. This document describes the format as
implemented by `crates/fat/src/ea.rs` (used by the kernel's FAT driver and
by CHKDSK's checker). It was worked out from public descriptions of the
format and checked against the `EA DATA. SF` files on the Warp 4.52 boot
diskettes, which rs2 reads and updates without complaint from its own
checker (`os2_floppy_eas` test). IBM's CHKDSK could not be run against
rs2-written volumes because `UFAT.DLL` is not on the available media.

All integers are little-endian.

## FAT12 and FAT16: `EA DATA. SF`

### The EA handle

Every directory entry (files and directories alike) may refer to an *EA
set* through a 16-bit **EA handle** stored at offset 0x14 of the 32-byte
entry, the word that FAT32 uses for the high half of the first cluster.
Handle 0 means "no EAs". The root directory has no entry and therefore no
EAs. `.` and `..` entries carry handle 0.

### The file

All EA sets of a volume live in one file in the root directory named
`EA DATA. SF` (8.3 form `EA DATA ` + ` SF`, a name ordinary programs cannot
type). OS/2 gives it the attributes read-only, hidden, system and archive
(0x27), so `DIR` does not list it (`ATTRIB` does) and it cannot be deleted.
The file is created when the first EA is written and stays (possibly with
no sets) afterwards.

The file is addressed in **clusters of the volume** ("cluster *n*" below
means the *n*-th cluster of the file, at byte offset *n* × cluster size).
Its size is always a whole number of clusters.

```
cluster 0 ..            header (512 bytes), handle tables, padding
cluster base[0] ..      EA sets of handles 0-127
cluster base[1] ..      EA sets of handles 128-255
...
```

### Header (512 bytes)

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 2 | signature `ED` (0x4445) |
| 0x02 | 2 | format type (0) |
| 0x04 | 18 | recovery log: log type, then three (cluster, new value) pairs, then a handle and its new offset. All zero when no update is pending; rs2 always writes zeros. |
| 0x16 | 10 | reserved (0) |
| 0x20 | 480 | **base table**: 240 16-bit cluster numbers, one per group of 128 handles |

### Handle tables

From offset 0x200 to the end of the header area follow the handle tables:
one 16-bit entry per handle, 128 per group (256 bytes per table). The
header area ends at cluster `base[0]` (the first EA set of group 0 starts
there), so the number of tables is `(base[0] × cluster size − 512) / 256`,
at most 240 (30,720 handles). With 1 KB clusters, as on the boot diskettes,
cluster 0 holds the header and two tables (256 handles).

A table entry is `0xFFFF` if the handle is unused; otherwise it is the
cluster of the handle's EA set **relative to the base of its group**:

```
cluster of set(h) = base[h / 128] + (table[h] & 0x7FFF)
```

Handle 0 is never used. rs2 masks the top bit when reading and never sets
it.

### Layout of the sets

The sets of each group are stored together, groups in increasing order, so
the base table is non-decreasing and a group ends where the next non-empty
group begins. Bases of groups that hold no sets point at the end of the
previous group (on a fresh file every base equals `base[0]`).

Because positions are counted in clusters, sets can be inserted and removed
without copying data: the file's cluster chain is relinked. rs2 does the
following:

* **Add a set** of *n* clusters for handle *h* in group *g*: insert *n*
  new clusters into the chain at the end of group *g*, set
  `table[h]` = that position − `base[g]`, and add *n* to the bases of the
  later groups.
* **Remove a set**: unlink its clusters from the chain, subtract their
  number from the offsets of later sets in the same group and from the
  bases of later groups, and mark the handle unused.
* **Replace a set**: overwrite it in place if it needs as many clusters as
  before, otherwise remove and add it again (the handle is kept, so the
  directory entry does not change).
* **More handles**: when every handle in the tables is in use, insert one
  cluster after the header area and add 1 to every base; the tables grow
  into it (new entries `0xFFFF`).

### An EA set

Each set starts on a cluster boundary and occupies whole clusters:

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 2 | signature `EA` (0x4145) |
| 0x02 | 2 | the owning handle (must equal the handle that points here) |
| 0x04 | 4 | number of EAs with the NEEDEA flag |
| 0x08 | 14 | 8.3 name of the owner (`NAME.EXT`, NUL-padded; informational, updated on rename) |
| 0x16 | 4 | reserved (0) |
| 0x1A | 4 | cbList: size of the FEA list that starts here, including this field |
| 0x1E | | the FEAs |

An FEA (the OS/2 1.x format, also used inside HPFS) is:

| Offset | Size | Field |
|-------:|-----:|-------|
| 0 | 1 | flags: 0x80 = NEEDEA (critical EA) |
| 1 | 1 | name length *n* |
| 2 | 2 | value length *v* |
| 4 | *n* | name (upper case) |
| 4+*n* | 1 | NUL |
| 5+*n* | *v* | value |

FEAs follow each other without padding. A file's EAs are limited to 64 KB
(cbList ≤ 0xFFFF).

### Consistency rules (checked by CHKDSK)

* Every non-zero handle in a directory entry is in use, and its set has the
  `EA` signature and the same owner handle; otherwise the reference is
  removed.
* No two entries share a handle (the later one loses the reference).
* Every handle in use is referenced by some entry; orphaned sets are freed.
* Deleting a file or directory frees its set; renaming keeps the handle.

## FAT32: per-file EA files (FAT32.IFS)

The FAT32 directory entry uses offset 0x14 for the high half of the first
cluster, so the scheme above does not fit. OS/2 itself never had a FAT32
driver; the de facto standard is the Netlabs FAT32.IFS used by eComStation
and ArcaOS, and rs2 follows it so that disks can be exchanged:

* The EAs of a file or directory `NAME` are stored in a file named
  `NAME EA. SF` (the full, possibly long, name plus the suffix ` EA. SF`) in
  the same directory. Its contents are a plain FEA list: cbList followed by
  the FEAs as above.
* Byte 0x0C of the owner's directory entry (the byte Windows NT uses for
  lower-case flags 0x08 and 0x10) records the state: 0x40 = the file has
  EAs, 0x80 = it has critical (NEEDEA) EAs, 0 = none. Readers look for the
  EA file only when one of these bits is set.
* The EA file name is a long (VFAT) name; rs2 creates long-name entries for
  it with a generated `NAME~N.EXT` alias, and gives it the hidden and system
  attributes. The OS/2 directory listing APIs do not report EA files.
* Renaming or moving the owner moves the EA file; deleting the owner
  deletes it. CHKDSK reports EA files whose owner is gone (and deletes them
  with /F) and clears the flag of entries whose EA file is missing.

The details of FAT32.IFS were reconstructed from memory of its public
sources, not from a disk it wrote; the flag values in particular could not
be verified here.
