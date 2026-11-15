@echo off
rem The Dialog Editor's files, checked with wrc: C:\OWT\T1.RES bound
rem to a Windows program, C:\OWT\T2.DLG compiled in a resource script
rem (WDET3.RC). Run after saving them in wde (c:\owt\wde).
c:
cd \owt
wcl -zq -l=windows -bt=windows -i=%WATCOM%\h\win winhello.c
echo == wrc t1.res winhello.exe
wrc -q -bt=windows t1.res winhello.exe
if errorlevel 1 goto bad1
echo OK: t1.res bound to winhello.exe
goto dlg
:bad1
echo FAILED: t1.res
:dlg
echo == wrc -r wdet3.rc
wrc -q -r -bt=windows -i=%WATCOM%\h\win wdet3.rc
if errorlevel 1 goto bad2
echo OK: t2.dlg compiled to wdet3.res
goto end
:bad2
echo FAILED: t2.dlg
:end
dir t*.res t2.dlg wdet3.res
