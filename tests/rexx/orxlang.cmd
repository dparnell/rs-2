/* ORXLANG.CMD: Object REXX's language and classes, beyond ORXTEST.CMD
   (rs2's own test; it passes on IBM's Object REXX 6.00 and on rs2's).
   Prints the failures and "ORXLANG: n passed, m failed"; ORXLANG V lists
   every check. */
parse arg opt .
verbose = translate(opt) = 'V'
passed = 0; failed = 0

/* ---- message syntax ---- */
l = .list~new~~insert('a')~~insert('b')
call check l~items = 2, '~~ gives the receiver'
call check 'abc'~'REVERSE' = 'cba', 'message name as a string'
call check 3~'+'(4) = 7 & 'ab'~'||'('cd') = 'abcd', 'operators as messages'
a = .array~new
a[2] = 'two'
call check a[2] = 'two' & a~at(2) = 'two', '[]= and []'
call check a~size = 2 & a~items = 1, 'array size and items'
call check a[1] = .nil, 'an empty array slot is .nil'
s = 'escape'
call check s~pos('e', 3) = 6 & s~lastpos('e') = 6, 'pos and lastpos on strings'
call check 'abc'~insert('X', 1) = 'aXbc' & 'abcdef'~overlay('XY', 2) = 'aXYdef', 'insert and overlay'
call check 'a-b-c'~countstr('-') = 2 & 'a b c'~wordpos('b') = 2, 'countstr and wordpos'
call check 'Abc'~left(5, '.') = 'Abc..' & 'abc'~center(7, '*') = '**abc**', 'left and center'
call check (2 + 3)~max(9, 4) = 9 & 2~min(5) = 2, 'max and min'
call check 'a'~copies(3)~length = 3, 'chained messages'
call check -(5)~abs = -5, 'prefix minus after the message'

/* ---- classes ---- */
p = .point~new(1, 2)
call check p~x = 1 & p~y = 2, 'attributes set by INIT'
p~x = 10
call check p~x = 10, 'attribute assignment'
call check p~string = '(10,2)', 'STRING method'
call check 'P=' || p = 'P=(10,2)', 'concatenation uses STRING'
c = .point3d~new(1, 2, 3)
call check c~string = '(1,2,3)', 'subclass with super calls'
call check c~isA = 'Point3D', 'class method called on the instance class'
call check .point3d~superclasses[1] = .point, 'superclass'
call check .point~subclasses~items >= 1, 'subclasses'
call check .point~id = 'POINT' & .point3d~id = 'Point3D', 'class ids'
call check .point~defaultname = 'The POINT class', 'class default name'
call check p~class = .point & c~class = .point3d, 'class of an instance'
call check p~hasMethod('X=') & \p~hasMethod('Z'), 'hasMethod for attributes'
call check .point~instances = 1 & .point3d~instances = 1, 'class variables of each class object'
c~objectname = 'my point'
call check c~objectname = 'my point' & c~defaultname = 'a Point3D', 'objectname and defaultname'
q = p~copy
q~x = 99
call check p~x = 10 & q~x = 99, 'copy has its own variables'
call check .redpoint~new(1, 1)~describe = 'red (1,1)', 'mixin class inherited'
call check .point3d~queryMixinClass = 0 & .colored~queryMixinClass = 1, 'queryMixinClass'
call check .secretive~new~reveal = 'secret', 'private method from inside'
signal on syntax name priv
x = .secretive~new~hidden
call check 0, 'private method from outside'
priv:
signal off syntax
call check condition('C') = 'SYNTAX' & rc = 97, 'private method from outside'

/* methods made at run time */
o = .settable~new
o~add('GREET', 'return "hi" arg(1)')
call check o~greet('you') = 'hi you', 'setMethod'
o~remove('GREET')
call check \o~hasMethod('GREET'), 'unsetMethod'
signal on syntax name priv2
.object~new~setMethod('X', 'return 1')
call check 0, 'setMethod is private'
priv2:
signal off syntax
call check rc = 97, 'setMethod is private'
.point~define('NORM', 'expose x y; return x + y')
call check .point~new(3, 4)~norm = 7, 'define'
.point~delete('NORM')
call check \.point~new(1, 1)~hasMethod('NORM'), 'delete'
m = .method~new('double', 'return arg(1) * 2')
call check m~source[1] = 'return arg(1) * 2', 'method source'
e = .object~enhanced(.directory~new~~setentry('SHOUT', 'return "HEY"'))
call check e~shout = 'HEY', 'enhanced'
call check .object~new~request('STRING') = .nil, 'request of a string'
call check 12~request('STRING') = 12, 'request of a string from a string'
k = .object~subclass('Kid')
call check k~id = 'Kid' & k~new~class = k, 'subclass made at run time'

/* UNKNOWN and NOMETHOD */
u = .catchall~new
call check u~anything(1, 2) = 'ANYTHING 2', 'UNKNOWN method'
signal on nomethod name nometh
x = .object~new~nosuchmethod
call check 0, 'NOMETHOD condition'
nometh:
call check condition('C') = 'NOMETHOD' & condition('D') = 'NOSUCHMETHOD', 'NOMETHOD condition'

/* ---- collections ---- */
a = .array~new(2, 3)
a[2, 3] = 'x'
call check a[2, 3] = 'x' & a~dimension = 2 & a~dimension(2) = 3, 'two-dimensional array'
a = .array~of('a', 'b', 'c', 'd')
call check a~section(2, 2)~items = 2 & a~section(2, 2)[1] = 'b', 'array section'
call check a~first = 1 & a~last = 4 & a~next(1) = 2 & a~previous(1) = .nil, 'array first last next previous'
a~remove(2)
call check a~items = 3 & a~makearray~items = 3, 'array remove and makearray'
s = a~supplier
txt = ''
do while s~available
  txt = txt || s~index || s~item
  s~next
end
call check txt = '1a3c4d', 'array supplier'
l = .list~of('b')
i1 = l~insert('a', .nil)
i2 = l~insert('c')
call check l~firstitem = 'a' & l~lastitem = 'c' & l[i2] = 'c', 'list insert at the front and the end'
call check l~next(l~first) \= .nil & l~previous(l~first) = .nil, 'list next and previous'
call check l~remove(i1) = 'a' & l~items = 2, 'list remove'
q = .queue~new~~queue(1)~~queue(2)~~queue(3)
call check q~peek = 1 & q~items = 3 & q[2] = 2, 'queue peek, items, []'
q~push(0)
call check q~pull = 0 & q~pull = 1, 'queue push and pull'
d = .directory~new
d~setentry('size', 5)
call check d~entry('SIZE') = 5 & d~size = 5 & d['SIZE'] = 5, 'directory setentry, entry, unknown'
d~setmethod('TWICE', 'return self~size * 2')
call check d~twice = 10, 'directory setmethod'
call check d~hasentry('twice') & \d~hasindex('nothing'), 'directory hasentry and hasindex'
d['lower'] = 1
call check d~hasindex('lower') & \d~hasindex('LOWER'), 'directory [] keeps case'
call check d~remove('lower') = 1 & d~items = 2, 'directory remove'
t1 = .table~new; t1['a'] = 1; t1['b'] = 2
t2 = .table~new; t2['b'] = 3; t2['c'] = 4
call check t1~union(t2)~items = 3 & t1~intersection(t2)~items = 1, 'table union and intersection'
call check t1~xor(t2)~items = 2 & t1~difference(t2)~items = 1, 'table xor and difference'
call check t1~subset(t1~union(t2)) & \t1~union(t2)~subset(t1), 'table subset'
key = .object~new
t1[key] = 'obj'
call check t1[key] = 'obj' & t1~hasindex(key), 'table with an object index'
r = .relation~new
r['k'] = 'a'; r['k'] = 'b'; r['j'] = 'a'
call check r~items = 3 & r~allIndex('a')~items = 2, 'relation allIndex'
call check r~hasItem('b', 'k') & \r~hasItem('b', 'j'), 'relation hasItem'
call check r~removeItem('a', 'k') = 'a' & r~items('k') = 1, 'relation removeItem and items(index)'
st = .set~of('x', 'y')
st~put('x')
call check st~items = 2 & st~hasindex('y'), 'set put of an existing item'
bg = .bag~of('x', 'y')
bg~put('x')
call check bg~items = 3, 'bag put of an existing item'
s.1 = 'one'; s.2 = 'two'
n = 0
do t over s.
  n = n + 1
end
call check n = 2, 'do over a stem'
st = .stem~new('X.')
st[1] = 'a'
call check st[1] = 'a' & st[2] = 'X.2', 'stem object with a name'
st[] = 'dflt'
call check st[7] = 'dflt', 'stem object default value'
txt = ''
do i over .array~of(1, 2, 3, 4)
  if i = 2 then iterate
  if i = 4 then leave
  txt = txt || i
end
call check txt = '13', 'do over with iterate and leave'

/* ---- conditions ---- */
call check trapit() = 'USER WHOOPS|more|1 2', 'raise with description and array'
call check prop() = 'SYNTAX 42', 'raise propagate'
call check methodsyntax() = 'trapped 42 in caller', 'syntax error in a method trapped by the caller'
x = .thrower~new
call check x~safe = 'no error', 'raise in a method with return'
call on user ping name pinged
pinged = 0
call raisep
call check pinged = 1, 'call on user condition'
signal on syntax name s2
call r93
s2:
call check condition('O')~code = '93.903' & condition('O')~rc = 93, 'raise syntax with a subcode'
signal off syntax

/* ---- messages and concurrency ---- */
w = .worker~new
msg = .message~new(w, 'ADD', 'I', 2, 3)
call check msg~completed = 0, 'message not yet sent'
msg~start
call check msg~result = 5 & msg~completed = 1, 'message start and result'
m2 = .message~new(w, 'ADD', 'A', .array~of(4, 5))
call check m2~send = 9, 'message with an array of arguments'
call check w~reply2 = 'early', 'reply before the method ends'
call check w~forwarded(6) = 36, 'forward to another message'
call check w~continued = 'continued 49', 'forward continue'
b = .buffer~new
m = b~start('TAKE')
b~put('item')
call check m~result = 'item', 'start waits in GUARD WHEN for another activity'
t = .ticker~new
call check t~go = 'going', 'reply gives the result early'
do i = 1 to 100000 until t~count >= 3
end
call check t~count = 3, 'the method goes on after reply'
.local~orxlang.alarm = 0
a = .alarm~new(1, .message~new(.ticker, 'RING'))
call time 'R'
do until .orxlang.alarm = 1 | time('E') > 5
end
call check .orxlang.alarm = 1, 'an alarm goes off'
a2 = .alarm~new(60, .message~new(.ticker, 'RING'))
a2~cancel
call check 1, 'an alarm cancelled'

/* ---- the environment ---- */
.local~setentry('orxlang.thing', 'here')
call check .orxlang.thing = 'here', '.local entry by environment symbol'
call check value('ORXLANG.THING', , '') = '.ORXLANG.THING', 'value of the global environment'
call value 'ORXLANG.ENV', 'set', ''
call check .orxlang.env = 'set', 'value sets the global environment'
call check .environment~orxlang.env = 'set', '.environment'
call check .methods = '.METHODS', '.methods without floating methods'
call check .stdin~class = .stream & .output~class = .monitor, '.stdin and .output'
call check .nothing = '.NOTHING', 'unknown environment symbol'
call check arg() = 1 | arg() = 0, 'arg() of the program'
call check objarg(.array~of(1)) = 'an Array', 'arg(n) gives the object'
call check useargs(, 'b') = 'dropped b', 'use arg with an omitted argument'

/* ---- streams ---- */
f = .stream~new('ORXLANG.TMP')
call check f~open('write replace') = 'READY:', 'stream open'
f~arrayout(.array~of('one', 'two', 'three'))
call check f~close = 'READY:', 'stream close'
call check f~lines > 0 & f~linein = 'one', 'stream lines and linein'
call check f~supplier~item = 'one', 'stream supplier'
f~close
call check f~query('exists') \= '' & f~query('size') > 0, 'stream query'
sm = .stream~new('ORXLANG.TMP')
call check sm~makearray~items = 3, 'stream makearray'
call check sm~close = 'READY:', 'stream close after makearray'
'@del ORXLANG.TMP'


/* ---- UNINIT, .OUTPUT/.INPUT/.ERROR, REXX.IMG classes ---- */
call lineout 'ORXUNI.CMD', '/* */'
call lineout 'ORXUNI.CMD', 'o = .res~new; drop o; p = .res~new'
call lineout 'ORXUNI.CMD', '::class res'
call lineout 'ORXUNI.CMD', '::method uninit'
call lineout 'ORXUNI.CMD', "  say 'uninit'"
call lineout 'ORXUNI.CMD'
'@cmd /c orxuni.cmd > ORXUNI.TMP'
if stream('ORXUNI.TMP', 'c', 'query exists') \= '' then call check linein('ORXUNI.TMP') = 'uninit' & linein('ORXUNI.TMP') = 'uninit', 'UNINIT (dropped and at the end)'
call stream 'ORXUNI.TMP', 'c', 'close'
'@del ORXUNI.CMD ORXUNI.TMP'
log = .recorder~new
.local['OUTPUT'] = log
say 'to the log'
call lineout , 'lineout'
call charout , 'charout'
.local['OUTPUT'] = .stdout
call check log~got = ' SAY(to the log) LINEOUT(lineout) CHAROUT(charout)', 'SAY and LINEOUT to .OUTPUT'
log = .recorder~new
.local['ERROR'] = log
trace r
tv = 5
trace o
.local['ERROR'] = .stderr
call check log~got~pos('LINEOUT(') > 0 & log~got~pos('tv = 5') > 0, 'trace output to .ERROR'
log = .recorder~new
.local['INPUT'] = log
if queued() = 0 then pull pv
parse linein lv
.local['INPUT'] = .stdin
call check pv = 'INLINEIN' & lv = 'inLINEIN', 'PULL and PARSE LINEIN from .INPUT'
mon = .monitor~new(.stdout)
r1 = mon~destination(log)
mon~say('via')
call check log~got~pos('SAY(via)') > 0 & mon~destination == .stdout & mon~destination == .nil, 'monitor destinations pushed and popped'
call check .stdout~say('') = 0, 'stream SAY method'
call check .stdque~get = 'SESSION' & .stdque~string = 'SESSION' & .rx_queue~id = 'RX_QUEUE', '.STDQUE and .RX_QUEUE'
call check .stdque~queue('q1') = 0 & .stdque~push('q0') = 0 & .stdque~query = 2, 'RX_QUEUE queue, push and query'
call check .stdque~pull = 'q0' & .stdque~linein = 'q1' & .stdque~pull = .nil, 'RX_QUEUE pull and linein'
call check .rx_queue~new~get = 'NAMED_QUEUE', 'RX_QUEUE without a name'
call check .system~version = '2.30' & .system~name = 'OS/2', '.SYSTEM'
call check .stream~new('C:\CONFIG.SYS')~supplier~class = .stream_supplier, 'stream supplier class'
say 'ORXLANG:' passed 'passed,' failed 'failed'
exit failed

check:
  parse arg ok, what
  if ok == 1 then do
    passed = passed + 1
    if verbose then say 'ok' what
  end
  else do
    failed = failed + 1
    say 'FAIL' what
  end
  return

trapit: procedure
  signal on user whoops name t1
  call raiser1
  return 'not trapped'
t1:
  o = condition('O')
  return condition('C') || '|' || condition('D') || '|' || o~additional[1] o~additional[2]

raiser1:
  raise user whoops description 'more' array (1, 2) return

prop: procedure
  signal on syntax name p1
  call prop2
  return 'not trapped'
p1:
  return condition('C') rc


methodsyntax: procedure
  signal on syntax name ms
  .thrower~new~divide
  return 'not trapped'
ms:
  return 'trapped' rc 'in caller'

raisep:
  raise user ping return

pinged:
  pinged = 1
  return

objarg: procedure
  return arg(1)~string

r93:
  raise syntax 93.903 array (2) return

useargs: procedure
  use arg a, b
  if symbol('A') = 'LIT' then return 'dropped' b
  return 'set' b

::routine prop2
  signal on syntax name p2
  x = 1 / 0
  return 'no'
p2:
  raise propagate

::class point public
::method init class
  expose count
  count = 0
::method new class
  expose count
  count = count + 1
  forward class (super)
::method instances class
  expose count
  return count
::method x attribute
::method y attribute
::method init
  expose x y
  use arg x, y
::method string
  expose x y
  return '(' || x || ',' || y || ')'

::class 'Point3D' subclass point public
::method init
  expose z
  use arg x, y, z
  self~init:super(x, y)
::method string
  expose z
  s = self~string:super
  return left(s, length(s) - 1)','z')'
::method isA
  return self~class~id

::class colored mixinclass object
::method describe
  return 'red' self~string

::class redpoint subclass point inherit colored

::class secretive
::method hidden private
  return 'secret'
::method reveal
  return self~hidden

::class settable
::method add
  use arg n, s
  self~setMethod(n, s)
::method remove
  self~unsetMethod(arg(1))

::class catchall
::method unknown
  use arg name, args
  return name args~items

::class thrower
::method divide
  return 1 / 0
::method safe
  signal on user oops name caught
  self~raiser
  return 'not caught'
caught:
  return 'no error'
::method raiser
  raise user oops return

::class buffer
::method init
  expose item
  item = .nil
::method put
  expose item
  use arg item
::method take
  expose item
  guard on when item \= .nil
  return item

::class ticker
::method ring class
  .local~orxlang.alarm = 1
::method init
  expose n
  n = 0
::method count unguarded
  expose n
  return n
::method go
  expose n
  reply 'going'
  guard off
  do 3
    n = n + 1
  end

::class worker
::method add
  use arg a, b
  return a + b
::method reply2
  reply 'early'
  x = 1
::method forwarded
  forward message ('SQUARE')
::method square
  use arg n
  return n * n
::method continued
  forward message ('SQUARE') array (7) continue
  return 'continued' result

::class recorder
::method got attribute
::method init; self~got = ''
::method unknown
  use arg m, a
  if a~items > 0 then self~got = self~got m'('a[1]')'
  else self~got = self~got m'()'
  return 'in'm
