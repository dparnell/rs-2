# rs2

A re-implementation of IBM OS/2 Warp 4.52 in Rust that runs existing OS/2
programs unmodified, natively, on 64-bit PCs.

- 64-bit kernel; OS/2 programs (16-bit NE, 16/32-bit LX) run in
  compatibility mode with the original OS/2 address space layout (tiled
  LDT, private/shared arenas), so 16:16 and flat code mix as on OS/2.
- The OS/2 kernel API (DOSCALLS: files, memory, processes, threads,
  semaphores, exceptions, timers, queues, shared memory, pipes, modules,
  resources, signals) plus VIO, KBD, MSG and NLS.
- UEFI boot from CD, USB stick or hard disk; IDE, AHCI, NVMe, USB keyboard/mouse (xHCI),
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
- A desktop: rs2's own desktop shell on Presentation Manager (folders,
  drives and file windows, program objects kept in OS2.INI, pop-up
  menus, a settings notebook, shutdown with power off); see
  [Using the desktop](#using-the-desktop).
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

### Requirements

- **Rust**, stable, through [rustup](https://rustup.rs), with three
  extra targets: the UEFI loader, the kernel and the 32-bit OS/2 programs
  and DLLs:

  ```bash
  rustup target add x86_64-unknown-uefi x86_64-unknown-none i686-unknown-linux-gnu
  ```

- **QEMU** (`qemu-system-x86_64`) and its **UEFI firmware** (OVMF, from
  the EDK II project), only for running rs2 here:
  - macOS: `brew install qemu` (the firmware comes with it).
  - Debian/Ubuntu: `sudo apt install qemu-system-x86 ovmf`.
  - Fedora: `sudo dnf install qemu-system-x86 edk2-ovmf`.
  - Arch: `sudo pacman -S qemu-system-x86 edk2-ovmf`.

  (`cargo xtask run` needs macOS or Linux; the ISO itself can be used
  anywhere.)

  `cargo xtask run` finds the firmware in those packages' usual folders
  (QEMU's own `edk2-x86_64-code.fd` + `edk2-i386-vars.fd`, or the
  distributions' `OVMF_CODE*.fd` + `OVMF_VARS*.fd`, never the Secure
  Boot builds). If none is installed, the same two files are in every
  QEMU source release (download.qemu.org, `qemu-*.tar.xz`, compressed
  as `pc-bios/edk2-x86_64-code.fd.bz2` and `pc-bios/edk2-i386-vars.fd.bz2`:
  unpack them with `bunzip2`); put them in a folder and point
  `RS2_OVMF_DIR` at it. Any x86-64 OVMF build without Secure Boot works.

Nothing else: no C compiler, no ISO tools and no IBM files. The build
writes the ISO itself, and the live ISO contains only rs2's own code.

### Building the live ISO

From the repository root:

```bash
cargo xtask iso --dist --release -o rs2.iso
```

This builds everything in release mode (loader, kernel, the OS/2
programs and DLLs listed in `dist/manifest.txt`) and writes `rs2.iso`,
about 22 MB. Without `-o` it goes to `work/rs2.iso`. The first build
takes a few minutes; later ones only rebuild what changed.

The image is a hybrid: attach it to a virtual machine as a CD (see
[Virtual machines](#virtual-machines) for settings), or write it to a
USB stick for a real PC with UEFI firmware (Secure Boot off), e.g. on
macOS `diskutil list` to find the stick, then `sudo dd if=rs2.iso
of=/dev/rdiskN bs=4m`; on Linux `sudo dd if=rs2.iso of=/dev/sdX bs=4M
status=progress`. `dd` overwrites the whole device: check its name.

### Trying it in QEMU

```bash
cargo xtask run --iso rs2.iso
```

boots it in a QEMU window (UEFI, q35, 2 CPUs). With a blank disk to
install onto:

```bash
qemu-img create -f raw disk.img 1G
```

```bash
cargo xtask run --iso rs2.iso --hdd disk.img
```

and afterwards `cargo xtask run --boot-hdd --hdd disk.img` boots the
installed system. Other useful options: `--smp N`, `--vmware`,
`--virtio-gpu`, `--virtio-scsi IMG`, `--net`, `--headless` (see the top
of `xtask/src/main.rs` for all of them).

The ISO boots into the live system's desktop; open Programs > Install
rs2 (PMINSTAL.EXE) to install onto a hard disk, or run `INSTALL` at a
command prompt. A new installation erases the disk and lays it out in
one of two ways (GPT, booted by UEFI):

- one FAT32 partition holding everything (the boot loader, the kernel and
  the system, drive C:);
- a 256 MB FAT32 boot partition (the EFI system partition, with the boot
  loader, the kernel and `\RS2\CMDLINE`, whose `root=` names the system
  partition) and the system on an HPFS partition of its own (at most
  64 GB; the rest of a larger disk stays free), which becomes C:.

The installer lists each disk's installed rs2 version (from
`\RS2\VERSION`, which builds stamp with the commit and date) and offers
to update one in place: the system files are replaced, while
`CONFIG.SYS`, the `.INI` files (desktop, settings), `\MPTN\ETC`,
`\RS2\CMDLINE` and everything that is not part of rs2 are kept; new
`SET` variables and new `PATH`/`LIBPATH`/`DPATH`/`HELP`/`BOOKSHELF`
entries are merged into the existing `CONFIG.SYS`. On the command line,
`INSTALL /LIST` shows the disks, and `INSTALL /DISK:1 /NEW:HPFS /YES`
or `INSTALL /DISK:1 /UPDATE /YES` (also `/NEW:FAT32`, `/NORESTART`)
install without asking. Development builds (`cargo xtask run` without `--iso`) take extra
files for drive C: from `work/modules.txt`, which is how original OS/2
programs are tested.

## Using the desktop

![The rs2 desktop: Drives, drive C: and its OS2 folder open](docs/images/rs2-desktop.png)

*The live ISO's desktop (RS2SHELL) with Drives > C: > OS2 open.*

The live system and installed systems start Presentation Manager with
rs2's desktop shell (`C:\OS2\RS2SHELL.EXE`). Double click an object
(or select it and press Enter) to open it: Command Prompts (an OS/2
window or full screen), Programs, Drives (a window per drive, then per
folder; programs run, text files open in the System Editor when it is
installed), the Minimized Window Viewer, the Window List and Shutdown.
Mouse button 2 opens pop-up menus (Open, Settings, Rename, Delete;
Create program object, System setup, Shut down on the desktop) and
drags icons, whose places are kept; a file dragged onto a folder moves
there. Ctrl+Esc shows the window list. Shutdown closes the programs
(they may ask about unsaved work), flushes the disks and turns the
machine off. Details: [docs/PM.md](docs/PM.md#the-desktop-shell-rs2shell).
F1 shows help in PM programs that have it (rs2's help manager reads
OS/2 `.HLP` files along HELP), and `VIEW book [topic]` shows OS/2 books
(`.INF`, along BOOKSHELF): [docs/PM.md](docs/PM.md#the-help-manager-helpmgrdll-and-viewexe).
"Install OS/2 Applications" in the Programs folder installs the OS/2
applications rs2 runs (Solitaire, Chess, Mahjongg, Icon Editor, Pulse,
Locale, REXX, command line tools, the books) from your OS/2 Warp 4.52 CD:
[docs/PM.md](docs/PM.md#installing-os2s-applications-appinstexe).

**The text-mode command prompt instead:** press T while the boot loader
shows "Press T for a text-mode boot", or put `textmode` in
`\RS2\CMDLINE`, or in CONFIG.SYS replace `PROTSHELL=C:\OS2\RS2SHELL.EXE`
with `PROTSHELL=C:\OS2\CMD.EXE`. Development runs (`cargo xtask run`)
use `work/cmdline.txt` as before; `cargo xtask run --desktop` boots into
the desktop.

## Virtual machines

rs2 boots in any VM with UEFI firmware and a GOP framebuffer. Storage:
IDE (PIIX: disks and CD drives), SATA (AHCI), NVMe, VMware PVSCSI, LSI
Logic Parallel (53c1030) and LSI Logic SAS (SAS1068), USB sticks. CD
drives have their letter with or without a disc, so one inserted or
changed later is read. `video=1280x1024` (or any WxH) in
`\RS2\CMDLINE` picks the resolution: through the firmware if it offers
that mode, otherwise through the VMware SVGA II driver. With a virtio-gpu
display the kernel's driver always takes over and sets any size: the
`video=` one, else the firmware's mode, else the display's preferred
size.

- **VMware** (Workstation/ESXi on x86; Fusion on Apple Silicon only runs
  ARM guests): guest OS type **Other 64-bit** with UEFI firmware and
  Secure Boot off; with other guest types VMware's firmware reports "No
  compatible bootloader found". The VMware pointer (vmmouse) follows the
  host cursor without capturing it; the boot log shows "mouse: VMware
  absolute pointer". The live ISO boots and installs on VMware Workstation
  17 (Windows 11 host), and the installed system boots from its disk; QEMU's
  VMware models (`cargo xtask run --vmware --pvscsi IMG --pvscsi-cd ISO`,
  `--scsi-hba mptsas1068`) are what the tests use.
- **Proxmox**: BIOS OVMF (UEFI) with "Pre-Enroll keys" unticked (Secure
  Boot off); VirtIO SCSI, VirtIO Block or SATA disks; VirtIO or E1000
  network. Installing from the live ISO and booting the installed disk
  work there.
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
