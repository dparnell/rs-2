/* t.cmd: Object REXX behaviour probe 2 */
nop
call probe 'x = .list~new; x~insert(1, 99)'
call probe 'x = .stem~new; say x~foo'
call probe 'call nosuchroutine'
call probe 'x = "abc"~nomethod'
call probe 'x = .array~new(2,2); say x[1]'
call probe 'say .array~of(1,2)[3]'
call probe 'say .directory~new~foo'
call probe 'x = .directory~new; say x["a"]'
call probe 'x = .table~new; say x[1]'
call probe 'x = .queue~new; say x~pull'
call probe 'say .list~of(5,6)~firstitem .list~of(5,6)~first'
call probe 'say "abc"~substr(2) "abc"~left(5, "*")'
call probe 'say "abc"~insert("X", 1) "abc"~pos("b")'
call probe 'say "a b c"~words "a"~copies(3)'
call probe 'say .object~new~hasmethod("STRING") .object~new~hasmethod("FOO")'
call probe 'say 3~max(5, 4) "  x "~strip'
call probe 'x = .bag~of(1,2,2); say x~items x[2]'
call probe 'x = .relation~new; x[1] = "a"; x[1] = "b"; say x~items x~allat(1)~items'
call probe 'x = .set~of("a","b","a"); say x~items'
call probe 'say .string~new("q")'
call probe 'say .method~new("m", "return 42")~class~id'
call probe 'say .message~new(3, "+", "I", 4)~send'
signal on syntax name s1
x = .object~new~nope
s1:
o = condition('O')
say o~code '|' o~errortext '|' o~message '|' o~position o~rc
say o~condition o~description '|' o~instruction
say o~traceback~class~id o~traceback~items
do i over o~traceback; say '  tb:' i; end
trace r
y = 1 + 2
z = .array~of(1)
trace o
say .local~class~id (.environment~items > 10)
say .stdout~class~id .output~class~id .input .stdin
say .local~hasindex('STDOUT') .local~hasindex('OUTPUT') .environment~hasindex('TRUE')
call t2
exit

t2: procedure
  q = .worker~new
  q~go
  return

probe2:
  signal on syntax name probe_err2
  call raiser
  say 'raiser returned'
  return
probe_err2:
  say 'probe2 trapped' condition('O')~code
  return

raiser:
  raise syntax 40.12 array (1, 2)
  say 'after raise'
  return

probe:
  signal on syntax name probe_err
  interpret arg(1)
  say 'no error:' arg(1)
  return
probe_err:
  o = condition('O')
  say arg(1) '=>' o~code '|' o~message
  return

::class counter
::class worker
::method go
  call deep 2
::routine deep
  use arg n
  if n = 0 then return 1 / 0
  call deep n - 1
