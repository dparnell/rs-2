/* PMTESTS.CMD: starts the programs BUILD.CMD built that need the desktop,
   one after the other (copy it to C:\STARTUP.CMD to run them when the
   Workplace Shell starts): the PM sample FSHAPES, the Windows programs
   (FMATHW in its default window, the ELLIPSE and FWCOPY samples) on rs2's
   WIN16, and the FORTRAN 77 Language Reference in the help viewer. */
call RxFuncAdd 'SysSleep', 'REXXUTIL', 'SysSleep'
'@d:'
'@cd \'
'@start fshapes'
call SysSleep 25
'@start fmathw'
call SysSleep 25
'@start ellips16'
call SysSleep 25
'@start fwcopy16'
call SysSleep 25
'@start view f77lr'
