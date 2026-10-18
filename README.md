# rs2

A re-implementation of IBM OS/2 Warp 4.52 in Rust that runs existing OS/2
programs unmodified, natively, on 64-bit PCs.

- 64-bit kernel; OS/2 programs (16-bit NE, 16/32-bit LX) run in
  compatibility mode with the original OS/2 address space layout (tiled
  LDT, private/shared arenas), so 16:16 and flat code mix as on OS/2.
- The OS/2 kernel API (DOSCALLS: files, memory, processes, threads,
  semaphores, exceptions, timers, queues, shared memory, pipes, modules,
  resources, signals) plus VIO, KBD, MSG and NLS.
- UEFI boot from CD, USB stick or hard disk; AHCI, partitions (MBR/GPT),
  FAT and ISO 9660.
- Its own user space written in Rust and packed into genuine LX files:
  command interpreter, base utilities, installer.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design and
[docs/ROADMAP.md](docs/ROADMAP.md) for what is done and what is next.

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

## Repository

| Path | Contents |
|------|----------|
| `kernel/` | the kernel |
| `boot/` | UEFI loader |
| `crates/` | shared libraries: executable formats, FAT, boot hand-off |
| `userland/` | OS/2 programs in Rust (CMD, utilities, installer, tests) |
| `tools/` | `elf2lx`, `lxdump`, Ghidra scripts, analysis scripts |
| `xtask/` | build tool: images, ISO, QEMU |
| `dist/` | files of the distributed system (CONFIG.SYS, README) |
