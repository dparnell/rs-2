# Self-hosting rs2 (notes for later)

Status: under way. This records what it would take for rs2 to build
itself, i.e. `cargo xtask iso` running on rs2. The first step is done:
rs2 runs 64-bit programs (ELF64, long mode) with a native 64-bit OS/2
API, including PM: see [API64.md](API64.md). They can use memory above
4 GB (API64.md, "Memory"), and C programs have a C library of their own
([LIBC64.md](LIBC64.md)) and C++ with libc++ ([CXX64.md](CXX64.md)),
the base route B needs. Rust programs have std, threads, sockets and
processes included ([STD64.md](STD64.md)).

## What the build needs

The build is three Cargo workspaces (kernel and tools, `userland` and
`userland-std`, `userland64`), the stable toolchain, and an xtask that runs
`cargo`, `rustc` and `rust-objcopy`, and these besides:

- `clang`, for the 64-bit C and C++ libraries (`userland64/c/cc64.sh`) and
  for ring's C code in userland-std
- a POSIX shell, for `cc64.sh`, `userland-std/{cc,cc64,ar,lld}.sh` and the
  DOS test programs' `build.sh` (better rewritten in Rust than a shell
  ported)
- `git` (the version stamp), `curl` and `unzip` (optional downloads)

QEMU is needed only for `xtask run`, not for `build`, `iso` or `disk`.

## Why not as OS/2 programs

- OS/2 processes are 32-bit, and rs2 reproduces the 512 MB address space
  exactly (ARCHITECTURE.md). Even Warp's high-memory arena tops out around
  3 GB. rustc + LLVM compiling the kernel crate needs gigabytes.
- There is no known LLVM or rustc port to OS/2.

The compiler therefore has to run as a 64-bit process ([API64.md](API64.md)).

## Route A: a Linux system-call layer for 64-bit ELF programs

The same idea as WSL1 or FreeBSD's Linux compatibility: run the official
rustc/cargo binaries unchanged instead of porting anything. Targeting the
`x86_64-unknown-linux-musl` host toolchain keeps the libc side small.

Kernel work:

- 64-bit processes: long-mode user code next to the OS/2 compatibility-mode
  processes, a `syscall`/`sysret` entry, an ELF64 loader with the dynamic
  loader hand-off (`librustc_driver` and LLVM are shared objects), FS-base
  TLS.
- Memory: a large demand-paged address space with overcommit,
  `mmap`/`mprotect`/`munmap` (anonymous and file-backed), 8 MB+ thread
  stacks.
- About 120-150 Linux system calls:
  - files: `openat`, `newfstatat`/`statx`, `getdents64`, `pread`/`pwrite`,
    rename/unlink/mkdir/rmdir, `getcwd`/`chdir`, `readlink`
  - threads: `clone` + `futex` (`set_robust_list`, `rseq` may return
    ENOSYS)
  - processes: `clone(CLONE_VFORK)`/`execve`, `wait4`
  - I/O: `pipe2`, `dup3`, `fcntl`, `poll`/`ppoll` (cargo's jobserver),
    `ioctl(TCGETS)`
  - `flock` (cargo locks `target/`), `getrandom`, `clock_gettime`,
    `sched_getaffinity` (for `-j`), `prlimit64`
  - signals: `rt_sigaction`, `rt_sigprocmask`, SIGCHLD
- A few `/proc` and `/dev` entries: `/proc/self/exe` (rustc finds its
  sysroot through it), `/dev/null`, `/dev/urandom`.
- POSIX behaviour on HPFS/FAT: case-preserving names, fake permission
  bits, and mtimes precise enough for cargo's fingerprints (HPFS and FAT
  timestamps are coarse). Hard links may fail, because cargo falls back to
  copying.

### Milestones

1. A static musl "hello world" as a 64-bit process.
2. Static busybox: a shell, coreutils, process spawning.
3. Dynamically linked programs through musl's loader.
4. `rustc hello.rs`.
5. `cargo build` of a host-tested crate (e.g. `crates/inifile`).
6. `cargo xtask iso`: rs2 built on rs2.

Steps 1-3 are most of the kernel work, roughly on the scale of the
networking milestone. After that it is mostly chasing missing system calls
until rustc runs.

## Route B (preferred now): native tools

64-bit OS/2 builds of the tools. Its extra cost over route A was mostly
the C and C++ runtimes, which are done, and native compilers serve
programs of rs2's own as well.

1. Rust's std for 64-bit programs: done ([STD64.md](STD64.md)).
2. Rust DLLs with std, C and Rust interfaces (what proc macros and
   `rustc_driver` are): done ([STD64.md](STD64.md)).
3. LLVM's `lld`: done. `userland64/llvm/build.sh` fetches LLVM 21.1.8,
   applies `rs2.patch` (paths with drive letters, `;` in PATH, temporary
   files on FAT, no renaming of open files) and cross-builds it through
   `userland64/cmake/rs2.cmake`, on LIBC64.DLL and LIBCXX64.DLL. LLD.EXE on
   rs2 links CHELLO64-like C programs and CXXTEST with the same bytes as
   on the host (`tests/lld64/run.sh`).
   `clang`: done, from the same tree, which adds the target
   `x86_64-unknown-rs2` to it (`__RS2__`, emulated TLS; the driver compiles
   and links as cc64.sh does) and to lld `--rs2-imports` (an undefined
   symbol that is not `MODULE.name` is an error). See "The C and C++
   toolchain" below.
4. `rustc`: done (1.100.0, with its own LLVM 23). See "rustc" below.
5. `cargo`, offline and without curl and libgit2 as far as it goes.
   Milestone: `cargo build` of a host-tested crate.
6. `cargo xtask iso` on rs2, with the shell scripts gone.

## The C and C++ toolchain

`userland64/llvm/build.sh` cross-builds CLANG.EXE (65 MB) and LLD.EXE, and
`userland64/llvm/sdk.sh` puts them with the headers and libraries into one
tree (target/sdk64; target/sdk64.txt is it as a manifest for
`cargo xtask disk --hpfs`). The C++ headers' names need HPFS or JFS. With
`\sdk\bin` in PATH:

```text
clang hello.c -o HELLO.EXE                  a program (LIBC64.DLL)
clang++ -std=c++20 prog.cpp -o PROG.EXE     C++ (LIBCXX64.DLL)
clang -mwindows pmprog.c -o PMPROG.EXE      a PM program
clang -shared lib.c -o LIB.DLL              a DLL (visible symbols exported)
clang prog.c LIB.DLL -o PROG.EXE            uses that DLL
clang -static prog.c -o PROG.EXE            no library DLLs
```

clang finds the tree relative to itself. Temporary files go to TMPDIR, TMP
or TEMP, else the current directory. clang++.EXE runs CLANG.EXE. The same
tree is a cross SDK for the host's build of this clang:
`work/llvm/host/bin/clang --target=x86_64-unknown-rs2
--sysroot=target/sdk64`. Resources (`cc64.sh --rc`) are not in it yet:
os2-rc has to run on rs2 for that.

`tests/clang64/run.sh` builds a C program, a PM program, a C++ DLL,
CXXTEST and the C library's sources on rs2 and compares each result with
the host clang's: all the same bytes, and the programs run. An 8 MB
preprocessed file of clang's own (ExprConstant.cpp, about 500 MB of heap)
gives the same object too.

## rustc

`userland64/rustc/build.sh` fetches rustc's source (the installed rustc's
version), applies `rs2.patch` and cross-builds it; `userland64/rustc/sdk.sh`
adds it to the toolchain tree of userland64/llvm/sdk.sh. Then on rs2, with
`\sdk\bin` in PATH and BEGINLIBPATH (for RSLLVM.DLL):

```text
rustc -O hello.rs -o HELLO.EXE
rustc --crate-type proc-macro pm.rs
rustc --crate-type cdylib lib.rs          (libname.so: the DLL NAME)
```

- **RUSTC.EXE** (79 MB) is rustc's crates in one program, built with the
  installed stable rustc for `x86_64-unknown-linux-gnu` on rs2libc, as
  other std programs are (STD64.md). There is no rustc_driver DLL.
- **RSLLVM.DLL** (37 MB) is LLVM and rustc's C++ wrapper on LIBC64.DLL and
  LIBCXX64.DLL, behind the C functions rustc calls. What one side
  allocates it frees itself; the wrapper gets the function that writes
  into Rust strings as a pointer.
- **The target is still `x86_64-unknown-linux-gnu`**, and the standard
  library is rustup's, as it is: RUSTC.EXE says it is the same compiler
  (the same version string, so the same crate hashes).
- **RS2-LD.EXE** (userland-std/rs2ld) is the default linker for it: lld.sh
  in Rust. It links rs2libc in as `libc.a`, which std asks for, so programs
  need not name rs2libc, and runs LLD.EXE.
- **Paths.** std's `Path` is Unix's. rs2libc gives 64-bit programs their
  directories and program names with `/`, and takes `dir/C:/x`, which is
  what joining an absolute OS/2 path gives, as `C:\x`. rustc turns `\` in
  its arguments into `/`. It leaves PATH alone for the linker.
- **Stacks** that the stacker crate makes are mapped below 4 GB, as the
  OS/2 API takes no pointers above.

Not there yet: `dylib` crates that share std (userland-std/dyn64.sh) and
linkers for the other targets (kernel, boot loader, 32-bit userland).

`tests/rustc64/run.sh` compiles, links and runs programs there, a proc
macro and its user, and a cdylib and its user, and compares objects with
the installed rustc's: the same bytes, regex-syntax's 690 KB included.

## Needed by either route

- Offline builds: vendor the dependencies (`cargo vendor`) and build with
  `--offline`. (TLS itself works: rustls, in the browser.)
- The precompiled libraries for `x86_64-unknown-uefi`,
  `x86_64-unknown-none` and the userland's i686 target, copied into the
  sysroot as-is (they are data).
- Performance: each disk runs one command at a time (on the roadmap).
  Builds would be slow but workable on a
  VM with several CPUs and 8 GB+ of memory.
- Distribution: the toolchain's licences allow shipping it, but it is not
  rs2 code, so it belongs on a separate developer image rather than the
  dist ISO.
