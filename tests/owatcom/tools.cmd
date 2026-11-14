@echo off
rem Open Watcom toolchain tests (C++, threads, DLLs, wasm, wlib, wdis,
rem wstrip, owcc, wlink directive files, wmake, wrc, wipfc, cross targets).
rem Run in the directory holding the test sources; output in TOOLS.LOG.
set L=TOOLS.LOG
echo == tools.cmd > %L%
wcl386 -zq -l=os2v2 waitfile.c >> %L% 2>&1

echo == dos: wcl -l=dos xhello.c >> %L%
wcl -zq -l=dos -fe=xdos.exe xhello.c >> %L% 2>&1
echo == dos4g: wcl386 -l=dos4g xhello.c >> %L%
wcl386 -zq -l=dos4g -fe=xd4g.exe xhello.c >> %L% 2>&1
copy %WATCOM%\binw\dos4gw.exe . >> %L%
echo == win16: wcl -l=windows winhello.c >> %L%
wcl -zq -l=windows -bt=windows -i=%WATCOM%\h\win winhello.c >> %L% 2>&1
echo == nt: wcl386 -l=nt xhello.c >> %L%
wcl386 -zq -l=nt -bt=nt -i=%WATCOM%\h\nt -fe=xnt.exe xhello.c >> %L% 2>&1
wdump xnt.exe > XNT.DMP 2>&1
echo == start the DOS, DOS/4GW and Win16 programs >> %L%
del XDOS.OUT
del XD4G.OUT
del WINHELLO.OUT
xdos.exe dos XDOS.OUT
rem An LE program does not run in an OS/2 session (SYS0191, as on OS/2):
rem DOS/4GW programs run in a DOS session.
xd4g.exe d4g XD4G.OUT >> %L% 2>&1
rund4g.bat
winhello.exe

echo == wpp386 cpptest.cpp >> %L%
wcl386 -zq -l=os2v2 -xs -xr cpptest.cpp >> %L% 2>&1
del CPPDTOR.OK
cpptest >> %L% 2>&1
if errorlevel 1 echo cpptest FAILED >> %L%
type CPPDTOR.OK >> %L%
echo == wpp cpptest.cpp 16-bit >> %L%
wcl -zq -l=os2 -xs -xr -ml -i=%WATCOM%\h\os21x -fe=cpp16.exe cpptest.cpp >> %L% 2>&1
del CPPDTOR.OK
cpp16 >> %L% 2>&1
if errorlevel 1 echo cpp16 FAILED >> %L%
type CPPDTOR.OK >> %L%

echo == mttest -bm >> %L%
wcl386 -zq -l=os2v2 -bm mttest.c >> %L% 2>&1
mttest >> %L% 2>&1
if errorlevel 1 echo mttest FAILED >> %L%
echo == mttest -bm -br >> %L%
wcl386 -zq -l=os2v2 -bm -br -fe=mttestr.exe mttest.c >> %L% 2>&1
mttestr >> %L% 2>&1
if errorlevel 1 echo mttestr FAILED >> %L%

echo == dll: mydll + usedll >> %L%
wcl386 -zq -l=os2v2_dll -bd mydll.c >> %L% 2>&1
wlib -q -n mydll.lib +mydll.dll >> %L% 2>&1
wcl386 -zq -l=os2v2 usedll.c mydll.lib >> %L% 2>&1
usedll >> %L% 2>&1
if errorlevel 1 echo usedll FAILED >> %L%
echo == C++ dll -br: cppdll + usecpp >> %L%
wcl386 -zq -l=os2v2_dll -bd -br -xs cppdll.cpp >> %L% 2>&1
wlib -q -n cppdll.lib +cppdll.dll >> %L% 2>&1
wcl386 -zq -l=os2v2 -br -xs usecpp.cpp cppdll.lib >> %L% 2>&1
usecpp >> %L% 2>&1
if errorlevel 1 echo usecpp FAILED >> %L%

echo == wasm + wlib >> %L%
wasm -zq asmfn.asm >> %L% 2>&1
wcc386 -zq libfn.c >> %L% 2>&1
wcc386 -zq hello.c >> %L% 2>&1
wlib -q -n mylib.lib +asmfn.obj +libfn.obj >> %L% 2>&1
wlib -q mylib.lib >> %L% 2>&1
wcl386 -zq -l=os2v2 asmmain.c mylib.lib >> %L% 2>&1
asmmain >> %L% 2>&1
if errorlevel 1 echo asmmain FAILED >> %L%
echo == wdis asmfn.obj >> %L%
wdis asmfn.obj >> %L% 2>&1
echo == wlink @hellow.lnk >> %L%
wlink @hellow.lnk >> %L% 2>&1
hellow x >> %L% 2>&1
dir hellow.map >> %L%
echo == wstrip >> %L%
wcl386 -zq -l=os2v2 -d2 -fe=hellod.exe hello.c >> %L% 2>&1
dir hellod.exe >> %L%
wstrip -q hellod.exe hellod.exe hellod.sym >> %L% 2>&1
dir hellod.exe >> %L%
dir hellod.sym >> %L%
hellod >> %L% 2>&1
echo == owcc >> %L%
owcc -o hellou.exe -O2 -Wall hello.c >> %L% 2>&1
hellou u >> %L% 2>&1
echo == wmake -f mk2.mak >> %L%
del asmmain.obj
del asmfn.obj
del libfn.obj
wmake -h -f mk2.mak >> %L% 2>&1
mk2 >> %L% 2>&1
if errorlevel 1 echo mk2 FAILED >> %L%
echo == wtouch >> %L%
wtouch -f hello.c -d 01-02-2003 -t 04:05:06 xhello.c >> %L% 2>&1
dir xhello.c >> %L%
echo == wdump hello.exe >> %L%
wdump -q hello.exe > HELLO.DMP 2>&1
dir HELLO.DMP >> %L%

echo == wrc pmres >> %L%
copy %WATCOM%\watcom.ico . >> %L%
wcc386 -zq pmres.c >> %L% 2>&1
wlink op q system os2v2_pm name pmres file pmres.obj >> %L% 2>&1
wrc -q pmres.rc pmres.exe >> %L% 2>&1
del PMRES.OK
pmres
type PMRES.OK >> %L%

echo == wipfc owhelp.ipf >> %L%
wipfc -i owhelp.ipf >> %L% 2>&1
dir owhelp.inf >> %L%
wipfc owhelp.ipf >> %L% 2>&1
dir owhelp.hlp >> %L%

echo == results of the other sessions >> %L%
waitfile XDOS.OUT 60 >> %L% 2>&1
waitfile XD4G.OUT 60 >> %L% 2>&1
waitfile WINHELLO.OUT 60 >> %L% 2>&1
echo == done >> %L%
