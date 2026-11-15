# Open Watcom bugs found on rs2

These bugs were found while making Open Watcom 1.9 and 2.0 run on rs2.
They are in Open Watcom itself, not in rs2. Each one was traced in Open
Watcom's source (https://github.com/open-watcom/open-watcom-v2) or in the
1.9 binaries. Each also shows on other hosts, or follows from the code
whatever the system does.

Don't chase these as rs2 bugs, and don't work around them in rs2. Tests
avoid them instead, as noted under each one. The per-tool results are in
docs/COMPATIBILITY.md:

- "Open Watcom C/C++ 1.9 (OS/2 host)"
- "Open Watcom F77 1.9 (OS/2 host)"
- "Open Watcom 2.0 (OS/2 host)"

## Open Watcom 2.0 (GitHub Current-build of 2026-11-14)

| Bug | Symptom | Cause | How the tests avoid it |
|-----|---------|-------|------------------------|
| DWARF reader | `wd`/`wdw` fault in `DWARF.D32` on every `-d2` program, and on IDE builds (the IDE links with `d all`) | `read_Shdr` in `bld/dip/dwarf/c/dfld.c` reads `sh_name` twice, so every section-header field is shifted by one. The string table gets size 0, and `malloc(0)` returns NULL, which is then read. Introduced by commit e1134ccd (2026-06-14). On DOS, address 0 can be read, so the debugger just finds no source (upstream issue #1687). | Build with `-hw`, and set the IDE's linker debugging to `d watcom` |
| Debug-info file closed after loading | No source lines or locals for `-hw` programs. `CODEVIEW.D32` divides by zero on `-hc` programs. `wprof` faults. | `ProcImgSymInfo` in `bld/wv/c/dbgprog.c` closes the file after `DIPLoadInfo` even when the load succeeded (the 2015 code closed it only on failure). The readers keep reading through the closed handle (DosSetFilePtr returns error 6). `wprof`'s `setsamps.c` does the same, then `fseek`s a NULL stream. Upstream issue #1687. | None; debugging works without source |
| Named-pipe link name | `wd -tr=nmp` retries opening `TCN` forever | `bld/trap/nmp/c/nmplink.c` computes `NameEnd = NameBuff + strlen(NameBuff)` in `RemoteLinkSet`, before `OpenRequest` has filled `NameBuff` (the 2019 code computed it afterwards) | Remote-debug over TCP instead |
| Static C++ with `-bm` | Faults before `main` | `wpp386 -bm` makes objects import `__imthread` (`plusplus/c/cginfo.c`). Its initialiser calls `__InitMultipleThread`, which stores into `__ThreadData[1]` before anything has allocated it on 32-bit OS/2. | Use `-bm -br` (the runtime DLLs) |
| F77 `CP-09` | Every `wfc`/`wfc386` compile ends with `*ERR* CP-09` (preceded by `SM-02`) and exit code 1, though the object file is complete | Upstream issue #1623 (also seen on Windows). The DOS-hosted `wfc386` does it too. | Scripts check for the object file, not the exit code |
| `wfl`/`wfl386` | Fault in `CompLink` or loop forever | `ListAppendString` in `bld/f77/wfl/c/wfl.c` never sets the new item's `next` | Compile with `wfc386`/`wfc` and link with `wlink` |
| `fcenable` | Banner printed without line ends | The banner is printed without `\n` | Cosmetic |

## Open Watcom 1.9 (and 2.0 where noted)

| Bug | Symptom | Cause |
|-----|---------|-------|
| `wde` sizing (also in 2.0) | Application Error in FMEDIT.DLL (0001:4592) when a control is sized from a corner handle | `WdeGetCurrentDialog` asks the object being sized (an FMEDIT EATOM) for its parent. The EATOM doesn't answer, so WDE uses a pointer it never initialized. On rs2 the trigger is a toolbox WM_PAINT during sizing; whether rs2 should send that paint at all is being fixed in rs2's PMWIN. |
| `wde` Tools menu | Icons are drawn but most item texts are blank | 1.9's tool-bitmap table in `WdeInitToolMenu` has no end marker. The loop runs on into a second copy and owner-draws items again with text it can no longer read from them. This would happen on Windows too. |
| F77 FWCOPY sample | The program ends at start | Its AppendMenu calls push 8 bytes of arguments where the function takes 10 (`value far` of a LOC() value in WINAPI.FI) |
| F77 help object | The "Fortran Error Messages" object finds no book | It names WFCERRS.INF; the package only ships WFCERRS.HLP |

## Quirks rs2 accommodates

These are not crashes, but rs2 copes with them deliberately:

- **`wddespy`:** OW's `ddeml.h` declares `DdeAccessData` as returning a
  near pointer, and 1.9's wddespy reads the monitor data at `DS:offset`.
  rs2 puts monitor data handles in the monitor's DGROUP so this works. On
  real Windows it would read the wrong memory.
- **`wspy`:** the hWnd column shows a wrong high word, because Spy prints
  the 16-bit handle as 32 bits. This is Spy's own formatting.
