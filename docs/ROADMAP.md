# rs2 roadmap

Status markers: done, in progress, planned.

## Milestone 1: unmodified text-mode programs (in progress)

- done: UEFI boot, 64-bit kernel, framebuffer text screen, memory management
- done: OS/2 address space (arenas, tiled LDT), processes, threads, scheduler
- done: LX and NE loader, DLLs in the shared arena, API gate (16/32-bit)
- done: first DOSCALLS; 16-bit NE (`MKSYSLVL.EXE`) and 32-bit LX (`MAPSYM.EXE`)
  programs from the Warp CD run unmodified
- in progress: VFS with drive letters, FAT, full file API (DosOpen family,
  DosFind*, path info, attributes, extended attributes)
- planned: semaphores (16-bit RAM/system, 32-bit event/mutex/muxwait),
  DosCreateThread, critical sections, exit lists
- planned: DosExecPgm / DosWaitChild / DosCWait, pipes, handle inheritance
- planned: exception management (handler chains, DosRaiseException,
  unwinding, guard pages, 16-bit DosSetVec), signals (Ctrl+C/Break)
- planned: VIO (logical video buffers, popups), KBD, MOU, sessions
- planned: MSG (message files) and NLS (country/code page data)
- goal: the original CMD.EXE runs interactively; real DLL initialisation

## Milestone 2: storage and installed systems

- PCI enumeration, ACPI (MADT, IOAPIC/LAPIC, power off), SMP
- Disk drivers: AHCI, NVMe, virtio-blk, IDE/ATAPI; USB (xHCI, mass storage,
  HID)
- Partitioning: MBR, GPT, OS/2 LVM metadata
- File systems: FAT12/16/32 (with OS/2 EAs), HPFS, JFS, ISO 9660/Joliet, UDF
- Boot from an installed disk (UEFI and BIOS)

## Milestone 3: rs2 user space

- `rs2-link`: build genuine LX executables and DLLs from Rust (i686)
- Own implementations of system DLLs that are user-mode in OS/2
  (DOSCALL1 helpers, MSG/NLS data, REXX later)
- Own CMD.EXE and the base utilities (DIR, COPY, XCOPY, ATTRIB, CHKDSK,
  FORMAT, FDISK/LVM, ...)
- Own message files and country data

## Milestone 4: Presentation Manager

- Display driver model (GRADD-style) on the framebuffer, mouse pointer
- PMWIN (windows, messages, controls, dialogs), PMGPI (graphics, fonts),
  PMSHAPI, help manager
- SOM runtime and the Workplace Shell

## Milestone 5: install media

- Hybrid ISO/USB image (El Torito EFI + GPT), live session
- Installer: partitioning, formatting, file copy, CONFIG.SYS, boot manager

## Later

- TCP/IP stack and sockets (so32dll / tcp32dll APIs), NetBIOS
- MMPM/2 multimedia, audio drivers
- DOS sessions (MVDM) through a CPU emulator, since long mode has no V86
