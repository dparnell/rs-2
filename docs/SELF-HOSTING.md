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
3. LLVM's `lld`, then `clang`, cross-built with cc64.sh (C++ on
   LIBCXX64.DLL; what LLVM's Unix support code needs of POSIX, `mmap` of
   files first). Milestone: compile and link a C program on rs2.
4. `rustc`: LLVM in a DLL of its own, behind the C interface of rustc's
   LLVM wrapper (each module has its own C library), `rustc_driver`, proc
   macros. Milestone: `rustc hello.rs` on rs2.
5. `cargo`, offline and without curl and libgit2 as far as it goes.
   Milestone: `cargo build` of a host-tested crate.
6. `cargo xtask iso` on rs2, with the shell scripts gone.

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
