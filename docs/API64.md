# The 64-bit API

rs2 runs 64-bit programs next to OS/2's 16- and 32-bit ones. They use
the OS/2 API (DOSCALLS, and PM through PMWIN and PMGPI) with 64-bit
pointers. This is rs2's own extension: IBM never shipped a 64-bit OS/2.
It is the first step towards self-hosting (building rs2 on rs2, see
[SELF-HOSTING.md](SELF-HOSTING.md)), and it lets programs be written for
rs2 as native 64-bit code.

## The model

| | 64-bit API |
|-|------------|
| CPU mode | long mode, code selector `0x6B` (`gdt::USER_CS64`); SS/DS/ES are the flat `0x53`, FS the thread's 32-bit TIB `0x150B` |
| Calling convention | System V AMD64 (`extern "C"`), for API calls and for callbacks (window procedures) |
| Data model | ULONG, LONG, APIRET, BOOL and the handles (HFILE, HEV, HMTX, HMODULE, HWND, HPS, HAB...) stay 32 bits; pointers (PVOID, PSZ, PFN) and MPARAM/MRESULT are 64 bits |
| Structures | OS/2's, with pointer fields widened to 8 bytes (natural alignment) |
| Executable format | ELF64 shared objects (`ET_DYN`), x86-64 |

So a 64-bit program is an OS/2 program built with 64-bit pointers: the
same calls, ordinals and constants. A 64-bit process is an ordinary rs2
process: CMD starts it with DosExecPgm, it has handles, sessions, pipes
and a current directory, and it can start and be started by 32-bit
programs.

## Executables

A 64-bit EXE or DLL is an x86-64 ELF shared object, position independent,
linked with `userland64/link64.ld`: code and read-only data in one
segment, writable data in the next, each starting on 64 KB (the loader's
objects, as LX objects). rs2's loader (`kernel/src/loader/elfload.rs`)
recognises it by its ELF header; a file named `.EXE` or `.DLL` can be
either format.

- **Imports** are undefined dynamic symbols named as `elf2lx` names
  32-bit imports: `MODULE.ordinal` or `MODULE.Name`. Kernel API modules
  (DOSCALLS, ...) resolve to the gate stubs 32-bit programs call; other
  modules are 64-bit DLLs. Relocations: `R_X86_64_RELATIVE`, `_64`,
  `_GLOB_DAT`, `_JUMP_SLOT` (bound at load time; there is no lazy
  binding).
- **Exports** are the module's defined dynamic symbols, by name.
- **Resources**: a data symbol `RS2_RESOURCES` holds the module's
  resource table (`os2_rc::bind::table64`: "RS2RES", a count, then type,
  ID, offset and size per resource, then the data, in IBM's binary
  formats as in LX modules). A build script compiles the module's `.RC`
  script with `os2_rc::host::compile_table64` into
  `$OUT_DIR/resources.bin` and `os2::resources!()` embeds it;
  DosGetResource (and so WinLoadMenu, WinLoadDlg, WinLoadString,
  WinCreateStdWindow's FCF_MENU...) finds resources there. Named
  resources get IBM RC's numbers for their names (no RT_RESNAMES yet).
- **Program type**: a data symbol `RS2_APPTYPE` (u32) holds the LX-style
  type, 3 for PM (`os2::app_type!(PM)`); without it a program is window
  compatible.
- **64-bit DLLs** are a namespace of their own: a 64-bit module's import
  of PMWIN is the 64-bit PMWIN, not OS/2's. They are found in
  `C:\RS2\DLL64`, then along BEGINLIBPATH and LIBPATH (only ELF64 files
  count there). DosLoadModule from 64-bit code finds 64-bit DLLs; 32-bit
  callers never see them.

The entry point is `_start(hmod, 0, env, cmdline)` (RDI, RSI, RDX, RCX),
entered as if called from the exit stub: returning ends the process with
RAX as the result. Thread 1 has a 1 MB stack growing down over a guard
page.

## Calling the kernel

Imports of kernel modules bind to the same `int 0x99` stubs in the API
gate as 32-bit ones (ARCHITECTURE.md). The dispatcher knows a 64-bit call
by the code selector: the arguments are in RDI, RSI, RDX, RCX, R8, R9 and
then on the stack above the return address, the result goes to RAX, and
the return pops the 8-byte return address. A procedure address from
DosQueryProcAddr is the stub, so calls through it work too.

Most DOSCALLS take only ULONGs and pointers to buffers without pointers,
so their 32-bit implementation serves 64-bit callers as it is (`Ctx::arg`
reads the registers). `kernel/src/api/api64.rs` lists what 64-bit
programs may call:

- calls run unchanged (files, directories, named pipes, memory,
  semaphores, threads, processes, modules, time, NLS, queues);
- calls that return a pointer through an argument (DosAllocSharedMem,
  DosGetNamedSharedMem, DosScanEnv, DosQueryProcAddr, DosGetResource,
  DosSubAllocMem, DosAllocThreadLocalMemory, DosReadQueue, DosPeekQueue):
  the 32-bit code writes the low half, the dispatcher clears the high
  half (what they return is below 4 GB);
- calls with their own 64-bit versions: DosCreateThread (a 64-bit
  parameter, a long-mode thread function returning into the thread exit
  stub), DosGetInfoBlocks (64-bit TIB and PIB), and DosAllocMem,
  DosFreeMem, DosSetMem and DosQueryMem (memory above 4 GB, below);
- refused arguments: extended attribute lists (EAOP2 holds pointers), so
  DosOpen and DosCreateDir need a null `peaop2` and the EA levels of
  DosQueryPathInfo, DosQueryFileInfo, DosFindFirst, DosSetPathInfo and
  DosSetFileInfo return ERROR_INVALID_PARAMETER.

Anything else returns ERROR_CALL_NOT_IMPLEMENTED and is logged once
("not available to 64-bit programs yet"). Not yet: exception handlers
(DosSetExceptionHandler and friends, with a 64-bit CONTEXTRECORD),
DosExitList, muxwait semaphores (SEMRECORD holds an HSEM, a pointer),
DosStartSession (STARTDATA), DosQuerySysState, DosGetMessage, the VIO,
KBD and MOU calls.

### TIB and PIB

DosGetInfoBlocks gives 64-bit programs their own blocks, in a per-process
area at one address in every 64-bit process (`process::info64_base`):

```c
typedef struct _TIB {          /* 40 bytes */
    PVOID pexchain;            /* exception handlers: none yet (0) */
    PVOID pstack, pstacklimit; /* stack base and top */
    PTIB2 ptib2;               /* the 32-bit TIB's TIB2 (same layout) */
    ULONG version, ordinal;
} TIB;
typedef struct _PIB {          /* 40 bytes */
    ULONG pid, ppid, hmte;
    PCHAR pchcmd, pchenv;
    ULONG flstatus, type;
} PIB;
```

### Exceptions

Page faults resolve as for 32-bit code (committing, the stack's guard
page grows it). Any other exception in 64-bit code ends the process
(TC_TRAP) with a SYS3175 line on its console and the registers on the
log. 32-bit code that runs in a 64-bit process (the PM thunks below) has
the usual 32-bit exception handling.

## Memory

A 64-bit process has the OS/2 address space below 4 GB (the arenas of
ARCHITECTURE.md "Memory layout": the program in the private arena at its
link address, 64-bit DLLs in the shared arena, thread stacks in the
private arena) and, above it, the **64-bit arena**:

```text
0x00_0000_0000  the OS/2 arenas (as for 32-bit processes)
0x01_0000_0000  64-bit arena: private objects, bottom up, 64 KB aligned
0x80_0000_0000  end (512 GB, the first page-map entry's range)
```

- **DosAllocMem with OBJ_ANY** from 64-bit code allocates there (an
  object is at most 4 GB - 4 KB, as `cb` is a ULONG). Without OBJ_ANY it
  allocates in the private arena below 512 MB, as for 32-bit code. (So
  OBJ_ANY means "above 4 GB" to 64-bit callers instead of 32-bit "high
  memory": memory anywhere, which only 64-bit code can reach.)
- Pages are reserved, committed or guard pages as below 4 GB, and come
  in on first touch (demand paging, zeroed frames). DosSetMem commits,
  decommits (the frames go back at once), makes guard pages and changes
  the protection; DosQueryMem reports runs of pages and free space
  (PAG_FREE) up to the next object; DosFreeMem releases an object. A
  guard page's first touch commits it and makes the page below the guard
  page (the default action: 64-bit code has no exception handlers yet);
  an access to a reserved page or outside any object ends the process
  (SYS3175). Everything there goes when the process ends.
- 32-bit processes and 32-bit code never get addresses above 4 GB;
  nothing below 4 GB changed.
- The `os2` crate's heap takes memory below 4 GB while there is room and
  then grows into the 64-bit arena (so a large `Vec` lands above 4 GB).

**Kernel calls with buffers above 4 GB.** The kernel reaches user memory
at any address through `Ctx::uptr` (a pointer argument, all 64 bits) and
`uread`/`uwrite` (`AddressSpace::read_bytes64`/`write_bytes64`). Calls
written for it take pointers above 4 GB in every pointer argument:
DosRead, DosWrite (buffer and count), DosGetInfoBlocks, DosAllocMem,
DosFreeMem, DosSetMem, DosQueryMem (`HIGH_OK` in api64.rs). Any other
call given a pointer above 4 GB (api64.rs's `PTR_ARGS` lists which
arguments of each call are pointers) fails with ERROR_INVALID_ADDRESS
before it runs, logged once ("does not take memory above 4 GB yet"), so
32-bit code never sees a truncated pointer; `Ctx::ptr` gives such a
pointer as the invalid 0xFFFFFFFF. A 64-bit thread's stack may be above
4 GB too (arguments beyond the sixth and the return address are read
with 64-bit addresses), and code above 4 GB executing `int 0x99` is not
taken for the gate. More calls move to `uptr` as programs need them.

## Building 64-bit programs

`userland64/` is a Cargo workspace for `x86_64-unknown-linux-gnu`
(`no_std`; only its precompiled `core` and `alloc` are used, never
Linux). Every program and DLL is a `cdylib`; `.cargo/config.toml` links
with rust-lld and `link64.ld` (`-Bsymbolic`, `-z now`, SysV hash table).
The `os2` crate there is the 64-bit API: bindings (`#[link_name =
"DOSCALLS.282"]`), the `entry!` and `app_type!` macros, `println!`, a
heap on DosAllocMem, and the routines the compiler expects (memcpy...).
`cargo xtask build` builds the workspace and puts each program
(`PROGRAMS64` in xtask) on the boot volume as it was linked.

`C:\RS2\HELLO64.EXE` is the smoke test: TIB/PIB, DosQuerySysInfo, the
heap, DosAllocMem, DosScanEnv, threads with a 64-bit parameter, an event
semaphore, a file, DosQueryProcAddr and a call through its result, then
memory above 4 GB: a 2 GB object with OBJ_ANY, 1 GB of it committed and
every page touched, DosQueryMem, a guard page, DosWrite to the screen and
1 MB DosWrite/DosRead to a file with buffers there, a call refusing such
a buffer, decommit, DosFreeMem, and a 600 MB heap block above 4 GB. It
needs 4 GB of memory:

```text
cargo xtask run --mem 4G --headless --timeout 45 --init "OS2\CMD.EXE /K RS2\HELLO64.EXE"
```

## PM for 64-bit programs

PM's shared state (the window tree, classes, queues) lives in shared
memory of the 32-bit PMWIN.DLL as i686 Rust structures, so a 64-bit
PMWIN cannot share it. A 64-bit program's PMWIN (`C:\RS2\DLL64\PMWIN.DLL`,
userland64/pmwin) is therefore a layer over the 32-bit one in the same
process, the way Windows runs 32-bit programs on 64-bit Windows, the
other way round:

- **64 → 32.** Each export (all of the 32-bit PMWIN's, Win* and Gpi*, by
  name) calls the 32-bit function of the same name: `rs2_call32` copies
  the arguments, as 32-bit values, below the 64-bit frame, enters the
  function in compatibility mode with RETFQ (CS 0x5B), and is back in long
  mode by a far jump through [EBX] once the function returns to a 32-bit
  stub (EBX and EBP survive: the callee keeps them). The 32-bit DLL is
  loaded with rs2's private `Dos64LoadModule32` (DOSCALLS 2040), its
  procedures found by DosQueryProcAddr on first use. The generic exports
  are generated from the 32-bit PMWIN's trampoline list (name, argument
  count) and the 32-bit functions' signatures (which arguments are
  pointers, which MPARAMs): `python3 -I tools/scripts/gen_pm64.py` writes
  `userland64/pmwin/src/thunks.rs`.
- **Memory above 4 GB** is out of the 32-bit PMWIN's reach, and there are
  no bounce buffers (a buffer's size is not known in general): a pointer
  argument above 4 GB, or an MPARAM that is not a 32-bit value (zero- or
  sign-extended, so MPFROMLONG(-1) passes), makes the call fail with 0
  (FALSE, NULLHANDLE, MRESULT 0) and PMERR_INVALID_PARAMETERS (0x1208)
  for WinGetLastError, instead of being truncated. The same holds for the
  converted calls (QMSG parameters, window procedures' MPARAMs, window
  words set with WinSetWindowPtr, dialog parameters, DEVOPENSTRUC). Keep
  what PM gets (strings, structures, window data) below 4 GB: DosAllocMem
  without OBJ_ANY, the stack, or the heap while it is below 4 GB.
- **32 → 64.** A 64-bit window procedure given to PMWIN (WinRegisterClass,
  WinSubclassWindow, WinSetWindowPtr(QWP_PFNWP), WinDlgBox, WinLoadDlg,
  WinCreateDlg) is replaced by a 32-bit stub (`mov eax, slot; jmp far` to
  `rs2_entry64`), one per procedure, in a page of executable memory; the
  stub's long-mode side calls the procedure with System V arguments
  (MPARAMs zero-extended) and returns to PMWIN with RETFQ to a 32-bit RET.
  A 32-bit procedure PMWIN hands out (WinSubclassWindow's previous
  procedure, QWP_PFNWP, CLASSINFO) becomes a 64-bit stub that calls it
  through `rs2_call32`; a stub of our own gives its procedure back, so
  pointers compare equal.
- **Structures with pointers** are converted: QMSG (WinGetMsg,
  WinPeekMsg, WinDispatchMsg, WinTranslateAccel, WinCallMsgFilter),
  CLASSINFO, DEVOPENSTRUC (DevOpenDC), and WM_CREATE's CREATESTRUCT (a
  64-bit copy that remembers the original, so WinDefWindowProc gets that
  back).
- Not yet: hooks (WinSetHook fails), message parameters that point to
  structures with pointers other than CREATESTRUCT (container records,
  WM_QUERYWINDOWPARAMS' WNDPARAMS...), PMSHAPI and the other PM DLLs.

`C:\RS2\PMDEMO64.EXE` (userland64/pmdemo64) is a 64-bit PM program: a
frame window with a menu from its resources (File: About, a dialog
template; Exit), drawing its string table's greeting and shapes with GPI
and counting timer ticks. `PMDEMO64 /SELFTEST` checks the layer (window
and dialog procedures both ways, CREATESTRUCT, QMSG, CLASSINFO,
QWP_PFNWP, subclassing the frame, WinSendMsg results, the menu, string
and dialog resources, refusing pointers above 4 GB, the message loop,
painting) and prints PASS/FAIL and
`pmdemo64 selftest done: N failure(s)`:

```text
cargo xtask run --headless --timeout 45 --init "RS2\PMDEMO64.EXE /SELFTEST"
```
