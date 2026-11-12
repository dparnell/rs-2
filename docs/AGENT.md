# The guest agent (VMAGENT.EXE)

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
- `\DEV\QEMUGA$`: `org.qemu.guest_agent.0` (not used yet).

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
Workplace Shell starts the agent and it opens its port.
