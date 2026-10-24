# Win-OS/2

Win-OS/2 is IBM's Windows 3.1 for OS/2: Microsoft's Windows 3.1 modules
(USER, GDI, the drivers, the accessories) with IBM's own kernel
(`OS2K386.EXE` in place of `KRNL386.EXE`), loader (`WINOS2.COM` in place
of `WIN.COM`) and OS/2 glue (WINSHELD, the seamless display drivers,
the clipboard and DDE bridges). On OS/2 it runs as a DPMI client in a
DOS session, served by OS/2's virtual device drivers (VDPMI, VDPX, VWIN,
VVGA, VMOUSE).

rs2 never ships it. It is on the user's OS/2 CD, and APPINST copies it
from there (category *Win-OS/2*), as it does QBasic.

## Status

**Full screen works.** In a DOS session (rs2's DOS machine, docs/DOS.md)
`WINOS2.COM` starts the Win-OS/2 kernel, which loads GDI, USER, the VGA,
keyboard, mouse, sound and comm drivers, the fonts (bitmap, TrueType
and ATM) and Program Manager, in 640x480 with 16 colours. Checked:

| What | dosrun (host) | QEMU (rs2, full-screen DOS session) |
|------|---------------|-------------------------------------|
| Program Manager and its groups | works | works (about 2 minutes to start under TCG) |
| Clock, Notepad, Calculator, Write, Paintbrush, Control Panel | work | not tried one by one (same code path) |
| File Manager | works (after the INT 25h fix) | not tried |
| Keyboard (menus, File > Run) | works | works |
| Mouse | (dosrun has no mouse) | moves and clicks reach Windows; the pointer does not follow QEMU's absolute pointer exactly (see gaps) |
| `WINOS2.COM PROGRAM.EXE` | runs the program, Program Manager minimised | - |
| WINSHELD (`useos2shield=1`, IBM's default) | Windows starts but runs no shell | - |

Screenshots of each step were kept in the probe's progress folder
(`work/progress/winos2`).

## What it took

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
   selectors in DS/ES. VDPX copies buffers through a real-mode transfer
   area; rs2 does not need to: its DOS is Rust that reads guest memory,
   so it now reaches memory through the segment registers' *bases*
   (`seg_lin`) and serves the call directly on the client's selectors
   while the CPU stays in protected mode. Only calls that take or return
   segments need translating: 25h/35h (protected-mode vectors), 48h-4Ah
   (DOS memory with a selector, as 0100h-0102h), 1Ah/2Fh (the DTA by
   selector), 50h/51h/62h, 26h/55h (PSPs by selector), and every table
   pointer DOS returns (34h, 52h, 1Fh/32h, 5Dh06h...) becomes a selector
   (one per segment, as 0002h now gives). INT 10h-12h, 15h, 16h, 1Ah and
   33h are served the same way (VDPX translates 10h, 15h and 33h). If a
   real-mode program hooked one of those vectors, the call is reflected
   as before.
3. **VDPMI's INT 31h AX=0010h**: allocate between BX and CX contiguous
   selectors (OS/2-only; the kernel uses it for its selector arena).
4. **Load-high EXEs** (`dos/proc.rs`). `OS2K386.EXE` has no minimum and
   no maximum allocation, which DOS takes as "load high": the program
   owns the whole largest block, its image at the top. The kernel takes
   the end of its global heap from PSP:2; loaded low, it believed the
   heap ran into the VGA window at A0000h and put Clock's data there.
5. **INT 25h/26h** return with RETF and leave the flags on the caller's
   stack, as DOS's do (File Manager's POPF picked up a word with TF set).
6. **Mouse event handlers in protected mode**: MOUSE.DRV sets its event
   handler with INT 33h 0014h from protected mode; the mouse driver now
   far-calls it in protected mode (`dpmi_call_pm`).

Diagnostics added on the way: dosrun's `--trace` shows file opens, INT
31h results and client exceptions with segment bases and the stack;
`dosrun --watch ADDR` logs every write to an address. `tools/ghidra`
loads OS/2's VDDs (LX) with their `.sym` files; Win-OS/2's 16-bit NE
modules load with Ghidra's NE loader (symbols from
`debug/symbols/os2/mdos/winos2`).

## Installing

APPINST's *Win-OS/2* entry copies the English Win-OS/2 as IBM's
installer lays it out: `C:\OS2\MDOS\WINOS2` and `SYSTEM` from the
packed files WINENVFP (the fix pack's kernel, WINOS2.COM, WINSHELD,
taken first), WININIS (the INI files), WINENV (disks 20-23, W3, W5),
WINACCES, WINSOUND, WINREADM, WINSCRSV and the single-file packs on
DISK_W1/W2/W5 (drivers, fonts, keyboard and language DLLs): 286 files,
about 9 MB. The OS/2-side files (WINBASE: VWIN.SYS, VW32S.SYS,
PMCLIP.DLL, PMDDE.DLL, WINPRF.DLL) are left out; rs2 does not use them.

Then it sets SYSTEM.INI as IBM's installer does for a VGA display
(`display.drv=vga.drv`, `sdisplay.drv=swinvga.drv`, the VGA fonts, no
keyboard DLL for the US layout) through APPINST's new `ini` lines, and
`useos2shield=0` (see WINSHELD below). Objects: *WIN-OS/2 Full Screen*
in Command Prompts (`<WP_WINFS>`, as INI.RC has it) and a Win-OS/2
folder with Clock, Notepad, Calculator, Write, Paintbrush, File Manager
and Control Panel (`WINOS2.COM PROGRAM.EXE`, full screen).

Not checked end to end: APPINST against the CD in QEMU. The 286 file
lines were checked against the packed files' contents, and the probe's
test image used the same files.

## Gaps

| Gap | Size | Notes |
|-----|------|-------|
| WINSHELD, the OS/2 shield | medium (1-2 weeks) | With IBM's default `useos2shield=1` USER starts WINSHELD.EXE instead of Program Manager; it talks to VWIN.SYS through INT 66h (functions 8000h, 8001h, 8014h, 80B7h, 80FFh, 8A00h, E003h, E500h: registration, the program to start, messages) and INT 2Fh AX=4010h/4011h (OS/2 version, VDD entry points by name: `VWIN`, `VXMS`), and waits. rs2 answers none of these, so APPINST turns the shield off. A small VWIN in the DOS machine (start the shell, answer the queries) would bring back the OS/2 "Desktop" icon and switching. |
| Mouse position | small (days) | Events and clicks arrive, but MOUSE.DRV moves its pointer by the mickey counts (SI/DI), which rs2 derives from absolute positions with the 8/16 mickey ratio, so vertical motion is doubled and the pointer drifts from QEMU's (and VMware's) absolute pointer. OS/2's VMOUSE gives Win-OS/2 absolute positions; rs2 should do the same for a protected-mode handler (or scale mickeys 1:1). |
| Speed | medium | Starting takes seconds on dosrun (~100 MIPS) and about two minutes under QEMU TCG (~6 MIPS); KVM/HVF-class hardware will be near dosrun. The emulator has no decode cache yet (docs/DOS.md). |
| Higher resolutions | small to medium | Only VGA 640x480x16 tried. The CD has XGA and 8514 drivers (hardware rs2 does not emulate) and SuperVGA drivers for S3, WD, ATI Mach etc.; rs2's VBE modes are not used by any of them. A Windows VBE driver from elsewhere, or emulating one chip, would be needed. |
| Clipboard and DDE with OS/2 | medium | WINSCLIP/WINSDDE talk to VWIN.SYS and PMCLIP/PMDDE on the PM side; needs the VWIN pipes. |
| Sound | small to medium | SOUND.DRV/MMSYSTEM work with no device; the CD's SNDBLST.DRV/SNDBLST2.DRV with rs2's Sound Blaster 2.0 emulation are untested. |
| Printing | large | No printers in rs2. |
| DOS programs from Windows (WINOLDAP, PIFs) | medium | INT 21h 4Bh (exec) from protected mode is still reflected without translation. |
| 32-bit DPMI clients and the DOS API | small | The direct path is for 16-bit clients; for a 32-bit client that asks for the extensions INT 21h is still reflected untranslated (DOS would need 32-bit offsets in EDX/ESI/EDI). |
| Windowed and seamless | see below | |

## Seamless Win-OS/2

On OS/2, seamless Win-OS/2 puts each Windows program's windows on the
PM desktop. It is a joint effort of four parts:

- **A seamless display driver** (`sdisplay.drv`, SWINVGA.DRV for VGA):
  a Windows display driver that draws straight into the *real*
  framebuffer, clipped to the areas PM gives it, and takes and releases
  the screen through VVGA/VWIN (the WINDD calls in VWIN.SYS: request
  video controller, RAM semaphore, critical sections; INT 2Fh 4003h/4004h,
  which the full-screen VGA.DRV calls too).
- **VWIN.SYS**, the bridge: pipes between WINSHELD in the session and
  the PM side (PMSHIELD calls: the Windows windows' positions and
  z-order, focus, the pointer, messages), clipboard and DDE.
- **WINSHELD.EXE** in Windows, hooking window changes and reporting them.
- **The PM side**: the Workplace Shell starts the session with
  `PROG_31_ENHSEAMLESSCOMMON`/`...VDM`; PM and its display driver cut
  the Windows areas out of the desktop and route input to the session.

rs2's DOS sessions have an emulated VGA, not the real framebuffer, so
the IBM seamless driver's model (direct access to the PM screen) does
not carry over. Two ways to get there:

1. **Win-OS/2 in a PM window** (cheap: days). PMVIOWIN already shows a
   DOS session's graphics in a window with mouse and keys; Win-OS/2
   full screen is such a session. A *WIN-OS/2 Window* object of type
   WINDOWEDVDM would give a 640x480 Windows desktop in a PM window, with
   Alt+Home between window and full screen. Needs: the mouse position
   fix above (absolute positions in a window matter more), and checking
   the 16-colour planar picture's speed in PMVIOWIN. Not tried in this
   probe.
2. **True seamless** (large: 2-3 months). Emulate VWIN's PMSHIELD
   protocol well enough for WINSHELD (window rectangles, z-order, focus,
   pointer shape), and either run the seamless driver against the
   emulated VGA with rs2 compositing the reported window rectangles of
   the session's picture into PM windows (one PM frame per top-level
   Windows window, input routed back), or write rs2's own Windows
   display driver that hands GDI output to PM. The first reuses IBM's
   SWINVGA but needs a full-desktop-size emulated screen; the second is
   a Windows 3.1 DDK-class driver (bitblt, text, brushes, cursors) in
   16-bit code. Clipboard and DDE (WINSCLIP/WINSDDE through VWIN to
   PMCLIP/PMDDE, or rs2's own) belong to the same work.

Recommendation: do (1) next, with the mouse fix and a minimal VWIN for
WINSHELD; consider (2) only after that, as its own project.

## Next steps

1. Mouse: absolute positions for protected-mode handlers.
2. A minimal VWIN (INT 66h, INT 2Fh 4011h) so WINSHELD runs with IBM's
   default settings.
3. WIN-OS/2 Window: a WINDOWEDVDM object; check speed and input.
4. Check APPINST's Win-OS/2 entry against the CD in QEMU.
5. Exec (INT 21h 4Bh) from protected mode, for DOS programs started from
   Windows.
6. The emulator's speed (a decode cache), which decides how usable all
   of this is under QEMU TCG.
