# Printing

How OS/2 Warp 4.52 prints, and how rs2 does it. rs2 ships its own
spooler (PMSPL.DLL and PMSPOOL.EXE), queue processor (PMPRINT.QPR), a
PostScript presentation driver inside its GRE (PMWIN.DLL with
crates/gpi), port handlers for files, the parallel port and network
printers (LPR and IPP), and the Workplace Shell's printer objects. No
IBM file is shipped; IBM's PMSPL, PMPRINT, PMSPOOL and PSCRIPT.DRV were
studied in Ghidra with their `.sym` files (`debug/symbols/os2/dll/pmspl.sym`,
`os2/drivers/pmprint.sym`, `os2/pmspool.sym`, `os2/drivers/pscript/`),
and for the Print Screen key PMMERGE's winprint.c (`os2/dll/pmmerge.sym`);
WPPRTMRI.DLL's dialogs give the printer objects' pages.

## How OS/2 prints

```
 application                                  PMSPOOL.EXE (spooler process)
 ───────────                                  ──────────────────────────────
 DevOpenDC(OD_QUEUED, "*", DEVOPENSTRUC)      SPL32QMINITIALIZE (in PMSPL)
   pszLogAddress = queue name                   spooler thread: picks the next
   pszDataType   = PM_Q_STD | PM_Q_RAW          ready job of each printer
 GpiCreatePS / GpiAssociate                          │
 DevEscape(DEVESC_STARTDOC, "doc name")              ▼
 Gpi* drawing, DEVESC_NEWFRAME per page      queue processor (PMPRINT.QPR)
 DevEscape(DEVESC_ENDDOC) -> job id             SplQpOpen / SplQpPrint / SplQpClose
 DevCloseDC                                       PM_Q_STD: GpiLoadMetaFile,
      │                                           DevOpenDC(OD_DIRECT, printer),
      │ GRE: PM_Q_STD = metafile recording        STARTDOC, GpiPlayMetaFile, ENDDOC
      │      PM_Q_RAW = the driver renders        PM_Q_RAW: DevEscape(DEVESC_RAWDATA)
      ▼                                               │ chunks of the file
 SplQmOpen / SplQmStartDoc /                          ▼
 SplQmWrite / SplQmEndDoc / SplQmClose        presentation driver (PSCRIPT.DRV...)
      │                                         OD_DIRECT: renders, PrtOpen/PrtWrite
      ▼                                               │
 \SPOOL\<queue>\00001.SPL  (data)                     ▼
 \SPOOL\<queue>\00001.SHD  (shadow: job info)  port driver (PARALLEL.PDR, LPRPDRVR.PDR...)
                                                 SplPdOpen / SplPdWrite / SplPdClose
```

### The spooler

IBM's spooler is PMSPL.DLL itself (about 270 KB of 32-bit code plus the
16-bit DosPrint*/Prt* thunks): every process that calls the Spl* API
loads it, and the spooler *process*, PMSPOOL.EXE, is a 2 KB program
that calls `SPL32QMINITIALIZE` and keeps the spooler thread
(`SpoolerThread`, `SpoolSchedule`) running. State is shared between
processes through shared memory guarded by a semaphore
(`EnterSplSem`/`LeaveSplSem`); the configuration lives in OS2SYS.INI:

| Application | Key | Value |
|-------------|-----|-------|
| `PM_SPOOLER` | `DIR`, `QUEUE`, `PRINTER`, `SPOOL` | spool directory, default queue and printer (`"LPT1Q;"`), spooling on |
| `PM_SPOOLER_QUEUE` | queue name | queue processor, separator file, queue processor parameters |
| `PM_SPOOLER_QUEUE_DESCR` | queue name | the description (the printer object's title) |
| `PM_SPOOLER_QUEUE_DD` | queue name | `DRIVER.DEVICE` the queue prints with |
| `PM_SPOOLER_QUEUE_DDDATA` | queue name | the job properties (a DRIVDATA, binary) |
| `PM_SPOOLER_QUEUE_STATUS`, `_TYPE` | queue name | priority and times; `APPLDEFAULT`, held... |
| `PM_SPOOLER_PRINTER` | printer (device) name | `PORT;DRIVER.DEVICE,...;QUEUE,...;;` |
| `PM_SPOOLER_PRINTER_DESCR` | printer name | the description |
| `PM_SPOOLER_PORT` | port name | port settings (COM ports: `9600;0;8;1;1;`) |
| `PM_SPOOLER_QP` | queue processor | `C:\OS2\DLL\PMPRINT.QPR;;` |
| `PM_SPOOLER_DD` | driver | `PSCRIPT.DRV;;;` |
| `PM_PORT_DRIVER` | port driver | the `.PDR` that owns ports |

A *queue* is what applications print to (a printer object in the
Workplace Shell is a queue); a *printer* (device) is a port plus the
drivers and the queues that feed it. A queue sends its jobs to its
printers; a printer prints one job at a time.

Jobs are files in `\SPOOL\<queue>\`: `nnnnn.SPL` holds the data, and
`nnnnn.SHD` (the shadow) the job's description (document name,
submitter, data type, the DEVOPENSTRUC strings and DRIVDATA, status,
position), so jobs survive a reboot (`CreateJobFromShadow`).

### Queue drivers (queue processors)

PMPRINT.QPR exports `SplQpOpen`, `SplQpPrint`, `SplQpClose`,
`SplQpControl`, `SplQpQueryDt` (the data types it handles: `PM_Q_STD`
and `PM_Q_RAW`), `SplQpQueryFlags` and `SplQpPrintSeparator`.
`SplQpOpen` gets the job's DEVOPENSTRUC (with the *printer* name as
the logical address) and `SplQpPrint` the spool file's name:

- **PM_Q_STD**: the spool file is a metafile. `GpiLoadMetaFile`, a PS on
  an OD_INFO DC to read its page setup (a first `GpiPlayMetaFile`),
  then `DevOpenDC(OD_DIRECT, "*", printer DEVOPENSTRUC)`,
  `DEVESC_STARTDOC` with the document name, `GpiPlayMetaFile` on the
  output DC (the metafile's NEWFRAME escapes become pages),
  `DEVESC_ENDDOC`. The queue processor parameters (`XFM=`, `ARE=`,
  `FIT=`, `COP=`...) scale and position the picture (`SetViewMatrix`).
- **PM_Q_RAW**: the file is already in the printer's language: an
  OD_DIRECT DC opened with `DATATYPE=PM_Q_RAW`, then the file in 4 KB
  chunks of `DEVESC_RAWDATA`, then `DEVESC_ENDDOC`.

### Presentation drivers

A presentation driver (`.DRV`, a DLL) is the device part of the GRE:
it gets the GPI's drawing as device-level calls (the dispatch table
handed over at `OS2_PM_DRV_ENABLE`), answers `DevQueryCaps` and
`DevQueryHardcopyCaps` (forms), shows the job properties dialog
(`OS2_PM_DRV_DEVMODE`, behind `DevPostDeviceModes`) and lists its
devices (`OS2_PM_DRV_DEVICENAMES`, behind `DevQueryDeviceNames`).
PSCRIPT.DRV turns the drawing into PostScript; on OD_DIRECT DCs it
writes through `PrtOpen`/`PrtWrite`, which the spooler connects to the
printer's port.

### The device context path

1. `DevOpenDC(hab, OD_QUEUED, "*", n, &dop, 0)`: the DEVOPENSTRUC names
   the queue (`pszLogAddress`), the driver (`pszDriverName`, e.g.
   `PSCRIPT`), the job properties (`pdriv`, a DRIVDATA whose
   `szDeviceName` picks the device) and the data type. With the token
   `"*"` the GRE fills what is missing from the queue's INI entries.
2. `GpiCreatePS(..., GPIA_ASSOC)` or `GpiAssociate`: the page is the
   device's form in device pels (or the units asked for).
3. `DevEscape(DEVESC_STARTDOC, name)`: the GRE opens the job
   (`SplQmOpen` with the DEVOPENSTRUC, `SplQmStartDoc` with the name).
4. Drawing: for PM_Q_STD the GPI records it as a metafile; for
   PM_Q_RAW the driver renders and the bytes go to `SplQmWrite`.
   `DEVESC_NEWFRAME` ends a page.
5. `DevEscape(DEVESC_ENDDOC)` returns the job id (`SplQmEndDoc`);
   PM_Q_STD's metafile is written with `SplQmWrite` first.
   `DEVESC_ABORTDOC` drops the job (`SplQmAbort`).
6. `DevCloseDC` (`SplQmClose`). The spooler now has a ready job.

Programs that are not PM programs print with `DosOpen("LPT1")`; the
kernel's printer device driver hands the data to the spooler through
a pipe (`\PIPE\SPLRALRT`, `KernelPipeThread`), which makes it a
PM_Q_RAW job on the queue that prints to LPT1. In IBM's PMSPL the
kernel's requests are records: open (SplFSOpen: the port's printer's
queue, a job and its spool file name back), first write, close
(SplFSClose: the job is ready), set title and others; the kernel then
writes the data to the spool file itself. DOS sessions' printer output
(VLPT.SYS: INT 17h, the ports, PRN) takes the same way; a DOS program's
end closes its job (`PRINT_SEPARATE_OUTPUT`), and so does
`PRINT_TIMEOUT` seconds without output (DOS settings).

A queue's separator file (PRQINFO3 `pszSepFile`) is printed before each
of its jobs (`PrintSep`, `DoPrintSep`): LAN Manager's format, whose
first character is the escape character (OS/2 ships `SAMPLE.SEP`, text,
and `PSCRIPT.SEP`, a PostScript page). Queue processors that print
separators themselves say so with `QP_PRINT_SEPARATOR_PAGE`
(SplQpQueryFlags) and get the separator's bytes with
`SplQpPrintSeparator`; else the spooler sends them as DEVESC_RAWDATA.
A queue prints only between its start and stop times (minutes after
midnight; equal times: always), and a printer serves its queues by
their priority (1 highest, 9 lowest).

The Print Screen key (PMMERGE's winprint.c): `CheckPrintScreen` in
WinPeekMsg turns a VK_PRINTSCRN key-down without Ctrl or Alt, while
SV_PRINTSCREEN is on, into message 0x20F for the desktop window, whose
`WinPrintScreen` starts a print thread (one at a time; a beep
otherwise). The thread prints the top-level window under the mouse
pointer (`SET PM_PRINTSCREEN_ACTIVE_WINDOW=ON`, or no mouse: the active
window; the desktop: the whole screen): it copies the window's pels into
a bitmap and blits it on a PM_Q_STD queued DC of the default queue
(`PM_SPOOLER` `QUEUE`, `XFM=0`), as large as a whole multiple of its
size that fits inside a margin of 1/39 of the page, centred.

## What rs2 can reach

| Output | QEMU | VMware | How |
|--------|------|--------|-----|
| A file on rs2's disk | yes | yes | the FILE port: the job's bytes go to a file (`C:\SPOOL\OUTPUT\*.PS` unless the port names one) |
| The parallel port | `-parallel file:out.prn` (or a host pipe/socket) | `parallel0.fileName` (a file or a host printer on Windows hosts) | LPT1-LPT3: the kernel's LPT driver writes bytes to the port's data register with the strobe |
| A network printer, LPR | user networking reaches the host at 10.0.2.2 (port forwarding not needed for outgoing connections) | NAT or bridged | RFC 1179 to port 515 (or any port), over rs2's TCP/IP |
| A network printer, IPP | the same; macOS/Linux hosts run CUPS on port 631, which prints to the host's printers | the same | an HTTP POST of an IPP Print-Job with the PostScript |
| A PDF | through the host (CUPS's PDF printer, or ps2pdf on the FILE output) | the same | |

PostScript is the right first language: GPI maps onto it closely
(paths, fills, clipping, images, fonts as outlines), every printer
path above accepts it (CUPS and most network printers take it
directly; Ghostscript turns a file into PDF), and it is text, so tests
can compare it.

## rs2's design

```
 app ── DevOpenDC(OD_QUEUED) ──► PMWIN (GRE)              PMSPOOL.EXE (rs2)
            PM_Q_STD: metafile DC ─┐                        │ SPL32QMINITIALIZE
            PM_Q_RAW: PostScript ──┤                        ▼
                                   ▼                     PMSPL spooler thread
                           PMSPL SplQm* ─► C:\SPOOL\Q\    (event semaphore wakes it)
                           (job table in named            │
                            shared memory)                ▼
                                                    PMPRINT.QPR (rs2): SplQpPrint
                                                      DevOpenDC(OD_DIRECT, printer)
                                                      GpiPlayMetaFile / RAWDATA
                                                          │
                                         PMWIN PostScript device (crates/gpi `psdev`)
                                                          │ PrtOpen/PrtWrite (PMSPL)
                                                          ▼
                                      port: FILE · LPT1-3 · LPRn (RFC 1179) · IPPn
```

- **PMSPL.DLL** (`userland/pmspl`) is the spooler, as IBM's is. The
  configuration is OS2SYS.INI's `PM_SPOOLER*` applications in IBM's
  layout; the live state (queues' status, the job table, the next job
  id) is a named shared memory block (`\SHAREMEM\PMSPL`) under a named
  mutex; each job is `C:\SPOOL\<queue>\nnnnn.SPL` plus a shadow
  `nnnnn.SHD` (rs2's own format: a header and the strings), read back
  when the spooler starts. Spl* calls work in any process; the queue
  manager calls (SplQmOpen...) write the spool file in the caller's
  process and post the spooler's event semaphore.
- **PMSPOOL.EXE** (`userland/pmspool`, rs2's own) calls
  `SPL32QMINITIALIZE`, which runs the spooler loop on its thread: for
  each printer that is idle and not held, the first ready job of its
  queues (by priority, then submission), through the queue's queue
  processor. PMSPL starts PMSPOOL itself (detached) the first time a
  job is queued while no spooler is running, and the shell starts it
  at logon as OS/2 does.
- **PMPRINT.QPR** (`userland/pmprint`, rs2's own) does what IBM's does
  (above): PM_Q_STD by playing the metafile into an OD_DIRECT DC
  between STARTDOC and ENDDOC, PM_Q_RAW by DEVESC_RAWDATA.
- **The PostScript device** is in PMWIN's GRE: DevOpenDC with the
  driver `PSCRIPT` (OD_DIRECT, OD_QUEUED with PM_Q_RAW, and OD_INFO
  for queries) gives a printer DC whose presentation spaces draw
  through `gpi::psdev`, a vector back end of crates/gpi's `Ps`: lines,
  arcs, splines, areas and paths become PostScript paths in device
  space (stroke/fill/eofill, line types as dash patterns, wide lines),
  clipping becomes clip paths, text becomes Type 3 fonts built from
  rs2's outline fonts (glyph outlines) or image fonts (imagemasks)
  shown with `xshow` at exactly the advances the GPI computes, and
  bitmaps (GpiBitBlt, GpiWCBitBlt, WinDrawBitmap, GpiDrawBits) become
  `image` operators. Raster operations that need the destination (XOR
  and the other mixes) are approximated by overpaint, as printers do.
  Each page is a DSC page (`%%Page:`); the job a DSC-conforming
  PostScript document (level 2, with level 3's FlateDecode for images
  and ImageType 4 for masked ones). Images are compressed with rs2's
  deflate (`gpi::deflate`: LZ77 with hash chains, fixed Huffman codes);
  a full-screen Print Screen page is about 56 KB.
  The device is "PSCRIPT.Generic PostScript Printer" (300 dpi) with
  the forms Letter, A4 and Legal, portrait or landscape, and copies
  (the DRIVDATA rs2 keeps, `DevPostDeviceModes`'s job properties
  dialog, DevQueryHardcopyCaps).
- **Ports** are handled by PMSPL (`PrtOpen`/`PrtWrite` on the port of
  the printer):
  - `FILE`: to the file the port's settings name, or
    `C:\SPOOL\OUTPUT\<job>.PS`.
  - `LPT1`-`LPT3`: `DosOpen` of the kernel's LPTn device, which drives
    the parallel port (data, strobe, busy/ack).
  - `LPR1`...: settings `host;queue;port;` (port 515 by default); the
    job is sent with RFC 1179's *receive job* (control file, data file).
  - `IPP1`...: settings `host;path;port;` (631 and `/ipp/print` by
    default); an HTTP/1.1 POST of `application/ipp` with Print-Job and
    the document as `application/postscript`.
- **Workplace Shell** (WPS.md, `printer.rs` and `printobj.rs`):
  WPPrinter objects are queues; their job view shows the queue's jobs
  as job objects (WPJob: hold, release, start again, print next,
  delete; their settings); IBM's Printer driver, Output port and Queue
  options pages show printer driver, port and queue driver objects
  (WPPdr, WPPort, WPQdr; a port opens on its settings); the Spooler
  object (WPSpool) sets the spool path; dropping a file prints it (as
  text or PostScript), WPDataFile's Print does the same, and the
  Printers folder's template opens IBM's *Create a printer* (name,
  driver, port), which makes a queue and a printer. IBM's View page
  (physical name, default view Icon or Detail, the details shown,
  refresh interval) and Print options page (separator file with *Find
  file...*, start and stop times) are there too; the job view's details
  are document, status, job id, size and the time submitted; a job
  dragged onto another printer moves there (`wpMoveJobObject`:
  SplCopyJob, then the job leaves its queue).
- **Programs that print to a port** (`DosOpen("LPT1")`, `PRN`, DOS
  sessions, Win-OS/2): while the spooler runs, its thread serves the
  port pipe `\PIPE\SPOOLER\PORT` (PMSPL `lptpipe.rs`, four connections).
  The kernel (`chardev::lpt_spool_open`) connects each open of LPTn/PRN
  to it and sends `LPTn\0program\0pid\0` (a DOS session sends its
  title); the spooler answers 0 when a printer prints to that port (and
  spooling is on), and the handle (`Handle::Spool`) then carries what
  the program writes: from the first byte a PM_Q_RAW job on that
  printer's first queue, ready when the last handle closes. Otherwise,
  or when no spooler runs, the program writes to the port itself; the
  spooler's own process (it serves the pipe) always does, so jobs reach
  the parallel port. Printer status IOCtls on a spooled port answer
  "selected, not busy". DOS sessions (docs/DOS.md) open `LPT1`/`LPT2`
  for INT 17h, the 378h/278h registers, PRN and INT 21h 05h, and close
  the job at each program's end (`PRINT_SEPARATE_OUTPUT`) or after
  `PRINT_TIMEOUT` seconds without output; Win-OS/2's printer drivers
  write to `LPT1:` (or `LPT1.OS2`) through DOS, so a Windows program's
  print is a job too. VioPrtSc spools the screen's text the same way.
- **Separator pages, print times, priorities** (PMSPL `sep.rs`,
  `spooler.rs`): the queue's separator file in LAN Manager's format
  (escapes `N` owner, `I` job id, `J` document, `Q` queue, `D` date,
  `T` time, `L` text up to the next escape, `F` a file's contents, `H`nn
  a byte, `W`nn page width, `0`-`9` new lines, `E` eject; `B`, `S`, `M`
  block characters print as normal text) goes to PMPRINT's
  SplQpPrintSeparator, which sends it as raw data right after
  DEVESC_STARTDOC. rs2 ships its own `C:\OS2\SAMPLE.SEP` (text) and
  `C:\OS2\PSCRIPT.SEP` (a PostScript banner page). A queue outside its
  start/stop times keeps its jobs; a printer takes its queues by
  priority.
- **The Print Screen key** (PMWIN `prtscr.rs`, as IBM's above): the
  input thread asks the PM server's desktop window, whose print thread
  copies the window under the pointer (or the screen) from the screen
  into a bitmap and prints it to the default printer.

### First targets

1. The spooler and PMSPL (queues, printers, ports, jobs, the Spl*
   calls programs and the shell use), with a FILE printer installed by
   default.
2. The PostScript device: GPI on a printer DC gives correct PostScript;
   PRNTEST checks the output.
3. LPR (a host listener in the tests), then IPP (CUPS) and LPT.
4. The Workplace Shell's printer objects.
5. Programs: rs2's E, IBM's E, EPM, Picture Viewer, the help manager.

6. Programs that print to a port, DOS sessions and Win-OS/2 through the
   spooler; separator pages, print times and priorities; the Print
   Screen key.

Not planned yet: remote (LAN Server) queues, bidirectional printers,
and other printer languages (PCL).

## Status

**Printing works.** A program prints a page through the spooler to a
PostScript file, to a parallel port, and to network printers (LPR and
IPP), and the Workplace Shell's printer objects work:

| What | Where | Checked |
|------|-------|---------|
| The spooler: queues, printers, ports, jobs (`\SPOOL\queue\nnnnn.SPL`/`.SHD`); SplEnum/Query/Set/Create/Delete for queues, jobs, devices and ports, SplEnumPrinter, SplEnumDriver, SplEnumQueueProcessor, hold/release/purge/copy, SplControlDevice; the queue manager (SplQmOpen...SplQmClose, SplQmGetJobID, SplQmNewPage); Prt* | `userland/pmspl` | PRNTEST |
| The spooler thread (SPL32QMINITIALIZE), started on demand | `userland/pmspool`, PMSPL `spooler.rs` | PRNTEST: jobs printed and removed, held jobs wait, deleted jobs never print |
| The queue processor: PM_Q_STD (the metafile played on an OD_DIRECT DC, page setup from the metafile) and PM_Q_RAW (DEVESC_RAWDATA) | `userland/pmprint` | PRNTEST |
| The PostScript driver: DevOpenDC OD_QUEUED/OD_DIRECT/OD_INFO, STARTDOC/NEWFRAME/ENDDOC/ABORTDOC/RAWDATA/GETJOBID/job properties escapes, DevQueryCaps (300 dpi, CAPS_TECH_POSTSCRIPT), DevQueryHardcopyCaps (Letter, A4, Legal, A3, A5, Executive), DevQueryDeviceNames, DevPostDeviceModes (a job properties dialog: form, orientation, copies) | PMWIN `printer.rs`, `crates/gpi/src/psdev.rs` | PRNTEST, host unit tests (`cargo test -p gpi`), Ghostscript renders the output as the screen shows it |
| Ports: FILE (FILEn), LPT1-3 (the kernel drives the parallel port), COM1-4, LPRn (RFC 1179), IPPn (IPP/1.1 Print-Job) | PMSPL `port.rs`, kernel `chardev.rs` | PRNTEST LPR/IPP with `tools/scripts/printer_listen.py`; LPT1 with QEMU's `-parallel file:` |
| Printer objects: job view, settings (queue, driver, job properties, port and settings, default printer), new printers from the template, deleting, files dropped and data files' Print | PMWP `printer.rs` | QEMU desktop runs (screenshots in the session's progress folder) |
| Print in programs | rs2's E (File > Print), the help manager (Services > Print), IBM's Picture Viewer, the Toolkit's print sample (PRTSAMP: a metafile, sized to the page), EPM's formatted printing (File > Print file: its print thread draws each page on a `PU_TWIPS` presentation space of a queued DC) | QEMU runs, output rendered with Ghostscript |
| Windows 3.x programs (native Win16): printer DCs, StartDoc...EndDoc and the escapes, PrintDlg, WIN.INI's printers (docs/WIN16.md) | WIN16 `print.rs`: queued PM_Q_RAW DCs | W16TEST, `RS2\PRINT16.EXE` (output rendered with Ghostscript), NOTE16's File > Print |
| Programs that print to a port: `DosOpen("LPTn")`/`PRN` while the spooler runs is a PM_Q_RAW job on the port's printer's queue (nothing for an open without output); the spooler's own opens and ports without a printer go to the port; PRT_GETPRINTERSTATUS on a spooled port | kernel `chardev.rs` (`lpt_spool_open`, `Handle::Spool`), PMSPL `lptpipe.rs` | PRNTEST: LPT3 from an OS/2 program (job with the bytes and the program's name, printed unchanged) |
| DOS sessions print: INT 17h, the 378h/278h registers, PRN/LPTn handles, INT 21h 05h; a job per DOS program, PRINT_TIMEOUT, PRINT_SEPARATE_OUTPUT | `crates/dos` (`Host::print`), VDM.EXE | PRNTEST: a DOS session's PRINT.COM (tests/dos/print.S) gives one 63-byte job named after the session; `tests/dos/run.sh` (dosrun --lpt) |
| Win-OS/2 programs print through Windows' printer drivers to `LPT1:` (or `LPT1.OS2`) into the spooler | the DOS machine's printer path | dosrun: Notepad's File > Print with the CD's TTY.DRV as the default printer, with and without Print Manager (`tools/scripts/winos2_tree.py --printer`); not run in QEMU |
| Separator pages (LAN Manager's .SEP format; rs2's SAMPLE.SEP and PSCRIPT.SEP), print times, queue priorities | PMSPL `sep.rs`, `spooler.rs`; PMPRINT SplQpPrintSeparator | PRNTEST: owner, job id, document, queue, width, hex, new lines and eject before the job's data; a queue outside its times keeps its job, which prints when the times allow |
| Printer objects' View page (physical name, Icon/Detail, details shown, refresh interval; the job details view) and Print options page (separator file, Find file..., start and stop times); jobs dragged to another printer move | PMWP `printobj.rs`, `printer.rs`, `view.rs` | QEMU screenshots (pages, the details view of held jobs); the job move was not tried by dragging |
| The Print Screen key: the window under the pointer, or the whole screen, to the default printer | PMWIN `prtscr.rs`, `input.rs`, `server.rs` | QEMU: PrtSc over the desktop and over System Setup to a host LPD listener, rendered with Ghostscript as the screen shows them |
| Images in the PostScript Flate compressed | `crates/gpi/src/deflate.rs`, `psdev.rs` | `cargo test -p gpi` (round trip), Ghostscript renders the Print Screen pages unchanged (2.2 MB became 56 KB) |

**Tests.** `RS2\PRNTEST.EXE` (userland/prntest) makes a queue on a
printer on a file port and checks the spooler calls, a raw job (held,
changed, released, printed unchanged), two pages through a PM_Q_STD
queued DC, a PM_Q_RAW queued DC and an OD_DIRECT DC (each output is
checked as PostScript: DSC structure and page count, text with
`xshow`, outline fonts' Type 3 glyphs, a patterned area, a dashed line,
a box, an image), raw data on a raw OD_DIRECT DC, the job properties
and device names, and A4 landscape. `PRNTEST LPR host port queue` and
`PRNTEST IPP host port path` print to a network printer as well; with
`python3 -I tools/scripts/printer_listen.py lpd|ipp PORT DIR` on the
host and `cargo xtask run --net --init "RS2\PRNTEST.EXE LPR 10.0.2.2
PORT q"` the listener saves what arrives (QEMU's user network reaches
the host as 10.0.2.2). `PRNTEST SETPORT printer port settings RUN
program args` points a printer at a port and runs a program (how IBM
programs were printed to the host listener). A PM_Q_STD queued DC keeps
what was recorded before DEVESC_STARTDOC (colour tables, fonts: PRTSAMP
plays its metafile once to size it first). PRNTEST also prints to its
printer's port from an OS/2 program and from a DOS session (the
printer is pointed at LPT3 and LPT1 while its queue is held, the jobs
are checked, then printed on the file port), and checks separator
files and print times. The Print Screen key was checked with `cargo
xtask run --net --init "RS2\PRNTEST.EXE SETPORT PRINTER1 LPR1
10.0.2.2;q;PORT; RUN RS2\WPSTEST.EXE SHELL OPEN <WP_CONFIG> 1"`,
`{mouse_abs x y}{print}` typed and the LPD listener on the host.

Not done yet:
- The PrtSc key in a full-screen text session goes to LPT1 directly
  (the keyboard path cannot wait for the spooler); VioPrtSc spools.
  VioPrtScToggle's echo is not spooled either.
- Separator files' block characters (`B`, `S`, `M`) print as normal
  text; the Print options page has no Input bin (the PostScript driver
  has one tray) and no "apply only to jobs that exceed size limits".
  The Find file dialog starts in the root, not `\OS2`.
- The PostScript header says LanguageLevel 2 while images use level 3's
  FlateDecode (CUPS, Ghostscript and current printers take it).
- Win-OS/2 printing was run in dosrun, not yet in a QEMU Win-OS/2
  session (the same DOS machine and VDM path that PRNTEST's DOS check
  covers).
- The Toolkit's PRTSAMP.MET prints as rs2 draws it on the screen, a
  black silhouette: its colours are indices 0x800-0xE00 of its colour
  table, which rs2's metafile playing does not apply (a GPI matter; the
  PostScript follows whatever the GPI draws). `PRNTEST MET file` plays
  a metafile on a direct and a queued printer DC and lists the colours
  the PostScript sets. PRTSAMP's main window does not show (it prints
  from the keyboard); not looked at.
- Job retention, forms per queue, remote (LAN Server)
  queues, IBM's port driver (.PDR) and queue processor interfaces for
  other vendors' modules, and IBM's spool files' (.SPL) device
  escape order (rs2 records DEVESC_NEWFRAME as its own escape order
  0x80 and blits as 0x81-0x83).
- Glyphs are Type 3 procedures without ToUnicode, so PDFs made from the
  output have no searchable text.
- WinFillRect on a PM_Q_STD queued DC is not recorded in the metafile
  (the fill is missing from the printout; found by Win16 printing, which
  uses PM_Q_RAW DCs for that and for fidelity).
- Text on printers is laid out with the fonts' pel advances at 300 dpi
  (as on the screen), and outline faces are made at each size used
  (Source::at keeps 32 sizes); larger sizes are scaled.
