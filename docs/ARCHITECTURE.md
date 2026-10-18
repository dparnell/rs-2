# rs2 architecture

rs2 is a re-implementation of IBM OS/2 Warp 4.52 in Rust. It runs existing
OS/2 executables (16-bit NE, 16/32-bit LX) unmodified on x86-64 hardware.

## Guiding decisions

**64-bit kernel, 32/16-bit user space.** The kernel runs in long mode.
OS/2 processes run in *compatibility mode*, which executes 32-bit and 16-bit
protected-mode code natively with real segment descriptors. Nothing is
emulated or translated for application code. Virtual-8086 mode does not exist
in long mode, so DOS sessions (MVDM) will need a CPU emulator; that is
deliberately out of the first milestones.

**OS/2 memory layout is reproduced exactly.** Every process has its own LDT
in which the first 512 MB of the address space is *tiled*: LDT entry `n`
maps linear `n * 64K` with a 64K limit, so the 16:16 pointer `sel:off` and the
flat address `(sel >> 3) << 16 | off` name the same byte. This is what makes
mixed 16/32-bit code and thunking work. The private arena grows up from
64K, the shared arena grows down from 512 MB, and shared objects and system
DLLs have the same address in every process. The GDT uses the OS/2 selector
values that applications rely on: flat code `0x5B`, flat data `0x53` and the
per-thread TIB selector loaded into FS (`0x150B`).

**API entry is a kernel gate, not a fixed DLL.** The loader resolves imports
of kernel-implemented modules (DOSCALLS, and initially VIOCALLS, KBDCALLS,
MOUCALLS, MSG, NLS, SESMGR, QUECALLS) to stubs in a per-system gate object.
Each stub is `mov eax, id; int 0x99`. The kernel decodes arguments from the
caller's stack according to the API's convention (32-bit `_System`, or
16-bit far `_Pascal`), runs the Rust implementation and emulates the return
(`ret`, or `retf n` with the API's argument size). Because the stubs are real
code at real addresses, `DosQueryProcAddr` and friends behave normally.

User-mode libraries that are not kernel APIs in real OS/2 (PMWIN, PMGPI,
the Workplace Shell, the C runtime) are built in Rust for `i686` and packed
into genuine LX DLLs by the `rs2-link` tool, so they load like any other
module.

**Everything shipped is new code.** The IBM media in `iso/` is used to study
behaviour (Ghidra, the debug kernel's `.map` files) and to test
compatibility by running original binaries. None of it is copied into the
rs2 build or the install ISO.

**Kernel threads.** Work that has to wait in thread context without
belonging to an OS/2 process (setting up a hot-plugged USB stick, which
needs SCSI commands) runs on kernel threads (`task::kthread`): they run
on the kernel page tables, are scheduled like other threads and are fed
through an `Inbox` that interrupt handlers may post to. The kernel is
not preemptive: a thread runs until it blocks, yields or returns to user
mode with its time slice used up, and SpinLocks disable interrupts, so a
lock must never be held across a wait.

## Boot

1. UEFI firmware loads `\EFI\BOOT\BOOTX64.EFI` (`boot/`, Rust, `x86_64-unknown-uefi`).
2. The loader reads `\RS2\KERNEL` (ELF) and the boot volume image, records the
   memory map, GOP framebuffer and ACPI RSDP, builds the kernel page tables,
   exits boot services and jumps to the kernel.
3. The kernel (`kernel/`, `x86_64-unknown-none`) lives at `-2 GB`; all physical
   memory is mapped at `0xFFFF_8000_0000_0000`.

The install/live medium is a hybrid ISO 9660 + El Torito image whose EFI
boot image is a FAT file system. It is built by `xtask` without external
tools, so it can be written to a CD, a USB stick or attached to a VM.

## Repository layout

| Path | What |
|------|------|
| `crates/os2-exe` | `no_std` MZ/NE/LX parser, shared by the kernel loader and tools |
| `boot` | UEFI loader |
| `kernel` | The kernel |
| `xtask` | Host build tool: builds everything, makes disk images and ISOs, runs QEMU |
| `tools/lxdump` | Inspect OS/2 executables |
| `tools/ghidra` | Headless Ghidra scripts for reverse engineering |
| `tools/scripts` | One-off analysis scripts (Python) |
| `docs` | Design notes and research |
| `work/` | Git-ignored: extracted media, RE databases, build images |
