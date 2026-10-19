@echo off
rem Toolkit TCP/IP samples: servers detached, clients in front.
echo === tcps/tcpc
detach C:\TK\TCPS.EXE 2000 > C:\TCPS.LOG
ping -c 1 127.0.0.1 > NUL
tcpc localhost 2000
echo === selects with tcpc and udpc
detach C:\TK\SELECTS.EXE 2001 > C:\SELECTS.LOG
ping -c 1 127.0.0.1 > NUL
tcpc 127.0.0.1 2001
udpc 127.0.0.1 2001
echo === echos/echoc
detach C:\TK\ECHOS.EXE -p2002 > C:\ECHOS.LOG
ping -c 1 127.0.0.1 > NUL
echoc -p2002 localhost
echo === accept/recv
detach C:\TK\ACCEPT.EXE 2003 1 > C:\ACCEPT.LOG
ping -c 1 127.0.0.1 > NUL
echo 1 | recv 2003
echo === os2ioctl
os2ioctl
echo === server logs
ping -c 2 127.0.0.1 > NUL
type C:\TCPS.LOG
type C:\SELECTS.LOG
type C:\ECHOS.LOG
type C:\ACCEPT.LOG
echo samples done
