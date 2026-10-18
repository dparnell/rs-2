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
| REXXUTIL.DLL | partial | `SysLoadFuncs` works; tested `SysCls`, `SysCurPos`, `SysTextScreenRead`, `SysTextScreenSize`, `SysSleep`, `SysGetKey`, `SysFileTree`, `SysFileDelete`, `SysMkDir`, `SysRmDir`, `SysDriveMap`, `SysTempFileName`, `SysBootDrive`, `SysVersion`, `SysOS2Ver`, `SysSearchPath`, `SysPutEA`, `SysGetEA` (on FAT16, FAT32 and HPFS). `SysIni` fails (needs PMSHAPI/PMWIN). |
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
| EAUTIL.EXE | works | Splits a file's EAs into a hold file (`/S`) and joins them back (`/J`) on FAT16, FAT32 and HPFS (16-bit `DosEnumAttribute`, `DosQFileInfo` level 3, `DosSetFileInfo` level 2). |
| EJECT.EXE | works | Usage text. |
| EXECMODE.EXE | works | Usage text. |
| FIND.EXE | works | |
| HELPMSG.EXE | works | Message and explanation from OSO001.MSG/OSO001H.MSG. |
| LABEL.COM | works | Shows and sets the volume label. |
| MAKEINI.EXE | works | Usage text only. |
| MODE.COM | works | `MODE CO80`. |
| MORE.COM | works | |
| PATCH.EXE | works | Usage text only. |
| PSTAT.EXE | works | `/C` processes and threads, `/L` libraries (via `DosQuerySysState`). |
| PSFILES.EXE | works | |
| PSSEMS.EXE | partial | Runs; no semaphores are reported by `DosQuerySysState`. |
| REPLACE.EXE | works | Add (`/A`) and replace. |
| SETBOOT.EXE | works | `/Q`: "Boot Manager is not installed". |
| SORT.EXE | works | |
| STRACE.EXE | works | Usage text (loads `DOSCALL1` by name). |
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
| EAS.EXE (sample) | not tested | The EA editor sample is a Presentation Manager program. |

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
- Extended attributes are stored: on FAT12/16 in `EA DATA. SF` as OS/2
  does, on FAT32 in FAT32.IFS-style `NAME EA. SF` files, on HPFS in the
  F-node or outside it (see [formats/fat-ea.md](formats/fat-ea.md) and
  [formats/hpfs.md](formats/hpfs.md)). Query/set with EAOP2 (and the
  16-bit EAOP) in `DosQueryPathInfo`/`DosQueryFileInfo` levels 2-4,
  `DosSetPathInfo`/`DosSetFileInfo` level 2, EAs at creation in `DosOpen`,
  `DosOpenL`, `DosOpen2`, `DosCreateDir`, `DosMkDir2`, `DosFindFirst`
  levels 2 and 3, `DosEnumAttribute`; `DosCopy` copies EAs and `DosMove`
  keeps them; CHKDSK checks EA references.

## Missing or incomplete

- `DosFSCtl` (CACHE, CACHEJFS, DEFRAGFS, EXTENDFS, CLRBBLKS),
  `DosSysCtl`, `DosQueryRASInfo`, `DosDumpProcess`,
  `DosSystemService`.
- SYSLEVEL lists nothing yet: re-test now that DASD drive opens exist.
- Extended attributes: `ERROR_NEED_EAS_FOUND` is only reported by
  `DosCopy` (critical EAs to a target without EA support); `DosOpen` does
  not refuse files with critical EAs to programs that are not EA-aware.
  The 16:16 EAOP2 of `Dos16OpenL` is assumed to point at an FEA2LIST
  (unverified). IBM's CHKDSK could not check rs2-written FAT volumes
  (`UFAT.DLL` is not on the media); the FAT32 EA flag values follow
  FAT32.IFS from memory and are unverified against a real disk.
- Bound message segments (MSGBIND) are not read: programs fall back to the
  message files, so they need OSO001.MSG and friends on disk.
- `DosQuerySysState` reports no semaphores or shared memory.
- Writing to QEMU's virtual FAT drive (the ESP directory) can hang (seen
  with BACKUP); FAT disk images are fine.
- The 32-bit `DosQueryMessageCP` argument layout is unverified (no caller
  found among the tested programs).
