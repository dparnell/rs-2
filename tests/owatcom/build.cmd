@echo off
rem Builds and runs the Open Watcom test programs; output in D:\BUILD.LOG
d:
cd \
echo == wcl386 hello.c > build.log
wcl386 -l=os2v2 hello.c >> build.log 2>&1
echo == hello >> build.log
hello a b >> build.log 2>&1
echo == wcl hello16.c >> build.log
wcl -l=os2 -i=%WATCOM%\h\os21x hello16.c >> build.log 2>&1
echo == hello16 >> build.log
hello16 >> build.log 2>&1
echo == wcl386 pmhello.c >> build.log
wcl386 -l=os2v2_pm pmhello.c >> build.log 2>&1
echo == pmhello >> build.log
pmhello
type pmhello.ok >> build.log
echo == wmake >> build.log
del *.obj
del *.exe
wmake -h >> build.log 2>&1
dir *.exe >> build.log
echo == done >> build.log
