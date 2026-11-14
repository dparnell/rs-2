/* DDE Spy watching DDE between two other Windows tasks: DDECLI16 (raw
   WM_DDE_* messages) and DDESRV16 (a DDEML server it starts). From the
   command prompt of an owsys.img made with INIT="OS2\CMD.EXE":
     c:\owt\ddespy */
call RxFuncAdd "SysLoadFuncs", "RexxUtil", "SysLoadFuncs"
call SysLoadFuncs
"start" value("WATCOM",,"OS2ENVIRONMENT")"\binw\wddespy.exe"
call SysSleep 30
"c:\rs2\ddecli16.exe"
