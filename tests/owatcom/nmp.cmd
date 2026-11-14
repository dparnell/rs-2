@echo off
rem Remote debugging over named pipes on one machine: NMPSERV (the debug
rem server) in one session, the text-mode debugger with the NMP trap file
rem in another; NMPBIND (CONFIG.SYS RUN=) links them. Boot program:
rem INIT="OS2\CMD.EXE /C C:\OWT\NMP.CMD [wdw]" tests/owatcom/mkdisk.sh
rem (the argument: the debugger, wd by default; wdw is the PM one)
c:
cd \owt
set D=%1
if "%D%" == "" set D=wd
wcl386 -zq -l=os2v2 waitfile.c
wcl386 -zq -l=os2v2 -d2 hello.c
start /win nmpserv
waitfile NOFILE 10
start /win %D% -tr=nmp hello.exe
waitfile NOFILE 600
setboot /b
