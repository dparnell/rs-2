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
  Ctrl+C signals, module APIs, CONFIG.SYS processing, DosDebug (a debugger
  driving a traced program: Open Watcom's `wd`)
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
- done: AVIO (VioCreatePS & co. in PM windows, drawn with
  `crates/viocell`)
- planned: parallel port hardware,
  DosDevIOCtl coverage, mandatory file locks, finer-grained kernel locking
  (in place of the big kernel lock)

## Milestone 2: storage and installed systems (in progress)

- done: PCI, AHCI (SATA disks, ATAPI drives), MBR/GPT, FAT, ISO 9660
- done: PCI IDE (PIIX: ATA disks and ATAPI CD drives, PIO, legacy and
  native channels; Proxmox's and VMware's IDE CD drives); CD drives keep
  their letter without a disc and mount the one inserted or changed later;
  disk write caches flushed (NVMe FLUSH, FLUSH CACHE) at restart and
  shutdown
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
  virtio-rng (\DEV\RANDOM$), virtio-console with MULTIPORT (\DEV\VIOCON$,
  named ports \DEV\VDAGENT$ and \DEV\QEMUGA$; every controller)
- done: guest agent VMAGENT.EXE (docs/AGENT.md): clipboard text shared with
  the host through the SPICE agent protocol (QEMU's window, Proxmox, libvirt,
  UTM), VirtualBox's shared clipboard (VMMDev driver, HGCM) and VMware's
  copy/paste (GuestRPC, version 4 and the legacy version 1)
- done: QEMU guest agent QEMUGA.EXE (org.qemu.guest_agent.0): shutdown and
  restart through the Workplace Shell, addresses, OS, time, file system
  freeze (flush), files (docs/AGENT.md)
- next: run the VirtualBox and VMware backends on those hosts; bitmaps;
  guest-exec
- done: virtio-gpu 2D (`virtio-vga`, `virtio-gpu-pci`): guest-RAM
  framebuffer for the text screens and PM, `video=WxH` (any size)
- done: virtio-gpu hardware cursor (cursor queue) for PM
- done: damage-based display updates: PMWIN notes what it draws on the
  screen (gpi's surface keeps up to 16 merged rectangles) and hands it to
  the kernel when it lets go of its lock (`Rs2UpdateDisplay`); the text
  screens add the rows they draw; about 60 times a second VMware SVGA II
  sends an UPDATE and virtio-gpu a TRANSFER_TO_HOST_2D and
  RESOURCE_FLUSH per rectangle, and on a plain firmware framebuffer PM
  draws into a RAM back buffer whose damaged rectangles the tick copies
  to the screen (idle desktop at 1024x768: 24.5 Mpx/s before, 0 now;
  dragging a window: 1-3 Mpx/s)
- done: resolution changes under PM (SVGA II, virtio-gpu): PM maps
  memory for the largest mode, gets EV_MODE and resizes the desktop,
  the shell's desktop window and maximized frames; `C:\OS2\DISPLAY.EXE`
  (list of modes, Apply, keep or revert within 15 s) saves `VIDEO=WxH`
  in CONFIG.SYS, applied at boot; virtio-gpu display-change events
  switch to the host's new preferred size
- planned: VMware "fit guest" resizing (needs the tools RPC channel,
  `Resolution_Set`), write-combining (PAT) mappings of framebuffers on
  real hardware
- done: networking (docs/NETWORK.md; receive interrupts on e1000e,
  vmxnet3 and virtio-net through MSI/MSI-X, on the 8254x and PCnet
  through INTx routed by ACPI _PRT): Intel
  e1000/e1000e (8254x, 82574L), VMware vmxnet3, AMD PCnet (VMware's
  vlance) and virtio-net drivers (QEMU's e1000, e1000-82545em, e1000e,
  vmxnet3, pcnet and virtio-net tested;
  `xtask run --net MODEL`); `crates/tcpip`, a host-tested
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
- done: rs2's own TCP/IP applications (userland/netapps, NETWORK.md):
  FTP (active and passive), FTPD (TRUSERS), TELNET (VT100/VT220 through
  crates/vt100), TELNETD (a command prompt on pipes), LPR/LPQ/LPRM,
  FINGER, TFTP/TFTPD, REXEC, RSH and the TCPCFG2 notebook (NETWORK=,
  SET HOSTNAME=, RESOLV2, HOSTS); NETCLI with tools/scripts/nethost.py
  checks them; a TCP/IP folder on the desktop
- done: TELNETD with pseudo terminal sessions (kernel pty sessions,
  RS2API 18/19; VIO programs over telnet as VT100/ANSI), an LPD server
  into the spooler queues, TALK/TALKD, REXECD/RSHD, `rexec()` (TCPIP32,
  TCP32DLL, TCPIPDLL), code page translation (-cp) in FTP/TELNET/TELNETD/
  LPD and TNLS32's tables; TCPIPDLL's resolver, sendmsg/recvmsg,
  bsdselect, iterators and a real `_alarm`/`_xsignal` (IBM TFTP
  retransmits)
- planned: DBCS translation in TNLS32/TNLS16, a BOOTP/DHCP server, a
  SLIP/PPP dialer
- done: TCP out-of-order reassembly, SACK, window scaling and
  timestamps (PAWS), delayed ACKs, Nagle, keepalives and urgent data
  (MSG_OOB, SIOCATMARK, SO_OOBINLINE, select's exception set); a new
  SYN reuses a connection in TIME_WAIT; per-socket wait queues; a
  routing table (host/network routes, metrics), aliases, static and
  proxy ARP entries, IP forwarding (sysctl); ROUTE and ARP, IFCONFIG
  changing interfaces, NETSTAT with IBM's options, TCPCFG2 applying its
  Network page at once, DHCP release at shutdown (Rs2NetControl, RS2API
  240); the socket DLLs' route/ARP/interface/SIOSTAT ioctls and
  sysctl() (the Toolkit's sysctl and HPS samples); PCI INTx routing
  through a small ACPI namespace walker (`aml.rs`)
- planned: multicast/IGMP
- planned: virtio-scsi hot-plug events, virtio-console multiport
- done: sound output: Intel High Definition Audio (`drivers/hda.rs`, PCI
  class 04.03: ICH6+/ICH9, QEMU `intel-hda`/`ich9-intel-hda`, Proxmox,
  VMware "HD Audio"): controller reset, CORB/RIRB (immediate commands as
  a fallback), codec widget walk from line-out/speaker/headphone pins to
  a DAC, amplifiers unmuted at 0 dB, one 48 kHz 16-bit stereo output
  stream; the boot log names the codec and path. A software mixer
  (`audio.rs`) fills the stream's ring 0.1 s ahead of the hardware from
  the timer tick: any number of streams, each 4000-96000 Hz, 8-bit
  unsigned or 16-bit signed, mono or stereo, converted, resampled
  (linear interpolation) and summed with per-stream and master volume.
  RS2API 300-308 (`api/audio.rs`), for rs2's MDM.DLL; 32-bit `_System`,
  OS/2 error codes, stream handles per process (closed when it ends):

  | Ord | Call | |
  |-----|------|-|
  | 300 | Rs2AudioQuery(PULONG pulDevices) | 50 without sound hardware, else 1 device |
  | 301 | Rs2AudioOpen(ULONG ulRate, ULONG ulChannels, ULONG ulBits, PULONG phStream) | 50 without hardware |
  | 302 | Rs2AudioWrite(ULONG hStream, PVOID pData, ULONG cb) | queues; waits while more than 0.5 s is queued |
  | 303 | Rs2AudioQueryPos(ULONG hStream, PULONG pulBytes) | input bytes played |
  | 304 | Rs2AudioDrain(ULONG hStream, ULONG ulTimeoutMs) | waits until all has played; 640 on timeout |
  | 305 | Rs2AudioStop(ULONG hStream) | discards the queue (blocked writers return) |
  | 306 | Rs2AudioClose(ULONG hStream) | |
  | 307 | Rs2AudioSetVolume(ULONG hStream, ULONG ulVolume) | 0-100; handle 0 = master |
  | 308 | Rs2AudioPause(ULONG hStream, ULONG fPause) | |

  `cargo xtask run --sound` adds the QEMU devices (CoreAudio in the
  window), `--sound-wav FILE` records to a WAV file; AUDTEST.EXE checks
  the calls and `tools/scripts/audcheck.py FILE` the recording
- done: more sound hardware behind the same mixer (`audio::Output`, the
  first device found is used; the mixer resamples to its rate):
  - Intel ICH AC'97 (`drivers/ac97.rs`: ICH-ICH7, 440MX, AMD 768/8111,
    nVidia nForce; QEMU `AC97`, VirtualBox, Proxmox, older VMware):
    cold reset, primary codec at 0 dB unmuted, 48 kHz (set through VRA
    when the codec has it), PCM out over all 32 buffer descriptors with
    the last valid index kept behind the current one
  - Ensoniq AudioPCI (`drivers/es137x.rs`): the ES1370 (QEMU `ES1370`;
    AK4531 codec, DAC2 at 44100 Hz) and the ES1371/CT5880/EV1938
    (VMware's "Sound Blaster" `es1371`; AC'97 warm reset, sample rate
    converter set to 48 kHz on DAC2, codec at 0 dB), DAC2 looping a ring
  - `cargo xtask run --sound ac97` / `--sound es1370` (`es1371` is the
    same: QEMU has no ES1371). AUDTEST and audcheck pass on QEMU's HDA,
    AC97 and ES1370, Solitaire's sounds play on AC97; the ES1371 path is
    untested (reviewed against the register layout only)
- done: recording, one capture stream at a time (`audio::Input`,
  `audio::Capture`): HDA's first input stream from an ADC routed to a
  line-in or microphone pin (selectors, mixers, pin bias, amplifiers at
  0 dB), AC'97 PCM in (record select, record gain 0 dB, line in and
  microphone off the analog output), the AudioPCI's ADC (AK4531 input
  mixer on the ES1370, AC'97 record select on the ES1371). The tick takes
  what the hardware recorded, scales it by the stream's volume, resamples
  and converts it into a queue of at most 2 s; a stuck input becomes
  silence by the clock. RS2API 309-312, and 303/305-308 on a capture
  handle:

  | Ord | Call | |
  |-----|------|-|
  | 309 | Rs2AudioOpenInput(ULONG ulRate, ULONG ulChannels, ULONG ulBits, PULONG phStream) | 50 without input hardware, 170 while another capture is open |
  | 310 | Rs2AudioRead(ULONG hStream, PVOID pBuf, ULONG cb, ULONG ulTimeoutMs, PULONG pcbRead) | waits for `cb` bytes (at most 1 s's worth) or the timeout, then copies what there is |
  | 311 | Rs2AudioQueryLevel(ULONG hStream, PULONG pulLevel) | recent peak, left low word, right high word, 0-32767 |
  | 312 | Rs2AudioSetInput(ULONG ulSource, PULONG pulSource) | 1 line in, 2 microphone, 0 keeps; the source taken |

  QueryPos gives the bytes recorded, Stop discards the queue (waiting
  readers return), SetVolume is the recording level, Pause drops what
  comes meanwhile. `cargo xtask run --sound-in FILE|tone:HZ` feeds QEMU's
  input through its D-Bus audio backend (tools/scripts/soundin.py);
  `AUDTEST RECORD` checks the calls: it passes on QEMU's HDA, AC97 and
  ES1370. Recording on VMware (ES1371, HD Audio) is untested
- planned: jack sensing, codec volume controls, MMPM/2's audio device
  driver interface for IBM's own drivers
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
  for the other devices without MSI (done for the NICs: `msi::setup_intx`)
- done: HPFS (read/write/format)
- done: a block cache under HPFS, FAT and JFS, lazy writing under the
  rules of IBM's HPFS (HPFS and FAT on fixed disks), the HPFS dirty flag,
  `IFS=...HPFS.IFS /CACHE:n /CRECL:n`, CACHE's controls (docs/ARCHITECTURE.md)
- done: AUTOCHECK (`/AUTOCHECK:drives`, `DISKCACHE=...,AC:drives`), the FAT
  dirty flag
- done: HPFS's name cache (`crates/hpfs`): names found or not, kept per DNODE;
  a path cache above FAT, HPFS and JFS (`crates/pathcache`): directories and
  missing names
- planned: giving cache memory back to programs, a name cache inside JFS, the
  cost left in DosFindFirst, lazy writing for JFS in the order its log needs
- done: extended attributes on FAT12/16 (`EA DATA. SF`), FAT32
  (FAT32.IFS-style EA files) and HPFS, with the EA APIs, DosCopy/DosMove
  and CHKDSK checks
- done: HPFS system volumes: the boot command line's `root=GUID` makes a
  GPT partition C: whatever its file system (`fs::probe_mount`); the
  installer can put the system on HPFS or JFS with a FAT32 EFI system
  partition (PMINSTAL.EXE, or INSTALL.EXE /NEW:HPFS|JFS)
- done: JFS (read/write/format, `crates/jfs`, layout in
  [formats/jfs.md](formats/jfs.md)): OS/2 volumes (case-insensitive) and
  Linux ones (index tables), inline and extent EAs; MBR type 0x35, GPT and
  `root=GUID` system volumes; checked with Linux jfs_fsck. No log replay:
  volumes another system left with a busy log open read-only
- done: OS/2 LVM drive letter tables (volumes get their LVM letters), CHKDSK
  /F for HPFS and JFS (no JFS log replay: the repair marks the log redone)
- planned: JFS log replay, LVM volumes spanning partitions

## Milestone 3: rs2 user space (in progress)

- done: `elf2lx` toolchain (Rust/i686 to LX executables and DLLs)
- done: CMD.EXE (internal commands, batch files, pipes, redirection),
  MORE, FIND, SORT, ATTRIB, TREE, XCOPY, INSTALL
- done: LABEL, CHKDSK (FAT check and /F repair; HPFS read-only scan; JFS
  read-only check), FORMAT (FAT, HPFS, JFS) on DASD handles and the category 8 disk IOCtls, MODE
  (screen mode through a 16-bit VIO thunk), HELP
- done: own message files: OSO001.MSG/OSO001H.MSG (every message number
  of OS/2's, in rs2's words) and OS/2's other system message files,
  compiled from MKMSGF source by crates/msgf; own help: WPHELP.HLP (the
  shell's help, F1 and the Help menus in PMWP), the Command Reference
  (CMDREF.INF) and Start Here (START.INF)
- done: the base utilities (docs/UTILITIES.md): BACKUP/RESTORE, PACK/UNPACK,
  LVM/FDISK, COMP, DISKCOPY/DISKCOMP, EAUTIL, PATCH, PSTAT, REPLACE, SETBOOT,
  SYSLEVEL, BLDLEVEL, CACHE, ANSI, EJECT, PRINT, RECOVER, HELPMSG, CHKDSK /F
  for HPFS and JFS, FORMAT /P
- planned: UNDELETE with deleted-file tracking (DELDIR), LVM volumes over
  several partitions and Boot Manager; Cyrillic/Greek/Hebrew keyboard
  layouts
- done: rs2's own classic REXX (REXX.DLL, REXXAPI.DLL, REXXINIT.DLL,
  REXXUTIL.DLL, RXQUEUE, RXSUBCOM, PMREXX/PMREXXIO, REXXTRY, REX.MSG),
  binary compatible with IBM's (IBM's CMD runs procedures on it); see
  [REXX.md](REXX.md)
- done: rs2's own Object REXX (OREXX.DLL, OREXUTIL.DLL, OREXXSOM.DLL,
  OREXXWPS.DLL, REXXC, OREX.MSG/OREXH.MSG, SWITCHRX.CMD, WPSINST.CMD):
  IBM's Object REXX 6.00 language and classes, activities on threads,
  SOM classes and Workplace Shell scripting; ORXTEST, ORXSOM, ORXWPS and
  ORXLANG pass without IBM's REXX files ([REXX.md](REXX.md#object-rexx))
- done: Object REXX's remaining gaps: UNINIT (on the last reference and at
  the end), SAY/PULL/LINEIN/LINEOUT/trace through .OUTPUT, .INPUT and
  .ERROR as IBM's, IBM's REXX.IMG classes (.RX_QUEUE, .STDQUE, .SYSTEM,
  .Stream_Supplier), monitor destinations as IBM's; IBM's tokenized
  programs examined (flattened C++ object images) and refused with Error
  3.904; the kernel clock monotonic and exact (TIME('E')), LAPIC timer
  calibration and lost ticks fixed, DosDelete of an open file refused
- open: IBM's Object REXX hangs when ORXTEST, ORXSOM and ORXTEST run in
  one CMD (a race in IBM's interpreter restart, REXX.md); rs2's
  .Stream objects share the stream functions' handles (no UNINIT close)
- done: Rust's standard library for rs2 programs (`userland-std/`,
  rs2libc: the glibc subset std calls, over the OS/2 API; TLS through a
  per-thread GS selector; unwinding panics); STDTEST passes 69 checks
  ([BROWSER.md](BROWSER.md))
- done: web browsers (BROWSER.md): WEBTEXT (text mode) and WebRS2
  (WEB.EXE, PM): HTTP/HTTPS (rustls, ring), html5ever, CSS with Servo's
  selectors and cssparser, Taffy layout, GPI painting, images, forms,
  JavaScript through Boa while a page loads; WPUrl objects open it.
  planned: scripts after loading (events, XMLHttpRequest), background
  images, position: fixed, HTTP keep-alive and a cache
- done: GCC/kLIBC programs of the ArcaOS RPM repository, unmodified:
  coreutils, grep, bash (fork, pipes), curl, and bitwiseworks' Firefox
  45.9 (its UI, HTTP and HTTPS pages) (BROWSER.md, "OS/2 open-source
  browsers"). done: memory above 512 MB as OS/2 4.5 has it
  (VIRTUALADDRESSLIMIT, OBJ_ANY, DLL objects marked OBJHIGHMEM loaded
  high; ARCHITECTURE.md "Memory layout"); kLIBC's high heap and Firefox
  run with it. planned: Qt 5 programs (Designer dies in QtCore;
  QtWebEngine waits for PTHR01 functions)
- done: OS/2 package managers, unmodified (PACKAGES.md): RPM, yum
  (installing from rpm.netlabs.org inside rs2) and WarpIN (WPI archives
  under the Workplace Shell), ANPM (VX-REXX: lists and an install from its
  window) on rs2's own RXSOCK.DLL
- done: PKGSETUP and the installer's option: RPM and yum downloaded and set
  up on the first start (docs/PACKAGES.md)
- done: PKGMGR.EXE, rs2's Package Manager window over yum and RPM

## Milestone 4: Presentation Manager (largely done)

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
- done (shell): copying and moving files and folders (Copy/Move menu
  items with a target folder dialog, Ctrl+drag; a worker thread with a
  progress window; EAs through DosCopy and for directories; the name
  collision dialog: rename, replace, skip), program objects copied and
  moved between groups; shadows (Create shadow, Ctrl+Shift+drag; kept
  in OS2.INI, in the shadow colour through the container's owner draw,
  following their objects, broken when they are gone); the Startup
  folder (its objects start once after each boot); the tree view of
  drives and directories (expanding on demand); drag and drop through
  PMDRAG's DrgDrag protocol (DM_DRAGOVER, DM_DROP, DRAGINFO in shared
  memory, string handles as atoms, DM_RENDER, DM_ENDCONVERSATION; the
  container's CN_DRAGOVER/CN_DROP), with other programs: files dropped
  on programs start them, the System Editor opens a dropped file;
  `RS2\DESKTEST.EXE` checks them with a drag between two processes
- done: PMDRAG's lazy drag, DrgDragFiles/DrgAcceptDroppedFiles, drag
  images with the pointer
- done (shell): templates and the SOM-based Workplace Shell (below)
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
- done (help): IBM's help window menu bar (copying a panel, viewed
  pages, expanding and collapsing the contents), the library list
  (marked libraries searched), bookmarks for books (kept in `NAME.cp`)
- done (help): Services > Print (docs/PRINTING.md)
- done (help): metafiles in dynamic data, viewport styles, HELPMGR's
  ordinals 38-44
- done: PMWIN hooks as IBM's PMWIN calls them (send, input, message
  filter, journal record and playback, loader, the rest; system hooks
  in every process), DDE (WinDdeInitiate/PostMsg/Respond with another
  process), owner-drawn items (list boxes, menus, user buttons, value
  sets, sliders, notebook tabs, container text), presentation
  parameters as IBM's PMWIN keeps them (any id, inherited along the
  owner chain, WM_PRESPARAMCHANGED; the controls draw in their
  colours), PMDRAG's lazy drag, DrgDragFiles/DrgAcceptDroppedFiles and
  drag images with the pointer, PMWINX/REGISTRY (REGEDIT2) and PMBIDI
  (TEXTCON1); `RS2\PMXTEST.EXE`
- done: PMDDEML, PMVIOP, IBM's presentation-space rules, the registry's
  W calls and INI-mapped keys; Borland's IDE (editor focus, Open project,
  Build all), EPM's formatted printing, the circular slider and IBM's
  Multimedia Setup (STPM) and MINSTALL
- done (multimedia without IBM's files): MMPMMMIO.INI and
  mmioIniFileHandler, MMIOPROC.DLL's I/O procedures (BMP, JPEG, GIF, PCX,
  TIFF, Targa read and written; AU, SND, VOC, AIFF, 8SVX; MIDI; AVI), MDM
  playing AVI (DIB, RLE8, Video 1, Ultimotion) with sound and AU/VOC/AIFF,
  the multimedia Workplace Shell classes with MMPARTS's names and the
  Multimedia folder, the applets MPPM, WEPM, CDPM, MMVOLUME and STPM
  ([MMPM.md](MMPM.md), [WPS.md](WPS.md))
- done (shell): the System object's Logo, Print Screen, Title and Window
  pages, the desktop's Lockup and Archive pages with the lockup, Menu and
  Self-close pages, print job, driver, port and Spooler objects, Create
  a printer
- done: PMGRE (programs calling the graphics engine directly: COMETRUN's
  comet trail)
- done (shell): Warp 4's folder menu bars (Folder, Edit, View, Selected,
  Help), the System object's Folder pages (menus and menu bars, default
  folder view, automatic close), Input page and 256-colour page, the
  lockup's background image, file system handles that cannot collide
  between processes (SysOpenObject of a path opened another folder)
- done (shell): the View menu's Arrange submenu with IBM's directions and
  Undo arrange/Undo sort, Pickup and Drop (lazy drag) on object and folder
  menus, Paste in folders (IBM's dialog: clipboard format and class) and
  rs2's object clipboard (Ctrl+C/X/V), the lockup image in any format MMIO
  reads, the Recovery Choices (R at boot: restore an archive of the desktop
  and system files) ([WPS.md](WPS.md))
- done: DDEML monitors (APPCLASS_MONITOR, every MF_* kind, across
  processes); IPF metafile artwork (`:artwork.` of .MET files in the
  compiler, played by HELPMGR)
- done: the Print Screen key (docs/PRINTING.md)
- done: the window manager acting on the Input page's settings (outline
  dragging without full window drag, the Alt+Tab switching window, VIO
  mouse actions, asynchronous focus change) and on Special Needs (the
  acceptance delay and time-out), docs/WPS.md
- done (shell): the rest of System Setup's WPCONFIG and WINCFG objects:
  Country (PM_National), Power (ACPI power off, status view), Special
  Needs, WIN-OS/2 Setup (data exchange, session type, WIN-OS/2 settings),
  the System object's Screen pages (resolution and colours, display
  type) and Fonts page; WINCFG.DLL's forwarders
- done: display driver model (GRADD-style) on the framebuffer, mouse
  pointer; PMWIN (windows, messages, controls, dialogs), PMGPI
  (graphics, fonts), PMSHAPI
- done: 256 colours for one program on a true-colour desktop
  (`RS2COLORS=256` in its environment, the program object's
  Compatibility page)
- done: SOM runtime (SOM.DLL, binary compatible with IBM's: class data,
  method/data tokens, somBuildClass, initializer controls, class
  manager; `RS2\SOMTEST.EXE`, also run against IBM's SOM.DLL; IBM's
  SOMU and SOMIR classes load on it); docs/WPS.md
- done: SOM's class libraries SOMTC.DLL (TypeCodes), SOMU.DLL (before/after,
  traced and single-instance metaclasses, SOMBuf, UUIDs) and SOMIR.DLL
  (the interface repository, reading and updating .IR files), binary
  compatible with IBM's; SOMDesc, SOMArgList and SOMCommand (calls written
  out as text) from the repository; IBM's SOMU, SOMTC and SOMIR work on
  rs2's SOM (before/after and traced metaclasses included) and rs2's on
  IBM's
- done: rs2's own `\OS2\ETC\SOM.IR` (the interfaces of the SOM classes
  rs2 ships), written at build time from userland/somir/som-ir.txt
  (tools/scripts/gen_som_ir.py makes it from the Toolkit's IDL)
- done: rs2's own `WPSH.IR` (the Workplace Shell's classes), `WPDSERV.IR`
  (WPDServer, SOMDServer, the socket classes) and `REXX.IR` (REXXSOM's
  DLF classes), made the same way (rs2's own IDL for the classes the
  Toolkit does not describe: WPImageFile, WPMinWindow, the DLF classes);
  CONFIG.SYS's SOMIR lists all four; SOMTEST, ORXTEST, ORXSOM and ORXWPS
  pass with no IBM files, and IBM's SOMIR reads them
- done: SOMD.DLL as a stub: IBM's exports and classes, every method
  raising NO_IMPLEMENT; IBM's Object REXX runs (ORXTEST, ORXSOM, ORXWPS)
  without IBM's SOMD, SOMEM, SOMS and SOMSEC
- done (milestone 1): OpenDoc for OS/2 (docs/OPENDOC.md): rs2's own
  OPENDOC.DLL with IBM's ordinals (decoded from MMODPTS's and OREXXSC's
  imports), its SOM classes in OpenDoc's release orders: session, storage
  (rs2's own container file), drafts, storage units, parts and part
  handler registration, frames, facets, shapes, transforms, canvases,
  window state; `RS2\ODTEST.EXE`, ODSHELL.EXE (a document's root part in
  a PM window); IBM's MMODPTS parts load, save, reopen and draw on it
- done (milestone 2): OpenDoc's event dispatching (ODDispatcher and
  dispatch modules, mouse to the facet under the pointer, click
  activation with the active border, key and menu focus), ODMenuBar as a
  PM menu with the docshell's base menu bar, shapes as unions of
  rectangles with GPI regions; IBM's MMAudioPart imports a WAV file and
  plays it in ODSHELL, MMImagePart shows a JPEG (MMIO compound element
  names, MCIAPI's play-file palette)
- done (milestone 3): OpenDoc's settings notebook (ODSettingsExtension
  pages, IBM's MMSettingsExtension's included), MMODPTS's own menus,
  AdjustMenus and button-2 popups, the play palette's state observer and
  position slider, ODClipboard on PM's clipboard (text and parts, storage
  unit reference tables and CloneInto), drag and drop on PMDRAG (parts
  moved within a document, a WAV dropped from a WPS folder becomes an
  MMAudioPart), ODHelp through HELPMGR and the session's status line
- done (milestone 4): OpenDoc scripting with Object REXX: rs2's own
  SOMUC.DLL, the Apple Event Manager, OSAScriptingComponent, the
  Apple-event terminology classes, OREXX.DLL's OSA calls, native-method
  API and native methods; IBM's OREXXSC.DLL runs REXX scripts that send
  Apple events to the application (ODSHELL's Document/Run script)
- done (milestone 5): OpenDoc scripting: the terminology's classes and
  properties (NOVALUE, object specifiers, named parameters), AEResolve
  with object accessors, Apple events between processes (CMD scripts
  drive ODSHELL), OSA recording
- planned: OpenDoc's links, undo, nested drafts, rendering exchange for
  parts dropped on other programs; recording events with parameters
- not planned: DSOM itself (distributed objects, the SOMDD daemon,
  SOMDSVR; SOMEM, SOMS, SOMSEC): nothing on the CD uses it; rs2's own Object REXX does not
  use SOMD
- done: the Workplace Shell's object model in PMWP.DLL (58 classes with
  IBM's class data and ordinals; WPObject, WPProgram, WPShadow,
  WPFileSystem, WPDataFile, WPFolder, WPDrives/WPDisk, the desktop,
  WPClassManager; OS2.INI and .CLASSINFO/.LONGNAME persistence; the
  object and class API; the Toolkit's sample class DLLs work on it;
  `RS2\WPSTEST.EXE`)
- done: its views: folder windows (icon, details, tree), the desktop,
  settings notebooks, pop-up menus, direct editing, drag and drop, all
  through the classes' methods (class DLLs add menu items and settings
  pages); `RS2\WPSHELL.EXE` runs them
- done: the shell process: OS2\PMSHELL.EXE with PMWP's ShlStartWorkplace
  (the desktop from OS2.INI's PM_InstallObject/PM_InstallClass entries
  and rs2's own the first time, the Startup folder, shutdown, the
  object API from other processes); the default desktop since it
  has RS2SHELL's objects (Minimized Window Viewer, Window List, System
  Setup, Shut Down; drives open their root folders)
- done: folder features: copying and moving with progress and name
  collisions (Copy..., Move..., Create shadow..., Create another...),
  Sort and Include pages, templates (the Templates folder), Find
- done: the LaunchPad, the View tab's three pages (icon format and
  size, tree view, details columns), the lazy writer, the Help Index's
  letter tabs, System Setup's Keyboard, Mouse, Sound, Color Palette
  and Scheme Palette (PMWIN's WinSetSysValue and WinSetSysColors)
- done: anti-aliased fonts: rs2's TrueType and Type 1 rasteriser, the
  Liberation fonts as the system font and the outline faces, IBM's
  fonts (WarpSans, Helvetica, Times New Roman, Courier) when installed
  from the CD (docs/PM.md, Fonts)
- done: the Font Palette and folder fonts (PP_FONTNAMESIZE)
- done: deleting with IBM's confirmations and the Shredder, IBM's
  settings pages (File, Type, Window, Association, Session, Details, the
  View page's font), the System object and System Clock, Connections
  (Network folder, Printers with shell-side printer objects), objects'
  use lists; the Toolkit's wpstutor.dll registered in the running shell
  and working (docs/WPS.md)
- done: printing (docs/PRINTING.md): the spooler, the PostScript
  driver, printer objects
- done: network groups, servers and shares, drives and printer ports
  assigned from them (docs/LAN.md); network printers (WPRPrinter), the
  network objects' Network/Server/Resource pages, Logon and Logoff,
  Access another server/directory/printer, Access control, rs2's
  network icons
- done: the Hardware Manager (IBM's PNP.DLL classes WPHwManager and the
  WPDevice classes, in PMWP) on the kernel's resource manager
  (`RESMGR$`, IBM's IOCtls; RMINFO.DLL; IBM's RMVIEW runs): the
  machine's processors, memory, PCI functions, disks, ISA devices and
  their IRQ, DMA, I/O and memory (docs/WPS.md); planned: Touch,
  Selective Install (docs/WPS.md, "What is not there")

## Printing (done: first version)

- done: design (docs/PRINTING.md); PMSPL as a spooler (queues, printers,
  ports, jobs in \SPOOL; the Spl*, SplQm* and Prt* calls), PMSPOOL.EXE,
  PMPRINT.QPR (PM_Q_STD metafiles and PM_Q_RAW); a PostScript printer
  driver in PMWIN's GRE (crates/gpi psdev: paths, patterns, images,
  Type 3 fonts from rs2's fonts); ports FILE, LPT1-3 (the kernel drives
  a parallel port), LPR and IPP; Workplace Shell printer objects (job
  view, settings, new printers, printing files); Print in E, the help
  manager; IBM's Picture Viewer prints; PRNTEST
- done: EPM's formatted printing (frames pass WM_ACTIVATE to their
  client, GpiQueryCharBox in world units)
- done (second round): programs that print to a port (DosOpen of
  LPTn/PRN) and DOS sessions (INT 17h, the parallel port registers,
  PRN, PRINT_TIMEOUT and PRINT_SEPARATE_OUTPUT) print into the spooler;
  Win-OS/2's printer drivers through LPT1 (checked in dosrun); separator
  pages (LAN Manager's .SEP format, rs2's SAMPLE.SEP and PSCRIPT.SEP),
  queue start/stop times and priorities; the printer objects' View and
  Print options pages, the job details view, moving jobs between
  printers; the Print Screen key; Flate-compressed images
- done (third round): network printers on SMB print shares (local
  alias queues, PRQINFO6, `\\SERVER\QUEUE` ports), Start again of a
  printing job (PRD_RESTART), a disabled spooler printing PM jobs at
  once, the separator's input bin (PrtQuery/PrtSet), print times for
  large jobs only (PRQINFO8), the network job view
- planned: the server's own queue listing for network printers (RAP or
  MS-RPRN), other printer languages (PCL),
  Win-OS/2 printing checked in a QEMU session, block characters in
  separator pages

## DOS sessions (done: first version)

- done: design (docs/DOS.md): VDM.EXE, a user process leading the
  session, runs rs2's DOS machine on an x86 emulator (long mode has no
  V86)
- done: `crates/x86emu`: 80486SX-class real and protected mode (16/32-bit
  segments, LDT/GDT/IDT, gates, privilege levels, TSS stack switch,
  exceptions), x87 FPU; ALU and protected-mode tests recorded on QEMU
- done: `crates/dos`: PIC, PIT, 8042, CMOS, VGA (text, CGA, EGA/VGA
  planar, mode 13h, mode X), BIOS, DOS kernel (files on rs2's file
  systems, FCBs, memory with UMBs, EXEC, TSRs), XMS, EMS, mouse driver,
  rs2's COMMAND.COM, DPMI 0.9 host; `tools/dosrun` runs it on the host
- done: DOS sessions full screen and in PM windows (PMVIOWIN shows text
  and graphics modes), raw keyboard and mouse, Alt+Home, from CMD
  (DosExecPgm of a DOS program), DosStartSession types 4/7, desktop
  objects (DOS Window, DOS Full Screen, PROG_VDM/PROG_WINDOWEDVDM)
- done: PC speaker, Sound Blaster 16 (8/16-bit DMA, mixer, BLASTER),
  OPL3/OPL2 FM synthesizer, MPU-401 UART to the GM synthesizer, all mixed
  into one stream on the machine's clock; VESA direct colour modes (15
  to 32 bits per pixel, dithered); DOS settings in WPProgram's Session
  page (DOS properties dialog, kept in the program's environment) applied
  by VDM.EXE (DOS_RMSIZE, DOS_VERSION, DOS_BREAK, DOS_SHELL, HW_NOSOUND,
  memory sizes...)
- planned: DPMI 1.0, a decode or block cache for the CPU, true colour
  for VESA modes, more DOS settings (DOS_DEVICE, DOS_FILES, VIDEO_*)

## Native Win16 (stages 1 to 3 done)

Windows 3.x programs as OS/2 processes, without Win-OS/2 or any
Microsoft file (docs/WIN16.md).

- done (stage 1): design (docs/WIN16.md); the NE loader loads Windows
  modules (target OS 2) with WIN16.DLL as their KERNEL, USER, GDI and
  KEYBOARD (Windows ordinals at fixed bases); Windows' start registers;
  INTs and protection faults of Windows programs reflected to WIN16
  (RS2API 420: INT 21h, InitTask, invalid ES/DS loads); InitTask, global
  and local memory, modules, the DOS calls; window classes, windows as
  PM frames and windows, the message loop with translated messages and
  y-down coordinates, painting (BeginPaint, TextOut, DrawText, FillRect,
  lines), MessageBox; `RS2\HELLO16.EXE` and `RS2\W16TEST.EXE` (built
  with Open Watcom from `userland/win16test`)
- done (stage 2): the program's resources (strings, menus,
  accelerators, dialog templates, FindResource/LoadResource); menus as
  PM menus with Windows' messages; accelerators; dialogs (DialogBox,
  CreateDialog, DefDlgProc, IsDialogMessage with Tab, arrows, Enter,
  Esc and mnemonics); BUTTON, STATIC, EDIT, LISTBOX, COMBOBOX and
  SCROLLBAR as Windows 3.1-style controls drawn through WIN16's GDI;
  subclassing; window scroll bars; the caret; CF_TEXT on PM's clipboard;
  CreateFont; `RS2\MENU16.EXE`, `RS2\DLG16.EXE`, `RS2\NOTE16.EXE` and
  W16TEST's stage-2 checks
- done (rest of stage 2): MDI as Windows does it (maximized children in
  the frame's menu bar and title, tiling, cascading, icons in the
  client), bitmaps and DIBs (raster operations, hatched and pattern
  brushes, 1/4/8/24-bit DIBs, bitmap and cursor resources), regions
  (clipping, InvalidateRgn), mapping modes, font enumeration on Windows'
  faces with real metrics, Windows DLLs (imports, LoadLibrary, usage
  counts, WEP), WIN.INI and private profiles as files in C:\WINDOWS;
  `RS2\MDI16.EXE`, W16TEST's MDI, GDI, RGN, MAP, FONT, DLL and INI groups;
  Microsoft's Multipad sample runs
- done (stage 3): COMMDLG (Open, Save As, Color, Font, Print, Print
  Setup, Find, Replace as WIN16 dialogs, hooks and templates); printing
  from Windows programs into rs2's spooler and PostScript driver
  (printer DCs, Escape, StartDoc...EndDoc, abort procedures, WIN.INI's
  printers); DDE between Windows tasks (proxy windows, GMEM_DDESHARE as
  shared memory, global atoms), to PM programs' DDE servers, and DDEML;
  the clipboard's formats (CF_BITMAP, CF_DIB, CF_METAFILEPICT,
  registered formats, delayed rendering) on PM's; NOTE16's dialogs,
  `RS2\PRINT16.EXE`, `RS2\DDECLI16.EXE`/`RS2\DDESRV16.EXE`, W16TEST's
  stage-3 checks
- planned (stage 4): DPMI, the floating point emulator, MMSYSTEM, OLE,
  TOOLHELP and the task/module databases

## Milestone 5: install media (first version done)

- done: hybrid ISO/USB image (El Torito EFI + MBR ESP), live session
- done: installer (GPT + EFI system partition, FAT32, copy, restart)
- done: installer updates an existing installation in place (keeps CONFIG.SYS, INI files, MPTN\ETC; merges new CONFIG.SYS entries)
- planned: choice of partitions and file systems, BIOS boot, boot manager

## Later

- LAN requester (first version done, docs/LAN.md): SMB 2.0.2/2.1 and
  SMB1 client with NTLMv2 and signing, NetBIOS over TCP, the redirector
  (network drives, UNC paths, LPTn), NET.EXE, NETAPI/NETAPI32, the
  Network folder; planned: SMB 3 (encryption), oplocks and caching,
  byte-range locks, IBM's NET.EXE on rs2's NETAPI, NFS
- MMPM/2: MPEG and Indeo video, IBM's Ultimotion codebook (rs2 draws
  those blocks flat), multiple images per file, compound files (RIFF CF)
- DOS sessions: CPU speed (a block cache), DPMI 1.0
- self-hosting (building rs2 on rs2): 64-bit processes with a Linux
  system-call layer for the Rust toolchain; see [SELF-HOSTING.md](SELF-HOSTING.md)
