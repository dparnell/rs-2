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
| Battery | ACPI control-method battery behind the EC | None | Needs an AML interpreter (`_BST`/`_BIF`). Low priority |
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
  `cargo xtask iso --dist -o work/rs2-live.iso --cmdline bootlog`. Nothing
  is written from a crashed kernel, so the file can miss the last
  seconds; the QR code below has them.
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
images, BOOT.LOG written during `boot::start`). The gate (`klog::BOOTED`)
stays but is no longer needed.

Still to do: UDP syslog once a network adapter works (phase 2); saving a
crash report across a warm reboot (a RAM region the loader writes to
`\RS2\CRASH.LOG` on the next boot, with `panic=SECONDS` to restart).

## Phase 1: boot to CMD and PM with an external keyboard

1. Boot the dist ISO written to a USB stick (`dd`; the ISO is already
   hybrid). Secure Boot off.
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
the syslog log. CDC-ECM/NCM (class drivers; Realtek RTL8153 adapters can
switch to ECM) is the least work and also works in QEMU (`-device
usb-net`). This needs bulk transfers in the xHCI driver used by a network
driver, the same path USB mass storage takes.

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
    fn caps(&self) -> WlanCaps;           // bands, ciphers, full/soft MAC, hw crypto
    fn mac_address(&self) -> [u8; 6];
    fn scan(&self, req: &ScanRequest) -> Result<(), WlanError>;  // results arrive as events
    fn connect(&self, bss: &Bss, ies: &[u8]) -> Result<(), WlanError>; // full MAC
    fn disconnect(&self, reason: u16);
    fn set_key(&self, key: &Key) -> Result<(), WlanError>; // pairwise/group, CCMP/TKIP
    fn send(&self, frame: &[u8]) -> bool; // 802.3 frames (EAPOL included)
    fn poll_events(&self, f: &mut dyn FnMut(WlanEvent)); // scan result, associated, deauth, rx frame
    // soft MAC only:
    fn set_channel(&self, _ch: Channel) -> Result<(), WlanError> { Err(WlanError::Unsupported) }
    fn tx_raw(&self, _frame: &[u8]) -> bool { false }
}
```

Everything above the trait (network choice, WPA2, configuration, the
user interface, the TCP/IP binding) is shared. A new full-MAC chip is
just its transport and command set, and the first soft-MAC chip adds
the MAC layer once for all later ones. The supplicant and the 802.11
element parsing live in `crates/wlan` and are tested on the host against
the IEEE 802.11 test vectors and `hostapd` captures.

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
| USB CDC-ECM/NCM Ethernet | medium | yes (`usb-net`) |
| MEI + IPTS single touch | medium | no |
| 88W8897 WiFi + WPA2 | large | no (WPA2 part on the host) |
| AML interpreter (battery, buttons, power/sleep) | large | yes |

## References

- linux-surface: <https://github.com/linux-surface/linux-surface>,
  IPTS driver <https://github.com/linux-surface/intel-precise-touch>,
  `iptsd` <https://github.com/linux-surface/iptsd>
- Linux `drivers/misc/mei` (MEI host interface),
  `drivers/net/wireless/marvell/mwifiex`
- The Surface quirks for the 88W8897:
  <https://lore.kernel.org/all/20211011134238.16551-1-verdre@v0yd.nl/>
- Older SP4 WiFi instability: <https://bugzilla.kernel.org/show_bug.cgi?id=109681>
