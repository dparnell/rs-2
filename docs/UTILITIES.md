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
| LINK, LINK386 | The OMF linkers: LINK makes 16-bit segmented (NE) programs and DLLs, LINK386 32-bit linear (LX) ones | IBM's command line, prompts, response files and options, .DEF files, map files; output laid out as IBM's (below) |
| LVM, FDISK | Partitions and volumes | IBM's LVM command line (/QUERY, /CREATE:PARTITION and COMPATIBILITY volumes, /DELETE, /SETNAME, /SETSTARTABLE, /HIDE, /NEWMBR) plus `/SETLETTER`, and an `LVM>` prompt without arguments. MBR disks with OS/2 LVM drive letter tables, GPT disks (no letters). IBM's LVM reads what rs2's writes. |
| PACK, PACK2, UNPACK, UNPACK2 | FTCOMP packed files (the installation media format) | PACK writes LZ-coded blocks without the entropy stage (IBM's UNPACK reads them); /H /D /T /C /A /R /L; UNPACK /SHOW /N /F, EAs restored |
| PATCH | Patches files | Interactive, or /A with a control file (`FILE`, `VER`, `CHA` lines) |
| POWERCFG | The processor power plan, idle states, speed control and power readings, the power source and battery (rs2's own) | `Rs2PowerControl` and `APM$` 0x60 (ARCHITECTURE.md "Power management"); `/SET:PERFORMANCE\|BALANCED\|POWERSAVER` changes the plan now, `/SAVE` writes `POWERPLAN=` to CONFIG.SYS, `/WATCH[:n]` shows the readings every second |
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

## The linkers (`crates/omflink`)

`\OS2\LINK.EXE` and `\OS2\LINK386.EXE` (`userland/link`) are thin
wrappers around `crates/omflink`, a `no_std` linker that also runs on the
host (`cargo run -p omflink --example link -- link|link386 COMMAND LINE`,
file names looked up case-insensitively as on OS/2).

- **Command line**: `LINK objs,exe,map,libs,def;` as IBM's: objects
  separated by `+` or blanks, options (`/NAME[:value]`, any unique
  abbreviation, numbers in C syntax) anywhere, `;` takes the defaults for
  the rest. Fields not given are asked for (`Object Modules [.OBJ]:`, `Run
  File [X.EXE]:`, `List File [NUL.MAP]:`, `Libraries [.LIB]:`, `Definitions
  File [NUL.DEF]:`; a line ending in `+` continues the field), also from
  redirected input. `@file` reads a response file, each line answering one
  prompt. Options also come from the `LINK` / `LINK386` environment
  variables, libraries and objects are searched for in the current
  directory, directories given in the libraries field and `LIB`. An empty
  map field (`a,,;`) makes `A.MAP`, none (`a;`) no map unless /MAP.
- **Options**: /ALIGNMENT, /BASE, /BATCH, /CODEVIEW and /DEBUG (accepted,
  no debug information is written), /DLL, /DOSSEG, /EXEPACK[:1|:2],
  /FARCALLTRANSLATION (accepted), /HELP and /?, /INFORMATION, /LINENUMBERS
  and /MAP (map file), /NODEFAULTLIBRARYSEARCH[:lib], /NOEXTDICTIONARY,
  /NOIGNORECASE, /NOLOGO, /NONULLSDOSSEG, /NOOUTPUTONERROR, /NOPACKCODE,
  /PACKCODE[:n], /PACKDATA[:n], /PMTYPE:PM|VIO|NOVIO, /SEGMENTS, /STACK,
  /WARNFIXUP, and rs2's /IBMFRAMES (below); real-mode and overlay options
  draw L4013, unknown ones L4017.
- **Objects**: THEADR/LHEADR, LNAMES/LLNAMES, SEGDEF (16 and 32), GRPDEF,
  PUBDEF/LPUBDEF, EXTDEF/LEXTDEF/CEXTDEF, COMDEF/LCOMDEF (communal
  variables into `c_common` or `FAR_BSS`), LEDATA/LIDATA (16 and 32, fix-ups
  in iterated data repeated), FIXUPP with threads and all frame and target
  methods, COMDAT (first one kept), BAKPAT/NBKPAT, ALIAS, COMENT (default
  libraries, DOSSEG, IMPDEF/EXPDEF, weak externals), MODEND's start address;
  line numbers and debug records are read and dropped. Libraries are read
  module by module (the dictionary is not needed). Library modules are
  pulled in IBM's order: each library in turn, one pass over its modules in
  file order per round, loading those that define a symbol undefined when
  the pass started; default libraries (named by objects) after the given
  ones.
- **.DEF files**: free-form as IBM's grammar (NAME's WINDOWCOMPAT may be on
  the next line): NAME/LIBRARY (WINDOWAPI, WINDOWCOMPAT, NOTWINDOWCOMPAT,
  NEWFILES, INITINSTANCE/INITGLOBAL, TERMINSTANCE/TERMGLOBAL), PHYSICAL and
  VIRTUAL DEVICE, DESCRIPTION, PROTMODE, STACKSIZE, HEAPSIZE, CODE, DATA,
  SEGMENTS (CLASS, IOPL, ALIAS, MIXED1632...), EXPORTS (`name[=internal]
  [@ord] [RESIDENTNAME|NONAME] [pwords] [NODATA] [PRIVATE]`), IMPORTS
  (`[internal=]module.entry`), STUB (file or NONE), BASE, EXETYPE, OLD.
- **NE (LINK)**: as IBM's LINK (Microsoft's 5.10) lays it out: segments in
  class order (or DOSSEG's), those named in SEGMENTS first; code segments
  packed up to 65500 bytes unless /NOPACKCODE; groups in one segment;
  segments fixed, IOPL segments movable with INT 3Fh entries; file data up
  to the last non-zero byte, 512-byte alignment; internal fix-ups resolved
  at link time except selectors, which are chained per target (far pointers
  become an offset and a chained selector fix-up); imports chained per
  target; the records in IBM's order (internal by segment, imports by
  (module * 8 + ordinal) mod 128), module references numbered by their last
  fix-up going backwards; a STACK segment at the end of DGROUP becomes the header's stack
  size (SS:SP = DGROUP:0); exports without an ordinal get resident names;
  without DESCRIPTION the file name is the description; the checksum is
  the sum of the file's dwords. Exports with the same internal name: the
  first wins (L4023), as IBM's LINK.
- **LX (LINK386)**: as IBM's LINK386: objects at 64 KB steps from 0x10000
  (or /BASE), code packed into one object, a group (DGROUP) per object,
  16-bit segments in objects of their own with the 16:16 alias flag;
  internal fix-ups kept (a program can still be moved), records sharing a
  target joined into source lists, in IBM's (hash table) order: imports,
  then internal targets, by ordinal or offset mod 64; page data padded to the file alignment (/ALIGNMENT, default 512),
  the pages of writable non-shared objects first; STACKSIZE added to the
  automatic data object (ESP at its end, L4049 without one); module version
  2.2; module names in lower case (from NAME/LIBRARY or the file), exported
  and .DEF-imported names in upper case unless /NOIGNORECASE; exports
  without an ordinal resident; all DLL entries with the shared-data flag; 16-bit (type 1),
  call-gate (2), 32-bit (3) and forwarder (4) entries (an export of an
  imported name forwards it); /EXEPACK:1 iterated and /EXEPACK:2 compressed
  pages (whichever is smaller). The warnings L4021, L4036 and L4071 come in
  IBM's order and LINK386's map file starts with those shown before it was
  written.
- **One deliberate difference**: a 32-bit offset into a 32-bit object is a
  flat address whatever frame the object module names. IBM's LINK386 makes
  it relative to the object when the frame is a segment (no OS/2 program
  can use that); Open Watcom's run time names segment frames, and Open
  Watcom C programs linked by IBM's LINK386 crash at start-up, while rs2's
  LINK386 links them into working programs. `/IBMFRAMES` (rs2's own
  option) does as IBM's.
- **Messages**: IBM's numbers (L1xxx fatal, L2xxx errors, L4xxx warnings)
  in `WHERE : error Lnnnn: text` form, rs2's own wording; exit codes 0, 2
  (errors) and 4 (fatal).

How it was checked: the same objects linked by IBM's LINK and LINK386 on
rs2 and by rs2's on the host and under rs2, compared with `wdump`, a
relocation/fix-up record decoder and `cmp`. "Identical" means byte for
byte after the DOS stub (whose message is rs2's own; the NE checksum
differs by it):

| Input | Result |
|-------|--------|
| Borland's THUNK example (`BCOS2\EXAMPLES\THUNK`, `MAKE -B` running rs2's LINK) | EXE16.EXE identical to IBM LINK's; runs and prints the 32-bit DLL's string. The same with the fields typed at LINK's prompts (from redirected input). |
| Open Watcom 16-bit C, large model (`wcc -ml`, CLIBL.LIB, OS2286.LIB) | Identical to IBM LINK's; both print the same under rs2. |
| A 16-bit DLL in assembly (IOPL segment, movable entry, exports with and without ordinals, RESIDENTNAME, internal far pointers) | Identical to IBM LINK's. |
| Assembly (wasm) and Open Watcom C (`wcc386`, CLIB3R.LIB) LX programs, a program importing from a DLL by name and by ordinal | Identical to IBM LINK386's with `/IBMFRAMES`. Without it the Open Watcom C program runs under rs2 (IBM LINK386's crashes at start-up), also with /EXEPACK:1 and :2. |
| The Toolkit's PM sample `samples\pm\stdwnd\hello.c` (compiled by Open Watcom `wcc386 -3s`, linked by rs2's LINK386 under rs2, resources bound by the Toolkit's RC16) | Runs: its window with icon, menu and text. |
| A 32-bit DLL with a 16-bit code segment (16-bit, 32-bit and forwarder entries, NONAME, aliases, DATA MULTIPLE NONSHARED) and a program using it | Runs under rs2 (IBM's LINK386 refuses to forward an export to a .DEF import, L4025/L2022). |
| Open Watcom C++ (`wpp386`: templates and inline functions in COMDATs, virtual functions, a static constructor; PLIB3R.LIB) | Runs under rs2 (`C++ on LINK386: 42 2.5 49 206`). IBM's LINK386 rejects the 32-bit COMDAT records (L1101). |
| /EXEPACK:1 and :2 | Pages expand (os2-exe's loader code) to those of a plain link: `cargo run -p omflink --example lxcmp -- A B`. |

Borland C++'s own objects (C02D.OBJ, C2.LIB) are made for TLINK: IBM's
LINK386 rejects C02D.OBJ (a Borland COMENT) and rs2's misses TLINK's
`___StackBase`.

Host tests: `cargo test -p omflink` (command line, .DEF files, EXEPACK
round trips).

Not done: debug information (/DEBUG writes none), /MAP:FULL details and
line numbers in the map, the order of map entries with the same address
and of L2029 messages (hash ordered in IBM's), echoing response files,
incremental linking, overlays, device drivers beyond the module flags.

## Not (yet) rs2's own

From IBM's `\OS2`: XDFCOPY and VMDISK (XDF diskette images and the XDF
filter: no diskette drives), CACHEJFS, DEFRAGFS, EXTENDFS and CLRBBLKS
(JFS tools), EXECMODE, PSFILES, PSSEMS, STRACE, MAKEINI, SPOOL, SYSLOG,
LD2FIX, ARCINST/ARCRECOV (desktop archives), APMDAEMN, the
removable-media drivers' FCHECK2/FFORMAT2/ATAMNT2/AUTOUTL2, and
CHKDSK32/JFSCHK32 (IBM's file-system modules behind CHKDSK). PM programs,
REXX tools and network clients are covered elsewhere.
