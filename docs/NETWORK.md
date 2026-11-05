# rs2 networking

rs2 has its own TCP/IP stack in the kernel, network drivers for the
adapters virtual machines provide, a kernel socket interface, and its
own versions of OS/2's socket DLLs on top, so unmodified OS/2 network
programs (IBM FTP, TELNET, FINGER, RXSOCK, the Toolkit samples) run.

```
  OS/2 programs ── TCPIP32 / SO32DLL / TCP32DLL (+ TNLS32, PMSHAPI)
                          tcpipcore (user mode, per process)
  rs2 programs (NETTEST, PING, IFCONFIG, NETSTAT, ROUTE, ARP) ──┐   │
                                 ▼                     ▼
                    RS2API 200-221, 240 gate (api/sockets.rs)
                                 │
                    net::socket: per-process handles,
                    blocking / non-blocking, select, options
                                 │
          ┌──── net: Stack behind a sleeping lock, network thread ────┐
          │      crates/tcpip: Ethernet ARP IPv4 ICMP UDP TCP DHCP DNS │
          └───────────────┬────────────────────────────┬───────────────┘
                     Nic trait                     loopback
         ┌────────────────┴──┬───────────────┬───────────────┐
  drivers/e1000.rs  drivers/vmxnet3.rs  drivers/pcnet.rs  drivers/virtio/net.rs
```

The LAN requester (SMB file and print sharing: network drives, UNC
names, NET.EXE, NETAPI, the Network folder) is described in
[LAN.md](LAN.md); it uses the kernel socket calls below.

## Drivers

All implement `net::Nic` (`kernel/src/net/mod.rs`): name, MAC address,
link state, MTU, `send(frame)`, `receive(callback)` and `rx_pending()`.

* **Intel e1000/e1000e** (`drivers/e1000.rs`): 82540EM (QEMU's `e1000`,
  VirtualBox's default), 82545EM (VMware's `e1000`, VirtualBox "MT
  Server"), 82543GC, 82544, 82541/82547, 82546 and 82574L (QEMU's
  `e1000e`, the q35 default). Reset, link up (CTRL.SLU), MAC address from
  RAL0/RAH0 or the EEPROM (EERD, both register layouts; on the
  82540-82546 the Microwire interface clocked through EECD when EERD
  does not answer), multicast table
  cleared, legacy descriptors: 128 receive buffers of 2 KB kept owned by
  the hardware, 64 transmit buffers frames are copied into. CRC stripped,
  short frames padded by the hardware.
* **VMware vmxnet3** (`drivers/vmxnet3.rs`, 15ad:07b0; QEMU's
  `vmxnet3`): device and UPT revision 1 selected through BAR 1's
  VRRS/UVRS, MAC address from MACL/MACH (or the GET_PERM_MAC commands),
  the "driver shared" area (magic, MTU 1500, no offloads, unicast and
  broadcast receive filter, VLAN 0 allowed, one interrupt) with a queue
  descriptor table of one transmit and one receive queue, then
  ACTIVATE_DEV. Rings hand descriptors over by generation bits: 64
  transmit descriptors with 2 KB copy buffers and their completion
  ring (plus the 128-byte data ring revision 1 expects), 256 receive
  "head" buffers of 2 KB on ring 1 with ring 2 (body buffers) left
  empty, and the receive completion ring; producer indexes go to BAR 0
  (TXPROD, RXPROD). Events (link change, queue errors) are read from the
  shared area, acknowledged through ECR, and the link state is asked
  for with GET_LINK. The boot log line names the model and PCI address.
* **AMD PCnet** (`drivers/pcnet.rs`, 1022:2000: Am79C970A and the
  later PCnet-PCI/FAST parts; VMware's `vlance`/"Flexible", QEMU's
  `pcnet`): I/O ports in 16-bit mode (the mode IBM's PCNTND.OS2 uses),
  or 32-bit mode when the chip was left in it; reset, software style 2
  (32-bit descriptors and initialisation block) through BCR20, transmit
  padding (CSR4 APAD_XMT), MAC address from the address PROM (else
  CSR12-14), an initialisation block with an empty multicast filter, a
  128-descriptor receive ring and a 64-descriptor transmit ring with
  2 KB buffers handed over by OWN bits, CSR0 TDMD to transmit at once.
  Link state from LED0 (BCR4) once it has been seen lit; until then the
  link counts as up (VMware's vlance never lights it). No MSI: the INTx
  pin is routed through ACPI (below), CSR0 IENA set with the transmit and
  initialisation interrupts masked in CSR3; the handler acknowledges
  CSR0. Without a route it is polled.
* **virtio-net** (`drivers/virtio/net.rs`) on the existing virtio PCI
  transport (modern and legacy): features MAC and STATUS, the 12-byte
  (1.x) or 10-byte (legacy) `virtio_net_hdr` without offloads, header and
  frame as a two-descriptor chain (what legacy devices without
  ANY_LAYOUT need), the receive queue kept full.

NICs with MSI (the 82574L) or MSI-X (virtio-net, vmxnet3) interrupt on received
frames and link changes: the handler acknowledges the device and calls
`net::kick()`, which wakes the network thread (`Nic::interrupt_driven`).
The 8254x and the PCnet have no MSI: their **INTx** pin is routed
through the ACPI namespace (`msi::setup_intx`). `kernel/src/aml.rs`
walks the DSDT and SSDTs (Scope, Device, Name, Method and the data
objects; methods are not run) for the `_PRT` of the PCI root bridges
(`_HID`/`_CID` PNP0A03/PNP0A08, bus `_BBN`) and of bridges below them;
a `_PRT` method (QEMU q35's `If (PICF) Return (PRTA) ...`) is searched
for the packages it returns and the APIC-mode one chosen. Entries name
a GSI directly or a link device whose `_CRS` (else a single-choice
`_PRS`) holds an IRQ or Extended Interrupt descriptor with its trigger
and polarity; a computed `_CRS` (PIIX's LNKA-D) falls back to the
interrupt line the firmware set. Devices behind bridges without a
`_PRT` are swizzled up to one. The GSI gets an IOAPIC input and a
vector, programmed and unmasked when the LAPICs are up (`msi::start`);
level-triggered lines are acknowledged after the handlers ran, and
devices on one line share it. On QEMU q35 the e1000 at 00:02.0 lands on
GSI 22 (level, active high). `nointx` on the kernel command line turns
this off. NICs without a vector (`nomsi` for the MSI ones, `nointx`
for the others) keep interrupts masked: the timer tick (interrupt
context) calls `rx_pending()` on them, which only peeks at a descriptor
or the used ring under `try_lock`, and wakes the network thread when a
frame is waiting. (vmxnet3 without MSI-X/MSI sets IC_DISABLE_ALL and
keeps its interrupt masked.) The network thread also looks at every NIC
each 20 ms, so a NIC whose interrupts never arrive still works.

Virtual machines: QEMU's `e1000`, `e1000-82545em`, `e1000e`, `vmxnet3`
and `virtio-net-pci`; VirtualBox's Intel adapters and virtio-net;
VMware's `e1000` (82545EM), `e1000e` (82574L) and `vmxnet3`
(`ethernet0.virtualDev` in the .vmx), and `vlance` / "Flexible" (AMD
PCnet, the default for the OS/2 guest type, behind a PCI bridge at
02:00.0).

## The stack: `crates/tcpip`

A `no_std` + `alloc` crate with no kernel dependencies, so it is tested
on the host (`cargo test -p tcpip`, 55 tests): packets in and out as
byte buffers, time as a millisecond count passed in, nothing blocks.

| Module | What |
|--------|------|
| `wire` | byte order, RFC 1071 checksum, pseudo-header |
| `eth` | Ethernet II framing |
| `arp` | RFC 826 packets; neighbour cache (10 min), resolution with 3 requests 1 s apart, packets parked meanwhile; entries set by hand (permanent, or published: proxy ARP) |
| `ipv4` | header, fragmentation on output, reassembly on input (range list, 30 s timeout, 16 datagrams) |
| `icmp` | echo request/reply, destination unreachable |
| `udp` | datagrams with checksum |
| `tcp` | segment format (MSS, window scale, SACK-permitted, SACK, timestamps options) and the `Tcb` state machine |
| `dhcp` | client: INIT/SELECTING/REQUESTING/BOUND/RENEWING/REBINDING, back-off, T1/T2, lease expiry, NAK, release and renewal on request |
| `dns` | A queries, answers with compression and CNAME chains |
| `stack` | interfaces (addresses and aliases), the routing table, IP input/output and forwarding, sockets, listeners and accept queues, raw sockets, DHCP, the resolver, counters |

**TCP** (RFC 9293) has the full state machine: active and passive open,
simultaneous open and close, FIN handshake, TIME_WAIT (60 s), RST
generation for closed ports and on abort, RFC 5961 checks (RSTs must hit
RCV.NXT, SYNs in the window get a challenge ACK). Retransmission uses the
RFC 6298 estimator (Karn's rule, or timestamp echoes; back-off, 200 ms -
60 s), going back to SND.UNA but skipping what the peer reported with
SACK blocks (RFC 2018), fast retransmit on three duplicate ACKs; slow
start and congestion avoidance (RFC 5681). Out-of-order segments are kept
(up to the window, 64 segments) and joined to the stream when the gap is
filled; meanwhile duplicate ACKs carry SACK blocks for them, the most
recent first. Window scaling and timestamps (RFC 7323, with PAWS) and
SACK-permitted are offered in every SYN and used when the peer offers
them too; the receive window is the free space of the receive buffer
(64 KB by default, up to 1 MB with SO_RCVBUF: the window scale is chosen
from it), window updates go out as soon as the window opens by a segment;
a zero send window is probed on the persist timer. ACKs are delayed up
to 100 ms unless two full segments are unacknowledged, data came out of
order or filled a gap, or the window is about closed. Small segments
wait while data is in flight (Nagle) unless TCP_NODELAY is set, urgent
data is queued or the peer's window is small. Keepalives (SO_KEEPALIVE):
after 2 hours idle, a probe every 75 s, 8 unanswered ones end the
connection with ETIMEDOUT (all three settable through Rs2NetControl).
Urgent data (BSD semantics): MSG_OOB sends with the urgent pointer just
past the last byte; on input the urgent byte is taken out of the stream
(left in with SO_OOBINLINE), reads stop at the mark, SIOCATMARK tells
when the next read is there, and select's exception set reports pending
urgent data. A SYN gives up after 5 retransmissions (63 s), data after
12. A connection closed with unread data is reset, as BSD does; an
orphaned connection in FIN_WAIT_2 is dropped after 60 s. A new SYN for a
connection in TIME_WAIT (higher sequence number or newer timestamp)
opens a new connection (RFC 6191); TIME_WAIT ignores resets (RFC 1337).

**Routing**: 127/8 and our own addresses (aliases included) go over the
loopback interface (interface 0, MTU 16384), which delivers before the
sending call returns. Otherwise the longest prefix wins among the
networks of the interfaces' addresses and the routing table (network
and host routes with a gateway, or directly on an interface; an
interface's own network beats a route of the same length, then the
lower metric). The default gateway of DHCP or `net=` is a default route
of the table, replaced when that configuration changes; ROUTE ADD
replaces it too. Only interfaces up and with link are used. Limited and
subnet broadcasts go to the Ethernet broadcast address. With IP
forwarding on (sysctl, off by default) datagrams for other hosts go on
with their TTL lowered (ICMP time exceeded, unreachable, fragmentation
needed otherwise). Interfaces can hold several addresses (aliases); an
address, mask, MTU, metric, up/down state, DHCP renewal or release
change at run time (Rs2NetControl). The stack counts IP, ICMP, UDP,
TCP and ARP events for NETSTAT -s and the SIOSTAT ioctls.

**Sockets in the stack**: TCP, UDP and raw (any IP protocol; sends the
protocol payload, receives IP header + payload, as BSD raw sockets do —
PING uses raw ICMP). Ephemeral ports 49152-65535. SO_REUSEADDR lets a
listener bind a port that connections (TIME_WAIT included) still use.
UDP sockets with a peer get ICMP port unreachable as ECONNREFUSED.
Errors are `tcpip::Error`, with `errno()` giving the 4.4BSD numbers.

**Resolver**: names that are dotted quads, `localhost` and the host name
(`rs2`) answer at once; others go to the DNS servers of the interfaces
(from DHCP), 2 s per try, 4 tries rotating over the servers, results and
NXDOMAIN cached by TTL.

## In the kernel: `kernel/src/net`

`net::start` (from `boot::start`, after CONFIG.SYS is read) builds the
`Stack`, adds every registered NIC as `eth0`, `eth1`, ... and configures
it, then starts the **network thread** (a kernel thread): it sleeps
until the tick reports frames or 20 ms pass, takes up to 8 batches of
frames per NIC into the stack, updates link states, runs the stack's
timers, transmits the frames the stack produced and prints its log
messages (link changes, DHCP leases) on the console.

The stack lives behind a sleeping `Mutex`; API calls run it in the
calling thread (`net::with_stack`) and transmit what it produced at
once. Everything is thread context under the big kernel lock; only the
tick and the NIC interrupt handlers run in interrupt context and only
take driver spin locks with `try_lock`.

**Waiting**: the stack reports which sockets changed (data, state,
accept queue, room to send) and whether something global did (DNS
answers, interface configuration). A blocking call, or select(),
subscribes a `net::Waiter` to the stack sockets it waits for and sleeps
until one of them changes (a global change wakes every waiter;
so_cancel wakes the socket's); it then retries its stack operation. A
one-second recheck is the safety net. `net::EVENTS`/`net::WAIT` still
see every change.

**`net::socket`** is the kernel-internal socket API: per-process handle
tables (handles from 3 up, unique in the system: as on OS/2 any process
may use any socket, the tables only say whose it is and so which
process's end closes it), each handle naming a stack socket plus
FIONBIO, SO_RCVTIMEO/SO_SNDTIMEO, SO_LINGER (SO_KEEPALIVE, SO_OOBINLINE
and TCP_NODELAY are the stack socket's; accepted connections inherit
them). Calls: `create`, `bind`, `listen`, `accept`,
`connect` (EINPROGRESS for non-blocking sockets, then select for
writing), `send` (a blocking stream send returns when everything is
queued; MSG_OOB sends urgent data), `recv` (MSG_PEEK, MSG_DONTWAIT,
MSG_OOB), `shutdown`, `close`, `select`
(OS/2's form: one array of read, write and exception sockets, -1 for the
ones not ready, millisecond timeout), `ioctl` (FIONBIO, FIONREAD,
SIOCATMARK),
`set_option`/`get_option` (SO_REUSEADDR, SO_BROADCAST, SO_SNDBUF,
SO_RCVBUF, SO_LINGER, timeouts, SO_ERROR, SO_TYPE, SO_ACCEPTCONN,
TCP_NODELAY, TCP_MAXSEG), `name` (getsockname/getpeername) and
`resolve`. Blocking calls end with EINTR when the thread is killed or
signalled. A process's sockets are closed when it ends. DosShutdown
gives the DHCP leases back (`net::shutdown`).

## User interface: RS2API 200-219

Exposed through the rs2-private gate module RS2API (32-bit `_System`,
return 0 or a BSD errno); the table and structure layouts are in
`kernel/src/api/sockets.rs`, the bindings in `userland/os2/src/net.rs`.
Programs built with Rust's std (`userland-std/`, [BROWSER.md](BROWSER.md))
reach the same calls through rs2libc's BSD socket functions
(`std::net`, `getaddrinfo` over Rs2NetResolve, `poll` over
Rs2SockSelect); WEBTEXT and WEB (rs2's web browsers) use them with rustls
for HTTPS.
Addresses are `sockaddr_in` in the 4.3BSD layout (USHORT family); the
4.4BSD layout (`sin_len`, `sin_family` bytes) of the 4.5 Toolkit headers
is accepted as input.

| Ord | Entry |
|-----|-------|
| 200 | Rs2NetQueryIface |
| 201-205 | Rs2SockCreate, Bind, Listen, Accept, Connect |
| 206-209 | Rs2SockSend, Recv, SendTo, RecvFrom |
| 210-213 | Rs2SockShutdown, Close, Select, Ioctl |
| 214-216 | Rs2SockSetOpt, GetOpt, GetName |
| 217 | Rs2NetResolve |
| 218 | Rs2SockCancel (so_cancel: blocked calls of other threads end with EINTR) |
| 219 | Rs2NetQuerySockets (every socket of the stack, with its owner: netstat) |
| 220 | Rs2SockTake (addsockettolist: the caller owns the socket) |
| 221 | Rs2SockRelease (removesocketfromlist: the socket outlives its owner) |
| 240 | Rs2NetControl (routes, ARP entries, interface settings, counters, stack variables: ROUTE, ARP, IFCONFIG, NETSTAT, TCPCFG2, the DLLs' ioctls and sysctl) |

## Configuration

Interfaces come up at boot with DHCP. `net=` on the kernel command line
(`work/cmdline.txt` in development) or `NETWORK=` in CONFIG.SYS
overrides it (the command line wins):

```
net=dhcp                                   default
net=off                                    NICs stay down (loopback only)
net=10.0.2.15/24,10.0.2.2,10.0.2.3         static: address/prefix, gateway, DNS servers
```

The static form configures the first interface; others stay down.

At run time IFCONFIG changes interfaces (address and mask, aliases,
up/down, MTU, metric, DHCP renewal or release), ROUTE and ARP the
routing and ARP tables, and TCPCFG2's Save applies its Network page to
lan0 at once besides writing CONFIG.SYS; these changes last until the
next start. DHCP leases are renewed at T1/T2 and lost at expiry, and
released at shutdown (DosShutdown).

## Testing

```
cargo test -p tcpip                                    # protocol unit tests
cargo xtask run --net ...                              # e1000 (82540EM) on QEMU user networking
cargo xtask run --net vmxnet3 ...                      # also 82545em, e1000e, virtio
cargo xtask run --net-model DEVICE ...                 # any QEMU NIC device
cargo xtask run --virtio-net [--virtio-legacy] ...     # virtio-net
cargo xtask run --net --hostfwd tcp::8080-:80 ...      # more port forwards (5555 -> 23 is always there)
cargo xtask run --net --guestfwd "tcp:10.0.2.100:79-cmd:nc 127.0.0.1 7979" ...   # guest connections to a host port
```

QEMU's user network: the guest is 10.0.2.15, the gateway (and the host)
10.0.2.2, the DNS forwarder 10.0.2.3 (it asks the host's resolver).

`RS2\NETTEST.EXE` checks: the interface configuration (DHCP), loopback
TCP (non-blocking connect, select, 200 KB echoed) and UDP, the socket
list (Rs2NetQuerySockets), so_cancel of a receive blocked in another
thread, a refused
connection, ping of 127.0.0.1 and the gateway, DNS (`-name`, default
www.ibm.com) and localhost, a TCP connection to the host (10.0.2.2:5556:
send a line, read the answer), 1 MB to the host (port 5557) and 1 MB
from it (5558), and a listener on port 23 the host reaches through
`hostfwd` (host port 5555). The host side is a small script with three
listeners and a client that retries until the guest listens (a `nc -l
5556` and `nc localhost 5555` do for the first and last). Verified with
e1000 (82540EM), e1000e (82574L), e1000-82545em, virtio-net (1.x and
legacy) with `--smp 4`, vmxnet3 (MSI-X, and polled with `nomsi`), and
pcnet (also behind a PCI bridge, and with its registers in 32-bit mode;
IBM FTP's `get`/`put` of 300 KB byte-identical on each model too): all checks pass, about 5-7 MB/s each way under
TCG; apitest and stress still pass with a NIC present.

Further NETTEST sections: `tcp` (urgent data over loopback: MSG_OOB,
select's exception set, SIOCATMARK, the read stopping at the mark,
SO_OOBINLINE; TCP_NODELAY and SO_KEEPALIVE read back), `tables` (a
network and a host route added, listed and deleted, an off-link gateway
refused, a permanent ARP entry, an alias that answers pings, the
counters, a stack variable) and `urgent` (urgent data to the host,
which sees it out of band; urgent data from it, which QEMU's user
network passes on in band, without URG, so that check is skipped then;
keepalives on an idle connection: probes go out, QEMU's stack answers
and the connection lives). The host side of `connect`, `bulk`, `urgent`
and `listen` is `tools/scripts/nettesthost.py`:

```
python3 -I tools/scripts/nettesthost.py --port 5586 --client 5595 &
cargo xtask run --net [MODEL] --init "RS2\NETTEST.EXE -port 5586 -listen 2323" --hostfwd tcp::5595-:2323
```

Out-of-order delivery, SACK, PAWS, window scaling, delayed ACKs, Nagle,
keepalive timeouts, routing, forwarding, aliases, proxy ARP and DHCP
release are covered by the host tests of `crates/tcpip`: QEMU's user
network terminates TCP itself, so segments cannot be reordered on the
way to the guest.

`PING [-c count] [-s size] [-w ms] host` (`OS2\PING.EXE`) uses a raw ICMP
socket.

## Gaps

* No multicast/IGMP (the NICs' multicast filters are empty; NetBIOS
  over TCP/IP's name service uses broadcasts, which work), no IPv6, no
  ICMP redirects or path MTU discovery, no routing protocols (RIP).
* Interrupts of PCI devices other than the NICs still come through MSI
  only; the AML walker runs no methods, so routing tables computed at
  run time (other than QEMU's PICF choice) are not understood.
* TCPCFG2 cannot change the host name or the kernel resolver's name
  servers of a DHCP interface until the next start.
* No BOOTP/DHCP server and no SLIP/PPP dialer; TELNETD's sessions get
  their size once (a client resized later keeps the first size); TALKD
  announces calls on its own screen only (there is no per-user
  terminal to write to); code page translation is single-byte only (no
  DBCS conversion).

## The OS/2 socket API

OS/2 programs reach TCP/IP through DLLs. rs2 has its own, built from
Rust like PMWIN (`userland/`, i686, packed by `elf2lx`, in `C:\OS2\DLL`):

| DLL | What | Exports |
|-----|------|---------|
| TCPIP32 | TCP/IP 4.1+ API: sockets, resolver, network database, 4.4BSD `sockaddr` (`sin_len`) | all 145 of the Toolkit's `tcpip32.lib` |
| SO32DLL | TCP/IP 4.0 sockets: 4.3BSD `sockaddr`, `select()` in OS/2's form, `bsdselect()` with bit masks, 4-argument `ioctl()` | 32 of 61 (not the `_xxxMSC` variants) |
| TCP32DLL | TCP/IP 4.0 network database and resolver, `h_errno` and `_res` variables | 56 of 101 (not the `MSC`/16-bit variants) |
| TNLS32 | IBM's code page helper for its TCP/IP applications | the 15 ordinals FTP, TELNET, FINGER, TALK, REXEC, RSH use; translation tables for `-cp` |
| PMSHAPI | the `Prf*` profile calls (TELNET reads its options with them) | 20 |
| TCPIPDLL | the 16-bit API (`_cdecl far`, 16-bit ints, far pointers) | 82 of 111: all the calls (with the resolver, `_sendmsg`/`_recvmsg`, `_bsdselect`, the `get*ent` iterators, `_rexec`, `_alarm`/`_xsignal`); not the resolver's data and debugging exports (`__res`, `_p_*`, `_fp_query`, `__gethtby*`...) |
| TNLS16 | the 16-bit code page and message catalog helper (replaces IBM's, which needs PMWIN's 16-bit API) | 15 |

The work is in `userland/tcpipcore`, linked into each DLL; every export
goes through a stack-aligning trampoline generated by
`tools/scripts/gen_pm_exports.py` (`LIB.TSV DLLDIR MODULE`, with
`exports.extra` for data exports), since OS/2 compilers keep the stack
only 4-byte aligned. DLL data is per process, so errno tables and
result buffers are too.

* **Sockets** map onto RS2API 201-221. Errors are kept per thread
  (indexed by the TID from the TIB) for `sock_errno()`/`psock_errno()`
  as SOCBASEERR (10000) + the BSD errno. Addresses go out in the
  caller's layout (4.4: `sin_len` 16; 4.0: USHORT family) and come in
  either; a destination of INADDR_ANY means this host (loopback).
  Message flags are translated (4.1's MSG_WAITALL 0x80/MSG_DONTWAIT
  0x100, 4.0's MSG_FULLREAD; MSG_WAITALL loops in the DLL); MSG_OOB
  sends and receives urgent data. `os2_select()` is the
  kernel's select; BSD `select()` is built on it (TCPIP32's fd_set is
  {count, array}, SO32DLL's a bit mask). `readv`/`writev`/`sendmsg`/
  `recvmsg` gather and scatter in the DLL; `socketpair()` connects two
  loopback sockets; `accept_and_recv()` and `send_file()` are composed
  of the basic calls. Raw sockets of protocol 0 (opened just for ioctls)
  are IPPROTO_RAW. Options the stack lacks but programs set in passing
  (IP TTL/TOS/multicast, SO_DEBUG, TCP_MAXSEG) are accepted and ignored.
* **ioctl/os2_ioctl**: FIONBIO, FIONREAD, FIOASYNC, SIOCATMARK; the
  interface queries SIOCGIFCONF, SIOCGIFADDR, SIOCGIFFLAGS,
  SIOCGIFBRDADDR, SIOCGIFNETMASK, SIOCGIFDSTADDR, SIOCGIFMETRIC,
  SIOCGIFMTU (4.0 `ioc(g,n)` and 4.4 `_IOWR` numbers alike) and their
  settings SIOCSIFADDR, SIOCSIFNETMASK, SIOCSIFFLAGS (IFF_UP),
  SIOCSIFMTU, SIOCSIFMETRIC, SIOCAIFADDR, SIOCDIFADDR; routes
  SIOCADDRT/SIOCDELRT (`ortentry`: the mask is a host's, the interface's
  network's or the address class's), SIOFLUSHRT, SIOFLUSHRTIFP; ARP
  SIOCSARP/SIOCGARP/SIOCDARP (`arpreq`, ATF_PERM/ATF_PUBL); statistics
  SIOSTATTCP, SIOSTATUDP, SIOSTATIP, SIOSTATICMP (and their Z forms),
  SIOSTATIF/SIOSTATIF42 (`ifmib`), SIOSTATAT, SIOSTATARP with
  SIOSTATCNTAT, SIOSTATCNTRT/SIOSTATRT; SIOCSSYN accepted. Interfaces
  have OS/2's names: `lan0`, `lan1`, ... for the adapters, `lo`.
  Layouts are the 4.5 Toolkit's; counters the stack does not keep are 0.
* **sysctl()** (TCPIP32): CTL_NET.PF_INET IP forwarding
  (IPCTL_FORWARDING/IPCTL_FORWARD, settable), TCP keepalive probes, send
  and receive buffer sizes (settable), MSS, TTL, window scaling and
  timestamps (on), the TCP/UDP/ICMP statistics structures; CTL_KERN host
  name, host id and the stack version (KERNCTL_INETVER); CTL_OS2
  OS2_MEMMAPIO (15 4 KB blocks for MSG_MAPIO sends; the semaphores in
  the array are posted once a send has copied a block, which is at
  once) and OS2_QUERY_MEMMAPIO.
* **Names**: `gethostbyname` looks at the HOSTS file, the host name and
  then the kernel resolver (Rs2NetResolve, DNS servers from DHCP);
  `gethostbyaddr` at HOSTS, the own addresses, then a PTR query. The
  `res_*` calls (`res_init`, `res_mkquery`, `res_send`, `res_query`,
  `res_search`, `res_querydomain`, `dn_expand`/`dn_comp`/`dn_skipname`)
  are a small resolver in the DLL over a UDP socket, configured from
  `RESOLV2`/`RESOLV` (`nameserver`, `domain`, `search`) or the
  interfaces. Services, protocols, networks: SERVICES, PROTOCOL,
  NETWORKS files, with built-in tables when they are missing. Results
  are in per-thread buffers; `h_errno` per thread (TCPIP32's
  `tcp_h_errno1()`, TCP32DLL's `h_errno` variable too).
* **Files** come from `%ETC%` (`SET ETC=C:\MPTN\ETC` in CONFIG.SYS, the
  boot drive's `\MPTN\ETC` otherwise). rs2 ships HOSTS, SERVICES,
  PROTOCOL, NETWORKS and RPC (`dist/etc`). The host name is `%HOSTNAME%`,
  else `rs2`.
* Socket numbers are system-wide, as on OS/2: a socket belongs to the
  process that made it and closes when that process ends, but any process
  may use it; `addsockettolist` takes it over (Rs2SockTake),
  `removesocketfromlist` lets it outlive its owner (Rs2SockRelease).
  This is how INETD hands an accepted connection to the server it starts
  (`%COMSPEC% /c program socket`).
* **TNLS32** has no import library or headers: the entries are what
  IBM's applications are seen to use in their disassembly (`_Optlink`
  calls: arguments in EAX, EDX, ECX, then the stack). Its init (24, or
  re-init 40; 16-bit 9) takes the application's translation name (FTP's
  and TELNET's `-cp name`): a code page of rs2's NLS tables by number or
  name (`819`, `ISO8859-1`, `latin1`, `IBM-850`, `CP437`...). It then
  builds two 256-byte tables between the process code page and that one
  and reports "tables" (bit 1); an unknown name reports bit 0, no name no
  translation. Query tables (39; 16-bit 51 with 16:16 pointers) returns
  (inbound, outbound): IBM's FTP, studied in Ghidra, runs what it
  receives through the first and what it sends through the second.
  Converters copy (ordinal 16 widens bytes to the USHORT characters
  TELNET's terminal emulators take) and the DBCS table is empty: there
  is no DBCS conversion (the 932/EUC/JIS forms).
* **PMSHAPI**: profiles are real INI files shared by all processes
  (see [PM.md](PM.md#profiles-ini-files)).
* **16-bit programs** (`userland/thunk16`): `entries16!` puts 16-bit
  stubs (`mov ax, N; jmp common`) and a common thunk in a 4 KB-aligned
  piece of the DLL's code object, which elf2lx gives 16:16 code aliases;
  the loader resolves a 16-bit program's far import of the export to
  that alias. The thunk saves the 16-bit registers, jumps to flat code,
  loads the flat selectors and the TIB into FS, moves to a per-thread
  64 KB stack, calls `dispatch(N, flat address of the arguments)` and
  returns the result in DX:AX through the alias of its 16-bit epilogue
  (`retf`; the caller removes the arguments). TCPIPDLL widens 16-bit ints
  and far pointers for `tcpipcore`, narrows `int *` results, and rebuilds
  `hostent`/`servent`/`protoent` with 16:16 pointers in per-thread
  buffers of its data. TNLS16 has no catalogs (`catopen` fails,
  `catgets` returns the built-in text), `xpg_printf`/`xpg_fprintf`/
  `xpg_sprintf` format with 16-bit arguments and XPG `%n$` positions;
  TNLS32 also has the three 16-bit entries (9, 37, 51) these programs
  call. The resolver (`_res_*`, `_dn_*`, `__getlong`...), `_sendmsg`/
  `_recvmsg` (the 16-bit `msghdr` of far pointers and ints),
  `_bsdselect` (bit-mask `fd_set`s, as 32-bit) and the iterators
  (`_gethostent`, `_getnetent`, ...; `netent` rebuilt like `hostent`)
  go to tcpipcore; `_rexec` returns the host's name in a buffer 16-bit
  code can address.
* **`_alarm`/`_xsignal`**: IBM's 16-bit TFTP (seen in its disassembly)
  calls `_xsignal(sig, handler)` and `_alarm(seconds)` before each
  `_recvfrom`; the handler calls `_sock_cancel` and longjmps back to the
  retransmission. TCPIPDLL keeps the deadline and the handler; a timer
  thread (started by the first alarm) cancels the socket call the
  alarm's thread is blocked in (`so_cancel`: EINTR), and that thread,
  on its way out of TCPIPDLL (or into its next call, if it was not in
  one), calls the handler with the signal number on its own 16-bit
  stack (thunk16's `call16`), as a signal would arrive. The handler's
  longjmp abandons the 32-bit frames, which thunk16 allows (it already
  does for WinThrow).

### Utilities

`IFCONFIG [interface]` shows the interfaces (addresses and aliases,
flags, MAC, DHCP state, DNS servers, counters); `IFCONFIG interface
[address] [netmask m] [mtu n] [metric n] [up|down] [alias|-alias]
[delete address] [dhcp] [release]` changes one. `ROUTE [-n] [-v] [-f]
ADD|DELETE [net|host] destination gateway [metric] [-netmask mask]`,
`ROUTE -f`/`FLUSH`, `ROUTE PRINT` (TCP/IP 4.x's syntax: `default`,
class masks, `address/bits` too). `ARP host`, `ARP -a`, `ARP -d host`,
`ARP -s host ether [temp] [pub]`, `ARP -f file`. `NETSTAT` with OS/2's
options: `-a` interface addresses, `-c` ICMP, `-i` IP, `-u` UDP counters
and sockets, `-x` TCP counters (with keepalive and buffer settings),
`-n` interfaces, `-p` ARP table, `-r` routes, `-s` all sockets with
owning process and socket number, `-t` TCP connections (default; `-l`
adds listeners), `-g`, `-m`. IBM's NETSTAT, ROUTE, ARP and IFCONFIG are
MPTN programs, not on the Warp 4.52 media (only their `.sym` files), so
these are rs2's own with their syntax. `HOSTNAME`; `HOST name|address`
resolves through TCPIP32.DLL (names and PTR lookups); `PING` as before.

### rs2's TCP/IP applications

rs2 has its own versions of Warp 4.52's base TCP/IP clients and servers
(`userland/netapps`, in `C:\OS2`), so it needs nothing from IBM's
TCP/IP: Rust programs on the RS2API socket calls, with IBM's names and
command-line forms. Shared code (`src/lib.rs`): a socket type, line
reading, files, the %ETC% directory and SERVICES ports, console input
(`KbdCharIn` for keys and passwords), connects from reserved ports
(`connect_from`: a port whose connection does not complete in 3 seconds,
or is refused, gives way to the next), the RFC 1179, TFTP and talk
protocols (`lpd.rs`, `tftp.rs`, `talk.rs`) and the server side of rexec
and rsh (`rcmd.rs`: the request, the standard error connection, TRUSERS,
the command on pipes).

| Program | What |
|---------|------|
| FTP | `FTP [-n] [-v] [-d] [-i] [-g] [-p] [-cp codepage] [host [port]]`: IBM's and BSD's commands (open, user, close, bye/quit, ascii, binary, type, cd, cdup, lcd, pwd, ls, dir, get/recv, put/send, append, mget, mput, mdelete, delete, mkdir, rmdir, rename, size, system, quote/literal, site, rhelp, status, verbose, debug, hash, prompt, glob, passive/sendport, `!`, help/?). Data connections are active (PORT to a port FTP listens on, as IBM's FTP makes them) or passive (`passive`, `-p`). Logs in at once unless -n, with the host's entry in %ETC%\NETRC or by asking (the password without echo). ASCII transfers give CR LF line ends; Ctrl+Z ends a text file sent; with `-cp` (819, ISO8859-1, IBM-850...) ASCII transfers and listings are translated between that code page and the local one. Commands come from standard input: `FTP -n host < script`. |
| FTPD | `FTPD [-p port]`: each client on a thread. Users and their directories from %ETC%\TRUSERS (`user: name password`, then `rw: dirs` and `r: dirs`; neither means the whole system; `anonymous`/`ftp` with any password when listed, never writing). USER, PASS, QUIT, NOOP, SYST, FEAT, HELP, TYPE, MODE, STRU, ALLO, PORT, PASV, LIST (Unix `ls -l` form), NLST, RETR, STOR, APPE, SIZE, MDTM, DELE, RNFR/RNTO, CWD/CDUP, PWD, MKD, RMD and their X forms. Paths are OS/2 paths, shown with `/`. |
| TELNET | `TELNET [-p port] [-t termtype] [-l logfile] [host [port]]`: the host's output goes through `crates/vt100` (a host-tested VT100/VT220/ANSI emulator: cursor movement and erasing, scrolling regions, insert/delete lines and characters, tab stops, the DEC line-drawing set as CP437 box characters, SGR colours and attributes, UTF-8 to CP437, device attribute and cursor reports) onto the VIO screen with VioWrtCellStr; a keyboard thread sends keys as a VT220's (cursor keys after DECCKM, F1-F12, editing keys). Options: ECHO and SUPPRESS GO AHEAD (local echo while the host does not echo), TERMINAL TYPE (VT100 unless -t) and NAWS (the screen's size). Ctrl+] ends the session; without a host it asks (`open host [port]`). `-cp codepage`: the host sends and takes single bytes of that code page (not UTF-8), translated to and from the local one. |
| TELNETD | `TELNETD [-p port] [-cp codepage] [-n]`: each client gets a command prompt (%COMSPEC%) in a pseudo terminal session (below): full-screen VIO programs (TEDIT, CMD's ANSI colours, anything that draws with VIO or KBD) show on the client as VT100/ANSI, with colours and the cursor; cursor, editing and function keys work; the client's window size (NAWS) sets the session's. Characters go out as UTF-8, or as single bytes of `-cp`'s code page. `exit` ends the connection. `-n` (or a kernel without pseudo terminals): the old line mode, the command processor on pipes. `SET TELNET.PASSWORD=` asks clients for a password, as IBM's TELNETD does. |
| LPD | `LPD [-b [banner]] [-c] [-f] [-s] [-cp codepage] [-p port]` (IBM's options): an RFC 1179 server for rs2's spooler queues. A job sent to a queue name becomes a raw job (PM_Q_RAW) of that PM queue through SplQmOpen/SplQmStartDoc/SplQmWrite/SplQmEndDoc, named after the job (J line) or file, with `user@host` as its comment; formatted text (`f`/`p`, or every file with -f) gets CR LF and -cp translation; -b a banner page (host, user, job, class; or the banner file); -s only reserved client ports; %ETC%\HOSTS.LPD (when it lists hosts) says who may print. Queue state (short and long) lists the queue's jobs (SplEnumJob); remove deletes jobs by number or owner (SplDeleteJob). |
| REXECD, RSHD | `REXECD [-t seconds \| -1] [-p port]`, `RSHD [-t seconds \| -1] [-p port]` (port 512, 514): the command runs under `%COMSPEC% /c` with the connection as its standard input and output and, when the client asks, standard error on a second connection (RSHD's from a reserved port); `USER` is set for it; it is ended after `-t` seconds (180; -1 no limit). REXECD checks the user and password against %ETC%\TRUSERS (FTPD's `user:` lines) or, for users TRUSERS does not name, the USER and PASSWORD environment variables (as IBM's). RSHD takes only reserved client ports and the hosts (and users) of %ETC%\RHOSTS (`host [user]`, `+` for any). |
| TALK, TALKD | `TALK user[@host] [tty]`, `TALKD [-p port]`: the 4.3BSD ntalk protocol (UDP 518). TALK looks up an invitation at the other host's daemon and connects, or leaves one with its own daemon and has the other announce the call (again every 30 s) until the other side answers; a split VIO screen (you above, them below; Backspace, Ctrl+U, Ctrl+W edit, Esc ends). With redirected standard input it sends that and prints what arrives (for scripts). TALKD keeps the invitations (a minute) and shows announcements on its screen with a beep. The local user name is %USER%. |
| LPR, LPQ, LPRM | `LPR [-p printer] [-s server] [-b\|-f\|-o] [-C class] [-J job] [-T title] [-# copies] [-h] [file...]`, `LPQ [-p printer] [-s server] [-l]`, `LPRM [-p printer] [-s server] [-a\|job...]`: RFC 1179 from a reserved port (721-731, a different one each time), the printer and server also as `printer@server` or from LPR_PRINTER and LPR_SERVER, `server:port` for an LPD on another port. |
| FINGER | `FINGER [-l] [user]@host`. |
| TFTP | `TFTP [-b blocksize] host [port]`, then connect, mode, ascii, binary, get, put (`host:file` names), verbose, trace, status, rexmt, timeout, blksize, quit. RFC 1350 with the block size option (RFC 2348), netascii conversion, retransmission. |
| TFTPD | `TFTPD [path] [-l] [-n] [-t seconds] [-b maxblock] [-p port]`: reads and writes files under `path` (names may not leave it), a thread per request, block size negotiation unless -n. |
| REXEC, RSH | `REXEC host [-l user] [-p password] [-n] command` (port 512; the user and password also from NETRC or asked for), `RSH host [-l user] [-n] command` (port 514 from a reserved port; the local user is %USER%). Redirected standard input goes to the command. |
| TCPCFG2 | The TCP/IP Configuration notebook (PM): Network (lan0 on or off, DHCP or a fixed address, subnet mask and router, and the interfaces' state now), Host Names (host name, local domain, three name servers), Hosts (the HOSTS file in an MLE). Save writes `NETWORK=` and `SET HOSTNAME=` in CONFIG.SYS (for the next start; the name servers of a fixed address go into NETWORK=), %ETC%\RESOLV2 (`domain`, `nameserver`) and %ETC%\HOSTS. `TCPCFG2 /QUERY` and `TCPCFG2 NETWORK=... HOSTNAME=... DOMAIN=... NAMESERVERS=a,b` do the same from scripts. The desktop's Programs folder has a TCP/IP folder with it, FTP and Telnet. |

**Pseudo terminals.** IBM's TELNETD hands each connection to a helper
(TELNETDC) that starts the command processor. rs2's gives each client a
*pseudo terminal session* (`kernel/src/pty.rs`, `api/pty.rs`): an ordinary session with
its own `Screen` and keyboard queue, flagged `pty`, that no screen shows.
It never comes to the foreground (`session::select` refuses it), is in
no window list (Ctrl+Esc, Alt+Esc, Rs2VioQuerySessions leave it out)
and is not windowable, so PMVIOWIN never sees it; everything else (VIO,
KBD, the console handles, ANSI, Ctrl+C and Ctrl+Break signals,
DosStartSession from inside it) works as in any session. The server side is what PMVIOWIN
does for a windowed session, without PM:

| RS2API | Call |
|--------|------|
| 18 | `Rs2PtyStart(PSZ pszProgram, PSZ pszArgs, PSZ pszEnv, ULONG ulSize, PULONG psid, PULONG ppid)`: start a program (found as DosStartSession finds it) in a new pseudo terminal session of LOWORD x HIWORD(ulSize) cells (0: 80x25), console handles, the caller's environment unless pszEnv |
| 19 | `Rs2PtyWait(ULONG sid, PULONG pulChanges, ULONG ulTimeout)`: wait until the session's screen changed since *pulChanges (its `changes` count; every change of a pty screen wakes the waiters); ERROR_SMG_SESSION_NOT_FOUND once the session ended |
| 12 | `Rs2VioReadScreen`: its cells, cursor, code page (as for windowed sessions) |
| 14 | `Rs2VioKey`: a keystroke into its keyboard queue |
| 16 | `Rs2VioControl(sid, 3, 0)`: end it (its processes are killed) |

TELNETD reads the screen after each change (20 ms to let a burst
settle) and `crates/vt100`'s `server::Renderer` sends the differences:
it keeps the screen the client shows, turns a screen that moved up into
line feeds at the bottom, rewrites changed runs (short unchanged gaps
included) with cursor positioning and SGR colours (PC attribute:
foreground, background, intensity as bold, blink or bright background),
erases blank tails with EL, and shows or hides the cursor. Control
positions go out as their PC pictures in UTF-8. `server::KeyDecoder`
turns the client's bytes into PC keystrokes (scan codes of the US
layout, Ctrl+letter, Enter, Tab, Backspace, Esc, the VT100/VT220/xterm
sequences of the cursor, editing and F1-F12 keys with xterm modifiers,
UTF-8 back through the code page). Both are host-tested (round trips
through the crate's own emulator).

**Testing.** `RS2\NETCLI.EXE` starts FTPD, TFTPD, TELNETD, LPD, REXECD,
RSHD and TALKD and runs the clients with scripted input (36 checks):
LPR/LPQ/LPRM against rs2's LPD (the job lands in a spooler queue, is
listed, is removed), REXEC and RSH against REXECD/RSHD (output, a wrong
password, standard input to `sort`, USER), `rexec()` through TCPIP32
(output and standard error on the second connection), two TALKs
talking through TALKD; FTP against rs2's FTPD
(login, put and get of 300 KB in active and passive mode, compared byte
for byte; pwd, size, mkdir, rename, ls, dir, delete, rmdir; anonymous
read-only, a wrong password refused) and against the host's FTP server
(active and passive); TFTP against TFTPD (octet with 512- and
1428-byte blocks, netascii, a missing file); LPR, LPQ, LPRM, FINGER,
REXEC and RSH against servers on the host; TCPCFG2's command-line form.
On the host, `tools/scripts/sockhost.py` (FTP) and
`tools/scripts/nethost.py` (LPD, finger, rexec, rsh, a VT100 test
screen; with `--client` it logs in to the guest's FTPD in active mode,
sends a job to the guest's LPD and drives its TELNETD through a small
VT100 emulator of its own: CMD's prompt with ANSI colours, `dir`
scrolling, IBM's TEDIT full screen, cursor keys, `exit`; `--pty` runs
only those, `--shots DIR` saves the emulated screens as PNGs drawn with
the kernel's font) serve and check;
`tools/scripts/netcli.sh` runs the guest with the port forwards: the
guest reaches the host's servers as 10.0.2.100 on their standard ports
(`xtask run --guestfwd tcp:10.0.2.100:515-cmd:nc 127.0.0.1 5515`: QEMU
runs nc for each connection), the host reaches FTPD and TELNETD on 5521
and 5523 (and LPD on 5525). TELNET was checked against nethost.py's VT100 screen (colours,
line-drawing box, scrolling region, terminal type VT100 and size 80x25
negotiated, cursor and function keys sent) and TCPCFG2 with screenshots.

### Testing with IBM's programs

From the TCP/IP 4.x applications on the Warp 4.52 media
(`cid/server/tcpapps/install/base`, added to `work/modules.txt` as
`C:\TCPIP\BIN` and `C:\TCPIP\DLL`; MPTN's own programs and DLLs are not
on the media) and the Toolkit (`samples/tcpiptk`), under QEMU user
networking with a small host-side server (`tools/scripts/sockhost.py`:
FTP on 2121 with PASV and PORT, a line server on 2323, TCP echo on
2007; PORT data connections reach the guest through a `hostfwd_add`
sent to QEMU's monitor). The guest-side scripts are in `tools/socktests`
(add them to `work/modules.txt` with the programs; REXX files need CRLF
line ends):

* FTP: `ftp 10.0.2.2 2121`, login, `dir`, `get` text and 300 KB binary,
  `put` back, identical on the host (e1000 and virtio-net, `--smp 4`).
* TELNET: `telnet 10.0.2.2 2323`, the server's lines shown, typed lines
  sent.
* FINGER: resolves names through DNS (10.0.2.3), connects (port 79 is
  not reachable on the test host).
* rs2's RXSOCK.DLL gives the same output as IBM's in `tests/rexx/RXSOTEST.CMD`.
* IBM's RXSOCK.DLL from REXX: SockInit, SockGetHostId, SockGetHostByName,
  SockGetHostByAddr, a TCP echo through the host, UDP to itself.
* TFTP (16-bit, TCPIPDLL/TNLS16): `tftp 10.0.2.2`, `binary`, `get` of a
  300 KB file and a text file from QEMU's TFTP server (`xtask run
  --tftp DIR`), byte-identical. Lost packets: `tools/scripts/tftplossy.py
  ROOT` (port 6969) drops a DATA block of each read and an ACK of each
  write; `tools/socktests/tftploss.cmd` gets and puts 300 KB through it,
  which finishes only because TFTP's `_alarm` fires and it retransmits.
* INETD (16-bit): with `echo tcp C:\INETECHO.CMD` in `%ETC%\INETD.LST` it
  listens on port 7; a host client (through `--hostfwd tcp::5507-:7`)
  is served by the REXX script INETD starts with the socket number,
  through RXSOCK/TCPIP32.
* SYSLOGD: a datagram sent with RXSOCK to 127.0.0.1:514 lands in
  `%ETC%\SYSLOG.MSG`.
* Toolkit samples: tcps/tcpc, selects (BSD select with TCP and UDP)
  with tcpc and udpc, accept/recv (accept_and_recv), os2ioctl (routing
  table); IBM's PORTMAP and RPCINFO and the RPC samples rawex and
  geneserv/genesend on SO32DLL/TCP32DLL with IBM's RPC32DLL.

## Gaps in the socket API

* TCPIPDLL lacks the resolver's data and debugging exports (`__res`,
  `_p_*`, `_fp_query`, `__gethtby*`, `_hostalias`); `_alarm` reaches a
  thread only when it is in, or next enters, TCPIPDLL (not in a busy
  loop elsewhere). Nothing on the media imports the 16-bit SOCKETS.DLL,
  which is not provided.
* SIOSTATSO (sockets, layout unknown) and the INETCFG sysctl variables
  (inetcfg_ctl) are not there.
* TNLS32 translates single-byte code pages only (no DBCS conversion
  for the 932/EUC/JIS countries).
* The Toolkit's rpcgen sample server (rgs) fails: it calls RPC32DLL's
  `svcudp_create`/`svctcp_create` (`_Optlink`, socket in EAX) with
  `_System` linkage, so the socket argument is whatever EAX holds (here
  pmap_unset's TRUE: socket 1).
