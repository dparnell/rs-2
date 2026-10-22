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
| mmioIdentifyFile, mmioIdentifyStorageSystem, mmioQueryFormatCount, mmioGetFormats, mmioGetFormatName | The format list holds WAVE; other files are DOS files. |
| mmioInstallIOProc, mmioSendMessage, mmioStringToFOURCC, mmioGetLastError | As IBM's; installed procedures are per process and take precedence over the built-in ones. |
| DOSIOProc, MEMIOProc, WAVEIOProc | The built-in procedures (WAVEIOProc is an rs2-private export, ordinal 300; IBM's is in WAVEPROC.DLL). |
| mmioCF*, BNDIOProc, CFIOProc, mmioIniFileHandler, CODEC calls, mmiohlp* | Not supported: MMIOERR_UNSUPPORTED_FUNCTION (NULL for mmioCFOpen and mmioLoadCODECProc). |

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

## MDM.DLL

mciSendCommand and mciSendString both become one request (`src/req.rs`)
that the device carries out; a string command is split by `crates/mci`
and its keywords set the message's flags and fields, as IBM's MDM does
with its command tables. Devices belong to the process (IBM's are
system-wide and can be shared); they are closed when it ends.

| Device | rs2 |
|--------|-----|
| waveaudio (Waveaudio01) | A WAV file as the element (PCM, or µ-law/A-law/IMA ADPCM decoded by mmfmt) or a memory playlist (MCI_OPEN_PLAYLIST: DATA, BRANCH, LOOP, CALL/RETURN, MESSAGE, CUEPOINT, SEMWAIT/SEMPOST, EXIT). MCI_SET of the wave format (as the games set it before playing a playlist), time formats (MMTIME, milliseconds, bytes, samples), volume. PLAY (FROM/TO), STOP, PAUSE/RESUME, SEEK, STATUS (mode, length, position, the format). Played through the kernel's audio streams (RS2API 300-308, docs/ROADMAP.md); without sound hardware the device plays silently in real time, so PLAY WAIT and notifications behave the same. |
| digitalvideo (Digitalvideo01) | FLI/FLC animations. Frames are drawn with GpiDrawBits into the application's window (WINDOW HANDLE, PUT DESTINATION) or into MDM's own video window (a frame window titled with the file name, run by a thread of MDM's with its own message queue). PLAY (FROM/TO, FAST), STOP, PAUSE/RESUME, SEEK, CUE, LOAD, WINDOW (HANDLE, DEFAULT, STATE, TEXT), PUT, WHERE, STATUS (length, position, extents, hwnd). No audio track. |

Common to both: MM_MCINOTIFY (successful, superseded by a new command,
aborted by STOP/SEEK/CLOSE), CAPABILITY/GETDEVCAPS, INFO, ACQUIRE/RELEASE
(always granted), SYSINFO (one device of each type), mciGetErrorString's
texts and mciGetDeviceID. A string command whose object is a file name
opens the file's device for that command (`play KLONDIKE.FLC wait`) and
closes it when the command, or the play it started, ends. Connections,
groups, recording and saving are not supported.

The games find their sounds in `%MMBASE%\SOUNDS` (rs2's CONFIG.SYS sets
`MMBASE=C:\MMOS2;`) and their movies in their own directory; APPINST
installs both from the OS/2 CD with the games.

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
