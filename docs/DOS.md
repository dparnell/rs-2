# DOS sessions (MVDM)

OS/2 runs DOS programs in *virtual DOS machines*: each DOS session is a
virtual 8086 (V86-mode) task with its own first megabyte, DOS kernel and
virtual devices, shown full screen or in a PM window, and DPMI 0.9 lets
DOS-extended programs run in protected mode inside it. rs2's kernel runs
in long mode, where V86 mode does not exist, so rs2 runs DOS code on an
x86 CPU emulator. Everything around the CPU (BIOS, DOS, the devices, the
DPMI host, COMMAND.COM) is rs2's own Rust code that runs outside the
emulated CPU, the way DOSBox does it. IBM's DOS files (`OS2\MDOS`,
`DOSKRNL`, `COMMAND.COM`, `VDPMI.SYS`) are used only to compare
behaviour; none of them is needed or shipped.

## Pieces

| Path | What |
|------|------|
| `crates/x86emu` | `no_std` CPU emulator: 8086-80386 real mode and protected mode (16/32-bit segments, descriptors, privilege levels, exceptions), x87 FPU. Host-tested. |
| `crates/dos` | `no_std` virtual machine around the CPU: memory map, PIC, PIT, keyboard controller, CMOS, VGA/EGA/CGA, PC speaker, BIOS, DOS kernel, XMS, EMS, mouse driver, DPMI host, rs2's COMMAND.COM. Talks to the outside world through a `Host` trait. Host-tested. |
| `tools/dosrun` | Host program running DOS programs on the host's files and terminal (`cargo run -p dosrun -- PROG.COM`), for quick tests without QEMU. |
| `userland/vdm` | `VDM.EXE`: the OS/2 program that is a DOS session. Implements `Host` with OS/2 calls (files, VIO, KBD, MOU, timers, audio) and runs the machine. |
| `kernel` | Starting DOS sessions (DosStartSession types 4 and 7, DOS programs from CMD and program objects), the session's pixel screen for graphics modes, raw scan codes for DOS sessions. |
| `userland/pmviowin` | Also shows windowed DOS sessions, text and graphics. |
| `tests/dos` | Small DOS test programs (assembler source, built by xtask). |

## Where the emulator runs: a user process

A DOS session is an ordinary OS/2 process, `VDM.EXE`, the leader of an
ordinary session. The CPU emulator, the DOS machine and the guest's
memory all live in that process.

The alternative was a kernel-side session thread. It would save the API
calls for screen and keyboard and run 64-bit code, but:

- **The kernel is not preemptive and runs kernel threads under the big
  kernel lock.** An emulated CPU is CPU-bound by nature; in the kernel it
  would have to drop the lock and yield every few hundred microseconds by
  hand, and a slip (a long string instruction, a tight DPMI loop) would
  stall every API call in the system. In user mode the timer simply
  preempts it, priorities and DosSetPriority apply, and several DOS
  sessions share the processors like any other programs, on every CPU at
  once.
- **Containment.** Guest memory (up to the DPMI limit, tens of MB) is
  private process memory; a bug in the emulator or a DOS program that
  crashes it ends one session with an error, not the system.
- **Reuse.** Being a session leader gives a DOS session everything OS/2
  sessions already have: its own virtual screen and keyboard queue,
  Ctrl+Esc and Alt+Esc, the window list, Alt+Home between full screen and
  window (the windowed VIO path: `kernel/src/viowin.rs`,
  `PMVIOWIN.EXE`), termination queues, DosStopSession, killing from the
  window list, and every file system through DosOpen and friends with
  OS/2's share modes, so a DOS program and an OS/2 program see the same
  files the same way.
- **Testing.** The CPU and DOS layers are `no_std` crates with a `Host`
  trait, so they run unchanged in host unit tests and in `dosrun`.

The cost is that the emulator is 32-bit code (i686, as all rs2
userland) and pays an API call for each screen update and key; both are
batched (shared screen buffers, one call per frame, events read in
bulk), so this does not matter next to the instruction emulation.

OS/2's own MVDM is a kernel component (V86 tasks, virtual device drivers
`V*.SYS`); rs2 keeps its visible behaviour (sessions, settings, what a DOS
program sees) and not its structure.

## The machine

### Memory

Guest memory is one block of the VDM process, addressed by the guest's
linear (= physical: paging is not emulated) address:

| Linear | What |
|--------|------|
| 0x00000-0x003FF | Interrupt vector table |
| 0x00400-0x004FF | BIOS data area (kept current by the BIOS code) |
| 0x00500-0x005FF | DOS communication area |
| 0x00600-...     | DOS kernel data: SYSVARS (list of lists), SFT, CDS, DPB, buffers, the DOS swappable area, then the first MCB |
| ... -0x9FFFF    | Conventional memory, 640 KB, in MCB chains |
| 0xA0000-0xBFFFF | Video memory window (VGA planes, CGA/MDA text) |
| 0xC0000-0xC7FFF | Video BIOS (ROM: the font tables, the INT 10h entry stub) |
| 0xC8000-0xEFFFF | Upper memory blocks (DOS=UMB) and the EMS page frame (64 KB at 0xE0000) |
| 0xF0000-0xFFFFF | System BIOS (ROM: entry stubs, the configuration table, the reset vector) |
| 0x100000-0x10FFEF | HMA (A20 on), DOS=HIGH puts DOS data there |
| 0x110000-...    | Extended memory: XMS blocks and DPMI memory, from one pool |

The pool size comes from the session's DOS settings (`XMS_MEMORY_LIMIT`,
`DPMI_MEMORY_LIMIT`, `EMS_MEMORY_LIMIT`: as on OS/2, 3 MB DPMI, 2 MB EMS
and 3 MB XMS by default) and is allocated on demand, so a session that
never uses DPMI does not pay for it. A20 is a flag of the memory
interface: with it off, real-mode addresses wrap at 1 MB.

EMS (LIM 4.0, INT 67h) pages are 16 KB blocks of the pool; mapping a page
into the frame copies nothing, the memory interface translates the frame's
four windows to the mapped pages. XMS (INT 2Fh 4300h/4310h) serves the HMA,
UMBs and extended memory blocks with move (11h) done by the host.

### ROM stubs and callbacks

Every interrupt the BIOS or DOS provides has a real vector in the IVT
pointing to a stub in ROM: `FE 38 nn nn IRET` — an invalid opcode
encoding that the emulator reports as callback `nnnn` to the machine,
which runs the Rust handler on the guest registers and resumes after it.
Programs can hook vectors, chain to the old one, call `INT 21h` from an
`INT 21h` hook, or `PUSHF; CALL FAR old` exactly as on DOS. The same
mechanism ends programs (the return address of the PSP), implements the
XMS and DPMI entry points and real-mode callbacks, and runs rs2's
COMMAND.COM (a small `.COM` file that is a callback).

### DOS and BIOS services

All served in Rust, on the guest's registers and memory:

- **INT 21h**: the DOS 5 set COMMAND.COM-class programs use, with the
  version OS/2's VDM reports (major 20 for OS/2 2.x and later, minor
  from the OS/2 version; `AX=3306h` gives the same), checked against
  IBM's DOSKRNL. Files and directories (handles: 3Ch-46h,
  5Ah-5Bh, 67h, 6Ch; FCBs 0Fh-24h, 27h-29h for old programs; find
  first/next with the DTA; drives and current directories per drive,
  `IOCTL` 44h for devices and drives), memory (48h-4Ah, 58h with UMB
  link and strategy), processes (4Bh load/exec/overlay, 4Ch/31h/00h,
  4Dh, 50h/51h/62h, 55h/26h), the console (01h-0Ch through STDIN/STDOUT
  and CON), Ctrl+C (INT 23h) and critical errors (INT 24h), country
  information from the system's COUNTRY settings, date and time, the
  DOS internal tables (52h list of lists, 5Dh06h swappable area) that
  TSRs and COMMAND.COM-alikes peek at.
- **Files** go to rs2's file systems through the `Host` trait (DosOpen,
  DosRead, DosFindFirst...): the DOS session sees OS/2's drives, with 8.3
  names (long HPFS/JFS names are not visible to FCB and find calls in
  short form, as on OS/2: they are skipped). The SFT and job file table
  are real guest structures (PSP handle table at PSP:18h, `SetHandleCount`
  67h), each SFT entry backed by an OS/2 file handle.
- **INT 10h** video BIOS (modes 0-7, 0Dh-13h, cursor, scrolling, teletype,
  palette and DAC, fonts 8x8/8x14/8x16; VBE 2.0 with 8-bit modes 100h,
  101h, 103h, 105h through a banked window at A000h and a linear
  framebuffer at E0000000h), **INT
  16h** keyboard (00h-12h, buffer in the BDA at 40:1E, fed by IRQ 1 / INT
  9 with real scan codes), **INT 13h** (floppy and hard disk: answers "no
  drive" for direct disk access — OS/2 does not let VDMs at raw disks by
  default either), **INT 1Ah** clock (BDA tick count from the PIT), **INT
  15h** (A20, 88h extended memory = 0 when XMS is present, 87h block move,
  C0h configuration, 86h wait, 4Fh key intercept), **INT 11h/12h**,
  **INT 2Fh** (multiplex: XMS 43h, DPMI 1687h, 1680h idle, OS/2 4010h
  version query, `ISWINDOW`-style queries), **INT 33h** mouse driver
  (reset, show/hide, position, buttons, event handler 0Ch/14h, text and
  graphics cursors drawn by the driver), **INT 67h** EMS.

### Devices

Emulated in the machine, with the I/O ports programs touch directly:

- **8259 PIC** pair (ports 20h/21h/A0h/A1h): masks, EOI, the vector base.
- **8254 PIT** (40h-43h): channel 0 drives IRQ 0 at the programmed rate
  (18.2 Hz default; games reprogram it up to several kHz), channel 2 the
  speaker. Time is the host's: the VDM reads a millisecond (or finer)
  clock between instruction slices and delivers the IRQ 0s that are due,
  catching up a bounded number, so the BIOS tick count, `INT 1Ah` and
  timing loops follow real time.
- **8042 keyboard controller** (60h/64h): IRQ 1 delivers set-1 scan codes
  (make and break, E0 prefixes) to INT 9, which programs may hook; the
  BIOS INT 9 handler translates them into the BIOS buffer.
- **CMOS/RTC** (70h/71h): date and time from the host.
- **Video**: a VGA (sequencer, graphics controller, CRTC, attribute
  controller, DAC at 3C0h-3DFh, planar memory at A0000h with write modes
  0-3 and read modes 0-1, latches, chain-4, mode X) that also covers EGA
  and CGA modes, and MDA/CGA text at B000h/B800h.
- **PC speaker** (61h + PIT channel 2) and a **Sound Blaster 2.0** (DSP
  2.01 at 220h, IRQ 7, 8-bit single-cycle and auto-init DMA on channel 1
  of an 8237; `BLASTER=A220 I7 D1 T3`), both as PCM to the host's audio.
  No FM synthesizer (AdLib) yet.
- Ports nobody emulates read 0xFF and ignore writes (logged once).

### Screen

The machine keeps the video state; the VDM shows it through the session:

- **Text modes** (0-3, 7, 25/43/50 rows): the cells at B800h (B000h for
  mode 7) are copied into the session's VIO physical buffer
  (`VioGetPhysBuf`), the cursor position and shape go through
  `VioSetCurPos`/`VioSetCurType`, the mode through `VioSetMode`. So the
  kernel's existing text screen (full screen) and `PMVIOWIN` (windowed)
  show a DOS session in text mode with no new code, and blinking, the
  palette and fonts behave as for OS/2 text sessions.
- **Graphics modes** (CGA 4-6, EGA 0Dh-10h, VGA 11h-13h, mode X, VBE):
  the VDM renders the planar or packed video memory into an 8-bit
  indexed picture with a 256-entry palette (only scan lines whose memory
  changed). The kernel gives the session a *pixel screen*, a shared
  object the VDM writes (`Rs2SetPixelMode` / `Rs2UpdatePixels`, RS2API):
  while the session is in the foreground the kernel scales it (integer
  factor, centred, aspect corrected for 200-line modes) into the
  framebuffer; when the session is windowed `PMVIOWIN` reads it
  (`Rs2QueryPixels`) and draws it with `GpiDrawBits` into its window. A
  generation counter wakes PMVIOWIN as for text changes.

### Keyboard and mouse

A DOS program needs scan codes, key releases included, so DOS sessions
get raw input: the kernel queues set-1 scan code bytes for a session
marked as a DOS session (`Rs2ReadScanCodes`) — from the PS/2 or USB
keyboard when it is in the foreground, from PMVIOWIN (which gets PM's
WM_CHAR scan codes, with KC_KEYUP) when it is windowed. Hot keys
(Ctrl+Esc, Alt+Esc, Alt+Home) stay the kernel's. The mouse comes from the
session's mouse queue (MouReadEventQue: full screen, and windowed through
PMVIOWIN as for VIO sessions) and is scaled to the mode's virtual screen
for INT 33h.

### Sound

The speaker's square wave (PIT channel 2 gated by port 61h bits 0-1) is
synthesised at 22050 Hz into a stream on rs2's mixer (`Rs2AudioOpen`,
RS2API 300-308); the Sound Blaster's DMA blocks go to a stream of their
own at the rate the program set.
Without sound hardware the session is silent.

### Timing and idle

The VDM runs the CPU in slices (a few thousand instructions), then
services the host side: elapsed time to PIT ticks, input events, screen
updates (at most about 60 a second and only when video memory changed).
A guest that waits — `HLT`, `INT 16h` with no key, `INT 2Fh AX=1680h`,
`INT 28h`, a DOS console read — makes the VDM sleep until the next timer
interrupt is due or input arrives, so an idle DOS prompt takes no
processor time (OS/2's `IDLE_SENSITIVITY` idea, with detection of the
common polling loops: repeated `INT 16h AH=01h`/`11h` without a key).

## Protected mode and DPMI 0.9

The CPU emulates 80386 protected mode: GDT, LDT, IDT, TSS (only for the
stack switch on privilege changes), 16- and 32-bit code and stack
segments, expand-down data segments, call gates, the exceptions with their
error codes (#GP, #SS, #NP, #PF is never raised without paging, #UD, #DE,
#DB on TF), `IRET` to lower privilege, and CR0.PE switches. Paging is not
emulated: DPMI hosts may do without it and the DPMI host here owns the
linear address space.

The DPMI 0.9 host is in Rust (`crates/dos::dpmi`), entered through the
`INT 2Fh AX=1687h` entry point (a callback). It sets up a GDT and IDT of
its own in host memory above the pool, gives the client an LDT (selectors
with TI=1, RPL 3), and implements INT 31h: LDT descriptor management
(0000h-000Ch), DOS memory (0100h-0102h), interrupt and exception vectors
(0200h-0205h), translation services (0300h-0306h: simulate real-mode
interrupt/call, real-mode callbacks, raw mode switch), version 0400h,
memory blocks (0500h-0503h, 0600h-0604h as no-ops, 0800h physical address
mapping for the video memory), the virtual interrupt flag (0900h-0902h),
debug registers (0B00h-0B03h, stubs). Software interrupts the client does
not hook are reflected to real mode (INT 21h with pointer translation is
the extender's job, as DPMI specifies); hardware interrupts go to the
client's protected-mode handler first, then to real mode; an IRQ the
client hooked that arrives while the CPU runs real-mode code (a
reflected DOS call) reaches its protected-mode handler too, through a
real-mode stub the host puts in the IVT meanwhile. Programs bound to
DOS/4GW, PMODE/W, DOS/32A or CWSDPMI-style extenders run on it.

## Starting DOS sessions

- **From CMD**: a program that is a DOS executable (`.COM`, or an `.EXE`
  whose MZ header has no NE/LX/PE header behind it) starts a DOS session
  (DosStartSession type 7 when CMD is windowed, 4 when full screen), as
  OS/2's CMD does; `.BAT` files run in a DOS session under COMMAND.COM.
- **DosStartSession** `SSF_TYPE_VDM` (4) and `SSF_TYPE_WINDOWEDVDM` (7),
  and `SSF_TYPE_DEFAULT` for a DOS executable, start `VDM.EXE` as the
  session leader with the program and its arguments; the environment's
  DOS settings (`DOS_SETTING=value` lines, as OS/2 passes them in
  `PgmInputs` / the program object's environment) configure the machine.
- **Program objects** of type `PROG_VDM` / `PROG_WINDOWEDVDM` on the
  desktop start the same way.
- **Alt+Home** switches a DOS session between full screen and its window;
  a DOS session in a graphics mode can be windowed too.

The DOS session ends when its first program (or the `COMMAND.COM` it
started) exits; `EXIT` at rs2's COMMAND.COM ends it.

## COMMAND.COM

rs2's COMMAND.COM is a callback stub on disk (`C:\OS2\MDOS\COMMAND.COM`)
whose shell is Rust code in `crates/dos` that runs commands through the
same DOS calls a real shell uses (it runs child programs through 4B00h,
so `COMSPEC`, TSRs and programs shelling out with `COMMAND /C` work).
Internal commands: DIR, CD/CHDIR, MD/MKDIR, RD/RMDIR, COPY, DEL/ERASE,
REN/RENAME, TYPE, ECHO, SET, PATH, PROMPT, VER, VOL, CLS, DATE, TIME,
EXIT, LH/LOADHIGH, TRUENAME, and batch files with IF (ERRORLEVEL,
EXIST, ==, NOT), GOTO, FOR, CALL, SHIFT, PAUSE, REM, `@`; redirection
`<`, `>`, `>>` and pipes `|` (through temporary files, as DOS does).

## Phases

1. This document.
2. `crates/x86emu`: real mode 8086-80386, host tests per instruction
   group; then protected mode.
3. `crates/dos`: loader, PSP, environment, INT 21h, memory, BIOS,
   devices, `dosrun`; COMMAND.COM.
4. `VDM.EXE` and the kernel side: sessions full screen and windowed,
   graphics, keyboard, mouse, speaker; CMD and the desktop.
5. DPMI 0.9 and an extender test; CPU performance if needed.

Test results are kept in [COMPATIBILITY.md](COMPATIBILITY.md#dos-sessions).
