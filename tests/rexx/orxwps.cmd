/* ORXWPS.CMD: scripting the Workplace Shell from Object REXX (REXXWPS.DLL
   in the running shell, registered by WPSINST.CMD). Run it with PMSHELL
   running: RS2\WPSTEST.EXE SHELL RUN C:\CMD.EXE /C ORXWPS.CMD.
   Prints one line per check and "ORXWPS: n passed, m failed". */
passed = 0
failed = 0

call 'C:\OS2\WPSINST.CMD' '+'
call check .wps~class \= .nil, '.WPS (the Workplace Shell server)'
call check .wpdesktop~wpQueryTitle \= '', 'the desktop:' .wpdesktop~wpQueryTitle

f = .wpfolder~new('REXX Folder', '', .wpdesktop, 1)
call check f~wpQueryTitle = 'REXX Folder', 'a folder made on the desktop'
f~wpSetTitle('REXX Test Folder')
call check f~wpQueryTitle = 'REXX Test Folder', 'wpSetTitle'
p = .wpprogram~new('REXX Program', 'EXENAME=C:\CMD.EXE', f, 1)
call check p~wpQueryTitle = 'REXX Program', 'a program object in the folder'
call check p~wpSetup('NODELETE=NO') = 1, 'wpSetup'
call check p~wpDelete(0) \= 0, 'the program object deleted'
call check f~wpDelete(0) \= 0, 'the folder deleted'

say 'ORXWPS:' passed 'passed,' failed 'failed'
exit failed

check: procedure expose passed failed
  parse arg ok, what
  if ok = 1 then do
    passed = passed + 1
    say 'ok -' what
  end
  else do
    failed = failed + 1
    say 'FAIL -' what
  end
  return
