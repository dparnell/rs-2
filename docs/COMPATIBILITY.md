# Compatibility with original OS/2 programs

Text-mode (VIO) programs from OS/2 Warp 4.52 and the OS/2 Toolkit 4.5,
run unmodified under rs2 in QEMU. Programs that need Presentation Manager
DLLs (PMWIN, PMGPI, PMSHAPI, ...) are out of scope here, except where a
text-mode program only reads profiles (TELNET).

The test setup is the development C: drive built from `work/modules.txt`:
IBM `CMD.EXE` as the shell, the programs under `C:\IBM` and `C:\TK`, the
REXX DLLs in `C:\OS2\DLL`, and `OSO001.MSG`, `OSO001H.MSG`, `REX.MSG`,
`REXH.MSG` in `C:\OS2\SYSTEM` (the kernel's MSG code looks there and
along `DPATH`). The IBM files are test inputs only and are not part of
the repository or the rs2 ISO. Programs with bound messages (MSGBIND)
also run without OSO001.MSG; see [Messages and national language
support](#messages-and-national-language-support).

Status: **works** = the tested function behaves as on OS/2;
**partial** = runs, but something tested is missing or differs;
**fails** = does not do its job.

## Command interpreter and REXX

| Program | Status | Notes |
|---------|--------|-------|
| CMD.EXE (IBM) | works | Interactive use, internal commands, pipes, redirection, `SET`, batch files (`FOR`, `IF`, `IF ERRORLEVEL`, `GOTO`, `SETLOCAL`/`ENDLOCAL`, `DETACH`), `DATE`/`TIME` set the clock, REXX `.CMD` files. |
| CMD.EXE (rs2) | works | Runs REXX `.CMD` files through REXX.DLL when it is installed, and the IBM utilities. |
| REXX (REXX.DLL, REXXAPI.DLL, REXXINIT.DLL) | works | `.CMD` files starting with `/* */` run through CMD. Tested: arithmetic, procedures/recursion, `PARSE`, `PULL` from the keyboard, the external data queue (`QUEUE`/`PUSH`/`PULL`/`QUEUED`), stream I/O (`LINEOUT`, `LINEIN`, `LINES`, `STREAM`), commands sent to CMD (`'dir'`, `rc`), `INTERPRET`, `SIGNAL ON SYNTAX`, `TIME('E')`, `RANDOM`, `VALUE(..., 'OS2ENVIRONMENT')`, error reports (`REX0040: Error 40 running ...`). A file with bare LF line ends gives REX0013, which is REXX's own behaviour. |
| REXXUTIL.DLL | partial | `SysLoadFuncs` works; tested `SysCls`, `SysCurPos`, `SysTextScreenRead`, `SysTextScreenSize`, `SysSleep`, `SysGetKey`, `SysFileTree`, `SysFileDelete`, `SysMkDir`, `SysRmDir`, `SysDriveMap`, `SysTempFileName`, `SysBootDrive`, `SysVersion`, `SysOS2Ver`, `SysSearchPath`, `SysPutEA`, `SysGetEA` (on FAT16, FAT32 and HPFS). `SysIni` needs PMSHAPI's profiles, which rs2 keeps in memory only (no INI files). |
| REXXTRY.CMD | works | With an expression on the command line and interactively. |
| RXQUEUE.EXE | works | `dir \| rxqueue`, then the queue read from another REXX process. |
| RXSUBCOM.EXE | works | Usage text only. |
| REXXC.EXE | fails | Object REXX's tokenizer: it imports `REXX.24` (`RexxTranslateProgram`) and `REXX.114`, which only Object REXX's interpreter exports. That is OREXX.DLL on the media (module name REXX, installed as REXX.DLL when Object REXX is selected), and it needs the SOM runtime (SOM, SOMD, SOMTC, REXXSOM), which rs2 does not have yet. Not a kernel issue. |
| CALLREXX.EXE (Toolkit sample) | works | 32-bit program calling `RexxStart` with a function file. |

## Base utilities

| Program | Status | Notes |
|---------|--------|-------|
| ATTRIB.EXE | works | List and change attributes. |
| BACKUP.EXE | works | To a FAT disk (`--hdd` image). On QEMU's virtual FAT drive (the ESP directory) QEMU itself stops when the attribute of `\BACKUP\CONTROL.001` is changed right after it was written: its monitor no longer answers either, so the hang is in QEMU's vvfat driver, not in rs2. |
| RESTORE.EXE | works | Restores a deleted file from that backup set. |
| BLDLEVEL.EXE | works | |
| CACHE.EXE | works | Shows and sets the HPFS cache parameters (`DosFSCtl` 0x9001-0x9003); lazy writing is reported off, as rs2's HPFS writes through. |
| CACHEJFS.EXE, DEFRAGFS.EXE, EXTENDFS.EXE, CLRBBLKS.EXE | works | Report that there is no JFS (`DosFSCtl` to the JFS FSD fails with ERROR_INVALID_FSD_NAME). |
| COMP.COM | works | Including the Y/N prompts. |
| DISKCOMP.COM | works | Reports that it cannot use a hard disk (no diskette drive). |
| EAUTIL.EXE | works | Splits a file's EAs into a hold file (`/S`) and joins them back (`/J`) on FAT16, FAT32 and HPFS (16-bit `DosEnumAttribute`, `DosQFileInfo` level 3, `DosSetFileInfo` level 2). |
| EJECT.EXE | works | Usage text. |
| EXECMODE.EXE | works | Usage text. |
| FIND.EXE | works | |
| HELPMSG.EXE | works | Message and explanation from OSO001.MSG/OSO001H.MSG. |
| LABEL.COM | works | Shows and sets the volume label. |
| MAKEINI.EXE | works | Usage text only. |
| MODE.COM | works | `MODE CO80`, `MODE CO80,43`/`50`/`60` (the 8x16, 8x14 and 8x8 fonts), `MODE COMn:baud,parity,databits,stopbits` and the COM status display. |
| MORE.COM | works | |
| PATCH.EXE | works | Usage text only. |
| PSTAT.EXE | works | `/C` processes and threads, `/L` libraries (via `DosQuerySysState`). |
| PSFILES.EXE | works | |
| PSSEMS.EXE | works | Shared event and mutex semaphores with their openers. |
| REPLACE.EXE | works | Add (`/A`) and replace. |
| SETBOOT.EXE | works | `/Q`: "Boot Manager is not installed". |
| SORT.EXE | works | |
| STRACE.EXE | works | Usage text (loads `DOSCALL1` by name). |
| TEDIT.EXE | works | Opens, edits (cursor keys, typing in the file and on the command line) and redraws through VIO and KBD. |
| SYSLEVEL.EXE | works | Finds `SYSLEVEL.OS2` and lists the component, version and CSD levels. |
| TREE.COM | works | `TREE [d:] [/F]`. |
| UNDELETE.COM | works | Reports the missing `DELDIR` setting. |
| UNPACK.EXE, UNPACK2.EXE | works | Unpack IBM packed files (UNPACK hands newer files to UNPACK2). |
| XCOPY.EXE | works | Including `/S /E`, wildcard targets. |
| XDFCOPY.EXE | works | Reports that XDFLOPPY.FLT is not loaded. |
| KEYB.COM | fails | Imports PMMERGE (Presentation Manager). rs2 has its own KEYB.EXE. |
| ANSI.EXE | works | |
| MKSYSLVL.EXE, LAPSDEL.EXE | works | (tested earlier) |

## Toolkit

| Program | Status | Notes |
|---------|--------|-------|
| ALP.EXE | works | Assembles a small source to an object file (needs ALP.MSG). |
| LINK386.EXE | works | Links that object file into an LX executable. |
| NMAKE.EXE | works | Builds a makefile target, running commands through CMD. NMMSG.MSG is not on the media, so error texts are missing. |
| EXEHDR.EXE | partial | NE files are dumped completely. On every LX file (ALP.EXE as well as the LINK386 output) it prints the header and object table, then its C runtime reports an internal error whose text is in DDE4.MSG (not on the media) and faults freeing a heap block whose header holds the text EXE (the VAC heap signature check at 0x2907D). No failing API call precedes it (every DosRead/DosSetFilePtr result checked); not resolved. |
| IMPLIB.EXE | works | |
| MAPSYM.EXE, MARKEXE.EXE | works | |
| MKMSGF.EXE, MSGBIND.EXE | works | Usage text only. |
| RC.EXE, RC16.EXE, RCPP.EXE | works | Usage text only. |
| GENCAT.EXE, RPCGEN.EXE, FWDSTAMP.EXE | works | Usage text only. |
| PACK.EXE, PACK2.EXE | partial | PACK.MSG is not on the media. |
| WORMS.EXE (sample) | works | Mouse pointer and clicks through MOUCALLS (Exit by clicking). |
| TIMER0.EXE (sample) | works | Reports that the TIMER0$ sample driver is missing. |
| EAS.EXE (sample) | not tested | The EA editor sample is a Presentation Manager program. |

## Network programs

On rs2's TCPIP32/SO32DLL/TCP32DLL/TNLS32/PMSHAPI DLLs (see
[NETWORK.md](NETWORK.md#the-os2-socket-api)), QEMU user networking
(`--net` e1000 or `--virtio-net`, also `--smp 4`), the host at 10.0.2.2
running a test FTP/line/echo server. The TCP/IP 4.x applications come
from `cid/server/tcpapps/install/base` on the client CD; MPTN's own
programs (IBM's PING, NETSTAT, IFCONFIG, HOST, the stack DLLs) are not
on the media, so rs2's own utilities take their place.

| Program | Status | Notes |
|---------|--------|-------|
| FTP.EXE | works | Login, `dir`, `get` (ASCII and binary, 300 KB), `put` (byte-identical on the host), `quit`; active-mode data connections (PORT, Raccept/Rbind/Rlisten). About 2-3 MB/s each way under TCG. |
| TELNET.EXE | works | Connects, shows the remote side (TNLS32 16-bit character conversion into its VT emulator), sends typed lines, ends when the host closes. Needed KbdSetStatus to accept binary mode with echo when switching from ASCII, and PMSHAPI for its profile reads. |
| FINGER.EXE | partial | Runs; names resolve through DNS (10.0.2.3); no finger server was reachable to test the exchange. |
| RXSOCK.DLL (REXX) | works | SockInit, SockGetHostId, SockGetHostByName/ByAddr, SockSocket, SockConnect, SockSend/Recv (TCP echo through the host), SockGetPeerName, SockBind, SockSendTo/RecvFrom (UDP), SockSoClose. |
| PORTMAP.EXE, RPCINFO.EXE (with RPC32DLL.DLL) | works | `rpcinfo -p localhost` lists the registered programs (SO32DLL/TCP32DLL). |
| Toolkit tcps/tcpc, udpc, selects | works | TCP client/server, BSD `select()` server with TCP and UDP clients. |
| Toolkit accept/recv | works | `accept_and_recv()`; the client connects to INADDR_ANY. |
| Toolkit os2ioctl | works | SIOSTATCNTRT/SIOSTATRT routing table, SIOCSSYN. |
| Toolkit rawex, geneserv/genesend | works | Sun RPC over UDP through the portmapper. |
| Toolkit rgs/rgc (rpcgen) | fails | `svcudp_create - cannot getsockname` inside IBM's RPC32DLL; not investigated. |
| Toolkit echos/echoc (HPS) | fails | Need `sysctl()` memory-mapped I/O (OS2_MEMMAPIO), which rs2 does not have. |
| TFTP.EXE, INETD.EXE, LPD.EXE, TELNETD.EXE, BOOTP.EXE | fails | 16-bit: need TCPIPDLL.DLL (not written yet) and IBM's TNLS16.DLL, which imports PMWIN's 16-bit entry points. |

## Messages and national language support

Bound messages, tested with OSO001.MSG removed from the disk: XCOPY,
REPLACE (old-style stub with the header at CS:0), BACKUP, RESTORE and
HELPMSG (16-bit, version 2 segments) and XDFCOPY (32-bit, its own
component XDF) print their messages from the bound segment, as on OS/2.
Messages a program did not bind still need the file: SORT, and EAUTIL
for SYS1003, report `SYS0318: Message file OSO001.MSG cannot be found
for message  1003.` (DOSCALL1 inserts the number with a leading blank).

With `COUNTRY=049`, `CODEPAGE=437,850` and `DEVINFO=KBD,GR,...` in
CONFIG.SYS, IBM CMD.EXE's DATE and DIR use German formats (`dd.mm.yy`,
`10.19`), rs2 CMD's CHCP lists 437 and 850 and switches between them,
and the keyboard types the German layout (dead keys, AltGr).

| Area | Status | Notes |
|------|--------|-------|
| DosGetMessage, DosInsMessage, DosPutMessage | works | Bound segments first, then the file (as named, DPATH, root of the boot drive). Class `?` gives MR_MID_NOT_FOUND; E/W get the `SYSnnnn: ` prefix; `%1`-`%9` up to the count, other `%` sequences are kept (MKMSGF consumes `%0`); truncation ends with CR LF (316); the retriever's own SYS0316-0321 texts on errors. DosPutMessage breaks lines at 80 columns as DOSCALL1 does. |
| DosQueryMessageCP | works | 16- and 32-bit (the stubs append the bound segment as a fifth argument; the 32-bit entry does not pop it). Code pages, language and sublanguage from the bound block or the file's extended header. |
| Country information | works | 87 countries (date order, separators, currency, decimals, clock, list separator). Compared with IBM's COUNTRY.SYS: 209 of 282 tables identical; the others differ in dummy entries of COUNTRY.SYS (country 1 with foreign code pages) and Arabic currency glyphs. |
| Case maps | works | 21 single-byte code pages (437, 850, 852, 855, 857, 860-866, 869, 874, 1125, 813, 819, 912, 915, 921, 1004); DBCS code pages map ASCII only. 263 of 282 identical to COUNTRY.SYS. |
| Collating sequences | partial | Western code pages match IBM's tables (country 1 group, Nordic and Spanish variants); Central European, Turkish, Cyrillic and Greek use alphabet-order weights that sort correctly but are not IBM's numbering (103 of 282 identical). |
| DBCS environment | works | Lead byte ranges for 932/942/943, 934/944, 936/938/946/948, 949, 950, 1381, 1386; empty for single-byte code pages (all identical to COUNTRY.SYS). |
| Code pages | works | DosQueryCp, DosSetProcessCp (prepared code pages only, ERROR_INVALID_CODE_PAGE otherwise), DosSetCp, VioSetCp, KbdSetCp/KbdGetCp, CONFIG.SYS CODEPAGE= and COUNTRY=. The screen shows all single-byte code pages above. |
| Keyboard layouts | works | US UK GR FR BE NL IT SP LA PO SG SF SV SU DK NO IS CF TR PL with AltGr (or Ctrl+Alt) and dead keys; DEVINFO=KBD,xx; rs2 KEYB.EXE. No Cyrillic, Greek or Hebrew layouts yet. |

## Console subsystems and character devices

What VIO, KBD, the device monitors and the character devices do, and how
it is checked (`rs2\apitest` runs the VIO/KBD/monitor/device checks;
`rs2\apitest keys` shows the packets a keyboard monitor sees):

- Text modes 40/80/132 columns by any number of rows that fits the
  framebuffer; the font is the largest of 8x16, 8x14 and 8x8 that fits
  (at 1024x768: 25 and 43 rows 8x16, 50 rows 8x14, 60 rows 8x8). The
  8x14 and 8x8 fonts are derived from unscii-16 (public domain) by
  `tools/scripts/mksmallfonts.py`. VioGetFont/VioSetFont (current and
  built-in fonts, user fonts).
- VioGetState/VioSetState: palette registers, DAC colour registers,
  border colour, blink versus background intensity (blinking is drawn),
  underline location, target display. Cursor shapes in scan lines or
  percentages, hidden cursor.
- VioGetPhysBuf and the SCREEN$ SCR_ALLOCLDT IOCtl map a "video memory"
  buffer with the session's cells in VGA layout; writes appear on the
  screen within a few ticks. VioGetBuf/VioShowBuf (logical buffer),
  VioScrLock/UnLock, VioSavRedrawWait/Undo and VioModeWait/Undo
  (notified on session switches and pop-ups), VioPrtSc/PrtScToggle and
  the PrtSc key (to LPT1, which is a sink).
- VioPopUp/VioEndPopUp: a separate pop-up screen over the foreground
  session (opaque or transparent), keyboard input goes to it, session
  switching is held off; the screen comes back unchanged. The kernel's
  hard-error pop-up (e.g. SYS0021 "The drive is not ready" when a CD is
  ejected) offers Retry, return the error, or end the program;
  DosError(0) turns it off for a process.
- VioWrtTTY ANSI support per session (VioSetAnsi): cursor movement and
  positioning, save/restore, erase display/line, attributes (bold,
  blink, reverse, concealed, colours, bright colours), `ESC[6n` (the
  cursor position report is typed into the keyboard buffer), screen
  modes and line wrap (`ESC[=7h/l`).
- KBD: per-session logical keyboards (KbdOpen/KbdClose, KbdGetFocus/
  KbdFreeFocus), KBDINFO per keyboard (echo, ASCII/binary, turnaround
  character, interim flags, shift reporting), KbdXlate, KbdSetCp/GetCp,
  KbdGetHWID. Line editing for KbdStringIn and cooked DosRead from CON:
  Left/Right, Home/End, Ins (insert/overwrite), Del, Backspace, Esc,
  F1/F3 (copy from the previous line); binary DosRead returns keys as
  typed. In ASCII mode Ctrl+C raises the signal and Ctrl+S suspends
  screen output until the next key; Ctrl+Break always signals. The
  shift state includes Insert and the lock keys held down; PS/2
  keyboards get LED and typematic commands (KBD_SETTYPAMATICRATE, `MODE
  CON RATE= DELAY=`), USB keyboards repeat at the same rate.
- Device monitors (MONCALLS): DosMonOpen/Reg/Read/Write/Close for KBD$
  (14-byte key packets, makes and breaks with the raw scan code) and
  MOUSE$.
- Replacement subsystems: VioRegister/VioGlobalReg/VioDeRegister,
  KbdRegister/KbdDeRegister, MouRegister/MouDeRegister route the chosen
  functions to a 16-bit entry point in a DLL (apitest registers a
  16-bit replacement in TESTDLL.DLL that answers VioGetCurPos and passes
  VioSetCurPos on to the base VIO).
- Devices: `COM1`-`COM4` (16550 UARTs; receive interrupt driven, DCB
  read timeouts; category 1 IOCtls for baud rate, line control, modem
  control, DCB, queues, errors; tested with QEMU's second serial port:
  `-- -chardev socket,id=c2,path=...,server=on,wait=off -serial
  chardev:c2`), `LPT1`-`LPT3` and `PRN` (sinks with printer status
  IOCtls), `CLOCK$` (read/write the clock record), `MOUSE$`/`POINTER$`
  (category 7 queries), `KBD$` category 4 IOCtls (input mode, interim
  flags, shift state, typematic rate, read/peek, code page, translate,
  hardware ID). COM1 also carries the kernel log.

## Fixed while getting these to run

- KbdSetStatus rejected binary mode with echo on in one call; OS/2
  judges the echo bit by the mode the call starts from (IBM TELNET sets
  binary, echo and shift reports together).

- Loader: DLL objects whose preferred base is below the shared arena (the
  linker default 0x10000 of application DLLs) are relocated into the
  shared arena instead of colliding with the process's private memory.
- Loader: a 16-bit selector fixup to a 32-bit object without the alias
  flag yields the flat selector; `THK32R3DS` (DOSCALLS.295) and
  `DOS32FLATDS` (370) are absolute exports. IBM's 16-to-32 thunks rely on
  both.
- Loader: `DOSHUGESHIFT` (135) and `DOSHUGEINCR` (136) are absolute
  exports; `DOSCALL1` names the kernel API module.
- DLL initialisation inside `DosLoadModule` runs on the caller's stack
  below its live frames (it used the top of the thread's stack and
  overwrote CMD.EXE's frames).
- Kernel stacks are 128 KB (the loader recursing through a chain of new
  DLLs overflowed 32 KB into the neighbouring heap block); the scheduler
  checks for overflow.
- `DosExecPgm`/`DosLoadModule` report the failing module in the failure
  buffer; `DosExecPgm` looks for a bare program name along `PATH`.
- rs2 CMD's DIR panicked on volume labels longer than 11 characters (CD
  labels); the AHCI driver now reads the sense data after a failed ATAPI
  command, so a changed medium's unit attention is cleared.
- REXX API server: the PM shell normally loads `REXXINIT` at start-up
  (`SYS_DLLS LoadOneTime`). rs2 starts `SYSDLLS.EXE REXXINIT` (an rs2
  program that keeps the DLL resident) when REXXINIT.DLL is present.
- 16-bit `DosSetFilePtrL`, `DosOpenL`, `DosSetFileSizeL` take the 64-bit
  value low dword first (IBM CMD.EXE batch files did not run).
- Console input: a cooked `DosRead` from CON hands out the edited line in
  pieces, keeping the rest for the next read.
- The 32-bit `DosTrueGetMessage` receives the bound message segment as an
  extra first argument and pops it (`RET 4`).
- 16-bit `DosCreateThread` pushes a far return address below `pbStack`.
- The environment segment holds the program's full path between the
  variables and the command line and is exactly as long as its contents,
  as on OS/2 (IBM CMD.EXE's first `SET` corrupted the environment).
- rs2's own programs called `MSG.6` as if it were DosGetMessage; the
  os2 crate now has OS2386.LIB's stub (push the bound segment, let
  DosTrueGetMessage pop it), so HELP and the utilities get message texts.
- rs2 CMD wrote accented characters as UTF-8 (two bytes); user programs
  now write text one code page byte per character.
- `THK16_UNITHUNK` (DOSCALL1's generic register-based 16-to-32 thunk) and
  the register-based 16-bit `DosSelToFlat`/`DosFlatToSel`.
- New APIs: `DosSubSetMem`/`DosSubAllocMem`/`DosSubFreeMem`/`DosSubUnsetMem`
  and 16-bit `DosSubSet`/`DosSubAlloc`/`DosSubFree`, `DosSizeSeg`,
  `DosEditName`, `DosEnumAttribute`, `DosQuerySysState`, `DosCopy`,
  `DosForceDelete`, `DosMkDir2`, `DosReplaceModule`, `DosSetDateTime`,
  `DosAllocHuge`, `DosGetHugeShift`, `DosReadAsync`, `DosWriteAsync`,
  `DosQueryPathInfo` levels 4 and 6, `DosSetFileInfo`/`DosSetPathInfo`
  level 11.
- Extended attributes are stored: on FAT12/16 in `EA DATA. SF` as OS/2
  does, on FAT32 in FAT32.IFS-style `NAME EA. SF` files, on HPFS in the
  F-node or outside it (see [formats/fat-ea.md](formats/fat-ea.md) and
  [formats/hpfs.md](formats/hpfs.md)). Query/set with EAOP2 (and the
  16-bit EAOP) in `DosQueryPathInfo`/`DosQueryFileInfo` levels 2-4,
  `DosSetPathInfo`/`DosSetFileInfo` level 2, EAs at creation in `DosOpen`,
  `DosOpenL`, `DosOpen2`, `DosCreateDir`, `DosMkDir2`, `DosFindFirst`
  levels 2 and 3, `DosEnumAttribute`; `DosCopy` copies EAs and `DosMove`
  keeps them; CHKDSK checks EA references.

- `DosGetMessage`: an insertion table slot holding NULL made the call fail
  with ERROR_INVALID_ADDRESS; OS/2 only reads the strings the message uses
  (SYSLEVEL listed nothing).
- A text-mode program that initialises PM (REXXUTIL's `SysIni` calls
  WinInitialize in rs2's PMWIN) was moved into the PM session, so CMD.EXE
  never got another key after running RXTEST.CMD; only PM programs join
  the PM session now.
- Found by the stress test (`RS2\STRESS.EXE`, see below): every program
  run leaked its executable module and file image; thread IDs were never
  reused (1020 threads per process lifetime) and thread stacks never
  freed; a thread woken between marking itself blocked and blocking could
  sit on the run queue twice and a stale entry resume a dead thread;
  waits missed kills and signals that arrived before they went to sleep;
  a killed or signalled pipe writer waited for a reader forever and an
  interrupted pipe read returned end-of-file; waiters on a mutex whose
  owner died were not woken; semaphore waits interrupted by a signal
  returned ERROR_TIMEOUT; uncollected children stayed in the process
  table after their parent ended; DosKillProcess from another process
  skipped the exit list; DosSleep ended early on stray wake-ups; a
  program touching more memory than the machine has could starve the
  kernel heap (user pages now leave a 4 MB reserve); 16-bit timers wrote
  to the physical frame of a RAM semaphore even after it was freed.

## Missing or incomplete

- Extended attributes: `ERROR_NEED_EAS_FOUND` is only reported by
  `DosCopy` (critical EAs to a target without EA support); `DosOpen` does
  not refuse files with critical EAs to programs that are not EA-aware.
  The 16:16 EAOP2 of `Dos16OpenL` is assumed to point at an FEA2LIST
  (unverified). IBM's CHKDSK could not check rs2-written FAT volumes
  (`UFAT.DLL` is not on the media); the FAT32 EA flag values follow
  FAT32.IFS from memory and are unverified against a real disk.
- AVIO (VioCreatePS and the other Presentation Manager VIO calls) is not
  implemented.
- Changing a CD for a different one is not detected (the old volume stays
  mounted); re-inserting the same medium and choosing Retry works.
- Writing to QEMU's virtual FAT drive (the ESP directory) can hang QEMU
  itself (seen with BACKUP; QEMU's monitor stops answering too); FAT disk
  images are fine.
- IBM KEYB.COM needs PMMERGE; its KEYBOARD.DCP tables are not read (rs2
  has its own layouts and KEYB.EXE).
- Message files with DBCS text: DosPutMessage writes them unwrapped.
- File locks (`DosSetFileLocks`) are advisory: they conflict with each
  other but reads and writes are not checked against them.
- The `DosProtect*` functions hand out a file handle lock ID but do not
  check it.

