/* SWITCHRX.CMD: make Object REXX the system's REXX, or classic REXX again.

   rs2 installs both: classic REXX as REXX.DLL, REXXUTIL.DLL, REX.MSG and
   REXH.MSG, Object REXX beside it as OREXX.DLL, OREXUTIL.DLL,
   OREXXSOM.DLL, OREXXWPS.DLL, OREX.MSG and OREXH.MSG. Switching renames
   the files of the REXX that runs to C... (classic) or O... (Object) names
   and the other REXX's files to the system's names. Programs that have
   REXX loaded keep it: the switch takes effect when the system restarts.

   SWITCHRX      asks before it switches
   SWITCHRX Y    switches without asking */

parse arg answer .
parse version . level .
parse source . . me
drive = left(me, 2)
if substr(drive, 2, 1) \= ':' then drive = 'C:'
dll = drive'\OS2\DLL\'
sys = drive'\OS2\SYSTEM\'

/* which REXX the files make the system's: CREXX.DLL is classic REXX put
   aside (the REXX that runs may still be the other one until the system
   restarts) */
if stream(dll'CREXX.DLL', 'c', 'query exists') \= '' then level = 6
else if stream(dll'OREXX.DLL', 'c', 'query exists') \= '' then level = 4
if level >= 6 then do
  now = 'Object REXX'; next = 'Classic REXX'; old = 'O'; new = 'C'
end
else do
  now = 'Classic REXX'; next = 'Object REXX'; old = 'C'; new = 'O'
end

/* the files: system name, classic name (- none), Object REXX name */
files.0 = 6
files.1 = dll'REXX.DLL'     dll'CREXX.DLL'     dll'OREXX.DLL'
files.2 = dll'REXXUTIL.DLL' dll'CREXUTIL.DLL'  dll'OREXUTIL.DLL'
files.3 = dll'REXXSOM.DLL'  '-'                dll'OREXXSOM.DLL'
files.4 = dll'REXXWPS.DLL'  '-'                dll'OREXXWPS.DLL'
files.5 = sys'REX.MSG'      sys'CREX.MSG'      sys'OREX.MSG'
files.6 = sys'REXH.MSG'     sys'CREXH.MSG'     sys'OREXH.MSG'

/* the files the other REXX needs must be there */
do i = 1 to files.0
  parse var files.i cur c o
  if c = '-' then c = ''
  if new = 'O' then want = o; else want = c
  if want \= '' & stream(want, 'c', 'query exists') = '' then do
    say 'SWITCHRX: 'want' is missing; 'next' cannot be installed.'
    exit 2
  end
end

say 'The system''s REXX is 'now'. SWITCHRX makes it 'next'.'
if translate(left(answer, 1)) \= 'Y' then do
  say 'Switch to 'next'? (Y/N)'
  pull answer
  if left(answer, 1) \= 'Y' then do
    say 'Nothing was changed.'
    exit 1
  end
end

do i = 1 to files.0
  parse var files.i cur c o
  if c = '-' then c = ''
  if old = 'O' then keep = o; else keep = c
  if new = 'O' then want = o; else want = c
  /* the running REXX's file to its own name (Object REXX's SOM and WPS
     DLLs have no classic counterpart) */
  if stream(cur, 'c', 'query exists') \= '' then do
    if keep = '' then do
      /* Object REXX's own DLL left from an earlier switch */
      if new = 'O' & stream(want, 'c', 'query exists') \= '' then '@DEL "'cur'" >NUL 2>&1'
      if new = 'C' then call rename cur, o
    end
    else call rename cur, keep
  end
  if want \= '' then call rename want, cur
end

say next' is installed. It is the system''s REXX once the system restarts.'
exit 0

/* rename a file (full names) */
rename: procedure
  parse arg from, to
  '@REN "'from'" "'filespec('name', to)'" >NUL 2>&1'
  if rc \= 0 then say 'SWITCHRX: 'from' could not be renamed to 'to' ('rc').'
  return
