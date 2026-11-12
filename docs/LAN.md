# rs2's LAN requester

rs2 reaches files and printers on network shares the way OS/2 Warp 4's
File and Print Client (the IBM LAN Server requester) does: network
drives and printer ports with NET USE, `\\SERVER\SHARE` names in every
file call, NET VIEW, the LAN Server API (NETAPI.DLL, NETAPI32.DLL), and
the Workplace Shell's Network folder. The servers are today's: SMB 2.0.2
and 2.1 with NTLMv2 logons (Windows 10 and later, Samba, macOS), and
SMB1 (NT LM 0.12) for servers that speak nothing newer.

```
 NET.EXE   OS/2 programs ── NETAPI32.DLL (32-bit)   PMWP: Network folder,
 LANTEST        │            NETAPI.DLL (16-bit)          shares, Drives
    └───────────┴──────────── lanreq (userland/lanreq) ─────┘
                                     │ RS2API 600-608 (api/lan.rs)
 Dos* file calls on X:\... and \\SERVER\SHARE\...
                │                    │
           fs: drives, path.rs ── fs::redir: uses, servers, sessions,
                                  RedirFs (one per share), names
                                     │
                       crates/smb: SMB2 / SMB1, NTLMv2, NetBIOS, RPC
                                     │ Transport
                       net::socket (kernel TCP and UDP sockets)
```

## crates/smb

A `no_std` + `alloc` crate with no kernel dependencies, tested on the
host (`cargo test -p smb`: unit tests with the MS-NLMP NTLMv2 vectors
and RFC hash vectors, and six live tests that start
`tools/scripts/smbhost.py` and run a whole session over TCP). Messages go
through a `Transport` (a connected byte stream); time and random numbers
come from an `Env`.

| Module | What |
|--------|------|
| `crypto` | MD4, MD5, SHA-256, HMAC, RC4 |
| `ntlm` | NTLMSSP NEGOTIATE/CHALLENGE/AUTHENTICATE with NTLMv2 and LMv2 responses, key exchange (the exported session key), anonymous logons; SPNEGO NegTokenInit/NegTokenResp |
| `netbios` | RFC 1001/1002 names, the session service's SESSION REQUEST, NBNS name queries and node status, LMHOSTS (`#DOM:` tags) |
| `smb2` | SMB 2.0.2 and 2.1: NEGOTIATE (also the answer to an SMB1 multi-protocol negotiate, with the 2.??? wildcard), SESSION_SETUP, TREE_CONNECT, CREATE, CLOSE, READ, WRITE, FLUSH, QUERY_DIRECTORY (FileFullDirectoryInformation), QUERY_INFO and SET_INFO (basic, end of file, rename, disposition, full EA list, file system size), IOCTL (pipe transceive), ECHO, LOGOFF; HMAC-SHA256 signing when the server requires it |
| `smb1` | NT LM 0.12: extended security (NTLMSSP) or NTLMv2 in the password fields, TREE_CONNECT_ANDX, NT_CREATE_ANDX, READ_ANDX/WRITE_ANDX, TRANS2 FIND_FIRST2/NEXT2, QUERY/SET_PATH/FILE_INFORMATION (EAs as OS/2 FEA lists), QUERY_FS_INFORMATION, DELETE, RENAME, (DELETE_)CREATE_DIRECTORY, TRANSACTION on `\PIPE\LANMAN`; MD5 signing |
| `rpc` | DCE/RPC (NDR) BIND and srvsvc NetrShareEnum level 1 for SMB2 servers; RAP NetShareEnum and NetServerEnum2 for SMB1 servers |
| `connect` | the NetBIOS session request (port 139), the SMB1 negotiate that offers SMB 2.002 and 2.???, then SMB2 or SMB1 and the logon |
| `status` | NTSTATUS and DOS error classes to OS/2 error codes |

Both dialects implement one `Session` trait (open, read, write, list,
stat, set attributes and times, mkdir, rmdir, delete, rename, EAs, free
space, share and server lists). Requests are one at a time, at most
64 KB each (one SMB2 credit); names are converted between OS/2's code
page 850 and Unicode.

## The redirector: `kernel/src/fs/redir.rs`

The kernel's "LAN" file system, the role of IBM's NETWKSTA.200.

* **The requester** is started (`Rs2LanStart`) with this machine's
  NetBIOS name, its workgroup and options (WINS server, LMHOSTS file,
  the SMB ports), normally by NET START REQUESTER, NETAPI's
  NetServiceInstall or the Network folder from IBMLAN.INI. LOGON sets the
  user name, password and domain sessions use (no user: guest/anonymous).
* **Servers**: one SMB session per server behind a sleeping lock
  (`sync::Mutex`): a request and its response go over the kernel's
  socket calls, which sleep on the network's wait queue, so other
  threads run meanwhile. Connecting tries port 445, then the NetBIOS
  session service on 139 (the called name is the server's, or
  `*SMBSERVER` for an address); a connect gives up after 6 s, a request
  after 45 s. A transport failure, or a session the server ended,
  reconnects once and retries; open files reopen their handles on the
  new session.
* **Names** resolve through a dotted address, LMHOSTS
  (`%ETC%\LMHOSTS`), NBNS (a unicast query to the WINS server, else a
  broadcast to port 137) and DNS. Right after boot it waits up to 30 s
  for DHCP.
* **RedirFs** is a share's `FileSystem`: stat, list, open (read/write,
  else read-only), create, delete, mkdir, rmdir, rename, set_info
  (attributes and times), volume (FSALLOCATE from the share's size,
  scaled to 32-bit counts; the share name as label), and EAs (SMB2 full
  EA lists, SMB1 FEA lists; replacing a list removes names no longer in
  it). FILETIMEs become FDATE/FTIME (rs2 keeps local time equal to UTC).
* **UNC paths**: `path::canonical` maps `\\SERVER\SHARE\...` to a
  pseudo-drive from 32 up (one per share named), whose `FullPath`
  displays as the UNC name again; `fs::drive` makes a RedirFs for it. A
  UNC path cannot be a current directory.
* **Uses**: `use_add(local, remote, password, user)` connects a drive
  (mounted as `X:`), LPT1-LPT9 or nothing (a deviceless connection) to a
  share, checking the share's type (a drive needs a disk share, a port a
  print queue); a password or user given applies to that server's
  session. `use_del` refuses a drive with open files unless forced
  (NERR_OpenFiles). The last use of a server ends its session.
* **Printing**: an open of a redirected LPTn (by a program, or by the
  spooler printing to that port) creates a print file on the share;
  closing it ends the job, which the server prints.
* DosQueryFSAttach reports a network drive as FSAT_REMOTEDRV, file
  system "LAN", with `\\SERVER\SHARE` as its data.

### RS2API 600-608 (`kernel/src/api/lan.rs`)

| Ord | Entry |
|-----|-------|
| 600 | Rs2LanStart(computer, workgroup, options) |
| 601 | Rs2LanStop(force) |
| 602 | Rs2LanQuery(buf, cb, pcb): `key=value` lines |
| 603 | Rs2LanLogon(user, password, domain) |
| 604 | Rs2LanUseAdd(local, remote, password, user) |
| 605 | Rs2LanUseDel(name, force) |
| 606 | Rs2LanUseEnum(buf, cb, pcb) |
| 607 | Rs2LanShareEnum(server, buf, cb, pcb) |
| 608 | Rs2LanServerEnum(domain, buf, cb, pcb) |

Lists are text, one entry per line, fields separated by tabs.

## Browsing

NetServerEnum2 (NET VIEW, the Network folder) lists the LMHOSTS
entries, the servers sessions were set up with, the master browser of
the workgroup when NBNS knows one (`WORKGROUP<1D>`, then a node status
for its name), and what the browse lists of SMB1 servers among them add
(RAP NetServerEnum2). Each server's workgroup comes from LMHOSTS
(`#DOM:`), else from its NTLM challenge (the NetBIOS domain name), else
the requester's own. SMB2 servers keep no browse list, and WS-Discovery
and mDNS are not used.

## Configuration: IBMLAN.INI

`\IBMLAN\IBMLAN.INI` on the boot drive (rs2 ships a default):

```
[requester]
  computername = RS2
  domain = WORKGROUP
  othdomains =
  wins =                 rs2: address[:port] of a WINS server (NBNS)
  smbport = 445          rs2: the SMB ports (for test servers)
  nbport = 139
  lmhosts =              rs2: default %ETC%\LMHOSTS
```

NET START REQUESTER switches (`/COMPUTERNAME:x`, `/DOMAIN:x`,
`/OTHDOMAINS:a,b`) take precedence; the computer name defaults to
%HOSTNAME%. NETAPI's NetConfigGet(2)/GetAll(2) read the file and
Net32ConfigSet writes it.

## NET.EXE

rs2's own (`userland/lanreq/src/bin/net.rs`), with IBM's command forms
and message numbers in rs2's own words (`userland/msg/NET.TXT`, built as
`OS2\SYSTEM\NET.MSG`). As IBM's: a NERR_ code prints the NET.MSG message
of that number, an OS/2 error message 3502 and then its OSO001.MSG text;
USE and VIEW start the requester when it is not running.

* `NET USE` lists the connections (Status, Local name, Remote name);
  `NET USE d:` shows one; `NET USE d: \\server\share [password|*]
  [/USER:name]`, `NET USE LPTn: \\server\queue`, `NET USE \\server\share`
  (deviceless); `NET USE d: /DELETE` (asks before closing open files;
  `/YES`, `/NO`).
* `NET VIEW` (servers and remarks), `NET VIEW \\server` (shares, types,
  the local names they are used as), `/DOMAIN[:name]`.
* `NET START [REQUESTER [/switches]]`, `NET STOP REQUESTER` (lists the
  connections it cancels and asks).
* `NET LOGON [user [password|*]] [/DOMAIN:name]`, `NET LOGOFF` (IBM's
  are LOGON.EXE and LOGOFF.EXE of UPM; rs2 has them in NET).
* `NET CONFIG [REQUESTER]`.

## NETAPI.DLL and NETAPI32.DLL

The LAN Server API with the names and ordinals of IBM's DLLs from the
LAN Server 5.0 requester (studied with Ghidra and the netapi/netapi32
symbol files): NETAPI.DLL's 279 exports (16-bit `_pascal far`, through
`thunk16`; `netapi.tsv` holds each one's argument size), NETAPI32.DLL's
145 `Net32*` and mailslot exports (32-bit `_System`). Both are thin
layers over `lanreq::netapi`, which builds the Toolkit's packed
structures (16:16 pointers and USHORT counts for 16-bit callers), with
the records at the start of the buffer, their strings at its end, and
ERROR_MORE_DATA or NERR_BufTooSmall as LAN Server gives them.

| Call | Levels / what |
|------|---------------|
| NetUseAdd, NetUseDel, NetUseEnum, NetUseGetInfo | use_info_0/1 |
| NetShareEnum, NetShareGetInfo | share_info_0/1 of a remote server |
| NetServerEnum2, NetServerGetInfo | server_info_0/1; SV_TYPE_DOMAIN_ENUM lists the workgroups |
| NetWkstaGetInfo | wksta_info_10 (computer, user, workgroup, logon domain, other domains) |
| NetWkstaSetUID2 | logon and logoff (what UPM's LAN logon calls) |
| NetUserValidate2 | accepted (the server checks the password at connection time) |
| NetServiceInstall, NetServiceControl, NetServiceEnum, NetServiceGetInfo | the REQUESTER service (start, interrogate, stop) |
| NetConfigGet, NetConfigGetAll, NetConfigGet2, NetConfigGetAll2, Net32ConfigSet | IBMLAN.INI |
| NetAccessGetUserPerms | full rights on a resource that can be reached (the server checks each operation) |
| NetGetDCName | NERR_DCNotFound (workgroups have no domain controller) |

Everything else (server administration, users and groups on a domain
controller, auditing, messaging and aliases, mailslots, remote IPL,
DASD limits) returns ERROR_NOT_SUPPORTED.

## The Workplace Shell

`userland/pmwp/src/connections.rs`: the Network folder (WPNetwork)
starts the requester and holds a network group (WPNetgrp, real name
`\\GROUP`) per workgroup; a group holds its servers (WPServer,
`\\SERVER`), a server its disk shares and print queues (WPSharedDir,
`\\SERVER\SHARE`: a folder on the UNC name, so opening a disk share shows
its files and subfolders). A share's menu has Assign drive (the next
free letter from Z: down; the drive appears in the Drives folder, which
is refreshed) or, for a print queue, Assign port (LPT3, LPT2 or LPT1:
a printer object on that port prints to the server's queue through
rs2's spooler), and Unassign. The Network page shows the requester's
state.

Print queues show in a server's folder as **network printers**
(WPRPrinter, `userland/pmwp/src/rprinter.rs`): dragged out they become
printers kept on the desktop or in the Printers folder, printing through
a local alias queue whose printer's port is `\\SERVER\QUEUE`
([PRINTING.md](PRINTING.md) "Network printers"); their menu has Assign
port (NET USE LPTn) and Unassign port, Logon, Logoff and Access another
network printer; the Printers folder has a "Network printer" template.
The network objects have IBM's pages (`netpages.rs`): a group's Network
page, a server's Server page, a shared directory's Resource page and a
network printer's Network page (the network, server, resource, its
remark, the status and the assigned drive or port); Logon and Logoff
(a group's: the requester's NET LOGON; a server's or resource's: a
connection to the server's IPC$ with that user and password, which its
other connections use), Access another server / network directory, and a
shared directory's Access control (what the user may do there, tried on
the server). Groups, servers, shares and network printers have rs2's own
icons.

## Testing

```
cargo test -p smb                       # host: unit tests and live sessions with smbhost.py
tools/scripts/lantest.sh [seconds]      # QEMU: LANTEST.EXE and LANTEST.CMD against smbhost.py
tools/scripts/lanwps.sh                 # QEMU desktop: the Network folder down to a share, Drives
tools/scripts/lanprint.sh               # QEMU desktop: a network printer, a job on the server's queue
```

`tools/scripts/smbhost.py` is a standard-library SMB server for the
tests: SMB 2.0.2/2.1 (or `--smb1`), NTLMv2 with `--user name:password`
accounts (`--guest`), signing (`--require-signing`), disk shares on host
directories (EAs and DOS attributes in a hidden sidecar file), print
queues whose jobs land in a directory, srvsvc and RAP share lists, the
NetBIOS session service, and NBNS name queries and node status. It was
checked against impacket's client (all three dialects, signing, 5 MB
transfers byte for byte).

`lantest.sh` starts it with a `DATA` share and an `LPT` queue on
`work/lantest`, runs QEMU with `--init "OS2\CMD.EXE /C
C:\RS2\LANTEST.CMD"` and then compares what the guest left on the share
(`tools/scripts/lantest_check.py`). QEMU's user network reaches the
host's loopback as 10.0.2.2, so the guest uses the server's own ports:
`tools/lantest/IBMLAN.INI` sets `wins = 10.0.2.2:4137`, `smbport = 4445`
and `nbport = 4139`; `tools/lantest/LMHOSTS` names it LMHSRV as well
(SMBHOST itself is found through NBNS). work/modules.txt needs:

```
tools/lantest/IBMLAN.INI -> IBMLAN\IBMLAN.INI
tools/lantest/LMHOSTS -> MPTN\ETC\LMHOSTS
tools/lantest/LANTEST.CMD -> RS2\LANTEST.CMD
```

`RS2\LANTEST.EXE` (56 checks): starting the requester, logon, the server
and share lists, NET USE X: (twice: ERROR_ALREADY_ASSIGNED; a missing
share: ERROR_BAD_NET_NAME), NETAPI32's NetWkstaGetInfo, NetUseEnum
(and ERROR_MORE_DATA with a small buffer), NetUseGetInfo, NetShareEnum,
NetServerEnum2 (servers and domains) and NetConfigGet2, DosQueryFSAttach,
reading a host file by drive and by UNC name, 5 MB read and written and
read back, mkdir, wildcards (`*.TXT`, `?.*`, by UNC name, directories),
rename, delete, rmdir (and of a directory that is not empty),
attributes and dates, extended attributes, DosCopy both ways (one of
them by UNC name), free space, NET USE /D with an open file
(NERR_OpenFiles), printing to a redirected LPT2, deviceless uses.
`LANTEST.CMD` then runs NET START, LOGON, VIEW, VIEW \\server, USE (a
drive and LPT3), DIR, MD, COPY in and out, COMP, XCOPY both ways, REN,
ATTRIB, DEL of a read-only file (refused) and after ATTRIB -R, RD, COPY
to LPT3, USE /D, CONFIG REQUESTER and LOGOFF; the host checks the 5 MB
files byte for byte and the two print jobs.

`lanprint.sh` (2026-11-12): a network printer made with
NETID=WORKGROUP\\SMBHOST\LASER gets its local alias (LASER, with a
printer on `\\SMBHOST\LASER`), a raw job on it arrives in smbhost.py's
LASER spool directory byte for byte (80 bytes, one job), and its
settings show the Network page. (A WPSTEST SHELL run that seemed to
stall after a creation with a server's folder open was its command list
cut short: the boot loader kept only 255 bytes of the kernel command
line, so WPSTEST ran out of commands and waited for PMSHELL. It keeps
1023 bytes now, and `xtask run` refuses a longer `--init`.)

Results (2026-11-04): `cargo test -p smb` 18 passed; LANTEST 56 of 56
and the host checks (7) pass; `lanwps.sh` shows WORKGROUP in the Network
folder, SMBHOST in it, the DATA, PUBLIC and LASER (printer) shares, DATA's
files, and drive Y: in the Drives folder. APITEST and NETTEST (its
loopback, DHCP, ping and DNS checks) still pass with the requester in.

## Gaps

* SMB 3.x (encryption, AES signing) is not offered: servers that
  require it (Windows with "Reject unencrypted access") refuse rs2. Kerberos
  is not used (NTLMv2 only), nor DFS referrals.
* One request at a time per server, 64 KB each; no oplocks or leases (no
  client-side caching), byte-range locks (DosSetFileLocks on network
  files) are not sent to the server, sharing modes are checked locally
  only.
* LAN Manager dialects older than NT LM 0.12 (LANMAN2.1 and before) are
  not spoken.
* Browsing knows only LMHOSTS, the servers used, NBNS master browsers and
  SMB1 browse lists; no WS-Discovery or mDNS.
* NETAPI: the server-side and domain administration calls are not there;
  IBM's NET.EXE (16-bit) needs NETAPI internals (NetIMakeLMFileName,
  NetISort, I_NetPathType) rs2 does not provide.
* Times are passed as UTC without a time zone (rs2 has none).
