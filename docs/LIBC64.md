# The 64-bit C library

64-bit rs2 programs written in C get a C library ([API64.md](API64.md),
"Building 64-bit programs"): userland64/libc, written for rs2 on top of
the OS/2 API, with the standard headers in userland64/include next to the
OS/2 ones. `userland64/c/cc64.sh` builds it into `target/libc64/libc.a`
(again whenever a source or header is newer than the archive) and links
every program and DLL against it.

```text
userland64/c/cc64.sh -o PROG.EXE prog.c util.c       a program
userland64/c/cc64.sh -c prog.c -o prog.o              compile only
userland64/c/cc64.sh -o PROG.EXE prog.o libfoo.a      link objects
userland64/c/cc64.sh --libc                           just the library
```

Code is compiled as hosted C (`-std=gnu11`) with `-nostdinc`, so only
rs2's headers are seen, never the host's. The library itself is built
`-ffreestanding -fno-builtin`.

## Data model

LP64, as on Linux and other 64-bit Unix systems: `int` is 32 bits;
`long`, `size_t`, pointers and `time_t` are 64. `wchar_t` is 32 bits.
`off_t` and `fpos_t` are 64-bit, so files over 4 GB can be read and
written (DosOpenL, DosSetFilePtrL, DosSetFileSizeL). `char` is signed.
`long double` is the x87 80-bit type, but the library computes its
`long double` functions in double precision, and printf prints
`long double` values as doubles.

## What is in it

| Header | Contents |
| --- | --- |
| `assert.h`, `ctype.h`, `errno.h`, `float.h`, `inttypes.h`, `iso646.h`, `limits.h`, `locale.h`, `math.h`, `setjmp.h`, `signal.h`, `stdalign.h`, `stdarg.h`, `stdbool.h`, `stddef.h`, `stdint.h`, `stdio.h`, `stdlib.h`, `stdnoreturn.h`, `string.h`, `time.h` | C11 (no threads.h, wchar.h, uchar.h, complex.h, fenv.h yet) |
| `strings.h`, `unistd.h`, `fcntl.h`, `sys/types.h`, `sys/stat.h`, `sys/time.h`, `dirent.h`, `alloca.h` | the POSIX subset that portable programs use most |
| `io.h`, `direct.h`, `process.h`, `malloc.h` | what OS/2 and DOS C compilers provide: `setmode`, `filelength`, drives, `_beginthread`, `spawnv`/`spawnvp`, `cwait` |

## How it maps onto OS/2

- **Start-up.** `_start` (start.c) is called by the loader as
  `_start(hmod, 0, env, cmdline)`. It splits the arguments the way OS/2
  C runtimes do: blanks separate them, double quotes group, `\"` is a
  quote, and backslashes double only before a quote. It builds `environ`,
  registers the exception handler for signals, runs constructors
  (`.init_array`, which `link64.ld` collects) and calls
  `main(argc, argv, envp)`. `exit` runs the `atexit` functions and
  destructors, flushes the streams, deletes temporary files and calls
  `DosExit(EXIT_PROCESS)`.
- **File descriptors are OS/2 file handles.** 0, 1 and 2 are stdin,
  stdout and stderr. `open` maps O_* flags onto DosOpenL's open action
  and mode, always with `OPEN_FLAGS_FAIL_ON_ERROR`, so that errors come
  back as errors and not as hard-error pop-ups. `open` is binary unless
  given `O_TEXT`; `setmode` switches a handle.
- **Text and binary.** Streams opened without "b" are text streams:
  CR LF reads as LF, Ctrl+Z ends the file, and LF is written as CR LF.
  The stream buffer holds the bytes as they are in the file, so `ftell`
  and `fseek` positions are file offsets. stdin, stdout and stderr are
  text. Handles 0-2 are also text at the handle level, for `read` and
  `write`. stdout is line buffered on a console (DosQueryHType says
  "device") and fully buffered otherwise. stderr is unbuffered.
- **Memory.** malloc keeps chunks with boundary tags in 4 MB regions
  from DosAllocMem. Free chunks sit in size-class bins and are merged
  with their neighbours when freed. Blocks of 1 MB or more get regions
  of their own, which are given back by `free`. All of it is below
  4 GB (no OBJ_ANY), so any malloc pointer can be passed to any OS/2
  call (API64.md, "Memory"). One spin-then-yield lock covers all
  threads.
- **Threads.** Per-thread state (errno, strtok, the gmtime and asctime
  buffers, tmpnam, the signal save area) lives in a page per thread. One
  dword of thread-local memory (DosAllocThreadLocalMemory) points to it.
  Threads that use the library should start with `_beginthread`, whose
  `_endthread` frees that page. Threads made with DosCreateThread work
  too, but their page is not freed when they end. Streams have
  recursive locks.
- **errno** is set from the APIRET of the failing call. For example,
  ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND become ENOENT, and
  ERROR_ACCESS_DENIED and ERROR_SHARING_VIOLATION become EACCES. DosOpen's
  ERROR_OPEN_FAILED becomes EEXIST under `O_CREAT|O_EXCL` and ENOENT
  otherwise. Some calls check further, so that a directory gives EISDIR
  and an existing name gives EEXIST from `mkdir`. `rename` replaces an
  existing file, as C expects and DosMove does not.
- **Signals.** `_start` registers one exception handler. It turns
  XCPT_SIGNAL into SIGINT (Ctrl+C), SIGBREAK (Ctrl+Break) and SIGTERM
  (DosKillProcess). Access violations become SIGSEGV, illegal and
  privileged instructions SIGILL, and integer or floating-point
  exceptions SIGFPE. When a handler is set, the exception handler
  acknowledges the signal and saves the registers. It then changes the
  context so that the thread resumes in a trampoline (setjmp.S) and
  returns XCPT_CONTINUE_EXECUTION. By the time the C handler runs, the
  kernel has finished with the exception, so the handler may `longjmp`
  out. If it returns, IRETQ resumes the interrupted code. IRETQ loads
  RSP without touching the red zone below it. A fault resumes at the
  faulting instruction, as on Unix. With SIG_DFL the exception goes on
  to the system's default action. Setting a SIGINT or SIGBREAK handler
  also calls DosSetSignalExceptionFocus, so that Ctrl+C reaches the
  program.
- **Programs.** `system` runs `%COMSPEC% /c` (CMD.EXE by default) with
  DosExecPgm. `spawnv`/`spawnvp` quote the arguments so that the child's
  runtime splits them back the same way, and pass `environ` as the
  environment. `spawnvp` searches PATH and adds `.EXE`. `popen` puts one
  end of a DosCreatePipe pipe on the child's handle 0 or 1 for the
  length of the DosExecPgm call. `pclose` waits with DosWaitChild.
- **Time.** DosGetDateTime gives local time. `TZ` says how far that is
  from UTC. Both POSIX rules (`EST5EDT,M3.2.0,M11.1.0`) and OS/2's
  (`EST5EDT,3,2,0,7200,11,1,0,7200,3600`) are understood. A DST name
  without rules gets the US rules. With no TZ, DATETIME's time zone field
  is used if it is set, and local time is UTC otherwise. `clock` counts
  milliseconds from QSV_MS_COUNT since start-up.

## Numbers

- **printf** converts floating point exactly: the decimal expansion of a
  double is finite, and printf computes it with base-10^9 big integers
  (at most about 770 digits). Rounding is half to even. So
  `%.0f` of DBL_MAX prints all 309 digits, and `%.20e` of the smallest
  denormal is right to the last digit. `%a` is exact, and so are the
  integer conversions.
- **strtod** keeps 19 significant digits exactly and applies the power
  of ten in 80-bit long double. The result is correctly rounded except,
  rarely, very close to a halfway case. CLIBTEST round-trips 20,000
  random doubles through `%.17g` with no differences. Hexadecimal input
  is exact.
- **math** uses the x87 unit in 80-bit precision: fyl2x, f2xm1 and
  fscale for logarithms, exponentials and pow; fsin, fcos and fpatan for
  trigonometry, with fprem1 argument reduction for large arguments; fprem
  for fmod. The rounding functions work on the bits. erf and erfc use a
  series and a continued fraction in long double, and the gamma functions
  use Lanczos. Domain and range errors set errno (`math_errhandling` is
  MATH_ERRNO). fma is not fused.

## Not there yet

- wide characters beyond `mbtowc`/`wctomb` in the "C" locale (wchar.h,
  wctype.h); other locales.
- threads.h and C11 atomics headers (clang's `__atomic` builtins work).
- a shared C library DLL: each program and DLL links its own copy. A DLL
  has no start-up, so it has no `environ` (getenv asks DosScanEnv) and no
  constructors, and it has its own heap.
- `fork`, `exec*`, `select`, sockets, `mmap`, `uid`/`gid` (there are
  none on OS/2).
- `long double` functions in full precision.

## Tests

`C:\RS2\CLIBTEST.EXE` (userland64/c/samples/clibtest.c) makes 152 checks:
strings, conversions, printf (including exact float output), scanf,
strtod round trips, the heap under a random load and in large blocks,
sorting, math against known values, setjmp, SIGSEGV, SIGFPE and SIGINT
handlers that longjmp or return, time zones and strftime, text and
binary streams, handles, directories, the environment, threads with their
own errno, `spawnv` with quoted arguments and an inherited variable,
and `popen`. It prints one line per check and
`clibtest: N checks, M failures`, and exits with M:

```text
cargo xtask run --headless --timeout 120 --init "RS2\CLIBTEST.EXE"
```
