# Porting rs2 to other architectures (AArch64, RISC-V)

A study of what it would take to run rs2 on ARM (QEMU `virt`, the
Raspberry Pi) and RISC-V. Nothing here is implemented yet; the work is
listed under "Later" in [ROADMAP.md](ROADMAP.md).

## The basic problem

OS/2 programs are 16- and 32-bit x86 machine code (NE and LX modules,
segmented 16:16 code mixed with flat 32-bit code). On x86-64 rs2 runs
them natively in long mode's compatibility mode and emulates nothing
(see [ARCHITECTURE.md](ARCHITECTURE.md)). An ARM or RISC-V processor
cannot run them, so a port means:

- the **kernel** and, where possible, the **system libraries** native
  to the new architecture, and
- every **OS/2 program** run on an **x86 CPU emulator**, the way
  Windows on ARM, FEX-Emu or Box86/Box64 run x86 programs.

This changes one of rs2's guiding decisions ("nothing is emulated or
translated for application code") for those targets; x86-64 stays as it is.

## How tied the code is to x86 today

Measured on the tree as of November 2026.

### Kernel (about 66,000 lines)

The machine-specific code is concentrated:

- Inline assembly sits in about ten files: `arch/` (GDT, IDT, port I/O,
  interrupt flag), `task/sched.rs` (context switch), `smp.rs` (the
  INIT/SIPI trampoline), `power.rs`, `main.rs`, `chardev.rs`,
  `api/debug.rs` (debug registers) and `vmware.rs` (the backdoor port).
- Port I/O (`inb`/`outb`/`inl`/`outl`...) is used about 350 times, nearly
  all for legacy PC hardware: PCI configuration through 0xCF8/0xCFC, the
  PIT, PIC, CMOS clock, PS/2 keyboard and mouse, 16550 serial, IDE, AC'97,
  ES137x, PCnet, VGA, the VMware and VirtualBox backdoors.
- The control registers, MSRs, TSC, `swapgs`, `iretq`, `invlpg`, LAPIC,
  IOAPIC and MSI address format belong to the trap, paging, timer and SMP
  code.

Largely portable as it is: the file systems (FAT, HPFS, JFS, ISO 9660),
the TCP/IP stack, SMB, NLS and code pages, sessions, queues, semaphores,
pipes, the scheduler's policy, the loader's parsing of NE/LX, and the
drivers for MMIO and PCI devices (NVMe, AHCI, xHCI and USB HID, virtio,
e1000e) once configuration space and DMA go through an architecture
layer.

The deeper x86 assumptions are in the OS/2 personality itself: each
process's tiled LDT, the selector values programs rely on (`0x5B`,
`0x53`, the TIB selector `0x150B` in FS), the `int 0x99` API gate, and
APIs that decode their arguments from the caller's x86 stack and
registers (`_System` and 16-bit `_Pascal`). On another architecture
these do not go away; they move into the emulator, where they get
simpler: a tiled selector's base is just `(sel >> 3) << 16`, and an API
call is recognised when the emulated program transfers control to a
gate address, with no trap needed.

### User space (about 274,000 lines of Rust, plus userland-std)

PMWIN, PMGPI, the shells, the C runtime, REXX, SOM, the multimedia and
network DLLs are built for `i686-unknown-linux-gnu` and packed into LX
modules by `elf2lx`/`rs2-link`. The source is mostly architecture
neutral, but 85 files contain assembly: naked thunks for 16-bit
callbacks and exports (`pmwin/src/cb16.rs`, `exports.rs`, `win16.rs`),
lock fast paths, and `fs:[...]` reads of the TIB (`pmwin/src/shared.rs`).
Shared-memory structures (PMWIN's window tree, PMSHAPI's state) hold
32-bit pointers.

### The emulator rs2 already has

`crates/x86emu` (about 3,800 lines, `no_std`) emulates an 80486SX: real
and protected mode, 16- and 32-bit segments, GDT/LDT, call/interrupt/
trap gates, privilege levels, the exceptions, and an optional x87 FPU.
It does not emulate paging, which suits a user-mode runner: the host's
MMU does that. It runs about 108 million instructions a second natively
on an Apple M-series host (BENCH.COM under `dosrun`).

Missing for 32-bit OS/2 programs built with later compilers: CPUID,
RDTSC, CMOVcc, the Pentium additions, MMX, SSE/SSE2 (GCC-built programs
such as Firefox use them), and FS-relative TIB access with the OS/2
selector values.

## Proposed design

1. **A native kernel** with an architecture layer for: interrupt
   controller (LAPIC/IOAPIC, GIC, PLIC/APLIC), timer (LAPIC + TSC,
   generic timer, `time`/SBI timer), paging and TLB shootdown, context
   switch and trap entry, per-CPU access (GS base, `TPIDR_EL1`, `tp`),
   bring-up of other processors (SIPI, PSCI, SBI HSM), port I/O versus
   MMIO, PCI configuration (0xCF8 versus ECAM), DMA mapping and cache
   maintenance, and firmware tables (ACPI versus device tree).
2. **An x86 runner per OS/2 process.** Each OS/2 process becomes a
   native user-mode program running x86emu, with the guest's linear
   addresses mapped one to one into the low 4 GB of the process (the
   same layout as today: private arena from 64K, shared arena down from
   512 MB, the high arenas). The LDT and GDT become tables in the
   runner; transfers to the gate object call the kernel API with the
   emulated registers; x86 faults become OS/2 exceptions with the same
   CONTEXTRECORD as now. DosDebug keeps working on x86 register sets,
   because the debuggee is x86.
3. **Native system DLLs behind thunks (optional, most of the speed).**
   PMWIN, PMGPI, PMSHAPI, the C runtime and the shells are compiled for
   the host architecture and called from emulated code through entry
   thunks, as Box64's wrapped libraries are; only the application's own
   code is emulated. This needs shared-memory structures with 32-bit
   offsets instead of native pointers, callback thunks for window
   procedures and hooks back into x86 code, and the 16-bit entry points
   handled in the runner.
4. **A JIT** (dynamic translation of x86 basic blocks) once the
   interpreter works. The interpreter is enough for most 1990s OS/2
   programs; large ones (Firefox, OpenOffice) need the JIT.
5. **DOS and Win16 sessions** become native programs (VDM.EXE and the
   Win16 driver around x86emu), so DOS code is not emulated twice.

## Targets

| Target | Notes |
|---|---|
| QEMU `virt`, AArch64 | First target: UEFI (edk2), GICv3, PL011, ECAM PCIe, virtio-pci; rs2's UEFI loader builds for `aarch64-unknown-uefi`. On an Apple Silicon host QEMU can use HVF, so the kernel runs at native speed instead of under TCG; application code is slower than TCG until the JIT exists. |
| Raspberry Pi 4 | Realistic real hardware: the community UEFI firmware provides UEFI, ACPI and a GOP framebuffer. USB through the VL805 is xHCI (rs2 has a driver). New drivers needed: SD card (EMMC2/SDHCI), Ethernet (GENET), audio. Care with DMA address limits on the PCIe bus. |
| Raspberry Pi 5 | Harder: most peripherals sit behind the RP1 chip; its UEFI firmware is less mature. |
| QEMU `virt`, RISC-V 64 | Second target once the architecture layer exists: OpenSBI, PLIC/APLIC, Sv39/Sv48 paging. Hardware: VisionFive 2 and similar. |

## Suggested order

1. **Architecture layer in the x86-64 kernel.** Put port I/O, PCI
   configuration, interrupt controller, timer, paging and context switch
   behind traits while the kernel is still x86 only. Low risk, useful on
   its own, checked with the existing tests (APITEST, WPSTEST).
2. **AArch64 kernel on QEMU `virt`**: boot, memory, interrupts, timer,
   SMP, virtio, the file systems, a native test program.
3. **The x86 runner**: x86emu with CPUID, RDTSC, CMOV, then MMX/SSE;
   threads, exceptions, signals, the gate; IBM CMD.EXE as the first
   unmodified program, then APITEST.
4. **Native PM and system DLLs behind thunks.**
5. **JIT.**
6. **Raspberry Pi 4 hardware**, then RISC-V.

Steps 2 and 3 are the large ones, each comparable to a milestone.

## A smaller variant: hosted rs2

The runner and native-DLL work (steps 3-5) do not need a bare-metal ARM
kernel: the same pieces could run as a user-space program on AArch64
macOS or Linux, with the OS/2 kernel API implemented over the host, the
way Wine runs Windows programs. That would make OS/2 programs usable on
ARM machines early, and the code would carry over to the native port.
