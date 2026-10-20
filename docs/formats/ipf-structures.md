# OS/2 INF/HLP file format: on-disk structures (from HELPMGR.DLL)

Derived from the Ghidra decompilation of IBM's HELPMGR.DLL loaders (function names from
IBM debug symbols) and checked against real files from the rs2 media.

* Decompiled with `tools/ghidra/lxload.sh` and `lxdecomp.sh` (with IBM's chained
  internal fixups applied, which absolute addresses such as string constants and
  switch tables need).
* Checked with a parser written for the purpose (the Rust reader is `crates/ipf`,
  tested on the same files).
* Corpus: 279 distinct HSP files under `work/` (195 HLP and 84 INF). All of them parse with
  the layouts below: the TOC offsets agree with a sequential walk, the index entry count
  equals `nindex`, and the dictionary length equals `dictlen`.

Evidence tags: **[C]** = confirmed in the decompiled code, **[S]** = confirmed in sample
files, **[C+S]** = both, **[inf]** = inferred, not proven.

All integers are little-endian and unaligned. "Offset" means an absolute file offset
unless stated otherwise.

---------------------------------------------------------------------------------------

## 1. Main header (0x9B bytes at file offset 0)

`IOLoadDocHeader` always reads exactly 0x9B bytes from offset 0. It does not use the
`hdrsize` field to decide how much to read.

| Off  | Size | Name | Meaning | Ev. |
|------|------|------|---------|-----|
| 0x00 | 3 | magic | `"HSP"` | C+S |
| 0x03 | 1 | flags | 0x10 = HLP (help library), 0x01 = INF (VIEW book). The loader compares the whole first dword. A Help Manager instance (`inst+0xC == 0`) accepts only `HSP\x10`. A VIEW instance accepts `HSP\x01` or `HSP\x10`. Anything else gives HMERR_INVALID_LIB_FILE (0x2006). | C+S (195 files 0x10, 84 files 0x01) |
| 0x04 | 2 | hdrsize | Always 0x009B. Not checked by the loader. | S |
| 0x06 | 1 | version major | Must be 2 or less, otherwise 0x2006. If it is below 2, the database is marked "old format" (`db+0x42 = 1`) and the control-data section is ignored (see 2.6). | C+S |
| 0x07 | 1 | version minor | Not checked. Samples use 2.2 (252 files) and 2.0 (27 files). | S |
| 0x08 | 2 | ntoc | Number of TOC entries. If 0, the error is HMERR_CONTENT_NOT_FOUND (0x2002). | C+S |
| 0x0A | 4 | tocstrtablestart | Start of the packed TOC entries (section 3.2). | C+S |
| 0x0E | 4 | tocstrlen | Total byte length of the TOC entries. | C+S |
| 0x12 | 4 | tocstart | Start of the TOC offset table: `ntoc` × u32 absolute offsets (section 3.1). | C+S |
| 0x16 | 2 | nres | Count of the resource-id table. The loader masks it with 0x7FFF; the top bit is never set in the samples. | C+S |
| 0x18 | 4 | resstart | Start of the resource-id table (section 6). | C+S |
| 0x1C | 2 | nname | Count of the name table (also masked with 0x7FFF). | C+S |
| 0x1E | 4 | namestart | Start of the name table (section 7). | C+S |
| 0x22 | 2 | nindex | Number of index entries. | C+S |
| 0x24 | 4 | indexstart | Start of the index (section 8). | C+S |
| 0x28 | 4 | indexlen | Byte length of the index. | C+S |
| 0x2C | 2 | nglobindex | Count for the alternative "global" index table. | C (zero in all 279 files) |
| 0x2E | 4 | globindexstart | Start of that table. Same entry format as section 8. | C (zero in samples) |
| 0x32 | 4 | globindexlen | Byte length of that table. | C (zero in samples) |
| 0x36 | 4 | searchstart | Start of the full-text search table. Bit 31 set means the records use 16-bit sizes (section 12). | C+S |
| 0x3A | 4 | searchlen | Byte length of the search table. 0 means no search table. | C+S |
| 0x3E | 2 | nslots | Number of cells ("slots"). If 0, the error is 0x2002. | C+S |
| 0x40 | 4 | slotsstart | The COT: `nslots` × u32 absolute offsets of the cells (section 5). | C+S |
| 0x44 | 4 | dictlen | Byte length of the global dictionary. | C+S |
| 0x48 | 2 | ndict | Number of dictionary words. | C+S |
| 0x4A | 4 | dictstart | Start of the dictionary (section 4). | C+S |
| 0x4E | 4 | imgstart | Base offset for bitmaps. Bitmap escapes store an offset relative to this value (section 11). | C+S |
| 0x52 | 1 | maxCVTIndex | 0xF5 (245) in all samples. This is the maximum local-dictionary size per cell; the observed maximum `nlocal` is exactly 245. | S |
| 0x53 | 4 | nlsstart | Start of the NLS/country section (section 10). | C+S |
| 0x57 | 4 | nlslen | Length of the NLS section. The value 0x54 is significant (bidi field, section 10). | C+S |
| 0x5B | 4 | extstart | Start of the extended header. 0 means none (1 file in the corpus has none). | C+S |
| 0x5F | 12 | reserved | All zero in every sample. | S |
| 0x6B | 48 | title | NUL-padded title. It may fill all 48 bytes without a NUL (pm1.inf does). | S |

Your header layout was correct. The "10 unknown bytes" at 0x2C are
`nglobindex` (u16), `globindexstart` (u32) and `globindexlen` (u32). `IOLoadIndex(…, type != 2)`
reads 0x2E and 0x32, and `GetIndexEntries` reads 0x2C.

## 2. Extended header (0x40 bytes at `extstart`)

`IOLoadExtHeader` reads 0x40 bytes. The offsets in the table are relative to `extstart`.

| Off  | Size | Name | Meaning | Ev. |
|------|------|------|---------|-----|
| 0x00 | 2 | nfont | Number of font entries | C+S |
| 0x02 | 4 | fontstart | Font table offset (2.1) | C+S |
| 0x06 | 2 | ndb | Number of external databases | C+S |
| 0x08 | 4 | dbstart | Database-name table offset (2.2) | C+S |
| 0x0C | 4 | dbsize | Byte size of the database-name table | C+S |
| 0x10 | 2 | ngname | Count of the global-name table (masked 0x7FFF) | C+S |
| 0x12 | 4 | gnamestart | Global-name table offset (2.3) | C+S |
| 0x16 | 4 | stringsstart | Strings section offset (2.4) | S (2 files); no reader found |
| 0x1A | 2 | stringssize | Strings section size | S |
| 0x1C | 4 | childstart | Child-pages table offset (2.5) | C+S |
| 0x20 | 4 | childsize | Child-pages table size | C+S |
| 0x24 | 4 | nGIndex | Number of index entries flagged *global* (0x40) in this file's index. The loader reads it as a u16. | C+S (equal to the 0x40-entry count in all 14 files that set it) |
| 0x28 | 4 | ctrlstart | Control-data offset (2.6) | C+S |
| 0x2C | 4 | ctrlsize | Control-data size (8 in almost all files) | C+S |
| 0x30 | 16 | reserved | All zero in the samples | S |

### 2.1 Font table (`nfont` × 0x27 bytes, `IOLoadFontTable`) [C+S]

| Off  | Size | Meaning |
|------|------|---------|
| 0x00 | 33 | Face name, NUL-padded |
| 0x21 | 2  | Height (point size) |
| 0x23 | 2  | Width |
| 0x25 | 2  | Codepage |

Sample entry: `Courier 18 11 437`. Entry 0 is normally `System Proportional 0 0 437`, the
document default.

Behaviour of the loader:

* With no extended header it builds two entries: "System Proportional", plus a font from
  string resources 0x1784, 0x1785 and 0x1786. Resource 0x1785 sets width (+0x23) and
  0x1786 sets height (+0x21).
* For entry 1, if `GpiQueryFonts` reports that exactly one face matches, the name is replaced
  with "System Monospaced". The string constants were confirmed in HELPMGR's data object at
  0x2F0, 0x304 and 0x318.
* `:font` escapes in the text index into this table.

### 2.2 Database table (`IOLoadDataBaseInfo`) [C+S]

The table holds `ndb` entries in `dbsize` bytes. Each entry is `u8 len` (including the length
byte itself) followed by `len-1` characters, with no NUL.

Examples:
* EHXHP.HLP: `\x16\OS2\HELP\WPINDEX.HLP`
* PICVIEW.HLP: `\x1c\OS2\HELP\GLOSS\WPGLOSS.HLP`

These are the external files (`:link database=` / `:docprof`) that get chained after the
main file (section 13).

### 2.3 Global-name table [C+S]

The layout is identical to the name table (section 7), using `ngname` and `gnamestart`:
`ngname` × u16 dictionary indices (sorted), then `ngname` × u16 TOC indices. Used for
panels declared `global`.

Example from WPINSDEV.HLP: `('32001',37), ('32020',27)…`.

### 2.4 Strings section [S only]

Sequence of `u16 len` (including the 2-byte length field) followed by `len-2` characters.

Example from TASK.INF, 54 bytes: `OnlineInfoCommObject`, `ipfcntrl`, `OptionsFile=task.opt`.
These look like `:docprof` objectname/dll/objectinfo values **[inf]**. No HELPMGR loader for
this section was found.

### 2.5 Child-pages table [C+S]

`MarkSearchFlags` reads `childsize` bytes at `childstart`. The table is a sequence of
records:

```
u8  len               ; record length including this byte
u16 parentTOC
u16 childTOC[(len-3)/2]
```

During a search, a parent is marked as a hit if one of its children is a hit. In
WPINSDEV.HLP the records are (1→[2,3]), (8→[9,10]), and so on, and the parents (TOC 1, 8, 16,
22, 29) are exactly the TOC entries that carry ext-flag 0x800 (section 3.3).

### 2.6 Control data (`IOLoadCtrlData`) [C+S]

This section is used only when the version byte (0x06) is 2 or more and `ctrlsize != 0`.

```
u16 nCtrls
u16 nGroups
u16 unknown/default (read into ctrlinfo[0]; 0xFFFF or 0 in the samples)
u16 unused (0)
nCtrls × { u16 a; u16 b; u8 textlen; char text[textlen]; }   ; :ctrl definitions (field roles of a and b: [inf] style and ctrl res id)
nGroups × { u16 count; u16 ctrlIndex[count]; }               ; :ctrlarea groups
```

How the in-memory structures are built:

* The control array starts with 7 standard controls loaded from resources. These are
  indices 0–6 and match `CTRL_PREVIOUS..CTRL_FORWARD` in pmhelp.h. Controls from the file
  are appended at index 7 and up.
* Group 0 is built-in: the identity list `[0..3]` for a Help Manager instance, `[0..6]` for
  VIEW.
* Groups from the file become groups 1 through nGroups.

Examples:
* An 8-byte section of all zeros means no custom controls (the common case).
* OS2CHESS.HLP: `0000 0100 ffff 0000 | 0400 0000 0100 0400 0300`, which is 1 group of 4
  controls [0,1,4,3]. Its TOC entries carry ext-flag 0x40 with value 1, which selects that
  group.

---------------------------------------------------------------------------------------

## 3. Table of contents

### 3.1 TOC offset table [C+S]

At `tocstart` there are `ntoc` × u32 absolute offsets of the TOC entries.
`IOLoad1TOCElementOffset(i)` reads `tocstart + 4*i`, and `HMGetTOCItemInfo` seeks there and
reads one entry. The offsets match a sequential walk in all 279 files.

### 3.2 TOC entries (at `tocstrtablestart`, `tocstrlen` bytes, packed back to back) [C+S]

The bulk loader `IOLoadTOC` reads the area in chunks of at most 0x3FFF bytes and parses it
with `MscInitPartialTOC`. A single entry is loaded by `IOLoad1TOCElement`, which reads the
3-byte header and then `len-3` more bytes. Both paths call `MscFillTOCInfo`.

```
u8  len          ; total entry length including these 3 bytes (an entry is at most 255 bytes)
u8  flags
u8  nslots
[extended block, only if flags & 0x20]
u16 slot[nslots] ; indices into the COT (section 5); the panel text is these cells in order
char title[]     ; the rest of the entry, (len-3) - bytes consumed so far; not NUL-terminated
```

The title is copied only if the level is not 0 [C]. Level-0 entries have empty titles.

**Flags byte** [C+S]:

| Bits | Meaning |
|------|---------|
| 0x0F | Heading level (0–15). |
| 0x10 | Never set in the samples; ignored by the code. |
| 0x20 | Extended block present. |
| 0x40 | Hidden (not shown in the Contents view). |
| 0x80 | Has children (expandable). |

The code behaves as follows:

* If 0x80 is set, the internal state is "has children" and 0x40 is **ignored**.
* Otherwise, if 0x40 is set or the whole byte is 0, the entry is hidden (internal 0x800).
* A visible level-1 entry also gets internal flag 4 (top level).
* Level-0 entries (flags = 0) are the hidden, untitled entries (footnotes/popups). They are
  never marked by full-text search.

### 3.3 Extended block (when flags & 0x20) [C+S]

The block starts with a `u16 ext`. The optional fields follow **in this exact order**,
which is the order `MscFillTOCInfo` reads them; it was verified by clean titles across
the whole corpus.

| ext bit | Data that follows | Meaning |
|---------|-------------------|---------|
| 0x0001 | 5 bytes: `u8 posunits; u16 x; u16 y` | Window origin. The units byte goes to the high byte of the internal field +0x2B. Sample: `(0x11,25,20)`, `(0x44,1,4)`. |
| 0x0002 | 5 bytes: `u8 sizeunits; u16 cx; u16 cy` | Window size. The units byte goes to the low byte of +0x2B. Sample: `(0x11,70,50)`. |
| 0x0008 | `u16 style` | Window style bits (titlebar, scroll, rules and so on). Default if absent: 0x0738. `PnlMergeViewportStyle` combines it. Samples: 0x7328, 0x6708, 0x7000. |
| 0x0400 | `u16 group` | Viewport group (`group=`). `HMDisplayPanel` passes it to `PnlFindBestMatchViewportGroup`. Sample: EHXHP 'Help for File' has group 42. |
| 0x0040 | `u16 ctrlgroup` | Control-area group index (section 2.6). Default 0xFFFF. **[inf]** for the IPF name (ctrlrefid). The data layout is [C+S] in OS2CHESS. |
| 0x0004 | none | Viewport (internal +5 \|= 0x40). The panel opens in its own viewport [C], `viewport` attribute [inf]. |
| 0x0010 | none | Internal +6 \|= 0x02 (= 0x200). Excluded from the search hit count and from child-page propagation. This is `nosearch` [C, name inf]. |
| 0x0020 | none | Internal +6 \|= 0x01 (= 0x100). Probably `noprint` [inf]. |
| 0x0080 | none | Internal +6 \|= 0x10 (= 0x1000). Meaning unknown. Seen in combination 0x50B. |
| 0x0100 | none | Internal +5 \|= 0x80. `HMDisplayPanel` then calls `PnlClearAllViewport`, so this is `clear` [C]. |
| 0x0800 | none | Internal +5 \|= 0x20. The entry is a parent listed in the child-pages table (2.5) and is excluded from "search all" marking [C+S]. |
| 0x0200, 0x1000 and up | none | Not handled by the code. |

Bits observed in the samples: 0x0, 0x3, 0x4, 0xB, 0x10, 0x30, 0x3B, 0x40, 0x50, 0x400, 0x40B,
0x410, 0x50B, 0x808, 0xC0F.

---------------------------------------------------------------------------------------

## 4. Global dictionary (CLVT) [C+S]

At `dictstart` there are `ndict` records packed into `dictlen` bytes. Each record is `u8 len`
(including itself) followed by `len-1` characters, with no NUL. `IOLoadCLVT` reads the area in
chunks of at most 0x3FFF bytes, and `MscInitPartialCLVTTable` builds a table of pointers to
the records.

The words are **sorted**. `MscCLVTBinarySearch` binary-searches them using `MscCompareWord`
(NLS-aware, case-insensitive) and backs up to the first equal word.

A dictionary index (0..ndict-1) is the universal word id. It is used by:
* the local dictionaries in cells,
* the name and global-name tables,
* the search table (one record per word, in the same order).

## 5. Cells (COT, cell header, text, local dictionary) [C+S]

**COT**: at `slotsstart` there are `nslots` × u32 absolute cell offsets
(`IOLoadCOT` / `IOLoad1COTItem`).

**Cell header**: 8 bytes, read by `IOLoadCellHeader`.

| Off | Size | Meaning |
|-----|------|---------|
| 0 | 1 | Unused by the loader. Always 0 in the samples. |
| 1 | 4 | Absolute offset of the local dictionary. |
| 5 | 1 | `nlocal`: number of local-dictionary entries (at most `maxCVTIndex`). |
| 6 | 2 | `textlen`: number of text bytes. |

* **Text**: `textlen` bytes that immediately follow the header. `IOLoadCDI` reads them with
  offset −1, meaning "continue from the current position".
* **Local dictionary (CVT)**: `nlocal` × u16 global dictionary indices at the offset given by
  header+1 (`IOLoadCVT`, size `nlocal*2`). It is sorted ascending; `CVTBinSearch` relies on
  that, and all 1959 cells checked are sorted. In every sample the CVT sits right after the
  text bytes.

**Text byte codes**: these come from `FormatLine` and `PnlCELength` in HELPMGR, plus
`GetChildPages`.

| Byte | Meaning |
|------|---------|
| 0x00–0xF9 | Word: `dict[cvt[b]]`. When spacing is on, the word is followed by a blank. |
| 0xFA | End of paragraph (spacing resets to on). |
| 0xFB | Centre. |
| 0xFC | Toggle spacing mode (starts on). |
| 0xFD | Line break. |
| 0xFE | Explicit space. |
| 0xFF | Escape: `FF len code data…`. `len` counts itself, `code` and the data, so the next item is at `p + 1 + len`. |

Escape codes:

* 0x05: link. `u16` TOC index at escape+3 (local to the database). Used by `GetChildPages`.
* 0x0E: bitmap (section 11).
* 0x02 and 0x12: margin codes, which count as a positioning element in `FormatLine`.

All other escape codes are outside the scope of this document.

## 6. Resource-id table (HM_RESOURCEID → TOC) [C+S]

At `resstart`: `nres` × u16 resource ids, **sorted ascending**, followed by `nres` × u16
TOC indices.

* `IOLoadHelpPanelNumTable` reads the id half.
* `MscFindMatchNum` runs the CRT `bsearch` over it.
* `IOLoadHelpPanelNumTOCEntry(i)` reads `resstart + 2*nres + 2*i`.

The reverse lookup (`HMGetNumFromTOCIndex`) is a linear scan.

Samples: EHXHP has (1091→12), (4144→0), …; CMDREF has (1→5).

## 7. Name table (HM_PANELNAME → TOC) [C+S]

At `namestart`: `nname` × u16 **dictionary indices** (sorted ascending), followed by
`nname` × u16 TOC indices. Panel names are stored as dictionary words, for example
`A4144`, `4624`, `7230`.

`HMFindExtNameFromIOInfo` resolves a name as follows:
1. Upper-case a–z.
2. `MscCLVTBinarySearch` for the word.
3. Step forward over equal words until `MscCompareExactWord` matches.
4. `bsearch` the u16 word id in the name table.
5. Read the TOC index from the second half.

## 8. Index (`IOLoadIndex` / `MscInitPartialIndex`) [C+S]

At `indexstart` (or `globindexstart`) the entries are packed into `indexlen` bytes. The area
is read in chunks of at most 0x3FFF bytes.

```
u8  len          ; length of the text field only
u8  flags
u8  nsyn         ; number of synonym references
u16 toc          ; TOC index (local; the loader adds the database's TOC base)
u8  text[len]    ; no NUL
u32 synoff[nsyn] ; absolute offsets of synonym records
```

Entry size = 5 + len + 4·nsyn.

**Flags**:

| Bit | Meaning | Ev. |
|-----|---------|-----|
| 0x01 | Level 1 (`:i1`) | S (0x01, 0x41, 0x81) |
| 0x02 | Level 2 (`:i2`). Internally prefixed with three spaces (a string in HELPMGR's data object at +0x370). A level-2 entry belongs to the preceding level-1 entry and travels with it when indexes are merged (`IOMergeIndex`). | C+S |
| 0x40 | Global entry (`global`). In master-index mode (`inst+0x22 & 0x8000`) only these entries are loaded, and the count comes from `ext+0x24`. | C+S |
| 0x80 | Has sort key: `text[0]` is the sort-key length k, `text[1..k]` is the sort key, and `text[k+1..]` is the displayed text. Merging and sorting use the sort key; without one they use the text. | C+S (alpref.inf, for example `\x03086` + `.8086 assembler directive`) |

**Synonym record** (`IOLoad1SynRoot` / `InxSearch1SynRoot`), at `synoff`: `u16 size`, then
`size` bytes made of strings that are each `u8 len` (including itself) plus characters, ended
by a 0 byte. An index search for X also matches an entry if X equals one of its synonyms.
**C only**: none of the 279 files contain synonyms.

## 9. Global index table

Header fields 0x2C, 0x2E and 0x32 use the same entry format as section 8 (code path
`GetIndexEntries`/`IOLoadIndex` with type != 2). They are zero in every sample [C].

The used mechanism for global index entries is the 0x40 flag inside the normal index,
together with `ext.nGIndex` [C+S].

## 10. NLS / country section [C+S]

At `nlsstart` (`nlslen` bytes, `IOLoadCountryInfo` / `MscFillCountry`) the section is a
sequence of records, each starting `u16 len; u8 type; u8 format`. The first record
(len 0x0C, type 0) holds:

| Off | Size | Meaning |
|-----|------|---------|
| +4 | 2 | Value |
| +6 | 2 | Country code |
| +8 | 2 | Codepage. 874 (0x36A) sets the Thai flag. |
| +10 | 2 | Bidi attribute. Read only if `nlslen >= 0x54` and a bidi system is present. |

Country and codepage are stored in the database (`db+0x3A` and `db+0x3E`) and passed to
`WinCompareStrings` when the indexes are merged.

The remaining records (types 1 and 2, 0x24 bytes each in the 0x54-byte form) are character
class bitmaps **[S, meaning inf]**.

Sample:
```
0c00 0000 0000 0100 b501 0000 | 2400 0100 … | 2400 0200 …
```
This decodes to country 1, codepage 437. Lengths seen: 0x54 (227 files), 0x52, 0x12,
0x2E and 0x0A.

## 11. Bitmap area [C+S]

Bitmaps are addressed by escape 0x0E:

```
FF 07 0E flags u32 offset
```

* `flags` holds the ART_* bits from pmhelp.h: 0x01 left, 0x02 right, 0x04 centre, 0x08 fit,
  0x10 runin. All samples use 0x11.
* The bitmap record is at `imgstart + offset` (`ArtFormatBitmap` → `LoadBitmap`).

The record (`LoadBitmapHeader` / `LoadBitmap`) has this layout:

```
14-byte BITMAPFILEHEADER: u16 type, u32 cbSize, i16 xHot, i16 yHot, u32 offBits
   type: 'bM' (compressed), 'BM' (plain), 'mF' (metafile, 0x12-byte header);
   'BA' arrays are walked to pick the entry that matches the device
BITMAPINFOHEADER: cbFix 12 (1.x: u16 cx, cy, planes, bitcount) with a 3·2^bpp palette,
                  or cbFix 0x40 (2.0) with a 4·2^bpp palette
u32 dataSize      ; = 2 + Σ(2 + chunkLen)
u16 blockSize     ; bytes per uncompressed block ('BM': in rows, multiplied by the stride)
repeat: u16 chunkLen (0 = uncompressed block); chunk bytes
```

* Chunks are decompressed by `DeCompressAlg02`, `ArtDecompressAlg01` or `DeCompressAlg00`.
  The first byte of each chunk is 2 in the samples, which looks like an algorithm tag.
* Verified on REXX.INF: 91×35 4bpp gives dataSize 352 = 2 + 2 + 348, blockSize 1680 = raw
  size. The first record sits exactly at `imgstart`.
* The compression algorithms themselves are not specified here.

## 12. Full-text search table (`MarkPages`) [C+S]

At `searchstart & 0x7FFFFFFF` (`searchlen` bytes) there is **one record per dictionary word,
in dictionary order**. The record count equals `ndict` in every file checked.

```
size  : u8, or u16 if searchstart bit 31 is set ; includes itself
u8    type
data  : size - (1 or 2) - 1 bytes
```

| type | Data |
|------|------|
| 0 | Word not in any panel. |
| 1 | Word in every panel. |
| 2 | RLE bitmap (`ArtDecompressAlg01`) over TOC entries, MSB-first. |
| 3 | List of u16 TOC indices. |
| 4 | Every panel except the u16 TOC indices listed. |
| 5 | Raw bitmap, MSB-first, starting at TOC 0. |
| 6 | `u16 startByte` then a bitmap whose first bit is TOC `startByte*8`. |

Checked against the actual words of each panel's cells:
* EHXHP: 695/695 records match.
* wps2.inf: 3510/3510 match (16-bit sizes).
* REXX and CMDREF: the only differences are on level-0 (hidden footnote) entries, which the
  compiler leaves out.

Search proceeds as follows:
1. The query words are resolved via `CLVT*Search` to dictionary ids.
2. `MarkPages` ORs one bit per query word into a per-TOC word array.
3. Phrase and sentence search then re-checks cells via the CVT (`BookWordSearch`,
   `CVTBinSearch`).
4. Results are propagated through the child-pages table.

## 13. How Help Manager panel lookups work

A help instance holds a chain of databases: the main file, then the `ext` database files
(linked via `db+0x48`). Each database has a TOC base `db+0x24`, which is the sum of the
`ntoc` values of the files before it. A global TOC index is local index + base, and
`MscGetLocalTOCFromGlTOC` maps it back.

**`HMDisplayPanel(id, type)`**:

* `HM_RESOURCEID` (0): `HMGetTOCIndexFromNumber`. For each database in the chain it bsearches
  the resource-id table (section 6) and returns tocIdx + base.
* `HM_PANELNAME` (1): `HMGetTOCIndexFromName(name, 0)`, which uses the local name table
  (section 7) in each database. If that fails and the message is HM_DISPLAY_HELP (0x222), it
  retries with flag 0x10, which uses the global-name table (section 2.3).
* Flag 4: the id is a TOC index relative to the current database.
* Flags 0x10/0x20: name lookup against a specific database whose filename ends with the
  given string.

After the lookup, `HMGetTOCItemInfo` turns the TOC index into a file offset via the offset
table (3.1) and reads one TOC entry. Its slots are then loaded through the COT into cells
(`PnlLoadCellList`).

The application-side HELPTABLE/HELPSUBTABLE (`HOOKSearchHelpTable`, resource types 18/19)
maps (window id, control id) to a resource id. That resource id is then looked up as
HM_RESOURCEID. The HELPTABLE is not part of the file.

---------------------------------------------------------------------------------------

## 14. Validation output (excerpt)


```
EHXHP.HLP header: flags 0x10, ver 2.2, ntoc 114, nres 114, nname 80, nindex 51, nslots 114, ndict 695, 'System Editor Help'
EHXHP.HLP ext: nfont 2 @5275, ndb 1 @5253 (22 bytes), ctrl @5353 (8 bytes), no gnames/strings/child pages
  TOC   0 lvl1 fl=21 ext=400 slots=[0] 'Help for the System Editor'
  TOC   1 lvl1 fl=61 ext=40b slots=[1] 'OS/2'
  TOC   2 lvl1 fl=61 ext=010 slots=[2] 'Help for File'
  TOC   7 lvl1 fl=61 ext=410 slots=[7] 'Help for Type of File'
  TOC  12 lvl1 fl=41 ext=000 slots=[12] 'Help for Cancel'
  … (all 114 decoded) …
  TOC 112 lvl1 fl=61 ext=400 slots=[112] 'Help for Copyright'
  TOC 113 lvl1 fl=41 ext=000 slots=[113] 'Key Assignments for the System Editor'
res table (first 8): [(1091,12),(4144,0),(4160,4),(4163,5),(4176,3),(4192,13),(4208,14),(4210,15)]  sorted: True
name table (first 8): [('4624',108),('7230',54),('A4144',0),('A4160',4),('A4176',3),('A4192',13),('A4208',14),('A4224',23)]
index:  fl=01 toc=0 'About the System Editor' | fl=01 toc=81 'Adding and inserting text' | fl=01 toc=93 'Canceling a previous action' …
cell 0: off 9842, b0 0, ldictoff 10061 (= 9842+8+211), nlocal 80, textlen 211, ldict sorted True
'\n\nUse the <esc 05 010000046e00>OS/2* <esc 08 >System Editor as a text editor to create and edit your
 CONFIG.SYS and AUTOEXEC.BAT system files.  You can also use the System Editor as a convenient tool to
 display text files or perform basic editing tasks.  The System Editor runs in a window. …'
CMDREF first 20 TOC:
    0 lvl1 fl=41 'Journaled File System (JFS)'      1 lvl1 fl=41 'High Performance File System (HPFS)'
    2 lvl1 fl=41 'File Allocation Table (FAT)'      3 lvl1 fl=01 'Introduction'
    4 lvl1 fl=01 'How to Use This Book'             5 lvl1 fl=61 'Example of Linked Information'
    6 lvl1 fl=01 'Part 1 - Information about OS/2 Commands'
    7 lvl1 fl=81 'Introduction to OS/2 Commands'    8 lvl2 fl=02 'File and Directory Concepts'
    9 lvl2 fl=02 'How to Read a Syntax Diagram'    10 lvl2 fl=02 'Using the Command Prompt'
   11 lvl1 fl=81 'OS/2 Commands by Task Category'  12 lvl2 fl=02 'File and Directory Management'
   13 lvl2 fl=02 'Program Management'             14 lvl2 fl=02 'Disk and Diskette Management'
   15 lvl2 fl=02 'Problem Determination'          16 lvl2 fl=02 'System Customization and Control'
   17 lvl2 fl=02 'Batch File and Command Processing'  18 lvl2 fl=02 'System Configuration Commands'
   19 lvl1 fl=81 'OS/2 Commands by Name'
CMDREF index first 10: (01,682,'/? command') (01,328,'absolute priority') (01,486,'access permission')
   (02,601,'assigning') (02,486,'for Local Security') (02,670,'for process after logoff') …
CMDREF res table [(1, 5)]
cells (EHXHP+CMDREF, 1959): b0=0 always, CVT sorted always, CVT always right after text; max nlocal 245
CMDREF cell of TOC 4: '\n\nThis section describes how to use an online book.  Learn how to: <esc 02 03>\n\n\x07<esc 02 06>View the contents of a book …'
search table: EHXHP 695 recs = ndict, types {3:383,5:123,6:107,0:80,1:1,4:1}, 695/695 match cell words;
              wps2.inf (16-bit sizes) 3510/3510 match
```

### Notes for the Rust implementation

* The loaders read regions in chunks of at most 0x3FFF bytes, and the 0x7FFF masks on
  counts are IBM implementation limits. A Rust reader can read whole sections.
* Bounds to enforce:
  * An index entry must fit in its section, with 5 + len + 4·nsyn bytes available.
  * Every TOC entry needs at least 3 bytes.
  * A cell byte below 0xFA must be less than `nlocal`.
  * Every `cvt` value must be less than `ndict`.
* Every TOC/slot/word index taken from file data must be range-checked. IBM checks only some
  of them (the TOC offset index against `ntoc`, table indices against their counts).
