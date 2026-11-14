@echo off
rem Boot program for the Open Watcom tests on work/owsys.img (mkdisk.sh):
rem runs BUILD.CMD, TOOLS.CMD and MORE.CMD in C:\OWT, then restarts the system
rem (xtask runs QEMU with -no-reboot: it ends there).
c:
cd \owt
call build.cmd
call tools.cmd
call more.cmd
setboot /b
