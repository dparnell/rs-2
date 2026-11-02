/* RXQTEST.CMD: RXQUEUE.EXE and RXSUBCOM.EXE. RXQTEST [dir] runs the
   programs in dir (e.g. C:\IBM\ for IBM's), else those on the PATH.
   It prints one line per check and "RXQTEST: n passed, m failed". */
parse arg dir .
rxq = dir'RXQUEUE'
rxs = dir'RXSUBCOM'
tmp = 'C:\RXQTEST.TMP'

/* The session queue, FIFO */
call drain
'@echo first line|' rxq
call check rc = 0, 'rc 0 after queueing'
'@echo second line|' rxq
call check queued() = 2, 'two lines in the session queue'
parse pull l1
parse pull l2
call check l1 = 'first line' & l2 = 'second line', 'FIFO order, case kept'

/* Several lines, CR LF, an empty line, a long line */
call drain
'@del' tmp '2>nul'
call lineout tmp, 'alpha'
call lineout tmp, ''
call lineout tmp, '  gamma  '
call lineout tmp, copies('z', 5000)
call lineout tmp
'@type' tmp '|' rxq
call check queued() = 4, 'four lines from a file'
parse pull l1
parse pull l2
parse pull l3
parse pull l4
call check l1 = 'alpha' & l2 = '' & l3 = '  gamma  ', 'empty line and blanks kept'
call check length(l4) = 4096, 'long line cut at 4096 characters'

/* Bare LF and CR line ends */
call drain
'@del' tmp '2>nul'
call charout tmp, 'one'||'0a'x||'two'||'0d'x||'three'||'0d0a'x||'four'
call charout tmp
'@type' tmp '|' rxq
call check queued() = 4, 'LF, CR and CR LF end lines, the last needs none'
parse pull l1
parse pull l2
parse pull l3
parse pull l4
call check l1 l2 l3 l4 = 'one two three four', 'lines split at LF and CR'

/* Ctrl+Z ends the input */
call drain
'@del' tmp '2>nul'
call charout tmp, 'kept'||'0d0a'x||'dropped'||'1a'x||'after'||'0d0a'x
call charout tmp
'@'rxq '<' tmp
call check queued() = 1, 'Ctrl+Z ends the input, its line dropped:' queued()
call drain
'@del' tmp '2>nul'
call charout tmp, 'start'||'0d0a'x||'1a'x||'more'
call charout tmp
'@'rxq '<' tmp
call check queued() = 2, 'Ctrl+Z starting a line queues an empty line:' queued()
call drain

/* /LIFO and /FIFO on the session queue */
'@echo bottom|' rxq '/FIFO'
'@echo top|' rxq '/lifo'
parse pull l1
parse pull l2
call check l1 = 'top' & l2 = 'bottom', '/LIFO queues at the front'
'@echo dash|' rxq '-LIFO'
parse pull l1
call check l1 = 'dash', '-LIFO as /LIFO'

/* A named queue made by RXQUEUE('Create') */
qn = rxqueue('Create')
'@echo named one|' rxq qn
'@echo named two|' rxq qn '/LIFO'
call check queued() = 0, 'named queue: session queue untouched'
old = rxqueue('Set', qn)
call check queued() = 2, 'named queue has two lines'
parse pull l1
call check l1 = 'named two', 'named queue /LIFO'
'@echo named three|' rxq qn
'@'rxq qn '/CLEAR'
call check rc = 0 & queued() = 0, '/CLEAR empties the queue'
call rxqueue 'Set', old
call rxqueue 'Delete', qn

/* %RXQUEUE% names the default queue */
qn = rxqueue('Create', 'RXQTESTQ')
call check qn = 'RXQTESTQ', 'queue created by name'
call value 'RXQUEUE', 'RXQTESTQ', 'OS2ENVIRONMENT'
'@echo via environment|' rxq
call value 'RXQUEUE', '', 'OS2ENVIRONMENT'
call check queued() = 0, 'environment queue: session queue untouched'
old = rxqueue('Set', 'RXQTESTQ')
parse pull l1
call check l1 = 'via environment', '%RXQUEUE% used as the queue name'
call rxqueue 'Set', old
call rxqueue 'Delete', 'RXQTESTQ'

/* Errors: messages on standard error */
call drain
'@'rxq 'NOSUCHQ 2>&1 <nul |' rxq
call check queued() >= 1, 'message for a missing queue'
parse pull l1
call drain
call check pos('REX0124', l1) > 0 & pos('NOSUCHQ', l1) > 0, 'REX0124 names the queue:' l1
'@'rxq '/BOGUS 2>&1 <nul |' rxq
parse pull l1
call drain
call check pos('SYS1003', l1) > 0, 'bad option gives SYS1003:' l1
'@'rxq 'NOSUCHQ <nul 2>nul'
call check rc = 0, 'error return code 0 (as IBM):' rc
'@'rxq 'A B <nul 2>nul'
call check rc = 0, 'two queue names: rc' rc

/* RXSUBCOM */
'@'rxs '2>nul'
call check rc = -1, 'RXSUBCOM without parameters: rc' rc
'@'rxs '2>&1 |' rxq
parse pull l1
call drain
call check pos('REX0115', l1) > 0, 'usage message:' l1
'@'rxs 'REGISTER X 2>&1 |' rxq
parse pull l1
call drain
call check pos('REX0116', l1) > 0, 'REGISTER usage:' l1
'@'rxs 'DROP 2>&1 |' rxq
parse pull l1
call drain
call check pos('REX0117', l1) > 0, 'DROP usage:' l1
'@'rxs 'LOAD 2>&1 |' rxq
parse pull l1
call drain
call check pos('REX0118', l1) > 0, 'LOAD usage:' l1
'@'rxs 'QUERY 2>&1 |' rxq
parse pull l1
call drain
call check pos('REX0125', l1) > 0, 'QUERY usage:' l1
'@'rxs 'FROB X 2>nul'
call check rc = -1, 'unknown keyword: rc' rc
'@'rxs 'QUERY RXQTESTENV'
call check rc = 30, 'QUERY of an unknown environment: rc' rc
'@'rxs 'REGISTER RXQTESTENV REXXUTIL SysCls'
call check rc = 0, 'REGISTER: rc' rc
'@'rxs 'register RXQTESTENV REXXUTIL SysCls'
call check rc = 30, 'REGISTER again (lower case): rc' rc '(REXXAPI''s answer)'
'@'rxs 'QUERY RXQTESTENV REXXUTIL'
call check rc = 0, 'QUERY with the DLL: rc' rc
'@'rxs 'QUERY RXQTESTENV'
qrc = rc
'@'rxs 'LOAD RXQTESTENV REXXUTIL'
call check rc = 0, 'LOAD: rc' rc
'@'rxs 'DROP RXQTESTENV REXXUTIL'
call check rc = 0, 'DROP: rc' rc
'@'rxs 'QUERY RXQTESTENV REXXUTIL'
call check rc = 30, 'QUERY after DROP: rc' rc
'@'rxs 'DROP RXQTESTENV REXXUTIL'
call check rc = 30, 'DROP again: rc' rc
say 'QUERY without the DLL gave rc' qrc

'@del' tmp '2>nul'
say 'RXQTEST:' passed 'passed,' failed 'failed'
exit failed

drain: procedure
  do queued()
    pull
  end
  return

check: procedure expose passed failed
  if symbol('passed') = 'LIT' then do; passed = 0; failed = 0; end
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
