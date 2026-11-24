# The 64-bit C library

64-bit rs2 programs written in C get a C library ([API64.md](API64.md),
"Building 64-bit programs"): userland64/libc, written for rs2 on top of
the OS/2 API, with the standard headers in userland64/include next to the
OS/2 ones. It comes as a DLL, `C:\RS2\DLL64\LIBC64.DLL`, which programs
and DLLs use by default, and as a static library for `--static`.

```text
userland64/c/cc64.sh -o PROG.EXE prog.c util.c       a program (LIBC64.DLL)
userland64/c/cc64.sh -o PROG.EXE --static prog.c     the library linked in
userland64/c/cc64.sh -o MY.DLL --dll my.c            a DLL (LIBC64.DLL)
userland64/c/cc64.sh -c prog.c -o prog.o              compile only
userland64/c/cc64.sh -o PROG.EXE prog.o libfoo.a      link objects
userland64/c/cc64.sh --libc                           just the library
```

`cc64.sh` builds these into `target/libc64` (again whenever a source,
a header or the script is newer than them):

| File | What |
| --- | --- |
| `LIBC64.DLL` | the library as a DLL (xtask installs it in `C:\RS2\DLL64`); it exports the library's 410 functions and its data by name |
| `libc64.a` | its import library: one member per function, a stub `name: jmp *LIBC64.name@GOTPCREL(%rip)`, so a program imports only what it calls |
| `crt1.o` | a program's own start-up code (`_start`), linked into each program that uses the DLL |
| `crtdll.o` | a DLL's own start-up code (`_DLL_InitTerm`), linked into each DLL cc64.sh makes |
| `libc.a` | the library to link in (`--static`), crt1.o included |

## The DLL

- **One library per process.** A program and the C DLLs it loads share
  LIBC64.DLL, and so they share its state: one heap (memory malloc'd in a
  DLL may be freed by the program), one set of streams and buffers
  (`stdout` is one buffer), one environment, one `atexit` list, and one
  `errno` per thread. With `--static`, each module has its own copy of
  all of these.
- **Per process.** As with any 64-bit DLL, LIBC64.DLL's code is shared
  and its writable data is each process's own copy (the loader keeps the
  relocated data and copies it into each process that loads the DLL).
  It is linked at 0x18100000, which the loader uses if that range is
  free.
- **Start-up.** `crt1.o`'s `_start` passes the program's module handle,
  environment, command line, `main` and its own constructor and
  destructor lists (`__init_array_start` and the rest) to `__rs2_start`
  in the library. That function does everything else, as it does when
  linked statically. LIBC64.DLL itself has no start-up code: its state
  starts as its data was initialised.
- **C DLLs' constructors and destructors.** Every DLL cc64.sh links gets
  `crtdll.o`, which exports `_DLL_InitTerm(hmod, flag)`. The kernel calls
  that once per process (see API64.md, "Executables"): when the process
  starts using the DLL (before `main` for a DLL the program imports, or
  inside DosLoadModule) and when it stops. With flag 0 it runs the DLL's
  constructors and puts a function running its destructors on the C
  library's exit list with `__cxa_atexit`, tagged with the DLL's
  `__dso_handle`. So `exit()` runs the DLL's destructors, in reverse
  order with the program's own and the `atexit` functions, while stdio
  still works. With flag 1 (DosFreeModule, or a process that ends without
  `exit`) it calls `__cxa_finalize` for the DLL, which runs its
  destructors if they have not run yet. A DLL loaded again after its last
  DosFreeModule gets fresh data and is constructed again, as OS/2 does.
  Constructors run before the program's `__rs2_start`, so `environ` is
  not set yet, but `getenv` asks DosScanEnv until it is.
- **Functions** are imported through the stubs. A stub is a single jump,
  so even `setjmp` works through one. **Data** is imported directly: with
  `-D_RS2_LIBC_DLL`, which `cc64.sh` passes unless given `--static`, the
  headers give the library's variables asm labels (`stdin`, `stdout`,
  `stderr`, `environ`, `optarg`, `optind`, `opterr`, `optopt`,
  `timezone`, `daylight`, `tzname`, `signgam`). The program then refers to
  `LIBC64.stdout`, and the loader resolves that to the DLL's variable. So
  an object compiled for one way must not be linked the other way.

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
| `assert.h`, `ctype.h`, `errno.h`, `float.h`, `inttypes.h`, `iso646.h`, `limits.h`, `locale.h`, `math.h`, `setjmp.h`, `signal.h`, `stdalign.h`, `stdarg.h`, `stdbool.h`, `stddef.h`, `stdint.h`, `stdio.h`, `stdlib.h`, `stdnoreturn.h`, `string.h`, `time.h`, `fenv.h`, `wchar.h`, `wctype.h`, `uchar.h` | C11 (no threads.h, complex.h yet) |
| `strings.h`, `unistd.h`, `fcntl.h`, `sys/types.h`, `sys/stat.h`, `sys/time.h`, `sys/statvfs.h`, `sys/random.h`, `dirent.h`, `alloca.h`, `pthread.h`, `sched.h` | the POSIX subset that portable programs use most |
| `sys/mman.h`, `sys/resource.h`, `sys/wait.h`, `sys/ioctl.h`, `sys/utsname.h`, `sys/statvfs.h`, `spawn.h`, `dlfcn.h`, `pwd.h`, `sysexits.h`, `endian.h` | more of POSIX, what LLVM uses (below) |
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
- **Memory.** malloc keeps chunks with boundary tags in regions from
  DosAllocMem. Free chunks sit in size-class bins and are merged with
  their neighbours when freed. Blocks of 1 MB or more get regions of their
  own, which are given back by `free`. The first 96 MB of regions are
  below 4 GB (4 MB each), so a program of ordinary size can pass any
  malloc pointer to any OS/2 call (API64.md, "Memory"). After that, and
  for single blocks of 16 MB or more, memory comes from above 4 GB (16 MB
  regions); whichever side is full, the other is tried. The library's own
  functions take pointers from anywhere: they copy names and argument
  blocks below 4 GB for the calls that need it (and take a path `/C:/dir`,
  as cargo writes them, for `C:/dir`). A program that uses a lot
  of memory and calls the OS/2 API itself passes memory from the stack,
  static data or DosAllocMem without OBJ_ANY. One block is at most
  4 GB - 64 KB. One spin-then-yield lock covers all threads.
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

- **More of POSIX.** `mmap` gives memory above 4 GB; a file is read into
  it whole, and a shared writable mapping is written back by `msync` and
  `munmap` (which takes whole mappings). `posix_spawn` does the file
  actions in the parent around DosExecPgm, and `waitpid` gives its status
  in Unix's form; `exec*` runs the program and ends with its result;
  `fork` fails. `st_ino` is a hash of the full path, kept for open handles.
  `sigaction` is `signal`; signals cannot be blocked. `fcntl` locks are
  DosSetFileLocks, which keep others from reading too.

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

## Threads, thread_local

`pthread.h` (threads, mutexes, condition variables, read-write locks,
once, keys) is built on a futex, `__rs2_wait`/`__rs2_wake`
(userland64/libc/pthread.c). `thread_local` variables work (emulated TLS)
and are freed, with their destructors run, when a thread ends by
`_endthread` or `pthread_exit` or returns. C++ is in
[CXX64.md](CXX64.md).

## Not there yet

- other locales than "C": multibyte text is single bytes, and wide
  formatted I/O goes through the narrow functions.
- threads.h and C11 atomics headers (clang's `__atomic` builtins work).
- with `--static`, a DLL has no `environ` (getenv asks DosScanEnv), and
  its destructors run at its termination, after the program's `exit` has
  flushed the program's own streams.
- versioning of LIBC64.DLL's exports: programs import by name, so adding
  functions is safe, but changing a structure's layout (`FILE` is opaque;
  `struct stat`, `struct tm` and `jmp_buf` are not) means rebuilding
  programs.
- `select`, sockets; `fork` and users (there are none on OS/2).
- `long double` functions in full precision.

## Tests

`C:\RS2\CLIBTEST.EXE` (userland64/c/samples/clibtest.c, with LIBC64.DLL)
and `C:\RS2\CLIBTSTS.EXE` (the same program, `--static`) make 175 checks:
strings, conversions, printf (including exact float output), scanf,
strtod round trips, the heap under a random load and in large blocks,
sorting, math against known values, setjmp, SIGSEGV, SIGFPE and SIGINT
handlers that longjmp or return, time zones and strftime, text and
binary streams, handles, directories, the environment, threads with their
own errno, `spawnv` with quoted arguments and an inherited variable,
`popen`, and C DLLs' constructors and destructors. The constructor of
`C:\RS2\DLL64\CLIBTDL2.DLL` (clibtdll.c again), which the program
imports, ran before `main`. DosLoadModule of `CLIBTDLL.DLL` runs its
constructor, and DosFreeModule its destructor, which leaves a file
behind. Loaded again, the DLL is constructed again. A child process that
loads one DLL and imports the other leaves both files when it calls
`exit`. With the DLL there are four more checks: CLIBTEST loads
`C:\RS2\DLL64\CLIBTDLL.DLL` (clibtdll.c, a C DLL), frees a string the
DLL malloc'd, has the DLL print into the same `stdout` buffer, and sees
the `errno` the DLL set. It prints one line per check and
`clibtest: N checks, M failures` (195 and 191), and exits with M:

```text
cargo xtask run --headless --timeout 120 --init "RS2\CLIBTEST.EXE"
cargo xtask run --headless --timeout 120 --init "RS2\CLIBTSTS.EXE"
```
