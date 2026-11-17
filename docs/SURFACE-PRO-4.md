# rs2 on the Surface Pro 4

What it takes to run rs2 on a Microsoft Surface Pro 4 (2015, Intel
Skylake), including WiFi and the touch screen. This is a plan, not a
status report: nothing here has been tried on the device yet, and rs2
has not run on real hardware of any kind before (everything so far has
been in QEMU, VirtualBox and VMware).

## The hardware, and what rs2 has for it

| Part | Surface Pro 4 | rs2 today | Work |
|---|---|---|---|
| Firmware | UEFI only (no CSM), Secure Boot on by default | UEFI loader (`boot/`), hybrid ISO that boots from a USB stick (`xtask iso`) | Turn Secure Boot off in Surface UEFI (hold Volume Up while powering on) |
| CPU / chipset | Skylake m3/i5/i7 (2-4 threads), Sunrise Point-LP PCH | ACPI MADT, local and I/O APICs, SMP, MSI/MSI-X, PCIe ECAM (MCFG) | Check the real-hardware paths: no PS/2 controller, possibly no HPET/PIT, AML that QEMU never produces |
| Memory | 4, 8 or 16 GB | Frames above 4 GB are used (`mm/phys.rs`) | Check the UEFI memory map holes on a real machine |
| Display | 12.3" 2736x1824, Intel HD 515/520/540 | GOP framebuffer from the loader; software rendering | Probably only the native mode. A 2x scaled output (PM at 1368x912) and write-combining (PAT) are needed for usable speed and size |
| Storage | NVMe SSD (Samsung PM951 or Toshiba, 128 GB-1 TB) | `drivers/nvme.rs` | Real-controller details (doorbell stride, queue sizes, APST power states). Most likely works as is |
| USB | 1x USB 3.0 Type-A, Sunrise Point xHCI (8086:9D2F) | `drivers/xhci.rs`, hubs, HID boot protocol | Intel xHCI quirks (port routing, BIOS handoff, USB 2 companion ports) |
| Type Cover | USB HID device on the internal xHCI (keyboard, consumer keys, precision touchpad) | Boot-protocol keyboards and mice only | The keyboard should work in boot protocol. The touchpad needs a HID report-descriptor parser (or still works as a boot mouse) |
| Touch screen + pen | Intel Precise Touch & Stylus (IPTS): touch controller behind the Intel ME, reached through MEI (HECI) | None | New: MEI driver plus IPTS client (see below) |
| WiFi | Marvell AVASTAR 88W8897 (PCIe; Bluetooth on USB) | None (rs2 has wired NICs only) | New: mwifiex-style driver, 802.11 association, WPA2 (see below) |
| Audio | Realtek ALC298 on HD Audio (8086:9D70) | `drivers/hda.rs` | Probably the codec's amplifier/pin setup. Low priority |
| Buttons | Power and volume buttons on GPIOs (ACPI MSHW0040) | ACPI power-off only | Needs a real AML interpreter (GPIO controller + `_DSM`). Low priority |
| CPU power | Skylake HWP, MWAIT C-states to C10, RAPL | `cpupower.rs`: power plans (POWERCFG, the Power object's Plan page), MWAIT idle, tickless idle APs, HWP requests, RAPL readings | Untested on the Surface: check `POWERCFG /WATCH` (package watts on each plan) |
| Battery | ACPI control-method battery behind the EC | AML interpreter (`crates/aml`), EC driver (`ec.rs`), PNP0C0A/ACPI0003 into `APM$`, POWERCFG and the Power object (`acpipm.rs`); checked in QEMU with a test SSDT | Not tried on the Surface: its DSDT, its EC (ECDT or PNP0C09) and the `_Qxx` events; look for `acpi: AML:` and `acpi: battery` in the boot log |
| Cameras, sensors | IPU3-class cameras, sensor hub | None | Not planned |

## Phase 0: get any log out of the machine

The SP4 has no serial port. Done (2026-11-15):

- **The log in memory** (`kernel/src/klog.rs`): everything written to the
  serial port also goes into a 256 KB ring buffer.
- **`bootlog`** on the kernel command line: a kernel thread copies the
  ring to `\RS2\BOOT.LOG` on the boot volume (the drive holding
  `\RS2\KERNEL` that is not the RAM C: or a CD: on a live USB stick, the
  stick's EFI partition) every 5 s while the log grows. Make the stick
  with
  `cargo xtask iso --dist -o work/rs2-live.iso --cmdline 'bootlog bootdiag'`. Nothing
  is written from a crashed kernel, so the file can miss the last
  seconds; the QR code below has them. Nor to a boot volume that was not
  shut down cleanly until AUTOCHECK's CHKDSK /F has been over it (a line
  on the log says so); FAT on a USB stick is written through and never
  gets the dirty flag.
- **The crash screen** (`kernel/src/crash.rs`): a kernel panic or kernel
  trap fills the framebuffer with what happened in large text (message,
  registers, backtrace), the end of the log, and a QR code holding a crash
  report: a header (version, uptime, command line) plus as much of the
  log as fits, compressed (`crates/crashqr`: zlib, base45 into the QR
  alphanumeric mode, up to version 40; 20-25 KB of log). Scan it with any
  phone QR reader and decode it on the development machine:
  `python3 -I tools/scripts/crashqr.py 'RS2C1:...'`, or give it a photo
  or screenshot (decoded by macOS Core Image through
  `tools/scripts/qrdecode.swift`). The screen code allocates nothing and
  takes no lock another CPU may hold. At 2736x1824 the code is drawn at
  about 6 pixels per module.
- **`bootdiag`** (2026-11-15; the first live stick stopped with the
  loader's text on screen, after "exiting boot services", and nothing
  else): the loader prints the framebuffer it found (size, stride, pixel
  format, address; "none usable" if GOP gave none the kernel can draw
  on), the top of memory and where the page tables, stack and boot info
  are, then gives up after 16 failed ExitBootServices instead of looping.
  Then every stage draws the next square in a row at the top right of
  the framebuffer, straight into its pixels, so even a hang before the
  kernel's screen works shows how far boot got. Squares are 16 pixels on
  a 1024-wide screen, 32 at 2736x1824. Blue = the loader: 0 before
  ExitBootServices (firmware still up; if it is missing, drawing into the
  framebuffer does not work at all), 1 boot services left (red instead:
  the firmware refused ExitBootServices), 2 jumping to the kernel. Green
  with every fifth yellow = kernel stages, which also print
  `bootdiag: N name` once the screen works (and `bootdiag: pci ...` for
  each PCI function before its driver probes it). `bootdiag=N` stops
  after square N, to photograph the kernel's text before the desktop
  covers it. The squares: 0-2 loader, 3 kernel entered, 4 serial,
  5 screen, 6 gdt, 7 smp::init_bsp, 8 idt and fpu, 9 mm, 10 acpi, 11 rtc,
  12 irq, 13 sched and fs, 14 vmware, 15 drivers, 16 loader/infoseg/perf,
  17 kbd, 18 mouse, 19 chardev, 20 interrupts on, 21 apic, 22 other CPUs,
  23 kernel threads, 24 boot thread, 25 CONFIG.SYS read, 26 network,
  27 SYSDLLS/RUN=, 28 starting programs, 29 first program started
  (2026-11-16: the second stick still stopped after "exiting boot
  services" with no squares seen, so square 0 was added before it and
  the squares grew on large screens)
  (`bootdiag.rs`; a QEMU boot with `bootdiag` prints the current list).
- **The panel's own mode** (2026-11-16: no squares at all, not even
  the one before ExitBootServices, with "framebuffer 1024x768 stride 1024
  format 1 at 0x90000000"): the loader used to switch every machine to
  1024x768. The Surface's GOP offers that mode, but only as a buffer its
  Blt scales onto the 2736x1824 panel, so the firmware's text showed and
  nothing written straight into the "framebuffer" (squares, the kernel's
  screen) ever did. On real hardware (no CPUID hypervisor bit) the loader
  now picks the largest mode it can draw on unless `video=` asks for
  another; virtual machines still get 1024x768. A bitmask mode (GOP
  format 2) whose masks are plain RGBX or BGRX counts as drawable (the
  first try, keeping the firmware's mode, still ended in 1024x768: the
  Surface's native mode was presumably refused for its pixel format).
  With `bootdiag` the loader lists every mode, with format and masks,
  after it switches, and writes all it printed to `\LOADER.LOG` on the
  boot volume just before ExitBootServices (the list scrolled off the
  Surface's screen too fast to read).
- **ExitBootServices refused** (2026-11-16, with the native mode kept:
  one blue square, then a red one: the firmware refused
  ExitBootServices, every try, which is also where the very first stick
  stopped). The loader allocated everything the kernel keeps as memory
  type 0x80000001 (OS-defined), which some vendors' firmware mishandles;
  OVMF and VMware never minded. Now it allocates plain EfiLoaderData,
  remembers the ranges itself and hands them to the kernel as
  `MemoryKind::Loader` (QEMU: the same 454 MB usable of 510 as before).
  If the firmware still refuses, the second row shows its error number
  as that many red squares (2 = invalid parameter).
- **Type Cover touchpad** (2026-11-16: the desktop came up, the
  touchpad did nothing): the USB HID driver only knew boot keyboards and
  boot mice, and a precision touchpad is neither. HID interfaces that are
  not boot devices now have their report descriptor read
  (`drivers/hidreport.rs`); the first Mouse application collection with
  an X and a Y becomes a mouse, relative or absolute, with or without a
  report ID. The touchpad sends such mouse reports until a host switches
  it to touchpad mode, which we never do. The log names each interface
  (`interface N: mouse in its reports ...` or `no mouse in its ...`).
  Tested in QEMU with `cargo xtask run --usb-tablet` (absolute, no report
  ID) and the parser alone on a touchpad-style descriptor (relative,
  report ID 1, a digitizer report before it).
  Confirmed on the Surface Pro 4 the same day: the touchpad moves the
  pointer and clicks, and rs2 installed from the live stick to the
  internal drive.
- **Type Cover keyboard** (2026-11-16, after the install: no keys): its
  keyboard is presumably on the touchpad's report-protocol interface
  under its own report ID, not a boot keyboard. The report descriptor
  is now searched for a Keyboard application collection too (modifiers,
  a key array or a key bitmap), and its reports are turned into boot
  keyboard reports for the existing code (scancodes, repeat, Ctrl+C).
  Ignored when the device also has a boot keyboard interface; no LEDs
  for such keyboards yet. Checked: the parser alone on a keyboard plus
  mouse descriptor (report IDs 1 and 2) and a bitmap keyboard; QEMU's
  boot keyboard and usb-tablet unchanged. Not yet on the Surface.
- **Boot log on an installed system** (2026-11-16: the system installed
  from the stick stopped during boot, and there was no BOOT.LOG to read):
  the installer copies `bootlog` and `bootdiag` from the live stick's
  `\RS2\CMDLINE` to the installed one, after `root=`. That system then
  writes `\RS2\BOOT.LOG` on its EFI system partition (the small FAT
  partition, a drive letter of its own when the live stick is booted).
  On a system installed before that: boot the stick and
  `ECHO bootlog>>X:\RS2\CMDLINE`, X: being the drive whose
  `\RS2\CMDLINE` starts with `root=`.
- **The previous boot's log** (2026-11-17): bootlog renames the last
  boot's `\RS2\BOOT.LOG` to `\RS2\BOOTPREV.LOG` before writing this
  one's, so a boot that went wrong (with the Surface Dock attached, USB
  stopped working) can be read on the next boot. A boot volume left
  dirty gets its log only after AUTOCHECK's CHKDSK (the installed
  CONFIG.SYS checks every drive, the EFI partition too).
- **Which drive BOOT.LOG goes to** (2026-11-17): bootlog wrote to the
  first drive holding `\RS2\KERNEL`. Booted from the live stick with
  rs2 installed on the internal disk, that was the internal EFI
  partition, not the stick: two live boots replaced the installed
  system's BOOT.LOG and BOOTPREV.LOG (the log of a boot with the dock was
  lost so). The drivers now note the partition the firmware booted from
  (by position and GPT GUID, `drivers::BOOT_DRIVE`) and bootlog writes
  only there; on a live boot where it is not known, nowhere.
- **A live boot whose stick dies** (2026-11-17): with the dock, USB stops
  and the stick with it, so a live boot could keep no log. After three
  failed saves to the stick (or with no stick drive at all), bootlog
  writes `\RS2\LIVEBOOT.LOG` (the one before: `LIVEPREV.LOG`) on another
  drive holding an rs2 kernel, the internal disk's EFI partition. The
  installed system's BOOT.LOG and BOOTPREV.LOG are not touched.
- **Surface Dock at boot: all USB stops** (2026-11-17): a text-mode boot
  with the dock attached ends with `xhci 00:14.0: port 7: timed out (step
  EnableSlot)` over and over (port 7 is the Type Cover). Commands on the
  xHCI command ring run one at a time, and rs2 never aborted one: a
  command the controller does not complete (probably an Address Device
  to something behind the dock) blocked every later one, so no device
  could be enumerated. Now a command not completed 5 s after reaching the
  head of the ring is aborted (CRCR Command Abort; it completes as
  "command aborted", the ring is restarted on what is left), with a line
  naming the command type, port and slot; steps waiting behind another
  command are not timed out.
  - The dock boot's LIVEBOOT.LOG named it: Address Device (TRB type 11)
    for root port 14 (a USB 3 port: the dock's USB 3 hub). rs2 did not
    reset USB 3 ports ("they enable themselves"), so the hub the
    firmware had already addressed and configured was still in that
    state. It probably held the SET_ADDRESS off with NRDY, which USB 3
    allows forever. Every port is now reset before its device is
    addressed, as Linux does (a hot reset; a warm reset for a USB 3 link
    left Inactive or in Compliance).
  - After the abort that boot logged nothing more, and its log stopped
    at the kernel threads. The ring had also not been handled right if
    it stopped on the aborted command without running it: it was
    started again on the same command. That command is now made a
    No Op and fails as aborted. "command ring stopped" is logged, and
    the abort line has the root port's PORTSC and CRCR.
  - The second dock boot (with the port reset) hung the same way: port
    14 at SuperSpeed, enabled, link in U0 (PORTSC 0x1203), Address
    Device still not completed. Its log again stopped at the kernel
    threads, with no line from the abort's aftermath.
  - `xhcistall` on the command line queues the first Address Device
    with the wrong cycle bit, so QEMU's controller stops in front of it
    as in front of a hung command. The watchdog found the ring not
    running (CRR 0). An abort then has no effect and sends no event, so
    the controller was given up 5 s later. A ring that is not running
    is now handled as stopped at once: the command is skipped, it fails
    as aborted, and the commands behind it run (the keyboard and mouse
    enumerate).
  - That dock boot reached the prompt and the desktop: only the log
    stopped. Both dock LIVEBOOT.LOGs (and the installed system's BOOT.LOG
    of 2760 bytes) stopped under 4 KB, as if the internal EFI partition
    were full: the first save fits in one cluster, and growing the file
    fails. Failed saves were silent. Now the first failure is logged
    with the error and the volume's free space, and the file keeps the
    newest lines that fit in the space it already has. Checked in QEMU
    with an EFI partition filled to 4 KB free: "saving
    D:\RS2\LIVEBOOT.LOG failed (error 112, 0 KB free)" is in the file.
  - On the Surface (2026-11-17), installed system at c311c67f, booting
    with the dock attached: the boot completes, but the Type Cover still
    does not work. The fixes above are not enough. That boot's
    BOOT.LOG is needed.
  - A live dock boot with c311c67f: the abort line reads `root PORTSC
    0x1203, CRCR 0x8`. The ring is running (CRR 1), unlike in QEMU's
    stall, so the controller was sent Command Abort. Nothing came back
    in the 3 s of boot enumeration left: no "ring stopped", and the
    Type Cover on port 7, queued behind, was never enumerated. With no
    answer, the controller is given up 5 s after the abort, which alone
    explains a dead Type Cover. The log again stops at the kernel
    threads, but with no save failure: the full partition was not the
    reason (that partition's BOOT.LOG is 7.8 KB). Why later lines do
    not reach the file is still open.
  - Now a hung command goes through three steps: (1) its device's root
    port is disabled (USB 3: link state Disabled; USB 2: PED) and left
    alone for the rest of the boot; (2) a second later, Command Abort
    (or the skip, if the ring is not running); (3) if the abort gets no
    answer within 5 s during boot, the controller is reset and
    enumerated again without that port. Boot enumeration waits for the
    steps to finish. After boot, the controller is still given up at
    (3). `xhcistall=hard` makes the stalled command ignore (1) and (2)
    in QEMU: the controller is reset, root port 3 is left alone, and
    the keyboard and mouse enumerate. Plain `xhcistall` still ends at
    (2) with "command aborted".
  - Leaving the port alone means the dock's USB 3 hub, and SuperSpeed
    devices behind it, do not work. Its USB 2 hub is on a USB 2 root
    port of its own, so slower devices behind the dock may.
  - On the Surface (2026-11-17), live stick at 58edf14a with the dock
    attached: **the Type Cover works.** The log is complete and was saved
    to the stick. Port 14's Address Device hung (CRCR 0x8), disabling
    the port did not help, and the abort got no answer (`running:
    true`). The controller was reset with port 14 left alone, and then
    the stick (port 1) and the Type Cover (port 7) enumerated. After
    the reset, `port 2: control transfer failed, completion code 4
    (step DevDesc8)`: a transaction error on the first request, most
    likely from the dock's USB 2 hub, which the mouse behind the dock
    hangs off. A failed device was never tried again. A root-port
    device that fails before it runs is now dropped and found again by
    the next port scan, from a fresh port reset, up to 2 more times
    while it stays connected (not on a port left alone). `xhcifail`
    fails the first device once in QEMU: port 3's stick enumerates on
    the second try.
  - With the retry (096dfd0e) on the Surface: the Type Cover works, the
    mouse behind the dock does not. Port 2 failed `GET_DESCRIPTOR(8)`
    with a transaction error on all 3 tries, then came back later
    (disconnected and connected) and failed 3 more times. rs2 sent that
    first request at once after Address Device. USB 2.0 gives a device
    2 ms after SET_ADDRESS, and Linux waits 10 ms ("some device
    hardware wants it"); QEMU does not care. Now the first request
    waits 10 ms (step Addressed). A root-port failure line also carries
    PORTSC (speed, link state, enabled), for the next log.
  - **Working** (2026-11-17, live stick at c64eb097, dock attached): the
    Type Cover and the mouse behind the dock both work. Port 14 still
    hangs, and the controller is reset without it (about 11 s more at
    boot). Port 2 then enumerates on the first try: the dock's USB 2
    hub 045e:09b3, a second hub 045e:09b4 on 2.1, the mouse 1c59:0027
    on 2.1.3, and HID interfaces without mouse or keyboard reports on
    2.3, 2.5 and 2.1.5. The log was written to the end.
  - Still open: the dock's USB 3 hub (port 14) is left alone, so
    devices behind the dock run at USB 2 speed at most. Why its Address
    Device never completes is not known; the controller reset costs
    about 11 s of boot. The installed system needs this kernel too
    (`rs2-kernel-c64eb097.elf`).
- **CHKDSK progress** (2026-11-17): AUTOCHECK's CHKDSK of an HPFS
  volume said nothing between the volume label and its totals, which on
  the Surface's large C: looked like a boot that stopped. It now prints
  a line per stage and one every 3 seconds.
- **Boot watch** (`bootdiag`): if no PM program has the display 60 s
  after boot (and the boot is not `textmode` or `init=`), the kernel
  logs the Ctrl+Alt+F12 thread report once, now with the kernel threads
  (boot, wlan, network, hot-plug...) as well as the processes, so
  BOOT.LOG shows what the boot waits for.
- **No UART, no wait**: the kernel log checks COM1's scratch register at
  boot and skips the port when nothing answers. A missing port that reads
  00h made every logged byte wait out the "transmitter empty" timeout.
- **`crashtest=panic|trap[,SECONDS]`** crashes on purpose (default after
  5 s), to try the crash screen on a new machine first.
- **`cargo xtask run --bare`**: q35 without the PS/2 controller and HPET,
  USB keyboard and mouse; `--cmdline 'WORDS'` adds kernel options. The
  first run found a real-hardware bug at once: `kbd::init` drained port
  60h while port 64h's "output full" bit was set, and without a keyboard
  controller port 64h reads FFh, so boot hung forever. Booting the dist
  ISO as a USB stick on such a machine:
  `cargo xtask run --bare --boot-hdd --usb-disk work/rs2-live.iso`.

Found on the way, fixed: `boot::start` (CONFIG.SYS, starting the shell)
ran on CPU 0's idle thread. When it blocked (a disk I/O lock the first
bootlog writes held while creating BOOT.LOG), `sched::wake` put the idle
thread on the shared run queue and CPU 1 took it, while CPU 0, with
nothing else to run, also resumed its idle thread (`c.idle()` in
`schedule()`, which does not look at `on_cpu`) from the registers saved
earlier: two CPUs on one stack, a kernel trap in `schedule()` right after
`config.sys:`, every time on fresh images (USB and AHCI). Not a stray
write (the QEMU watchpoint saw only the second CPU's legitimate pushes).
Fixed (2026-11-15): idle threads are never queued (waking one only marks
it ready) and `boot::start` runs on its own kernel thread ("boot"; its
128 KB stack is what shell restarts have always loaded programs on).
Checked with the repro and the bootlog gate removed (USB and AHCI, fresh
images, BOOT.LOG written during `boot::start`). The `klog::BOOTED` gate that
kept bootlog waiting until `boot::start` returned is gone: BOOT.LOG gets
its first copy as soon as the boot volume is up.

Still to do: UDP syslog once a network adapter works (phase 2); saving a
crash report across a warm reboot (a RAM region the loader writes to
`\RS2\CRASH.LOG` on the next boot, with `panic=SECONDS` to restart).

## Phase 1: boot to CMD and PM with an external keyboard

1. Boot the dist ISO written to a USB stick (`dd`; the ISO is already
   hybrid). Secure Boot off. Since 2026-11-15 the ISO also carries a GPT
   (one EFI system partition over the ESP image, a hybrid MBR keeping
   the old 0xEF entry): firmware reads a stick's GPT before El Torito,
   and El Torito's 16-bit sector count (32 MB) cut the 57 MB ESP image
   short, so the Surface stopped finding a boot loader on the stick
   (QEMU's OVMF and VMware's CD boot never minded). macOS `diskutil
   list` on the image shows the GPT; `xtask run --bare --boot-hdd
   --usb-disk`, `--hdd` and `--iso` all boot it.
2. Expected first faults: assumptions that only hold in VMs: the 8042
   probe (`kbd.rs`, `mouse.rs`) must time out cleanly; timers
   (`apic.rs` calibrates against the PIT, which Skylake still has but
   may gate); the ACPI `_PRT` walk in `aml.rs` meeting real Intel
   DSDTs; xHCI BIOS handoff (USBLEGSUP) and Intel's USB 2/3 port
   switching (XUSB2PR/USB3_PSSEN on the Sunrise Point xHCI);
   NVMe on a real controller.
3. Display: 2x integer scaling in `display::present` (PM draws at
   1368x912, every pixel copied 2x2) with touch and pointer coordinates
   scaled the same way. Mark the framebuffer write-combining (PAT),
   already on the roadmap; without it each full-screen update of a 20 MB
   framebuffer is slow. Check which GOP modes the Surface firmware offers:
   if it has a lower mode, use that instead.
4. Type Cover keyboard in boot protocol. If the cover's keyboard interface
   does not accept SET_PROTOCOL(boot), it needs the report-protocol parser
   from phase 3 earlier.

## Phase 2: wired network first

Before WiFi, a USB Ethernet adapter gives a network for testing and for
the syslog log. Done (2026-11-16) for adapters of the communications
class, CDC ECM and CDC NCM (`drivers/usbnet.rs`, with the transfers in
`drivers/xhci.rs`):

- The function is found by its communications interface (subclass 6
  ECM or 0x0d NCM) with its union descriptor naming the data interface,
  whose alternate setting with two bulk endpoints is selected after the
  setup; the MAC address is the Ethernet functional descriptor's
  iMACAddress string (a made-up locally administered one if it is
  missing). NCM: GET_NTB_PARAMETERS, then SET_NTB_INPUT_SIZE (up to
  16 KB receive blocks); frames go out one per NTB16 and every datagram
  of the NTB16s that arrive is taken. Both: SET_ETHERNET_PACKET_FILTER
  (directed, broadcast, all multicast); link state from the
  NETWORK_CONNECTION notifications on the interrupt endpoint (up until
  the first says otherwise).
- Unlike mass storage the transfers are asynchronous: 8 receive buffers
  stay queued on bulk IN, completed ones are copied out by the network
  thread and queued again; frames to send fill one of 16 page-sized
  transmit buffers on bulk OUT, freed by their completion. A transfer of
  whole packets gets a padding byte instead of a zero-length packet (as
  Linux does).
- A device whose first configuration has no driver has its other
  configurations read and the first usable one is set. QEMU's `usb-net`
  lists RNDIS first and CDC ECM second; Realtek RTL8153 (and RTL8152)
  adapters have their vendor mode first and ECM in the second
  configuration, so they should work through ECM as they are.
  ASIX AX88179 and other vendor-protocol-only adapters do not.
- A NIC that appears after the network started (a plugged-in adapter)
  gets the next `ethN` interface and DHCP (or stays down with a static
  configuration); one that is unplugged keeps its interface with the
  link down, and comes back on it when plugged in again (same MAC).

Tested in QEMU (`cargo xtask run --usb-net`, also `--net usb`): found as
ECM, DHCP lease from the user network, NETTEST passes everything (ping,
DNS, TCP to the host, 1 MB each way at about 2 MB/s out and 20 MB/s in
under TCG, the listener reached through `hostfwd`); APITEST passes with
the adapter, a USB keyboard and a tablet attached. Hot-plug through the
monitor (`device_del usbnet`, `device_add usb-net,netdev=n0,bus=xhci.0`)
during a PING: eth0's link goes down, the same adapter plugged in again
brings it back and the pings continue, one with another MAC address
becomes eth1 and gets its own lease. The NCM framing was
checked on the host only: QEMU has no NCM device. Not yet tried with a
real adapter on the Surface.

## Phase 3: touch

### HID report protocol (also needed by the Type Cover touchpad)

`drivers/hid.rs` only knows boot-protocol reports. A HID report-descriptor
parser (items, usage pages, report IDs, logical/physical ranges) is the
base for the Type Cover touchpad (Windows Precision Touchpad: digitizer
page, contact count, per-finger X/Y), for USB touch screens and tablets
in general, and for the IPTS single-touch reports below. Absolute
positions go to `display::mouse_abs`, which already exists for VM
tablets.

### IPTS (the screen)

The touch controller is not on USB or I2C. It is a device of the Intel
Management Engine: the host talks to the ME's IPTS client through the
MEI (HECI) PCI function of the PCH (Linux's `mei_me`, PCI 8086:9D3E is
the "iTouch" MEI function on Sunrise Point-LP), and the ME moves the
touch data into host memory buffers.

The linux-surface project's `ipts` driver is the reference (read it with
the same care as IBM's binaries: understand the protocol, write our own
code). Its control flow:

1. MEI: reset the host interface, enumerate ME clients, connect to the
   IPTS client by its UUID, flow-control credits.
2. IPTS commands over MEI: NOTIFY_DEV_READY, GET_DEVICE_INFO (the SP4 is
   an "EDS v1" device), SET_MODE, SET_MEM_WINDOW (hands the ME the
   physical addresses of 16 data buffers, feedback buffers, doorbell and
   work-queue pages), READY_FOR_DATA, then FEEDBACK after each buffer is
   consumed.
3. On EDS v1 devices (the SP4) the driver starts the ME in **event
   mode**, and the touch controller then sends **ready-made single-touch
   HID reports (report ID 64)**: one finger, absolute X/Y, tip switch.
   No heatmap processing is needed for that.
4. Multitouch and the pen need the raw mode: the controller sends
   capacitive heatmaps, and contacts are computed on the host
   (linux-surface's `iptsd`, which on Windows ran as GPU kernels). That
   is a large job of its own, and OS/2 PM has no use for multitouch.

So the plan is single touch only: MEI driver, IPTS client in event mode,
report 64 to an absolute pointer. Tap = button 1 down/up; long press =
button 2 (OS/2 opens context menus on button 2). The pen is said to work
only in raw mode (single-touch mode disables it); whether its reports
need host processing like the finger heatmaps is to be checked in
`iptsd`.

Known problems: the 2016-2017 out-of-tree drivers loaded vendor
firmware/GPU kernels from Windows (users reported only "v78" working on
the SP4); the current linux-surface driver does not load any, so we
expect no blob either, to be confirmed on the device. The linux-surface
wiki notes phantom touches on some SP4 panels with `iptsd`; that is the
heatmap path, not single touch.

## Phase 4: WiFi

### A generic wireless layer, the Marvell chip as its first driver

WiFi is built as a layer of its own (`kernel/src/net/wlan/`, with the
protocol code host-testable in a `crates/wlan` crate), so that other
chips can be added later without touching the rest. The split follows
what differs between chips and what does not:

```
  WIFI.EXE / PM applet / \MPTN\ETC\WIFI.CFG
                │ RS2API calls: scan, connect, disconnect, status
        net::wlan (one per adapter)
          - connection manager: profiles, choosing a network from scan
            results, reconnect after loss, roaming between BSSs of an ESS
          - supplicant: WPA2-PSK 4-way and group handshakes, PMK cache
            (later WPA3-SAE, 802.1X/EAP)
          - Nic adapter: link up only while associated and keyed
                │ trait WlanDriver
     ┌──────────┴─────────────┬───────────────────────────┐
  full-MAC drivers         soft-MAC drivers (later)
  (firmware associates:    + net::wlan::mac: 802.11 frame building,
   Marvell 88W8897,          auth/assoc state machine, CCMP in
   Broadcom FullMAC,         software, rate control
   Intel's iwlwifi is NOT    (Realtek, MediaTek USB dongles,
   this kind)                Atheros ath9k, Intel iwlwifi)
```

`WlanDriver` is the boundary every chip implements. A full-MAC driver
implements the high-level calls; a soft-MAC driver implements only
`tx_raw`/`rx_raw`, channel setting and key installation, and gets the
rest from `net::wlan::mac`:

```rust
pub trait WlanDriver: Send + Sync {
    fn name(&self) -> String;
    fn caps(&self) -> WlanCaps;           // bands, full/soft MAC (later: ciphers, hw crypto)
    fn mac_address(&self) -> [u8; 6];
    fn start(&self) -> Result<String, WlanError>; // firmware, init; on the wlan thread
    fn scan(&self, req: &ScanRequest) -> Result<Vec<Bss>, WlanError>; // blocks a few seconds
    fn diagnostics(&self) -> Vec<(String, String)>; // for WIFI STATUS
    // next rounds:
    // fn connect(&self, bss: &Bss, ies: &[u8]) -> Result<(), WlanError>; // full MAC
    // fn disconnect(&self, reason: u16);
    // fn set_key(&self, key: &Key) -> Result<(), WlanError>; // pairwise/group, CCMP/TKIP
    // fn send(&self, frame: &[u8]) -> bool; // 802.3 frames (EAPOL included)
    // fn poll_events(&self, f: &mut dyn FnMut(WlanEvent)); // associated, deauth, rx frame
    // soft MAC only: set_channel, tx_raw
}
```

Everything above the trait (network choice, WPA2, configuration, the
user interface, the TCP/IP binding) is shared. A new full-MAC chip is
just its transport and command set, and the first soft-MAC chip adds
the MAC layer once for all later ones. The supplicant and the 802.11
element parsing live in `crates/wlan` and are tested on the host against
the IEEE 802.11 test vectors and `hostapd` captures.

### Status (2026-11-16): round 1, firmware and scan

Works on the Surface Pro 4 (2026-11-16, live stick of commit ae8f0384):
the firmware loads and `WIFI SCAN` lists the nearby networks. What it
consists of:

- `crates/wlan` (host tests: `cargo test -p wlan`): 802.11 elements and
  `Bss` (SSID, channel, RSSI, RSN/WPA security), `WIFI.CFG` profiles,
  and Marvell's host command format (`wlan::mwifiex`: framing,
  PCIE_DESC_DETAILS, GET_HW_SPEC, legacy and extended scan commands and
  their results).
- `kernel/src/net/wlan`: the trait above (its first half), the adapters,
  a `wlan` kernel thread started after CONFIG.SYS is read that starts
  each chip, reads `\MPTN\ETC\WIFI.CFG` and scans once, logging every
  network found.
- `kernel/src/drivers/mwifiex.rs`: the 88W8897 (11AB:2B38). Firmware
  from `C:\RS2\FIRMWARE\PCIE8897.BIN` (xtask fetches
  `mrvl/pcie8897_uapsta.bin` from linux-firmware into `work/firmware`;
  Marvell's licence, which allows binary redistribution, is
  `RS2\FIRMWARE\MARVELL.TXT` from `dist/firmware/`), then
  PCIE_DESC_DETAILS, FUNC_INIT, GET_HW_SPEC, RECONFIGURE_TX_BUFF,
  MAC_CONTROL. Scans with the legacy command, four channels at a time
  (2.4 GHz 1-11 active, 12-13 passive; 5 GHz 36-48 active, the rest
  passive), and falls back to SCAN_EXT (results as events, what Linux
  uses on this chip) if the firmware refuses it or it finds nothing.
  Interrupts are not used yet: the driver polls the host interrupt
  status while it waits for a command.
- RS2API 620-622 (`kernel/src/api/wlan.rs`) and `WIFI.EXE`
  (`userland/netapps/src/bin/wifi.rs`): `WIFI STATUS`, `WIFI SCAN`,
  `WIFI LIST`, `WIFI RELOAD`.

Testing on the Surface: boot the live stick with `bootlog`, wait for the
desktop, then either open a command prompt and run `WIFI STATUS` and
`WIFI SCAN`, or read `BOOT.LOG` afterwards: the lines starting
`88W8897` and `wlan0` show each start-up step (BAR, revision, firmware
status, the firmware blocks sent, each command), the first scan per
channel group, and the networks found.

### Status (2026-11-16): round 2, joining networks (WPA2-PSK)

Written, not yet run on the chip:

- `crates/wlan`: `crypto` (SHA-1, HMAC-SHA1, PBKDF2, the 802.11 PRF,
  AES-128, RFC 3394 key wrap; FIPS/RFC/Annex J vectors) and `wpa`, the
  supplicant: 4-way handshake and group rekeying for WPA2-PSK with CCMP
  pairwise (TKIP or CCMP group), checked against a test authenticator,
  with replay, MIC and RSN-element checks and no key reinstallation.
  `mwifiex` gained ASSOCIATE, DEAUTHENTICATE, KEY_MATERIAL (both
  layouts, chosen by the key API version GET_HW_SPEC reports), the TxPD
  and received data (RxPD, 802.3 + SNAP to Ethernet II).
- The driver: the transmit ring (under its own lock, so the network
  thread never waits behind a scan), received frames, association, keys,
  link events, and MSI waking the wlan thread (it also polls every 5 ms,
  in case MSI does not arrive: `WIFI STATUS` shows `interrupts=`).
- `net::wlan`: a connection manager on the wlan thread. It joins the
  network from `WIFI CONNECT`, else the first network of WIFI.CFG in
  range; open and WPA2-Personal (also WPA2/WPA3 transition networks)
  work, WEP, WPA1, WPA3-only, Enterprise and networks that require
  protected management frames are refused with a reason. It runs the
  supplicant on EAPOL frames, installs the keys (message 4 is flushed
  before the pairwise key goes in, so it leaves unencrypted), and retries
  with growing pauses after failures or a lost link. Each adapter is a
  `net::Nic` (an `eth`N interface) registered once the chip is up; its
  link is up only while associated and keyed, so DHCP runs then.
- RS2API 623/624 and `WIFI CONNECT ssid [passphrase]` (saves the
  network first in WIFI.CFG, then follows the state until connected or
  failed), `WIFI DISCONNECT`, `WIFI FORGET ssid`. The passphrase is kept
  in the system keychain (docs/KEYCHAIN.md), encrypted and out of
  programs' reach, not in WIFI.CFG; a `PASSPHRASE=` line found in
  WIFI.CFG is moved there. On the live stick C: is a RAM disk, so
  WIFI.CFG and the keychain do not survive a reboot there; on an
  installed system they do.
- Not yet: 802.11n/ac (association offers legacy rates only, up to
  54 Mbit/s), power saving (kept off on purpose), hidden networks, roaming
  between access points of one network, a PM applet.

First try on the Surface: association worked, but the AP gave up the
4-way handshake (deauthenticated, reason 15) because no frame ever left.
The 88W8897's transmit read pointer starts at 0 with the rollover bit
set; rs2 compared it the other way, so the first reclaim took back all
32 slots and the ring looked full from then on, and WPA2 message 2 was
refused. Fixed in b5ae0880 (frames in flight counted, at most 31), not
yet tried on the chip. WiFi log lines are also kept in memory since
then: `WIFI STATUS` shows the last 40, and a failed `WIFI CONNECT`
prints the frame counts and what happened.

Second try (b5ae0880, then 4384d6fc with every received packet logged):
still reason 15, and no WPA2 message was logged. The AP's message 1 did
arrive, three or four times per try: 113 bytes (an Ethernet II header
and a 99-byte EAPOL-Key), RxPD type 2, offset 90. The parser took only
RxPD type 0 as data and dropped them. Linux handles every type but
A-MSDU (3), BAR (0x0a) and management (0xe5) as an ordinary frame; rs2
now does too (A-MSDUs are not taken apart yet; they come only with
802.11n, which rs2 does not offer).

**Works** (2026-11-17, live stick at f46ac952): `WIFI CONNECT` joined
a WPA2 network on 5 GHz (channel 136, -63 dBm) at the first try, the
4-way handshake completed (messages 1-4), `eth0` came up and DHCP got a
lease (address, gateway and DNS). Encrypted frames therefore go both
ways (CCMP in the firmware, keys from KEY_MATERIAL v2). 802.11n/ac
and a PM window followed (round 3, below). The keychain and WIFI.CFG on
the live stick last until reboot only.

Testing on the Surface: `WIFI CONNECT "your ssid" passphrase`, then
`IFCONFIG` (or `PING`) once it says connected. `BOOT.LOG` has every step
on lines starting `wlan0` and `88W8897` (key API version, association
result, handshake failures).

### Round 3: 802.11n and 802.11ac (written 2026-11-17, not yet run on the chip)

Following Linux `mwifiex` (`join.c`, `11n.c`, `11ac.c`,
`11n_rxreorder.c`, `11n_aggr.c`):

- GET_HW_SPEC's 802.11n/ac capability words and stream count become
  `wlan::mwifiex::Caps`; the log says what the chip offers ("802.11n
  (2 stream(s), 20/40 MHz) and 802.11ac"). 11N_CFG at start-up lets
  the firmware send at HT rates.
- ASSOCIATE to a network that has them adds HT Capabilities (the
  chip's), the channel with its secondary channel for 40 MHz, the
  network's 20/40 coexistence and extended capabilities, VHT
  Capabilities and Operation (at most 80 MHz unless the chip says
  more), operating mode notification, and the WMM information element
  (802.11n needs QoS). A network that refuses that association is
  joined again at legacy rates. `associated as 802.11ac` (or n, a, g)
  is logged, and `WIFI STATUS` shows `standard=`.
- Block-ack sessions the AP asks for (EVENT_ADDBA) are accepted
  (11N_ADDBA_RSP, window 32). Received frames to us of such a traffic
  class go through a reorder window (`wlan::reorder`): duplicates and
  old frames are dropped, a gap is skipped after 50 ms, a Block Ack
  Request moves the window, DELBA or a timeout ends it.
- A-MSDUs (RxPD type 0xe6) are split into their frames; BARs are 0xe7.
- Host tests use the Surface's own capability words.

Not done: transmit aggregation (A-MPDU/A-MSDU from rs2), power saving,
roaming, hidden networks.

### The WiFi window (WIFIPM.EXE)

`C:\OS2\WIFIPM.EXE` (userland/wifipm) is the Presentation Manager
counterpart of WIFI.EXE, on the same calls (RS2API 620-624, the
keychain's 630 and 633). Its object, **WiFi**, is in the System Setup
folder next to Display (`<RS2_WIFI>`; desktop version 17 adds it to
existing desktops); from a command prompt, `START WIFIPM` or just
`WIFIPM`. One window:

- Connection: the adapter, the state (disconnected, joining, checking
  the passphrase, connected, or failed with the kernel's reason while
  it keeps trying), the network's signal (dBm and a word for it),
  channel, security and the standard the link runs (802.11ac, n, a or
  g), and the IP address of the adapter's interface
  (found by its MAC address; "Waiting for DHCP..." until the lease).
  Read again every 3 seconds; a line under it says what the last action
  did.
- Networks: one line per SSID (its strongest access point), strongest
  first, with signal bars, the security and "Saved", "Joining" or
  "Connected". Hidden networks are left out and counted in the heading.
  The list shows the last scan's results at once and starts a new scan
  (on a second thread: the window stays usable); Scan does it again.
- Connect (or a double click): an open network is joined at once; a
  WPA2 network not saved yet, or saved without a passphrase in the
  keychain, first asks for its passphrase (8-63 characters, typed as
  asterisks). As WIFI CONNECT, the network goes first in WIFI.CFG and
  the kernel keeps the passphrase in the keychain; the status then
  follows the join. WEP, WPA1, WPA3-only and Enterprise networks get a
  "not supported" message.
- Disconnect (as WIFI DISCONNECT); Forget (asks first) removes the
  selected saved network from WIFI.CFG and its passphrase from the
  keychain, and leaves it when connected to it.

Without an adapter (QEMU has none) it says "No WiFi adapter found" and
offers only Close. On the serial port it writes one line, `[wifipm]
started: ...`, with what it found. `WIFIPM /DEMO` shows made-up networks
and a connection without asking the kernel (to try the window in QEMU;
its Connect, Disconnect and Forget only say what they would do).

### The first driver: Marvell 88W8897

Marvell 88W8897 on PCIe (Linux `mwifiex` + `mwifiex_pcie`). Its firmware
does the 802.11 MAC work: scanning, authentication, association and
encryption are host commands to the firmware, and data frames pass as
802.3 frames. So rs2 does not need a full 802.11 stack, unlike a soft-MAC
USB dongle. It does need:

1. **Firmware download.** `pcie8897_uapsta.bin` from linux-firmware
   (Marvell's licence allows redistribution in binary form; check it
   before putting the file on a dist ISO, and keep it out of the repo
   like the IBM files: fetched by xtask into `work/`). Downloaded in
   chunks through the PCIe card's boot registers.
2. **The PCIe transport.** TX/RX/event descriptor rings, the command
   buffer, interrupt status registers, MSI.
3. **Host commands.** GET_HW_SPEC, FUNC_INIT, MAC address, 802_11_SCAN
   (and the scan result parsing), 802_11_ASSOCIATE, KEY_MATERIAL, power
   save off, deauthenticate.
4. **WPA2-PSK on the host** (in the shared supplicant, not in the driver). The firmware encrypts (CCMP) once the keys
   are installed with KEY_MATERIAL, but the 4-way handshake is the host's
   job (Linux runs wpa_supplicant): PBKDF2-SHA1 for the PMK from the
   passphrase, the PRF for the PTK, HMAC-SHA1 MIC on EAPOL-Key frames, AES
   key unwrap for the group key, group rekeying. Roughly 1000-1500 lines
   of Rust including SHA-1/HMAC/AES. WPA3-SAE is not supported by this
   firmware; open networks need none of this.
5. **The Surface quirks Linux carries for this card:** after a firmware
   crash the card only comes back after a D3cold cycle of the PCIe slot
   through ACPI (`QUIRK_FW_RST_D3COLD`), and Linux resets the PCIe bridge
   once after firmware start because the firmware reports a fixed LTR
   value that blocks deep package C-states (`QUIRK_DO_FLR_ON_BRIDGE`).
   The second only costs battery; the first matters if the firmware
   crashes, which it is known to do with power saving on: keep power save
   off.
6. **Plumbing into rs2:** a `net::Nic` that is up only while associated
   (link state), DHCP as today. Configuration: SSID and passphrase from a
   file to start with (`\MPTN\ETC\WIFI.CFG`), later a scan/connect
   command (`WIFI SCAN`, `WIFI CONNECT ssid`) and a PM applet.

Estimate: the largest single item in this plan, comparable to the xHCI
driver (about 2000-3000 lines) plus the WPA2 code. Hard to develop
because QEMU has no 88W8897; every test runs on the Surface. The PCIe
transport and command protocol can be checked against the Linux driver,
and the WPA2 handshake can be tested on the host against
`hostapd`/known test vectors (IEEE 802.11 Annex J) before any hardware
test.

Alternative if the Marvell driver stalls: a USB WiFi dongle in the
Type-A port. Most are soft-MAC (Realtek, MediaTek) and need rs2 to do the
802.11 MAC itself, so this is not less work, only easier to debug.

## Order and size

| Step | Size | Testable in QEMU |
|---|---|---|
| Log to file, big panic screen, q35 without 8042/HPET | small | yes |
| Boot, NVMe, xHCI, keyboard on the Surface | unknown until tried | partly |
| 2x display scaling, PAT write-combining | small | yes |
| HID report protocol (Type Cover touchpad, USB tablets) | medium | yes (`usb-tablet`, `usb-kbd` use report protocol too) |
| USB CDC-ECM/NCM Ethernet (done 2026-11-16, QEMU) | medium | yes (`usb-net`) |
| MEI + IPTS single touch | medium | no |
| 88W8897 WiFi + WPA2 | large | no (WPA2 part on the host) |
| AML interpreter (battery done 2026-11-17 in QEMU; buttons, power/sleep) | large | yes |

## References

- linux-surface: <https://github.com/linux-surface/linux-surface>,
  IPTS driver <https://github.com/linux-surface/intel-precise-touch>,
  `iptsd` <https://github.com/linux-surface/iptsd>
- Linux `drivers/misc/mei` (MEI host interface),
  `drivers/net/wireless/marvell/mwifiex`
- The Surface quirks for the 88W8897:
  <https://lore.kernel.org/all/20211011134238.16551-1-verdre@v0yd.nl/>
- Older SP4 WiFi instability: <https://bugzilla.kernel.org/show_bug.cgi?id=109681>
