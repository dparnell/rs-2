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
- planned: AVIO (VIO in PM windows), parallel port hardware,
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
- planned: VirtualBox VMMDev (absolute pointer), SVGA run-time mode
  changes under PM and hardware cursor, testing on real VMware
- done: virtio (MSI-X per queue): PCI transport, modern and legacy devices, split
  virtqueues; virtio-blk (read-only, block size, flush; booting from it),
  virtio-scsi (disks, CD-ROMs, unit attentions), virtio-input (keyboards,
  mice, tablets through an absolute pointer path for text mode and PM),
  virtio-rng (\DEV\RANDOM$), virtio-console port 0 (\DEV\VIOCON$)
- done: virtio-gpu 2D (`virtio-vga`, `virtio-gpu-pci`): guest-RAM
  framebuffer for the text screens and PM, `video=WxH` (any size),
  changed rows flushed from the timer tick (whole screen under PM)
- planned: virtio-gpu hardware cursor, PM dirty rectangles instead of
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
- planned: OS/2 socket DLLs on it (SO32DLL, TCP32DLL, TCPIP32, the
  16-bit SOCKETS/TCPIPDLL), ifconfig/netstat/route,
  vmxnet3/pcnet drivers, TCP keepalive/window scaling/SACK
- planned: virtio-scsi hot-plug events, virtio-console multiport
- done: ACPI (MADT, FADT reset and S5), LAPIC timer, IOAPIC routing
- done: SMP: all processors started (INIT/SIPI), per-CPU GDT/TSS/LAPIC
  timer, shared run queues, big kernel lock, IPI wake-ups and TLB shootdown
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
- Display driver model (GRADD-style) on the framebuffer, mouse pointer
- PMWIN (windows, messages, controls, dialogs), PMGPI (graphics, fonts),
  PMSHAPI, help manager
- SOM runtime and the Workplace Shell

## Milestone 5: install media (first version done)

- done: hybrid ISO/USB image (El Torito EFI + MBR ESP), live session
- done: installer (GPT + EFI system partition, FAT32, copy, restart)
- planned: choice of partitions and file systems, BIOS boot, boot manager

## Later

- NetBIOS, NFS/SMB clients
- MMPM/2 multimedia, audio drivers
- DOS sessions (MVDM) through a CPU emulator, since long mode has no V86
