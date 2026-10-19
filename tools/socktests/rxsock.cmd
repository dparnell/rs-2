/* RXSOCK test: name lookup, TCP echo through the host, UDP to self */
call RxFuncAdd 'SockLoadFuncs', 'RXSOCK', 'SockLoadFuncs'
call SockLoadFuncs 'quiet'
say 'SockInit' SockInit()
say 'hostid' SockGetHostId()
rc = SockGetHostByName('www.ibm.com', 'h.!')
say 'byname rc' rc 'addr' h.!addr 'name' h.!name
rc = SockGetHostByAddr('127.0.0.1', 'l.!')
say 'byaddr rc' rc 'name' l.!name
s = SockSocket('AF_INET', 'SOCK_STREAM', 0)
say 'socket' s
srv.!family = 'AF_INET'; srv.!port = 2007; srv.!addr = '10.0.2.2'
say 'connect' SockConnect(s, 'srv.!')
say 'send' SockSend(s, 'hello from rxsock')
say 'recv' SockRecv(s, 'data', 100)
say 'echo:' data
say 'peer' SockGetPeerName(s, 'p.!') p.!addr p.!port
say 'close' SockSoClose(s)
u = SockSocket('AF_INET', 'SOCK_DGRAM', 0)
me.!family = 'AF_INET'; me.!port = 7777; me.!addr = '127.0.0.1'
say 'bind' SockBind(u, 'me.!')
say 'sendto' SockSendTo(u, 'datagram', 'me.!')
say 'recvfrom' SockRecvFrom(u, 'dg', 100, 'from.!') dg from.!port
say 'close' SockSoClose(u)
say 'rxsock done'
