# Compatibility with original OS/2 programs

Text-mode (VIO) programs from OS/2 Warp 4.52 and the OS/2 Toolkit 4.5,
run unmodified under rs2 in QEMU. Programs that need Presentation Manager
DLLs (PMWIN, PMGPI, PMSHAPI, ...) are out of scope here.

The test setup is the development C: drive built from `work/modules.txt`:
IBM `CMD.EXE` as the shell, the programs under `C:\IBM` and `C:\TK`, the
REXX DLLs in `C:\OS2\DLL`, and `OSO001.MSG`, `OSO001H.MSG`, `REX.MSG`,
`REXH.MSG` in `C:\OS2\SYSTEM` (the kernel's MSG code looks there and
along `DPATH`). The IBM files are test inputs only and are not part of
the repository or the rs2 ISO.

Status: **works** = the tested function behaves as on OS/2;
**partial** = runs, but something tested is missing or differs;
**fails** = does not do its job.

## Command interpreter and REXX

| Program | Status | Notes |
|---------|--------|-------|
| CMD.EXE (IBM) | works | Interactive use, internal commands, pipes, redirection, `SET`, batch files (`FOR`, `IF`, `IF ERRORLEVEL`, `GOTO`, `SETLOCAL`/`ENDLOCAL`, `DETACH`), `DATE`/`TIME` set the clock, REXX `.CMD` files. |
| CMD.EXE (rs2) | works | Runs REXX `.CMD` files through REXX.DLL when it is installed, and the IBM utilities. |
| REXX (REXX.DLL, REXXAPI.DLL, REXXINIT.DLL) | works | `.CMD` files starting with `/* */` run through CMD. Tested: arithmetic, procedures/recursion, `PARSE`, `PULL` from the keyboard, the external data queue (`QUEUE`/`PUSH`/`PULL`/`QUEUED`), stream I/O (`LINEOUT`, `LINEIN`, `LINES`, `STREAM`), commands sent to CMD (`'dir'`, `rc`), `INTERPRET`, `SIGNAL ON SYNTAX`, `TIME('E')`, `RANDOM`, `VALUE(..., 'OS2ENVIRONMENT')`, error reports (`REX0040: Error 40 running ...`). A file with bare LF line ends gives REX0013, which is REXX's own behaviour. |
| REXXUTIL.DLL | partial | `SysLoadFuncs` works; tested `SysCls`, `SysCurPos`, `SysTextScreenRead`, `SysTextScreenSize`, `SysSleep`, `SysGetKey`, `SysFileTree`, `SysFileDelete`, `SysMkDir`, `SysRmDir`, `SysDriveMap`, `SysTempFileName`, `SysBootDrive`, `SysVersion`, `SysOS2Ver`, `SysSearchPath`. `SysIni` fails (needs PMSHAPI/PMWIN). EA functions see no EAs. |
| REXXTRY.CMD | works | With an expression on the command line and interactively. |
| RXQUEUE.EXE | works | `dir \| rxqueue`, then the queue read from another REXX process. |
| RXSUBCOM.EXE | works | Usage text only. |
| REXXC.EXE | fails | Imports `REXX.24`, which this REXX.DLL does not export (the files come from different REXX versions). |
| CALLREXX.EXE (Toolkit sample) | works | 32-bit program calling `RexxStart` with a function file. |

## Base utilities

| Program | Status | Notes |
|---------|--------|-------|
| ATTRIB.EXE | works | List and change attributes. |
| BACKUP.EXE | works | To a FAT disk (`--hdd` image). On QEMU's virtual FAT drive (the ESP directory) the final attribute update of `\BACKUP\CONTROL.001` hangs. |
| RESTORE.EXE | works | Restores a deleted file from that backup set. |
| BLDLEVEL.EXE | works | |
| CACHE.EXE | partial | `DosFSCtl` is not implemented; reports the error and default values. |
| COMP.COM | works | Including the Y/N prompts. |
| DISKCOMP.COM | works | Reports that it cannot use a hard disk (no diskette drive). |
| EAUTIL.EXE | works | Files have no EAs (EAs are not stored yet). |
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
| PSSEMS.EXE | partial | Runs; no semaphores are reported by `DosQuerySysState`. |
| REPLACE.EXE | works | Add (`/A`) and replace. |
| SETBOOT.EXE | works | `/Q`: "Boot Manager is not installed". |
| SORT.EXE | works | |
| STRACE.EXE | works | Usage text (loads `DOSCALL1` by name). |
| TEDIT.EXE | works | Opens, edits (cursor keys, typing in the file and on the command line) and redraws through VIO and KBD. |
| SYSLEVEL.EXE | partial | Runs but lists nothing: it opens drives with DASD opens, which return ERROR_NOT_SUPPORTED. |
| TREE.COM | works | `TREE [d:] [/F]`. |
| UNDELETE.COM | works | Reports the missing `DELDIR` setting. |
| UNPACK.EXE, UNPACK2.EXE | works | Unpack IBM packed files (UNPACK hands newer files to UNPACK2). |
| XCOPY.EXE | works | Including `/S /E`, wildcard targets. |
| XDFCOPY.EXE | works | Reports that XDFLOPPY.FLT is not loaded. |
| KEYB.COM | fails | Imports PMMERGE (Presentation Manager). |
| ANSI.EXE | works | |
| MKSYSLVL.EXE, LAPSDEL.EXE | works | (tested earlier) |

## Toolkit

| Program | Status | Notes |
|---------|--------|-------|
| ALP.EXE | works | Assembles a small source to an object file (needs ALP.MSG). |
| LINK386.EXE | works | Links that object file into an LX executable. |
| NMAKE.EXE | works | Builds a makefile target, running commands through CMD. NMMSG.MSG is not on the media, so error texts are missing. |
| EXEHDR.EXE | partial | Dumps executables. On the LINK386 output above it reports an internal library error (DDE4.MSG is not on the media) and then crashes in the C runtime's heap check. |
| IMPLIB.EXE | works | |
| MAPSYM.EXE, MARKEXE.EXE | works | |
| MKMSGF.EXE, MSGBIND.EXE | works | Usage text only. |
| RC.EXE, RC16.EXE, RCPP.EXE | works | Usage text only. |
| GENCAT.EXE, RPCGEN.EXE, FWDSTAMP.EXE | works | Usage text only. |
| PACK.EXE, PACK2.EXE | partial | PACK.MSG is not on the media. |
| WORMS.EXE (sample) | works | Mouse pointer and clicks through MOUCALLS (Exit by clicking). |
| TIMER0.EXE (sample) | works | Reports that the TIMER0$ sample driver is missing. |

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
- `THK16_UNITHUNK` (DOSCALL1's generic register-based 16-to-32 thunk) and
  the register-based 16-bit `DosSelToFlat`/`DosFlatToSel`.
- New APIs: `DosSubSetMem`/`DosSubAllocMem`/`DosSubFreeMem`/`DosSubUnsetMem`
  and 16-bit `DosSubSet`/`DosSubAlloc`/`DosSubFree`, `DosSizeSeg`,
  `DosEditName`, `DosEnumAttribute`, `DosQuerySysState`, `DosCopy`,
  `DosForceDelete`, `DosMkDir2`, `DosReplaceModule`, `DosSetDateTime`,
  `DosAllocHuge`, `DosGetHugeShift`, `DosReadAsync`, `DosWriteAsync`,
  `DosQueryPathInfo` levels 4 and 6, `DosSetFileInfo`/`DosSetPathInfo`
  level 11.

## Missing or incomplete

- `DosFSCtl` (CACHE, CACHEJFS, DEFRAGFS, EXTENDFS, CLRBBLKS),
  `DosSysCtl`, `DosQueryRASInfo`, `DosDumpProcess`,
  `DosSystemService`.
- SYSLEVEL lists nothing yet: re-test now that DASD drive opens exist.
- Extended attributes are not stored: `DosEnumAttribute` reports none and
  EA writes are accepted and dropped.
- Bound message segments (MSGBIND) are not read: programs fall back to the
  message files, so they need OSO001.MSG and friends on disk.
- `DosQuerySysState` reports no semaphores or shared memory.
- AVIO (VioCreatePS and the other Presentation Manager VIO calls) is not
  implemented; keyboard layouts other than US are the NLS work.
- Changing a CD for a different one is not detected (the old volume stays
  mounted); re-inserting the same medium and choosing Retry works.
- Writing to QEMU's virtual FAT drive (the ESP directory) can hang (seen
  with BACKUP); FAT disk images are fine.
- The 32-bit `DosQueryMessageCP` argument layout is unverified (no caller
  found among the tested programs).
