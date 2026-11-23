# C++ for 64-bit programs

`userland64/c/cc64.sh` ([LIBC64.md](LIBC64.md)) also builds C++: sources
named `.cpp`, `.cc` or `.cxx` are compiled as C++20 (`-std=...` picks
another), with exceptions and RTTI, against LLVM's libc++.

```text
userland64/c/cc64.sh -o PROG.EXE main.cpp util.c
userland64/c/cc64.sh -o MY.DLL --dll my.cpp
userland64/c/cc64.sh -c main.cpp -o main.o
userland64/c/cc64.sh -o PROG.EXE --cxx main.o          objects only
userland64/c/cc64.sh --libcxx                           just the library
```

`os2.h` and the rest of the OS/2 API work in C++ as they do in C.

## What it is made of

userland64/cxx holds libc++, libc++abi and libunwind from LLVM 21.1.8
(Apache 2.0 with LLVM Exceptions; userland64/cxx/README.md lists rs2's
changes). `cc64.sh` builds them, on first use and when they change, into
`target/libc64/libcxx64.a` (`libcxx64s.a` for `--static`) and links that
into each module. Nothing of it is exported and what a module does not
use is dropped, so a small program that uses iostreams, `std::format` and
`std::filesystem` is about 0.9 MB.

- **Exceptions** are the Itanium ABI's: no cost until something is
  thrown. `link64.ld` keeps `.eh_frame` and `.gcc_except_table` and
  defines the symbols libunwind's bare-metal mode finds them by; a
  module's unwinder knows that module's functions only.
- **Static objects** are constructed from `.init_array` (programs:
  before `main`; DLLs: from `_DLL_InitTerm`) and destroyed through
  `__cxa_atexit`.
- **`thread_local`** (C's `_Thread_local` too) is emulated
  (`-femulated-tls`): the C library keeps each thread's variables and
  runs their destructors when the thread ends (`std::thread`,
  `pthread_create`, `_beginthread`) or the program calls `exit`.
- **Threads** are POSIX threads (`pthread.h`, part of the C library) on
  OS/2 threads with 1 MB stacks. Mutexes, condition variables and the
  rest wait with `__rs2_wait` and `__rs2_wake`, a futex: waiters queue by
  address, each sleeping on its thread's own event semaphore.
  `std::atomic::wait` uses it directly.
- **`std::filesystem::path`** has OS/2 paths: drive letters are root
  names, both `\` and `/` separate, `\` is preferred, and the characters
  are `char`. There are no links.
- **Locales**: only "C"; multibyte text is single bytes, so wide
  characters above 255 cannot be written to narrow streams.
- **Compiler target**: `x86_64-unknown-none-elf` with `__RS2__` and
  `__OS2__` defined. (`__linux__` is not, for C either: it was while C
  programs were built for the Linux target, and would have misled
  portable code.) Frames of a page or more touch every page, as OS/2
  stacks grow over a guard page (`-fstack-clash-protection`); C programs
  needed that too.

`cc64.sh` fails a link that leaves anything undefined but imports, since
such a module would not load.

## Limits

- Each module has its own copy of the C++ library, so exceptions must
  not leave the module they are thrown in, and C++ objects should not
  cross: give a DLL a C interface (`extern "C" RS2_EXPORT`). Exceptions
  do not pass through the C library either (a `qsort` comparison that
  throws ends the program).
- A text-mode `fstream` that seeks is off by the CRs it skipped; open
  files that are seeked in with `std::ios::binary`.
- `std::random_device` uses RDRAND; without it, timers (not fit for
  keys).
- Experimental parts of libc++ 21 (`std::jthread`, `<execution>`, the
  time-zone database) and C++20 modules are not there.
- The program's main thread must not end with `pthread_exit`: on OS/2
  that ends the process.

## Tests

`C:\RS2\CXXTEST.EXE` (userland64/c/samples/cxxtest.cpp, with LIBC64.DLL)
and `C:\RS2\CXXTSTS.EXE` (`--static`) make 98 checks each: construction
order, exceptions (through 50 frames, virtual bases, nested, rethrown,
from futures, function try blocks, `terminate` in child processes, after
a caught SIGSEGV), RTTI, containers, ranges, coroutines, formatting,
regular expressions, `from_chars`, streams, files and filesystem, all the
thread primitives, `thread_local`, and a C++ DLL (CXXTDLL.DLL). They end
with `cxxtest: 98 checks, 0 failures`:

```text
cargo xtask run --headless --timeout 120 --init "RS2\CXXTEST.EXE"
```
