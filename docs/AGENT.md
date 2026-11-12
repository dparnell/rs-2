# The guest agents (VMAGENT.EXE, QEMUGA.EXE)

rs2 has two: VMAGENT shares the clipboard with the host (below), and
QEMUGA is the QEMU guest agent, through which Proxmox, libvirt and other
QEMU/KVM managers shut the guest down, see its addresses and freeze its
file systems for backups ([The QEMU guest agent](#the-qemu-guest-agent-qemugaexe)).

## Sharing the clipboard (VMAGENT.EXE)

When rs2 runs in a virtual machine, `\OS2\VMAGENT.EXE` shares the
clipboard's text with the host: text copied on the host can be pasted in
OS/2 programs, and text copied in OS/2 can be pasted on the host. There is
nothing to install or set up in the guest; the Workplace Shell starts the
agent every time it starts, and the agent finds the hypervisor's channel by
itself (or ends at once on real hardware). A second copy ends at once too
(a named mutex semaphore, `\SEM32\RS2\VMAGENT`), so a restarted shell does
not start another.

| Host | Channel | What the host needs |
|------|---------|---------------------|
| QEMU (its own window, macOS Cocoa, GTK, VNC) | SPICE agent, virtio-serial | `-device virtio-serial-pci -chardev qemu-vdagent,id=vdagent,clipboard=on -device virtserialport,chardev=vdagent,name=com.redhat.spice.0` (`cargo xtask run --agent`) |
| Proxmox VE | SPICE agent | Display "SPICE" (the clipboard is the SPICE client's: virt-viewer/remote-viewer), or the VNC console's clipboard option (Display → Clipboard: VNC, Proxmox 8.1 and later) |
| libvirt / virt-manager, oVirt | SPICE agent | a SPICE display with its `spicevmc` channel (virt-manager adds it) |
| UTM (macOS) | SPICE agent | QEMU backend with "Clipboard Sharing" on (the default) |
| VirtualBox | VMMDev, HGCM shared clipboard service | the machine's Shared Clipboard set to Bidirectional (Disabled by default): Settings → General → Advanced, Devices → Shared Clipboard while it runs, or `VBoxManage controlvm VM clipboard mode bidirectional` |
| VMware Workstation, Fusion, Player | backdoor: GuestRPC, copy/paste version 4 (else the legacy version 1) | "Enable copy and paste" (Options → Guest Isolation; on by default) |
| VMware ESXi | as Workstation | `isolation.tools.copy.disable` and `isolation.tools.paste.disable` set to FALSE in the VM's advanced settings, then a cold boot; the clipboard is VMRC's (the web console has none) |

The agent tries the channels in that order: the SPICE agent's port, then
VirtualBox's device, then VMware's backdoor (only if GuestRPC answers: QEMU's
`vmport` answers the backdoor's version query too but has no GuestRPC).

Not covered, as these hypervisors have no clipboard channel a guest can
use: Hyper-V (only Windows guests, or the RDP of an enhanced session),
Parallels Desktop (Parallels Tools' protocol is not public), Xen and
XCP-ng (none). Their consoles can still type text into the guest.

**Checked so far**: the SPICE agent channel (QEMU, below). The VirtualBox
and VMware backends are written from their protocols as VirtualBox's
(7.1, 7.2) and open-vm-tools' sources define them, and the VMware protocol
is tested on the host against a simulated host (`cargo test -p
guestagent`), but neither has run on VirtualBox or VMware yet.

## How it works

### The SPICE agent's channel

The SPICE agent protocol (spice-protocol's `vd_agent.h`) runs over a
virtio-serial port named `com.redhat.spice.0`. QEMU's own `qemu-vdagent`
character device speaks it and connects it to QEMU's clipboard, which the
display (Cocoa on macOS, GTK, VNC) shares with the host's; a SPICE server
(Proxmox, libvirt) hands it to the SPICE client instead.

The kernel's virtio-console driver negotiates MULTIPORT and keeps its
control queues: the host adds ports, names them and says when its side is
open. Every virtio-serial controller present is used (Proxmox puts the SPICE
port and the QEMU guest agent's port on two). Named ports are character
devices:

- `\DEV\VIOCON$`: the first console port (port 0 without MULTIPORT);
- `\DEV\VDAGENT$`: `com.redhat.spice.0`;
- `\DEV\QEMUGA$`: `org.qemu.guest_agent.0` (QEMUGA.EXE).

Opening a named port tells the host the guest's side is open (closed, then
open, so that the host's side starts afresh after an agent ended). A
reader that falls behind leaves the data in the device: nothing is dropped.

The agent announces the capabilities `CLIPBOARD` and `CLIPBOARD_BY_DEMAND`
(QEMU shares the clipboard only with the second). Then:

- Host to guest: the host sends `CLIPBOARD_GRAB` with the types it has; the
  agent asks for UTF-8 text (`CLIPBOARD_REQUEST`), gets it (`CLIPBOARD`) and
  puts it on the OS/2 clipboard as `CF_TEXT`.
- Guest to host: when the OS/2 clipboard changes, the agent sends
  `CLIPBOARD_GRAB` (text) or `CLIPBOARD_RELEASE` (no text); when the host
  asks, it answers with the text.

It announces neither the selection nor the grab serial capability, so
messages carry no prefix for them, nor a line-end capability: the agent
converts line ends itself.

### VirtualBox

VirtualBox's VMMDev (PCI 80EE:CAFE) takes requests whose physical address
the guest writes to its I/O port. The kernel's driver
(`kernel/src/drivers/vbox.rs`) reports the guest (interface 1.4, OS/2 Warp
4.5; nothing reaches the host's services before that), and calls the
shared clipboard service through HGCM (Call64 requests with embedded
buffers, below 4 GB). It polls each request's DONE flag and keeps the
device's interrupt off (the PCI command register's INTx disable), as the
host turns the HGCM interrupt on by itself.

A VirtualBox host allows one clipboard client per machine, and frees a
request only when it has written its result back. So the kernel owns the
client for good, and keeps the request that waits for host messages
(MSG_OLD_GET_WAIT) even when the thread waiting for it ends: a restarted
agent carries on with the same client and wait. `\DEV\VBOXGST$` gives the
agent the service's four calls as IOCtls (category 0x80: 1 wait, 2 report
formats, 3 read, 4 write; parameters `{format, rc, a, b}`).

The protocol is the service's legacy one, which VirtualBox's own OS/2
additions use and current hosts still take: the host reports formats when
its clipboard changes and the agent reads the text (UTF-16LE, CR LF, NUL
counted); the agent reports its formats when the OS/2 clipboard has text,
and the host asks for the data when something is pasted on its side.

### VMware

The VMware backdoor is an `in eax, dx` on port 0x5658 with the magic
number in EAX. The kernel makes the call for the agent (`\DEV\VMWARE$`,
IOCtl 0x80/1: the six registers in and out; EAX and the port are always
the backdoor's), and the protocol is in `crates/guestagent` (from
open-vm-tools):

- GuestRPC's message layer (command 0x1E: open, send size and payload,
  receive size and payload and status, close; four bytes per call);
- RPCI, the guest's commands to the host (`tools.set.versiontype`,
  `tools.capability.copypaste_version 4`, `copypaste.transport ...`);
- two TCLO channels (`toolbox` and `toolbox-dnd`) on which the host's
  commands arrive when the guest polls (every 10 to 100 ms): `reset`,
  `ping`, `Capabilities_Register`, `Set_Option copypaste 1`, and
  `copypaste.transport` packets;
- copy/paste version 4: after the version is agreed and copy/paste turned
  on, the guest sends a ping (text capabilities); the host sends its
  clipboard (RECV_CLIPBOARD, a CPClipboard whose first slot is
  NUL-terminated UTF-8; large ones in pieces the guest asks for), and asks
  for the guest's (REQUEST_CLIPBOARD, answered with SEND_CLIPBOARD);
- version 1, if the host does not take version 4 (or nothing is agreed
  within about 30 seconds): the host's selection is read with backdoor
  commands 6 and 7 every second, the guest's given with 8 and 9 when it
  changes (UTF-8, at most 65436 bytes).

### The OS/2 side

The agent is a PM program with an object window and a second thread that
reads the host's channel and posts what arrives to the window, which does
the clipboard work. It follows the clipboard through PMWIN's change count
(`Rs2QueryClipbrdSeq`, PMWIN's private ordinal 9034, read four times a
second), not as the clipboard viewer: OS/2 has one viewer, and the
Clipboard Viewer (CLIPOS2) wants it. Text it puts on the clipboard itself is
not given back to the host.

Text: `CF_TEXT` is the process code page's bytes (DosQueryCp) with CR LF
line ends; hosts exchange UTF-8 (or UTF-16) with LF. Characters the code
page lacks become a close ASCII spelling where there is one (typographic
quotes and apostrophes, dashes, the ellipsis, no-break spaces, the bullet,
™ © ®), else `?`. The conversions and the protocol are in
`crates/guestagent` (host tests: `cargo test -p guestagent`).

Only text is shared for now; bitmaps (`CF_BITMAP`, which the SPICE agent
could carry as PNG or BMP) are not.

## Testing

`cargo test -p guestagent`: text conversions, the SPICE agent's framing,
and VMware's protocol against a simulated VMX (handshake, version 4 both
ways, a 100 KB clipboard in pieces, the version 1 fallback, unknown
commands).

`cargo xtask run --agent` adds the channel, with QEMU's clipboard shared with
the Mac's through its window. `VMAGENT /TEST` (as the boot program:
`--init 'C:\OS2\VMAGENT.EXE /TEST'`) says on the serial log what it does,
puts a test text on the clipboard when the host is there and answers each
text from the host with another.

Headless (`--headless --agent`) QEMU's VNC server, on the socket
`work/qemu-vnc.sock`, stands for the host's clipboard (QEMU's VNC clipboard
is the same as its window's): `tools/scripts/vnc_clipboard.py` offers a
text and prints what the guest offers, through VNC's extended clipboard
(UTF-8).

```
cargo xtask run --headless --agent --init 'C:\OS2\VMAGENT.EXE /TEST' --timeout 120 &
python3 -I tools/scripts/vnc_clipboard.py work/qemu-vnc.sock --set 'über “quotes”'
```

Checked: headless, `Mac to rs2: über naïve “quotes” — ok` arrived as code
page 850 text (`\x81ber na\x8bve "quotes" -- ok`) and the guest's
`rs2 got 37 bytes: café` reached the host as UTF-8; in QEMU's Cocoa window,
`Grüße “hi”` copied on the Mac (pbcopy) arrived in OS/2 and the guest's
`café` could be pasted on the Mac (pbpaste). Booted to the desktop, the
Workplace Shell starts the agent and it opens its port. Without
`--agent`, QEMU's `vmport` answers the VMware backdoor but not GuestRPC, and
the agent ends ("no channel to a host").

## The QEMU guest agent (QEMUGA.EXE)

`\OS2\QEMUGA.EXE` speaks the QEMU guest agent's protocol (JSON commands
and replies, as QEMU's own qemu-ga) on the virtio-serial port
`org.qemu.guest_agent.0` (`\DEV\QEMUGA$`). CONFIG.SYS starts it
(`RUN=C:\OS2\QEMUGA.EXE`, before the desktop, so it also works when
booted to the command prompt); without the port it ends at once.

On the host's side:

- Proxmox VE: Options → QEMU Guest Agent → Use QEMU Guest Agent. The VM's
  Shutdown and Reboot buttons then go through the agent, its Summary shows
  the IP addresses, and backups freeze and thaw the file systems.
- libvirt: a `<channel type='unix'>` with `<target type='virtio'
  name='org.qemu.guest_agent.0'/>` (virt-manager adds it);
  `virsh shutdown --mode agent`, `virsh domifaddr --source agent`,
  `virsh qemu-agent-command`.
- QEMU: `-device virtio-serial-pci -chardev socket,path=qga.sock,server=on,wait=off,id=qga0
  -device virtserialport,chardev=qga0,name=org.qemu.guest_agent.0` (`cargo
  xtask run --agent` adds it beside the SPICE agent's port, on
  `work/qemu-ga.sock`).

Commands (`guest-info` lists them):

| Command | What rs2 does |
|---------|---------------|
| `guest-sync-delimited`, `guest-sync`, `guest-ping`, `guest-info` | the protocol's own: a 0xFF from the host resets the reader; the delimited sync's reply starts with 0xFF |
| `guest-shutdown` (`powerdown`, `halt`, `reboot`) | with the Workplace Shell running, its Shut down (programs are asked to close, objects saved), without asking; otherwise the file systems are flushed and the machine turned off or restarted at once. No reply when it works, as qemu-ga |
| `guest-network-get-interfaces` | the adapters as OS/2 names them (`lan0`.., `lo`): hardware address, IPv4 address and prefix, counters |
| `guest-get-osinfo`, `guest-get-host-name` (%HOSTNAME%), `guest-get-vcpus`, `guest-get-users` (none) | |
| `guest-get-time`, `guest-set-time`, `guest-get-timezone` | the system clock (its time zone offset from DosGetDateTime, UTC when not set) |
| `guest-fsfreeze-freeze`, `-freeze-list`, `-thaw`, `-status` | rs2 cannot hold writes back: a "freeze" writes every file system's cache out (DosResetBuffer) and so does the thaw; the status follows. Enough for a consistent snapshot of what was written before it |
| `guest-fstrim` | nothing to trim |
| `guest-file-open`, `-read`, `-write`, `-seek`, `-flush`, `-close` | files of the guest (modes `r`, `r+`, `w`, `w+`, `a`, `a+`); only handles the agent opened |

Not there: `guest-exec` (running programs), memory blocks, SSH keys, disk
and file system details.

The Workplace Shell's shutdown is reached through PMWP's private
`Rs2ShutdownSystem(restart)` (ordinal 9101), which hands it to the shell's
process when called from another (its object call server, operation 15);
`WinShutdownSystem` does the same now, so REXX's `SysShutDownSystem` from a
command line shuts down through the shell too.

The protocol is in `crates/guestagent` (`qga`, `json`; host tests: sync and
errors, the information commands, shutdown and freeze, files), the system's
side in `userland/vmagent/src/qemuga.rs`. `tools/scripts/qga.py SOCKET
COMMAND [JSON]` asks as Proxmox does (a delimited sync, then the command).

Checked in QEMU (`--agent`): every information command; a file written
through the agent read back; freeze, status, thaw; `guest-shutdown` booted
to the command prompt (flushed, powered off) and with the desktop (through
the Workplace Shell: powered off, and `reboot` restarted); with `--net`,
`lan0` 52:54:00:12:34:56 10.0.2.15/24 from DHCP.
