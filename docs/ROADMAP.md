# rs2 roadmap

Status markers: done, in progress, planned.

## Milestone 1: unmodified text-mode programs (largely done)

- done: UEFI boot, 64-bit kernel, framebuffer text screen, memory management
- done: OS/2 address space (arenas, tiled LDT), processes, threads,
  scheduler (class and level priorities, MAXWAIT starvation boost,
  PRIORITY=/MAXWAIT=/TIMESLICE=), FPU/SSE state
- done: LX and NE loader, DLLs in the shared arena, DLL initialisation,
  API gate (16/32-bit conventions, DOSCALL1 aliases)
- done: files (VFS, FAT, OS/2 paths and wildcards, share modes), pipes,
  DosExecPgm/DosCWait, threads, 16- and 32-bit semaphores, exception
  handling, exit lists, timers, queues, shared memory, resources,
  Ctrl+C signals, module APIs, CONFIG.SYS processing
- done: VIO, KBD, MOU (PS/2 and USB mice, text pointer), MSG (message
  files and bound message segments), NLS (87 countries, 21 single-byte
  and the DBCS code pages, CODEPAGE=/COUNTRY=), national keyboard layouts
  (DEVINFO=KBD, KEYB)
- done: VIO/KBD completeness: text modes with 8x16/8x14/8x8 fonts (43,
  50, 60 rows), physical buffer, palette/DAC/blink state, pop-ups and the
  hard-error pop-up (DosError), save/redraw and mode notifications,
  replacement subsystems, logical keyboards and focus, line editing,
  Ctrl+S, PS/2 LEDs and typematic; device monitors (KBD$, MOUSE$)
- done: character devices COM1-COM4 (16550) with the ASYNC IOCtls and
  `MODE COMn`, LPT1-3, CLOCK$, MOUSE$/POINTER$, KBD$ and SCREEN$ IOCtls
- done: the original CMD.EXE runs interactively; userland/apitest passes
- done: DLL termination routines
- done: IBM REXX (.CMD scripts, REXXUTIL, the REXX API server) and most
  text-mode base utilities and Toolkit tools; see
  [COMPATIBILITY.md](COMPATIBILITY.md)
- done: named pipes (byte and message mode, DosTransactNPipe, DosCallNPipe)
- done: sessions: DosStartSession/Select/Set/StopSession, a virtual screen
  and keyboard queue per session, Ctrl+Esc window list, Alt+Esc
- done: the rest of the kernel API programs on the media use outside PM
  and DOS sessions (DosFSCtl, DosSysCtl, DosPerfSysCall, thread-local
  memory, signal exceptions between processes, must-complete sections,
  file locks, DosProtect*, DosAliasMem, ...); termination exceptions for
  handlers; see the coverage table in [COMPATIBILITY.md](COMPATIBILITY.md)
- done: kernel stress test (RS2\STRESS.EXE) and the leak, race and
  wake-up bugs it found; kernel object counters for leak checks
- done: kernel threads (no OS/2 process) with a work queue; the scheduler
  reports a thread that blocks while holding a spin lock
- done: windowed VIO sessions ("OS/2 Window"): SSF_TYPE_WINDOWABLEVIO
  and the default type for WINDOWCOMPAT programs from PM, `START /WIN`,
  the PMVIOWIN host (rendering, scroll bars, mode and font changes, Mark,
  Copy, Copy All, Paste, Close), keys and mouse into the session,
  Alt+Home between window and full screen, booting into PM with a
  command prompt window (`PROTSHELL=C:\OS2\PMVIOWIN.EXE /SHELL`)
- planned: AVIO (VioCreatePS & co. in PM windows, reusing
  `crates/viocell`), parallel port hardware,
  DosDevIOCtl coverage, mandatory file locks, finer-grained kernel locking
  (in place of the big kernel lock)

## Milestone 2: storage and installed systems (in progress)

- done: PCI, AHCI (SATA disks, ATAPI drives), MBR/GPT, FAT, ISO 9660
- done: boot from an installed disk (UEFI)
- done: NVMe (MSI-X/MSI completion interrupts, polled fallback)
- done: USB: xHCI (MSI-X/MSI interrupter 0), boot-protocol HID keyboard and mouse, hot-plug
- done: USB hubs (nested), USB mass storage (bulk-only), boot from a USB stick
- done: USB storage hot-plug: a kernel thread mounts sticks connected after
  boot on the next free drive letters and unmounts them on removal
- done: SCSI host adapters (MSI-X/MSI): VMware PVSCSI, LSI Logic MPT Fusion
  (53c1030, SAS1068); disks and CD/DVD drives
- done: VMware guest support: backdoor (host time), absolute pointer
  (vmmouse) for PM and text-mode MOU, SVGA II mode setting (`video=WxH`,
  also through GOP); PCI device list in the boot log
- done: SVGA II hardware cursor for PM (DEFINE_CURSOR, cursor registers)
- planned: VirtualBox VMMDev (absolute pointer), SVGA run-time mode
  changes under PM, testing on real VMware
- done: virtio (MSI-X per queue): PCI transport, modern and legacy devices, split
  virtqueues; virtio-blk (read-only, block size, flush; booting from it),
  virtio-scsi (disks, CD-ROMs, unit attentions), virtio-input (keyboards,
  mice, tablets through an absolute pointer path for text mode and PM),
  virtio-rng (\DEV\RANDOM$), virtio-console port 0 (\DEV\VIOCON$)
- done: virtio-gpu 2D (`virtio-vga`, `virtio-gpu-pci`): guest-RAM
  framebuffer for the text screens and PM, `video=WxH` (any size),
  changed rows flushed from the timer tick (whole screen under PM)
- done: virtio-gpu hardware cursor (cursor queue) for PM
- planned: PM dirty rectangles instead of
  full-screen flushes, display hot-resize events (EDID), run-time mode
  changes under PM
- done: networking (docs/NETWORK.md; receive interrupts on e1000e and
  virtio-net, the 8254x without MSI polled): Intel e1000/e1000e
  (8254x, 82574L) and virtio-net drivers; `crates/tcpip`, a host-tested
  stack (Ethernet, ARP, IPv4 with fragmentation and reassembly, ICMP,
  UDP, TCP with retransmission, congestion and window management, DHCP
  client, DNS resolver, loopback); kernel socket layer (per-process
  handles, blocking and non-blocking calls, select) exposed as RS2API
  200-217; NETTEST and PING; `net=`/NETWORK= configuration
- done: OS/2 socket DLLs (TCPIP32, SO32DLL, TCP32DLL, with TNLS32 and
  a profile-only PMSHAPI): sockets, select, ioctls, resolver (res_*,
  PTR), network database from %ETC%; IBM FTP, TELNET, RXSOCK and the
  Toolkit socket and RPC samples run; so_cancel and the socket list
  (RS2API 218/219); IFCONFIG, NETSTAT, HOSTNAME, HOST; the 16-bit
  TCPIPDLL and TNLS16 (thunk16: 16-bit entry points for Rust DLLs), IBM's
  16-bit TFTP runs; system-wide socket numbers with owners
  (addsockettolist/removesocketfromlist, RS2API 220/221): IBM's INETD
  hands connections to the servers it starts
- done: INI files for PMSHAPI (see milestone 4)
- planned: route/arp and run-time configuration,
  code page translation in TNLS32/TNLS16,
  vmxnet3/pcnet drivers, TCP keepalive/window scaling/SACK/urgent data
- planned: virtio-scsi hot-plug events, virtio-console multiport
- done: ACPI (MADT, FADT reset and S5), LAPIC timer, IOAPIC routing
- done: SMP: all processors started (INIT/SIPI), per-CPU GDT/TSS/LAPIC
  timer, shared run queues, big kernel lock, IPI wake-ups and TLB shootdown
- done: per-processor performance counters (time not idle, dispatches,
  interrupts) kept at every switch to and from a processor's idle
  thread: DosConfigurePerf/DosAliasPerfCtrs as OS/2's kernel lays them
  out (OS/2's Pulse shows them), DosPerfSysCall CMD_KI_RDCNT per
  processor
- planned: the other kernel performance blocks (memory, file system,
  threads, perfview's message queue), taking processors offline
- done: MSI/MSI-X (`msi`): device vectors 0x30-0xEF spread over the
  CPUs, per-vector masking, Event hand-off to sleeping threads; AHCI,
  NVMe, virtio, xHCI, e1000e, PVSCSI and MPT converted; `nomsi` keeps
  every driver polled; idle CPUs no longer take the run queue lock on
  every tick
- planned: several requests in flight per disk (the drivers still run
  one command at a time), interrupt coalescing, per-CPU queues, INTx
  routing through ACPI _PRT (AML) for devices without MSI
- done: HPFS (read/write/format)
- done: extended attributes on FAT12/16 (`EA DATA. SF`), FAT32
  (FAT32.IFS-style EA files) and HPFS, with the EA APIs, DosCopy/DosMove
  and CHKDSK checks
- planned: JFS, HPFS boot volumes, OS/2 LVM metadata

## Milestone 3: rs2 user space (in progress)

- done: `elf2lx` toolchain (Rust/i686 to LX executables and DLLs)
- done: CMD.EXE (internal commands, batch files, pipes, redirection),
  MORE, FIND, SORT, ATTRIB, TREE, XCOPY, INSTALL
- done: LABEL, CHKDSK (FAT check and /F repair; HPFS read-only scan),
  FORMAT (FAT, HPFS) on DASD handles and the category 8 disk IOCtls, MODE
  (screen mode through a 16-bit VIO thunk), HELP
- planned: CHKDSK repair for HPFS, FDISK/LVM and the rest of the base
  utilities; own message files (OSO001.MSG); Cyrillic/Greek/Hebrew
  keyboard layouts; REXX

## Milestone 4: Presentation Manager (started)

- done: design (docs/PM.md); kernel display/input services (RS2API) and a
  PM session; `crates/gpi` and `crates/pmwin`; PMWIN.DLL/PMGPI.DLL run the
  Toolkit's `hello.exe` (windows, painting, mouse moves, closing)
- done: several PM programs at once: PM state shared by all processes,
  the PM server (PMSRV.EXE) with a time-critical input thread routing
  every event straight to the target thread's queue, hardware or
  server-drawn pointer, not-responding programs handled by the server
  (moved, sized, hidden, placeholders, "(not responding)" titles,
  PM_HANGTIME), the Ctrl+Esc window list ending programs, clean-up when
  a PM process ends
- done: the OS/2 System Editor (E.EXE) runs unmodified: opening,
  editing, finding, fonts, word wrap, saving (with its .TYPE dialogs),
  the "save changes?" prompt, cut/copy/paste also between programs
- done: rs2's own System Editor (userland/e, `C:\OS2\E.EXE`) in the
  distribution, behaving as OS/2's E (studied with Ghidra): E's menus,
  ids, accelerators, dialogs and help table, its .TYPE rules, Ctrl+Z
  end-of-file mark and OS2.INI settings (EHXMAIN), the close and
  shutdown questions, `E /SELFTEST`, and its own help file (E.HLP,
  written in IPF and compiled by `crates/ipf`)
- done: the multi-line entry field (WC_MLE, its text engine host-tested
  in `crates/pmwin`), combo boxes, spin buttons, sliders and value sets;
  the clipboard and atom tables shared by all processes (CF_TEXT and
  other pointer formats, CF_BITMAP, registered formats, delayed
  rendering, owner and viewer); WinTrackRect; PMCTLS's file and font
  dialogs; PMDRAG, PMMERGE and LIBUNI as far as programs need them
  (`RS2\CTLTEST.EXE` checks the controls and the clipboard)
- done: windowed VIO sessions (see milestone 1); the PM window list lists
  full-screen sessions and starts command prompts; PM draws into a
  shadow buffer while a text session has the screen
- done: notebooks (WC_NOTEBOOK: major and minor tabs on any side, back
  pages, status line and page buttons, page windows and page dialogs,
  the Warp 4 tabbed dialog; pages and geometry host-tested in
  `crates/pmwin`; `RS2\NBTEST.EXE` checks them)
- done: foundations for a desktop shell: pointers and icons (the OS/2
  pointer/icon formats with multi-resolution arrays, RT_POINTER,
  WinLoadPointer/WinCreatePointer[Indirect]/WinDrawPointer, rs2's own
  system pointers and icons, the mouse pointer's shape in hardware or
  software, SS_ICON/SS_SYSICON, message box icons, frame icons and the
  mini icon on the system menu button, file icons from the .ICON EA and
  executables through PMWP.DLL); minimize and restore (frames become
  icons at the bottom of the desktop, also for programs that do not
  respond; a minimized window viewer hook for a shell); WC_CONTAINER
  (icon, name, text, detail and tree views, emphasis, selection styles,
  keyboard, scrolling, sorting, direct editing, moving icons; layout and
  selection host-tested); `RS2\SHELLTST.EXE` checks them
- done: rs2's own desktop shell (RS2SHELL.EXE, not the Workplace
  Shell): the desktop pinned below all windows, program groups as
  folders, Drives and directory windows (file icons, details view,
  subfolders), program objects in OS2.INI's program list with a
  settings notebook, pop-up menus, renaming, deleting, moving files and
  icons by dragging (positions kept), associations by extension, the
  Minimized Window Viewer, shutdown (WM_SAVEAPPLICATION/WM_CLOSE,
  DosShutdown, ACPI power off); the default boot of the distributed and
  installed systems, with T at the loader or `textmode` for the
  command prompt; `RS2\DESKTEST.EXE`
- planned (shell): file copy, shadows, a Startup folder, a directory
  tree view with subfolders, drag and drop with other programs
  (DrgDrag), then the SOM-based Workplace Shell
- done: profiles on real INI files (`crates/inifile`, PMSHAPI): OS2.INI
  and OS2SYS.INI from USER_INI/SYSTEM_INI, shared by all processes,
  written through safely; the program list calls, kept in OS2.INI for
  a later shell (docs/PM.md, Profiles); `RS2\INITEST.EXE`
- done: Unicode locales (LIBUNI.DLL on `crates/uls`: rs2's own system
  locale tables, user locales in USER.LOC, locale objects, items,
  UniStrftime, the `Locale` table export) and UCONV.DLL, as IBM's LIBUNI
  and LOCALE.DLL (studied with Ghidra); OS/2's Locale applet
  (GLOCALE.EXE) runs on them
- done: the help manager (HELPMGR.DLL: help instances, help tables, the
  help hook IBM's way, HM_ messages, a help window with contents, index,
  search and footnotes, on `crates/ipf`, which reads, lays out and
  compiles INF/HLP files) and VIEW.EXE for books; IBM's E.EXE shows its
  help
- done (help): dynamic data (the Ddf* API, `:ddf.`), panels in windows
  of their own (link and heading window data, groups, auto, dependent)
  and `:acviewport.` (the Toolkit's IPF sample runs), IPF fonts and real
  italic, MB_HELP, IBM's VIEW.EXE/VIEWDOC.EXE on rs2's HELPMGR
  (ordinals 26, 28-37, 45-47); `RS2\HMTEST.EXE`
- done: APPINST.EXE installs the OS/2 applications rs2 runs from the
  OS/2 Warp CD (a tree of categories and applications with check boxes,
  unpacking with `crates/os2-pack`, desktop folders and objects)
- planned (help): printing, bookmarks and the library list, the help
  window's menu, metafiles in dynamic data, viewport styles
- planned: per-process presentation-space limits, hooks other than the
  help hook (WinSetHook only records them), DDE, owner-drawn items (also in
  containers), presentation parameters other than the colours, drag
  and drop
- Display driver model (GRADD-style) on the framebuffer, mouse pointer
- PMWIN (windows, messages, controls, dialogs), PMGPI (graphics, fonts),
  PMSHAPI
- SOM runtime and the Workplace Shell

## Milestone 5: install media (first version done)

- done: hybrid ISO/USB image (El Torito EFI + MBR ESP), live session
- done: installer (GPT + EFI system partition, FAT32, copy, restart)
- planned: choice of partitions and file systems, BIOS boot, boot manager

## Later

- NetBIOS, NFS/SMB clients
- MMPM/2 multimedia, audio drivers
- DOS sessions (MVDM) through a CPU emulator, since long mode has no V86
- self-hosting (building rs2 on rs2): 64-bit processes with a Linux
  system-call layer for the Rust toolchain; see [SELF-HOSTING.md](SELF-HOSTING.md)
