# Native Win32

Stage 1 (console programs) works; stages 2 and 3 are plans.

rs2 runs Windows 3.x programs as ordinary OS/2 processes on its own
WIN16.DLL (docs/WIN16.md). This plan does the same for 32-bit Windows
programs (PE32 executables for the Win32 API, i386): their KERNEL32,
USER32, GDI32 and the other system DLLs' imports go to rs2 DLLs written
in Rust on rs2's kernel, PM and GPI. No Microsoft files are involved, and
the rule from docs/WIN16.md holds: rs2 ships only its own code, and
Windows programs are tested locally.

IBM went down this road twice. **Win32s** let Windows 3.1 run a subset of
Win32. **Open32** (PMWINX.DLL, which rs2 has as a stub that forwards the
registry calls: docs/PM.md, "Open32 and the registry") was a source-level
porting layer: programs were rebuilt against it as OS/2 programs. Neither
loaded PE files. The Odin project (Odin32) did load them on OS/2, with a
PE loader and Win32 DLLs over PM, and is worth reading for where it ran
into trouble.

## Why it is different from Win16

Win32 is easier than Win16 in its machine model and much bigger in its
API.

Easier, because a Win32 program is a flat 32-bit program, as an LX
program is. rs2 already runs those natively, so none of the Win16
machinery is needed: no selectors or tiles, no DGROUP or local heap, no
prologue patching, no INT reflection, no 16-bit thunks, no huge blocks.
Pointers in messages are flat pointers.

Bigger, because:

- **The API surface.** KERNEL32 alone exports about a thousand
  functions; a typical program also uses ADVAPI32, COMCTL32, SHELL32,
  COMDLG32, OLE32, WINMM, Winsock and a C runtime DLL.
- **Threads.** Win32 programs are preemptively multithreaded, and each
  thread can own windows and its own message queue. WIN16 runs one task
  per process with one thread and no locks; that does not carry over.
- **Unicode.** Most calls come in an ANSI (A) and a wide (W) form, the W
  form in UTF-16. Programs built for NT use the W forms throughout.
- **Programs read the system's structures directly**: the TEB through FS,
  the PEB through the TEB, the loader's module lists through the PEB.

## Stages

| Stage | What | Done when |
|-------|------|-----------|
| 1 | Console programs: the PE loader, TEB/PEB, SEH, TLS, KERNEL32's core, the console on VIO, a C runtime DLL | Open Watcom and MinGW console programs (hello, file I/O, threads, exceptions, `getenv`, `system`) run from CMD, with exit codes |
| 2 | GUI programs: USER32 and GDI32 on a core shared with WIN16, resources, COMDLG32 (USER32, GDI32, dialogs and controls done: "Stage 2 as built") | Open Watcom's and MinGW's Win32 GUI samples; Win32s-era programs |
| 3 | The compatibility tail: COMCTL32, SHELL32, WINMM, Winsock, OLE, printing, the rest of ADVAPI32 | Chosen real programs (an editor, an installer, a game) |
| later | Win64 (PE32+) on rs2's long-mode path (docs/API64.md) | |

The test programs are built with Open Watcom (`wcl386 -bt=nt`, on the
developer disk: docs/DEVDISK.md) and, for MSVCRT, MinGW-w64 (i686), as
WIN16's are with Open Watcom's `-bt=windows` (docs/WIN16.md, "Building
the test programs"). `W32TEST.EXE`, like W16TEST, checks each stage's
calls.

## Stage 1: the kernel and the console

### Status

| What | State |
|------|-------|
| PE32 programs and DLLs loaded (relocated, imports bound, the system DLLs' names mapped) | works |
| The PEB, a TEB per thread (FS), the program's stack from its header, growing through a guard page | works |
| DllMain (process and thread attach and detach), `.tls` blocks and callbacks, TlsAlloc slots | works |
| Structured exceptions: CPU exceptions, RaiseException, `_try`/`_except`/`_finally`, RtlUnwind, EXCEPTION_CONTINUE_EXECUTION with a changed context, the unhandled exception filter | works |
| Files, directories, searches, paths, drives | works |
| VirtualAlloc/Free/Query/Protect, heaps, GlobalAlloc/LocalAlloc | works |
| Threads, events, mutexes, semaphores, critical sections, waits on one or many, Interlocked | works |
| CreateProcess, waiting for a child, its exit code; the environment and the command line | works |
| LoadLibrary, GetProcAddress, GetModuleHandle, GetModuleFileName | works (FreeLibrary does not unload) |
| Code pages, the string calls, times and clocks, versions, system information | works |
| The console: colours, the cursor, the screen buffer calls, key input | works (no mouse input, no Ctrl+C handlers yet) |
| The C runtime (MSVCRT.DLL, MSVCR70-120, UCRTBASE and the UCRT's `api-ms-win-crt-*` DLLs) as W32CRT: stdio, file descriptors, printf and scanf, math, time, setjmp, threads, spawn | works (MinGW-w64 programs, C and C++ with exceptions) |
| The PEB's loader lists (Ldr) and process parameters | not yet |

`W32TEST.EXE` passes its 145 checks (`cargo xtask run --headless
--timeout 120 --init "RS2\W32TEST.EXE"` ends with `w32test done: 0
failure(s) in 145 checks`); `HELLO32.EXE` prints its arguments and ends
with exit code 3; the MinGW-w64 programs `MTEST.EXE` (UCRT) and
`MTESTM.EXE` (MSVCRT.DLL) pass their 43 checks of the C runtime, and
`MCPP.EXE` its 7 of C++ (static constructors, the STL, exceptions).

### The PE loader

`kernel/src/loader/peload.rs`, beside `lxload.rs`, `neload.rs` and
`elfload.rs`, on the PE parser in `crates/os2-exe` (`pe.rs`;
`os2_exe::detect` reports `Format::Pe`, and `cargo run -p os2-exe
--example peinfo -- FILE` prints a module's headers, sections, imports
and exports on the host).

A PE module is **one object**: its headers and sections at their RVAs,
readable, writable and executable. A program goes into its process's
private arena at its preferred base (Open Watcom and MSVC link at
0x400000, often without relocations); if that is taken and the image has
relocations, it goes elsewhere and is relocated (the in-memory header's
ImageBase is updated). A DLL goes into the shared arena at its preferred
base if that is free, else anywhere, relocated; all of it is per-process
instance data, as sections are 4 KB aligned and the arena's objects 64
KB. Section protections are not applied yet.

**Imports** are bound when the module is loaded. `WIN32_MODULES`
(`kernel/src/loader/mod.rs`) maps the system DLLs to rs2's: `KERNEL32` →
`W32KERN`, `NTDLL` → `W32NTDLL`, `USER32` → `W32USER`. These are LX DLLs
that export Windows' names. Any other DLL is looked for in the program's
directory, then along the LIBPATH. An import that does not resolve is
logged (`loader: X.EXE: import KERNEL32.DLL.Foo not found (in
W32KERN)`) and the load fails with ERROR_PROC_NOT_FOUND, as on Windows.
Exports (by name and by ordinal, forwarders included) are the module's
OS/2 exports, so DosQueryProcAddr, and with it GetProcAddress, finds
them. A program also gets W32NTDLL, whose `Rs2W32Start` the kernel starts
it at.

The kernel runs no initialisation for PE modules (their `init` is none):
DllMain and TLS are W32NTDLL's business. `DosQueryAppType` reports
FAPPTYP_32BIT | FAPPTYP_WINDOWSPROT31 (and FAPPTYP_DLL); the subsystem
makes a GUI program a PM process and a console program a text one. A
file with a PE signature that is not a PE32 i386 image is
ERROR_INVALID_EXE_SIGNATURE, as OS/2 says of all of them.

### The PEB and the TEBs

A Win32 process gets a **Win32 area** of 0x3FD pages
(`task/process.rs`, `W32_AREA`, reserved at exec): the PEB in the first
page, the TEB of thread n in page n, committed when the thread is made.
The kernel fills in the PEB's ImageBaseAddress (0x08),
NumberOfProcessors (0x64) and the version (0xA4: 5.1, build 2600,
VER_PLATFORM_WIN32_NT).

A Win32 thread's **TEB is its OS/2 TIB**: `Thread::tib` is the TEB, FS
(`TIB_SEL`) points at it, and DosGetInfoBlocks returns it. The first
words agree:

| Offset | Win32 TEB | OS/2 TIB |
|--------|-----------|----------|
| 0x00 | ExceptionList | tib_pexchain |
| 0x04 | StackBase (the top) | tib_pstack (OS/2: the bottom) |
| 0x08 | StackLimit (the lowest committed page) | tib_pstacklimit (OS/2: the top) |
| 0x0C | SubSystemTib: here OS/2's tib_ptib2, in the process information area as for any thread | tib_ptib2 |
| 0x10, 0x14 | FiberData, ArbitraryUserPointer: OS/2's version and ordinal | tib_version, tib_ordinal |
| 0x18 | Self | |
| 0x20 | ClientId (process, thread) | |
| 0x2C | ThreadLocalStoragePointer (W32NTDLL's `.tls` blocks) | |
| 0x30 | ProcessEnvironmentBlock | |
| 0x34 | LastErrorValue | |
| 0xE10 | TlsSlots[64] | |

The stack fields are Windows' (rs2's own DLLs do not read them, except
HELPMGR, which a Win32 program does not use). When the stack grows
through its guard page the kernel moves StackLimit down
(`api/win32.rs`, `stack_grew`): compilers' stack probes compare with it.
Thread 1's stack is the header's reserve with its commit committed at
the top and a guard page under it; CreateThread's threads get a
committed stack of at least 1 MB.

### Exceptions

The kernel does not walk a Win32 process's handlers: Win32 handlers
take another context record and return other values than OS/2's. As NT
does with KiUserExceptionDispatcher, it hands every CPU exception of a
Win32 process (after a debugger's first chance) to a **dispatcher in
user mode**, W32NTDLL's, which registered itself with `Rs2W32Init`
(RS2API 430). The kernel puts an EXCEPTION_RECORD and a CONTEXT (i386,
0x2CC bytes; control, integer and segment registers) 512 bytes below the
thread's stack pointer and calls the dispatcher with them.

The exception codes are NT's, which are not OS/2's beyond the access
violation (0xC0000005, with NT's parameters: 0 read, 1 write, 8
execute): integer divide by zero is 0xC0000094 (OS/2: 0xC000009B),
illegal instruction 0xC000001D (0xC000001C), breakpoint 0x80000003 (its
address the INT 3's, as Windows reports it), single step 0x80000004, the
floating point ones 0xC000008D-0xC0000093.

The dispatcher (`userland/w32ntdll`) walks the registration records from
FS:[0], calling each handler as `(record, frame, context, dispatcher
context)` through a shim that keeps EBX, ESI, EDI and EBP and the stack
whatever the handler does. ExceptionContinueExecution (0) resumes in the
context through `Rs2W32Continue` (431, NtContinue); ExceptionContinueSearch
(1) goes on. With no handler left the unhandled exception filter runs
(SetUnhandledExceptionFilter); EXCEPTION_EXECUTE_HANDLER ends the process
with the exception code, EXCEPTION_CONTINUE_EXECUTION resumes, and
EXCEPTION_CONTINUE_SEARCH (also W32KERN's UnhandledExceptionFilter
without a filter) goes to `Rs2W32Unhandled` (432): the kernel's default
action, the trap screen and POPUPLOG as for an OS/2 program, with the
OS/2 code.

RaiseException is W32NTDLL's `Rs2W32Raise`, which captures the caller's
registers (as after its return) and runs the same dispatcher.
**RtlUnwind** calls the handlers of the frames above its target with
EXCEPTION_UNWINDING (and EXCEPTION_EXIT_UNWIND for a whole unwind),
takes each off the chain and returns to its caller with the return
value, as Windows on i386 does. Signals and the kernel's own exceptions
(DosKillThread, a guard page) take the default action in a Win32
process: its handlers never see OS/2's.

Not yet: nested-exception frames (an exception in a handler is
dispatched afresh), vectored handlers, the floating point state in the
CONTEXT.

### W32NTDLL, W32KERN, W32USER

`userland/w32ntdll` → `\OS2\DLL\W32NTDLL.DLL`:

- `Rs2W32Start(entry, base, peb)`, where a program starts: it registers
  the dispatcher, asks the kernel for the process's PE modules
  (`Rs2W32Modules`, RS2API 433: image bases and module handles,
  dependencies first), gives each with a TLS directory its index and
  thread 1 its block, calls the DLLs' entry points with
  DLL_PROCESS_ATTACH, and then the program's entry point; when that
  returns, the DLLs get DLL_PROCESS_DETACH and the process ends with its
  value. A DLL whose DllMain refuses ends it with
  STATUS_DLL_INIT_FAILED.
- `Rs2W32ThreadAttach`/`Detach`, `Rs2W32ProcessDetach`,
  `Rs2W32InitModules` (for LoadLibrary), `Rs2W32HmteOfBase` and
  `Rs2W32BaseOfHmte`, the unhandled filter; RtlUnwind, RtlRaiseException,
  RtlCaptureContext, RtlZeroMemory/MoveMemory/FillMemory, RtlGetVersion,
  NtCurrentTeb.

`userland/w32kern` → `\OS2\DLL\W32KERN.DLL`, KERNEL32 (about 300 calls),
by area:

| File | What | On |
|------|------|----|
| handles.rs | the handle table (HANDLE = (index + 1) × 4: files, searches, waitable objects); the standard handles start as OS/2's 0, 1, 2 | |
| file.rs | CreateFile (dispositions, share modes, CONIN$/CONOUT$; handles not inherited unless asked), Read/WriteFile, SetFilePointer, sizes, times, attributes, Find*File, Delete/Move/CopyFile, directories, GetFullPathName, temporary files, drives, free space | DosOpen, DosQueryPathInfo, DosFindFirst... |
| mem.rs | VirtualAlloc/Free/Query/Protect, IsBad*Ptr; heaps; GlobalAlloc and LocalAlloc (handles are pointers) | DosAllocMem, DosSetMem, DosQueryMem; a reservation at a given address through `Rs2W32AllocAt` (RS2API 434) |
| sync.rs | threads, waits, events, mutexes, semaphores (named ones shared within the process), critical sections, TLS slots, Interlocked | waitable objects under one lock; a waiting thread waits on its own OS/2 event semaphore, which whoever changes an object posts |
| process.rs | the command line (OS/2's as Windows' one string), the environment (a copy, given to children), CreateProcess (search order, STARTF_USESTDHANDLES, a current directory), exit codes, modules | DosExecPgm, DosWaitChild (children are looked at every 20 ms while waited for), DosLoadModule, DosQueryProcAddr |
| text.rs | code pages (ANSI and OEM are the process's OS/2 page, 437 or 850, so its bytes need no translation; UTF-16 through that page's table; UTF-8), lstr*, CompareString, LCMapString, GetStringType, GetLocaleInfo (US English) | |
| time.rs | system and local time, FILETIME conversions, time zone, tick count, performance counter | DosGetDateTime, DosTmrQueryTime |
| console.rs | the console | VIO and KBD (16-bit, through os2::vio::call16) |
| misc.rs | versions (XP), system information, the Windows directory (`\WIN32` on the boot drive), errors, FormatMessage, OutputDebugString (to the kernel's log) | |

Module handles are image bases for PE modules and OS/2 module handles
for rs2's DLLs (GetModuleHandle("KERNEL32") is W32KERN's handle, which
GetProcAddress takes). Error codes are OS/2's, which are Windows' for
most numbers.

`userland/w32user` → `\OS2\DLL\W32USER.DLL`: USER32 and GDI32 (stage
2: "Stage 2 as built"); for a console program, MessageBox writes to
standard error (it has no PM queue).

**Stack alignment.** Windows programs (as OS/2 ones) keep the stack
4-byte aligned, and Rust code for i686 assumes 16: its SSE stores to the
stack fault on a misaligned one. Every exported stdcall function is
therefore reached through a thunk (`src/thunks.rs`, made with
`exports.def` by `tools/scripts/gen_w32_exports.py` from the functions'
signatures) that realigns the stack, copies the arguments and calls it.
Run the script after adding or changing an exported function;
`exports.extra` holds forwarders and fixed ordinals. Callbacks into
Windows code go through a shim that saves the registers and restores the
stack (`rs2_call4`).

### The console

The console is the session's screen. Text goes out with DosWrite while
the colour is the session's; in another colour (SetConsoleTextAttribute)
through VioWrtCharStrAtt, with control characters through VioWrtTTY.
The screen buffer calls (the cursor, filling characters and attributes,
reading and writing cells, scrolling) are VIO's on the whole screen; key
input (ReadConsoleInput, PeekConsoleInput, GetNumberOfConsoleInputEvents)
is KbdCharIn and KbdPeek, with virtual key codes for the letters,
digits, cursor keys, function keys and the editing keys. ReadFile and
ReadConsole on the console read a line with OS/2's line editing.

Not yet: mouse input, Ctrl+C and Ctrl+Break handlers
(SetConsoleCtrlHandler is accepted and does nothing), the window title,
several screen buffers.

### The C runtime (W32CRT)

`userland/w32crt` → `\OS2\DLL\W32CRT.DLL` is the C runtime of every
Windows program that does not carry its own: the loader maps MSVCRT,
MSVCR70 to MSVCR120, UCRTBASE and the UCRT's API sets (`api-ms-win-crt-
runtime-l1-1-0` and the others) to it. It exports both runtimes' names
(about 600): the old one's start-up (`__getmainargs`, `_initterm`,
`__p__iob`, `_iob` and its other variables as data) and the UCRT's
(`_configure_narrow_argv`, `__acrt_iob_func`, `__stdio_common_vfprintf`
and the other `__stdio_common_*`). Functions are cdecl; those with
variable arguments are shims (`exports.extra`, `NAME=va TARGET K`) that
pass the address of the rest to the v-form.

| File | What |
|------|------|
| start.rs | the arguments (split by Windows' rules) and the environment, `_initterm`, atexit and the DLL onexit tables, exit, abort, signals, the C locale |
| mem.rs | malloc and friends (16-byte aligned), `_aligned_malloc` |
| string.rs | strings, wide strings, the character class table (`_pctype`), number conversions, qsort, bsearch, rand (Microsoft's generator) |
| fmt.rs | printf (narrow and wide; three exponent digits for the old runtime, two for the UCRT unless its options say otherwise), scanf, strtod (Rust's exact conversions) |
| io.rs | file descriptors over Windows handles; text mode (CR LF, Ctrl+Z) |
| stdio.rs | FILE streams: MSVCRT's FILE layout, stdin/stdout/stderr the first three of `_iob` (old `getc` macros reach `_filbuf`); line buffering on a terminal |
| fs.rs | remove, rename, `_stat` in all six layouts, `_findfirst` in all four, directories, `_fullpath`, `_splitpath` |
| math.rs | <math.h> on the x87 unit (core has only `abs`), the FPU control calls |
| time.rs | time, localtime and gmtime (32- and 64-bit), mktime, strftime, clock |
| misc.rs | setjmp/longjmp (longjmp runs the `__finally` blocks it leaves through RtlUnwind), `_beginthread(ex)`, system and `_spawn*`/`_exec*` (CreateProcess), the environment, conio |

`abort` writes its callers' return addresses to the kernel's log
(`w32crt: abort, callers ...`): look them up in the program's map.

MinGW-w64's i686 GCC uses SJLJ exceptions and links winpthreads, which
needs the program's own TLS callbacks to run before its entry point and
`DuplicateHandle(GetCurrentThread())` to work for thread 1: both do.

### Building the test programs

`userland/win32test/build.sh` builds them with Open Watcom v2's NT target
(`wcl386 -bt=nt`), installed as for the Win16 programs (docs/WIN16.md,
"Building the test programs"), and with MinGW-w64 (`brew install
mingw-w64`: `i686-w64-mingw32-gcc`, which links against the UCRT unless
told otherwise); `cargo xtask build` puts them on drive C: as
`C:\RS2\*.EXE`:

- `HELLO32.EXE`: the hello world; prints its arguments and PATH, ends
  with exit code 3.
- `W32DLL.DLL`: a DLL with a DllMain that counts its calls, exporting a
  function, a variable and a function that finds its own base.
- `W32TEST.EXE`: the checks, in groups BASIC FILE MEM THREAD SEH PROC
  STR TIME CON DLL (`W32TEST GROUP...` runs some); it imports W32DLL
  (its import library from the DLL) and starts HELLO32.
- `MTEST.EXE`, `MTESTM.EXE`: the C runtime's checks (`mtest.c`):
  arguments, environment, strings, conversions, printf and scanf, math,
  files in text and binary mode, `_stat`, time, setjmp, threads, wide
  characters; MTEST imports the UCRT's DLLs, MTESTM MSVCRT.DLL (linked
  against `libmsvcrt-os.a`). Both end with `mtest done: 0 failure(s) in
  43 checks`.
- `MCPP.EXE`: C++ (`mcpp.cpp`, static libstdc++): static constructors,
  std::string, vector, map, unique_ptr, exceptions.
- `GUI32.EXE`: stage 2's checks (`gui32.c`, `gui32.rc`): a MinGW GUI
  program (`-mwindows`) with a menu, accelerators, strings, an icon and
  a dialog in its resources. See "Stage 2 as built".

A load that fails names the import in the kernel's log
(`loader:` lines); `apitrace=W32TEST` on the kernel command line traces
the program's OS/2 calls, among them W32KERN's on its behalf.

## Stage 2: windows on a core shared with WIN16

### Stage 2 as built

| Part | State |
|------|-------|
| USER32: classes, windows, messages (A and W), painting, timers, properties | done |
| GDI32: DCs, objects, text, shapes, bitmaps, DIBs, DIB sections, regions, palettes, fonts | done (no paths or world transforms; no printers) |
| Menus and accelerators (PE resources, MENUEX, MENUITEMINFO) | done |
| Dialogs (DLGTEMPLATE, DLGTEMPLATEEX) and BUTTON, STATIC, EDIT, LISTBOX, COMBOBOX, SCROLLBAR | done |
| The clipboard (text), the caret, icons and cursors from resources | done |
| MSIMG32 (AlphaBlend, TransparentBlt, GradientFill) | done, on the pels |
| WIN16 as a facade over the 32-bit core | to do (see below) |
| COMDLG32 | to do |

`GUI32.EXE` passes `gui32 done: 0 failure(s) in 112 checks`
(`cargo xtask run --headless --timeout 120 --init "RS2\GUI32.EXE"`;
`RS2\GUI32.EXE WAIT` keeps its window and dialog up).

**The order changed.** The plan below moves WIN16's 16-bit-independent
code into crates first, and builds the Win32 DLLs on them. WIN16's
USER and GDI turned out to be 16-bit throughout: far pointers in
messages, 16-bit wParams, Win16 structure layouts in every call. So the
32-bit form came first instead: W32USER is WIN16's window manager, GDI,
dialogs and controls ported to 32 bits (as Wine's USER32 is the core
and its USER.EXE16 a thunking layer over it). WIN16 is unchanged for
now; making it a facade over W32USER's core (16-bit messages and
structures translated to the 32-bit ones, as Wine's WINPROC_CallProc16To32
does) is the step that removes the second copy. Until then a fix to the
window manager is made in both.

**One DLL.** USER32, GDI32 and MSIMG32 are all W32USER (the loader maps
the three names, and W32KERN's GetModuleHandle does): windows, DCs and
GDI objects share their state, as WIN16's one DLL does. Handles stay
the small numbers WIN16 gives out, in ranges of their own: GDI objects
0x0100 and up, windows 0x4004, DCs 0x8002, menus 0xA004, icons 0xE000;
accelerator tables 0x000A0000 and up.

**Text** is kept in the process's code page; a window whose procedure
takes W messages (its class registered with RegisterClassW,
SetWindowLongW(GWL_WNDPROC), a dialog of DialogBoxParamW) gets the W
form of WM_SETTEXT, WM_GETTEXT, WM_CREATE's CREATESTRUCT, WM_CHAR...,
converted in `wnd::call_conv`; SendMessageW to an A window converts the
other way. GetMessageW gives WM_CHAR's character in UTF-16.

**Built-in procedures.** DefWindowProc, DefDlgProc and the controls'
procedures are Rust functions; programs see the addresses of their
exports (the realigning thunks: `Rs2ButtonProc`, `Rs2EditProc`...), so
subclassing and CallWindowProc work, and calls from the window manager
go to the Rust function directly.

**Threads.** A thread gets a PM message queue on its first window or
message call. A window belongs to the thread that made it; SendMessage
to another thread's window is PM's WinSendMsg of a private message
(`XSEND_A`/`XSEND_W`), which that thread's window procedure takes while
the sender waits. The window manager's tables are not locked: programs
that make windows on several threads at once are not safe yet.

**DIB sections** are a PM bitmap and the program's buffer (DosAllocMem),
kept in step around each GDI call that uses one: the buffer goes into
the bitmap before the call reads it, the bitmap back into the buffer
after a call drew into it. PM's bitmaps are bottom-up with 1, 4, 8 or 24
bits per pel: top-down DIBs and 16/32-bit ones (BI_RGB, BI_BITFIELDS)
are converted (`gdi32::normalize_dib`, `read_dib`).

**Dialogs** take their base units from the template's font (DS_SETFONT;
MS Shell Dlg is MS Sans Serif): the average width of the letters and the
height, as Windows computes them, so dialogs come out their designed
size; every control gets WM_SETFONT.

**Found on the way**: PM's frame destruction does not send WM_DESTROY
to a frame's client, so DestroyWindow sends WM_DESTROY itself (before
the children hear theirs) and WM_NCDESTROY after; W32USER's imports of
W32KERN must be declared `extern "stdcall"` (W32KERN's exports pop their
arguments).

**Not yet**: paths, world transforms and metafiles; printing; MDI;
hooks are accepted but not called; WM_MOUSELEAVE, WM_MOUSEWHEEL; a
window's region; fonts are WIN16's mapping of faces onto rs2's (no
TrueType files).

### The plan

#### Sharing the core

`userland/win16` already holds Windows' window manager and GDI semantics
on PM: window and class tables, the PM frame and `Win16Wnd` client
mapping, message translation both ways, DefWindowProc, the dialog
manager and IsDialogMessage, menus and accelerators, the controls WIN16
draws itself (BUTTON, STATIC, EDIT, LISTBOX, COMBOBOX, SCROLLBAR), MDI,
the caret, the clipboard, DCs on PM presentation spaces, the coordinate
flip, fonts, bitmaps, regions, COMMDLG, printing and DDE. That is the
hard part of USER and GDI, and most of it does not depend on 16 bits.

Copying it into Win32 DLLs would leave two copies to fix. Instead:

1. Move the 16-bit-independent part into a crate,
   `crates/winuser` (USER: windows, messages, dialogs, controls, menus)
   and `crates/wingdi` (GDI on GPI), with the program's procedures
   behind a trait: "call this window procedure with (hwnd, msg, wparam,
   lparam)" and "this message's parameters as the program sees them".
2. WIN16.DLL becomes the 16-bit facade: Pascal entry points, 16:16
   pointers, 16-bit handles, PascalCall for callbacks. Its behaviour
   stays as W16TEST checks it.
3. `W32USER.DLL` and `W32GDI.DLL` are the 32-bit facades: stdcall entry
   points, flat pointers, A and W forms, 32-bit handles and the Win32
   message packing.

The order matters: step 1 is a refactor of a large working DLL, done in
small commits with W16TEST and the real programs in docs/WIN16.md
("Real programs") run after each, before any Win32 code depends on it.

#### What the 32-bit facade changes

- **Message packing.** WPARAM is 32 bits, so messages that packed a
  handle and a code into LPARAM move them: WM_COMMAND (notification code
  in WPARAM's high word, the control in LPARAM), WM_HSCROLL/WM_VSCROLL,
  WM_MENUSELECT, WM_MENUCHAR, WM_PARENTNOTIFY, WM_ACTIVATE,
  WM_CHARTOITEM/VKEYTOITEM, WM_MDIACTIVATE, the WM_CTLCOLOR* messages
  (one message each, not WM_CTLCOLOR with a type). The facade translates
  between this and the core's form. WM_NOTIFY (COMCTL32) is new.
- **Threads and queues.** Each thread that creates a window gets a
  message queue (PM's WinCreateMsgQueue, made on demand as Windows does);
  a window belongs to its thread, SendMessage to another thread's window
  waits for that thread to take it (PM does the same across threads),
  and PostThreadMessage, AttachThreadInput and GetWindowThreadProcessId
  follow. The core's tables need locks. WIN16's per-process state is
  used by one thread; the shared core takes a lock per table (windows,
  classes, GDI objects) and never holds one while calling a window
  procedure. docs/WIN16.md's tasks section says which assumptions this
  touches.
- **Handles.** HWND, HDC, HGDIOBJ, HMENU become 32-bit values. The core's
  tables give out the same small numbers to both facades, so a Win32
  program and WIN16 could share a clipboard or DDE conversation with no
  translation.
- **Unicode.** The core keeps text in UTF-8 or UTF-16 internally (a
  choice for step 1) and the facades convert: WIN16 and the A forms
  through the process's code page, the W forms not at all. A window
  remembers whether its class was registered with RegisterClassW, and
  messages with text are converted when an A window procedure gets a W
  message and back (IsWindowUnicode, CallWindowProcA/W, as Windows
  does).
- **Resources** are read from the PE resource directory (the type, name
  and language tree), not the NE table. Dialog templates are the 32-bit
  layout (DLGTEMPLATE and DLGTEMPLATEEX, UTF-16 strings, DWORD-aligned
  items), menus the UTF-16 layout (and MENUEX), string tables UTF-16 in
  blocks of 16, icons and cursors the same groups as Win16. The core
  takes resources already decoded, so only the decoding is per facade.
- **GDI**: 32-bit coordinates (GPI's are 32 bits too; the 16-bit limits
  go), world transforms on NT (SetWorldTransform, GPI's model transform),
  paths (BeginPath ... StrokeAndFillPath, GPI's paths), DIB sections
  (CreateDIBSection: a bitmap whose bits the program writes directly),
  extended pens (ExtCreatePen), enhanced metafiles in stage 3.

### COMDLG32

The same dialogs as WIN16's COMMDLG (open, save, colour, font, print,
find/replace), with the 32-bit structures (OPENFILENAMEA/W with the
Explorer-style hook flags, CHOOSEFONT, PRINTDLG, ...), on the core.

## Stage 3: the compatibility tail

| DLL | rs2 DLL | What | On |
|-----|---------|------|----|
| ADVAPI32 | W32ADV | the registry (RegOpenKeyEx, RegQueryValueEx, ... A and W), then stubs for the security calls (OpenProcessToken, GetUserName, the ACL calls succeed with one user), services refused, the crypto calls (CryptAcquireContext, hashes, random) | rs2's REGISTRY.DLL (`userland/registry`), which already implements the Win32 registry calls for Open32; rs2crypto |
| COMCTL32 | W32CCTL | list view, tree view, toolbar, status bar, progress bar, trackbar, up-down, tab, tooltips, image lists, property sheets, the rebar later; version 5.8 (no visual styles) | the shared core's controls; none of these exist in WIN16 |
| SHELL32 | W32SHELL | ShellExecute(Ex) (rs2's associations, WPS's), SHGetFolderPath and SHGetSpecialFolderLocation (rs2's folders), SHBrowseForFolder, drag and drop of files (DragQueryFile, from PM's DM_DROP), Shell_NotifyIcon refused or a tray of rs2's own | WPS and PMSHAPI |
| SHLWAPI | W32SHLW | the path and string helpers | |
| WINMM | W32MM | waveOut, midiOut, PlaySound, the mixer calls, MCI, timeGetTime and the multimedia timers, joystick refused | rs2's MMPM (docs/MMPM.md), as WIN16's MMSYSTEM will be |
| WSOCK32, WS2_32 | W32SOCK | Berkeley sockets with Windows' names and errors, WSAStartup, WSAAsyncSelect (PM messages), WSAEventSelect, getaddrinfo | rs2's TCP/IP sockets (docs/NETWORK.md) |
| OLE32, OLEAUT32 | W32OLE | CoInitialize, CoCreateInstance for in-process servers from the registry, the IMalloc allocator, BSTRs and VARIANTs, structured storage; out-of-process COM later | |
| VERSION | W32VER | GetFileVersionInfo, VerQueryValue | WIN16's VER code |
| WINSPOOL | W32SPOOL | printers and jobs | rs2's spooler, as WIN16's printing (docs/PRINTING.md) |
| IMM32, MSIMG32, others | | stubs as programs need them | |

Programs that only check for a DLL or a function get a stub that fails
politely (returns an error and sets LastError) rather than a missing
import.

## Out of scope

- **Drivers and the native NT API beyond the few calls above**: kernel
  drivers (.SYS), VxDs, NTDLL's system calls, the Native API's object
  manager.
- **DirectX and OpenGL** at first; DirectDraw on rs2's display surfaces
  is a later possibility.
- **.NET** (managed programs need Microsoft's runtime or Mono).
- **Programs that need Windows' own files** (Microsoft's DLLs beyond the
  C runtime, Internet Explorer's components).
- **Copy protection and anti-cheat** that inspect the system.
- **Win64 (PE32+)** until stage 3 is done. It would load into a 64-bit
  process (docs/API64.md), with the same DLLs built for x86-64 and
  Microsoft's x64 calling convention and unwind tables (`.pdata`) in
  place of SEH frames.

## Open questions

- **Where the shared core's text lives**: UTF-16 (Windows' own, cheap
  for the W facade and PM's UCS-2 calls) or UTF-8 (Rust's, cheap for
  WIN16 and the A facade).
- **One DLL or many**: WIN16 is one DLL for every Windows module, mapped
  by ordinal ranges. Win32 imports by name, so one DLL per Windows
  module (W32KERN, W32USER, ...) keeps the export tables Windows'; one
  big DLL with name-mapped tables would keep shared state simpler.
- **Mixing with Open32**: Open32 programs are OS/2 programs calling
  Win32-style functions in PMWINX. Once W32USER exists, PMWINX's
  functions could forward to it (IBM's Open32 programs would run on it
  too), with Open32's differences (its own handle types, `_System`
  calls) in PMWINX.
- **Window classes of other programs**: Win32 has process-local classes
  plus the system's; WIN16 makes them per process. RegisterClass with
  CS_GLOBALCLASS in a DLL is per process on NT too, so nothing changes.
