/* ORXTEST.CMD: IBM Object REXX on rs2 (classes, directives, collections,
   the environment objects, streams, conditions, messages, REXXUTIL).
   Run it with Object REXX installed as REXX.DLL; it prints one line per
   check and "ORXTEST: n passed, m failed" at the end. */
parse version ver
call check left(ver, 7) = 'OBJREXX', 'version' ver
parse source sys how name
call check sys = 'OS/2', 'parse source' sys how

/* Classes, methods, attributes, inheritance */
c = .counter~new(5)
c~step
c~step
call check c~value = 7, 'class method and attribute'
d = .bigcounter~new(10)
d~step
call check d~value = 20, 'subclass overriding with forward to super'
call check .bigcounter~superclasses[1] = .counter, 'superclasses'
call check .bigcounter~subclass('X')~superclasses[1] = .bigcounter, 'subclass at run time'
call check c~class~id = 'COUNTER', 'class id'
call check .counter~instances = 1 & .bigcounter~instances = 1, 'class methods with class object variables'
call check c~string = 'a COUNTER', 'default string'
call check d~hasMethod('STEP') & \d~hasMethod('NOPE'), 'hasMethod'
call check c~defaultname = 'a COUNTER', 'defaultname'

/* Routines and ::requires */
call check twice(21) = 42, '::routine'
call check orxreq_hello('rs2') = 'hello rs2', '::requires public routine'
r = .greeter~new
call check r~greet('OS/2') = 'Greetings, OS/2', '::requires public class'

/* USE ARG and argument passing by reference */
a = .array~new
call fill a, 3
call check a~items = 3 & a[3] = 'item3', 'use arg (object passed by reference)'
s. = 0
s.1 = 'x'
call usestem s.
call check s.2 = 'set by routine', 'use arg with a stem'

/* Collections */
arr = .array~of('b', 'a', 'c')
call check arr~size = 3 & arr[2] = 'a', '.array~of'
txt = ''
do i over arr
  txt = txt || i
end
call check txt = 'bac', 'do over an array'
l = .list~new
l~insert('one'); l~insert('two')
call check l~items = 2 & l~firstItem = 'one', '.list'
q = .queue~new
q~queue('a'); q~push('b')
call check q~pull = 'b' & q~pull = 'a', '.queue'
dir = .directory~new
dir~colour = 'blue'
dir['SIZE'] = 3
call check dir~colour = 'blue' & dir['SIZE'] = 3 & dir~hasIndex('COLOUR'), '.directory'
t = .table~new
t[1] = 'first'
t['k'] = 'second'
call check t~items = 2 & t['k'] = 'second', '.table'
st = .set~of(1, 2, 2, 3)
call check st~items = 3, '.set'
bag = .bag~of(1, 2, 2, 3)
call check bag~items = 4, '.bag'
rel = .relation~new
rel['x'] = 1; rel['x'] = 2
call check rel~allAt('x')~items = 2, '.relation'
stem = .stem~new
stem[1] = 'one'
call check stem[1] = 'one', '.stem'
total = 0
do k over t
  total = total + 1
end
call check total = 2, 'do over a table'
call check l~makearray~items = 2, 'list~makearray'

/* Strings, numbers, logicals */
call check 'abc'~reverse = 'cba' & 'Hello'~translate = 'HELLO' & 'a-b'~changestr('-', '+') = 'a+b', 'string methods'
call check (3 + 4)~d2x = '7' & 10~format(3, 2) = ' 10.00', 'number methods'
call check .true & \.false & .nil~string = 'The NIL object', '.true .false .nil'
call check 'abc'~class = .string, 'class of a literal'

/* The environment objects */
.local~orxtest.value = 42
call check .orxtest.value = 42, '.local'
call check .environment~hasIndex('ARRAY'), '.environment'
.stdout~lineout('ok - .stdout~lineout')
call check .stdout~class = .stream, '.stdout is a stream'

/* Streams */
f = .stream~new('C:\ORXTEST.TMP')
f~open('write replace')
f~lineout('line one')
f~lineout('line two')
f~close
f = .stream~new('C:\ORXTEST.TMP')
lns = f~arrayin
f~close
call check lns~items = 2 & lns[2] = 'line two', '.stream arrayin'
call check stream('C:\ORXTEST.TMP', 'c', 'query exists') \= '', 'stream query exists'
'@del C:\ORXTEST.TMP'

/* Conditions */
signal on syntax name trapped
x = 1 / 0
call check 0, 'syntax trap'
resume1:
call check condition('C') = 'SYNTAX' & result = 'trapped', 'signal on syntax'
call check catcher() = 'caught MYERROR', 'raise user condition'

/* Messages and concurrency */
m = .message~new(c, 'VALUE')
call check m~send = 7, '.message~send'
w = .worker~new
msg = w~start('COMPUTE', 6)
call check msg~result = 36, 'start (another thread) and result'
call check w~early = 'replied', 'reply'

/* Classic REXX inside Object REXX */
queue 'queued line'
pull line
call check line = 'QUEUED LINE', 'the external data queue'
'@ver > nul'
call check rc = 0, 'commands to CMD'
interpret 'y = 6 * 7'
call check y = 42, 'interpret'
call check value('PATH', , 'OS2ENVIRONMENT') \= '', 'value() of the environment'

/* REXXC, the tokenizer, and running a tokenized program */
'@del C:\ORXTOK*.CMD 2> nul'
call lineout 'C:\ORXTOKS.CMD', "/* tokenized */ return 'tokenized' arg(1)"
call lineout 'C:\ORXTOKS.CMD'
'@REXXC C:\ORXTOKS.CMD C:\ORXTOK.CMD > nul'
call check rc = 0, 'REXXC rc' rc
head = charin('C:\ORXTOK.CMD', 1, 14)
call stream 'C:\ORXTOK.CMD', 'c', 'close'
call check head \= '/* tokenized *', 'REXXC output is not the source'
call 'C:\ORXTOK.CMD' 'ok'
call check result = 'tokenized ok', 'a tokenized program runs'
'@attrib C:\ORXTOK*.CMD'
'@del C:\ORXTOKS.CMD'
'@del C:\ORXTOK.CMD'

/* REXXUTIL */
call rxfuncadd 'SysLoadFuncs', 'REXXUTIL', 'SysLoadFuncs'
call SysLoadFuncs
call check SysBootDrive() = 'C:', 'REXXUTIL SysBootDrive'
call SysFileTree 'C:\*.CMD', 'files.', 'FO'
call check files.0 > 0, 'REXXUTIL SysFileTree'
call check SysSearchPath('PATH', 'CMD.EXE') \= '', 'REXXUTIL SysSearchPath'

say 'ORXTEST:' passed 'passed,' failed 'failed'
exit failed

trapped:
  result = 'trapped'
  signal resume1

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

fill: procedure
  use arg arr, n
  do i = 1 to n
    arr[i] = 'item'i
  end
  return

usestem: procedure
  use arg st.
  st.2 = 'set by routine'
  return

/* RAISE leaves the routine and raises the condition in its caller */
catcher: procedure
  signal on user myerror name gotit
  call raiser
  return 'not caught'
gotit:
  return 'caught' condition('C')~substr(6)

raiser: procedure
  raise user myerror return

::requires 'orxreq.cmd'

::routine twice
  return arg(1) * 2

::class counter public
::method init class
  expose count
  count = 0
::method instances class
  expose count
  return count
::method new class
  expose count
  count = count + 1
  forward class (super)
::method value attribute
::method init
  expose value
  use arg value
::method step
  expose value
  value = value + 1

::class bigcounter subclass counter
::method step
  self~step:super
  self~value = self~value * 2 - 2

::class worker
::method compute
  use arg n
  return n * n
::method early
  reply 'replied'
  nop
