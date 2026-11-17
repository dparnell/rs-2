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
| RC | The resource compiler: `.RC` scripts to `.RES` files, resources bound into LX and NE modules | `crates/os2-rc`, below. `rc file.rc [module]`, `rc -r file.rc [file.res]`, `rc file.res [module]` (no module: the `.RES` name with `.EXE`); -d, -i, -r, -p, -x[1\|2], -cp, -cc, -n, -w2, -h; INCLUDE. The return code is the number of errors. |
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
-p hpfs -p jfs -p os2-rc`.

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

## The resource compiler (`crates/os2-rc`)

RC.EXE replaces Warp's `\OS2\RC.EXE` (the 16-bit RC 4.00.011, the same
compiler as the Toolkit's RC16). The language is the Toolkit's (Tools
Reference, "Resource Compiler"): a C preprocessor (`#define` with
function-like macros, `#` and `##`, `#if`/`#elif` with `defined`, `#include`
of which only the directives count, as IBM's, and `RCINCLUDE`, which takes a
whole script), constant expressions (casts such as `(PSZ)` skipped, `NOT`
clearing default style bits) and every statement: ICON, POINTER, BITMAP,
FONT, DEFAULTICON, DLGINCLUDE, RESOURCE, RCDATA, MENU (SUBMENU, MENUITEM,
SEPARATOR, PRESPARAMS), ACCELTABLE, STRINGTABLE, MESSAGETABLE,
DLGTEMPLATE/WINDOWTEMPLATE with DIALOG, FRAME, WINDOW, CONTROL and the
control statements, PRESPARAMS, CTLDATA, HELPTABLE, HELPSUBTABLE
(SUBITEMSIZE), ASSOCTABLE and CODEPAGE. It also takes what IBM's takes
without complaint: missing commas between fields, a trailing `|` or comma
before the next statement, `;` after values.

What the output follows (IBM's, where the Toolkit book says otherwise):

- `.RES`: per resource `FF type`, `FF id` (or the name and a NUL), the
  flags word, a u32 size, the data; in script order, string and message
  bundles (16 strings each, ID / 16 + 1) at the end in the order of their
  first string. Default flags MOVEABLE PURE DISCARDABLE (0x1030);
  ACCELTABLE and RCDATA 0x0030; bitmaps over 4K or named in quotes lose
  DISCARDABLE unless it is given; FIXED clears MOVEABLE and DISCARDABLE.
  Icon 1 is also written as resource 22.22 (the default icon).
- Strings are C strings: they end at `\0`, CR characters are dropped,
  `""` is a quote, a line end inside quotes is a newline. RCDATA strings get
  their NUL, RESOURCE data strings do not; numbers are 2 bytes, 4 with an
  L anywhere in the expression.
- Dialog templates: a 14-byte header (size, 0, code page, 14, 1, -1, 0),
  30-byte items in tree order, then per item its class name, text (an
  ordinal text is `FF id`), control data and presentation parameters.
  DIALOG and FRAME always carry the frame creation flags as 4 bytes of
  control data; DIALOG adds WS_SAVEBITS WS_CLIPSIBLINGS and FS_DLGBORDER
  (unless a border style or FCF_BORDER/FCF_SIZEBORDER is given), FRAME
  WS_VISIBLE. Control statements default to WS_VISIBLE and their class's
  bits: PUSHBUTTON WS_TABSTOP, CHECKBOX BS_CHECKBOX WS_TABSTOP, LTEXT
  SS_TEXT WS_GROUP, GROUPBOX SS_GROUPBOX WS_GROUP, COMBOBOX CBS_SIMPLE
  WS_TABSTOP (a given style replaces CBS_SIMPLE), MLE MLS_BORDER WS_TABSTOP,
  SPINBUTTON, CONTAINER, VALUESET, NOTEBOOK and SLIDER WS_TABSTOP only.
- Menus: size, version (1 with PRESPARAMS), code page, 4, count, the
  items (style, attribute, ID, text, a submenu's own template). SEPARATOR
  is MIS_SEPARATOR, MIA_DISABLED, ID 0xFFFF; `id,` with nothing after the
  comma gives style 0.
- HELPITEM is (window, subtable, 0xFFFF, extended panel), the table ends
  with 0; a HELPSUBTABLE starts with its item size in words.

Binding: LX modules lose their trailing resource objects and pages; the
resources go into new objects (PRELOAD ones, other MOVEABLE ones, FIXED
ones), sorted by type and ID, 4-byte aligned, pages in whole page-shift
units after the module's own pages; the loader and fixup sections are
rebuilt and the data pages, non-resident names and debug information
follow them. Named resources get ID 0 in NE modules (as IBM's) and in LX
modules IBM's number: the upper-cased name's bytes in pairs, alternately
added as little-endian and subtracted as big-endian 16-bit words (0
becomes 0x8000, a taken number moves up by one, the last resource is
numbered first), listed per type in an RT_RESNAMES (255) resource: the
count, the IDs and the NUL-ended names. NE modules get the resources as
their last segments on sector boundaries (flags = load/memory bits |
0x0C81). `-x` writes iterated (EXEPACK 1) resource pages for both methods
when that is smaller; `-p` keeps resources from crossing 64K. Icon 1 (or
DEFAULTICON) also becomes the module's `.ICON` extended attribute, the
last ASSOCTABLE its `.ASSOC`.

Checked (`cargo test -p os2-rc`; `examples/rccheck.rs` and IBM's output
from runs under rs2): the 92 Toolkit sample scripts and the 18 Borland
C++ example scripts IBM's RC compiles give byte-identical `.RES` files,
332 resources equal those bound into the samples' modules, the test
scripts in `crates/os2-rc/tests/rc` give the `.RES` files IBM's RC wrote
for them (`tests/rc/ibm`), and binding is byte-identical to IBM's for
LX modules with and without resources (HELLO.EXE, IPFCNTRL.DLL with debug
information, OREXXWPS.DLL, REXXC.EXE with 30 named resources) and NE
modules (APMDAEMN.EXE; PMFORMAT.DLL except the non-resident table
offset, which IBM's RC writes 64K too high). The 52 `.RES` files Borland's
BRCC wrote for its examples bind. Not like IBM's: compressed pages
(IBM's `-x2 -p` ended abnormally under rs2, its `-x1` left HELLO's pages uncompressed),
`NOT` on a combo box's or spin button's style, and IBM's RC rejects some
expressions (`%`, `<<`, character constants) rs2's takes. FONT copies
the font file without a font directory.

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
