@echo off
rem RPC on SO32DLL/TCP32DLL: portmapper, rpcinfo, Toolkit RPC samples.
detach C:\TCPIP\BIN\PORTMAP.EXE
ping -c 1 127.0.0.1 > NUL
echo === rawex
rawex 1000
echo === geneserv/genesend
detach C:\TK\GENESERV.EXE > C:\GENESERV.LOG
ping -c 1 127.0.0.1 > NUL
genesend localhost 1234
echo === rgs/rgc
detach C:\TK\RGS.EXE > C:\RGS.LOG
ping -c 1 127.0.0.1 > NUL
rgc localhost hello_rpcgen
echo === rpcinfo
rpcinfo -p localhost
echo rpc done
