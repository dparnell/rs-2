# rs2 networking

rs2 has its own TCP/IP stack in the kernel, network drivers for the
adapters virtual machines provide, a kernel socket interface, and its
own versions of OS/2's socket DLLs on top, so unmodified OS/2 network
programs (IBM FTP, TELNET, FINGER, RXSOCK, the Toolkit samples) run.

```
  OS/2 programs ── TCPIP32 / SO32DLL / TCP32DLL (+ TNLS32, PMSHAPI)
                          tcpipcore (user mode, per process)
  rs2 programs (NETTEST, PING, IFCONFIG, NETSTAT) ──┐   │
                                 ▼                     ▼
                    RS2API 200-219 gate (api/sockets.rs)
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

NICs with MSI (the 82574L) or MSI-X (virtio-net) interrupt on received
frames and link changes: the handler acknowledges the device and calls
`net::kick()`, which wakes the network thread (`Nic::interrupt_driven`).
PCI INTx cannot be routed (no ACPI _PRT), so the others (8254x), and all
of them with `nomsi`, keep interrupts masked: the timer tick (interrupt
context) calls `rx_pending()` on them, which only peeks at a descriptor
or the used ring under `try_lock`, and wakes the network thread when a
frame is waiting.

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
tick and the NIC interrupt handlers run in interrupt context and only
take driver spin locks with `try_lock`.

**Waiting**: every change a socket waiter could care about (data, state
changes, accept queue, DNS answers) bumps `net::EVENTS` and wakes
`net::WAIT`, one wait queue for all sockets. A blocking call retries its
stack operation whenever the counter moves; this keeps select() trivial.
Per-socket queues are an optimisation for later.

**`net::socket`** is the kernel-internal socket API: per-process handle
tables (handles from 3 up, unique in the system: as on OS/2 any process
may use any socket, the tables only say whose it is and so which
process's end closes it), each handle naming a stack socket plus
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

## User interface: RS2API 200-219

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
| 218 | Rs2SockCancel (so_cancel: blocked calls of other threads end with EINTR) |
| 219 | Rs2NetQuerySockets (every socket of the stack, with its owner: netstat) |
| 220 | Rs2SockTake (addsockettolist: the caller owns the socket) |
| 221 | Rs2SockRelease (removesocketfromlist: the socket outlives its owner) |

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
legacy) with `--smp 4`: all checks pass, about 5-7 MB/s each way under
TCG; apitest and stress still pass with a NIC present.

`PING [-c count] [-s size] [-w ms] host` (`OS2\PING.EXE`) uses a raw ICMP
socket.

## Gaps

* NICs without MSI/MSI-X are polled from the tick; no multicast/IGMP, no IPv6, no
  IP forwarding, one address per interface, no routing table beyond the
  default gateway.
* TCP: no keepalives, urgent data, window scaling, SACK, timestamps,
  Nagle or delayed ACK; out-of-order data is dropped.
* One wait queue for all socket waiters (every event wakes every waiter).
* IFCONFIG and NETSTAT only show; there is no `route`/`arp`, no way to
  change the configuration at run time, no DHCP release at shutdown.

## The OS/2 socket API

OS/2 programs reach TCP/IP through DLLs. rs2 has its own, built from
Rust like PMWIN (`userland/`, i686, packed by `elf2lx`, in `C:\OS2\DLL`):

| DLL | What | Exports |
|-----|------|---------|
| TCPIP32 | TCP/IP 4.1+ API: sockets, resolver, network database, 4.4BSD `sockaddr` (`sin_len`) | all 145 of the Toolkit's `tcpip32.lib` |
| SO32DLL | TCP/IP 4.0 sockets: 4.3BSD `sockaddr`, `select()` in OS/2's form, `bsdselect()` with bit masks, 4-argument `ioctl()` | 32 of 61 (not the `_xxxMSC` variants) |
| TCP32DLL | TCP/IP 4.0 network database and resolver, `h_errno` and `_res` variables | 56 of 101 (not the `MSC`/16-bit variants) |
| TNLS32 | IBM's code page helper for its TCP/IP applications | the 15 ordinals FTP, TELNET, FINGER, TALK, REXEC, RSH use |
| PMSHAPI | the `Prf*` profile calls (TELNET reads its options with them) | 10 |
| TCPIPDLL | the 16-bit API (`_cdecl far`, 16-bit ints, far pointers) | 51 of 111, all 16-bit programs on the media import |
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
  0x100, 4.0's MSG_FULLREAD; MSG_WAITALL loops in the DLL); there is no
  urgent data, so `recv(MSG_OOB)` gives EINVAL. `os2_select()` is the
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
  SIOCGIFMTU (4.0 `ioc(g,n)` and 4.4 `_IOWR` numbers alike); the
  routing table SIOSTATCNTRT/SIOSTATRT; SIOCSSYN accepted. Interfaces
  have OS/2's names: `lan0`, `lan1`, ... for the adapters, `lo`.
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
  calls: arguments in EAX, EDX, ECX, then the stack). rs2 has no
  translation tables, so its init reports "no translation", converters
  copy (ordinal 16 widens bytes to the USHORT characters TELNET's
  terminal emulators take), the DBCS table is empty.
* **PMSHAPI**: profiles are kept in memory for the life of the process
  (no INI files yet).
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
  call.

### Utilities

`IFCONFIG [interface]` shows the interfaces (addresses, flags, MAC,
DHCP state, DNS servers, counters); `NETSTAT` the TCP connections, `-a`
all sockets with owning process and handle, `-r` routes, `-i`
interfaces, `-s` counters; `HOSTNAME`; `HOST name|address` resolves
through TCPIP32.DLL (names and PTR lookups); `PING` as before.

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
* RXSOCK.DLL from REXX: SockInit, SockGetHostId, SockGetHostByName,
  SockGetHostByAddr, a TCP echo through the host, UDP to itself.
* TFTP (16-bit, TCPIPDLL/TNLS16): `tftp 10.0.2.2`, `binary`, `get` of a
  300 KB file and a text file from QEMU's TFTP server (`xtask run
  --tftp DIR`), byte-identical.
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

* TCPIPDLL lacks the 16-bit resolver (`_res_*`, `_dn_*`), `_sendmsg`/
  `_recvmsg`, `_bsdselect` and the `_gethostent`-style iterators;
  `_alarm`/`_xsignal` accept but never signal (TFTP's retransmission
  timer does not fire). Nothing on the media imports the 16-bit
  SOCKETS.DLL, which is not provided.
* No urgent (out-of-band) data, no `sysctl()` (the Toolkit's HPS
  memory-mapped I/O samples need it), no ARP/interface statistics
  ioctls (SIOSTATAT, SIOSTATIF, SIOSTATTCP...), no setting of
  interfaces or routes, `rexec()` is missing.
* TNLS32 does no code page translation (FTP's and TELNET's `-x`
  translation tables, DBCS countries); PMSHAPI does not read or write
  INI files.
* The Toolkit's rpcgen sample server (rgs) fails: it calls RPC32DLL's
  `svcudp_create`/`svctcp_create` (`_Optlink`, socket in EAX) with
  `_System` linkage, so the socket argument is whatever EAX holds (here
  pmap_unset's TRUE: socket 1).
