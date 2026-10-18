# rs2 roadmap

Status markers: done, in progress, planned.

## Milestone 1: unmodified text-mode programs (largely done)

- done: UEFI boot, 64-bit kernel, framebuffer text screen, memory management
- done: OS/2 address space (arenas, tiled LDT), processes, threads,
  scheduler, FPU/SSE state
- done: LX and NE loader, DLLs in the shared arena, DLL initialisation,
  API gate (16/32-bit conventions, DOSCALL1 aliases)
- done: files (VFS, FAT, OS/2 paths and wildcards, share modes), pipes,
  DosExecPgm/DosCWait, threads, 16- and 32-bit semaphores, exception
  handling, exit lists, timers, queues, shared memory, resources,
  Ctrl+C signals, module APIs, CONFIG.SYS processing
- done: VIO, KBD, MSG (message files), NLS
- done: the original CMD.EXE runs interactively; userland/apitest passes
- done: named pipes (byte and message mode, DosTransactNPipe, DosCallNPipe)
- planned: sessions (DosStartSession, screen switching),
  DosDevIOCtl coverage, bound message segments, DLL termination routines,
  semaphore and memory corner cases found by running more programs

## Milestone 2: storage and installed systems (in progress)

- done: PCI, AHCI (SATA disks, ATAPI drives), MBR/GPT, FAT, ISO 9660
- done: boot from an installed disk (UEFI)
- planned: NVMe, virtio-blk, USB (xHCI, mass storage, HID)
- planned: ACPI (MADT, IOAPIC/LAPIC, shutdown), SMP
- planned: HPFS and JFS, extended attributes, OS/2 LVM metadata

## Milestone 3: rs2 user space (in progress)

- done: `elf2lx` toolchain (Rust/i686 to LX executables and DLLs)
- done: CMD.EXE (internal commands, batch files, pipes, redirection),
  MORE, FIND, SORT, ATTRIB, TREE, XCOPY, INSTALL
- planned: CHKDSK, FORMAT, FDISK/LVM, LABEL, MODE and the rest of the base
  utilities; own message files (OSO001.MSG) and country data; REXX

## Milestone 4: Presentation Manager

- Display driver model (GRADD-style) on the framebuffer, mouse pointer
- PMWIN (windows, messages, controls, dialogs), PMGPI (graphics, fonts),
  PMSHAPI, help manager
- SOM runtime and the Workplace Shell

## Milestone 5: install media (first version done)

- done: hybrid ISO/USB image (El Torito EFI + MBR ESP), live session
- done: installer (GPT + EFI system partition, FAT32, copy, restart)
- planned: choice of partitions and file systems, BIOS boot, boot manager

## Later

- TCP/IP stack and sockets (so32dll / tcp32dll APIs), NetBIOS
- MMPM/2 multimedia, audio drivers
- DOS sessions (MVDM) through a CPU emulator, since long mode has no V86
