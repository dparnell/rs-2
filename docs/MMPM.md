# Multimedia (MMPM/2) in rs2

OS/2's games (Solitaire, Mahjongg, Chess) use MMPM/2 for their sounds
(.WAV files) and opening animations (.FLC files). They load MMIO.DLL and
MDM.DLL with DosLoadModule and call them by name: from MMIO `mmioOpen`,
`mmioRead`, `mmioClose`, `mmioGetHeader` and `mmioSet`; from MDM
`mciSendCommand`, `mciSendString` and `mciGetErrorString`. When the DLLs
are missing they say that sounds need MMPM/2.

rs2 provides its own MMIO.DLL and MDM.DLL, written in Rust like the PM
DLLs, with IBM's module names and ordinals. The file formats are decoded
in a host-tested crate:

| Crate / DLL | Contents |
|-------------|----------|
| `crates/mmfmt` | RIFF chunks, WAVE audio, Autodesk FLI/FLC animations. `no_std` + `alloc`, no OS dependencies; tested on the host against OS/2's own files. |
| `userland/mmio` → MMIO.DLL | The multimedia I/O manager: buffered and memory files, RIFF chunk calls, the DOS, MEM and WAVE I/O procedures. |
| `userland/mmiotest` → RS2\MMIOTEST.EXE | Self-checks of MMIO.DLL. |
| `crates/mmimage` | Still images: OS/2 1.x and 2.0 bitmaps and Windows DIBs (RLE4/8/24, bit fields, bitmap arrays), JPEG (baseline and progressive decoding, baseline encoding), GIF, PCX, TIFF (none, PackBits, LZW), Targa; every format read and written. Host-tested against macOS `sips`, libtiff and the OS/2 files. |
| `crates/mmavi` | AVI files (idx1 or a scan of movi, OpenDML parts) and their video: uncompressed DIBs, RLE8, Microsoft Video 1, IBM Ultimotion. Host-tested against ffmpeg. |
| `userland/mmioproc` → MMIOPROC.DLL | rs2's file-format I/O procedures (images, AU/SND, VOC, AIFF, 8SVX, MIDI, AVI), listed in `MMOS2\MMPMMMIO.INI`. |
| `userland/mmapps` → MMOS2\*.EXE | The applets: MPPM (media player), WEPM (Digital Audio editor), CDPM (CD Player), MMVOLUME (Volume Control), STPM (Multimedia Setup). |
| `crates/mci` | The media control interface's string commands (words, keywords, WAIT/NOTIFY), device types and names, file extensions, MCIERR codes and texts, the memory playlist processor. Host-tested. |
| `userland/mdm` → MDM.DLL | The media device manager: mciSendCommand, mciSendString, the wave audio and digital video devices. |

IBM's files are only used for study and testing (under `work/`): the
Toolkit headers (`os2me.h`, `mmioos2.h`, `meerror.h`) give the structures
and constants, the MMPM/2 references (`book/mmref*.inf`) the API, and
IBM's MMPM.DLL (which MMIO.DLL forwards to) the behaviour and return codes.

## mmfmt

### `mmfmt::flc`: FLI and FLC animations

A file is a 128-byte header and a list of frame chunks, each holding
sub-chunks that change the palette or the previous frame's pixels. After
the last frame comes the ring frame, which turns the last frame back into
the first so an animation loops without starting over.

```rust
let mut d = flc::Decoder::new(data)?;     // D: AsRef<[u8]> (Vec<u8> by default): the whole file
d.header();                               // &flc::Header { size, magic, frames, width, height, depth,
                                          //   flags, speed (raw), oframe1, oframe2 }; Header::delay_ms()
d.width(); d.height();                    // usize
d.frame_count();                          // u16, without the ring frame
let f = d.next_frame()?;                  // flc::Frame { index: u16, palette_changed: bool,
                                          //   looped: bool, delay_ms: u32 }
d.frame();                                // &[u8]: width * height palette indexes, top row first
d.palette();                              // &[[u8; 3]; 256]: RGB, 0-255
d.current(); d.rewind();
```

`next_frame` decodes the next frame into the frame buffer; after the last
frame it decodes the ring frame, which gives frame 0 again
(`looped == true`); a file without a ring frame starts over from its first
frame. `delay_ms` is the frame's own delay or the header's speed (FLI
speed is in 1/70 s, FLC in ms). Chunks decoded: COLOR_256, COLOR_64,
DELTA_FLC (SS2), DELTA_FLI (LC), BLACK, BYTE_RUN, FLI_COPY; postage stamps,
prefix chunks and others are skipped. Errors: `NotFlc`, `Truncated`,
`BadFrame` (nothing panics on bad data).

The games' animations are 320×200 with 8-bit pixels: KLONDIKE.FLC 186
frames at 71 ms, OS2CHESS.FLC 182 at 71 ms, MAHJONGG.FLC 160 at 214 ms.
Their first frames are black (they fade in). `cargo run -p mmfmt --example
flcdump FILE.FLC OUTDIR [EVERY]` writes frames as PPM images.

### `mmfmt::wav`: WAVE audio

```rust
let w = wav::parse(&file)?;               // wav::Wave { format, data_offset, data_len }
w.data(&file);                            // the samples (cut short if the file is)
w.format;                                 // wav::WaveFormat { tag, channels, rate, bits,
                                          //   block_align, avg_bytes }
w.format.is_plain_pcm();                  // 8/16-bit PCM, mono/stereo: play as is
w.format.duration_ms(bytes); w.format.frames(bytes);
wav::decode_pcm16(&format, data);         // Option<Vec<i16>>: PCM 8/16, A-law, µ-law, IMA ADPCM
wav::build(&format, data);                // a 44-byte-header WAVE file
```

`parse` accepts the start of a file up to the data chunk's header (the
data range is what the chunk declares). All of MMPM/2's sample sounds are
PCM: 8-bit mono at 11025 or 22050 Hz, or 16-bit mono at 22050 Hz.

### `mmfmt::riff`

`fourcc(b"WAVE")` (first character in the low byte, as `mmioFOURCC`),
`fourcc_from_str` (`mmioStringToFOURCC`), `padded(size)`, `Header::parse`
and `chunks(data)`, an iterator of `Chunk { header, offset, data }` with
`children()` for RIFF and LIST forms.

### `mmfmt::sound`: other digital audio files

`sound::parse(&file)` reads Sun/NeXT `.AU`/`.SND` (µ-law, A-law, linear
8-32 bits, big-endian), Creative Voice `.VOC` (sound data, continuation,
silence, repeat, extended and new-format blocks), AIFF/AIFC (the 80-bit
rate; NONE, twos, sowt, raw, ulaw, alaw) and Amiga 8SVX into a `Sound`:
a `WaveFormat` for 8-bit unsigned or 16-bit signed PCM and the samples,
what a WAVE file of the sound holds. `build_au`, `build_voc` and
`build_aiff` write PCM back.

## MMIO.DLL

An MMIO handle is an open file with an I/O procedure. As in IBM's MMIO the
handle is an MMIOINFO that the procedure gets with each message
(MMIOM_OPEN, READ, WRITE, SEEK, CLOSE, GETHEADER...), with an optional
I/O buffer on top: `lDiskOffset` is the file offset of the buffer's first
byte, `lLogicalFilePos` the caller's position. HMMIOs are per process.

| Call | rs2 |
|------|-----|
| mmioOpen | Files by name, or a DOS file handle in `aulInfo[0]`; MMIO_READ/WRITE/READWRITE/CREATE/APPEND/DELETE and the sharing flags (IBM's mapping to DosOpen modes); MMIO_ALLOCBUF or a caller's buffer; memory files (`fccIOProc` = MEM) in a caller's buffer or one MMIO allocates and grows by `aulInfo[0]`. Without an I/O procedure the file is identified: RIFF WAVE files get the WAVE procedure, anything else is a DOS file. Errors in `ulErrorRet` (DosOpen's code for a missing file). |
| mmioRead, mmioWrite, mmioSeek, mmioFlush, mmioClose | Buffered or not; 0 at the end of the file with MMIOERR_EOF_SEEN. |
| mmioGetInfo, mmioSetInfo, mmioAdvance, mmioSetBuffer | Direct buffer access as IBM's (UNBUFFERED, READ_ONLY_FILE... codes). |
| mmioDescend, mmioAscend, mmioCreateChunk | RIFF chunks. With a parent, mmioDescend searches from the parent's first sub-chunk, as IBM's does; mmioAscend fixes the size of a chunk being written and pads it. |
| mmioGetHeader, mmioSetHeader, mmioQueryHeaderLength | Through the procedure: the WAVE procedure's MMAUDIOHEADER (40 bytes; `ulAudioLengthInBytes` and `ulAudioLengthInMS`). |
| mmioSet | Passed to the procedure (WAVE: no tracks or CODECs). |
| mmioIdentifyFile, mmioIdentifyStorageSystem, mmioQueryFormatCount, mmioGetFormats, mmioGetFormatName | The format list holds WAVE and the procedures of MMPMMMIO.INI (below); a file none of them recognises is a DOS file. |
| mmioIniFileHandler | MMPMMMIO.INI's procedure list, as IBM's: MMIO_INSTALLPROC (the DLL is loaded and its procedure asked for its MMFORMATINFO unless MMIO_NOVERIFY; the module name is kept unless MMIO_FULLPATH; a procedure for the same FOURCC is replaced), MMIO_REMOVEPROC and MMIO_FINDPROC with MMIO_MATCHFIRST/MATCHNEXT, MATCHFOURCC, MATCHDLL, MATCHPROCEDURENAME. |
| mmioInstallIOProc, mmioSendMessage, mmioStringToFOURCC, mmioGetLastError | As IBM's; installed procedures are per process and take precedence over the built-in ones. |
| DOSIOProc, MEMIOProc, WAVEIOProc | The built-in procedures (WAVEIOProc is an rs2-private export, ordinal 300; IBM's is in WAVEPROC.DLL). |
| mmioCF*, BNDIOProc, CFIOProc, CODEC calls, mmiohlp* | Not supported: MMIOERR_UNSUPPORTED_FUNCTION (NULL for mmioCFOpen and mmioLoadCODECProc). |

The WAVE procedure reads and seeks within the sample data (position 0 is
the first sample); writing a new file (MMIO_CREATE with `fccIOProc` =
WAVE) takes the format from mmioSetHeader and writes the RIFF header at
mmioClose. No data translation is done (MMIO_TRANSLATEDATA is ignored).

The exports (`userland/mmio/exports.def`, `src/exports.rs`) are generated
by `tools/scripts/gen_pm_exports.py` from a list of IBM's MMIO ordinals
(`lxdump info MMIO.DLL` of IBM's file, as `MMIO<TAB>ordinal<TAB>name` lines)
and `exports.private`.

MMIOTEST (`cargo xtask run --headless --timeout 60 --init "RS2\MMIOTEST.EXE"`)
writes a WAVE file with the chunk calls, reads it back as the games do
(also through DosLoadModule), descends through it, writes one with the
WAVE procedure, and checks memory files, buffered I/O and error codes.

### The installed I/O procedures (MMPMMMIO.INI)

IBM's MMIO keeps the file-format procedures installed in the system in
`%MMBASE%\MMPMMMIO.INI` (studied in Ghidra: mmioIniFileHandler,
mmiohlpLoadDefIOProcs in IBM's MMPM.DLL): application `MMIOINIT`, key
`IOCOUNT` (a USHORT) and key `IOMODINFO`, an array of MMINIFILEINFO
records of 316 bytes (FOURCC, DLL name, entry point name, capability
flags, extended length 16, media type, procedure type, default
extension), as MINSTALL's `mmioInstall` entries write them. rs2's MMIO
reads the list once per process (`userland/mmio/src/ini.rs`) and loads
a procedure (DosLoadModule, DosQueryProcAddr) when it is first needed.
mmioOpen without a procedure asks the procedures the process installed
itself, then WAVE, then the list's in its order (MMIOM_IDENTIFYFILE);
mmioOpen with a FOURCC, mmioInstallIOProc's MMIO_FINDPROC and the
format calls find the list's procedures too. Without the profile (or
without IOCOUNT) the list is rs2's own (`userland/mmio/ioprocs.txt`),
which xtask also writes into the distribution as `MMOS2\MMPMMMIO.INI`.

rs2's procedures are in MMIOPROC.DLL (`userland/mmioproc`, one entry
point per FOURCC; IBM ships them as separate DLLs):

| FOURCC | Entry point | Format | Read | Write |
|--------|-------------|--------|------|-------|
| OS13, OS20, WI30 | OS13IOProc, OS20IOProc, WI30IOProc | OS/2 1.x, OS/2 2.0 and Windows bitmaps (`.BMP`; each knows its own header) | yes | uncompressed |
| JPEG | JPEGIOProc | JPEG (`.JPG`) | baseline and progressive | baseline JFIF, quality 85 |
| GIFC | GIFCIOProc | GIF (`.GIF`, first image) | yes | LZW (true colour reduced to a 6x7x6 colour cube) |
| PCXC | PCXCIOProc | PCX (`.PCX`) | yes | RLE |
| TFIC, TFIU, TFMC, TFMU | TF..IOProc | TIFF, Intel or Motorola byte order, compressed or not (`.TIF`) | none, PackBits, LZW | LZW or uncompressed |
| TGAC, TGAU | TGACIOProc, TGAUIOProc | Targa, run-length encoded or not (`.TGA`; last in the list: no magic number) | yes | yes |
| `AU  `, `SND ` | AUIOProc, SNDIOProc | Sun/NeXT audio (µ-law, A-law, linear 8-32 bits) | yes | 8/16-bit linear |
| `VOC ` | VOCIOProc | Creative Voice | yes | 8/16-bit PCM |
| AIFF | AIFFIOProc | AIFF and AIFC (NONE, twos, sowt, raw, ulaw, alaw) | yes | 8/16-bit PCM |
| `IFF ` | IFFIOProc | Amiga 8SVX | yes | no |
| MIDI | MIDIIOProc | Standard MIDI files and RIFF RMID | the file's MIDI data, MMMIDIHEADER | no |
| `AVI ` | AVIIOProc | AVI movies | identified, read untranslated | no |

They work as IBM's image and audio procedures do (the Toolkit's M-Motion
sample, `samples/mm/mmioproc`, shows the protocol): the file is read
through a storage-system handle (the DOS procedure, or
`fccChildIOProc`) and decoded at MMIOM_OPEN. With MMIO_TRANSLATEDATA an
image reads as an OS/2 bitmap (rows bottom-up, padded to four bytes; 1,
4, 8 or 24 bits per pel, B, G, R), described by the MMIMAGEHEADER
(1116 bytes: lengths and types, the XDIB prefix, BITMAPINFOHEADER2 and
256 RGB2) that mmioGetHeader gives; without it the file's own bytes are
read. Sounds read as PCM (8-bit unsigned or 16-bit signed, as in a WAVE
file) with an MMAUDIOHEADER. A file opened MMIO_WRITE with translation
takes its header from mmioSetHeader and its bitmap or samples from
mmioWrite, and is encoded at mmioClose. Reading and writing one file at
once (MMIO_READWRITE) is refused, as by IBM's procedures. One image per
file (MMIOM_QUERYIMAGECOUNT is 1). The formats are `crates/mmimage` and
`mmfmt::sound`.

MMIOTEST checks the list (enumerated as the Toolkit's MMBROWSE sample
does it; a procedure installed, found, removed; a missing DLL refused),
the format list, and every image and audio procedure: a 37 x 23 picture
(and an 8-bit one with its palette for GIF and OS/2 bitmaps) and 0.1 s
of sound written with translation, identified again by mmioOpen and
mmioIdentifyFile and read back (identical, JPEG within a tolerance).
IBM's MMBROWSE (the Toolkit sample, not shipped) shows a JPEG through
rs2's JPEG procedure.

**Ultimotion:** IBM's codec draws some blocks from a 4096-entry table of
luma patterns that is IBM's data (ULDC.DLL); rs2 does not ship it, so
those blocks are drawn flat (a blockier picture). `mmavi::ulti` takes a
table when one is supplied (`find_codebook` finds it in ULDC.DLL's
object 1). Indeo (IV31/IV32) and MPEG are not decoded.

## MDM.DLL

mciSendCommand and mciSendString both become one request (`src/req.rs`)
that the device carries out; a string command is split by `crates/mci`
and its keywords set the message's flags and fields, as IBM's MDM does
with its command tables. Devices belong to the process (IBM's are
system-wide and can be shared); they are closed when it ends.

| Device | rs2 |
|--------|-----|
| waveaudio (Waveaudio01) | A WAV file as the element (PCM, or µ-law/A-law/IMA ADPCM decoded by mmfmt), or an AU, SND, VOC, AIFF or 8SVX file (`mmfmt::sound`, as their I/O procedures present them) or a memory playlist (MCI_OPEN_PLAYLIST: DATA, BRANCH, LOOP, CALL/RETURN, MESSAGE, CUEPOINT, SEMWAIT/SEMPOST, EXIT). MCI_SET of the wave format (as the games set it before playing a playlist), time formats (MMTIME, milliseconds, bytes, samples), volume. PLAY (FROM/TO), STOP, PAUSE/RESUME, SEEK, STATUS (mode, length, position, the format, LEVEL, MONITOR). Played through the kernel's audio streams (RS2API 300-308, docs/ROADMAP.md); without sound hardware the device plays silently in real time, so PLAY WAIT and notifications behave the same. Recording (see below): RECORD, SAVE, DELETE, CUE INPUT, CONNECTOR, CONNECTION, SET MONITOR, LOAD NEW. |
| digitalvideo (Digitalvideo01) | FLI/FLC animations and AVI movies (`crates/mmavi`: uncompressed, RLE8, Video 1, Ultimotion; their PCM sound plays along through the audio stream, the frames keep to the clock). Frames are drawn with GpiDrawBits into the application's window (WINDOW HANDLE, PUT DESTINATION) or into MDM's own video window (a frame window titled with the file name, run by a thread of MDM's with its own message queue). PLAY (FROM/TO, FAST), STOP, PAUSE/RESUME, SEEK, CUE, LOAD, WINDOW (HANDLE, DEFAULT, STATE, TEXT), PUT, WHERE, STATUS (length, position, extents, hwnd). No audio track. |

Common to both: MM_MCINOTIFY (successful, superseded by a new command,
aborted by STOP/SEEK/CLOSE), CAPABILITY/GETDEVCAPS, INFO, ACQUIRE/RELEASE
(always granted), SYSINFO (one device of each type; see below for
MCI_SYSINFO_ITEM), mciGetErrorString's
texts and mciGetDeviceID. A string command whose object is a file name
opens the file's device for that command (`play KLONDIKE.FLC wait`) and
closes it when the command, or the play it started, ends. Groups and
the amp-mixer device are not supported.

### The installed devices (MCI_SYSINFO_ITEM)

IBM's MDM keeps its installed devices in MMPM2.INI and answers
MCI_SYSINFO with MCI_SYSINFO_ITEM from it; rs2's devices are built in
(one waveaudio, sequencer, cdaudio and digitalvideo device), so MDM
answers the queries for them: QUERY_NAMES (install name `rs2Waveaudio01`,
logical name `Waveaudio01`, no alias, type and ordinal, found by any of
the names), QUERY_DRIVER (MCI_SYSINFO_LOGDEVICE: the type, controllable,
MCD "MDM", one resource unit), QUERY_PARAMS (none), QUERY_CONNECTORS
(none), QUERY_EXTENSIONS (WAV, AIF, AU, SND, VOC, _AU; MID, RMI, _MI; FLC,
FLI, AVI, MPG), QUERY_TYPES (the EA types "Digital Audio", "MIDI",
"Digital Video") and QUERY_DEFAULT. The items that change the
configuration (installing and deleting drivers, aliases, extensions,
types, the default device) are refused with
MCIERR_UNSUPPORTED_FUNCTION. AUDTEST checks the queries.

Multimedia Setup (IBM's STPM.EXE, in MMTEMP on the CD with its SW,
MMPMCRTS, MIPM, MMPM, FILT, HHP, DIVE and STPMMRI DLLs) builds its
notebook from them: the System page and a page for each device (name,
description, default device). It needs `MMBASE` set: IBM's FILT.DLL
writes through an uninitialised pointer when DosScanEnv does not find it.
MINSTALL.EXE (disk 11's MMTEMP) shows its installation window; it looks
for its CONTROL.SCR in its own directory (start it with its path, as the
installer does) and has nothing to install from without the packed
MMPM/2 source. MPPMNEW.EXE and IBM's multimedia Workplace Shell classes
(MMPARTS.DLL) work: see below.

### MMPARTS.DLL and MPPM

MMPARTS.DLL (in two pieces on the CD: disk 10's `MMPARTS.DL1` followed
by disk 11's `MMPARTS.DL2`, 1,606,604 bytes) holds IBM's multimedia
Workplace Shell classes (MMDataFile, MMAudio, MMWAV, MMVOC, MMImage,
MMJPG, MMTarga and the other sound, image and video classes, MMFolder,
the device objects) and the players, editors and viewers they open. It
is not OpenDoc: it is built with VisualAge C++'s Open Class library,
whose runtime DLLs are on the CD (OS2OM30.DLL, OS2OOOC.DLL, OS2OOR3U.DLL,
IBM's: used as they are where they are installed), with SW.DLL,
MMPMCRTS.DLL, DIVE.DLL and MMPTMRI.DLL (its texts). How the shell takes
its classes is in [WPS.md](WPS.md#ibms-multimedia-classes-mmparts).
MPPMNEW.EXE (installed as `\MMOS2\MPPM.EXE`; IBM's copies of it named
CDPM, WEPM, AB, IB, VB open a device of their own) opens a file in
MMPARTS's player: `MPPM C:\MMOS2\SOUNDS\DSK_STRT.WAV` shows the player
window with the sound's length, and its play button plays it (checked
in QEMU with `--sound hda --sound-wav`: the 4.5 seconds of sound came
out). Without a file it calls MMPARTS's DisplayFinder (not tried).

What Open Class's multimedia classes (IMMDevice and friends, compiled
into MMPARTS) needed of MDM, each as IBM's MDM does it (studied in
Ghidra in IBM's MMPM.DLL, which holds MDM):

- **MM_MCIPASSDEVICE.** A new device instance becomes the active one and
  its callback window gets `MM_MCIPASSDEVICE` (device ID, MCI_GAINING_USE)
  (IBM's MakeInstanceActive and mdmPassDeviceNotify). IMMDevice counts
  a device open only after it: without it every viewer gave up with
  "Device Not Open".
- **MCI_SYSINFO_ITEM 0x8000**, an item the Toolkit's headers leave out:
  `{ PSZ pszFile; CHAR szDevice[20] }`, the device a file plays on by its
  extension, as `waveaudio01` (IBM's mmFindExt and GetName). MPPM's
  generic player finds the device for its file so.
- **SET POSITION ADVISE** (`MCI_SET_POSITION_ADVISE_ON`/`OFF`,
  MCI_POSITION_PARMS): while the wave device plays, `MM_MCIPOSITIONCHANGE`
  (user word and device ID, the position) whenever the position reaches
  another multiple of the units; the player's position field and slider
  follow it.
- **COPY TO BUFFER** on the wave device (MCI_EDIT_PARMS): the samples
  from FROM to TO in the caller's buffer; a buffer too small gets the
  size needed and `MCIERR_INVALID_BUFFER` (IBM's StartCopy). The wave
  editor reads its samples so. Copying to the clipboard, CUT, PASTE,
  UNDO and REDO are not supported.
- The amp-mixer's STATUS items on the wave device (IBM's passes them to
  its amp-mixer; rs2 has none): treble, bass, balance, gain and mid at
  50, pitch 0.

`SET MDMTRACE=1` writes every mciSendCommand (with its parameter
block's first words) and mciSendString and what they return to the
serial port; MMPARTS's own log comes with `SET MCICPPLG=1`.

**Not yet:** IBM's wave editor (an MMAudio object's default view under
IBM's classes) opens with its menus, buttons and the file, but draws no
waveform; the digital video and MIDI viewers were not tried. rs2 now has
its own classes and applets for all of this (below), so IBM's MMPARTS is
not needed.

### Recording

The wave audio device records through the kernel's capture stream
(RS2API 309-312) in its format (MCI_SET of the rate, channels and bits
before RECORD; PCM only):

- RECORD [FROM pos] [TO pos] [INSERT | OVERWRITE]: from the position (or
  FROM) until TO, STOP or PAUSE (RESUME goes on); OVERWRITE is the
  default, as in IBM's waveaudio. A new file (`open NEW.WAV type
  waveaudio`, a name that does not exist), `open new type waveaudio` /
  `load ... new`, or a device opened without an element records into an
  empty element. With a memory playlist the recording fills its DATA
  buffers in order (operand 3: the bytes recorded), with MESSAGE,
  CUEPOINT and the semaphores as in playing. A RECORD NOTIFY is answered
  when TO is reached (successful), superseded by another RECORD or PLAY,
  or aborted by STOP. Without input hardware the device records silence
  in real time.
- SAVE [file]: the element as a 44-byte-header WAV file, under its name
  or the one given (which becomes the element's name).
- DELETE [FROM pos] [TO pos]: cuts a range out of the element.
- CUE INPUT opens the capture stream ahead of RECORD; STATUS LEVEL is then
  the input's recent peak, left and right as percentages in the low and
  high words (0 when nothing is cued or recording).
- CONNECTOR ENABLE TYPE MICROPHONE | LINE IN chooses the input source
  (Rs2AudioSetInput; the other one when the hardware lacks it), QUERY
  tells which is on; the wave stream and the outputs are always on.
- SET MONITOR ON plays what is recorded as it comes, about 0.4 s late.
- CONNECTION QUERY TYPE WAVE STREAM [ALIAS name] answers with the
  device's own ID: rs2 has no amp-mixer device, so the alias names the
  wave device, and the commands IBM's RECORD.CMD sends to it (SET MONITOR
  OFF, CONNECTOR ENABLE TYPE MICROPHONE) reach the wave device.

IBM's RECORD.CMD (MMTEMP on the CD) runs: `RECORD C:\FILE.WAV`, Enter to
stop. Tests: `cargo xtask run --headless --timeout 60 --sound-in tone:440
--init "RS2\AUDTEST.EXE MCI"` (the command interface: playlist and file
recording, SAVE, CONNECTOR, CONNECTION), `AUDTEST RECORD` for the
kernel calls; `--sound ac97` and `--sound es1370` for the other cards.

The games find their sounds in `%MMBASE%\SOUNDS` (rs2's CONFIG.SYS sets
`MMBASE=C:\MMOS2;`) and their movies in their own directory; APPINST
installs both from the OS/2 CD with the games.

## rs2's multimedia applets

`userland/mmapps`, PM programs in `%MMBASE%` (`C:\MMOS2`), which rs2's
multimedia classes (WPS.md) open:

- **MPPM** (`MPPM [file]`), the media player: sounds (WAV, AU, SND, VOC,
  AIF, IFF), MIDI songs and movies (AVI, FLC, FLI) through MDM's devices
  (`open file alias mppm`; a movie's device draws into the player's
  window with `window mppm handle`), with Play, Pause, Stop, Rewind and
  Forward (5 s), a position bar to seek with, the time, a volume bar
  (`set mppm audio volume`); images (BMP, DIB, JPG, GIF, PCX, TIF, TGA)
  read through MMIO's image procedures with translation and drawn fitted
  into the window. Without a file, or Open..., the open dialog.
- **WEPM** (`WEPM [file]`), Digital Audio: a sound read through MMIO
  (any audio procedure) shown as a waveform; a selection dragged with the
  mouse; Play (the selection, or all; through a temporary WAV in
  %MMBASE%), Stop, Record (MDM's wave audio device records in the
  sound's format and the recording is inserted at the cursor), Cut,
  Copy, Paste, Delete, New, Open..., Save and Save as... (through MMIO,
  the format by the extension: WAV, AU, SND, VOC, AIF). An empty file (a
  new one from the Wave Audio template) starts a new sound.
- **CDPM** (`CDPM [device]`), the CD Player: the first drive with a disc
  (Cdaudio01, 02, ...), its tracks (double-click one to play it), Play,
  Pause, Stop, previous and next track, Eject, the track and time.
- **MMVOLUME**, Volume Control: the master volume of all sound (the
  kernel mixer's, Rs2AudioSetVolume with stream 0) and Mute, kept in
  OS2.INI `MMPM2` `MasterVolume`/`Mute`; MDM gives them to the mixer
  when a process first opens a device (the mixer starts at 100).
  `MMVOLUME n`, `MUTE`, `UNMUTE`, `QUERY` without the window.
- **STPM**, Multimedia Setup: the System page (the sound hardware, the
  master volume and mute, the recording source, Rs2AudioSetInput, kept
  as `MMPM2` `RecordSource`, and a test sound) and a page for each
  device MDM has (its name, what it plays, its file types). rs2's
  devices are built in, so nothing is installed or removed there.

Checked in QEMU with screenshots: MPPM playing a Video 1 AVI (and showing
a JPEG opened from the shell), WEPM with a WAV and a selection, CDPM
playing `cdsim`'s simulated disc, STPM's System page.

## The MIDI sequencer and MCIAPI.DLL

`crates/midi` reads Standard MIDI files (formats 0-2, running status,
the tempo map, RIFF RMID) and has a small General MIDI software
synthesizer: two-operator FM timbres, one per GM program family, with
envelopes; synthesized drums on channel 10; volume, expression, pan,
sustain, pitch bend (with its RPN range) and program changes; a soft
limiter at the output. `Player` renders a song from any position (a seek
replays the controllers and programs set before it).
`cargo run -p midi --example midi2wav IN.MID OUT.WAV [seconds]` renders a
song on the host.

MDM's sequencer device (`userland/mdm/src/sequencer.rs`, Sequencer01)
plays a .MID element through it at 32 kHz, streamed through the RS2API
audio calls like the wave audio device (clock-paced without sound
hardware): PLAY (FROM/TO), STOP, PAUSE/RESUME, SEEK, LOAD, STATUS
(length, position, tempo, volume), SET volume and time format (MMTIME or
milliseconds). MIDI output to external ports and the port mapper are
not supported.

MCIAPI.DLL (`userland/mciapi`) is MMPM/2's REXX interface:
`RXFUNCADD('mciRxInit', 'MCIAPI', 'mciRxInit')`, then
`mciRxSendString(cmd, 'var', hwnd, user)`, `mciRxGetErrorString`,
`mciRxGetDeviceID` and `mciRxExit`; it finds REXX's RexxVariablePool and
RexxRegisterFunctionDll when first needed. `mciPlayFile` plays any media
file to the end (asynchronously with MCI_ASYNC). IBM's PLAY.CMD (from the
multimedia pack) works: `PLAY FILE=FURELISE.MID`.

## CD audio

MDM's cdaudio device (`userland/mdm/src/cdaudio.rs`, Cdaudio01 for the
first CD drive, Cdaudio02 for the second...) plays audio CDs digitally:
it opens the drive as a volume (`DosOpen("E:", OPEN_FLAGS_DASD)`), reads
the table of contents with OS/2's CD-ROM IOCtls (category 0x81: disc and
track information; category 0x80: device status for the disc) and the
audio with READ LONG (0x80/0x72, raw 2352-byte frames), and streams them
as 44.1 kHz 16-bit stereo through the RS2API audio calls like the wave
audio device. Playing through the drive's analog output (PLAYAUDIO) is
not used. Commands: OPEN/CLOSE; PLAY FROM/TO; STOP, PAUSE/RESUME, SEEK
(TO START/END/position); SET time format (MMTIME, milliseconds, MSF,
TMSF), audio volume, door open/closed/locked/unlocked; STATUS media
present, ready, mode (not ready without a disc), number of tracks,
length (of the disc or a TRACK), position (current, START, of a TRACK),
current track, position in track, track type/copy permitted/channels/
pre-emphasis; CAPABILITY; INFO product, ID, UPC. Data tracks are not
played. String commands take TMSF times as `t:mm:ss:ff` and MSF as
`mm:ss:ff` (and answer in that form); positions in MMTIME, milliseconds
and MSF are disc addresses (the first track starts at 00:02:00), lengths
are durations. The time logic is `crates/mci/src/cd.rs` (host tests).

The kernel side (`kernel/src/fs/cdaudio.rs`, `kernel/src/api/cdioctl.rs`):
CD drives take MMC commands through `BlockDev::packet` (IDE and AHCI
ATAPI, PVSCSI/MPT SCSI, virtio-scsi; not USB), and the IOCtls follow
IBM's OS2CDROM.DMD packets (studied in Ghidra): category 0x80 reset,
eject/close tray, lock, seek, device status, driver ID, sector size, head
location, READ LONG, volume size, UPC; category 0x81 channel control,
PLAYAUDIO/STOP/RESUME, channel info, disc info, track info, Q channel
(BCD track and index), audio status.

Tested in QEMU: READ CD on PIIX IDE and AHCI (data frames of an ISO);
QEMU's SCSI CD (virtio-scsi, PVSCSI, MPT) rejects READ CD, and only ISO
data tracks exist there, so `cdsim` on the kernel command line adds a
drive with a simulated audio disc (tracks of 440 Hz and 660 Hz tones and
a data track) for testing MDM end to end; the played audio was checked in
a `--sound-wav` recording. Not yet tried with a real audio CD, on real
drives, or on VMware.
