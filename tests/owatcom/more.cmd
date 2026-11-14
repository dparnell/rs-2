@echo off
rem The other text-mode tools in %WATCOM%\BINP: each with its usage and one
rem real task. Output in MORE.LOG.
set L=MORE.LOG
echo == more.cmd > %L%
echo == vi -k (text-mode editor, keystrokes) >> %L%
vi -k "Goadded by vi\e:wq\n" vitest.txt >> %L% 2>&1
type vitest.txt >> %L%
echo == wsample busy.exe >> %L%
wcl386 -zq -l=os2v2 -d2 busy.c >> %L% 2>&1
wsample busy.exe >> %L% 2>&1
dir busy.smp >> %L%
echo == ctags >> %L%
ctags -? >> %L% 2>&1
ctags hello.c mttest.c >> %L% 2>&1
type tags >> %L%
echo == dmpobj hello.obj >> %L%
dmpobj hello.obj > DMPOBJ.DMP 2>&1
find "PUBDEF" DMPOBJ.DMP >> %L%
echo == cvpack >> %L%
wcl386 -zq -l=os2v2 -hc -d2 -fe=hellocv.exe hello.c >> %L% 2>&1
cvpack hellocv.exe >> %L% 2>&1
hellocv cv >> %L% 2>&1
echo == exe2bin (a .COM program) >> %L%
wasm -zq com.asm >> %L% 2>&1
wlink op q system dos name com.exe file com.obj >> %L% 2>&1
exe2bin com.exe com.com >> %L% 2>&1
dir com.com >> %L%
del COM.OUT
com.com
echo == ms2wlink >> %L%
ms2wlink hello.obj,hellom.exe,,,; > hellom.lnk 2>> %L%
type hellom.lnk >> %L%
echo == wbind (usage) >> %L%
wbind >> %L% 2>&1
echo == fcenable (usage) >> %L%
fcenable >> %L% 2>&1
echo == edbind (usage) >> %L%
edbind >> %L% 2>&1
echo == wasaxp, wasppc (usage) >> %L%
wasaxp -? >> %L% 2>&1
wasppc -? >> %L% 2>&1
echo == wlib -l (list) >> %L%
wlib -q -l=mylib.lst mylib.lib >> %L% 2>&1
type mylib.lst >> %L%
echo == wdump -a mydll.dll >> %L%
wdump -q -e mydll.dll > MYDLL.DMP 2>&1
find "DllAdd" MYDLL.DMP >> %L%
echo == ide2make (usage) >> %L%
ide2make -? >> %L% 2>&1
echo == NMPBIND (CONFIG.SYS RUN=, on NAMPIPES.DLL) is running >> %L%
pstat | find /i "nmpbind" >> %L%
echo == results of the other sessions >> %L%
waitfile COM.OUT 30 >> %L% 2>&1
echo == done >> %L%
