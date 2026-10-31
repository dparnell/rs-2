# rs2 architecture

rs2 is a re-implementation of IBM OS/2 Warp 4.52 in Rust. It runs existing
OS/2 executables (16-bit NE, 16/32-bit LX) unmodified on x86-64 hardware.

## Guiding decisions

**64-bit kernel, 32/16-bit user space.** The kernel runs in long mode.
OS/2 processes run in *compatibility mode*, which executes 32-bit and 16-bit
protected-mode code natively with real segment descriptors. Nothing is
emulated or translated for application code. Virtual-8086 mode does not exist
in long mode, so DOS sessions run on a CPU emulator: `VDM.EXE`, an
ordinary OS/2 process leading the session, runs rs2's own DOS machine
(BIOS, DOS kernel, DPMI host and COMMAND.COM in Rust, around an 80486
emulator); see [DOS.md](DOS.md).

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

**Scheduling.** Threads run strictly by priority: four classes (idle,
regular, foreground server, time-critical) times 32 levels, round robin
within one level, with a time slice of TIMESLICE milliseconds (32 by
default). A thread that becomes ready preempts a lower-priority one at
once (on another CPU through an IPI); a thread hogging the CPU in user mode
is switched out when its slice ends, because the timer interrupt brings
it into the kernel. Kernel code itself is not preempted. With
PRIORITY=DYNAMIC (the default) a regular-class thread that has been ready
for MAXWAIT seconds (3) without running gets one time slice at
time-critical level 0, above the regular and foreground-server classes;
PRIORITY=ABSOLUTE turns that off. DosEnterCritSec stops the process's
other threads on every CPU before it returns.

**Multiprocessor.** All processors the ACPI MADT lists are started at
boot (INIT/SIPI through a real-mode trampoline below 640 KB; `nosmp` on
the kernel command line keeps the others parked). Each CPU has its own
GDT and TSS (the TIB and LDT descriptors are per thread), LAPIC timer,
idle thread and per-CPU block, reached through the GS base: the trap path
runs `swapgs` on every transition, so OS/2 code can keep using GS as an
ordinary selector. User code runs on all CPUs at once; kernel code (API
calls, exceptions, kernel threads) runs under one big kernel lock that a
thread drops whenever it blocks or returns to user mode, while interrupt
handlers use only spin locks. Run queues are shared; a thread is never
resumed on one CPU while another is still switching away from it.
Unmapping or write-protecting pages flushes the TLBs of the other CPUs
that have the address space loaded (IPI shootdown). Thread-local memory
(DosAllocThreadLocalMemory) is at one address in every thread but each
thread has its own page there: a 2 MB range is reserved for it in every
address space, and each CPU running a process that uses it gets private
copies of the top page-table levels whose entry for that range maps the
running thread's page (`mm::tlm`); everything else stays shared, and
page tables the process adds later are copied in on the first fault. Finer-grained locking can replace the big lock subsystem by
subsystem later.

**Kernel threads.** Work that has to wait in thread context without
belonging to an OS/2 process (setting up a hot-plugged USB stick, which
needs SCSI commands) runs on kernel threads (`task::kthread`): they run
on the kernel page tables, are scheduled like other threads and are fed
through an `Inbox` that interrupt handlers may post to. The kernel is
not preemptive: a thread runs until it blocks, yields or returns to user
mode with its time slice used up, and SpinLocks disable interrupts, so a
lock must never be held across a wait.

**Interrupts.** The LAPIC timer drives the scheduler tick on every
CPU; legacy ISA interrupts (keyboard, mouse, serial) go through the
IOAPIC to vectors 0x20-0x2F; IPIs use 0xF0-0xF2. PCI INTx cannot be
routed without ACPI's AML (_PRT), so PCI devices interrupt through MSI
or MSI-X (`msi`): vectors 0x30-0xEF (except the API gate 0x99) are
handed out with a handler and a context word, the message (address
0xFEE00000 | APIC id << 12, data = vector, edge, fixed delivery) is
written to the device's MSI capability or MSI-X table, and the trap
dispatcher acknowledges the LAPIC and calls the handler. Messages stay
off while the PCI bus is probed (the LAPIC is not set up yet) and are
turned on, spread round robin over the online CPUs, once the other
processors are running. Handlers run in interrupt context without the
big kernel lock, use only spin locks (`try_lock` where a thread may hold
the lock) and hand over through an `msi::Event` (a signal counter and a
wait queue) or a kernel thread's inbox: a thread waiting for a disk
request polls the device briefly (a fast device answers within a few
register reads), then sleeps until the completion interrupt, which also
gives up the big kernel lock meanwhile. API calls may sleep although
they run with interrupts disabled (the gate is an interrupt gate; the
switch restores the flag); what may not is code holding a spin lock or
an interrupt handler (a per-CPU nesting count), and those poll. NVMe, AHCI,
virtio (blk, scsi, input, console, rng, net), xHCI, e1000e (82574L),
PVSCSI and LSI MPT use interrupts; the display queue of virtio-gpu and
devices without MSI (the 8254x NICs) are polled, and so is everything
with `nomsi` on the kernel command line or during boot, when drivers
probe with interrupts still off. Idle CPUs halt and only look at the
run queues when woken for a reason. `diskbench` on the kernel command
line measures idle CPU time, the tick's own cost and disk throughput
and latency.

**Networking.** The TCP/IP stack is a host-tested `no_std` crate
(`crates/tcpip`) that the kernel drives (`net`): NIC drivers implement a
small `Nic` trait, a NIC's receive interrupt (or, for NICs without
MSI, the timer tick) wakes the network kernel thread, which feeds the
stack and runs its timers; socket calls run the stack in the calling thread and wait on one
wait queue for stack events. rs2 programs use it through RS2API
ordinals 200-219, OS/2 programs through rs2's TCPIP32/SO32DLL/TCP32DLL
(user-mode DLLs over the same calls); see [NETWORK.md](NETWORK.md).

**Presentation Manager.** PM runs in the processes that use it, as on
OS/2: PMWIN.DLL keeps the window tree, queues and input state in one
named shared memory object (same address in every process) behind one
named mutex semaphore that is never held while application code runs.
A PM server process (PMSRV.EXE, started with the first PM program) owns
the desktop and a time-critical input thread that posts every key and
mouse event to the queue of the target window's thread at once, so a
program that stops reading its queue holds up nobody else; the server
handles such programs' frames itself. The kernel helps with RS2API
services (display mapping, raw input, a hardware cursor it moves itself,
a process-ended event) and by letting must-complete sections finish
before a killed thread dies. See [PM.md](PM.md). Objects that
several processes may draw (pointers and icons, a notebook's tab texts)
live in the shared state; container records stay application memory,
as on OS/2, and are only read on the container's own thread, so the
server paints a placeholder for a hung program's container instead.

**Profiles.** OS2.INI, OS2SYS.INI and the INI files programs open are
real OS/2 binary INI files (format in `crates/inifile`). PMSHAPI.DLL
serialises access with one named mutex for the whole system, keeps a
parsed copy per process that a generation counter in named shared
memory invalidates, and writes every change through to disk at once by
IBM's replace-by-rename scheme, so profiles are shared between
processes and survive crashes. See
[PM.md](PM.md#profiles-ini-files).

**The desktop.** rs2's own desktop shell, `RS2SHELL.EXE`, is an
ordinary PM program and the protected shell (PROTSHELL) of the
distributed system; the Workplace Shell's object model (SOM) is not
reimplemented yet. Its objects are OS/2's program list in OS2.INI
(PMSHAPI's `Prf*` program list calls) plus a few entries of its own
(icon positions, window places, associations), so the profile stays
the single source of truth across reboots and for other programs.
PMWIN helps with rs2-private calls: a desktop window pinned below all
others and a minimized window viewer hook. Shutdown ends with
`DosShutdown` and an RS2API call that enters ACPI S5. The boot loader
and kernel offer a text-mode fallback (T at the loader, `textmode`).
See [PM.md](PM.md#the-desktop-shell-rs2shell).

**Windowed text sessions.** A VIO session shown in a PM window ("OS/2
Window") stays a kernel session with its own virtual screen, keyboard
and mouse queues; only the framebuffer belongs to PM. A PM program,
`PMVIOWIN.EXE`, started by the kernel on demand, waits in the kernel
for screen changes (a generation counter, no polling), reads the cells
and draws them; PM's input thread hands keys for such a window straight
to the session, so a busy host or a hung text program never delays the
other. Alt+Home moves the session between its window and the full
screen. While a text session has the screen, PM draws into a shadow
buffer. See [PM.md](PM.md#windowed-vio-sessions-os2-window).

**Debugging OS/2 programs (DosDebug).** A program started with the
trace option (DosStartSession TraceOpt, DosExecPgm EXEC_TRACE) waits
before its first instruction for a debugger's DBG_C_Connect
(`kernel/src/api/debug.rs`). Its threads stop on their way back to user
mode (and a new thread before its first instruction) whenever the
debuggee is stopped; a breakpoint, single step, exception, new thread or
module stops the whole process and posts a notification, which the
debugger collects one per DosDebug call with the posting thread's
registers, as IBM's kernel does (studied in OS2KRNLB: DbgCall, DbgTrap,
DbgContinue). Registers are the stopped thread's saved user frame; the
FPU state its FXSAVE area, converted to FSAVE's layout. Watchpoints
(DBG_C_SetWatch) are the debuggee's DR0-DR3 and DR7, kept on its
Process and loaded by the context switch; as in IBM's DbgWPHit and
DbgWatchpoint they are one-shot, and a hit (a #DB in user or kernel
mode) is reported when the thread next returns to user mode. The kernel
copies to and from user memory through the physical frames, where the
debug registers do not see it, so `AddressSpace::read_bytes`/
`write_bytes` check the current process's watchpoints themselves (OS/2's
kernel uses the user's addresses, so its watchpoints catch an API
writing a result). `dbgtrace` on the
kernel command line logs every DosDebug call and result to the serial
port.

**Debugging a wedged system.** Ctrl+Alt+F12 (QEMU: `--type '{ctrl-alt-f12}'`)
prints to the serial port the device interrupt counts, every process's
threads with their state, priority, last user-mode address and the
kernel return addresses on their stacks, and then (through an IPI) where
each CPU is. Symbolise the addresses against the kernel ELF
(`target/x86_64-unknown-none/{debug,release}/rs2-kernel`, e.g. with
`llvm-nm -n`). Locks are only tried, so the dump works when something
holds them.

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
| `crates/x86emu`, `crates/dos` | The CPU emulator and the DOS machine of DOS sessions ([DOS.md](DOS.md)) |
| `boot` | UEFI loader |
| `kernel` | The kernel |
| `xtask` | Host build tool: builds everything, makes disk images and ISOs, runs QEMU |
| `tools/lxdump` | Inspect OS/2 executables |
| `tools/dosrun` | Run DOS programs on the host with rs2's DOS machine |
| `tools/ghidra` | Headless Ghidra scripts for reverse engineering |
| `tools/scripts` | One-off analysis scripts (Python) |
| `docs` | Design notes and research |
| `work/` | Git-ignored: extracted media, RE databases, build images |
