@echo off
rem Builds and runs the Open Watcom FORTRAN 77 test programs in C:\F77T
rem (tests/owatcom/f77 with this directory's files); everything goes to
rem C:\F77T\F77.LOG. tests/owatcom/f77/BUILD.CMD for Open Watcom 2.0, whose
rem Current-build has two bugs of its own (not rs2's, see
rem docs/COMPATIBILITY.md): every compilation ends with "*ERR* CP-09"
rem (dynamic memory not freed, Open Watcom issue 1623) and exit code 1
rem although the object file is complete, and WFL/WFL386 leave the next
rem pointer of their file list uninitialised (ListAppendString in wfl.c)
rem and fault or loop. So this compiles with wfc/wfc386, links with wlink
rem and runs wmake with -i; wfl386 is tried once by itself at the end.
c:
cd \f77t
set LOG=c:\f77t\f77.log
echo == FORTRAN 77 tests (Open Watcom 2.0) > %LOG%
echo == wfc386 + wlink: fmtio fileio fmath fchars >> %LOG%
for %%f in (fmtio fileio fmath fchars) do wfc386 -q %%f.for >> %LOG% 2>&1
for %%f in (fmtio fileio fmath fchars) do wlink op q system os2v2 file %%f >> %LOG% 2>&1
fmtio >> %LOG% 2>&1
fileio >> %LOG% 2>&1
fmath >> %LOG% 2>&1
fchars >> %LOG% 2>&1
echo == wfc + wlink (16-bit OS/2) >> %LOG%
wfc -q -fo=fmath16.obj fmath.for >> %LOG% 2>&1
wlink op q system os2 name fmath16 file fmath16 >> %LOG% 2>&1
fmath16 >> %LOG% 2>&1
wfc -q -fo=fchar16.obj fchars.for >> %LOG% 2>&1
wlink op q system os2 name fchar16 file fchar16 >> %LOG% 2>&1
fchar16 >> %LOG% 2>&1
wfc -q -fo=fmtio16.obj fmtio.for >> %LOG% 2>&1
wlink op q system os2 name fmtio16 file fmtio16 >> %LOG% 2>&1
fmtio16 >> %LOG% 2>&1
echo == wfc386 -d2 + wlink debug all >> %LOG%
wfc386 -q -d2 -fo=filelnk.obj fileio.for >> %LOG% 2>&1
wlink op q d all system os2v2 file filelnk >> %LOG% 2>&1
filelnk >> %LOG% 2>&1
echo == mixed Fortran and C >> %LOG%
wcc386 -q cmix.c >> %LOG% 2>&1
wfc386 -q fmix.for >> %LOG% 2>&1
wlink op q system os2v2 name fmix file fmix,cmix >> %LOG% 2>&1
fmix >> %LOG% 2>&1
echo == samples: mthread (multithreaded runtime) >> %LOG%
copy %WATCOM%\samples\fortran\os2\mthread.for . > nul
wfc386 -q -bm mthread.for >> %LOG% 2>&1
wlink op q system os2v2 file mthread >> %LOG% 2>&1
mthread >> %LOG% 2>&1
echo == samples: DLL >> %LOG%
copy %WATCOM%\samples\fortran\os2\dll\dll*.for . > nul
wfc386 -q -bd dllsamp.for >> %LOG% 2>&1
wlink system os2v2_dll initinstance terminstance op q,implib,manyautodata export DLL_ENTRY_1 export DLL_ENTRY_2 file dllsamp >> %LOG% 2>&1
wfc386 -q dlltest.for >> %LOG% 2>&1
wlink op q system os2v2 file dlltest library dllsamp >> %LOG% 2>&1
dlltest >> %LOG% 2>&1
echo == samples: REXX external functions (wmake -i) >> %LOG%
copy %WATCOM%\samples\fortran\os2\rexx\*.* . > nul
wmake -h -i -f makefile >> %LOG% 2>&1
cmd /c testdll.cmd >> %LOG% 2>&1
echo == samples: fshapes (PM) and calendar (VIO) >> %LOG%
copy %WATCOM%\samples\fortran\os2\fshapes.* . > nul
wfc386 -q fshapes.for >> %LOG% 2>&1
wlink op q system os2v2_pm file fshapes >> %LOG% 2>&1
copy %WATCOM%\samples\fortran\os2\calendar.for . > nul
wfc386 -q calendar.for >> %LOG% 2>&1
wlink op q system os2v2 file calendar >> %LOG% 2>&1
echo == a debug build for wd >> %LOG%
wfc386 -q -d2 -fo=fmathdbg.obj fmath.for >> %LOG% 2>&1
wlink op q d all system os2v2 name fmathdbg file fmathdbg >> %LOG% 2>&1
echo == DOS and Windows targets (built here, run separately) >> %LOG%
wfc386 -q -fo=fmathx.obj fmath.for >> %LOG% 2>&1
wlink op q system dos4g name fmathx file fmathx >> %LOG% 2>&1
wfc -q -fo=fmathd.obj fmath.for >> %LOG% 2>&1
wlink op q system dos name fmathd file fmathd >> %LOG% 2>&1
wfc -q -fo=fileiod.obj fileio.for >> %LOG% 2>&1
wlink op q system dos name fileiod file fileiod >> %LOG% 2>&1
wfc -q -windows -bw -fo=fmathw.obj fmath.for >> %LOG% 2>&1
wlink op q system windows op heapsize=32K name fmathw file fmathw >> %LOG% 2>&1
copy %WATCOM%\samples\fortran\win\*.for . > nul
copy %WATCOM%\samples\fortran\win\*.fi . > nul
set FINCLUDE=%WATCOM%\src\fortran\win;%FINCLUDE%
wfc -q -explicit -nowarn -d2 -windows -fo=ellips16 ellipse.for >> %LOG% 2>&1
wlink d all op q system windows op heapsize=32K file ellips16 >> %LOG% 2>&1
wfc -q -explicit -nowarn -d2 -windows -fo=fwcopy16 fwcopy.for >> %LOG% 2>&1
wlink d all op q system windows op heapsize=32K file fwcopy16 >> %LOG% 2>&1
copy %WATCOM%\binw\dos4gw.exe . > nul
echo == wfl386 by itself (Open Watcom's file list bug) >> %LOG%
wfl386 -q -c fmath.for >> %LOG% 2>&1
dir *.exe >> %LOG%
echo == done >> %LOG%
