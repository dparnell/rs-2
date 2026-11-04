# Rust's std and rs2's web browser

rs2's own web browser, **WebRS2** (`OS2\APPS\WEB.EXE`), is a Presentation
Manager program written in Rust from crates of the Servo project and the
Rust ecosystem, with no C++ anywhere. Getting there took three steps, each
usable on its own:

1. **Rust's standard library on rs2** (`userland-std/`, `rs2libc`): programs
   use `std` (threads, files, sockets, processes, panics with unwinding)
   and any crate built on it.
2. **WEBTEXT** (`OS2\APPS\WEBTEXT.EXE`), a text-mode browser: HTTP/1.1,
   HTTPS through rustls, html5ever.
3. **WEB** (`OS2\APPS\WEB.EXE`): CSS (Servo's `selectors` and `cssparser`),
   layout (Taffy for blocks, flexbox, grid and tables; inline text of its
   own), painting with GPI and rs2's outline fonts, images, links, history,
   forms. WPUrl objects open it.

JavaScript (Boa) is not done.

Porting Servo whole was ruled out: it needs SpiderMonkey (C++), WebRender
on OpenGL and a 64-bit address space.

## Stage 1: std for rs2

### The choice

rs2's userland was all `no_std`, built for `i686-unknown-linux-gnu` and
converted to LX by `tools/elf2lx`; the `os2` crate wraps the Dos* calls.
Two ways to get `std`:

- (a) a custom target with an rs2 `sys` layer in a patched std, built with
  `-Zbuild-std`. That needs a nightly toolchain and a fork of
  `library/std` to rebase on every Rust release.
- (b) keep `i686-unknown-linux-gnu` and rustup's **precompiled** std, and
  give it the C library it expects: a static library implementing the
  POSIX subset std calls, on top of the OS/2 API.

rs2 takes (b). The precompiled std works unchanged, on the stable
toolchain: a Rust upgrade is a rebuild, and only an std that starts calling
a new libc function needs a matching function in rs2libc (the link then
names it). The interface is glibc's, which std must keep stable for real
Linux systems, and the `libc` crate gives its exact i386 structure layouts.
C code links too: ring's C and assembly build with clang against rs2libc's
few headers (`userland-std/include`).

### How it fits together

`userland-std/` is a separate Cargo workspace (its own `.cargo/config.toml`,
profile with `panic = "unwind"` and link script) next to `userland/`:

| Piece | What it does |
|-------|--------------|
| `rs2libc` | The C library: `start` (LX entry point, argv from the command line as OS/2 C runtimes split it, `environ` from the environment block, `.init_array` called with argc/argv/envp as glibc does: std's `env::args` gets them that way), `tls`, `malloc` (dlmalloc over DosAllocMem in 1 MB steps), `thread` (pthreads over DosCreateThread; keys; `__cxa_thread_atexit_impl`; pthread mutexes for C), `futex` (std's Mutex, Condvar, RwLock, Once and parking are `syscall(SYS_futex)`: wait queues hashed by address, each thread sleeping on its own event semaphore), `file` (descriptors, files, directories, `openat`/`unlinkat` family, pipes), `net` (BSD sockets over RS2API 201-218, `poll`, `getaddrinfo` over Rs2NetResolve), `time`, `process` (`posix_spawn` over DosExecPgm, `waitpid`, `kill`), `misc` (environment, `sysconf`, signals as no-ops, anonymous `mmap`, a ChaCha20 `getrandom` seeded from RDRAND, the timers and the TSC), `math` (libm's functions under C names), `string` |
| `link.ld` | Code, read-only data and the unwind tables (`.eh_frame` found through `.eh_frame_hdr`) at 64 KB; data, `.init_array` and the TLS template in the next object |
| `syslib/` | Empty `libc.a`, `libpthread.a`... for the `-l` options std's link line carries |
| `include/`, `cc.sh`, `ar.sh` | C code: clang for i686, freestanding, rs2libc's headers; llvm-ar from rustup's llvm-tools |
| `tools/elf2lx` | Also skips TLS relocations (the linker has resolved them), undefined weak symbols (0) and `.tbss` |
| xtask | `STD_PROGRAMS`: builds the workspace, converts with a 4 MB stack |

Every program has `use rs2libc as _;` in its `main.rs`.

**Thread-local storage.** std's `thread_local!`s are ELF TLS: the linker
turns every access of the static executable into the local-exec form,
`%gs:-offset`. OS/2 leaves GS unused, so each thread gets an LDT alias
selector of its own (RS2API 421, the call WIN16.DLL uses for
AllocSelector: base the thread pointer, limit 4 GB so that negative
offsets wrap) loaded into GS. The kernel saves and restores GS with the
thread's other selectors; no kernel change was needed. The thread control
block at the thread pointer holds errno, pthread keys and the futex
semaphore. Each thread costs one 64 KB tile of address space for the
selector.

**Panics** unwind: the `unwinding` crate provides `_Unwind_RaiseException`
and the rest, reading `.eh_frame` through `.eh_frame_hdr`; std's own
personality routine does the rest. `catch_unwind`, drops while unwinding and
`JoinHandle::join` of a panicking thread work; backtraces print addresses
(no symbols: there is no `dl_iterate_phdr` information).

**Memory.** A process can allocate 464 MB in 16 MB blocks (the private
arena below the 512 MB line, less the program and the shared DLLs);
memory above 512 MB (OBJ_ANY) is not asked for, as rs2 does not offer it
yet.

**What does not work or differs:** `fork` (ENOSYS; std's `Command` uses
`posix_spawn`, which works, with redirections, a working directory and an
environment), signals (accepted and ignored; faults are OS/2 exceptions:
std's stack overflow message does not appear, the process ends with the
trap), non-blocking anonymous pipes (`Command::output` reads standard
output to its end before standard error, so a child writing more than a
pipe's worth to standard error first can block), IPv6, `select()`,
symbolic links, `mmap` of files (a private copy is read), file sizes over
2 GB. Paths may use `/` or `\`; `/dev/null` is `NUL`. Console output gets
`\r\n` for `\n`, console input the reverse.

### STDTEST

`RS2\STDTEST.EXE [sections]` (default: all but `host`): `env` (arguments,
variables, `current_exe`, `current_dir`), `time`, `threads` (a Mutex
under 8 threads, Condvar ping-pong and timeouts, RwLock, channels, Barrier,
scoped threads, thread-locals and their destructors, park/unpark, named
threads with a stack size, 200 threads in turn, sleeping), `fs` (create,
write and read 100 KB, append, seek, set_len, copy, rename over a file,
read_dir with file types, metadata and times, errors, canonicalize,
relative paths, remove_dir_all on C:), `panic`, `net` (loopback TCP with 1
MB echoed through `try_clone`, refused connections, `connect_timeout`, read
timeouts, non-blocking reads and accepts, UDP, name lookup), `process`
(`Command` output through a pipe, exit codes, piped standard input and
output, a missing program), `mem` (a 1M-entry HashMap, a 100 MB Vec, the
address space limit), `host HOST:PORT` (an HTTP GET from a server on the
host). 69 checks, all passing:

```
cd /tmp/www && python3 -m http.server 8471 --bind 127.0.0.1 &
cargo xtask run --headless --net --timeout 200 \
  --init "RS2\STDTEST.EXE host 10.0.2.2:8471 env time threads fs panic net process mem"
```

## Stage 2: WEBTEXT

`userland-std/webcore` is the engine both browsers share:

- `http`: HTTP/1.1 (one connection per request; chunked and length bodies;
  gzip and deflate; redirects; a cookie jar; POST), HTTPS through
  **rustls** with the **ring** crypto provider (its C and assembly build
  for i686 with clang) and Mozilla's root certificates (`webpki-roots`);
  `file:` (directories get an index page), `data:` and `about:` URLs;
  charsets from the header, `<meta>` or a guess (`encoding_rs`).
- `dom`: html5ever builds an arena tree (`TreeSink` of its own).
- `text`: a document as wrapped text with numbered links.
- `codepage`: Unicode to code pages 437/850 (with look-alikes for quotes,
  dashes and the like).

`WEBTEXT [-dump] [-width N] URL` pages a document on the console: Enter for
more, a number to follow a link, `g URL`, `b` back, `r` reload, `l` links,
`q`. `-dump` prints the page and its links. Verified under QEMU with a
host page, https://example.com and https://en.wikipedia.org/wiki/OS/2
(certificate checked; 3900 lines, 1068 links).

## Stage 3: WEB, the PM browser

```
WEB [URL]
```

A frame with Back, Forward, Reload, Home, the address field and Go, the
page with its scroll bar, and a status line (what is loading, then where
the link under the pointer goes). Keys: arrows, Page Up/Down, Space,
Home/End; Tab and Shift+Tab move a dotted focus through the links, Enter
follows it; Alt+Left or Backspace back, Alt+Right forward, F5 reload,
Ctrl+L the address field. A panic in the engine does not end the program:
the status line says so and the message goes to WEB.LOG.

How a page is shown:

1. A worker thread fetches the page, parses it, fetches its style sheets
   (`<link rel=stylesheet>`, `@import`, `<style>`; `media` attributes) and
   computes every element's style (`webcore::page`, `style`), posting
   progress to the window.
2. The window's thread lays the page out at the window's width
   (`webcore::layout`) with GPI measuring text, and paints the display
   list (`web/src/draw.rs`) on scrolling and exposure.
3. Another thread fetches and decodes the images (`png`, `jpeg-decoder`,
   `gif`, BMP); as their sizes arrive the page is laid out again (once per
   burst, on a timer).

**Styles** (`css.rs`, `select.rs`, `style.rs`): `cssparser` tokenizes;
Servo's `selectors` crate parses and matches selectors (the `Element`
trait over the arena DOM; `:link`, `:checked`, `:disabled`, `:is()`,
`:where()`, `:not()`, `:nth-child()` and the rest of what it supports;
`:hover` and pseudo-elements never match). Rules are indexed by the id,
class or element name of their rightmost compound selector. The cascade
orders the user-agent sheet (HTML's defaults), presentational attributes
(`width`, `bgcolor`, `align`, `<font>`, table `border` and `cellpadding`),
author rules and `style` attributes by origin, importance, specificity and
order. `@media` is evaluated for the window (`min-width`, `max-width`,
orientation, `prefers-color-scheme: light`...); custom properties and
`var()` are substituted; lengths take px, em, rem, %, pt, vw/vh and
`calc()`/`min()`/`max()`/`clamp()`. Colours: hex, `rgb()`, `hsl()`, the
named colours, `light-dark()`. Restyled when the window's width changes.

**Layout** (`layout.rs`): a box tree is built from the styled DOM —
block-level elements become Taffy nodes (block, flex, grid; tables become
grids with a column per cell position; `display: contents` is flattened),
inline content between them becomes inline formatting contexts, which
are Taffy leaves measured and laid out by webcore itself: white space
(normal, nowrap, pre, pre-wrap, pre-line), line breaking at spaces (long
words broken between letters), line boxes with half-leading, text
alignment, `text-indent`, `text-transform`, list markers (bullets drawn,
numbers, letters and roman numerals as text), inline boxes' margins,
borders and padding, backgrounds behind inline text, sub/superscript, and
atomic inlines: images (sized from attributes, CSS or the image), form
controls and inline-blocks (laid out as separate Taffy trees). Taffy
places floats; a second pass shortens the lines of inline contexts beside
them (each context gets the floats before it that overlap it as
exclusions). `overflow: hidden` clips (GPI clip rectangles); boxes with
`opacity: 0`, clipped to nothing (`clip: rect(0 0 0 0)`, `clip-path:
inset(50%)`) or hidden by a 1-pel overflow box are not drawn. The result is a display list
(rectangles, text runs, images, markers, controls) with link rectangles
and element positions for `#fragment` links.

**Painting** (`web/src/draw.rs`): GPI outline fonts at the CSS size
(Helvetica, Times New Roman and Courier and their bold, italic and bold
italic faces, which rs2 draws from the Liberation fonts), text in code page
850, `GpiBox` for backgrounds and borders, `GpiDrawBits` for images (alpha
blended over the page background), `GpiFullArc` for bullets. Text widths
come from `GpiQueryTextBox`, a table of 256 per font.

**Forms:** text and password fields (an entry field is put over the field
while it is edited), checkboxes, radio buttons, selects (a click shows the
next option), submit buttons; sent with GET or POST
(`application/x-www-form-urlencoded`).

**Default browser.** A WPUrl object (`userland/pmwp/src/url.rs`) starts the
browser OS2.INI names (`WPURLDEFAULTSETTINGS`, `DefaultBrowserExe`); when
none is set up it now starts `OS2\APPS\WEB.EXE` from the boot drive.

### Tested

Under QEMU (screenshots in `work/progress/browser-rust/`):

- the host test site (flexbox header, a three-column grid, a sidebar,
  borders, an alpha PNG, a form);
- https://example.com/;
- https://en.wikipedia.org/wiki/OS/2 (9000 elements, 1700 rules; styled in
  about 60 ms on the host): vector-2022's grid areas put the table of
  contents beside the article and the text wraps around the infobox;
- a WPUrl object on the desktop (made with WPSTEST's CREATE), selected
  with the arrow keys and opened with Enter, starts WEB.EXE on its URL
  (OS2.INI names no browser).

`webshot` renders a page on the host to a PNG with webcore and the same
Liberation fonts (fontdue), for working on styles and layout without
QEMU:

```
cd userland-std
cargo run --release -p webshot --target aarch64-apple-darwin -- https://example.com/ /tmp/shot.png 900 1200
WEBCORE_TREE=1 ...      # print Taffy's tree
WEBCORE_MEASURE=1 ...   # trace inline measurements
```

### Not done

JavaScript (Boa, stage 4), `position: fixed` (laid out as absolute),
background images, named grid lines (areas work), boxes avoiding floats
(only lines do), `::before`/`::after` content,
web fonts, borders' styles (all are solid), rounded corners, transforms,
`:hover`, HTTP keep-alive and caching, persistent cookies, IPv6, frames.
Italic GPI faces look smaller than the upright ones at the same size (an
rs2 font issue).

## Third-party code

All from crates.io, under permissive licences: rustls (Apache-2.0/ISC/MIT),
ring (Apache-2.0 and ISC), rustls-webpki and untrusted (ISC),
webpki-roots (CDLA-Permissive-2.0, Mozilla's root list), html5ever,
markup5ever, string_cache, url, idna, flate2, miniz_oxide, png, gif,
jpeg-decoder, taffy, dlmalloc, unwinding, gimli, libm, libc (MIT and/or
Apache-2.0), encoding_rs (MIT/Apache-2.0 and BSD-3-Clause), cssparser,
selectors, dtoa-short (MPL-2.0, used unmodified), foldhash and slotmap
(Zlib), subtle (BSD-3-Clause), ICU4X data crates (Unicode-3.0). webshot
(host only) adds fontdue (MIT/Apache-2.0/Zlib).
