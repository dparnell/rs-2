# OS/2 packed files ("FTCOMP")

The OS/2 Warp 4.52 installation media ship most files compressed. The
installer expands them with `UNPACK2.EXE` (16-bit NE program; the
compression library is statically linked, no DLL is involved). The same
library is also linked into WinOS2's `LZEXPAND.DLL`. This document describes
the format as determined from disassembly of `UNPACK2.EXE` and from the media
themselves. It is implemented by `crates/os2-pack` (`tools/unpack` is the
host front end).

All integers are little-endian.

## Container

A packed file is a sequence of entries followed by a 32-bit entry count.
Single-file packs (`FOO.EX_`, `FOO.DLL`, ...) have one entry; `BUNDLE`,
`REQUIRED`, `PACKnn`, ... have many.

Entry header (0x29 bytes, then the path):

| Offset | Size | Field |
|-------:|-----:|-------|
| 0x00 | 4 | magic `A5 96 FD FF` |
| 0x04 | 2 | last-write date (FAT/OS/2 `FDATE` layout) |
| 0x06 | 2 | last-write time (`FTIME` layout) |
| 0x08 | 4 | file attributes (`FILE_ARCHIVED` = 0x20 ...) |
| 0x0C | 4 | file offset of the compressed EA stream, 0 if none |
| 0x10 | 4 | uncompressed file size |
| 0x14 | 4 | file offset of the next entry header, 0 for the last entry |
| 0x18 | 9 | algorithm name, NUL terminated (`FTCOMP`; bytes after the NUL are junk) |
| 0x21 | 2 | algorithm version (1) |
| 0x23 | 4 | uncompressed EA size (a FEA2LIST; equals its `cbList`, 4 = empty) |
| 0x27 | 2 | path length including the NUL |
| 0x29 | n | target path, e.g. `\OS2\DLL\PMVIOP.DLL` |

The compressed file data starts right after the path and runs to the EA
stream (if present), else to the next header, else to the trailing count.
The EA stream runs from its offset to the next header / trailing count.
UNPACK2 checks name (case-insensitive) and version before decompressing.

## Stream

Each stream (file data or EAs) is decompressed independently and starts with
four bytes that are skipped (always `80 60 00 00` on the media). Then come
blocks, each starting with the signature `fT19` (`66 54 31 39`). If the
remaining data is shorter than four bytes or does not start with the
signature, the rest of the stream is stored uncompressed.

A block decodes (Huffman stage) into a *token buffer* of at most 0xFE00
bytes. The packer produces one block per 0x6000 input bytes. The token
buffer is a sequence of chunks `u16 length, length bytes`; each chunk holds
one flag byte (0 = the rest is stored, else LZ coded) and at most 0x3008
bytes of output (0x3000 in practice).

### LZ stage

The LZ window is 0xCFDC bytes and persists across the chunks and blocks of
one stream. At the start of a stream it is initialised to 0xBC62 zero bytes,
0x140 spaces, 0x140 0xFF bytes, 0x140 zero bytes and a 0xFBA-byte preset
dictionary (common strings from executables: C runtime messages, x86
prologues, device names, ...; reproduced in `crates/os2-pack/src/tables.rs`).

Every byte except 0x9E is a literal. 0x9E introduces an escape `9E t ...`:

| `t` | meaning |
|-----|---------|
| 0x40 | literal 0x9E |
| 0x80 | `9E 80 n dlo dhi`: copy `0x43 + n` bytes, distance word |
| bit 0x40 set | `9E t dlo dhi`: copy `3 + (t & 0x3F)` bytes, distance word |
| otherwise | `9E t d`: copy `3 + t` bytes, distance byte |

A copy reads from `distance + 1` bytes back, byte by byte (overlap repeats).

### Huffman stage

Block layout after the signature:

* `u16 total`. If `total == 0xFFFF` the block is not entropy coded: a `u16`
  length and that many token bytes follow.
* otherwise `total` is the token buffer size, followed by four weight bytes
  `w0 w1 w2 w3` and an MSB-first bit stream.

The main alphabet has 0x1B1 symbols:

| symbol | token bytes produced |
|--------|----------------------|
| 0x000-0x0FF | that literal byte |
| 0x100-0x180 | `9E`, `sym - 0x100`, then the escape's extra bytes (below) |
| 0x181-0x190 | repeat the 2 token bytes found `sym - 0x17F` bytes back |
| 0x191-0x1A0 | repeat the `(sym - 0x191)`-th most recent word of the above kind |
| 0x1A1-0x1B0 | repeat the `(sym - 0x1A1)`-th most recent escape with its extra bytes |

The two recency lists are move-to-front lists kept as a downward-growing
stack of 0x30 slots starting at slot 0x20; when it wraps, the newest 16
(escapes) or 15 (words) entries are copied to slot 0x20 upwards and the stack
restarts at slot 0x1F.

Escape extra bytes are coded with a fixed tree "B", per a table of escape
types: types 0x00-0x3F one distance byte; 0x41-0x7F a distance word, sent as
a 1/2/2-bit class prefix (`0`, `10`, `11`) with 4/6/7 low bits and a tree-B
symbol `h` giving `((h + 0x10) << 4)`, `((h + 0x44) << 6)` or
`((h + 0xA2) << 7)` plus the low bits; type 0x80 three bytes (length, distance
low, distance high). In every case tree-B symbol 0x100 means "same value as
last time in this position" (for the first two bytes of type 0x80 it means 0,
other symbols give `sym + 1`). The five "last" registers, the recency lists
and the trees are reset for every block.

Per block, the 0x1B1 symbol counts (0..255) are sent first with a fixed tree
"A" (symbol 0x100 = run of 16 zero counts). Two trees are built from them:
symbol frequency = count × weight, where the weight depends on a fixed
per-symbol context class (escapes 0x100-0x13F, 0x141-0x180 and repeats
0x1A1-0x1B0 are class 1). Tree 1 uses `w0`/`w1` for class 0/1, tree 2 uses
`w3`/`w2`. If `w2 == w1 && w3 == w0` tree 2 equals tree 1. Frequencies whose
maximum exceeds 255 are rescaled by `(f * (0xFFFF / max) & 0xFFFF) >> 8`
(nonzero stays nonzero). The tree used for the next main symbol is tree 2
when the previous main symbol was class 1, else tree 1 (tree 1 at block
start).

Trees are never transmitted: both sides build them from frequencies with a
specific procedure whose tie-breaking is part of the format (16-bit
wrapping sums; frequency-1 leaves gathered first; an unstable
quicksort/insertion-sort hybrid; merged nodes re-inserted before the first
entry of equal or higher frequency; a 1 bit selects the lower-frequency
child). `crates/os2-pack/src/huffman.rs` documents the exact steps. Fixed
trees A and B come from constant frequency tables.

After a block, the decoder returns the whole bytes still buffered in its bit
reader; the next block starts at the next byte.
