# Win-OS/2

Win-OS/2 is IBM's Windows 3.1 for OS/2: Microsoft's Windows 3.1 modules
(USER, GDI, the drivers, the accessories) with IBM's own kernel
(`OS2K386.EXE` in place of `KRNL386.EXE`), loader (`WINOS2.COM` in place
of `WIN.COM`) and OS/2 glue (WINSHELD, WINSMSG, the seamless display
drivers, the clipboard and DDE bridges). On OS/2 it runs as a DPMI client
in a DOS session, served by OS/2's virtual device drivers (VDPMI, VDPX,
VWIN, VVGA, VMOUSE).

rs2 never ships it. It is on the user's OS/2 CD, and APPINST copies it
from there (category *Win-OS/2*), as it does QBasic.

## Status

**Seamless Win-OS/2 works**: Windows programs' windows are windows on
the PM desktop beside OS/2 programs'. Win-OS/2 runs full screen, in a PM
window and seamless, with IBM's default `useos2shield=1` (WINSHELD, the
OS/2 shield) and rs2's own display driver, RS2DISP.DRV, at any size.

| What | dosrun (host) | QEMU (rs2) |
|------|---------------|------------|
| Full screen (`WIN-OS/2 Full Screen`) | works | works (about 2 minutes to start under TCG) |
| In a PM window (`WIN-OS/2 Desktop in a Window`), RS2DISP at 800x600 | works (`--display WxH`) | works, Windows' pointer is the PM pointer |
| Seamless (`WIN-OS/2 Window`, the accessories' objects) | works (`--seamless`) | works: each Windows window a PM window |
| WINSHELD (`useos2shield=1`) | works: starts the programs, or Program Manager with the OS/2 *Desktop* icon | works |
| Clock, Notepad, Calculator, Write, Paintbrush, Control Panel, File Manager, Cardfile on RS2DISP | work | Clock, Notepad and Program Manager checked, seamless |
| Seamless windows: typing, clicks, activation, moving by the title bar (with the outline shown), closing (Alt+F4, Windows' message boxes as windows), the window list | works (`--keys` with `{press}`, `{mouse}`, `{release}`) | works |
| Mouse | absolute positions (`{mouse X Y}` in `--keys`) | absolute (QEMU's tablet, VMware's pointer) |
| Keyboard | works | works |
| Clipboard and DDE with OS/2 | - | not yet |

Screenshots of each step: the progress folder `work/progress/winos2-seamless`
(and `work/progress/winos2` for the first, full-screen probe).

## The pieces

```
 Windows (in the DOS session)                       rs2
 ----------------------------                       ---
 GDI ── RS2DISP.DRV ── traps ──────────────► crates/dos::disp: the DDK in Rust,
                                             drawing an 8-bit surface
 USER ── WINSMSG ── INT 66h / entry ───────► crates/dos::vwin: VWIN's shield
 WINSHELD ──┘                                messages, the window list
 MOUSE.DRV ── INT 33h, VMOUSE$ ────────────► crates/dos::mouse, ::vdd
                                                  │
                     VDM.EXE ── shared surface (win16drv::surface:
                                pixels, damage, pointer, windows)
                                                  │  RS2API 406/407
                     PMVIOWIN ── one PM frame per Windows window
                                 (seamless.rs), or one window (surface.rs)
```

## What it took (full screen)

All in `crates/dos` (the DOS machine both dosrun and VDM.EXE run):

1. **The "MS-DOS" DPMI extensions** (`dpmi.rs`). Win-OS/2's kernel asks
   for them with INT 2Fh AX=168Ah, first under the name `WINOS2`, then
   `MS-DOS`, and stops with *Inadequate DPMI_DOS_API Server* without
   them. OS/2's VDPX.SYS provides them (studied in Ghidra with
   `vdpx.sym`): the entry point's function 0000h returns the version
   0100h, 0100h/0101h the *LDT alias*, a data selector the kernel
   writes its descriptors through. rs2's CPU reads descriptors from
   memory on every segment load, so the alias needs nothing else.
2. **The DOS API in protected mode.** Asking for the extensions turns
   on VDPX's translation (OS/2's `DPMI_DOS_API=AUTO`): INT 21h with
   selectors in DS/ES. rs2's DOS is Rust that reads guest memory, so it
   reaches memory through the segment registers' *bases* (`seg_lin`) and
   serves the call directly on the client's selectors. Only calls that
   take or return segments need translating (25h/35h, 48h-4Ah, 1Ah/2Fh,
   50h/51h/62h, 26h/55h, the table pointers). INT 10h-12h, 15h, 16h, 1Ah
   and 33h are served the same way.
3. **VDPMI's INT 31h AX=0010h**: allocate several selectors.
4. **Load-high EXEs** (`dos/proc.rs`): `OS2K386.EXE` has no minimum and
   no maximum allocation, which DOS takes as "load high".
5. **INT 25h/26h** return with RETF and leave the flags on the stack.
6. **Mouse event handlers in protected mode** (MOUSE.DRV's).

## The mouse: absolute positions

MOUSE.DRV's INT 33h event handler (studied with `mouse.sym`) treats an
event with bit 15 of AX set as absolute: CX and DX multiplied by two
factors it gets from VMOUSE through `INT 2Fh AX=4011h` (`VMOUSE$`, entry
function AH=02h; 102 and 136 for 640x480 without it); in a seamless
session (INT 66h AH=80h says so) it takes CX and DX as they are. rs2's
`crates/dos::vdd` answers INT 2Fh 4011h by name in real and protected
mode (OS/2's VDDs register their entry points with VDHRegisterAPI);
VMOUSE$'s AH=02h returns factors of 1, and the mouse driver then hands a
protected-mode handler positions in 0-65535 across the screen, from the
host's absolute pointer (`Event::MouseAbs`). In a seamless session it
hands it pixels: IBM's USER (MOUSE_EVENT with fMSeamless) takes absolute
positions as pixels there, as VMOUSE gives them. ENABLE sets the mouse's
range to RS2DISP's screen. The pointer follows the PM pointer exactly in
a window and seamless.

## RS2DISP.DRV: rs2's display driver

Windows reaches the display only through its display driver, so Win-OS/2
on rs2 gets one of rs2's own: a paravirtual Windows 3.1 display driver
whose drawing the DOS machine does in Rust.

**The module** (crate `win16drv`) is a 16-bit NE library named DISPLAY
with one fixed code segment: for each DDK entry point a stub `FE 38 n n`
(the DOS machine's callback instruction, callback 800h + ordinal) and
`RETF size` (the Pascal arguments' size, checked against VGA.DRV's
RETFs). It carries VGA.DRV's resources (USER's system bitmaps, cursors,
icons, the OEMBIN table), names and description, so it is made from the
user's installed VGA.DRV: APPINST does it after copying Win-OS/2 (a
`make` line), `cargo run -p win16drv --bin mkrs2disp VGA.DRV RS2DISP.DRV`
for dosrun. SYSTEM.INI has `display.drv=rs2disp.drv` and
`sdisplay.drv=rs2disp.drv` (the seamless driver too).

**The DDK in Rust** (`crates/dos/src/disp`): the callback reads the
arguments from SS:SP (the first argument highest), does the work on
guest memory and the surface, sets DX:AX, and the stub returns.

| Entry | What |
|-------|------|
| ENABLE (5) | GDIINFO (with style bit 0) or the PDEVICE; the screen's size from the host |
| DISABLE (4) | back to the VGA picture |
| COLORINFO (2) | RGB to a physical colour (palette index in the low byte, the mono bit in bit 8) and back |
| REALIZEOBJECT (10) | pens and brushes (solid, hollow, hatched, pattern bitmaps 1 and 8 bpp); no device fonts |
| BITBLT (1) | all 256 ROP3s, brushes, mono to colour (1 = background, 0 = text colour) and colour to mono (background = 1), overlapping screen copies |
| OUTPUT (8) | polylines (styled: dash, dot, dash-dot, dash-dot-dot; background in OPAQUE mode) and scan lines with the ROP2s |
| PIXEL (9), SCANLR (12), FASTBORDER (17) | as the DDK has them (SCANLR's style bits checked in VGA.DRV) |
| EXTTEXTOUT (14), STRBLT (11), GETCHARWIDTH (15) | GDI's raster fonts (2.0 and 3.0 formats, the FONTINFO at 42h of the font's image), opaque rectangles, character extra, justification, the caller's widths, emboldening (TC_EA_DOUBLE) |
| DEVICEBITMAPBITS (19), DIBSCREENBLT (21) | DIBs of 1, 4, 8 and 24 bits to and from the device's formats |
| SAVESCREENBITMAP (92) | menus' save-under |
| INQUIRE, SETCURSOR, MOVECURSOR, CHECKCURSOR (101-104) | the pointer goes to the host (the PM pointer); dosrun and the full screen draw it in |
| CONTROL, ENUMDFONTS, ENUMOBJ, DEVICEMODE, SETATTRIBUTE, DEVICEBITMAP, CREATEBITMAP, USERREPAINTDISABLE | nothing to do |

GDIINFO keeps VGA.DRV's capabilities for what they share (polylines,
scan lines, rectangle clipping, BitBlt, 64 KB+ bitmaps, SaveScreenBitmap,
DIBs, 3.0 fonts, ExtTextOut widths, 96 dpi) and says 8 bits per pixel,
256 colours, no palette management: GDI simulates curves, polygons, wide
lines and StretchBlt with scan lines, polylines and BitBlt. The palette
is fixed and laid out as Windows lays out its system palette: the 20
static colours at 0-9 and 246-255, a 6x6x6 cube and greys in inverse
pairs between, so that inverting an index (DSTINVERT, XOR drawing)
inverts the colour. Memory bitmaps are GDI's: 1 bpp or 8 bpp in guest
memory, huge ones across segments (bmSegmentIndex, bmScanSegment).

**The surface** (`win16drv::surface`) is the screen in named shared
memory, `\SHAREMEM\RS2VDM\<pid>.DSP`, that VDM.EXE writes and PMVIOWIN
maps: a header (size, a ring of 64 damaged rectangles, the palette, the
pointer's shape and position, the seamless window list, the flags), then
the pixels. RS2API 406 `Rs2VdmSurface` tells the kernel the session
shows one (PMVIOWIN wakes up as for screen changes) and gives the PM
screen's size; on the full screen VDM.EXE hands the changed rows (with
the pointer drawn in) to the kernel's scaled picture.

Sizes: `WINOS2_SCREEN=WxH` (a DOS setting; 800x600 by default on a
1024x768 screen or larger, else 640x480), up to 1600x1200; seamless
sessions are the PM screen's size.

Not done: palette management (RC_PALETTE: SetPalette and the like), RLE
DIBs, device bitmaps, StretchBlt in the driver, enumerating pens and
brushes (ENUMOBJ), device fonts.

## VWIN: the OS/2 shield

With `useos2shield=1` (IBM's default) USER starts WINSHELD.EXE as the
Windows shell; it and WINSMSG.DLL (with IBM's additions to USER:
REGISTERSEAMLESS, USERCALLBACK, ...) talk to OS/2 through VWIN.SYS.
`crates/dos::vwin` is rs2's VWIN, from VWIN.SYS's router tables and
handlers (Ghidra, `vwin.sym`) and its callers (`winsheld.sym`,
`winsmsg.sym`). INT 66h's AH selects a table; the status comes back in
AX (0, 2 no message, 7Ah too small):

| AH | VWIN.SYS | rs2 |
|----|----------|-----|
| 01h | WINSHIELDADDUSERROUTINE: the routine in EBX (16:16) | kept; EDX = the message entry (called like an interrupt: WINSMSG's `_VWININT` pushes the flags) |
| 02h, 03h | WINSHIELDSENDMESSAGE(NO)BLOCK: the message at EBX, CX bytes | numbered (the first dword), read into the window list; 02h's reply in the same buffer |
| 04h, 07h, 08h | make dirty, send error, fix lost child | done |
| 05h | WINSHIELDGETNEXTMESSAGE: into EBX, EDI bytes, size in CX | the host's requests |
| 06h | WINSHIELDDELUSERROUTINE | forgotten |
| 7Fh | WINDIDWINDDINITIALIZE | yes (RS2DISP needs no VWIN display calls) |
| 80h | WINAMISEAMLESS: AX 0 seamless, BX the session | as the session is |
| 81h-8Ah | the seamless display driver's (RAM semaphore, video controller, critical sections) | done (no SWINVGA) |
| E0h-E5h | pipes for the clipboard and DDE | a handle; WINSHELD's notification pipe (E003h, its routine in EBX) is called at once with 21h, 0101h: start the programs, clipboard and DDE off |

**The shield messages** are 2Ch-byte records (longer ones carry text
after them): +0 VWIN's sequence number, +4 the session's id, +8 top,
+0Ah left, +0Ch height, +0Eh width, +10h a window tag (z-order: the
window behind), +18h style or flags, +1Ch the window's tag (session id,
hwnd at +20h), +24h the code. Windows sends (WINSMSG's WINSHOOKMSG, from
USER's hooks, and WINSHELD's ENUMTOPLEVELWINDOWS):

| Code | Meaning |
|------|---------|
| 1 | a top-level window: rectangle, style at +18h, owner at +28h; the title at the offset at +12h (2Ch) |
| 2 | destroyed |
| 3 | activated |
| 4 | moved, sized, z-order (+18h: 1 top, 2 bottom, 4 behind +10h's), shown (8), hidden (10h), rectangle (20h), restored (100h), minimised (1000h), maximised (4000h) |
| 5 | WINSHELD's first message (blocking): PM answers with its border width at +8 (USERSETBORDERWIDTH) |
| 7, 8 | pointer shapes made and destroyed |
| 0Ah | Windows captures the mouse (+18h 40h) or releases it (80h); blocking: PM's answer at +8 must be nonzero, or USER's MOVESIZE gives up the move |
| 12h | the title changed (text at +2Ch) |
| 16h | WINSHELD is ready |
| 17h | a program could not be started |

PM's side sends the same records (WINSMSG's WINSVWINMSG): 1 makes a
shadow window for a PM window, 2 closes (WM_CLOSE), 3 activates, 4
places, minimises, restores and maximises, 16h starts the program at the
offset at +8 (EXECPROGRAM). rs2 queues them and calls WINSHELD's user
routine (`VWINCALLBACK(0)`, which posts WINSHELD the message to read the
queue) as VWIN does from a context hook: from real mode, through the
same far call into protected mode as the mouse's event handler.

A Windows window that moves in a seamless session takes its picture
along in the surface (`Display::move_picture`): seamless Windows does
not repaint a moved window, as on OS/2 PM moves its bits on the screen.

So WINSHELD runs as on OS/2: in a normal session it starts Program
Manager (or the programs on its command line) and shows the OS/2
*Desktop* icon; in a seamless one it starts the programs and reports
every top-level window. `dosrun --seamless --vwin-trace` shows the
messages.

## Seamless Win-OS/2

A seamless session is a windowed DOS session whose VDM has
`WINOS2_SEAMLESS=1`: VWIN says seamless, RS2DISP draws a Windows desktop
the size of the PM screen, and the surface's flags say so. PMVIOWIN
(`userland/pmviowin/src/seamless.rs`) then shows no window of its own
for the session but:

- **one borderless PM frame per visible Windows top-level window**
  (menus, drop-downs and dialogs too), at the same place, painting its
  part of the surface (the rectangles the VDM reports damaged);
- **window list entries** for windows with a caption, titled as in
  Windows; Close in the window list closes the Windows window;
- **the mouse** as positions on the Windows screen, captured while a
  button is down, with Windows' pointer as the PM pointer;
- **keys** straight to the session while one of its frames has the focus
  (Rs2PmVioWindow, as for DOS windows);
- **z-order and activation** both ways: Windows' order among the frames
  while the user works in Windows, Windows' active window active in PM,
  a frame PM activates activated in Windows; moves and sizes made in
  Windows move the frames, and frames PM moves or sizes (the window
  list's Tile and Cascade) are placed there in Windows (RS2API 407
  `Rs2VdmRequest` carries the host's requests: activate, close, place,
  minimise, restore, maximise, run);
- **minimised Windows programs** keep a hidden frame for their window
  list entry, and showing it from the window list restores and activates
  them;
- **a see-through overlay**: while a mouse button is down, what Windows
  draws on its (never shown) desktop outside its windows, such as the
  outline of a window being dragged, is inverted on the PM screen and
  taken back as Windows erases it.

**Program objects**: the Win-OS/2 PROGTYPEs of OS/2's `pmshl.h`
(PROG_30_STDSEAMLESSVDM 13 ... PROG_31_ENHSEAMLESSCOMMON 18, PROG_31_ENH
19, PROG_31_STD 20) start Win-OS/2 through DosStartSession: VDM.EXE
running WINOS2.COM with the program, seamless ones windowed with
WINOS2_SEAMLESS=1, the full-screen ones full screen. The *common* types
(14, 16, 18) share one session: a program started while it runs is sent
to it (REQ_RUN) and WINSHELD starts it there. The Workplace Shell,
RS2SHELL and APPINST know the types; the Workplace Shell's Session page
and RS2SHELL's program settings offer *WIN-OS/2 window (seamless)* (the
common session), a separate seamless session and *WIN-OS/2 full
screen*. APPINST's *WIN-OS/2 Window* (IBM's
`<WP_WIN2WIN>`: Program Manager) and the accessories' objects are
PROG_31_ENHSEAMLESSCOMMON, as IBM's INI.RC has them. `DESKTEST WIN`
starts Clock from such an object through RS2SHELL and Notepad with
DosStartSession into the same session.

How it differs from IBM's: OS/2's seamless driver (SWINVGA) draws into
the real framebuffer, clipped to what PM gives it, and PM's display
driver cuts Windows' windows out of the desktop. rs2's DOS sessions have
an emulated screen, so Windows draws its whole desktop into the shared
surface and PM shows the windows' parts of it as PM windows, which also
gives them PM's window management (the window list, z-order with OS/2
programs). WINSHELD, WINSMSG and IBM's USER run unchanged.

## Installing

APPINST's *Win-OS/2* entry copies the English Win-OS/2 as IBM's
installer lays it out: `C:\OS2\MDOS\WINOS2` and `SYSTEM` from the
packed files WINENVFP (the fix pack's kernel, WINOS2.COM, WINSHELD,
taken first), WININIS (the INI files), WINENV (disks 20-23, W3, W5),
WINACCES, WINSOUND, WINREADM, WINSCRSV and the single-file packs on
DISK_W1/W2/W5 (drivers, fonts, keyboard and language DLLs): 286 files,
about 9 MB. The OS/2-side files (WINBASE: VWIN.SYS, VW32S.SYS,
PMCLIP.DLL, PMDDE.DLL, WINPRF.DLL) are left out; rs2 does not use them.

Then it makes RS2DISP.DRV from the installed VGA.DRV and sets SYSTEM.INI
(`ini` lines): `display.drv` and `sdisplay.drv` RS2DISP, the VGA fonts,
no keyboard DLL for the US layout, `useos2shield=1`. Objects: *WIN-OS/2
Full Screen* and *WIN-OS/2 Window* (seamless Program Manager) in Command
Prompts, *WIN-OS/2 Desktop in a Window* (the whole Windows desktop in a
PM window), and a Win-OS/2 folder with Clock, Notepad, Calculator,
Write, Paintbrush, File Manager and Control Panel, seamless.

Not checked end to end: APPINST against the CD in QEMU. The test images
used the same files (the probe's install script and mkrs2disp).

## Gaps

| Gap | Size | Notes |
|-----|------|-------|
| Clipboard and DDE with OS/2 | medium | WINSCLIP/WINSDDE use VWIN's pipes (INT 66h E0h-E5h: open, write, read, pass data; VWIN.SYS's DO_*CLIPBOARD and _DDE* routines) and PMCLIP/PMDDE on the PM side. rs2's pipes give handles and pass nothing, and the notification turns both off (flags 0101h). A bridge between the pipe messages and PM's clipboard (WinSetClipbrdData) in PMVIOWIN would bring the clipboard; DDE needs PMDDE's conversation protocol. |
| PM windows in Windows' z-order | small | VWIN's code 1 from PM makes "shadow" windows in Windows for PM windows above Windows ones, so that Windows does not paint where a PM window covers one of its windows; rs2 does not need them (each Windows window has its own PM window), but Windows' mouse capture and drag outlines do not know where PM windows are. |
| Drag outlines inside other Windows windows | small | Shown, as part of those windows' surface; outlines over PM windows are inverted on the screen and may leave marks if a PM window repaints meanwhile. |
| Speed | medium | Starting takes seconds on dosrun (~100 MIPS) and about two minutes under QEMU TCG (~6 MIPS). The emulator has no decode cache yet (docs/DOS.md). |
| Sound | small to medium | SOUND.DRV/MMSYSTEM work with no device; the CD's SNDBLST.DRV with rs2's Sound Blaster 2.0 emulation is untested. |
| Printing | large | No printers in rs2's Win-OS/2. |
| DOS programs from Windows (WINOLDAP, PIFs) | medium | INT 21h 4Bh (exec) from protected mode is still reflected without translation. |
| 32-bit DPMI clients and the DOS API | small | The direct path is for 16-bit clients. |
| RS2DISP | small each | No palette management (256-colour programs get the fixed palette), RLE DIBs, device fonts, ENUMOBJ. |
| The OS/2 *Desktop* icon | small | WINSHELD's GoPM icon (INT 66h E500h with VVID's "switch to PM") does not switch to the PM desktop yet. |

## Testing

- `cargo test -p dos --test winos2`: RS2DISP's raster operations and
  palette, VWIN's shield messages and requests; `cargo test -p
  win16drv`: the NE generator on a made-up VGA.DRV.
- dosrun with a Win-OS/2 tree (APPINST's files, RS2DISP.DRV from
  `mkrs2disp`): `dosrun --c TREE --cwd 'C:\OS2\MDOS\WINOS2' [--display
  1024x768] [--seamless] [--vwin-trace] [--disp-trace] --keys '{wait
  14}{dclick 211 140}{run NOTEPAD.EXE}{shot F.png}' --png F.png
  'C:\OS2\MDOS\WINOS2\WINOS2.COM' [PROGRAMS]`.
- QEMU: `DESKTEST WIN` (seamless objects), the *WIN-OS/2* objects, or
  `SET WINOS2_SEAMLESS=1` before WINOS2.COM in a windowed command prompt;
  `--virtio-tablet` with `{mouse_abs x y}` for the pointer.
