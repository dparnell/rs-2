# Native Win32 (plan)

This is a plan; nothing here is built yet.

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
| 2 | GUI programs: USER32 and GDI32 on a core shared with WIN16, resources, COMDLG32 | Open Watcom's and MinGW's Win32 GUI samples; Win32s-era programs |
| 3 | The compatibility tail: COMCTL32, SHELL32, WINMM, Winsock, OLE, printing, the rest of ADVAPI32 | Chosen real programs (an editor, an installer, a game) |
| later | Win64 (PE32+) on rs2's long-mode path (docs/API64.md) | |

The test programs are built with Open Watcom (`wcl386 -bt=nt`, on the
developer disk: docs/DEVDISK.md) and MinGW-w64 (i686), as WIN16's are
with Open Watcom's `-bt=windows` (docs/WIN16.md, "Building the test
programs"). A `W32TEST.EXE`, like W16TEST, checks each stage's calls.

## Stage 1: the kernel and the console

### The PE loader

`kernel/src/loader/peload.rs`, beside `lxload.rs`, `neload.rs` and
`elfload.rs`. `crates/os2-exe` already tells the formats apart by the header the
MZ stub's `e_lfanew` (0x3C) points to (`os2_exe::detect`: NE, LX, LE or
PE); the PE parser goes in that crate, beside its NE and LX ones, so
that tools on the host can use it too.

For a PE32 image (machine 0x14C, optional header magic 0x10B) it:

1. **Maps the image.** Headers and sections at their virtual addresses,
   with their protections (code read/execute, `.rdata` read-only, data
   read/write), uninitialised tails zeroed. Sections are file-backed and
   loaded on demand as LX pages are, where the file alignment allows it.
2. **Picks the address.** An EXE goes at its preferred base, almost
   always 0x00400000. Many EXEs are linked without relocations
   (`IMAGE_FILE_RELOCS_STRIPPED`), so there is no other choice. The
   address lies in the private arena below `SHARED_FLOOR` (0x04000000,
   `kernel/src/vm.rs`), so it is free if the image is mapped before the
   process allocates anything else. An image that would reach the
   shared arena's floor is refused unless it can be relocated.
   DLLs prefer 0x10000000, which lies in the range the shared arena may
   grow into, so a DLL is normally relocated. That is fine: DLLs keep
   their relocations.
3. **Applies base relocations** (`.reloc`: HIGHLOW, and ABSOLUTE as
   padding) when the image is not at its preferred base.
4. **Resolves imports.** The import directory names each DLL and its
   functions by name or ordinal (the hint speeds up the name lookup).
   The loader writes the addresses into the import address table, then
   makes the IAT read-only again if its section is. Forwarded exports
   (`NTDLL.RtlAllocateHeap` as KERNEL32's `HeapAlloc`) are followed.
   Bound imports are ignored (the binding never matches rs2's DLLs), and
   delay-load imports are left to the program's own helper, which calls
   LoadLibrary and GetProcAddress.
5. **Maps the system DLL names** through a table like WIN16's
   `WIN16_MODULES`: `KERNEL32` → rs2's `W32KERN`, `USER32` → `W32USER`
   and so on (the rs2 names are below). Unlike Win16, Win32 imports
   mostly by name, so the rs2 DLLs export Windows' names; where programs
   import by ordinal (MFC's DLLs, COMCTL32's and SHLWAPI's ordinal-only
   entries) the rs2 DLL exports those ordinals too. Any other name loads
   the program's own DLL from the program's directory, then the current
   directory, then LIBPATH and PATH (Windows' search order, roughly).
6. **Sets up TLS.** The `.tls` directory gives the template, the size of
   the zero fill, the index variable and the callbacks. Each thread gets
   a copy of the block, its pointer in the TEB's TLS array, and the
   callbacks are called with DLL_PROCESS_ATTACH / DLL_THREAD_ATTACH like
   DllMain.
7. **Calls DllMain** of each DLL it loads (`DLL_PROCESS_ATTACH`, in
   dependency order), and with `DLL_THREAD_ATTACH`/`DETACH` and
   `DLL_PROCESS_DETACH` as threads and the process start and end.
   OS/2's DLL initialisation calls (`_DLL_InitTerm`) stay for rs2's own
   DLLs.
8. **Enters the program** at the entry point on a stack of the header's
   reserve size with its commit size committed, the rest grown through
   a guard page as OS/2 stacks are (MSVC's `__chkstk` and MinGW's
   `___chkstk_ms` probe the stack a page at a time and rely on this).

`DosQueryAppType` already recognises PE files (`kernel/src/api/module.rs`:
FAPPTYP_32BIT | FAPPTYP_WINDOWSPROT31, as IBM's does), and the subsystem field (2 GUI, 3 console) decides whether the
process is a PM process or a windowed/full-screen VIO one, as the NE
WINDOWAPI flags do. `DosExecPgm`, CMD and the shell start PE programs
like any other.

The loader's modules are visible to OS/2's module calls
(`DosQueryModuleHandle`, `DosQueryProcAddr` on a PE DLL) only as far as
rs2's W32KERN needs them for LoadLibrary and GetProcAddress.

### The TEB and the PEB

Win32 code reaches its thread's data through FS. OS/2 does the same with
its TIB, and the first words even agree:

| Offset | Win32 TEB | OS/2 TIB |
|--------|-----------|----------|
| 0x00 | ExceptionList (SEH chain) | tib_pexchain (exception chain) |
| 0x04 | StackBase (top, high address) | tib_pstack (bottom, low address) |
| 0x08 | StackLimit (lowest committed, low address) | tib_pstacklimit (top, high address) |
| 0x0C | SubSystemTib | tib_ptib2 |
| 0x18 | Self (the TEB's flat address) | |
| 0x20 | ClientId: process and thread IDs | |
| 0x2C | ThreadLocalStoragePointer (the `.tls` blocks) | |
| 0x30 | ProcessEnvironmentBlock (PEB) | |
| 0x34 | LastErrorValue | |
| 0xE10 | TlsSlots[64] (TlsAlloc) | |
| 0xF94 | TlsExpansionSlots | |

The exception chain is at the same place; the stack bounds are swapped.
OS/2's TIBs are 0x40 bytes apart in the process information area
(`kernel/src/task/process.rs`, `info::TIB_STRIDE`), so a TEB cannot go
there. A Win32 process gets a 4 KB TEB per thread in its private memory,
and `gdt::set_tib_base` gives `TIB_SEL` that TEB's base and a 0xFFF
limit on each switch to a Win32 thread. The thread keeps its OS/2 TIB
too (DosGetInfoBlocks returns it, and rs2's own DLLs called by the
program use it); the kernel reads the exception chain from whichever
block FS points at, which is the same offset in both.

The PEB, one per process, carries what programs and runtimes read:
BeingDebugged (0x02), ImageBaseAddress (0x08), Ldr (0x0C, the loader's
module lists: InLoadOrder, InMemoryOrder, InInitializationOrder, each
LDR_DATA_TABLE_ENTRY with base, entry, size and the names in UTF-16),
ProcessParameters (0x10: the command line, image path, current
directory, standard handles and environment, as UNICODE_STRINGs),
ProcessHeap (0x18), NumberOfProcessors (0x64), the OS version (0xA4:
5.1 to start with, Windows XP, which most programs of interest accept).
Packers, copy protection and some runtimes walk Ldr; it has to be kept
right as DLLs load and unload.

### Structured exception handling

rs2's 32-bit exception dispatch (`kernel/src/api/exceptions.rs`) already
does what Win32 SEH needs: walk the registration records from FS:[0],
call each handler in user mode with a return through the API gate,
resume, search on, or unwind (`EH_UNWINDING`, DosUnwindException). The
exception codes are the same numbers (0xC0000005 for an access
violation, 0xC0000094 for an integer divide by zero, ...), and so is the
report record's start (code, flags, nested record, address, the number of
parameters, the parameters). A Win32 thread gets its own flavour of the
dispatch:

- **The context record** is Win32's CONTEXT: ContextFlags, the debug
  registers, the 112-byte FLOATING_SAVE_AREA, GS FS ES DS, EDI ESI EBX
  EDX ECX EAX, EBP EIP CS EFLAGS ESP SS, then 512 bytes of FXSAVE
  (ExtendedRegisters). OS/2's has a different order (EAX before EBX) and
  no debug registers; programs and C++ runtimes read and write it by
  offset.
- **Handler results** differ: Win32's ExceptionContinueExecution is 0 and
  ExceptionContinueSearch 1, where OS/2 uses 0xFFFFFFFF and 0.
  ExceptionNestedException (2) and ExceptionCollidedUnwind (3) are
  Win32's own.
- **Unwinding** is RtlUnwind in W32NTDLL (forwarded from KERNEL32), a
  user-mode walk of the chain calling handlers with EXCEPTION_UNWINDING
  (2, the same bit as OS/2's EH_UNWINDING), then restoring FS:[0]. It
  needs no kernel call; RaiseException uses the dispatch.
- **No handler**: SetUnhandledExceptionFilter's filter runs; with none,
  or if it returns EXCEPTION_CONTINUE_SEARCH, the process ends with the
  exception code as its exit code and rs2's trap screen (POPUPLOG), as
  an OS/2 program's would.
- **Vectored handlers** (AddVectoredExceptionHandler, XP) run before the
  frame chain.
- **C++ exceptions** are SEH exceptions with code 0xE06D7363 (MSVC) and
  need nothing more of the kernel; MinGW's DWARF-2 or SJLJ unwinding
  uses none of this except RaiseException for its foreign exceptions.

### KERNEL32 for stage 1

`userland/win32/kernel` → `W32KERN.DLL` (exports Windows' KERNEL32
names), with `W32NTDLL.DLL` for the handful of NTDLL calls programs and
runtimes import directly (RtlUnwind, RtlZeroMemory, RtlMoveMemory,
NtQueryInformationProcess for version checks, RtlGetVersion). Each call
is `extern "stdcall"` in Rust, on rs2's Dos* calls:

| Area | Calls | On |
|------|-------|----|
| Files | CreateFile, ReadFile, WriteFile, SetFilePointer(Ex), CloseHandle, GetFileSize(Ex), GetFileAttributes(Ex), FindFirstFile/FindNextFile, DeleteFile, MoveFile(Ex), CopyFile, CreateDirectory, RemoveDirectory, GetFullPathName, GetTempPath, GetFileTime, FlushFileBuffers, LockFile | DosOpen and friends; long names on HPFS and JFS; FILETIME from OS/2's date and time |
| Handles | one handle table per process: files, processes, threads, events, mutexes, semaphores, file mappings, console buffers; DuplicateHandle, inheritance | the OS/2 handles underneath |
| Memory | VirtualAlloc/Free/Protect/Query, HeapCreate/Alloc/Free/ReAlloc/Size, the process heap, GlobalAlloc and LocalAlloc (the Win16 names over the heap), IsBadReadPtr | DosAllocMem, DosSetMem, DosQueryMem (VirtualAlloc of MEM_RESERVE is a reservation without commit; high memory for large reservations) |
| Processes | CreateProcess, GetExitCodeProcess, ExitProcess, TerminateProcess, GetCommandLine, GetEnvironmentStrings, Get/SetEnvironmentVariable, GetModuleFileName, GetStartupInfo, GetCurrentProcess(Id) | DosExecPgm (the command line passed whole; Win32 programs parse their own), DosWaitChild, DosKillProcess |
| Threads | CreateThread, ExitThread, GetExitCodeThread, Suspend/ResumeThread, Sleep, SwitchToThread, Get/SetThreadPriority, TlsAlloc/Get/Set/Free | DosCreateThread with a TEB made first; DosSleep; DosSetPriority |
| Synchronisation | events (manual and auto reset), mutexes, semaphores, critical sections, WaitForSingleObject, WaitForMultipleObjects (all or any), InterlockedXxx | rs2's event, mutex and muxwait semaphores; critical sections spin then block on an event, all in user mode until contention |
| Modules | LoadLibrary(Ex), FreeLibrary, GetProcAddress, GetModuleHandle, the resource calls (FindResource, LoadResource, LockResource, SizeofResource) | the PE loader through a kernel call; resources read from the mapped image |
| Time | GetSystemTime(AsFileTime), GetLocalTime, GetTickCount, QueryPerformanceCounter/Frequency, the time zone calls | DosGetDateTime, the TZ variable, the TSC rs2 already calibrates |
| Strings and NLS | MultiByteToWideChar, WideCharToMultiByte, GetACP, GetOEMCP, CompareString, LCMapString, GetLocaleInfo, lstrlen/lstrcpy and friends | UCONV and ULS (docs/UTILITIES.md, the NLS files); the ANSI code page is the process's OS/2 code page mapped to its Windows twin (850 → 1252) |
| Errors | GetLastError, SetLastError, FormatMessage | LastError in the TEB; OS/2 error codes mapped to Windows' (they share most numbers below 100: 2 file not found, 5 access denied) |
| System | GetVersion(Ex), GetSystemInfo, GetSystemDirectory, GetWindowsDirectory, GetComputerName, GetDiskFreeSpace(Ex), GetDriveType, GetLogicalDrives | the directories are rs2's `\WIN32` (made at install; no Windows files in it) |

### The console

Win32 console programs use the console API, not VIO. `W32KERN` puts it
on rs2's VIO, KBD and MOU calls in the program's session (a windowed VIO
session from PM, or full screen): GetStdHandle, WriteConsole and
ReadConsole (A and W), WriteFile and ReadFile on console handles,
SetConsoleMode (line input, echo, processed output), the screen buffer
calls (SetConsoleCursorPosition, SetConsoleTextAttribute,
FillConsoleOutputCharacter/Attribute, ReadConsoleOutput,
WriteConsoleOutput, ScrollConsoleScreenBuffer, the buffer info),
ReadConsoleInput and PeekConsoleInput (key and mouse events from KBD and
MOU), SetConsoleCtrlHandler (Ctrl+C and Ctrl+Break from OS/2's signals),
SetConsoleTitle (the window list title), AllocConsole/FreeConsole for
GUI programs. Standard handles redirected by CMD are files and pipes,
not consoles, and ReadFile/WriteFile go to them directly.

### The C runtime

MinGW programs import MSVCRT.DLL; MSVC-built programs import their own
runtime (MSVCRT, MSVCR71 ... MSVCR100, or the UCRT's
`api-ms-win-crt-*` DLLs), which they usually ship beside the program but
which are Microsoft's. rs2 provides its own `MSVCRT.DLL`
(`userland/win32/msvcrt`): the C library functions, `__getmainargs`,
`_initterm`, `_onexit`, `_iob` and `__p__iob`, the locale and ctype
tables, `_except_handler3/4`, `_setjmp3` and `longjmp` with SEH unwind,
`_beginthreadex`. rs2's own C library for 64-bit programs
(docs/LIBC64.md) is a source of the portable parts. The versioned names
(MSVCR71 ...) map to the same DLL in the name table; the UCRT forwarders
come in stage 3.

## Stage 2: windows on a core shared with WIN16

### Sharing the core

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

### What the 32-bit facade changes

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
