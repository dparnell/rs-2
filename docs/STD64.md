# Rust's standard library in 64-bit programs

Programs written in ordinary Rust, with `std`, build as 64-bit OS/2
programs ([API64.md](API64.md)): threads, files, sockets, processes,
panics that unwind, `thread_local!`, and the OS/2 API and PM through the
`os2` crate next to them. They use the stable toolchain and rustup's
precompiled std for `x86_64-unknown-linux-gnu`.

rs2 has three ways to write 64-bit programs in Rust and C:

| | Library | Use it for |
| --- | --- | --- |
| `userland64`, `no_std` | the `os2` crate's small runtime | DLLs, small programs |
| `userland-std`, `std` | rs2libc (this document) | programs that use std or crates from crates.io |
| `userland64/c` | LIBC64.DLL, LIBCXX64.DLL | C and C++ ([LIBC64.md](LIBC64.md), [CXX64.md](CXX64.md)) |

## Building

```text
cd userland-std
cargo build --release --target x86_64-unknown-linux-gnu -p stdtest
```

The result (`target/x86_64-unknown-linux-gnu/release/stdtest`) is the
program as rs2's loader takes it; there is no conversion. A program is a
crate in the userland-std workspace:

```toml
[dependencies]
rs2libc = { path = "../rs2libc" }
os2 = { path = "../../userland64/os2", default-features = false }   # for the OS/2 API
```

```rust
use rs2libc as _;   // must be linked

fn main() {
    println!("Hello from std");
}
```

`default-features = false` leaves out the `os2` crate's own allocator,
panic handler and `mem*` functions: std and rs2libc have them.
`os2::app_type!(PM)` makes a PM program. `use os2::*` brings a `println!`
of its own; `use std::println;` picks std's. `os2::resources!()` works as
in `no_std` programs. Programs that go on the disk image are listed in
xtask's `STD_PROGRAMS64`.

## How it works

rs2libc (userland-std/rs2libc, [BROWSER.md](BROWSER.md)) is the C library
std expects: glibc's functions by name, with Linux's structure layouts and
constants, on top of the OS/2 API. It serves the 32-bit programs too; what
differs between the two:

- **Linking.** `.cargo/config.toml` and `lld.sh` link a position
  independent executable with `link64.ld` (segments on 64 KB, as the
  loader wants). Imports are undefined dynamic symbols, `DOSCALLS.273`,
  which lld keeps in an executable with `-z dynamic-undefined-weak`.
  `lld.sh` refuses a program with any other undefined symbol: a function
  rs2libc does not have shows up when linking.
  Cargo runs `linker64.sh`, which hands build scripts and proc macros to
  `cc`: on an x86-64 Linux host (CI) they have the same target triple.
- **Thread-local storage.** Code in an executable reaches
  `#[thread_local]` variables through FS (`mov %fs:offset`); rustc on
  stable offers no other model. OS/2 keeps FS for the TIB, but 64-bit code
  has no use for it there (DosGetInfoBlocks gives the 64-bit TIB), so each
  thread gets an LDT selector whose base is its thread pointer (RS2API
  421, as GS does in 32-bit programs) and loads it into FS. A descriptor's
  base has 32 bits, so the block comes from a heap below 4 GB.
  - The kernel saves and restores FS with the other registers, and calls
    a 64-bit exception handler with the FS of the 64-bit code it
    interrupted.
  - The 64-bit PMWIN puts the TIB's selector back around the 32-bit code
    it calls, and the thread's own in window procedures called back from
    there (it remembers it in a dword of thread-local memory).
  - The program exports `RS2_THREAD_START(param, pfn)`: the kernel starts
    the program's threads there, so threads made with DosCreateThread (by
    a DLL, or C code) have the storage too, and their destructors run when
    they return.
- **Memory.** Blocks of 16 MB and more come from above 4 GB
  (DosAllocMem with OBJ_ANY). The rest of the heap takes the first 96 MB
  below, so a program of ordinary size can pass any of its data to any
  call, PM's among them, and grows above 4 GB after that. rs2libc's own
  calls work with data anywhere: DosRead, DosWrite and the socket
  transfers take such pointers, and paths, argument blocks and small
  buffers go through copies below 4 GB. A program that calls the OS/2 API
  itself and uses a lot of memory has to keep what it passes below 4 GB
  (DosAllocMem without OBJ_ANY): the kernel and PMWIN refuse other
  pointers, they never truncate them.
- **Stacks** are below 4 GB. Thread 1 has 4 MB, other threads 2 MB unless
  `std::thread::Builder` says otherwise; they are committed as they grow.
- **Files** use the 64-bit calls (DosOpenL, DosSetFilePtrL,
  DosSetFileSizeL, FILESTATUS3L). `File::lock` is a region lock
  (DosSetFileLocks) on a byte that no file reaches, so others can read and
  write a locked file, as on Unix. Locks belong to the process: a second
  handle of the same process may be able to lock again. Times have OS/2's
  2 seconds.
- **Loading DLLs**: `dlopen`/`dlsym` (the libloading crate) are DosLoadModule and
  DosQueryProcAddr.

## DLLs

There are two kinds, for both of which the crate uses `rs2libc` as a
program does:

- `crate-type = ["cdylib"]`: a DLL with a C interface and a std, a heap
  and a C library of its own (stdtdll). Any 64-bit program can use it.
  What it allocates it has to free, and panics must not leave it.
- `crate-type = ["dylib"]`: a DLL with a Rust interface (stdtrlib). Its
  std is shared: programs and DLLs built on it exchange `String`s, trait
  objects, panics and thread-locals with it, and have to be built with the
  same compiler. rustc takes std from such a DLL only if std has no dylib
  of its own in the sysroot, so a program on one is built with
  `userland-std/dyn64.sh PACKAGE` (into target/dyn64), which makes a
  sysroot without rustup's `libstd.so`. Such a program does not name
  rs2libc: it is in the DLL.

`libfoo.so` (or `libfoo-HASH.so`) is the module FOO: it goes on the disk as
FOO.DLL, in C:\RS2\DLL64 or along the LIBPATH.

How they work:

- `lld.sh` links start-up code into every module, crt64.rs: a program's
  `_start`, a DLL's `_DLL_InitTerm`. They hand `RS2_MODULE`, which
  link64.ld makes (unwind tables, constructors, thread-local storage), to
  the rs2libc that serves the module, be it in the module or in a DLL.
- **Thread-local storage.** A DLL's code reaches its variables through
  `__tls_get_addr`. The loader relocates the module number
  (R_X86_64_DTPMOD64) to the address of the DLL's RS2_MODULE, and rs2libc
  makes a thread's block when it is first asked for it. A program that uses
  variables of a DLL it is linked to does so through FS, so the loader
  lays out those DLLs' blocks below the program's
  (R_X86_64_TPOFF64; at most 32 DLLs) and puts the layout in the program's
  RS2_MODULE.
- rs2libc in a DLL finds its own per-thread data in a dword of
  thread-local memory: FS may be a program's that knows nothing of it.
  64-bit callers of DosAllocThreadLocalMemory have 992 dwords (32-bit ones
  OS/2's 32, which 64-bit code leaves alone).
- **Unwinding** finds the tables of all modules that share an rs2libc
  (frames.rs).

What to know: a cdylib's thread-locals in threads it did not create are
not dropped when those threads end, and their memory is freed with the
process. rs2libc copies in one process each remember what kind of file a
handle is: a handle should be closed by the module that opened it.

## Not there yet

- Linking with C code that uses LIBC64.DLL: both define `malloc`, `open`
  and the rest. Through a DLL with a C interface it works, since each
  module has its own.
- A target of its own: to the compiler it is Linux (`cfg!(unix)`,
  `target_os = "linux"`), `std::path` uses `/`: rs2libc takes both `/` and
  `\` and gives `/` (`current_dir`, `current_exe`, `canonicalize`), but
  `Path` does not split at `\` or know drives. `dir/C:/x`, from joining, is
  taken as `C:\x`.
- `fork`, symbolic and hard links, Unix sockets, `socketpair`, signals
  (Ctrl+C ends the process), IPv6, non-blocking pipes.
- Backtraces show no function names (`dl_iterate_phdr` finds nothing).

## Tests

```text
cargo xtask run --headless --timeout 150 --mem 6G --init "RS2\STDTST64.EXE"
cargo xtask run --headless --timeout 60 --init "RS2\PMSTD64.EXE /SELFTEST C:\OS2"
```

STDTST64 (userland-std/stdtest, also built as the 32-bit STDTEST) makes 111
checks: threads, synchronisation, thread-locals and their destructors,
files, directories, locks, times, the environment, panics, TCP and UDP,
child processes, memory, DLLs (STDTDLL.DLL, and STDTRUST.EXE on
STDTRLIB.DLL); its `mem64` section works with data above 4 GB
(it needs `--mem 6G`). PMSTD64 (userland-std/pmstd64) is a PM program: a
std thread walks a directory and the window shows the totals; with
`/SELFTEST` it checks std and PM together.
