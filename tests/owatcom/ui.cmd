@echo off
rem The PM and Windows tools, one at a time, for screenshots (the xtask
rem run takes them: {shot ...} every UISTEP seconds). Boot program:
rem INIT="OS2\CMD.EXE /C C:\OWT\UI.CMD" tests/owatcom/mkdisk.sh
c:
cd \owt
set S=40
wcl386 -zq -l=os2v2 waitfile.c
rem 1: the help book compiled by wipfc on rs2
wipfc -q -i owhelp.ipf
start view owhelp.inf
waitfile NOFILE %S%
rem 2: a shipped book from the Tools Help folder (BOOKSHELF)
start view cguide.inf
waitfile NOFILE %S%
rem 3: the profiler on the samples wsample took
wcl386 -zq -l=os2v2 -d2 busy.c
wsample busy.exe
start wprof busy.smp
waitfile NOFILE %S%
rem 4: the PM debugger
wcl386 -zq -l=os2v2 -d2 hello.c
start wdw hello.exe
waitfile NOFILE %S%
rem 5: the text-mode editor in a window
start vi vitest.txt
waitfile NOFILE %S%
rem 6: the Windows editor (Win16) on rs2's WIN16
start %WATCOM%\binw\viw.exe vitest.txt
waitfile NOFILE %S%
setboot /b
