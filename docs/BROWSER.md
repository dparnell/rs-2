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
4. **JavaScript** through Boa with a small DOM (`userland-std/webjs`):
   scripts run once while a page loads.

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
| `lld.sh` | The linker: rustup's rust-lld with `link.ld` and `syslib/`, paths found from the script's own directory (rustc links crates.io crates' cdylibs in their own directories; those are not used on rs2 and come out empty) |
| `link.ld` | Code, read-only data and the unwind tables (`.eh_frame` found through `.eh_frame_hdr`) at 64 KB; data, `.init_array` and the TLS template in the next object |
| `syslib/` | Empty `libc.a`, `libpthread.a`... for the `-l` options std's link line carries |
| `include/`, `cc.sh`, `ar.sh` | C code: clang for i686, freestanding, rs2libc's headers; llvm-ar from rustup's llvm-tools (`rustup component add llvm-tools`), else `llvm-ar` on PATH. Building needs `clang` on PATH (Xcode's does) |
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

**Memory.** rs2libc's `malloc` and `mmap` ask DosAllocMem for OBJ_ANY
memory: above 512 MB while the high private arena has room
(VIRTUALADDRESSLIMIT, docs/ARCHITECTURE.md "Memory layout"), then below
(on a kernel without high memory OBJ_ANY is refused and it asks again
without). STDTEST's `mem` gets 896 MB in 16 MB blocks at the default
limit (1024: 464 MB low, 432 MB high) and 2688 MB with
VIRTUALADDRESSLIMIT=3072; with VIRTUALADDRESSLIMIT=512 it is the 464 MB
of the low private arena (less the program and the shared DLLs).

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

`tools/scripts/webtest.sh` serves `tools/webtest` (flexbox header, a grid
of cards, a sidebar, an alpha PNG, a form, a JavaScript page) and drives
WEB under QEMU: a mouse click on a link, Alt+Left, Tab Tab Enter, a word
typed into the form's field and Enter (the server sees
`GET /second.html?q=hello`), Ctrl+L and an address. Screenshots
`work/webtest1.png`...

Under QEMU (screenshots in `work/progress/browser-rust/`):

- the host test site (flexbox header, a three-column grid, a sidebar,
  borders, an alpha PNG, a form);
- https://example.com/;
- https://en.wikipedia.org/wiki/OS/2 (9000 elements, 1700 rules; styled in
  about 60 ms on the host): vector-2022's grid areas put the table of
  contents beside the article and the text wraps around the infobox;
  with its scripts run, 22 s with images under TCG (layout 180 ms);
- a test page whose scripts (inline and from a file) rewrite a heading,
  build a list, show a box on DOMContentLoaded and fill in a timer's
  result;
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

## Stage 4: JavaScript

`userland-std/webjs` runs a page's scripts with **Boa** (a JavaScript
engine in Rust) after the page is parsed and before it is styled, so what
they change is what is shown. Scripts run in document order (inline and
`src`; not modules or JSON), then DOMContentLoaded and load listeners,
then the timers they set (in time order, a timer's own timers too). The
DOM is JavaScript (`webjs/src/prelude.js`) over about thirty native
functions on webcore's node ids: `document.getElementById`,
`querySelector(All)` (Servo's selectors), `getElementsByTagName/ClassName`,
`createElement`, `createTextNode`, `textContent`, `innerHTML` and
`insertAdjacentHTML` (parsed by html5ever), `outerHTML`, attributes, `id`,
`className`, `classList`, `dataset`, `style` (a proxy writing the `style`
attribute), `appendChild`/`insertBefore`/`removeChild`/`remove`/`append`,
the tree's links, `document.title`, `document.write` (appends to the
body), `console` (to `WEB.LOG` with the scripts' errors), `location`,
`navigator`, `localStorage` (per page), `setTimeout`. Events after loading
(clicks, input) run nothing, there is no XMLHttpRequest or fetch, and
`<noscript>` content goes. A loop iteration limit (1 million), a recursion
limit and 2 MB of script per page keep pages from hanging the browser;
`WEB -nojs` runs no scripts. The host tests (`cargo test -p webjs --target
aarch64-apple-darwin`) change a document through most of the DOM and
check errors and limits.

### Not done

Scripts after loading (events, XMLHttpRequest/fetch), `position: fixed` (laid out as absolute),
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
(Zlib), subtle (BSD-3-Clause), ICU4X data crates (Unicode-3.0), Boa
(boa_engine and its crates: Unlicense or MIT). webshot (host only) adds
fontdue (MIT/Apache-2.0/Zlib).

<!-- Separate section (browser-os2 work): unmodified OS/2 ports of
     open-source browsers. -->

## OS/2 open-source browsers

The OS/2 ports of open-source programs that bitwiseworks and netlabs
build with GCC and kLIBC (the C library of the ArcaOS ecosystem) are
unmodified OS/2 programs, so they are a test of how far rs2's kernel,
DOSCALLS and PM go beyond what IBM's own programs use. The goal was
bitwiseworks' Firefox 45.9 ESR for OS/2; on the way, the command-line
programs of the same RPM repository.

None of these files are part of the repository or of rs2's images. They
are test inputs, downloaded from their official sources and unpacked on
the host with tools that only read them (bsdtar reads both RPM and 7z
archives), never run on the host.

### Getting the files

| Package | Where | Notes |
|---------|-------|-------|
| `firefox-45.9.0-4.oc00.pentium4.7z` | github.com/bitwiseworks/mozilla-os2, release FIREFOX_45_9_0esr_RELEASE_OS2_GA2 | Firefox itself; its `RPM_REQUIREMENTS` lists the RPMs it needs. |
| `libc-0_1_14-1_oc00.zip` | rpm.netlabs.org/release/00/zip | kLIBC: LIBCN0.DLL, and LIBC06x.DLL forwarders to it. Do **not** also unpack the old `libc-0.6.6` packages over it: their LIBC066.DLL is a whole second C library, and programs mixing the two die with SIGABRT. Its `usr/lib/log/libcn0.dll` is the logging build (see Debugging). |
| `libcx-0_7_5-1_oc00.zip` | same | LIBCX0.DLL (mmap, select, shared file handles). |
| RPMs from rpm.netlabs.org/release/00/i386/i686 | `libgcc-9.2.0-5`, `libstdc++-9.2.0-5`, `libsupc++6-4.9.2.1-3` and the `libgcc-fwd`/`libstdc++-fwd` 9.2.0-5 packages | GCC 9 runtimes (GCC1.DLL, STDCPP6.DLL, GCC4xx.DLL forwarders); Firefox's GCC 4.9 build runs on them. Do not unpack `libgcc1`/`libstdc++6` 4.9 over them (same DLL names, too old for the Qt ports). |
| Firefox's `RPM_REQUIREMENTS` (same directory; `pentium4/` for the pentium4 ones) | `cairo-1.12.18-3`, `fontconfig-2.12.4-1`, `freetype-2.8.0-1`, `hunspell-1.6.2-1`, `libcx`, `libicu-56.1-2`, `libjpeg-turbo-1.5.1-1`, `libkai-1.2.0-1`, `libpng-1.6.31-1`, `libvpx-1.6.1-1`, `nspr-4.12.0-4`, `nss-3.23.0-4`, `nss-util-3.23.0-4`, `os2-base-0.0.0-18`, `os2-mpts-0.0.0-2`, `pango-1.28.4-2`, `pixman-0.32.8-3`, `pthread` (20190130-23 from pentium4/ instead of the listed 20171227-22), `zlib-1.2.11-2`, `bww-resources-rpm-1.1.2-1` | |
| What those need in turn | `expat-2.2.9-1`, `bzip2-libs-1.0.8-1`, `libglib-2_0-0`, `libgmodule-2_0-0`, `libgobject-2_0-0`, `libgthread-2_0-0` (all 2.33.12-3), `urpo-20160630-12`, `pcre-8.45-1`, `gettext-libs-0.19.8.1-3`, `libffi-3.0.11-1`, `libiconv-1.15-1`, `nss-softokn-3.23.0-4`, `nss-softokn-freebl-3.23.0-4` | Found by walking the import tables of Firefox's DLLs (the GLib DLLs are in the `lib*-2_0-0` packages, not `glib2`). |
| Qt 5 (pentium4/; a separate tree `work/dlqt` with `libicu-68.1-1` and `freetype-2.10.0-2` in place of Firefox's ICU 56 and FreeType 2.8, disk `work/qt.img`) | `qt5-qtbase-5.15.2-2`, `qt5-qtbase-gui-5.15.2-2`, `qt5-qtdeclarative`, `qt5-qtsvg`, `qt5-qtwebchannel`, `qt5-qtwebengine` (5.15.2-1), `qt5-designer`, `qt5-qttools` and its `-libs-*`, `dooble-2022.10.15-1`, and for QtWebEngine `ffmpeg-libs-4.2.2-4`, `cups-libs-2.2.13-1`, `libevent-2.1.11-4`, `libkai-2.1.0-1`, `lcms2-2.11-1`, `libvpx-1.8.2-1`, `minizip-1.2.11-2`, `opus-1.3.1-2`, `pcre2-10.37-1`, `pcre2-utf16-10.37-1`, `libssp-9.2.0-5`, `libwebp-1.1.0-1`, `libxml2-2.9.10-3`, `libxslt-1.1.34-2`, `gnutls-3.6.12-1`, `nettle-3.5.1-1`, `gmp-6.2.0-2`, `p11-kit-0.23.21-1`, `libtasn1-4.14-1`, `lame-libs-3.100-1`, `openjpeg2-2.3.0-2`, `libtheora-1.1.1-1`, `libvorbis-1.3.7-2`, `libogg-1.3.4-1`, `x264-libs-0.160-1`, `xvidcore-1.3.7-2`, `xz-libs-5.2.3-3` | |
| Command-line tests | `coreutils-8.31-1`, `coreutils-common-8.31-1`, `bash-5.0.11-3`, `ncurses-libs-5.9-1`, `readline-8.0-3`, `grep-3.4-1`, `pcre-legacy-8-8.12-2` (PCRE8.DLL for grep), `curl-7.75.0-2`, `libcurl-7.75.0-2`, `openssl-libs-1.1.1l-1`, `libidn2-2.3.0-1`, `libunistring-0.9.10-1`, `libpsl-0.21.1-1`, `unzip-6.0-9`, `gzip-1.8-1`, `tar-1.23-7`, `wget-1.20.3-2`, `sqlite-libs-3.28.0-1` | |

Everything is unpacked into one tree, whose `@unixroot` directory
becomes an HPFS test disk (long file names: Firefox's files do not fit
8.3 names). The disk is built with `cargo xtask disk --hpfs --size 600
-o work/ff.img` from a `work/disk.txt` that lists every file of the tree
(`SRC -> usr\lib\...`; debug info, docs, man pages, headers and
translations left out), plus the Liberation fonts from `dist/fonts` in
`usr\share\fonts\ttf` and empty `HOME`, `TMP` and `var\cache\fontconfig`
directories. It is attached with `--hdd work/ff.img` and becomes drive
E: (the ESP is D:). The CONFIG.SYS of the test (`work/cfg/config.sys`)
adds:

```
LIBPATH=.;C:\OS2\DLL;C:\OS2;C:\RS2;C:\;E:\USR\LIB;E:\USR\LIB\FIREFOX-45.9.0
SET PATH=E:\USR\BIN;...
SET UNIXROOT=E:
SET HOME=E:\HOME
SET TMP=E:\TMP
SET TEMP=E:\TMP
SET MOZILLA_HOME=E:\HOME
```

Firefox runs as the boot program:
`cargo xtask run --hdd work/ff.img --init "E:\USR\LIB\FIREFOX-45.9.0\FIREFOX.EXE -no-remote about:blank" -- -m 1536`
(1.5 GB of memory: its DLLs alone are 80 MB).

### Results

| Program | Status | Notes |
|---------|--------|-------|
| coreutils (`ls`, `cat`, `echo`, `wc`, `uname`, `head`, `tr`) | works | Also with `LANG=en_US` (kLIBC then builds its ctype tables through UCONV and LIBUNI). |
| `grep` 3.4 | works | Character classes (`[[:xdigit:]]`) in en_US. |
| `bash` 5.0 | works | `-c` scripts, loops, arithmetic, pipelines (`ls \| head`) and command substitution: kLIBC's fork (the child is the same program started again, which rebuilds the parent's memory, semaphores and sockets) and its spawn. Only "ttyname() not implemented" is printed: kLIBC's own message for a console. |
| `curl --version`, `unzip -v` | works | OpenSSL 1.1, libidn2, libpsl, zlib, bzip2 load. |
| Firefox 45.9 | works | Starts (about 8 minutes under QEMU's TCG on an Apple Silicon host, the first time; its profile is made in `E:\HOME`), shows its whole window (menus, tab bar, navigation and search bars, icons), loads an HTTP page from the host (`http://10.0.2.2:8088/`: CSS, a list, JavaScript changing the page) and an HTTPS site (`https://example.com`, DNS through QEMU, TLS with NSS, the padlock shown). Scripts the Liberation fonts lack (CJK, Arabic...) show as boxes. DIVE, the IM module (OS2IM) and the SNAP/Panorama pointer workaround are absent and Firefox goes on without them. Screenshots: work/progress/browser-os2. |
| Qt 5.15 Designer | fails | QtCore, QtGui and the OS/2 platform plugin (QOS2.DLL) load and PM starts; the program dies in QtCore on its second thread (a lock in a null private object), not yet looked into. |
| Dooble (Qt 5 WebEngine browser) | fails | The RPMs' DLLs do not fit together: LZMA5.DLL and QT5WEBC.DLL import pthread functions (`pthread_sigmask`, `pthread_condattr_*`) that no PTHR01.DLL in the public repository exports, and QtWebEngine needs ICU 68 and FreeType 2.10 where Firefox needs ICU 56 (separate disks: `work/dlqt`). QT5WEBC.DLL's 136 MB code object carries the OBJHIGHMEM flag, so rs2 would now load it above 512 MB; with VIRTUALADDRESSLIMIT=3072 loading still stops at LZMA5.DLL's missing import, before QT5WEBC.DLL is placed. |

### Firefox 45.9

What it took, in the order Firefox met it (each a separate commit; the
kernel ones are small and can be taken on their own):

1. **DOSCALLS' constants.** kLIBC imports DOSCALLS 214-217, 369 and 419 as
   values, not entry points (its `dos-data.imp`): DosMaxPathLen,
   DosPageSize, DosLocalInfo and DosGlobalInfo (tiled selectors of the
   information segments, turned into flat pointers), DosFlatCS and
   DOS32TIB (it loads FS from it). The loader now resolves them.
2. **Private semaphores opened by handle.** kLIBC's heap opens the
   private event semaphore it has just created again with
   DosOpenEventSem; OS/2 counts the open.
3. **Code selectors for 32-bit code tiles.** kLIBC's 16-bit call path
   switches to a 16:16 stack and RETFs into the 16:16 alias of an
   address in its 32-bit code object, which carries no 16:16 alias flag.
4. **Exceptions on a 16-bit stack** in 32-bit code (the same path) are
   dispatched on the stack's flat address.
5. **Per-process private semaphore handles**, the lowest free number
   first: kLIBC's fork recreates the parent's semaphores in the child
   by creating semaphores until it gets the same handles.
6. **DosSysCtl(DOSSYSCTL_AM_I_DEBUGGED)** answers FALSE without a debugger.
7. **Sockets in several processes' lists**: kLIBC's pipes are socket
   pairs, and its fork adds the parent's sockets to the child's list
   (`addsockettolist`) while the parent keeps them.
8. **DosExecPgm passes all the argument strings** up to the double NUL,
   byte for byte: kLIBC passes "\x7fkLIBC\x7f" and one flag-prefixed
   string per argument.
9. **UCONV and LIBUNI for kLIBC's locales**: the substitution count of
   UniUconvToUcs is an output (kLIBC leaves it uninitialised and saw
   every character fail), UniTransUpper/Lower, transformation and
   attribute objects, UniQueryAttr, UniStrtod, UniStrxfrm (and
   ordinal 145 of later LIBUNIs, which GLib imports).
10. **The loader**: GCC's runtime DLLs export a value from a bundle with
    object 0 (the kernel panicked); ICU's 13 MB data DLL is linked at
    0x10000 with the "internal fixups removed" flag and no fixups, and
    may load anywhere.
11. **PM**: window properties (WinSetProperty and friends, PMMERGE
    5450-5452), PMMERGE's Gre32Entry forwarders, WinQueryVisibleRegion,
    GpiEnableYInversion/GpiQueryYInversion with mirrored blits to a
    turned-over target (cairo draws its top-down images that way), and
    PM set up on the first call that needs it (XUL opens an information
    DC before WinInitialize), WM_ADJUSTWINDOWPOS and WM_WINDOWPOSCHANGED
    from WinSetWindowPos and the frame's layout (Mozilla's windows
    follow their size by WM_WINDOWPOSCHANGED: without it the window was
    off the screen and the content window stayed 1x1),
    WinQueryDesktopWorkArea, WinQueryKbdLayout (Qt).
12. **Each thread its own local information segment.** kLIBC owns its
    mutexes by the LINFOSEG's tidCurrent; with two CPUs one shared page
    held only one thread's ID ("LIBC Heap release not owner").
    DosLocalInfo now names a page that is each thread's own, mapped
    through the per-CPU page tables of thread-local memory.

### Not done

- **High memory** (done since; ARCHITECTURE.md "Memory layout"): kLIBC
  takes its high heap with OBJ_ANY (above 512 MB at the default
  VIRTUALADDRESSLIMIT of 1024: `apitrace` shows its 16 MB heap at
  0x20030000), and its fork rebuilds it at the same addresses in the
  child. bash, coreutils, grep and curl work at 1024 and 3072, and
  Firefox 45.9 starts with VIRTUALADDRESSLIMIT=3072 (`--mem 3G`). The
  `firefox-highmem` package (marking Firefox's DLLs to load high) was
  not tried.
- Firefox's sound (KAI: DART/UNIAUD), DIVE and printing were not tested.

### Debugging kLIBC programs

- `SET LIBC_LOGGING=-all+LOCALE` (groups: PROCESS, HEAP, LOCALE, FORK,
  BACK_FS, SOCKET ... or `+all`) with `SET LIBC_LOGGING_OUTPUT=stderr`
  and `SET BEGINLIBPATH=E:\USR\LIB\LOG` (the logging LIBCN0.DLL of the
  libc zip) puts kLIBC's own trace on the serial log.
- `apitrace=PROG` on the kernel command line (work/cmdline.txt) traces
  the DOSCALLS of one program; `apitrace=CMD,PROG` also shows where the
  loader put each DLL (load happens in the parent).
- `SET PMTRACE=1` traces PM calls.
- kLIBC prints its panics ("LIBC PANIC!!", "Killed by SIGxxx") with the
  module and offset of the fault: `XUL 0:020E41B7` is object 0 offset
  0x20E41B7.
