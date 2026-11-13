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
DLLs have the same address in every process. Above 512 MB, up to
VIRTUALADDRESSLIMIT, are the untiled high arenas of OS/2 4.5 (OBJ_ANY);
see "Memory layout". The GDT uses the OS/2 selector
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
ordinary selector. User code and kernel code run on all CPUs at once:
API calls, exceptions and kernel threads take the locks of the data they
touch, as described under "Locking" below (there is no big kernel lock).
Run queues are shared; a thread is
never resumed on one CPU while another is still switching away from it.
Unmapping or write-protecting pages flushes the TLBs of the other CPUs
that have the address space loaded (IPI shootdown, see "Locking").
Thread-local memory
(DosAllocThreadLocalMemory) is at one address in every thread but each
thread has its own page there: a 2 MB range is reserved for it in every
address space, and each CPU running a process that uses it gets private
copies of the top page-table levels whose entry for that range maps the
running thread's page (`mm::tlm`); everything else stays shared, and
page tables the process adds later are copied in on the first fault.

**Kernel threads.** Work that has to wait in thread context without
belonging to an OS/2 process (setting up a hot-plugged USB stick, which
needs SCSI commands) runs on kernel threads (`task::kthread`): they run
on the kernel page tables, are scheduled like other threads and are fed
through an `Inbox` that interrupt handlers may post to. They run
concurrently with API calls on other CPUs, under the same locks. The
kernel is not preemptive: a thread runs until it blocks, yields or returns
to user mode with its time slice used up. Spin locks disable interrupts
and must never be held across a wait; sleeping locks may be (see
"Locking").

**Time.** The LAPIC timer (1 kHz) is calibrated against the PIT's
channel 2, polled, and the time-stamp counter is measured in the same
50 ms. The boot processor's timer interrupt counts the ticks and makes
up ticks that came late (QEMU without KVM loses about a third) from the
TSC, a few per interrupt, so sleeps and timers keep to real time. The
system clock (`rtc.rs`) is the CMOS clock read at boot, put in phase
with the CMOS second edge (watched by the timer interrupt in the first
second), plus TSC milliseconds; four times a second it is compared with
the CMOS clock and moved forward if behind or held back if ahead, and
the times handed out (DosGetDateTime, the infoseg, file stamps) never go
backwards except through DosSetDateTime.

**Interrupts.** The LAPIC timer drives the scheduler tick on every
CPU; legacy ISA interrupts (keyboard, mouse, serial, the parallel
ports' acknowledge) go through the IOAPIC to vectors 0x20-0x2F; IPIs use 0xF0-0xF2. PCI INTx cannot be
routed without ACPI's AML (_PRT), so PCI devices interrupt through MSI
or MSI-X (`msi`): vectors 0x30-0xEF (except the API gate 0x99) are
handed out with a handler and a context word, the message (address
0xFEE00000 | APIC id << 12, data = vector, edge, fixed delivery) is
written to the device's MSI capability or MSI-X table, and the trap
dispatcher acknowledges the LAPIC and calls the handler. Messages stay
off while the PCI bus is probed (the LAPIC is not set up yet) and are
turned on, spread round robin over the online CPUs, once the other
processors are running. Handlers run in interrupt context, use only spin
locks (`try_lock` where a thread on the same CPU may hold the lock; a
thread holding a spin lock has interrupts off, so `lock` is safe for
locks only other CPUs hold) and hand over through an `msi::Event` (a
signal counter and a wait queue) or a kernel thread's inbox: a thread
waiting for a disk request polls the device briefly (a fast device
answers within a few register reads), then sleeps until the completion
interrupt. API calls may sleep although
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
(`target/x86_64-unknown-none/{checked,release}/rs2-kernel`, e.g. with
`llvm-nm -n`). Locks are only tried, so the dump works when something
holds them.

## Locking

Kernel code runs on every CPU at once. Each piece of shared state has a
lock of its own (`kernel/src/sync.rs`); what a lock guards is said where
it is declared.

**Lock types.**

- `SpinLock<T>`: disables interrupts on the CPU and spins; for short
  sections, the only kind interrupt handlers may take. It is not
  re-entrant: `c.space().foo(c.arg(0))` deadlocks (the receiver's guard
  is taken before the argument reads user memory), and so do two guards of
  one lock in one expression. Never held across a wait: the scheduler
  reports a thread that switches away with one held.
- `Mutex<T>`: a sleeping lock (a flag and a wait queue) for sections that
  wait: file systems (`vol`, a file's `entry`, an open file's position),
  the block cache (`Cache.inner`), each disk controller, the network
  stack, the LAN redirector's connections, DosSub* heaps. Thread context
  only.
- `RecursiveLock`: a sleeping lock without data that its holder may take
  again (thread context only, never under a spin lock: checked builds
  assert it). Four of them:
  - the loader lock (`loader::lock()`), held across a whole load
    (lookup, file reads, the image, its imports, the registry) and until
    the module is recorded in the process that asked for it (DosExecPgm
    until the process is listed, DosLoadModule until `p.loaded` is
    updated), as IBM's loader semaphore. DLL init routines run in user
    mode, outside it: the thread about to run one claims it
    (`api/module.rs`), and another that reaches the same routine waits in
    the kernel (gate slot (0, 2046)) until it has returned, so a global
    initialisation runs once however many processes load the DLL at once;
  - the session lock (`session::SWITCH`), around everything that changes
    which session exists or is shown: selecting, Alt+Esc, the window
    list, pop-ups put up and taken down (VioPopUp, hard errors), creating
    and ending sessions, windowed/full screen, the PM session's creation,
    joining and end;
  - the mount lock (`fs::MOUNTING`), around mount, unmount and remount,
    and held by whoever picks a free drive letter until it is mounted
    (the hot-plug thread, NET USE);
  - the boot chain's (`boot::CHAIN`): starting the next CALL=/RUN=
    program or the shell, and checking an ended process against it.
- The keyboard's hot keys (Ctrl+Esc, Alt+Esc, Alt+Home, the window
  list's keys, PrtSc) arrive in interrupt context but switch sessions,
  which may start PMVIOWIN or print: the interrupt hands them to the
  session thread (`session::post`, an `Inbox`), and so every key typed
  while such work is queued or the window list is open, so keys stay in
  order. The session thread also starts the windowed sessions' PM host
  when a select needs it, outside the session lock.
- `WaitQueue`: not a lock, but `wait_until(ready)` calls `ready` with the
  queue's own spin lock held, so a condition that takes another lock
  makes an order "queue -> that lock"; code that holds that lock must
  not wake the queue (pty waits look at a change count for this reason).
  Waits are interruptible unless they say otherwise; a kill or signal is
  noticed under the scheduler lock (`sched::block_interruptible`), so
  one set on another CPU between the check and the sleep is not lost.
- Atomics, with `fetch_update`/`compare_exchange` for read-modify-write
  (counters, nesting counts, the clock base).
- Device locks: each UART's registers (`chardev`; COM1's is the kernel
  log's too), the parallel ports' owner and settings, the PCI
  configuration port (`pci::modify32` for read-modify-write), the CMOS
  port pair (held with interrupts off), the display's mode and PM
  mapping (`display::MODE`), the LAN requester's administration
  (`redir::ADMIN`: start, stop, logon, NET USE, held across a use's
  network I/O), DosShutdown, DosSetDateTime.
- DosDebug: the debugger reads and writes a debuggee thread's saved
  registers only under `DEBUGGEES` while the thread is parked or blocked
  in another kernel wait (`with_frame`); coprocessor registers it writes
  are loaded by the thread itself when it wakes.

There is no big kernel lock. Until this was finished the API gate took
one for the calls not converted yet (and `bkl` on the command line for
all of them, which is how the measurements below were made).

**Lock order** (outer before inner; a lock is never taken while one
further down is held):

1. The boot chain lock, the LAN requester's `ADMIN`, the windowed
   sessions' host start (`viowin::STARTING`).
2. The loader lock, then the mount lock.
3. Sleeping locks, outside in: an open file's position (`OpenFile.pos`,
   `Dasd.pos`), the LAN redirector's server connection, a file's
   `entry`, the volume (`vol`), the block cache (`Cache.inner`; another
   cache's only by `try_lock`), the network stack, a disk controller or
   USB storage unit (`Lun.cache` before `Bot.state`), DosSub*'s lock.
4. The session lock: taken inside all of the above (a hard-error pop-up
   comes from a disk read under `vol`), so nothing under it reads a
   disk, starts a program or takes the loader lock.
5. Spin locks of named objects and tables: semaphores (`TABLE` before a
   semaphore's `opens`, `SYS16`), queues, timers (`TIMERS` before the
   semaphore table: a timer posts with its list locked), file locks, the
   loader's registry, shared memory (`OBJECTS`), named pipes, sessions
   (`LIST` before `SCREEN`, the mouse before `SCREEN`, the console's
   `TTY` before `SCREEN`, `SCREEN` before `DAMAGE` and the viowin and
   pty wait queues), xHCI's `CTRLS` before the HID and keyboard state.
6. Per-process locks: `handles`, `threads` (before `exit`), `critsec`,
   `exitlist`, `loaded`, `inited`, then the address space (`space`).
   Two address spaces are only nested in DosDebug's alias bookkeeping,
   the debuggee's before the debugger's.
7. Memory: the shared arena (`ARENA`) before the high private ends
   (`ENDS`); a shared object's `pages` and `frames`.
8. Wait queues' own locks (`waiters`): inside whatever the waker holds,
   outside whatever a `ready` condition takes.
9. Leaves, taken inside anything: the process table (`PROCESSES`), the
   scheduler (`SCHED`), the kernel heap (`HEAP`), the frame allocator
   (`FRAMES`), the log.

Some sequences are check-then-act by nature and keep the check and the act
under one critical section: a name is checked and the object listed under
the table's lock (semaphores, shared memory), a file's one object is looked
up and created under its volume's lock and the open map's lock, a thread's
ID is chosen and the thread listed together, "the last thread of a
process" and "the last open of a semaphore" are decided under the list's
lock, a dead thread or child is collected by whoever takes it out of the
list, a session's leader is recorded by exec before the process runs, a
socket call reaches the stack only if its handle is still open (checked
with the stack locked, under which soclose closes it), a drive letter is
picked and mounted under the mount lock.

**Checking.** In builds with debug assertions (`xtask run`'s `checked`
profile) `kernel/src/lockdep.rs` tracks the spin locks each CPU holds and
the Mutexes each thread holds. A lock's class is where it was made
(`#[track_caller]` on the constructors; a lock inside a structure built in
one place is one class for every object). Every nesting is recorded as a
pair of classes; a pair seen in both orders, which can deadlock on two
CPUs, is reported once on the serial port with both creation sites and a
backtrace, and so is a spin lock taken again by its holder (which would
otherwise hang silently). Same-class nesting (two processes' locks) is not
checked. `RS2\STRESS.EXE smp` races threads and processes on the same
kernel objects and times a kernel-call load; `STRESS corner` runs
pop-ups, sessions started, selected and stopped, and keyboard and disk
IOCtls from several processes at once.

**TLB shootdowns.** Changing a mapping that other CPUs may have cached
bumps each target CPU's request count, sends it an IPI and waits until
its done count catches up. A CPU spinning for a lock has interrupts off
(API calls run with IF=0) and may be spinning for a lock the initiator
holds, so every spin loop (`SpinLock::lock`, the shootdown wait itself,
the CMOS lock) does the flush asked of it (`smp::service_tlb`); several
CPUs may shoot down at once. A CPU leaves an address space's CPU mask
only after CR3 has moved on, and freeing an address space waits until no
other CPU has it loaded.

**Measured** (QEMU 4 CPUs with multi-threaded TCG, `-- -accel
tcg,thread=multi`; without it QEMU runs one CPU at a time and nothing can
be faster): `STRESS smp`'s kernel-call mix (DosAllocMem/touch/DosFreeMem,
event post/reset, DosQuerySysInfo), 1500 rounds each in four processes,
took 123 ms against 420 ms with everything under the big kernel lock
(`bkl`, before it was removed; one process alone: 93 ms); four
threads contending for one mutex 256 ms against 460 ms, four processes
157 ms against 349 ms. In one process
the same mix on four threads stays slower than one thread either way:
every DosFreeMem shoots down the other three CPUs' TLBs.

## The block cache

Every volume on a fixed disk or USB stick reaches its device through
`fs/cache.rs`, which `harderr::wrap` puts below the hard-error pop-ups, so
HPFS, FAT, JFS and DASD handles all share it and see the same blocks.
Blocks are 4 KB page frames with a bit per sector for valid and dirty. The
frames are a budget shared by the volumes: an eighth of memory (at most
256 MB), or `IFS=...HPFS.IFS /CACHE:n` in KB. IBM's limit of 2048 KB is not
kept, so a CONFIG.SYS written for IBM's driver gets a small cache. A volume
under its share takes frames from one over it. Memory the cache holds does
not come back when programs run short of it.

Measured with yum and RPM under QEMU TCG, 95% of the sectors read were read
before, because every path lookup walks the directory B-tree from the root:
`yum repolist` made 530569 read requests, and with the cache 1764 of them
reach the disk.

| Command, release kernel, seconds | `nocache` | cache |
|---------|------|------|
| `rpm -qa`, second run | 0.51 | 0.37 |
| `rpm -e --nodeps --justdb cube` | 1.35 | 0.72 |
| `rpm -ivh --justdb --force E:/rpms/[a-c]*.rpm` | 2.56 | 1.11 |
| `rpm -ivh --justdb --force E:/rpms/*.rpm` | 10.8 | 5.2 |
| `yum repolist` | 27.2 | 8.2 |
| `yum -y install zip` | 36.8 | 12.2 |

Those are single runs, and the yum ones include the network. `xtask run`
without `--release` builds the `checked` profile: optimised like release, with
the overflow checks and `debug_assert`s of a debug build. It is close to
release (`yum repolist` 9.8 s, `yum -y install zip` 13.1 s, `rpm -ivh` of all
6.2 s). A kernel with no optimisation at all took 83 s, 83 s and 31 s, and
gained only about 20% from the cache, so don't time anything on one.
After the cache, about half of what remains in yum is the file system code:
DosFindFirst and DosQueryPathInfo walking cached blocks.

**Reads.** A miss reads its 4 KB block; a miss right after the last one
reads up to 32 KB. Requests over `/CRECL:n` (default here 64 KB; IBM's is
4 KB) go straight to the device, with any dirty sectors laid over a read
and any cached sectors of a write forgotten.

**Writes.** The lazy writer (a kernel thread, every 100 ms) follows
HPFS.IFS: a block is written when the disk has been idle for DiskIdle
(1000 ms), when it has not changed for BufferIdle (500 ms), or when it has
been dirty for MaxAge (5000 ms), all of it when more than DirtyMax blocks
(four fifths of a share) are dirty. The writer's own requests do not count
as the disk being busy. Writes merge into requests of up to 64 KB, in disk
order. `CACHE` sets the times and `-LAZY:OFF` stops it; there is no need for
IBM's `RUN=CACHE.EXE`, lazy writing is on from the start. FAT follows the same
settings, and `DISKCACHE=` is not read. Under QEMU writes and flushes are
under 2% of these workloads (the host's page cache takes them), so lazy
writing gains little there; it is for real disks.

What forces blocks out:

- **DosResetBuffer** writes the whole volume and then flushes the disk.
  IBM's FS_COMMIT writes one file and issues no device flush; rs2 keeps no
  owner per block, so a commit is a superset of IBM's.
- **OPEN_FLAGS_WRITE_THROUGH and OPEN_FLAGS_NO_CACHE**: each DosWrite ends
  with the volume written out, for the same reason.
- **Closing a file does not**, as in IBM's HPFS: data stays lazy.
- **DosShutdown**, Rs2PowerOff, the reboot request and the end of the
  `init=` program write everything. After DosShutdown lazy writing is off.
- **DosPhysicalDisk** goes past the volumes: everything is written first, and
  a write forgets all cached blocks.
- **Remounting** (FORMAT, CHKDSK /F, a new medium) writes out and forgets.

**What does not write lazily.** JFS: its log depends on the order of writes,
so it stays write-through under the read cache. USB disks can be pulled out,
so they are write-through too, and `fs::unmount` drops their blocks: reads
fail, and the next stick on that letter starts empty. Optical drives, RAM
disks and devices without 512-byte sectors have no cache.

**After a crash.** Anything not forced out is lost, up to MaxAge old, in no
particular order: nothing orders metadata, as in IBM's HPFS. HPFS keeps the
spare block's dirty flag as IBM does: set and flushed before the first write
of a mount, cleared by DosShutdown after everything is out, and left alone if
it was set at mount (CHKDSK /F clears it). FAT keeps the boot sector's flag
the same way, on volumes that write lazily: bit 0 of byte 25h, where OS/2's
kernel has it (FatDirtyBitToggle), byte 41h on FAT32. JFS mounts such a volume
read-only.

**Names.** kLIBC checks every component of every path, so installing 3,800
files made 203,000 DosFindFirst calls, half of the time, and each took apart
the DNODEs from the root down. `hpfs::Volume` keeps the answers, found or not,
each under the DNODE that decides it: the one holding the DIRENT, or the leaf
where the name would be inserted. All DNODE writes go through `write_dnode`,
which forgets what that DNODE decided, so nothing else has to know. The same
install went from 62.8 to 40.3 seconds, and block requests from 5.0 million to
410,000. `name_cache_stays_true` checks answers against the disk through random
changes.

Above the file systems, `fs::mount` wraps FAT, HPFS and JFS volumes in
`namecache::CachedFs` (`crates/pathcache`), which answers `stat` for directories
and for names that do not exist. Files are left out: their size and times also
change through handles, which it does not see. Every path operation forgets the
path, what is below it and its directory; an answer a change has overtaken is
dropped by its generation number. Keys fold ASCII case only, so a path that could
have another spelling (not ASCII, a trailing dot or space, on FAT anything but
8.3) is not kept, and a change through one forgets everything. Network drives are
not wrapped. With both caches the install above takes 31.6 seconds on HPFS; on
JFS, which has only this one, 83 seconds. `CACHETST NAMES dir` checks a volume
against a model.

**AUTOCHECK.** During boot, before RUN and CALL programs and the shell, the
kernel runs `CHKDSK d: /F /AUTOCHECK` on each volume that is dirty and listed
in CONFIG.SYS: `IFS=...HPFS.IFS /AUTOCHECK:DE`, `IFS=...JFS.IFS /AUTOCHECK:*`
or `DISKCACHE=...,AC:C`. The IFS line does not decide the file system: any
listed drive is checked. A dirty drive not listed gets a line on the console.
The CONFIG.SYS rs2 ships has `/AUTOCHECK:*`; updating an installation keeps
the user's CONFIG.SYS, which then needs the line added by hand.
IBM's CHKDSK.SYS runs before the volumes are in use; rs2's CHKDSK is an
ordinary program, which works on the boot drive because the loader keeps no
files open.

`CACHE -STATS:RESET` and `-STATS:SHOW` (`fs/iostat.rs`) report on the log
what a workload did: requests at the file systems and at the devices, sectors
read again, time in each API call and in exceptions. `nocache` on the kernel
command line runs without the cache, for comparing. `CACHETST` (RS2\) tests
it: `MODEL dir` checks random I/O of every size against a copy in memory,
`MARK dir` leaves files for looking at a disk image after a crash.

## Memory layout

A process's address space (`kernel/src/vm.rs`) is the user half of its
own x86-64 page tables. The kernel lives in the upper half (`-2 GB`, and
physical memory at `0xFFFF_8000_0000_0000`); nothing of it is mapped below
4 GB, so the whole 32-bit range is the program's. The layout follows
IBM's Warp 4.5 kernels (studied in the debug kernel `os2krnld` of Warp
4.52, 14.086: `PGSetupUserAddressSpace`, `VMAllocMem`,
`VMQueryArenaAvailMem`, `vmRecalcShrBound`, `VR32AllocMem`,
`VM32ApiAllocSharedMem`, the CONFIG.SYS handler, and DOSCALL1's
`DOS32FLATTOSEL`):

```text
0x00000000  first 64 KB: never mapped (null pointers)
0x00010000  private arena, bottom up                   \  tiled: LDT entry n
            ...                                         |  is the 64 KB at
0x04000000  lowest the shared arena goes                |  n << 16, so 16:16
            shared arena, top down (DLLs, shared memory)|  and flat pointers
0x1ffe0000  the API gate (128 KB)                       /  convert
0x20000000  high private arena, bottom up              \  "high memory":
            ...                                         |  no tiles, 32-bit
            high shared arena, top down                 |  code only
limit       VIRTUALADDRESSLIMIT (default 1 GB)         /
```

Objects start on 64 KB boundaries in every arena. Shared objects (named
and unnamed shared memory, DLL objects) have one address in every
process: their ranges are reserved in one system-wide map.

**VIRTUALADDRESSLIMIT** (CONFIG.SYS, in MB) sets the top of the high
arenas. As in IBM's kernel: a decimal number, default 1024, from 512 (no
high memory at all); a value out of range is reported and the default
kept; the value is rounded up to a multiple of 64 MB, and
DosQuerySysInfo(QSV_VIRTUALADDRESSLIMIT) returns the rounded value. IBM
accepts up to 3072, because its own system arena lies above the limit;
rs2's kernel needs no room there, so rs2 accepts up to 4032 (the top
64 MB stay unused so that no object's end wraps to 0). Programs that
treat addresses above 2 GB as negative fail on IBM's kernel with 3072 as
well.

**The high arenas.** IBM gives each side an eighth of the high region
(limit − 512 MB) for itself: the high private arena never reaches the top
eighth, the high shared arena never the bottom one; between them the
boundary moves. rs2 does the same: a high private object goes below the
lowest high shared object (and below the top eighth), a high shared
object above the highest high private object of any process (and above
the bottom eighth). Nothing above 512 MB has LDT tiles: 16:16 pointers
cannot reach it, `DosFlatToSel` of an address there returns 0 (as IBM's
DOSCALL1 does: addresses below 64 KB come back unchanged), and the kernel
gives such objects no selector.

**OBJ_ANY.** `DosAllocMem` with OBJ_ANY takes memory from the high
private arena when it has room there, else from the low one (tiled), as
`VR32AllocMem` does; OBJ_ANY with OBJ_TILE is ERROR_INVALID_PARAMETER.
`DosAllocSharedMem` with OBJ_ANY tries the high shared arena first. All
other memory calls (DosFreeMem, DosSetMem, DosQueryMem,
DosGetSharedMem/DosGiveSharedMem/DosGetNamedSharedMem, DosAliasMem,
DosQueryMemState, DosDebug's aliases) work on high objects unchanged;
DosAliasMem's alias is always low and tiled.

**DosQuerySysInfo**: QSV_MAXPRMEM, QSV_MAXSHMEM, QSV_MAXHPRMEM and
QSV_MAXHSHMEM add up the free 64 KB units of the calling process's
private arena, the shared arena, and the two high arenas (between their
moving boundaries), as `VMQueryArenaAvailMem` does; the high ones are 0
without high memory.

**DLLs in high memory.** Later OS/2 4.5 kernels load DLL objects whose LX
object flag 0x00010000 (OBJHIGHMEM, set by the community `highmem` tool)
is set into the high shared arena. Warp 4.52's own kernel (14.086) does
not yet: its loader's flag table (`_objflags`) has no such entry. rs2
does as the later kernels: a 32-bit DLL object with OBJHIGHMEM goes high
if the module can be moved (it carries its internal fixups), unless it
needs 16:16 aliases (OBJALIAS16) or is a resource; everything else, and
every EXE object, stays low. `elf2lx --highmem` marks rs2 DLLs so
(APITEST's TESTDLLH).

**Physical memory.** The frame allocator is a bitmap over all RAM (more
than 4 GB is fine). User pages come from frames above 4 GB first, so the
low ones stay for devices that can only address 32 bits. Thread-local
memory's per-CPU page-table copies pick up page directories added above
1 GB on the first fault there, as they do below. `cargo xtask run --mem
SIZE` sets QEMU's memory (512 MB by default).

## Boot

1. UEFI firmware loads `\EFI\BOOT\BOOTX64.EFI` (`boot/`, Rust, `x86_64-unknown-uefi`).
2. The loader reads `\RS2\KERNEL` (ELF) and the boot volume image, records the
   memory map, GOP framebuffer and ACPI RSDP, builds the kernel page tables,
   exits boot services and jumps to the kernel. Before that it waits a
   moment (`bootwait=MS`, SETBOOT /T) for a key: T adds `textmode` to the
   kernel command line (the command prompt instead of PROTSHELL's
   desktop), R or F1 adds `recovery` (the kernel gives the boot programs
   `RS2RECOVERY=YES`, and the first PMSHELL shows the Recovery Choices,
   [WPS.md](WPS.md#the-shell-process)). The loader itself reads nothing
   but the EFI partition. The command line comes from `\RS2\CMDLINE`; the
   boot information (`crates/bootinfo`, magic `RS2BOOT2`) has room for
   1023 bytes of it, so a kernel and a loader must come from the same
   build (the kernel stops with "bad boot info" otherwise).
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
| `crates/msgf`, `tools/mkmsgf` | OS/2 message files: MKMSGF's compiler, a reader and a decompiler ([PM.md](PM.md#rs2s-own-messages-and-help)) |
| `userland/msg`, `userland/help` | rs2's own system message files (MKMSGF source) and help books (IPF source) |
| `userland-std` | Programs using Rust's std: rs2libc (the C library std needs, over the OS/2 API), STDTEST, webcore and the web browsers WEBTEXT and WEB ([BROWSER.md](BROWSER.md)) |
| `tools/ghidra` | Headless Ghidra scripts for reverse engineering |
| `tools/scripts` | One-off analysis scripts (Python) |
| `docs` | Design notes and research |
| `work/` | Git-ignored: extracted media, RE databases, build images |
