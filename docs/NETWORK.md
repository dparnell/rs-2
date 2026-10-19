# rs2 networking

rs2 has its own TCP/IP stack in the kernel, network drivers for the
adapters virtual machines provide, and a kernel socket interface that
rs2 programs use today and OS/2's socket DLLs will sit on.

```
  OS/2 programs ── SO32DLL / TCP32DLL / TCPIP32 (next step, user mode)
  rs2 programs (NETTEST, PING) ──┐            │
                                 ▼            ▼
                    RS2API 200-217 gate (api/sockets.rs)
                                 │
                    net::socket: per-process handles,
                    blocking / non-blocking, select, options
                                 │
          ┌──── net: Stack behind a sleeping lock, network thread ────┐
          │      crates/tcpip: Ethernet ARP IPv4 ICMP UDP TCP DHCP DNS │
          └───────────────┬────────────────────────────┬───────────────┘
                     Nic trait                     loopback
              ┌───────────┴────────────┐
       drivers/e1000.rs       drivers/virtio/net.rs
```

## Drivers

Both implement `net::Nic` (`kernel/src/net/mod.rs`): name, MAC address,
link state, MTU, `send(frame)`, `receive(callback)` and `rx_pending()`.

* **Intel e1000/e1000e** (`drivers/e1000.rs`): 82540EM (QEMU's `e1000`,
  VirtualBox's default), 82545EM (VMware's `e1000`, VirtualBox "MT
  Server"), 82543GC, 82544, 82541/82547, 82546 and 82574L (QEMU's
  `e1000e`, the q35 default). Reset, link up (CTRL.SLU), MAC address from
  RAL0/RAH0 or the EEPROM (EERD, both register layouts), multicast table
  cleared, legacy descriptors: 128 receive buffers of 2 KB kept owned by
  the hardware, 64 transmit buffers frames are copied into. CRC stripped,
  short frames padded by the hardware.
* **virtio-net** (`drivers/virtio/net.rs`) on the existing virtio PCI
  transport (modern and legacy): features MAC and STATUS, the 12-byte
  (1.x) or 10-byte (legacy) `virtio_net_hdr` without offloads, header and
  frame as a two-descriptor chain (what legacy devices without
  ANY_LAYOUT need), the receive queue kept full.

rs2 has no MSI/MSI-X and cannot route PCI INTx yet, so the NICs keep
interrupts masked. The timer tick (interrupt context) calls
`rx_pending()` on every NIC, which only peeks at a descriptor or the
used ring under `try_lock`, and wakes the network thread when a frame is
waiting. With interrupts, the handler would do the same.

## The stack: `crates/tcpip`

A `no_std` + `alloc` crate with no kernel dependencies, so it is tested
on the host (`cargo test -p tcpip`, 38 tests): packets in and out as
byte buffers, time as a millisecond count passed in, nothing blocks.

| Module | What |
|--------|------|
| `wire` | byte order, RFC 1071 checksum, pseudo-header |
| `eth` | Ethernet II framing |
| `arp` | RFC 826 packets; neighbour cache (10 min), resolution with 3 requests 1 s apart, packets parked meanwhile |
| `ipv4` | header, fragmentation on output, reassembly on input (range list, 30 s timeout, 16 datagrams) |
| `icmp` | echo request/reply, destination unreachable |
| `udp` | datagrams with checksum |
| `tcp` | segment format (MSS option) and the `Tcb` state machine |
| `dhcp` | client: INIT/SELECTING/REQUESTING/BOUND/RENEWING/REBINDING, back-off, T1/T2, lease expiry, NAK |
| `dns` | A queries, answers with compression and CNAME chains |
| `stack` | interfaces and routing, IP input/output, sockets, listeners and accept queues, raw sockets, DHCP and the resolver |

**TCP** (RFC 9293) has the full state machine: active and passive open,
simultaneous open and close, FIN handshake, TIME_WAIT (60 s), RST
generation for closed ports and on abort, RFC 5961 checks (RSTs must hit
RCV.NXT, SYNs in the window get a challenge ACK). Retransmission uses the
RFC 6298 estimator (Karn's rule, back-off, 200 ms - 60 s), go-back-N from
SND.UNA, fast retransmit on three duplicate ACKs; slow start and
congestion avoidance (RFC 5681). Out-of-order segments are dropped and
answered with a duplicate ACK. The receive window is the free space of a
64 KB buffer (no window scaling), window updates go out as soon as the
window opens by a segment; a zero send window is probed on the persist
timer. A SYN gives up after 5 retransmissions (63 s), data after 12. A
connection closed with unread data is reset, as BSD does; an orphaned
connection in FIN_WAIT_2 is dropped after 60 s. Not implemented:
keepalives, urgent data, window scaling, timestamps, SACK, Nagle and
delayed ACKs (every segment is ACKed at once).

**Routing**: one address per interface. 127/8 and our own addresses go
over the loopback interface (interface 0, MTU 16384), which delivers
before the sending call returns; on-link destinations directly; the rest
through the first interface with a gateway. Limited and subnet
broadcasts go to the Ethernet broadcast address.

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
tick runs in interrupt context and only takes driver spin locks with
`try_lock`.

**Waiting**: every change a socket waiter could care about (data, state
changes, accept queue, DNS answers) bumps `net::EVENTS` and wakes
`net::WAIT`, one wait queue for all sockets. A blocking call retries its
stack operation whenever the counter moves; this keeps select() trivial.
Per-socket queues are an optimisation for later.

**`net::socket`** is the kernel-internal socket API: per-process handle
tables (handles from 3 up), each handle naming a stack socket plus
FIONBIO, SO_RCVTIMEO/SO_SNDTIMEO, SO_LINGER, SO_KEEPALIVE (accepted, not
acted on), SO_OOBINLINE. Calls: `create`, `bind`, `listen`, `accept`,
`connect` (EINPROGRESS for non-blocking sockets, then select for
writing), `send` (a blocking stream send returns when everything is
queued), `recv` (MSG_PEEK, MSG_DONTWAIT), `shutdown`, `close`, `select`
(OS/2's form: one array of read, write and exception sockets, -1 for the
ones not ready, millisecond timeout), `ioctl` (FIONBIO, FIONREAD),
`set_option`/`get_option` (SO_REUSEADDR, SO_BROADCAST, SO_SNDBUF,
SO_RCVBUF, SO_LINGER, timeouts, SO_ERROR, SO_TYPE, SO_ACCEPTCONN,
TCP_NODELAY), `name` (getsockname/getpeername) and `resolve`. Blocking
calls end with EINTR when the thread is killed or signalled. A process's
sockets are closed when it ends.

## User interface: RS2API 200-217

Exposed through the rs2-private gate module RS2API (32-bit `_System`,
return 0 or a BSD errno); the table and structure layouts are in
`kernel/src/api/sockets.rs`, the bindings in `userland/os2/src/net.rs`.
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

## Testing

```
cargo test -p tcpip                                    # protocol unit tests
cargo xtask run --net ...                              # e1000 (82540EM) on QEMU user networking
cargo xtask run --net --net-model e1000e ...           # 82574L (also e1000-82545em, ...)
cargo xtask run --virtio-net [--virtio-legacy] ...     # virtio-net
cargo xtask run --net --hostfwd tcp::8080-:80 ...      # more port forwards (5555 -> 23 is always there)
```

QEMU's user network: the guest is 10.0.2.15, the gateway (and the host)
10.0.2.2, the DNS forwarder 10.0.2.3 (it asks the host's resolver).

`RS2\NETTEST.EXE` checks: the interface configuration (DHCP), loopback
TCP (non-blocking connect, select, 200 KB echoed) and UDP, a refused
connection, ping of 127.0.0.1 and the gateway, DNS (`-name`, default
www.ibm.com) and localhost, a TCP connection to the host (10.0.2.2:5556:
send a line, read the answer), 1 MB to the host (port 5557) and 1 MB
from it (5558), and a listener on port 23 the host reaches through
`hostfwd` (host port 5555). The host side is a small script with three
listeners and a client that retries until the guest listens (a `nc -l
5556` and `nc localhost 5555` do for the first and last). Verified with
e1000 (82540EM), e1000e (82574L), e1000-82545em, virtio-net (1.x and
legacy) with `--smp 4`: all checks pass, about 5-7 MB/s each way under
TCG; apitest and stress still pass with a NIC present.

`PING [-c count] [-s size] [-w ms] host` (`OS2\PING.EXE`) uses a raw ICMP
socket.

## Gaps

* No interrupts (polling from the tick); no multicast/IGMP, no IPv6, no
  IP forwarding, one address per interface, no routing table beyond the
  default gateway.
* TCP: no keepalives, urgent data, window scaling, SACK, timestamps,
  Nagle or delayed ACK; out-of-order data is dropped.
* One wait queue for all socket waiters (every event wakes every waiter).
* No `ifconfig`/`route`/`netstat`/`arp` utilities yet, no hostname
  configuration, no DHCP release at shutdown.

## Next: the OS/2 socket API

OS/2 programs reach TCP/IP through DLLs: `SO32DLL` (32-bit BSD sockets,
TCP/IP 4.0), `TCP32DLL` (resolver and helpers: gethostbyname, inet_addr,
...), `TCPIP32` (4.1+: both, 4.4BSD structures) and the 16-bit `SOCKETS`
and `TCPIPDLL`. The plan:

1. Build them as rs2 user-mode DLLs (Rust, i686, packed by `elf2lx` like
   PMWIN) with `exports.def` files giving IBM's ordinals; the names and
   ordinals come from the Toolkit import libraries
   (`os2tk45/lib/so32dll.lib`, `tcp32dll.lib`, `tcpip32.lib`) and the
   `.sym` files in `debug/symbols/mptn/dll`.
2. Each export is a thin wrapper over RS2API 200-217: add SOCBASEERR
   (10000) to errno values and keep them per thread for `sock_errno()`
   and `psock_errno()`; `sock_init` checks that the stack runs; `soclose`,
   `so_cancel` (needs a kernel call that interrupts a blocked socket call:
   to add), `os2_select` maps directly, BSD `select()` with fd_sets is
   built on it in the DLL; `ioctl`/`os2_ioctl` map FIONBIO/FIONREAD and
   need SIOCGIFADDR/SIOCGIFCONF/SIOCGIFFLAGS (interface list from
   Rs2NetQueryIface); `getsockopt`/`setsockopt` pass through.
3. Structures: TCPIP32 uses 4.4BSD `sockaddr_in` (`sin_len`), SO32DLL the
   4.3 layout; the DLL converts on output (the kernel already accepts
   both). `hostent`/`servent`/`protoent` are built in DLL memory;
   `gethostbyname`/`gethostbyaddr` use Rs2NetResolve (a PTR query and a
   HOSTS file in `%ETC%` are additions), `getservbyname` reads
   `%ETC%\SERVICES`.
4. The 16-bit DLLs thunk to the 32-bit ones (as rs2's 16-bit VIO thunk
   does) once 32-bit programs work.
5. Socket handles may need to be inheritable and shareable between
   processes (`addsockettolist`, `removesocketfromlist`), and IBM's tools
   expect `ifconfig`/`route` IOCtls; test with the Toolkit's socket
   samples and the original FTP/TELNET/PING from the media.
