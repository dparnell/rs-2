/* Spy watching other Windows tasks: Spy > All Windows (its system-wide
   hooks in WSPYHK.DLL), then DDECLI16 and DDESRV16 run. From the
   command prompt of an owsys.img made with INIT="OS2\CMD.EXE":
     c:\owt\wspy   (and choose Spy > All Windows within 40 seconds) */
call RxFuncAdd "SysLoadFuncs", "RexxUtil", "SysLoadFuncs"
call SysLoadFuncs
"start" value("WATCOM",,"OS2ENVIRONMENT")"\binw\wspy.exe"
call SysSleep 40
"c:\rs2\ddecli16.exe"
