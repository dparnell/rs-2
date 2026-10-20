# INF/HLP cell text: encoding and how HELPMGR formats it

Sources: Ghidra decompiles of IBM HELPMGR.DLL (Warp 4.52, `work/unpacked/disk_6/OS2/DLL/HELPMGR.DLL`):
FormatPage, FormatCell, FormatLine, PnlCELength, PnlCalcCDISize, PnlGetExtLnkData, MscFormatWord,
MscInitFont, MscInitFontStyle, MscSetFontStyle, MscInitSystemChar, MscIsHideCDI, MscSkipHideCDI,
MscGetACVPInfo, ArtFormatBitmap, DisplayPage, IsHyper, HPGetHPTWordsWidth, ProcessPageButton1Down,
IOLoadDocHeader, IOLoadFontTable. The data tables FColorAra/BColorAra were read from object 4.

Validation used EHXHP.HLP, CMDREF.INF, REXX.INF and pm1.inf. A wider survey covered all 306
INF/HLP files under `work/`, with a decoder and a character-cell renderer written for the
purpose (`crates/ipf`'s `text` and `layout` follow them and are tested the same way).

Markers: **[code]** = confirmed in the decompile, **[files]** = confirmed in the sample files,
**[both]** = confirmed in both. *(inferred)* means a naming or meaning that the code does not
prove.

> **Tooling note:** IBM's DLLs use *chained* internal fixups (LX fixup records with
> target-flags bit 0x08: at each source the raw dword is `next_src_offset<<20 | target_offset`,
> 0xFFF ends the chain; in HELPMGR only 114 fixups are plain and 2883 sources are reached through
> chains). `crates/os2-exe` (`fixup_chain`), the kernel's loader and `lximage` apply them;
> without them every absolute pointer and switch table of a decompile is wrong.

---

## 1. File-level structures used by the formatter

| Item | Layout | Status |
|---|---|---|
| Header magic | `"HSP"` + byte 3 (0x01 for INF, 0x10 for HLP). IOLoadDocHeader accepts dword `0x10505348`, or `0x01505348` when the instance allows INF. Reads 0x9B header bytes. | [both] |
| Header byte 6 | Version major. Rejected if >2. All samples have 2 (byte 7 = 2). **This byte changes margin arithmetic (see §4).** | [both] |
| `u16 @0x08` | ntoc; `u32 @0x12` tocstart | [files] (task givens; consistent) |
| `u16 @0x3E` nslots, `u32 @0x40` slot table (u32 file offsets of cells) | | [both] (`IOLoadDocHeader` requires ntoc≠0 and nslots≠0) |
| `u16 @0x48` ndict, `u32 @0x4A` dict start | Dictionary = ndict entries, each `len, bytes[len-1]` (the length byte counts itself). | [files] (306 files parse cleanly, words decode to sensible text) |
| `u32 @0x4E` image base | Bitmap offsets in 0x0E / 0x0F-00 escapes are relative to this. ArtFormatBitmap calls `LoadBitmap(off + hdr[0x4E])`. | [code] |
| `u8 @0x52` | **Local-word limit** (0xF5 in every file seen). FormatCell loads `local_11a = hdr[0x52]`, and a text byte `< hdr[0x52]` is a word reference. | [both] |
| `u32 @0x5B` | Extended-header offset (0 = none). Ext header: `u16 nfonts, u32 fonttable_off, …`. | [both] |
| Font table | nfonts × 0x27 bytes: `char face[33]; u16 height @0x21; u16 width @0x23; u16 codepage @0x25`. With no ext header, a 2-entry default is built: "System Proportional" plus a monospace face from string resource 0x1784. EHXHP/REXX/pm1 have `0: System Proportional (0,0)`, `1: Courier 18×11`. CMDREF adds `2: System Monospaced 15×12` and `3: Courier 12×10`. | [both] |
| Cell | `u8 0; u32 localdict_off; u8 nlocal; u16 ntext; u8 text[ntext]`; local dict = `nlocal × u16` global word indexes. Byte 0 was 0 in every cell of the 4 samples, and every local index was < ndict. | [files] (FormatCell receives the decoded cell: `+6 u16 text length`, `+8 local dict ptr`, `+0xC text ptr`) |

---

## 2. Text byte values

The formatter walks `ntext` bytes. `L = hdr[0x52]` (0xF5).

| Byte | Meaning | Status |
|---|---|---|
| `0x00 … L-1` | Word = `dict[localdict[b]]`. Placed as one unit; see §3 for spacing. | [both] |
| `L … 0xF9` (0xF5–0xF9) | Not a word. Falls through the switch and is ignored (1 byte). Never emitted, apart from a single stray 0xF5 and 0xF9 in the 306-file survey. | [code] |
| `0xFA` | End of paragraph (§3.3). | [both] |
| `0xFB` | Centre the following text up to the next FA/FD (§3.4). | [both] |
| `0xFC` | Toggle autospacing. | [both] |
| `0xFD` | Line break. | [both] |
| `0xFE` | Explicit space. | [both] |
| `0xFF` | Escape: `FF len type data[len-2]`. **`len` counts itself and the type byte but not the FF**, so the whole escape is `len+1` bytes and the next byte is at `p + len + 1`. FormatCell does `p += p[1]` and then the common `p++`. PnlCalcCDISize and MscSkipHideCDI do the same. | [both] (all 306 files tile exactly) |

Bytes that are local-dictionary indexes but `>= nlocal`: 7 occurrences in 306 files. These are
garbage and IBM would index past the local dictionary; treat them as ignorable.

---

## 3. Layout engine: state, units and spacing

### 3.1 Units and per-page initial state [code]

Everything is in device pixels of the presentation space. The **margin unit `U`** is
`inst+0x92`: the default (system) font's character width. MscInitSystemChar also copies it into
the current space width. The line height `LH` is `inst+0x96`. The space width `SW` is `inst+0x98`.
Both are reset at page start to the system font values (`+0x90`, `+0x92`, `+0x94` = height,
width, descent). MscInitFont reloads them from per-font metrics (`entry+4` height, `+6` space
width, `+8` descent).

FormatPage state (`param_2` of FormatCell). The fields that matter:

| Field | Init | Meaning |
|---|---|---|
| `x` (+0) | `left` | current pen x |
| `y` (+4) | `sysLH − sysDescent` | **baseline** of the current line, measured **downward from page top** |
| `left` (+8) | `U` if version ≤1, `2U` if version >1 | left margin |
| `right` (+0xA) | `winWidth − U` | right edge for wrapping |
| `mono` (+0xC) | 0 | inside `:xmp.` |
| `spacing` (+0x10) | **reset to 1 at the start of every cell** | autospacing on/off |
| `wrap` (+0x12) | 1 | word-wrap enabled (0 inside `:lines.`) |
| `align` (+0x14) | 1 | line alignment: 1 left, 2 right, 4 centre |
| `attr` (+0x16) | 0 | current attribute word (§6), copied into each word record |
| `winWidth` (+0x1A) | window width | |
| `rebuild` (+0x1C) | page flag | when 0 (a re-format), records are only repositioned and link data is not rebuilt |
| `resTarget` (+0x24) | 0 per cell | set by 0x1D/0x1E (and art areas 0xC–0xF), cleared by 0x08 |
| `maxX` (+0x2E) | 0 | widest x reached (horizontal scroll extent) |

`x` and `y` carry over from one cell to the next, so a page is laid out as one stream across its
cells.

### 3.2 Words and autospacing [both]

For a word `w` with width `wid` (GpiQueryTextBox CONCAT.x in the current font and style):

1. **Line alignment** (only for the first word after FA/FD/0x1A, and only if `align & 1 == 0`).
   `len = PnlCELength(rest of line up to the next FA/FD)`. PnlCELength adds `SW` after each word
   while autospacing is on, adds `SW` (or `2·SW`) for FE, and follows FC, 0x04 and 0x19. If
   `len < right`: right alignment sets `x = right − len`, and centre sets `x = (right − len)/2`.
   Neither adds `left`. Otherwise `x = SW`.
2. **Wrapping.** Not in mono mode, and only if `wrap`.
   * Autospacing on: if `x + wid > right` **and** `x > left`, start a new line (`y += LH; x = left`).
   * Autospacing off (a *glued run*): sum the widths of this word and the following words up to
     the first FE, FA, FB or FD, any 0x02 escape, or the end of the cell. Other escapes are
     skipped. If an FC is met, the word after it is included and the scan stops. If
     `x + total > right` and `x > left` and `x − left > SW`, wrap before the run. The other words
     of the run are marked so that they never wrap individually. The run therefore wraps as one
     unit, e.g. `OS/2*` or `files.`.
3. The word is placed with its **baseline at `y`** and its left edge at `x`. Then `x += wid`.
   **If autospacing is on and not in mono mode, `x += SW`** (a trailing space after the word).
   `maxX` is updated.
4. Mono (`:xmp.`) mode: no wrapping, no alignment, and **no automatic space ever**. Spaces are
   encoded explicitly as dictionary words made of blanks ("   ") or as FE.

**Encoding idiom (IPFC).** Autospacing is on by default, so each word is followed by a space. To
glue tokens together, IPFC emits FC before the first glued token and FC again *before the last
token of the run*. The last token gets the trailing space again. For example
`the [FC] OS / 2 [FC] * System` renders as `the OS/2* System`, and
`files [FC] . [FC] [FE] [FC] You` renders as `files.  You`. An FE while autospacing is off adds
one space. Sentence ends are encoded as `. FC FE FC`, which gives 2 spaces. Confirmed by
rendering EHXHP and REXX with these rules (§8).

### 3.3 FA (paragraph), FD (break), FE (space), FC (toggle) [both]

* **FA:** `y += (x > left) ? 2·LH : LH; x = left; spacing = 1`, and alignment is re-applied on the
  next word. A paragraph ends the current line and leaves one blank line, unless the pen is
  already at the left margin. The first FA of a page therefore leaves a blank line at the top.
* **FD:** `y += LH; x = left; spacing = 1`, alignment re-applied. FD also updates `maxX`.
* **FE:** `x += SW`, and `x += SW` again if autospacing is on.
* **FC:** `spacing = !spacing`.

### 3.4 FB (centre) [both]

`y += LH`, which always starts a new line. Then `len = PnlCELength(from the byte after FB to the
next FA/FD)`, and `x = (right − len)/2` if `len ≤ right`, else `x = SW`. The left margin is not
added. Used by `:p align=center`-style text: 612 occurrences in 306 files.

### 3.5 Mixed line heights [code]

Escape 0x19 can switch to a font with a larger `LH`. In that case MscFormatWord re-baselines the
current line: if the gap to the previous line's baseline is less than the new `LH`, `y` becomes
`prevBaseline + LH`, or `LH − descent` on the first line of the page. All words already on the
line move with it.

---

## 4. Escapes (FF len type …)

`p` points at the FF. All margins are in units `U`. **Version-2 files (every sample) add one
extra `U` to every margin value from 0x02, 0x11 and 0x12.** In other words
`margin_px = U·arg + (hdr[6] > 1 ? U : 0)`, and the default left margin is `2U`.

### Layout and margins

| Type | Layout | Behaviour | Status |
|---|---|---|---|
| **0x02** set left margin (`:lm`, list indent) | `FF 03 02 n` | `left = U·n (+U)`. `if x < left: x = left`. There is no line break; it tabs to the margin. | [both] 375 932 uses |
| **0x03** right margin (`:rm`) | `FF 03 03 n` | `n==0`: `right = winWidth − U`. Else if `winWidth < U·n + left + U`: `right = (U+left < winWidth) ? U+left : 0`. Else `right = winWidth − U·n`. | [both] (11 uses, e.g. BDUSER.INF) |
| **0x11** margin + new line | `FF 03 11 n` | `left = U·n (+U); y += LH; x = left`. Used for `:pd.` in `:parml break=all` and similar ("Syntax ⏎ The function…"). | [both] 72 412 |
| **0x12** margin, new line only if past it | `FF 03 12 n` | `left = U·n (+U); if x > left: y += LH; x = left`. Used for `:dl`/`:parml` break=fit columns. | [both] 5 823 |
| **0x1C** left margin := current x | `FF 02 1C` | `left = x`. Used for hanging indents (`:note.`, `Note: text`). | [both] 3 457 |
| **0x1A** begin `:lines.` | `FF 03 1A a` | `align = a` (1 left, 2 right, 4 centre); `y += (x > left) ? 2·LH : LH`; `x = left (+SW if spacing)`; `wrap = 0`; re-align on the next word. | [both] 8 468 (a=1 in all samples) |
| **0x1B** end `:lines.` | `FF 02 1B` | `wrap = 1; align = 1`; no line break. | [both] |
| **0x0B** begin `:xmp.`/`:cgraphic.` | `FF 02 0B` | `y += (x > left) ? 2·LH : LH; x = left`. **If no font is selected (attr font nibble 0)**, select font 1 (the monospace entry of the font table) and set attr font = 1. `mono = 1`. | [both] 20 234 |
| **0x0C** end `:xmp.` | `FF 02 0C` | If attr font == 1: clear it and MscInitFont(0). `mono = 0`. **Snap `y` up to the line grid:** `r = (y + sysDescent) mod LH; if r: y += LH − r`. No line break; IPFC follows it with FA. | [both] |

### Character attributes

| Type | Layout | Behaviour | Status |
|---|---|---|---|
| **0x04** style (`:hpN.`, `:hp1-3,5-7`) | `FF 03 04 s` | `attr.style (bits 8–10) = s`. MscSetFontStyle(font, s) creates or selects logical font id `font·8 + s`, cloned from the current font metrics with fsSelection: 1→ITALIC(1), 2→BOLD(0x20), 3→BOLD\|ITALIC, 4→UNDERSCORE(2), 5→ITALIC\|UNDERSCORE, 6→BOLD\|UNDERSCORE, 7→all three. `LH`/`SW` are reloaded. IPF mapping *(inferred, standard IPF)*: hp1=1, hp2=2, hp3=3, hp5=4, hp6=5, hp7=6; hp4/8/9 are colours (0x0D). `s=0` returns to plain. | [both] 336 720 |
| **0x0D** special-text colour (`:hp4/8/9`) | `FF 03 0D c` | Only when `rebuild`: `attr.fg (bits 0–3) = c`. Samples: 1 (hp4, blue), 2 (hp8, red), 3 (hp9, pink), 0 to end. | [both] 10 222 |
| **0x13** foreground colour (`:color fc=`) | `FF 03 13 c` | Same code path as 0x0D: `attr.fg = c`. | [both] 4 631 |
| **0x14** background colour (`:color bc=`) | `FF 03 14 c` | `attr.bg (bits 4–7) = c`. | [both] 2 193 |
| **0x19** font (`:font.`) | `FF 03 19 i` | `attr &= ~0xF700` (**font change also resets the style to plain**); `attr.font (bits 12–15) = i`; MscInitFont(i). `i` indexes the file's font table (0 = default/System Proportional). If the new `LH` is larger, re-baseline the line (§3.5). Indexes ≥16 refer to DDF fonts (see 0x20). | [both] 11 919 |

Colour index → PM colour (`FColorAra`/`BColorAra`) [code]:
`0` → `SYSCLR_HELPTEXT` (−11) for fg or `SYSCLR_HELPBACKGROUND` (−12) for bg; `1` BLUE; `2` RED;
`3` PINK; `4` GREEN; `5` CYAN; `6` YELLOW; `7` → `CLR_BROWN` (14); `8` DARKGRAY; `9` DARKBLUE;
`10` DARKRED; `11` DARKPINK; `12` DARKGREEN; `13` DARKCYAN; `14` → `CLR_BLACK` (−1);
`15` PALEGRAY.
Words that carry a link (attr & 0x30000) are drawn in the instance hypertext colour instead,
unless the database has flag 0x40 ("no links"). Search hits (attr 0x400000) use the
highlight colour.

### Hypertext links (text)

All link escapes act only when `rebuild` is set. They set `attr` bits, and every word placed
until **0x08** inherits them.

| Type | Layout | Behaviour | Status |
|---|---|---|---|
| **0x05** link to panel (`:link reftype=hd refid=`) | `FF len 05 u16 toc [extdata]` (`len` 4, or 6–20 with extdata at `p+5` when `len>4`) | `linkTarget = toc`. With extdata: if `ext.flags & 0x40` (**auto**), the link is *not* made clickable and the panel is queued as an automatic viewport (PnlSaveAutoTOCIndex type 5). Otherwise `attr |= 0x800000` and the extdata is attached to the word records (+0xC). Then, if `toc < 0xFFFE` and links are allowed, `attr |= 0x20000`. | [both] 109 541 |
| **0x06** link, no extdata | `FF 04 06 u16 toc` | Same as 0x05 without extdata parsing. | [code] (none in survey) |
| **0x07** footnote (`reftype=fn`) | `FF 04 07 u16 toc` | `linkTarget = toc; attr |= 0x20000`. **The word record does not distinguish footnote from hd link.** The target TOC entry decides when the link is followed (MscSetViewPort; *inferred*). | [both] 2 918 |
| **0x1D** link by resource number (`res=`) | `FF len 1D u16 res [extdata@p+5 if len>4]` | `resTarget = 1`, then as 0x05. Auto links convert `res` with HMGetTOCIndexFromNumber. For words, the low nibble of the record flags is forced to 0xF, which marks the +6 target as a *res number*. `res` = 0xFFFE/0xFFFF means no link. | [both] 34 503 (pm1: `link-res 50192` → WinCreateMsgQueue) |
| **0x1E** like 0x1D, no extdata | `FF len 1E u16 res …` | Same as 0x1D. PnlGetExtLnkData ignores its tail. | [both] 34 (NSCUG.INF, all `fe ff` = no-op) |
| **0x16** inform (`reftype=inform res=`) | `FF len 16 u16 res [ext@p+5]` | `linkTarget = res`. If extdata has auto: queued (type 0x16). Else, if `res < 0xFFFE`, `attr |= 0xA0000` (link + inform). | [both] 121 |
| **0x10** launch (`reftype=launch object= data=`) | `FF len 10 m str…` where `m` = 0 or the offset at which extdata starts. The string is at `p+4`, `(m?m:len) − 3` bytes: `"program parameters"`. Ext at `p+1+m` when `m≠0 && m≠len`. | The string is copied (`state+0x26`). If extdata has auto, **StartProgram runs immediately at format time** (split at the first blank into program and args) and no link is made. Otherwise `attr |= 0x30000` (launch + link). Words get a `{u16 word, char* cmd}` launch block at +4. | [both] 157 (e.g. `view.exe rexx.inf`) |
| **0x1F** link to external database (`reftype=hd database= refid=`) | `FF len 1F db n id[n] [ext@p+5+n if n+4<len]` | Only if MscIsDatabaseAvailable(db). The id string is copied. `linkTarget = db`; `attr |= 0x120000`, plus 0x800000 when extdata exists. Words get `{u16 word, u16 db, char* id}` at +4. | [both] 2 108 (`db=0 id="193"`) |
| **0x08** end link | `FF 02 08` | `attr &= 0xFF64FFFF` (clears 0x10000, 0x20000, 0x80000, 0x100000, 0x800000); `resTarget = 0`. | [both] 141 926 |

**Extdata** (PnlGetExtLnkData) [code], flags confirmed in the samples:
`u16 flags`, then the fields that are present, in this order:
* `flags & 0x0001`: `u8 units, u16 x, u16 y` (origin)
* `flags & 0x0002`: `u8 units, u16 cx, u16 cy` (size)
* `flags & 0x0008`: `u16` (viewport/window id *(inferred)*)
* `flags & 0x0400`: `u16 group`

Bit 0x40 = auto. The other bits (seen: 0x04, 0x80, 0x200) are style or dependency flags passed
to MscSetViewPort, and their meaning was not traced. The units bytes seen are 0x44 and 0x11; the
encoding was not traced. Examples: EHXHP `05 01 00 | 00 04 6e 00` → flags 0x0400, group 110;
CMDREF `ext flags 0x0204`; ipf.inf acviewport ext `c3 00 | 44 02 00 04 00 | 11 37 00 64 00`.
The parsed result is a 20-byte block `{flags, id, x, y, cx, cy, units, …, target@+0x10, group@+0x12}`.

### Images and hypergraphics

| Type | Layout | Behaviour | Status |
|---|---|---|---|
| **0x0E** bitmap (`:artwork.`) | `FF 07 0E f u32 off` | Makes a bitmap record (attr 0x40000) and calls ArtFormatBitmap. Flags `f`: **0x10 runin**, 0x02 right, 0x04 centre, 0x08 fit, otherwise left (bit 0x01 is set with left/centre; in non-bidi mode only the absence of 2/4/8 matters). | [both] 2 064 (REXX: `0x11` = runin) |
| **0x0F/00** art for artlink | `FF 08 0F 00 f u32 off` | Same as 0x0E, but the record attr is 0x60000 (bitmap + link) when links are allowed. This art record becomes "current art", and the area escapes that follow attach to it. | [both] 493 |
| **0x0F/01,02,03** link area with rect | `FF len 0F t u16 tgt u16 x u16 y u16 cx u16 cy [ext@p+0xE]` | Area record type 1 (hd link) is appended to the current art's area list. Subtype 1 has extdata; 2 and 3 have none (they parallel 0x05/0x06/0x07). | [both] 81 |
| **0x0F/04,05,06** whole-bitmap link | `FF len 0F t u16 tgt [ext@p+6]` | Area type 1 covering `0,0,bmpW,bmpH`. | [both] 359 |
| **0x0F/07** launch, whole / **08** launch, rect | `FF len 0F 07 m cmd…` (cmd at `p+5`, `len−4` bytes) / `FF len 0F 08 m x y cx cy cmd…` (cmd at `p+0xD`, `len−0xC` bytes); ext at `p+1+m` | Area type 2 (launch), target 0xFFFF. | [both] 12 (WPHELP: `view.exe cmdref.inf`) |
| **0x0F/09** inform whole / **0A** inform rect | like 04 / 01 | Area type 4 (inform). | [both] 117 (rxftp) |
| **0x0F/0E,0F** res whole / **0C,0D** res rect | like 04 / 01 | Area type 1. Also sets `resTarget` for later words. | [code] |
| **0x0F/10** extdb whole / **11** extdb rect | `FF len 0F 10 db n id…` / `FF len 0F 11 db n x y cx cy id…` | Only if the database is available. Area type 8. | [code] |
| **0x0F/0B** | `FF 07 0F 0B 01 00 01 00` | **Ignored by FormatCell** (not handled in the subswitch). It always follows the 0x0F/00 art in samples. | [both] 493 |

Area record (ArtAllocHpBmpArea): `u8 type (1 link, 2 launch, 4 inform, 8 extdb; |0x80 = has extdata); u16 x, y, cx, cy; ptr next@+9; u16 target@+0xD; ptr ext@+0xF`.
Coordinates are bitmap pixels. **`y` is measured upward from the bitmap's bottom edge**, following
PM convention. IsHyper tests `bmpX+x ≤ mx ≤ bmpX+x+cx` and `bmpBottom−y−cy ≤ my ≤ bmpBottom−y`,
with document y pointing down. For a *fit* bitmap the area is scaled by
`window size / bitmap size`.

**Bitmap placement** (ArtFormatBitmap; `sysLH` and `sysDesc` are the *system* font metrics, not
the current font's):
* **Non-runin:** the bitmap always starts **below** the current line.
  `rec.y (bitmap bottom) = y + (⌊(h+sysDesc)/sysLH⌋ + 1)·sysLH`. x is `left`, `right − w`
  (right), or `(right − left − w)/2` (centre, which falls back to `left` if it does not fit;
  note the result is not offset by `left`). After the bitmap: `y = rec.y + sysLH; x = left`.
* **Fit (0x08):** `rec.x = rec.y = −1`. DisplayPage BitBlts it stretched to the whole window
  rect. It does not move the pen.
* **Runin (0x10):** if `x + w < right`, the bitmap goes at `x`. If the vertical gap to the
  previous line is less than `h + sysDesc`, the bitmap bottom becomes
  `y + ⌊(h+sysDesc)/sysLH⌋·sysLH` and the words already on the line move down to it. Then
  `x += w + SW`. Otherwise the bitmap wraps: it goes to `left` at `y + (⌊…⌋+1)·sysLH`, and
  `x = left + w + SW`. Either way `y = rec.y`.
* `maxX` includes the bitmap's right edge, except for fit bitmaps.

### Conditional text, titles, DDF, viewports

| Type | Layout | Behaviour | Status |
|---|---|---|---|
| **0x17** begin `:hide key=` | `FF len 17 key[len−2]` | Visible only if the env var **`IPF_KEYS`** (upper-cased) contains `key` as a `+`-separated item, ended by NUL, `+` or `;` (MscIsHideCDI). Otherwise skip to the next **0x18** (no nesting; MscSkipHideCDI). If the cell ends while skipping, a flag (state+0x22) makes the next cell continue skipping. | [both] 31 (toolsref.inf `FULKGL`) |
| **0x18** end hide | `FF 02 18` | Terminator for the skip. A no-op when the text is visible. | [both] |
| **0x15** | `FF len 15 str[len−2]` | Copies `str` into an instance field (`inst+0x11E`) and sends message 0x1007 to `inst+0x2A`. Meaning unknown (possibly a title or status override); **never seen in 306 files**. | [code] |
| **0x20** DDF (`:ddf res=`) | `FF 04 20 u16 res` | PnlProcessDDFData obtains the application's DDF. DdfFormatCell then lays it out with the same state, `attr | 0x1000000` (DDF records; font indexes +16 use DDF fonts). | [code] (not in survey) |
| **0x21** `:acviewport.` | `FF len 21 ?? n u16 objid s1 s2 s3 [ext]`, where `n` = total bytes of the three strings, and each string is `len(incl. itself), chars`. ipf.inf: `objid=1`, `s1="IPFMain"` (objectname), `s2="ipf"` (dll), `s3=""` (objectinfo). Ext at `p+7+n` when `n+6 < len`. | MscGetACVPInfo builds a block that is queued as an auto viewport (type 0x21). Nothing is drawn inline. | [both] 1 (ipf.inf) |
| **0x00, 0x01, 0x09, 0x0A, 0x22+** | | Not handled: skipped by length. | [code] (none seen) |

---

## 5. Word records (layout output) [code]

The page owns an array of 0x12-byte records (PnlFillWords). Their count comes from
PnlCalcCDISize: one record per word byte `< hdr[0x52]`, plus one per 0x0E escape and per
0x0F/00 escape. FE, FA and the other control bytes produce no record.

```
+0x00 u32 flags (= attr when placed, see §6)
+0x04 u16 global word index | ptr (launch block, extdb block, HpBitmap)
+0x06 u16 link target (TOC index, or res number when flags&0xF == 0xF)
+0x08 i16 x (left edge)
+0x0A i16 y (baseline; for bitmaps the bottom edge), document coordinates, downward
+0x0C ptr extdata (flags & 0x800000)
+0x10 u16 width (bidi only)
```

DisplayPage draws each record at `(x − hscroll, top + winH − y)`. It switches font and style
when `attr` changes and sets colours as in §4. **It does not draw words whose first character is
a blank unless a background colour is set.** When the background is set it fills the gap between
consecutive words on a line. For underline styles (4–7) it fills the gap with `_` glyphs so that
the underline is continuous.

## 6. Attribute word bits [code]

| Bits | Meaning |
|---|---|
| 0x0000000F | fg colour index (0x0D/0x13); forced to 0xF on res-number link words |
| 0x000000F0 | bg colour index (0x14) |
| 0x00000700 | style 0–7 (0x04) |
| 0x0000F000 | font-table index (0x19; 1 = automatic monospace inside :xmp) |
| 0x00010000 | launch |
| 0x00020000 | hypertext (clickable) |
| 0x00040000 | bitmap record (0x60000 = art carrying link areas) |
| 0x00080000 | inform |
| 0x00100000 | external-database link |
| 0x00200000 | "glued": the previous word had no trailing space and this word does not start with a blank (set in the record only) |
| 0x00400000 | search-hit highlight (set elsewhere) |
| 0x00800000 | extdata pointer at +0xC |
| 0x01000000 | DDF record |

## 7. Hyperlink hit-testing [code]

IsHyper (skipped entirely if database flag 0x40) converts the mouse position to document
coordinates and walks the records.

* **Text links** (byte 2 has 0x02 and not 0x11). HPGetHPTWordsWidth groups the record with the
  following records **on the same baseline that have the same target** (+6). Launch links are
  grouped by identical command string and extdb links by identical id and db. The hot box is
  `x0 … lastX + lastWidth` horizontally and `baseline − ascent … baseline` vertically, where the
  ascent is the TOPRIGHT.y of the text box. Each line of a multi-line link is a separate box. The
  target goes to `page+0x42`, or 0xFFFF for launch and extdb.
* **Art** (flags 0x60000): walk the area list as in §4. Area types 2 and 8 report target 0xFFFF.
* Clicking (ProcessPageButton1Down) highlights the region (SetCurOnHyperText or
  InvertHyperBitmap) and calls MscSetViewPort, which follows the link.

---

## 8. Validation

### 8.1 Decoded cells (`·` = blank inside a word, `[sp~]` = FC, `[_]` = FE)

EHXHP.HLP cell 0:
```
[para]
 Use the [link 1 ext[flags=0x400 group=110]] [sp~] OS / 2 [sp~] * [/link] System Editor as a text editor to
 create and edit your [sp~] CONFIG . [sp~] SYS and [sp~] AUTOEXEC . [sp~] BAT system [sp~] files [sp~] . [sp~] [_] [sp~]
 You can also use … at [sp~] once [sp~] . [sp~] [_] [sp~] System Editor performance is good for text files up to
 [sp~] 32K [sp~] . … performance may not be [sp~] acceptable [sp~] . [para]
 You can use menu bar and menu choices to edit text [sp~] files [sp~] . … [sp~] programs [sp~] . [_]
```
EHXHP.HLP cells 1–5:
```
--- cell 1:  [para] Trademark of the IBM [sp~] Corporation [sp~] . [_] [_]
--- cell 2:  [para] Use the choices on the [style b] File [style plain] menu to [sp~] create [sp~] , [sp~] open [sp~] , and save …
--- cell 4:  [para] Use [style b] Open [style plain] to edit an existing text [sp~] file [sp~] . [_] [_]
--- cell 5:
[lm 2] [para]
 [sp~] [_] 1 . [lm 6] [sp~] Select the arrow to the right of the [style b] Drive [style plain] list … [sp~] system [sp~] . [lm 2] [br]
 …
 For a detailed explanation of each [sp~] field [sp~] , select from the list [sp~] below [sp~] : [lm 3] [para]
 [sp~] \x07 [lm 6] [link 6 ext[flags=0x400 group=20]] [sp~] Open filename [/link] [lm 3] [br]
 …
 [sp~] \x07 [lm 6] [link 12] [sp~] Cancel pushbutton [/link] [lm 1] [br]
 [_] [_]
```
(`\x07` is a dictionary word; codepage 437/850 fonts draw it as a bullet.)

REXX.INF cell 18 (excerpt):
```
[para] SELECT tells the interpreter … looks like [sp~] this [sp~] : [xmp] SELECT [br]
 ··· WHEN · expression1 [br]
 ······ THEN · instruction1 [br] … END [br]
 [/xmp] [para]
 [style b] [sp~] Note [sp~] : [_] [sp~] [_] [lm-here] [style plain] [sp~] An [sp~] IF - [sp~] THEN instruction … [lm 1] [para]
 [lm 3] [para]
 [sp~] \x07 [lm 6] [sp~] If expression1 is [sp~] true [sp~] , instruction1 is [sp~] run [sp~] . … [lm 3] [br]
 …
 This diagram shows the SELECT [sp~] instruction [sp~] : [bitmap flags=0x11 off=0x1ae] [para]
```
Other sample cells: CMDREF 5 has `[art flags=0x5 off=0x0] [art-sub11 01000100] [artarea1 tgt=5 rect=[5,5,81,26] ext flags 0x204]`.
pm1 5 has `[lm+nl 6]` parml columns and `[link-res 50192] WinCreateMsgQueue [/link]`. pm1 1121 has `[hpcolor 1] … [hpcolor 0]`.

### 8.2 Rendering check (§3/§4 rules, 1 column = U = SW, 64 columns)

```
  Use the OS/2* System Editor as a text editor to create and
  edit your CONFIG.SYS and AUTOEXEC.BAT system files.  You can
  …
    1. Select the arrow to the right of the Drive list to
       display all the drives on your system.
  …
    *  Open filename
  …
  SELECT
     WHEN expression1
        THEN instruction1
  …
  Note:    An IF-THEN instruction cannot be used with a SELECT
           instruction unless it follows a WHEN or OTHERWISE
```
Glued punctuation, double spaces after sentences, hanging list indents, the `Note:` hanging
indent through 0x1C, and verbatim `:xmp.` all come out correctly. This confirms the spacing model.

### 8.3 Escape types per sample file (type, or (0x0F, subtype): count)

| Type | EHXHP | CMDREF | REXX | pm1 | All 306 files |
|---|---|---|---|---|---|
| 02 lm | 432 | 7097 | 1376 | 7687 | 375 932 |
| 03 rm | – | – | – | – | 11 |
| 04 style | 534 | 4898 | 2748 | 4918 | 336 720 |
| 05 link | 77 | 868 | 53 | 2407 | 109 541 |
| 07 footnote | – | 1671 | 1 | – | 2 918 |
| 08 end link | 78 | 2539 | 54 | 5219 | 141 926 |
| 0B/0C xmp | – | 2164/2164 | 374/374 | 280/280 | 20 234 each |
| 0D hp colour | – | – | – | 48 | 10 222 |
| 0E bitmap | – | – | 9 | – | 2 064 |
| 0F/00 art | – | 228 | 5 | – | 493 |
| 0F/01 area | – | 1 | – | – | 81 |
| 0F/04 area | – | 227 | 5 | – | 359 |
| 0F/07 launch area | – | – | – | – | 12 |
| 0F/0A inform area | – | – | – | – | 117 |
| 0F/0B (ignored) | – | 228 | 5 | – | 493 |
| 10 launch | – | – | – | – | 157 |
| 11 lm+nl | 17 | 459 | – | 2875 | 72 412 |
| 12 lm/nl-if-past | – | 263 | – | – | 5 823 |
| 13 fc | – | 22 | – | – | 4 631 |
| 14 bc | – | 22 | – | – | 2 193 |
| 16 inform | – | – | – | – | 121 |
| 17/18 hide | – | – | – | – | 31/31 |
| 19 font | – | 44 | – | – | 11 919 |
| 1A/1B lines | – | 33/33 | – | 158/158 | 8 468/8 456 |
| 1C lm-here | 8 | 138 | 41 | 19 | 3 457 |
| 1D res link | – | – | – | 3100 | 34 503 |
| 1E | – | – | – | – | 34 |
| 1F extdb link | 1 | – | – | – | 2 108 |
| 21 acviewport | – | – | – | – | 1 |
| 06, 15, 20 | – | – | – | – | 0 |

Control bytes in the 4 samples: FC 101 558, FD 22 436, FA 15 027, FE 6 271, FB 592.
Across all 306 files: FC 2 134 469, FE 520 462, FD 422 332, FA 281 614, FB 612.

## 9. Not covered / caveats
* DBCS code pages (`IsDBCSCodePage`) use a WinDrawText-based line breaker (the `-2`/`-3` record
  markers in FormatCell). Bidi modes (`inst+0x162` = 1 or 2) re-order lines with FormatLine.
  Both paths are out of scope.
* Printer variants (FormatCell2, PnlCELength2, MscFormatWord2) were not compared.
* Not traced: what extdata style bits other than 0x01/0x02/0x08/0x40/0x400 mean, the unit-byte
  encoding, the meaning of escape 0x15, and how MscSetViewPort tells footnotes apart.
