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
| EAUTIL.EXE | works | Files have no EAs (EAs are not stored yet). |
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
| PSSEMS.EXE | works | Shared event and mutex semaphores with their openers. |
| REPLACE.EXE | works | Add (`/A`) and replace. |
| SETBOOT.EXE | works | `/Q`: "Boot Manager is not installed". |
| SORT.EXE | works | |
| STRACE.EXE | works | Usage text (loads `DOSCALL1` by name). |
| SYSLEVEL.EXE | works | Finds `SYSLEVEL.OS2` and lists the component, version and CSD levels. |
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
| EXEHDR.EXE | partial | NE files are dumped completely. On every LX file (ALP.EXE as well as the LINK386 output) it prints the header and object table, then its C runtime reports an internal error whose text is in DDE4.MSG (not on the media) and faults freeing a heap block whose header holds the text EXE (the VAC heap signature check at 0x2907D). No failing API call precedes it (every DosRead/DosSetFilePtr result checked); not resolved. |
| IMPLIB.EXE | works | |
| MAPSYM.EXE, MARKEXE.EXE | works | |
| MKMSGF.EXE, MSGBIND.EXE | works | Usage text only. |
| RC.EXE, RC16.EXE, RCPP.EXE | works | Usage text only. |
| GENCAT.EXE, RPCGEN.EXE, FWDSTAMP.EXE | works | Usage text only. |
| PACK.EXE, PACK2.EXE | partial | PACK.MSG is not on the media. |
| WORMS.EXE (sample) | works | Mouse pointer and clicks through MOUCALLS (Exit by clicking). |
| TIMER0.EXE (sample) | works | Reports that the TIMER0$ sample driver is missing. |

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
  skipped the exit list; DosSleep ended early on stray wake-ups.

## Missing or incomplete

- Extended attributes are not stored: `DosEnumAttribute` reports none and
  EA writes are accepted and dropped.
- Bound message segments (MSGBIND) are not read: programs fall back to the
  message files, so they need OSO001.MSG and friends on disk.
- File locks (`DosSetFileLocks`) are advisory: they conflict with each
  other but reads and writes are not checked against them.
- The `DosProtect*` functions hand out a file handle lock ID but do not
  check it.
- The 32-bit `DosQueryMessageCP` argument layout is unverified (no caller
  found among the tested programs).

## Kernel API (DOSCALLS) coverage

The gate logs every unimplemented call once (`[unimplemented API ...]`).
The table below comes from `lxdump imports` over every executable and DLL
on the media (work/media, work/unpacked), counting the files that import
each DOSCALLS ordinal rs2 does not implement (DOSCALL1.DLL itself and
drivers left out).

Added in this round: `DosSendSignalException`,
`DosAcknowledgeSignalException` (further signals of a kind are held until
acknowledged), `DosSetSignalExceptionFocus`, `DosEnterMustComplete`/
`DosExitMustComplete` (signals held), `DosAllocThreadLocalMemory`/
`DosFreeThreadLocalMemory`, `DosCreateThread2`, `DosVerifyPidTid`,
`DosQueryModFromEIP`, `DosQueryThreadContext`, `DosDumpProcess` (a text
dump \PDUMP.nnn), `DosSuppressPopUps`, `DosSysCtl` and
`DosSystemService` (documented errors), `DosQueryRASInfo`,
`DosQueryABIOSSupport`, `DosPerfSysCall` (CPU utilisation),
`DosErrClass`, 16-bit `DosQSysInfo`, `DosGet/SetProcessorStatus`, spin
locks, `DosTestPSD`, `DosShutdown` (flushes open files),
`DosTmrQueryTime` interpolated from the TSC, `DosFSCtl` (generic
functions and the HPFS cache controls) and `DosFSAttach`,
`DosSetFileLocks`/`DosSetFileLocksL`/16-bit `DosFileLocks`/
`DosCancelLockRequest`, the 32-bit `DosProtect*` functions,
`DosAliasMem`, `DosQueryMemState`, `DosReallocHuge`, `DosLockSeg`/
`DosUnlockSeg`, `DosSetMaxFH`/`DosSetRelMaxFH`, 16-bit
`DosFSRamSemRequest`/`DosFSRamSemClear`, `DosSendSignal`,
`DosFlagProcess`, `DosSysTrace`, the `DosPM*` semaphore variants;
`DosKillProcess` with DKP_PROCESSTREE; `DosQuerySysState` now reports
16- and 32-bit semaphores and shared memory; `DosCloseEventSem`/
`DosCloseMutexSem` return ERROR_SEM_BUSY while threads wait; threads
killed or ending with their process call their exception handlers with
XCPT_ASYNC_PROCESS_TERMINATE / XCPT_PROCESS_TERMINATE first.

Not implemented (importing files on the media):

| Ordinals | Functions | Used by |
|----------|-----------|---------|
| 308-310, 266-268, 373-376 | DosOpenVDD, DosRequestVDD, DosCloseVDD, DosQuery/SetDOSProperty | 29 files: Win-OS/2 and VDM support (SEAMLESS, PMVDMH, WINCFG, display drivers). Needs DOS sessions. |
| 157 | DosCallback | 19: 16-bit display drivers, PMSPL, PMMERGE (ring-2 code calling ring 3). |
| 547, 115-117, 262, 293, 294, 546, 866-872, 577-581 | THK32_UNITHUNK, THK32* thunk helpers, HT16/HT32_STARTUP/CLEANUP | 12 (THK32_UNITHUNK: LIBUNI, UCONV, SHPIINST, VGAGRADD); the rest PM DLLs. |
| 88 | 16-bit DosSystemService | 7: HARDERR, SYSINST1, VIDEOPMI, PMMERGE. Needs a signature; the 32-bit entry (995) is answered. |
| 216, 217, 419 | DosLocalInfo, DosGlobalInfo, DosTIB | 4: display drivers, PMMERGE. |
| 682-717, 845-865, 745-769, 771-844, 853-857 | Session manager, keyboard, video and mouse shell internals (DosSm*, Kbd*/Vio* registration, BVS/BKS/BMS, QH*, SM* arrays) | HARDERR, PMMERGE, BVH*.DLL: the parts of the shell rs2 replaces. |
| 440-442, 437-439 | DosOpen/Reset/CloseChangeNotify | PMWP (Workplace Shell). |
| 582, 584 | DosQueryHeaderInfo | jvm.dll |
| 316, 165, 364-367, 422, 430-436 | DosDynamicTrace, performance counters, LOG* | DTRACE, TRACE, MPCPUMON, FFST. |
| 317, 12 | DosDebug, DosPTrace | debuggers (PMMERGE imports DosDebug). |
| 359 | 16-bit DosQueryModFromCS | PMMERGE (no signature yet). |
| 51, 69, 160 | DosCLIAccess, DosPortAccess, DosR2StackRealloc | I/O privilege: not available to rs2 programs. |
| 996, 990 | DosListIO, DosListIOL | no importer on the media. |
| 246, 247, 297, 358, 377, 414, 416, 548, 563, 564, 592, 594, 600, 649, 878, 875, 596, 574, 575, 585, 588 | DosRawRead/WriteNPipe, Dos32AllocProtectedMem, DosQueryPageUsage, DosProfile, thread affinity, DOSCALL1-internal entries (exception dispatcher, DLL term dispatch, ...), DosCloseMessageFile, PM 16-bit semaphore helpers, monitor I/O | at most one importer each, mostly PMMERGE or DOSCALL1 itself. |
| 186-189, 214, 420, 421, 573, 587, 622-635 | 16-bit DosFileIO, DosFindNotify*, DosMaxPathLen, DosTmrQuery*, DosQueryResourceSize, DosQueryProcType, DosProtect* | only DOSCALL1.DLL imports them (its 16-bit thunks); the 32-bit versions are implemented. |

## Stress test

`RS2\STRESS.EXE` (userland/stress) runs sections `threads sems kill signals
xcpt procs mem files ipc misc more` (all by default, or the ones named on the
command line) and prints PASS/FAIL per check; `STRESS ctrlc` waits in a
pipe read for Ctrl+C. It uses `RS2API.100` (`Rs2QueryCounters`: free
pages, live threads, processes and process objects, semaphores, shared
memory objects, open files, queues, timers, kernel heap use, file locks)
to check that nothing leaks, and prints the counters after each section.
All checks pass under rs2's CMD and IBM's CMD.EXE.
