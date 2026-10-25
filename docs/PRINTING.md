# Printing

How OS/2 Warp 4.52 prints, and how rs2 does it. rs2 ships its own
spooler (PMSPL.DLL and PMSPOOL.EXE), queue processor (PMPRINT.QPR), a
PostScript presentation driver inside its GRE (PMWIN.DLL with
crates/gpi), port handlers for files, the parallel port and network
printers (LPR and IPP), and the Workplace Shell's printer objects. No
IBM file is shipped; IBM's PMSPL, PMPRINT, PMSPOOL and PSCRIPT.DRV were
studied in Ghidra with their `.sym` files (`debug/symbols/os2/dll/pmspl.sym`,
`os2/drivers/pmprint.sym`, `os2/pmspool.sym`, `os2/drivers/pscript/`).

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
PM_Q_RAW job on the queue that prints to LPT1.

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
  PostScript level 2 document.
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
- **Workplace Shell**: WPPrinter objects are queues (printer objects
  created in the Printers folder make a queue and a printer); their
  job view lists the queue's jobs (SplEnumJob) with hold, release,
  delete and print next; their settings pages set the queue, the
  printer driver (job properties) and the port (and the port's
  settings); dropping a file prints it (as text or PostScript), WPDataFile's
  Print does the same, and the Printers folder's template is *Install
  New Printer*.

### First targets

1. The spooler and PMSPL (queues, printers, ports, jobs, the Spl*
   calls programs and the shell use), with a FILE printer installed by
   default.
2. The PostScript device: GPI on a printer DC gives correct PostScript;
   PRNTEST checks the output.
3. LPR (a host listener in the tests), then IPP (CUPS) and LPT.
4. The Workplace Shell's printer objects.
5. Programs: rs2's E, IBM's E, EPM, Picture Viewer, the help manager.

Not planned yet: DOS and Win-OS/2 printing through LPT1 into the
spooler, remote (LAN Server) queues, separator pages, bidirectional
printers, and other printer languages (PCL).

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
| Print in programs | rs2's E (File > Print), the help manager (Services > Print), IBM's Picture Viewer | QEMU runs, output rendered with Ghostscript |

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
programs were printed to the host listener).

Not done yet:
- DOS, Win-OS/2 and other programs that write to `LPT1` themselves go
  straight to the parallel port; OS/2's kernel hands such output to
  the spooler as raw jobs (EPM's Raw ASCII printing works that way).
- EPM's formatted printing starts but never opens its printer DC (its
  print loop waits; COMPATIBILITY.md).
- Separator pages, job retention, forms per queue, remote (LAN Server)
  queues, IBM's port driver (.PDR) and queue processor interfaces for
  other vendors' modules, and IBM's spool files' (.SPL) device
  escape order (rs2 records DEVESC_NEWFRAME as its own escape order
  0x80 and blits as 0x81-0x83).
- PostScript size: images are run-length and ASCII85 encoded (no
  Flate); glyphs are Type 3 procedures without ToUnicode, so PDFs made
  from the output have no searchable text.
- Text on printers is laid out with the fonts' pel advances at 300 dpi
  (as on the screen), and outline faces are made at each size used
  (Source::at keeps 32 sizes); larger sizes are scaled.
