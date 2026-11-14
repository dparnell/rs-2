@echo off
rem The Heap Walker with another Windows task running (its blocks are in
rem the global heap list too): Zoom, then the Heap Walker. From the
rem command prompt of an owsys.img made with INIT="OS2\CMD.EXE":
rem   c:\owt\heapwlk
start %WATCOM%\binw\wzoom.exe
%WATCOM%\binw\wheapwlk.exe
