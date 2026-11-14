@echo off
rem The Windows tools in %WATCOM%\BINW on rs2's WIN16, then remote
rem debugging over named pipes (NMPSERV, the debugger's NMP trap file,
rem NMPBIND), one at a time for screenshots. Boot program:
rem INIT="OS2\CMD.EXE /C C:\OWT\UI2.CMD" tests/owatcom/mkdisk.sh
c:
cd \owt
set S=30
wcl386 -zq -l=os2v2 waitfile.c
wcl386 -zq -l=os2v2 -d2 hello.c
start %WATCOM%\binw\wimgedit.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wre.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wzoom.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wspy.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wheapwlk.exe
waitfile NOFILE %S%
start nmpserv
waitfile NOFILE 10
start wdw -tr=nmp hello.exe
waitfile NOFILE 60
setboot /b
