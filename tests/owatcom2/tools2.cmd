@echo off
rem Open Watcom 2.0 checks beyond the 1.9 tests (TOOLS.CMD): its C99 and
rem file I/O runtime, its C++ library, the runtime DLLs (-br), a DLL used
rem by one program after another and rebuilt in between, the 16-bit
rem compilers and the linker run by themselves. Output in TOOLS2.LOG.
set L=TOOLS2.LOG
echo == tools2.cmd > %L%

echo == c99io (wcl386 -za99) >> %L%
wcl386 -zq -za99 -l=os2v2 c99io.c >> %L% 2>&1
c99io >> %L% 2>&1
if errorlevel 1 echo c99io FAILED >> %L%
echo == c99io -bm -br (runtime DLLs) >> %L%
wcl386 -zq -za99 -l=os2v2 -bm -br -fe=c99ior.exe c99io.c >> %L% 2>&1
c99ior >> %L% 2>&1
if errorlevel 1 echo c99ior FAILED >> %L%
rem The program above was the first to use PM (CLBR20 imports PMWIN): the
rem PM server it started must not hold TOOLS2.LOG (SYS0032).
echo == echo right after a -br program >> %L%
echo == c99io 16-bit (wcl -ml) >> %L%
wcl -zq -za99 -ml -l=os2 -i=%WATCOM%\h\os21x -fe=c99io16.exe c99io.c >> %L% 2>&1
c99io16 >> %L% 2>&1
if errorlevel 1 echo c99io16 FAILED >> %L%

echo == cpp2 (wcl386 -xs -xr -bm) >> %L%
wcl386 -zq -xs -xr -bm -l=os2v2 cpp2.cpp >> %L% 2>&1
cpp2 >> %L% 2>&1
if errorlevel 1 echo cpp2 FAILED >> %L%
echo == cpp2 -br (runtime DLLs) >> %L%
wcl386 -zq -xs -xr -bm -br -l=os2v2 -fe=cpp2r.exe cpp2.cpp >> %L% 2>&1
cpp2r >> %L% 2>&1
if errorlevel 1 echo cpp2r FAILED >> %L%
echo == cpp2 16-bit (wcl -xs -xr -ml) >> %L%
wcl -zq -xs -xr -ml -l=os2 -i=%WATCOM%\h\os21x -fe=cpp216.exe cpp2.cpp >> %L% 2>&1
cpp216 >> %L% 2>&1
if errorlevel 1 echo cpp216 FAILED >> %L%

echo == a DLL used twice, rebuilt, used again >> %L%
wcl386 -zq -l=os2v2_dll -bd mydll.c >> %L% 2>&1
wlib -q -n mydll.lib +mydll.dll >> %L% 2>&1
wcl386 -zq -l=os2v2 usedll.c mydll.lib >> %L% 2>&1
usedll >> %L% 2>&1
if errorlevel 1 echo usedll 1 FAILED >> %L%
usedll >> %L% 2>&1
if errorlevel 1 echo usedll 2 FAILED >> %L%
wcl386 -zq -l=os2v2_dll -bd mydll.c >> %L% 2>&1
usedll >> %L% 2>&1
if errorlevel 1 echo usedll 3 FAILED >> %L%

echo == wcc, wpp, wcc386, wpp386 and wlink by themselves >> %L%
wcc -zq -ml -i=%WATCOM%\h\os21x -fo=h16.obj hello16.c >> %L% 2>&1
wlink op q system os2 name h16s.exe file h16.obj >> %L% 2>&1
h16s >> %L% 2>&1
wpp -zq -xs -xr -ml -i=%WATCOM%\h\os21x -fo=c16.obj cpptest.cpp >> %L% 2>&1
wlink op q system os2 name c16s.exe file c16.obj >> %L% 2>&1
c16s >> %L% 2>&1
if errorlevel 1 echo c16s FAILED >> %L%
wcc386 -zq -fo=h32.obj hello.c >> %L% 2>&1
wlink op q system os2v2 name h32s.exe file h32.obj >> %L% 2>&1
h32s s >> %L% 2>&1
wpp386 -zq -xs -xr -fo=c32.obj cpptest.cpp >> %L% 2>&1
wlink op q system os2v2 name c32s.exe file c32.obj >> %L% 2>&1
c32s >> %L% 2>&1
if errorlevel 1 echo c32s FAILED >> %L%
echo == done >> %L%
