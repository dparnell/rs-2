# rs2's base utilities

rs2 ships its own versions of the OS/2 Warp 4.52 command-line utilities
(`userland/utils`, installed in `\OS2` by `cargo xtask` from
`USER_PROGRAMS`). They take IBM's options, show IBM's messages by their
OSO001.MSG numbers (built-in English text when the message file lacks
them; `utils::msgs`), lay out their output as IBM's do and return the
same codes. Where a file format is shared with IBM's tools it is IBM's,
both ways.

| Program | What it does | Notes |
|---------|--------------|-------|
| ANSI | Shows or switches ANSI escape processing | VioGetAnsi/VioSetAnsi |
| ATTRIB, FIND, MORE, SORT, TREE, XCOPY, LABEL, MODE, KEYB, HELP | (earlier) | |
| BACKUP, RESTORE | Backup sets in `\BACKUP` of the target drive | IBM's `BKUPJFS` format (`crates/os2-backup`, below); file and directory EAs; /S /M /A /D /T /L, RESTORE /S /P /B /A /E /L /M /N /D. IBM's RESTORE reads rs2's sets and rs2's RESTORE reads IBM's (checked in QEMU). |
| BLDLEVEL | Shows a module's `@#vendor:revision#@description` signature | Module description of LX/NE files (revision shown as numbers, as IBM's); other files are searched |
| CACHE | Block cache parameters; `-STATS:RESET`, `-STATS:SHOW` report on the log | DosFSCtl 0x9001-0x9003, 0x9f00-0x9f01 |
| CHKDSK | Checks a volume; /F repairs | FAT (earlier); HPFS: bitmaps and directory band bitmap against what the tree owns, leaks freed outside the fixed structures, dirty flag cleared (`hpfs::Volume::check`); JFS: block map, block and link counts, parents, inode map counts, orphans into `FOUND.nnn`, inline log marked redone so a volume another system left busy becomes writable (`jfs::Volume::repair`). /P for PMCHKDSK. |
| COMP | Compares files byte by byte | Wildcards, ten differences per pair, Y/N questions as IBM's |
| DISKCOPY, DISKCOMP | Whole diskettes | rs2 has no diskette drives: either side may be a diskette image file (`DISKCOPY A: DISK1.IMG`), a drive any volume of diskette size; hard disks are refused (SYS1231) |
| EAUTIL | Splits EAs into a hold file (/S) and joins them back (/J) | Hold files are 16-bit FEALISTs, as IBM's; /R /O /M /P; default hold file `EAS\name` |
| EJECT | Ejects removable media | CD-ROM category 0x80/0x44, else category 8/0x40 |
| FORMAT | (earlier) | /P for PMFORMAT (below); also installed as FORMAT.COM |
| HELPMSG | HELP's message help under IBM's name | (HELP.CMD calls it) |
| LVM, FDISK | Partitions and volumes | IBM's LVM command line (/QUERY, /CREATE:PARTITION and COMPATIBILITY volumes, /DELETE, /SETNAME, /SETSTARTABLE, /HIDE, /NEWMBR) plus `/SETLETTER`, and an `LVM>` prompt without arguments. MBR disks with OS/2 LVM drive letter tables, GPT disks (no letters). IBM's LVM reads what rs2's writes. |
| PACK, PACK2, UNPACK, UNPACK2 | FTCOMP packed files (the installation media format) | PACK writes LZ-coded blocks without the entropy stage (IBM's UNPACK reads them); /H /D /T /C /A /R /L; UNPACK /SHOW /N /F, EAs restored |
| PATCH | Patches files | Interactive, or /A with a control file (`FILE`, `VER`, `CHA` lines) |
| PRINT | Prints files to LPTn, cancels jobs | Through the kernel's LPT device into the spooler; /C and /T through PMSPL |
| PSTAT | Processes, threads, semaphores, shared memory, libraries | DosQuerySysState; /C /S /L /M /P /A |
| RMVIEW | The hardware as the resource manager knows it: adapters, devices, drivers, logical devices, the hardware tree; the IRQs, I/O ports, DMA channels and memory in use with their owners | RESMGR$ (WPS.md, "The Hardware Manager"); /P /D /L /HW /IRQ /IO /IOA /DMA /MEM |
| RECOVER | Saves the readable part of a damaged file | Whole-drive recovery is CHKDSK /F's |
| REPLACE | Replaces or adds files | /A /P /R /S /U /W /F, DosCopy (EAs kept) |
| SETBOOT | Start-up settings and restart | rs2's UEFI loader: `/T:x` sets `bootwait` in `\RS2\CMDLINE`, `/Q` shows it, `/B` and `/IBD:d` restart; Boot Manager options answer as IBM's SETBOOT without Boot Manager |
| SYSLEVEL | Lists SYSLEVEL.xxx files | Searches the hard disks (or a directory) |
| UNDELETE | Deleted files kept by DELDIR | rs2's file systems keep no deleted files yet: reports the missing DELDIR (SYS3194) or that there is nothing to undelete |

`RS2\UTILTEST.EXE` runs the utilities as child programs and checks their
results (31 checks); boot it with a test drive from `cargo xtask disk`:

```text
cargo xtask disk -o work/t1.img
cargo xtask run --headless --hdd work/t1.img --init "RS2\UTILTEST.EXE"
```

Host tests cover the formats: `cargo test -p os2-backup -p os2-pack -p lvm
-p hpfs -p jfs`.

## Backup sets (`crates/os2-backup`)

`\BACKUP\CONTROL.nnn` (one per backup disk, 001 for a hard disk) and
`BACKUP.nnn`: the data file holds each file's EAs (a 16-bit FEALIST)
followed by its data. The control file is a 139-byte header
(`8B "BKUPJFS "`, disk number, 0xFF on the last disk) and then per
directory a record (u16 length, u16 file count, u64 offset of the next
directory record or all ones, `D4`, u64 offset and u64 length of the
directory's EAs, u16 path length, the path without drive and leading `\`)
followed by its file records (u16 length, flags 3, u64 size, u16 disk,
u64 data offset, u64 part length, u16 attributes, u16 time, u16 date,
u64 EA offset, u64 EA length, u16 name length, the name). Appending (/A)
adds directory records. The backup date is the control file's date.

## LVM drive letter tables (`crates/lvm`)

The DLA table of a partition table is the last sector of its first track
(LBA 62 for the MBR, EBR + 62): signatures 0x424D5202 0x44464D50, a
CRC-32 (reflected, start 0xFFFFFFFF, no final inversion) at +8, disk
serial, boot disk serial, install flags, geometry, disk name (20 bytes)
and four 60-byte entries (volume serial, partition serial, size, start,
Boot Manager and installable flags, drive letter, volume and partition
names). Volumes whose entry has a letter get that letter when rs2 starts
(`kernel/src/drivers`): also unformatted ones, so FORMAT can format a
new volume; letters no entry asks for go to the other volumes in order.

## FORMAT /P and CHKDSK /P

PMFORMAT and PMCHKDSK run `FORMAT.COM d: /P ...` and `CHKDSK.COM d: /P`
with their standard handles on pipes and read lines `MSG_<n>,<parameter>`
(OSO001.MSG numbers), ignoring others. PMFORMAT (studied in Ghidra) shows
1312/538 as the percentage, 1294/635 as 100 %, 1283 and 1286 as total and
available bytes (1286 is the last line it waits for), answers 1271 and
1309 with `Y` and 1288 (volume label) with Enter, and shows its own text
for errors (1276, 1279, 1281, 1303, 1313, 1319, 19, 123, ...).

## Not (yet) rs2's own

From IBM's `\OS2`: XDFCOPY and VMDISK (XDF diskette images and the XDF
filter: no diskette drives), CACHEJFS, DEFRAGFS, EXTENDFS and CLRBBLKS
(JFS tools), EXECMODE, PSFILES, PSSEMS, STRACE, MAKEINI, SPOOL, SYSLOG,
LD2FIX, ARCINST/ARCRECOV (desktop archives), APMDAEMN, the
removable-media drivers' FCHECK2/FFORMAT2/ATAMNT2/AUTOUTL2, and
CHKDSK32/JFSCHK32 (IBM's file-system modules behind CHKDSK). PM programs,
REXX tools and network clients are covered elsewhere.
