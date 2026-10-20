# INF/HLP bitmaps: storage, decompression, layout (HELPMGR.DLL)

Sources: Ghidra decompiles of HELPMGR.DLL (`tools/ghidra/lxdecomp.sh`) plus a
decoder written for the purpose (the Rust one is `crates/ipf`'s `bitmap`) run over
**every** `*.INF`/`*.HLP` under `work/`
(654 files, 1137 bitmaps, 2905 LZW blocks, 3 stored blocks, 1 RLE block, 0 errors).
The decoded PNGs were checked by eye.

Tags: **[code]** = read in the decompile, **[file]** = checked against real files,
**[code+file]** = both. **[code only]** = no sample file uses it.

---

## 1. Where bitmaps live and how text refers to them

* INF/HLP main header `u32 imgstart` at **0x4E** [code+file]. ArtFormatBitmap calls
  `LoadBitmap(file_offset = imgstart + escape.offset)`, so offsets are relative to imgstart.
  When a file has images, `imgstart` is the first image. It is often 0x9B (=155), right
  after the header.
* Images are packed back to back from `imgstart` with no padding or directory [file]. A walker
  that steps by the stored sizes (§2) lands exactly on the next `bM`/`BA` every time.
* Cell-text escapes that place a bitmap (escape = `FF len type …`, total length `1+len`) [code+file]:
  * **`FF 07 0E flags:u8 offset:u32`**: ordinary `:artwork.`. FormatCell `case 0xe` passes
    `esc+3` as `param_6` (`param_6[0]` = flags, `*(u32*)(param_6+1)` = offset).
  * **`FF 08 0F 00 flags:u8 offset:u32`**: artwork that carries hypergraphic link areas (an art-link
    bitmap), followed by `0F 07`/`0F 0B`/`0F 04`… link escapes. FormatCell `case 0xf`, subcase
    `esc[3]==0`, passes `esc+4`. Seen in CMDREF.INF, MMREF2/3.INF.
  * PnlCalcCDISize counts both (`type==0x0E` or `type==0x0F && sub==0`) as graphic items.
* Observed flag values [file]: `0x01` (most common), `0x03`, `0x05`, `0x11`. Bit 0 is always
  set. Meanings are in §5.

## 2. Stored image layout

All multi-byte values are little-endian. One image record at `imgstart + offset`:

```
[optional BITMAPARRAYFILEHEADER, 14 bytes]   only if first 2 bytes are 'BA'
BITMAPFILEHEADER (14 bytes)
  +0  u16 usType      'bM' (0x4D62) compressed help bitmap  ← every sample file
                      'BM' (0x4D42) legacy old-style        [code only]
                      'mF' (0x466D) metafile                [code only]
  +2  u32 cbSize      = size of the original .BMP (or 0x1A inside BA). NOT USED
  +6  i16 xHotspot    (copied from source icon/pointer). NOT USED
  +8  i16 yHotspot    NOT USED
  +10 u32 offBits     original offBits. NOT USED
BITMAPINFOHEADER   (cbFix u32 at +14)
  cbFix == 12 (OS/2 1.x):  u16 cx, u16 cy, u16 cPlanes, u16 cBitCount   ← all 1137 samples
  cbFix == 0x40 (2.x):     u32 cx, u32 cy, u16 cPlanes, u16 cBitCount, … (64 bytes) [code only]
palette:  1.x: (1<<cBitCount) × RGB  {B,G,R}        (3 bytes each)
          2.x: (1<<cBitCount) × RGB2 {B,G,R,0}      (4 bytes each)
u32 cbData          compressed size of everything after this field
-- 'bM' (and 'BM' when the image is >0x7FFF bytes) --
u16 cbBlock         uncompressed bytes per block (decode buffer size)
repeat until all image bytes are produced:
    u16 cbComp
    u8  data[cbComp]      (if cbComp==0, see below)
```

Header byte count, exactly as LoadBitmapHeader computes it (USHORT arithmetic) [code]:
* 1.x: `hdr = 0x1A + 3*(1<<bpp)` (from the BITMAPFILEHEADER start)
* 2.x: `hdr = 0x4E + 4*(1<<bpp)`
* The result is kept in a **USHORT**. For **24 bpp**, `3<<24` truncates to 0, so `hdr = 0x1A` and
  there is no palette. That is correct for 24-bit images [code+file: sg244640.inf has two 24-bpp
  images, which decode correctly]. Implement it as "palette entries = 0 when bpp ≥ 16 (or 24)".

Pixel geometry [code+file]:
* `rowBytes = cPlanes * (((cBitCount*cx + 31) >> 5) * 4)` (DWORD-aligned rows)
* `total = rowBytes * cy`
* Rows are **bottom-up**, as in GpiSetBitmapBits. Pixels are packed MSB-first within each byte
  (1/4/8 bpp). 24 bpp is stored as B,G,R.
* Observed [file]: always `cPlanes=1`; bpp 1 (108 images), 4 (541), 8 (486), 24 (2).

`cbData` [file]: for `bM` it is always `2 + Σ(2 + cbComp_i)`, i.e. it covers the cbBlock word and all
block headers (verified for all 1137 images). HELPMGR reads it but only uses it in the
small-legacy-`BM` path. Use it to skip to the next image.

### 2.1 'bM' block loop (LoadBitmap) [code+file]

```
rem = total; scan = 0
rowsPerBlockStep = cbBlock / rowBytes          (local_54)
loop while rem != 0:
    chunk = min(cbBlock, rem); rem -= chunk
    rows  = chunk / rowBytes                   (local_52)
    read u16 cbComp
    if cbComp == 0:  read rows*rowBytes raw bytes        [code only; never seen]
    else:            read cbComp bytes, decode by data[0] (§3)
    GpiSetBitmapBits(start scan = scan, rows, buffer)    (first block creates the bitmap via GetBitmap)
    scan += rowsPerBlockStep
```
* cbBlock is always a multiple of rowBytes in the samples (1137/1137) [file]. Encoders use
  ≤ 64 KB (for example 64440, 64824, 64960). Small images have one block with `cbBlock == total`.
* Each block is self-contained. The decoder state (and the LZW dictionary) restarts for every block.
* HELPMGR's buffers: compressed `cbBlock+1`, output `cbBlock`, LZW table `0x4000` bytes.
  An implementation can just size these per block.
* The type byte decides the algorithm. HELPMGR tries `DeCompressAlg02`, then `ArtDecompressAlg01`,
  then `DeCompressAlg00`. Each returns 0 unless `data[0]` is 2, 1 or 0 respectively. Any other value
  means failure.

### 2.2 'BA' bitmap arrays (LoadBitmapHeader) [code+file]

```
BITMAPARRAYFILEHEADER: u16 'BA', u32 cbSize, u32 offNext, u16 cxDisplay, u16 cyDisplay
then a full image record as above (BITMAPFILEHEADER at +14)
```
* Array entries are **packed one after another**. The next entry starts at
  `entry + 14 + 14 + hdr' + 4 + cbData`, where `hdr' - 14` is the header computed above
  (code: `0x28 + 3*(1<<bpp)` for 1.x, `0x5C + 4*(1<<bpp)` for 2.x, measured from the BA start).
  Then the u32 cbData follows. `offNext` is **not** used as a pointer, only tested for 0
  (0 = last entry).
* Selection: walk the entries. Take the first one whose `cxDisplay == inst+0xA6` **or**
  `cyDisplay == inst+0xA8` (instance fields, presumably the display size). If the last
  entry (offNext==0) is reached without a match, use the **first** entry.
* Samples [file]: MAHJONGG.HLP and laserjet.hlp have single-entry arrays (offNext=0, 0×0).
  PSNS.INF has 2-entry arrays (entry 1: 0×0, offNext=0x58; entry 2: 1024×768, offNext=0).
  A walker over the image area must step through all array entries.

### 2.3 Legacy 'BM' (old-format) [code only, none in 654 files]

* If `total <= 0x7FFF`: there is no cbBlock word. Read `cbData` bytes. If `cbData == total` they are raw.
  Otherwise decompress the whole thing with DeCompressBitmap (if `planes*bpp < 2`) or
  DeCompressBitmap2.
* Otherwise: `u16 n` = **rows** per block (bytes per block = `n*rowBytes`). Then use the same
  block loop with `cbComp`/data, but decode with DeCompressBitmap / DeCompressBitmap2 instead of
  the type-byte algorithms. There is no type byte.
* **DeCompressBitmap** (1 bpp): every input byte `b` adds `b & 0x7F` bits of value `b >> 7`
  to an MSB-first bit stream. Output bytes start at zero. The input length is the byte count.
* **DeCompressBitmap2**: `(count:u8, value:u8)` pairs. Each writes `value` `count` times.
  The pair count is `len/2`.

### 2.4 'mF' metafile [code only, none in 654 files]

* The header is 0x12 bytes from `mF`: `+4 u32` uncompressed metafile length, `+8..+0x11` = a
  5-word placement record: `+8 x, +0xA y` (forced to 0), `+0xC cx, +0xE cy`,
  `+0x10` units (low byte; HELPMGR copies it into both bytes).
* Then `u32 cbData`, `u16 cbBlock`, and type-byte blocks (alg 0/1/2) exactly as in 'bM'. Each
  decoded block is appended into a metafile DC with PMGPI ordinal 536 (GpiSetMetaFileBits-like)
  at a running offset.
* If cx or cy is 0, it uses `cx = CXSCREEN/2`, `cy = CYSCREEN/2`, units=0x22. PnlConvertUnit converts
  the record. cx/cy become the item's size, then MtaPlayMetaFile renders it into a bitmap.

### 2.5 2.x header caveat [code]
LoadBitmap computes rowBytes for `cbFix==0x40` with `cBitCount` read from **+0x18 (the high word
of cy)** instead of +0x1C. This is an IBM bug that no data triggers. Use the correct 2.x fields.

---

## 3. Block compression algorithms (`data[0]` = type, `n = cbComp`)

### 3.0 Type 0: stored (DeCompressAlg00) [code+file: 3 blocks in gpi2.inf]
Output = `data[1 .. n-1]` (n-1 bytes).

### 3.1 Type 1: RLE (ArtDecompressAlg01) [code+file: 1 block in a3a4hm02.inf]
`rem = n-1`, `p = 1`. Loop until `rem == 0`:
```
c = data[p]; v = data[p+1]
if c < 0x80:          emit v, (c+1) times                         p+=2; rem-=2
elif c == 0x80:
    if v in {0,1}:    emit (v ? 0xFF : 0x00), (u16le(data[p+2..p+3]) + 1) times
    else:             emit pair data[p+2],data[p+3], (v+1) times  (2*(v+1) bytes)
                                                                  p+=4; rem-=4
else (0x81..0xFF):    copy (c&0x7F)+1 literal bytes data[p+1 ..]  p+=1+(c&0x7F)+1; rem likewise
```
* **The fill count in the `80 00|01 lo hi` form is a 16-bit word.** Ghidra shows only the low byte,
  but the only sample block (`01 | 00 60 | 80 8a 50 2c | 80 00 17 01 | 00 06 | 80 8b 60 3a`) gives the
  required 840 bytes = 3 rows×280 only when the count is u16 0x0117+1 = 280 [file]. The decoded
  image looks right. Treat the HELPMGR behaviour as a 16-bit count.
* The literal case `c==0x80` is never a literal (it is the escape). A literal run is 2..128 bytes.
* Returns the output length. HELPMGR does not bounds-check, so clamp to the block.

### 3.2 Type 2: LZW (DeCompressAlg02) [code+file: 2905 blocks]
Standard variable-width LZW. The details:
* **Bit order: MSB-first**. A 32-bit accumulator is refilled a byte at a time while
  `nbits < 25`: `acc |= byte << (24 - nbits)`. `code = acc >> (32 - width)`. Then
  `acc <<= width; nbits -= width`.
  The refill reads up to 3 bytes **past the end of the block** (HELPMGR's buffer is `+1` and
  reads garbage). Pad with zeros. Any bits past the END code are ignored.
* Codes: `0..255` literal bytes, **256 = CLEAR**, **257 = END**. First free code **258**.
* Width starts at **9** and can grow to **12**. It grows by one ("early change", TIFF-style) when,
  **after** adding an entry, `next_code == (1<<width) - 1`, while width < 12. So 9→10 when next_code
  reaches 511, 10→11 at 1023, 11→12 at 2047. At 12 bits it never grows. There is **no** automatic
  reset and **no** bounds check. Encoders send CLEAR first; the highest next_code seen in all files is 4091.
* Decoder state: `first = true`, `prev`, `firstch` (first byte of the previous string).
  ```
  loop:
    code = read(width)
    if code == 257: stop                  (END is checked before the bits are consumed; harmless)
    if first:   emit byte(code); prev=code; firstch=code; first=false; continue   (no table add)
    if code == 256: first=true; width=9; next=258; continue
    if code < 256:  s = [code]
    elif code < next: s = string(code)
    else (code >= next, KwKwK): s = string(prev) + firstch_of(prev string)
    emit s; firstch = s[0]
    add entry next = string(prev) + s[0]; next++
    if next == (1<<width)-1 and width < 12: width++
  ```
  The stream starts directly with a code. A leading CLEAR is allowed but not needed (state starts as after CLEAR).
* HELPMGR stores the dictionary as two `u16[4096]` tables (offset into the current block's output,
  length) instead of strings: an entry points at the previous string's position in the output with
  `len[prev]+1`. Literals also record `(pos,1)`. Use this design in Rust: no allocation, and
  copies come from earlier output (the source and destination never overlap because the source ends where the destination starts).
* Returns the number of output bytes. It should equal `rows*rowBytes` (true for every block in the files).

---

## 4. What happens after decoding (GetBitmap) [code]
* The first block creates a memory DC/PS of size (cx, cy) and calls `GpiCreateBitmap(hps, &bih, 0…)`
  with the **file's** BITMAPINFOHEADER and palette (header pointer +14). Then
  `GpiSetBitmapBits(start=0, rows, bits, &bih)`. Later blocks call `GpiSetBitmapBits(start=scan, rows…)`.
* Item record (`HpBitmap`): `[0]=hbm, [1]=hps, [2]=hdc, +0xC cx, +0xE cy, +0x14 = 0`.
  ScalePrinterBitmap rescales cx/cy for printing.

## 5. Layout: ArtFormatBitmap [code]

Inputs: `lineH` (`inst+0x90`, or the printer's), `extra` (`inst+0x94`), left margin `L` (`win+8`),
right margin `R` (`win+10`), pen `(px, py)`, `space` = word-space width (FormatCell `local_e0`),
`bidi` = `inst+0x162 ∈ {1,2}` (Arabic/Hebrew RTL). `w,h` = bitmap cx, cy. `hh = h + extra`.
Output: item `x` (+8), `y` (+10). `y` grows down the page in line units.

Flags byte:
| bit | meaning (as coded) |
|---|---|
| 0x01 | align left. Only tested in bidi mode (it overrides the right default there). IPFC always sets it |
| 0x02 | align right (LTR) |
| 0x04 | center |
| 0x08 | fit to window: x = y = 0xFFFF (scaled at draw time). It does not widen the cell's max width |
| 0x10 | run-in (inline with text) |

Not run-in. `NEXT = py + (hh/lineH + 1)*lineH` (integer division):
* LTR: if 0x02 → `x = R - w`. elif 0x04 → centre. elif 0x08 → fit. else `x = L`.
* bidi: if 0x01 → `x = L`. elif 0x04 → centre. elif 0x08 → fit. else `x = R - w`.
* centre: `x = (R - L - w) >> 1` if `R-L > w`, else `x = L`. **The code does not add L** (decompile is exact).
* Unless fit: `y = NEXT`. After that the pen is `(L, y + lineH)`.

Run-in (0x10):
* If `w + px < R` (it fits on the current line):
  `x = px` (LTR) or `x = R - px - w` (bidi).
  Look back over earlier items on this line (the item array from `cell+0x26` up to this one) to find
  how much height is already there (`dy`). It is 0 if the previous item's y < py, else `py - y` of the
  first earlier item whose y ≠ py.
  If `dy < hh`, then `y = py + lineH*(hh/lineH)` and every earlier item whose `y == py` is moved to
  this new y (so the line's baseline drops). Otherwise `y = py`.
  The pen becomes `px += w + space`.
* Otherwise (wrap): `x = L`, `y = py + lineH*(hh/lineH + 1)`, `px = L + w + space`.
* Afterwards `py = y`.
Finally, unless fit: `cell.maxRight (+0x2E) = max(cell.maxRight, x + w)`.

The bitmap is only loaded the first time (when `cell+0x1C != 0` and the item's `0x01000000` flag is
clear). On failure the cell's bitmap pointer (+0x1E) is zeroed. The error codes are 0x2004 (bad header
or read), 0x2007 (out of memory or GPI failure), 0x2008.
