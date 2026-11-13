# rs2 roadmap

What is still to do. Everything already finished is in
[COMPLETED.md](COMPLETED.md).

## Milestone 1: unmodified text-mode programs

- small SMP races left (docs/ARCHITECTURE.md "Locking"): the keyboard
  shift state and E0 prefix shared between keyboard sources, torn time
  fields in the infoseg, socket ids passed to select, DosDebug reading a
  debuggee thread that never parks
- parallel ports: ECP/EPP transfers, IEEE 1284 device IDs and PCI cards
  on real hardware (QEMU has none); PRT_QUERYJOBHANDLE on spooled ports
- DOSCALLS.443 (SYSINST2.EXE), diskette IOCtls, APM suspend and events

## Milestone 2: storage and installed systems

- next: run the VirtualBox and VMware guest agent backends on those
  hosts; bitmaps on the shared clipboard
- VirtualBox VMMDev (absolute pointer), testing on real VMware
- VMware "fit guest" resizing (needs the tools RPC channel,
  `Resolution_Set`), write-combining (PAT) mappings of framebuffers on
  real hardware
- networking: DBCS translation in TNLS32/TNLS16, a BOOTP/DHCP server, a
  SLIP/PPP dialer, multicast/IGMP
- virtio-scsi hot-plug events
- sound: jack sensing, codec volume controls, MMPM/2's audio device
  driver interface for IBM's own drivers
- the other kernel performance blocks (memory, file system, threads,
  perfview's message queue), taking processors offline
- several requests in flight per disk (the drivers still run one command
  at a time), interrupt coalescing, per-CPU queues, INTx for the other
  devices without MSI (done for the NICs: `msi::setup_intx`)
- caching: giving cache memory back to programs, a name cache inside JFS,
  the cost left in DosFindFirst, lazy writing for JFS in the order its log
  needs
- JFS log replay, LVM volumes spanning partitions

## Milestone 3: rs2 user space

- UNDELETE with deleted-file tracking (DELDIR), Boot Manager;
  Cyrillic/Greek/Hebrew keyboard layouts
- web browsers: scripts after loading (events, XMLHttpRequest),
  background images, position: fixed, HTTP keep-alive and a cache
- Qt 5 programs (Designer dies in QtCore; QtWebEngine waits for PTHR01
  functions)
- open problems: IBM's Object REXX hangs when ORXTEST, ORXSOM and ORXTEST
  run in one CMD (a race in IBM's interpreter restart, REXX.md); rs2's
  .Stream objects share the stream functions' handles (no UNINIT close)

## Milestone 4: Presentation Manager

- OpenDoc's links, undo, nested drafts, rendering exchange for parts
  dropped on other programs; recording events with parameters
- the Workplace Shell's Touch and Selective Install (docs/WPS.md, "What
  is not there")

## Printing

- the server's own queue listing for network printers (RAP or MS-RPRN)
- other printer languages (PCL)
- Win-OS/2 printing checked in a QEMU session
- block characters in separator pages

## DOS sessions

- DPMI 1.0
- CPU speed: a decode or block cache for the emulator
- true colour for VESA modes
- more DOS settings (DOS_DEVICE, DOS_FILES, VIDEO_*)

## Native Win16

Windows 3.x programs as OS/2 processes, without Win-OS/2 or any
Microsoft file (docs/WIN16.md).

- stage 4: DPMI, the floating point emulator, MMSYSTEM, OLE, TOOLHELP and
  the task/module databases

## Milestone 5: install media

- choice of partitions and file systems, BIOS boot, boot manager

## Later

- LAN requester (docs/LAN.md): SMB 3 (encryption), oplocks and caching,
  byte-range locks, IBM's NET.EXE on rs2's NETAPI, NFS
- MMPM/2: MPEG and Indeo video, IBM's Ultimotion codebook (rs2 draws
  those blocks flat), multiple images per file, compound files (RIFF CF)
- self-hosting (building rs2 on rs2): 64-bit processes with a Linux
  system-call layer for the Rust toolchain; see [SELF-HOSTING.md](SELF-HOSTING.md)
- other architectures (AArch64 on QEMU `virt` and the Raspberry Pi 4,
  then RISC-V): an architecture layer in the kernel, a native kernel,
  OS/2 programs on an x86 runner built from x86emu, native system DLLs
  behind thunks, later a JIT; see [PORTING.md](PORTING.md)
