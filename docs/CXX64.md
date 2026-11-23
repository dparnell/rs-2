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
`target/libc64`: `LIBCXX64.DLL`, which xtask installs in `C:\RS2\DLL64`
and which programs and DLLs use by default, and `libcxx64s.a`, linked in
with `--static`.

- **Linking** to LIBCXX64.DLL is ELF's own: the linker takes the DLL as a
  shared library, the module gets a `DT_NEEDED` entry with its name
  (`LIBCXX64`), and the loader looks the module's plain undefined symbols
  up in the DLLs it needs (API64.md, "Executables"). Functions, virtual
  tables, type descriptions and variables such as `std::cout` all come
  from the DLL; templates and inline functions are the module's own.
- **Exceptions** are the Itanium ABI's: no cost until something is
  thrown. They pass between modules, and through C code too (`qsort`):
  every module `cc64.sh` links keeps its unwind tables, its start-up code
  (`crt1.o`, `crtdll.o`) announces them to the C library
  (userland64/libc/frames.c), and libunwind asks there by address.
- **Types** are compared by name, since modules may each have their own
  description of one; types in unnamed namespaces by address.
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

## C++ DLLs

A DLL may have a C++ interface: mark functions, classes and variables
with `RS2_EXPORT` (os2.h) in its header, and name the DLL on the command
line of what uses it:

```text
cc64.sh -o SHAPES.DLL --dll shapes.cpp
cc64.sh -o PROG.EXE main.cpp SHAPES.DLL
```

The program then needs SHAPES (the file's name); C functions can be used
the same way. Strings, containers and smart pointers may be passed and
freed on either side, classes derived from and caught across, and
`thread_local` variables exported. userland64/c/samples/cxxtdl2.h is an
example. A DLL with a C interface can be loaded by any 64-bit program,
C or Rust, with DosLoadModule: LIBCXX64.DLL is loaded, and ended, with it.

## CMake

`userland64/cmake/rs2.cmake` is a toolchain file: CMake projects build
with clang and cc64.sh's flags (`cc64.sh --flags C|CXX`), and cc64.sh
links. Build the libraries first (`cc64.sh --libcxx`):

```text
cmake -G Ninja -DCMAKE_TOOLCHAIN_FILE=.../userland64/cmake/rs2.cmake ...
```

LLVM's lld is built this way ([SELF-HOSTING.md](SELF-HOSTING.md)).

## Limits

- With `--static` each module has its own copy of the C++ library, so
  exceptions and C++ objects must not cross: give a DLL a C interface.
- Variables in headers (`inline int x;`, static members of class
  templates) that are not exported by a DLL exist once in each module.
  An `operator new` a program replaces is used by that module only.
- LIBCXX64.DLL's interface has no versions: a program needs the one of
  the rs2 it was built with, or a newer one that kept its symbols.
- A text-mode `fstream` that seeks is off by the CRs it skipped; open
  files that are seeked in with `std::ios::binary`.
- `std::random_device` uses RDRAND; without it, timers (not fit for
  keys).
- Experimental parts of libc++ 21 (`std::jthread`, `<execution>`, the
  time-zone database) and C++20 modules are not there.
- The program's main thread must not end with `pthread_exit`: on OS/2
  that ends the process.

## Tests

`C:\RS2\CXXTEST.EXE` (userland64/c/samples/cxxtest.cpp, with the DLLs)
and `C:\RS2\CXXTSTS.EXE` (`--static`) check construction order,
exceptions (through 50 frames, virtual bases, nested, rethrown, from
futures, function try blocks, `terminate` in child processes, after a
caught SIGSEGV), RTTI, containers, ranges, coroutines, formatting, regular
expressions, `from_chars`, streams, files and filesystem, all the thread
primitives, `thread_local`, and a C++ DLL loaded with DosLoadModule
(CXXTDLL.DLL). CXXTEST also tests what crosses to CXXTDL2.DLL, a DLL with
a C++ interface. They end with `cxxtest: N checks, 0 failures` (114 and
98). CLIBTEST, a C program, loads CXXTDLL twice:

```text
cargo xtask run --headless --timeout 120 --init "RS2\CXXTEST.EXE"
```
