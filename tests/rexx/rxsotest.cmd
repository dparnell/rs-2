/* RXSOTEST: RXSOCK on the loopback interface, by its results alone: the
   same output from rs2's RXSOCK.DLL and IBM's. */
call RxFuncAdd 'SockLoadFuncs', 'RXSOCK', 'SockLoadFuncs'
call SockLoadFuncs 'quiet'
fails = 0
say 'version' SockVersion()
call check 'init', SockInit(), 0

/* A listener and a connection to it. */
l = SockSocket('AF_INET', 'SOCK_STREAM', 'IPPROTO_TCP')
call check 'socket', l >= 0, 1
call check 'lower case names', SockSoClose(SockSocket('af_inet', 'sock_stream', 0)), 0
srv.!family = 'AF_INET'; srv.!port = 2108; srv.!addr = 'INADDR_ANY'
call check 'reuseaddr', SockSetSockOpt(l, 'SOL_SOCKET', 'SO_REUSEADDR', 1), 0
call check 'bind', SockBind(l, 'srv.!'), 0
call check 'listen', SockListen(l, 5), 0
call check 'sockname', SockGetSockName(l, 'n.!'), 0
say 'name' n.!family n.!port n.!addr
c = SockSocket('AF_INET', 'SOCK_STREAM', 0)
to.!family = 'AF_INET'; to.!port = 2108; to.!addr = '127.0.0.1'
call check 'connect', SockConnect(c, 'to.!'), 0
r.0 = 1; r.1 = l
call check 'select listener', SockSelect('r.', '', '', 5), 1
say 'ready' r.0 (r.1 = l)
a = SockAccept(l, 'from.')
call check 'accept', a >= 0, 1
say 'from' from.family from.addr
call check 'peername', SockGetPeerName(c, 'p.!'), 0
say 'peer' p.!family p.!port p.!addr

/* Data, with the flags and the ioctls. */
call check 'send', SockSend(c, 'hello' || '00'x || 'there'), 11
r.0 = 1; r.1 = a
call check 'select data', SockSelect('r.', '', '', 5), 1
call check 'fionread', SockIoctl(a, 'FIONREAD', 'avail'), 0
say 'available' avail
call check 'peek', SockRecv(a, 'data', 5, 'MSG_PEEK'), 5
say 'peeked' data
call check 'recv', SockRecv(a, 'data', 100), 11
say 'received' c2x(data)
r.0 = 1; r.1 = a
w.0 = 1; w.1 = a
call check 'select timeout', SockSelect('r.', 'w.', '', 0), 1
say 'lists' r.0 w.0
call check 'nonblocking', SockIoctl(a, 'FIONBIO', 1), 0
call check 'nothing to read', SockRecv(a, 'data', 10), -1
say 'errno' errno SockSock_Errno() 'data' length(data)
call check 'blocking', SockIoctl(a, 'FIONBIO', 0), 0
say 'unknown ioctl' SockIoctl(a, 'FIOASYNC', 1)

/* Options. */
call check 'keepalive', SockSetSockOpt(c, 'SOL_SOCKET', 'SO_KEEPALIVE', 1), 0
call check 'get keepalive', SockGetSockOpt(c, 'SOL_SOCKET', 'SO_KEEPALIVE', 'v'), 0
say 'keepalive' (v <> 0)
call check 'linger', SockSetSockOpt(c, 'SOL_SOCKET', 'SO_LINGER', '1 3'), 0
call check 'get linger', SockGetSockOpt(c, 'SOL_SOCKET', 'SO_LINGER', 'v'), 0
say 'linger' word(v, 2)
call check 'get type', SockGetSockOpt(c, 'SOL_SOCKET', 'SO_TYPE', 'v'), 0
say 'type' v
call check 'get error', SockGetSockOpt(c, 'SOL_SOCKET', 'SO_ERROR', 'v'), 0
say 'so_error' v
say 'set type' SockSetSockOpt(c, 'SOL_SOCKET', 'SO_TYPE', 1)
call check 'shutdown', SockShutdown(c, 1), 0
call check 'end of data', SockRecv(a, 'data', 10), 0
call check 'close', SockClose(a), 0
call check 'soclose', SockSoClose(c), 0
call check 'close listener', SockSoClose(l), 0

/* UDP. */
u = SockSocket('AF_INET', 'SOCK_DGRAM', 'IPPROTO_UDP')
me.!family = 2; me.!port = 2109; me.!addr = '127.0.0.1'
call check 'udp bind', SockBind(u, 'me.!'), 0
call check 'sendto', SockSendTo(u, 'datagram', 'me.!'), 8
call check 'recvfrom', SockRecvFrom(u, 'dg', 100, 'f.!'), 8
say 'datagram' dg f.!family f.!port f.!addr
call check 'sendto flags', SockSendTo(u, 'two', 'MSG_DONTROUTE', 'me.!'), 3
call check 'recvfrom flags', SockRecvFrom(u, 'dg', 100, 'MSG_PEEK', 'f.!'), 3
call check 'recvfrom again', SockRecvFrom(u, 'dg', 100, 'f.!'), 3
call check 'udp close', SockSoClose(u), 0

/* Hosts and errors. */
call check 'by addr', SockGetHostByAddr('127.0.0.1', 'h.!'), 1
say 'host' h.!addrtype h.!addr h.!addr.0 h.!addr.1
call check 'by name', SockGetHostByName('no.such.host.invalid', 'h2.!'), 0
say 'bad socket' SockSoClose(9999) errno
say 'refused' SockConnectBad()
call badcall 'SockSocket("AF_INET")'
call badcall 'SockSocket("AF_UNIX", "SOCK_STREAM", 0)'
call badcall 'SockSocket("AF_INET", "SOCK_SEQ", 0)'
call badcall 'SockSoClose("x")'
call badcall 'SockInit(1)'
call badcall 'SockSend(1, "")'
call badcall 'SockGetSockOpt(1, "IPPROTO_TCP", "SO_TYPE", "v")'
say 'host id' (length(SockGetHostId()) >= 7)
call SockPSock_Errno 'rxsotest'
call SockDropFuncs
say 'dropped' RxFuncQuery('SockSocket') RxFuncQuery('SockLoadFuncs')
say 'RXSOTEST:' fails 'failure(s)'
exit

check: procedure expose fails
parse arg what, got, want
if got <> want then do
    say 'FAIL' what': got' got', wanted' want
    fails = fails + 1
end
return

SockConnectBad: procedure
s = SockSocket('AF_INET', 'SOCK_STREAM', 0)
t.!family = 'AF_INET'; t.!port = 2110; t.!addr = '127.0.0.1'
rc = SockConnect(s, 't.!')
e = errno
call SockSoClose s
return rc e

badcall:
signal on syntax name badcall.syntax
interpret 'x =' arg(1)
say 'no error from' arg(1)
return
badcall.syntax:
say arg(1) 'error' rc
return
