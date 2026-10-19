/* SYSLOGD test: a UDP datagram to the local syslog daemon */
'@detach C:\TCPIP\BIN\SYSLOGD.EXE'
'@ping -c 2 127.0.0.1 > NUL'
call RxFuncAdd 'SockLoadFuncs', 'RXSOCK', 'SockLoadFuncs'
call SockLoadFuncs 'quiet'
u = SockSocket('AF_INET', 'SOCK_DGRAM', 0)
to.!family = 'AF_INET'; to.!port = 514; to.!addr = '127.0.0.1'
say 'sendto' SockSendTo(u, '<13>rs2: hello syslog', 'to.!')
call SockSoClose u
'@ping -c 2 127.0.0.1 > NUL'
'@type C:\MPTN\ETC\SYSLOG.MSG'
say 'syslog done'
