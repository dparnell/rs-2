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
- virtio devices for KVM/QEMU, Proxmox, UTM and cloud VMs: block and SCSI
  disks (and booting from them), SCSI CD-ROMs, keyboards, mice and tablets
  (absolute pointer), entropy (`\DEV\RANDOM$`), console
  (`\DEV\VIOCON$`) and display (virtio-gpu, any `video=WxH`).
- Networking: a TCP/IP stack of its own (ARP, IPv4, ICMP, UDP, TCP, DHCP,
  DNS) on Intel e1000/e1000e and virtio-net adapters, a kernel socket API
  and OS/2's socket DLLs (TCPIP32, SO32DLL, TCP32DLL and the 16-bit
  TCPIPDLL) on it, so IBM's FTP, TELNET, TFTP and REXX RXSOCK run
  unmodified; `PING`, `IFCONFIG`, `NETSTAT`,
  `HOST`; see [docs/NETWORK.md](docs/NETWORK.md).
- Presentation Manager on rs2's own PM DLLs, with text-mode programs
  such as CMD.EXE in PM windows ("OS/2 Window": `START /WIN CMD`,
  Alt+Home to full screen, Mark/Copy/Paste), containers, notebooks,
  icons and pointers, minimizing to desktop icons and profiles in real
  OS/2 INI files; see [docs/PM.md](docs/PM.md).
- Its own user space written in Rust and packed into genuine LX files:
  command interpreter, base utilities, installer.

![The rs2 live system: the command interpreter running ver and dir](docs/images/rs2-shell.png)

*The live ISO booted in QEMU.*

![The OS/2 Toolkit's PM hello sample running on rs2's own PMWIN.DLL](docs/images/rs2-pm-hello.png)

*An unmodified Presentation Manager sample on rs2's own PMWIN/PMGPI.*

![IBM CMD.EXE in an OS/2 window next to the OS/2 System Editor, both on rs2](docs/images/rs2-pm-cmd.png)

*Booted straight into PM: IBM's CMD.EXE in an OS/2 window and the System Editor (E.EXE), both unmodified.*

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
that mode, otherwise through the VMware SVGA II driver. With a virtio-gpu
display the kernel's driver always takes over and sets any size: the
`video=` one, else the firmware's mode, else the display's preferred
size.

- **VMware**: choose UEFI firmware. The VMware pointer (vmmouse) follows
  the host cursor without capturing it; the boot log shows "mouse: VMware
  absolute pointer". Not tested on real VMware yet (only on QEMU's
  VMware models: `cargo xtask run --vmware --pvscsi IMG --pvscsi-cd ISO`,
  `--scsi-hba mptsas1068`).
- **VirtualBox**: enable EFI; use SATA or NVMe disks (VirtualBox's LSI
  Logic controllers use the same MPT interface and may work too). The
  mouse works in relative (captured) mode: VirtualBox's absolute pointer
  needs its VMMDev guest device, which rs2 does not drive yet.
- **QEMU**: `-machine q35` with OVMF, as `cargo xtask run` does. For
  virtio-gpu use `-vga none -device virtio-vga` (`cargo xtask run
  --virtio-gpu`): OVMF draws on its VGA side until the kernel takes over.
  With `virtio-gpu-pci` (`--virtio-gpu-pci`) the firmware has no
  framebuffer for the loader, so the screen only starts with the driver
  (earlier boot messages are on the serial port).

Network: Intel e1000 (82540EM, 82545EM: QEMU, VirtualBox, VMware's
`e1000`), e1000e (82574L) and virtio-net adapters are configured by DHCP
at boot (`net=off` or `net=ADDR/PREFIX,GATEWAY,DNS` on the kernel command
line, or NETWORK= in CONFIG.SYS, change that); `cargo xtask run --net` or
`--virtio-net` attaches one on QEMU's user network. Other adapters
(vmxnet3, pcnet, ...) are listed in the serial log but not used.

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
