@echo off
rem Builds and runs the Open Watcom FORTRAN 77 test programs on the
rem test disk (D:); everything goes to D:\F77.LOG.
d:
cd \
set LOG=d:\f77.log
echo == FORTRAN 77 tests > %LOG%
echo == wfl386 fmtio >> %LOG%
wfl386 -q -l=os2v2 fmtio.for >> %LOG% 2>&1
fmtio >> %LOG% 2>&1
echo == wfl386 fileio >> %LOG%
wfl386 -q -l=os2v2 fileio.for >> %LOG% 2>&1
fileio >> %LOG% 2>&1
echo == wfl386 fmath >> %LOG%
wfl386 -q -l=os2v2 fmath.for >> %LOG% 2>&1
fmath >> %LOG% 2>&1
echo == wfl386 fchars >> %LOG%
wfl386 -q -l=os2v2 fchars.for >> %LOG% 2>&1
fchars >> %LOG% 2>&1
echo == wfl -l=os2 (16-bit OS/2) >> %LOG%
wfl -q -l=os2 -fe=fmath16.exe fmath.for >> %LOG% 2>&1
fmath16 >> %LOG% 2>&1
wfl -q -l=os2 -fe=fchar16.exe fchars.for >> %LOG% 2>&1
fchar16 >> %LOG% 2>&1
echo == wfc386 + wlink >> %LOG%
wfc386 -q -d2 fileio.for >> %LOG% 2>&1
wlink op q d all system os2v2 file fileio name filelnk >> %LOG% 2>&1
filelnk >> %LOG% 2>&1
echo == wfc + wlink (16-bit) >> %LOG%
wfc -q fmtio.for >> %LOG% 2>&1
wlink op q system os2 file fmtio name fmtio16 >> %LOG% 2>&1
fmtio16 >> %LOG% 2>&1
echo == mixed Fortran and C >> %LOG%
if not exist %WATCOM%\binp\wcc386.exe echo no C compiler (C/C++ package not installed) >> %LOG%
if exist %WATCOM%\binp\wcc386.exe wcc386 -q cmix.c >> %LOG% 2>&1
if exist cmix.obj wfl386 -q -l=os2v2 fmix.for cmix.obj >> %LOG% 2>&1
if exist fmix.exe fmix >> %LOG% 2>&1
echo == samples: mthread (multithreaded runtime) >> %LOG%
copy %WATCOM%\samples\fortran\os2\mthread.for . > nul
wfl386 -q -l=os2v2 -bm mthread.for >> %LOG% 2>&1
mthread >> %LOG% 2>&1
echo == samples: DLL >> %LOG%
copy %WATCOM%\samples\fortran\os2\dll\dll*.for . > nul
wfc386 -q -bd dllsamp.for >> %LOG% 2>&1
wlink system os2v2_dll initinstance terminstance op q,implib,manyautodata export DLL_ENTRY_1 export DLL_ENTRY_2 file dllsamp >> %LOG% 2>&1
rem (wfl386 1.9 writes "librarydllsamp.lib" into its linker directives
rem when given a .lib, as the sample's makefile does: link with wlink)
wfc386 -q dlltest.for >> %LOG% 2>&1
wlink op q system os2v2 file dlltest library dllsamp >> %LOG% 2>&1
dlltest >> %LOG% 2>&1
echo == samples: REXX external functions >> %LOG%
copy %WATCOM%\samples\fortran\os2\rexx\*.* . > nul
wmake -h -f makefile >> %LOG% 2>&1
cmd /c testdll.cmd >> %LOG% 2>&1
echo == samples: fshapes (PM) and calendar (VIO) >> %LOG%
copy %WATCOM%\samples\fortran\os2\fshapes.* . > nul
wfl386 -q -l=os2v2_pm fshapes.for >> %LOG% 2>&1
copy %WATCOM%\samples\fortran\os2\calendar.for . > nul
wfl386 -q -l=os2v2 calendar.for >> %LOG% 2>&1
echo == a debug build for wd >> %LOG%
wfl386 -q -d2 -l=os2v2 -fe=fmathdbg.exe fmath.for >> %LOG% 2>&1
echo == DOS and Windows targets (built here, run separately) >> %LOG%
wfl386 -q -l=dos4g -fe=fmathx.exe fmath.for >> %LOG% 2>&1
wfl -q -l=dos -fe=fmathd.exe fmath.for >> %LOG% 2>&1
wfl -q -l=dos -fe=fileiod.exe fileio.for >> %LOG% 2>&1
wfl -q -l=windows -bw -fe=fmathw.exe fmath.for >> %LOG% 2>&1
copy %WATCOM%\samples\fortran\win\*.for . > nul
copy %WATCOM%\samples\fortran\win\*.fi . > nul
set FINCLUDE=%WATCOM%\src\fortran\win;%FINCLUDE%
wfc -q -windows -fo=ellips16 ellipse.for >> %LOG% 2>&1
wlink op q system windows op heapsize=32K file ellips16 >> %LOG% 2>&1
wfc -q -windows -fo=fwcopy16 fwcopy.for >> %LOG% 2>&1
wlink op q system windows op heapsize=32K file fwcopy16 >> %LOG% 2>&1
copy %WATCOM%\binw\dos4gw.exe . > nul
dir *.exe >> %LOG%
echo == done >> %LOG%
