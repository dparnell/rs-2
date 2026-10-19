# rs2

A re-implementation of IBM OS/2 Warp 4.52 in Rust that runs existing OS/2
programs unmodified, natively, on 64-bit PCs.

- 64-bit kernel; OS/2 programs (16-bit NE, 16/32-bit LX) run in
  compatibility mode with the original OS/2 address space layout (tiled
  LDT, private/shared arenas), so 16:16 and flat code mix as on OS/2.
- The OS/2 kernel API (DOSCALLS: files, memory, processes, threads,
  semaphores, exceptions, timers, queues, shared memory, pipes, modules,
  resources, signals) plus VIO, KBD, MSG and NLS.
- UEFI boot from CD, USB stick or hard disk; AHCI, NVMe, USB keyboard/mouse (xHCI),
  partitions (MBR/GPT), FAT, HPFS and ISO 9660.
- Runs as a guest in VMware (and QEMU's VMware device models): VMware
  absolute pointer (no mouse capture), PVSCSI and LSI Logic (MPT Fusion)
  SCSI disks and CD-ROMs, SVGA II resolutions (`video=WxH`); see
  [Virtual machines](#virtual-machines).
- Its own user space written in Rust and packed into genuine LX files:
  command interpreter, base utilities, installer.

![The rs2 live system: the command interpreter running ver and dir](docs/images/rs2-shell.png)

*The live ISO booted in QEMU.*

![The OS/2 Toolkit's PM hello sample running on rs2's own PMWIN.DLL](docs/images/rs2-pm-hello.png)

*An unmodified Presentation Manager sample on rs2's own PMWIN/PMGPI.*

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design,
[docs/ROADMAP.md](docs/ROADMAP.md) for what is done and what is next, and
[docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) for the original OS/2
programs tested so far.

## Building

Requirements: a Rust toolchain (stable) with the `x86_64-unknown-uefi`,
`x86_64-unknown-none` and `i686-unknown-linux-gnu` targets, and QEMU with
OVMF for running it.

```
rustup target add x86_64-unknown-uefi x86_64-unknown-none i686-unknown-linux-gnu
cargo xtask iso --dist --release -o rs2.iso   # bootable install/live ISO
cargo xtask run --iso rs2.iso                 # try it in QEMU
```

The ISO boots into the live system; type `INSTALL` to install onto a hard
disk. Development builds (`cargo xtask run`) take extra files for drive C:
from `work/modules.txt`, which is how original OS/2 programs are tested.

## Virtual machines

rs2 boots in any VM with UEFI firmware and a GOP framebuffer. Storage:
SATA (AHCI), NVMe, VMware PVSCSI, LSI Logic Parallel (53c1030) and LSI
Logic SAS (SAS1068), USB sticks. `video=1280x1024` (or any WxH) in
`\RS2\CMDLINE` picks the resolution: through the firmware if it offers
that mode, otherwise through the VMware SVGA II driver.

- **VMware**: choose UEFI firmware. The VMware pointer (vmmouse) follows
  the host cursor without capturing it; the boot log shows "mouse: VMware
  absolute pointer". Not tested on real VMware yet (only on QEMU's
  VMware models: `cargo xtask run --vmware --pvscsi IMG --pvscsi-cd ISO`,
  `--scsi-hba mptsas1068`).
- **VirtualBox**: enable EFI; use SATA or NVMe disks (VirtualBox's LSI
  Logic controllers use the same MPT interface and may work too). The
  mouse works in relative (captured) mode: VirtualBox's absolute pointer
  needs its VMMDev guest device, which rs2 does not drive yet.
- **QEMU**: `-machine q35` with OVMF, as `cargo xtask run` does.

Network cards (e1000, vmxnet3, ...) are listed in the serial log but not
used: rs2 has no network stack yet.

## Repository

| Path | Contents |
|------|----------|
| `kernel/` | the kernel |
| `boot/` | UEFI loader |
| `crates/` | shared libraries: executable formats, FAT, HPFS, installer unpacking, boot hand-off |
| `userland/` | OS/2 programs in Rust (CMD, utilities, installer, tests) |
| `tools/` | `elf2lx`, `lxdump`, Ghidra scripts, analysis scripts |
| `xtask/` | build tool: images, ISO, QEMU |
| `dist/` | files of the distributed system (CONFIG.SYS, README) |
