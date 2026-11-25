# The developer disk

An optional CD image, `rs2-dev.iso` (about 300 MB), with the compilers that
run on rs2 ([SELF-HOSTING.md](SELF-HOSTING.md)): everything to develop
64-bit programs there in Rust, C and C++, and to build rs2 itself. Nothing
on it is needed to run rs2; the compilers are not rs2's code, so they are
not on the live ISO.

## Getting it

Each GitHub release has it as `rs2-dev.iso`, and
[build-devdisk.yml](../.github/workflows/build-devdisk.yml) keeps main's as
the artifact `rs2-dev` of its run. (The compilers take hours to build the
first time; later runs use cached results unless something that goes into
them changed. It can also be started by hand.)

To build it, after the three compilers' build scripts:

```text
userland64/llvm/build.sh
userland64/rustc/build.sh
userland64/cargo/build.sh
userland64/devdisk/build.sh [ISO]      (work/rs2-dev.iso)
```

## Installing

rs2 has to be installed with the system, or another partition, on HPFS or
JFS: the compilers' file names need it. With the disk in the drive:

- **SETUP.EXE** is a Presentation Manager program: choose the drive, the
  directory and whether to install rs2's sources too (they are not unless
  the box is checked). It also makes a Developer folder on the desktop.
- **INSTALL.EXE** does the same at a command prompt:

  ```text
  INSTALL [/DIR:path | /DRIVE:d] [/SOURCE | /NOSOURCE] [/YES] [/LIST]
  ```

  Without options it asks. With `/YES` it does not, and installs the
  compilers only unless there is `/SOURCE` as well.

The directory is `C:\RS2DEV`, unless the boot drive is not HPFS or JFS or is
too full. The compilers take about 760 MB, the sources 260 MB more and a
build of rs2 about 2.5 GB. Running an installer again, with a newer disk for
example, replaces the files; with another directory, CONFIG.SYS follows.

What changes besides the directory is the boot drive's CONFIG.SYS (the old
one is kept as CONFIG.DEV):

| Setting | Gets |
| --- | --- |
| `SET PATH`, `LIBPATH` | `C:\RS2DEV\sdk\bin` at the end |
| `SET CARGO_HOME` | `C:\RS2DEV\cargo` (its config.toml: no network, no git) |
| `SET TMPDIR` | `C:\RS2DEV\tmp`, if it is not set already |

Restart the computer for that. `C:\RS2DEV\DEVENV.CMD` sets the same for one
command prompt without it; the Developer folder's command prompt runs it. To
remove everything, delete the directory and take those entries out.

## What is there

| Directory | |
| --- | --- |
| `sdk\bin` | clang, clang++, lld, llvm-ar, rustc, cargo, rc64, the linkers rustc runs |
| `sdk\include`, `sdk\lib` | headers and libraries for C and C++; Rust's standard library for 64-bit programs, the 32-bit userland, the kernel and the boot loader |
| `sdk\crates` | the `os2` crate (the OS/2 API for Rust) and the resource compiler |
| `samples` | text, PM and DLL programs; `BUILD.CMD` builds them all |
| `docs`, `licenses` | |
| `src` | rs2's sources with their dependencies (optional); `BUILD.CMD` makes the live ISO |

`rc64 PROG.RC -o PROG_RC.s` compiles a resource script into assembler source
that goes on clang's command line (`clang -mwindows prog.c PROG_RC.s`).

## How it is made

`userland64/devdisk/build.sh` puts together the toolchain's tree (sdk.sh),
`userland64/devdisk/files`, and the last commit's sources with `cargo
vendor`'s output; `pack.py` makes SDK.ZIP and SOURCE.ZIP of them, and `cargo
xtask devdisk` the installers (`userland/devinst`; crates/inflate) and the
ISO 9660 image, whose root directory has only these files.

A file `NAME.rs2in` in the archives is installed as `NAME` with `@DIR@`,
`@UDIR@` (with slashes) and `@DRIVE@` replaced; so userland-std's cargo
configuration, which names shell scripts elsewhere, gets the tools there.

`tests/devdisk/run.sh` installs it in QEMU and builds and runs the samples
and a new cargo project; `--rs2` also builds rs2's 64-bit programs there.
