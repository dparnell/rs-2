@echo off
rem Remote debugging over TCP/IP loopback on one machine: TCPSERV (the debug
rem server) in one session, the text-mode debugger with the TCP trap file
rem in another, through 127.0.0.1. Boot program:
rem INIT="OS2\CMD.EXE /C C:\OWT\TCP.CMD [wdw]" tests/owatcom/mkdisk.sh
c:
cd \owt
set D=%1
if "%D%" == "" set D=wd
wcl386 -zq -l=os2v2 waitfile.c
wcl386 -zq -l=os2v2 -d2 hello.c
start /win tcpserv
waitfile NOFILE 10
start /win %D% -tr=tcp;127.0.0.1 hello.exe
waitfile NOFILE 600
setboot /b
