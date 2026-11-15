@echo off
rem Boot program for the Open Watcom 2.0 tests on work/ow2sys.img
rem (tests/owatcom2/mkdisk.sh): the 1.9 tests (BUILD.CMD, TOOLS.CMD,
rem MORE.CMD) and TOOLS2.CMD in C:\OWT, then a restart (xtask runs QEMU
rem with -no-reboot: it ends there).
c:
cd \owt
call build.cmd
call tools.cmd
call tools2.cmd
call more.cmd
setboot /b
