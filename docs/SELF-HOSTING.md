# Self-hosting rs2 (notes for later)

Status: an idea, not planned work. This records what it would take for rs2
to build itself, i.e. `cargo xtask iso` running on rs2.

## What the build needs

The build is light: two Cargo workspaces (about 58 crates in total), the
stable toolchain, and an xtask that only runs `cargo`, `rustc` and
`objcopy` (debug-info stripping). QEMU is needed only for `xtask run`, not
for `build`, `iso` or `disk`. So the problem is running rustc, cargo and
LLVM on rs2.

## Why not as OS/2 programs

- OS/2 processes are 32-bit, and rs2 reproduces the 512 MB address space
  exactly (ARCHITECTURE.md). Even Warp's high-memory arena tops out around
  3 GB. rustc + LLVM compiling the kernel crate needs gigabytes.
- There is no known LLVM or rustc port to OS/2.

The compiler therefore has to run as a 64-bit process. rs2 has none today:
user space is 32/16-bit compatibility mode only.

## Route A (preferred): a Linux system-call layer for 64-bit ELF programs

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

## Route B: a native rs2 target

An `x86_64-unknown-rs2` target with a Rust `std` port. It needs the same
64-bit process and memory work, plus a C library (e.g. mlibc) and
cross-building LLVM (C++), rustc and cargo with their C dependencies (curl,
libgit2, zlib). It's cleaner and more "rs2", but much more work. It's only
worth it if native tools matter more than a compatibility layer.

## Needed by either route

- Offline builds: vendor the dependencies (`cargo vendor`) and build with
  `--offline`; crates.io stays out of reach without TLS in the network
  stack.
- The precompiled libraries for `x86_64-unknown-uefi`,
  `x86_64-unknown-none` and the userland's i686 target, copied into the
  sysroot as-is (they are data).
- Performance: each disk runs one command at a time and the kernel has a
  big lock (both on the roadmap). Builds would be slow but workable on a
  VM with several CPUs and 8 GB+ of memory.
- Distribution: the toolchain's licences allow shipping it, but it is not
  rs2 code, so it belongs on a separate developer image rather than the
  dist ISO.

## First step if this is picked up

A design for the 64-bit process model: its address-space layout next to
the OS/2 arenas, the second system-call entry, signals, and how such
processes appear to OS/2 programs (DosExecPgm of an ELF64 file, sessions,
pipes between the two worlds).
