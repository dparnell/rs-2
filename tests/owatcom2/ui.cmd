@echo off
rem The PM and Windows tools of Open Watcom 2.0, one at a time, for
rem screenshots every S seconds. Boot program:
rem INIT="OS2\CMD.EXE /C C:\OWT\UI.CMD" tests/owatcom2/mkdisk.sh
rem Programs for the debuggers and the profiler are built with -hw (Watcom
rem debug format): the Current-build's DWARF.D32 faults on every DWARF
rem program (dip/dwarf/c/dfld.c read_Shdr reads sh_name twice) and
rem CODEVIEW.D32 divides by zero on -hc programs.
c:
cd \owt
set S=40
wcl386 -zq -l=os2v2 waitfile.c
wcl386 -zq -l=os2v2 -d2 -hw busy.c
wcl386 -zq -l=os2v2 -d2 -hw hello.c
wipfc -q -i owhelp.ipf
rem 1: the help book compiled by wipfc on rs2
start view owhelp.inf
waitfile NOFILE %S%
rem 2: a shipped book (BOOKSHELF)
start view cguide.inf
waitfile NOFILE %S%
rem 3: the profiler
wsample busy.exe
start wprof busy.smp
waitfile NOFILE %S%
rem 4: the PM debugger
start wdw hello.exe
waitfile NOFILE 60
rem 5: the IDE
start ide
waitfile NOFILE %S%
rem 6: the source browser
wcc386 -zq -db hello.c
start wbrw
waitfile NOFILE %S%
rem 7: the Windows editor and tools on rs2's WIN16
start %WATCOM%\binw\viw.exe vitest.txt
waitfile NOFILE %S%
start %WATCOM%\binw\wimgedit.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wre.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wzoom.exe
waitfile NOFILE %S%
start %WATCOM%\binw\wheapwlk.exe
waitfile NOFILE %S%
setboot /b
