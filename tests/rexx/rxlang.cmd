/* RXLANG.CMD: classic REXX (OS/2 REXX, REXXSAA 4.00) language checks.
   Prints one line per failed check and "RXLANG: n passed, m failed"
   at the end; "RXLANG v" lists every check. Runs on IBM's REXX.DLL and
   on rs2's. Needs RXLANG2.CMD (an external routine) next to it. */
parse arg opt
verbose = translate(opt) = 'V'
passed = 0; failed = 0

/* ---- arithmetic and number formatting ---- */
call check 1/3, '0.333333333', 'division'
call check 2/3, '0.666666667', 'division rounds'
call check 10/4, '2.5', 'division trailing zeros'
call check 1.50+1, '2.50', 'addition keeps decimals'
call check 2**-1, '0.5', 'negative power'
call check 0.1*3, '0.3', 'multiplication'
call check 1e9*10, '1.0E+10', 'exponential notation'
call check 123456789*10, '1.23456789E+9', 'too many digits'
call check 1/7*7, '1.00000000', 'rounding of operands'
call check 12345678901+0, '1.23456789E+10', 'rounding to digits'
call check 0.0000001+0, '0.0000001', 'small numbers plain'
call check 1e-10+0, '0.0000000001', 'small numbers plain 2'
call check '-0'+0, '0', 'minus zero'
call check ' 3 '+0, '3', 'blanks around numbers'
call check ' - 3'+0, '-3', 'blank after sign'
call check 5//-2, '1', 'remainder'
call check -5//2, '-1', 'remainder sign'
call check 7%2 -7%2, '0', 'integer division'
call check (7%2) (-7%2), '3 -3', 'integer division truncates'
call check 2**31, '2.14748365E+9', 'power'
call check 1.1**3, '1.331', 'power of decimal'
call check 2**0, '1', 'power zero'
call check 1.0**2, '1', 'power drops trailing zeros'
call check -2**2, '4', 'prefix minus binds tightest'
call check 2**3**2, '64', 'power is left associative'
call check 1+2*3, '7', 'precedence'
call check (1+2)*3, '9', 'parentheses'
call check 3-+-2, '5', 'prefix operators'
numeric digits 20
call check 1/3, '0.33333333333333333333', 'numeric digits 20'
call check 2**64, '18446744073709551616', 'big power'
numeric digits
call check digits(), '9', 'numeric digits reset'
numeric digits 5
call check 123456+0, '1.2346E+5', 'digits 5'
numeric digits 9
numeric form engineering
call check 1e9*10, '10E+9', 'engineering'
call check 12345678901+0, '12.3456789E+9', 'engineering 2'
numeric form scientific
call check form(), 'SCIENTIFIC', 'form()'
numeric fuzz 1
call check 1.00000001 = 1, '1', 'fuzz'
call check fuzz(), '1', 'fuzz()'
numeric fuzz 0
call check 1.00000001 = 1, '0', 'nine digits compare exactly'
call check 1.0000001 = 1, '0', 'no fuzz'

/* ---- comparisons ---- */
call check '1.0' = 1, '1', 'numeric equality'
call check 'a' = ' a ', '1', 'blanks ignored'
call check 'a' == ' a ', '0', 'strict equality'
call check 'abc' < 'abd', '1', 'string comparison'
call check 'ab' < 'ab ', '0', 'padding with blanks'
call check 'ab' << 'ab ', '1', 'strict comparison'
call check 10 > 9, '1', 'numeric comparison'
call check '10' >> '9', '0', 'strict is not numeric'
call check 1 \= 2, '1', 'not equal'
call check 1 <> 2 & 2 >< 3, '1', 'other not equal forms'
call check \0, '1', 'not'
call check 1 & 0 | 1, '1', 'and/or'
call check 1 && 1, '0', 'exclusive or'
call check 'abc' >= 'abc' & 'abc' <= 'abc', '1', 'ge/le'
call check 'a' \== 'b' & 'a' \<< 'a' & 'b' \>> 'a', '0', 'strict negations'

/* ---- concatenation ---- */
a = 'x'; c = 'y'
call check a c, 'x y', 'blank concatenation'
call check a||c, 'xy', 'explicit concatenation'
call check a'z', 'xz', 'abuttal'
call check (1)(2), '12', 'abuttal of parentheses'

/* ---- strings ---- */
call check "it's", "it's", 'double quotes'
call check 'it''s', "it's", 'doubled quote'
call check '41 42'x, 'AB', 'hex string'
call check '0100 0001'b, 'A', 'binary string'
call check ''x, '', 'empty hex string'
call check length(''), '0', 'empty string'

/* ---- built-in functions ---- */
call check abbrev('PRINT', 'PRI'), '1', 'abbrev'
call check abbrev('PRINT', 'PRX'), '0', 'abbrev 2'
call check abbrev('PRINT', '', 1), '0', 'abbrev 3'
call check abs(-2.50), '2.50', 'abs'
call check bitand('ab', '  '), '  ', 'bitand'
call check bitor('a', ' '), 'a', 'bitor'
call check bitxor('A', ' '), 'a', 'bitxor'
call check bitand('ab',, 'ff'x), 'ab', 'bitand pad'
call check b2x('1111 0000'), 'F0', 'b2x'
call check center('ab', 6, '-'), '--ab--', 'center'
call check centre('abcdef', 3), 'bcd', 'centre truncates'
call check compare('abc', 'abd'), '3', 'compare'
call check compare('abc', 'abc'), '0', 'compare equal'
call check compare('ab ', 'ab'), '0', 'compare pads'
call check copies('ab', 3), 'ababab', 'copies'
call check c2d('A'), '65', 'c2d'
call check c2d('81'x, 1), '-127', 'c2d signed'
call check c2d('0081'x, 2), '129', 'c2d 2 bytes'
call check c2x('AB'), '4142', 'c2x'
call check datatype('12'), 'NUM', 'datatype num'
call check datatype('12a'), 'CHAR', 'datatype char'
call check datatype(' 1e5 ', 'N'), '1', 'datatype N'
call check datatype('ab', 'X'), '1', 'datatype X'
call check datatype('1.5', 'W'), '0', 'datatype W'
call check datatype('aB', 'M'), '1', 'datatype M'
call check datatype('ab', 'L') datatype('AB', 'U') datatype('a1', 'A') datatype('a.b', 'S') datatype('101', 'B'), '1 1 1 1 1', 'datatype others'
call check delstr('abcdef', 2, 3), 'aef', 'delstr'
call check delstr('abcdef', 3), 'ab', 'delstr to end'
call check delword('a b c d', 2, 2), 'a d', 'delword'
call check delword('a b c d', 2), 'a ', 'delword to end'
call check d2c(65), 'A', 'd2c'
call check d2c(-1, 2), 'FFFF'x, 'd2c negative'
call check d2x(255), 'FF', 'd2x'
call check d2x(-1, 4), 'FFFF', 'd2x negative'
call check d2x(0), '0', 'd2x zero'
call check errortext(0), '', 'errortext 0'
call check format(3.14159,,2), '3.14', 'format decimals'
call check format(12345.678,,,3), '12345.678', 'format expp'
call check format(1.5e10), '1.5E+10', 'format exponential'
call check format(0.000123,,,,2), '1.23E-4', 'format expt'
call check format(2, 3), '  2', 'format before'
call check format(-2.5, 3, 2), ' -2.50', 'format before and after'
call check format(1.996, , 2), '2.00', 'format rounds'
call check insert('x', 'abc', 1), 'axbc', 'insert'
call check insert('x', 'abc', 5, 2, '.'), 'abc..x.', 'insert pad'
call check lastpos('b', 'abcb'), '4', 'lastpos'
call check lastpos('b', 'abcb', 3), '2', 'lastpos start'
call check left('ab', 3, '*'), 'ab*', 'left'
call check length('abc'), '3', 'length'
call check max(1, 5, 3), '5', 'max'
call check min(2, -1), '-1', 'min'
call check max(1.0, 1), '1.0', 'max keeps first of equal'
call check overlay('x', 'abc', 2), 'axc', 'overlay'
call check overlay('xyz', 'ab', 4), 'ab xyz', 'overlay pads'
call check pos('b', 'abcb'), '2', 'pos'
call check pos('b', 'abcb', 3), '4', 'pos start'
call check pos('', 'abc'), '0', 'pos empty needle'
call check reverse('xyz'), 'zyx', 'reverse'
call check right('abc', 2), 'bc', 'right'
call check right('a', 3, '0'), '00a', 'right pad'
call check sign(-3) sign(0) sign(2.5), '-1 0 1', 'sign'
call check space(' a  b '), 'a b', 'space'
call check space(' a  b ', 2, '-'), 'a--b', 'space pad'
call check space('a b', 0), 'ab', 'space 0'
call check strip('  a  '), 'a', 'strip'
call check strip('xxaxx',, 'x'), 'a', 'strip char'
call check strip('  a  ', 'l'), 'a  ', 'strip leading'
call check strip('  a  ', 'T'), '  a', 'strip trailing'
call check substr('abc', 2), 'bc', 'substr'
call check substr('abc', 2, 4, '.'), 'bc..', 'substr pad'
call check substr('abc', 5, 2), '  ', 'substr past end'
call check subword('a b c d', 2, 2), 'b c', 'subword'
call check subword(' a  b  c ', 2), 'b  c', 'subword to end'
call check translate('abc'), 'ABC', 'translate upper'
call check translate('abc', 'xy', 'ab'), 'xyc', 'translate tables'
call check translate('abc', 'x', 'abc', '-'), 'x--', 'translate pad'
call check trunc(12.345, 2), '12.34', 'trunc'
call check trunc(12.3), '12', 'trunc 0'
call check trunc(-1.5), '-1', 'trunc negative'
call check trunc(1, 2), '1.00', 'trunc pads'
call check verify('abc', 'ab'), '3', 'verify'
call check verify('abc', 'c', 'M'), '3', 'verify match'
call check verify('abc', 'abc'), '0', 'verify all'
call check word('a b c', 2), 'b', 'word'
call check word('a b c', 4), '', 'word past end'
call check wordindex('a  b', 2), '4', 'wordindex'
call check wordlength('a bcd', 2), '3', 'wordlength'
call check wordpos('b c', 'a b c'), '2', 'wordpos'
call check wordpos('x', 'a b c'), '0', 'wordpos missing'
call check words(' a b  c '), '3', 'words'
call check xrange('a', 'e'), 'abcde', 'xrange'
call check length(xrange()), '256', 'xrange all'
call check x2b('0f'), '00001111', 'x2b'
call check x2c('4142'), 'AB', 'x2c'
call check x2c('1'), '01'x, 'x2c odd'
call check x2d('ff'), '255', 'x2d'
call check x2d('81', 2), '-127', 'x2d signed'
call check x2d('F', 1), '-1', 'x2d one digit'
call check filespec('d', 'C:\x\y.z') filespec('p', 'C:\x\y.z') filespec('n', 'C:\x\y.z'), 'C: \x\ y.z', 'filespec'
call check datatype(random()), 'NUM', 'random'
call check random(5, 5), '5', 'random range'
r1 = random(1, 100, 7); r2 = random(1, 100, 7)
call check r1 = r2, '1', 'random seed repeats'
call check datatype(time('S'), 'W'), '1', 'time S'
call check length(time()), '8', 'time N'
call check length(time('L')), '15', 'time L'
call check datatype(date('B'), 'W'), '1', 'date B'
call check length(date('S')), '8', 'date S'
call check date('W') = word('Monday Tuesday Wednesday Thursday Friday Saturday Sunday', date('B') // 7 + 1), '1', 'date W'
call check date('M') = word('January February March April May June July August September October November December', substr(date('S'), 5, 2)), '1', 'date M'
call check time('E') <> '', '1', 'time E'
call check symbol('zzz') symbol('1a') symbol('a+'), 'LIT LIT BAD', 'symbol'
q = 1
call check symbol('q'), 'VAR', 'symbol var'
call check value('q'), '1', 'value'
call check value('q', 2) q, '1 2', 'value sets'
call check address(), 'CMD', 'address()'
call check sourceline() > 10, '1', 'sourceline count'
call check sourceline(1), '/* RXLANG.CMD: classic REXX (OS/2 REXX, REXXSAA 4.00) language checks.', 'sourceline'
call check queued() >= 0, '1', 'queued'

/* ---- variables and stems ---- */
s. = 'd'
s.1 = 'one'
k = 1
call check s.k s.2, 'one d', 'stem default'
drop s.1
call check s.1, 'S.1', 'drop a tail with a default'
drop s.
call check s.1, 'S.1', 'dropped stem'
t.a.c = 5
call check t.x.y, 'T.X.Y', 'compound tails substituted'
name = 'k'
call check value(name), '1', 'value indirect'
drop k
call check k, 'K', 'dropped variable'
list = 'v1 v2'; v1 = 1; v2 = 2
drop (list)
call check v1 v2, 'V1 V2', 'drop (list)'
call check symbol('list'), 'VAR', 'drop (list) keeps list'

/* ---- control structures ---- */
out = ''
do i = 1 to 3; out = out i; end
call check out i, ' 1 2 3 4', 'do to'
out = ''
do i = 3 to 1 by -1; out = out i; end
call check out, ' 3 2 1', 'do by -1'
out = ''
do i = 1 to 10 by 3 for 2; out = out i; end
call check out, ' 1 4', 'do for'
out = ''
do 3; out = out'x'; end
call check out, 'xxx', 'do count'
n = 0
do forever; n = n + 1; if n > 4 then leave; end
call check n, '5', 'do forever leave'
out = ''
do i = 1 to 6; if i // 2 then iterate; out = out i; end
call check out, ' 2 4 6', 'iterate'
out = ''
do i = 1 while i < 4; out = out i; end
call check out, ' 1 2 3', 'do while'
out = ''
do i = 1 until i >= 3; out = out i; end
call check out, ' 1 2 3', 'do until'
out = ''
do i = 1 to 3; do j = 1 to 3; if j = 2 then iterate i; if i = 3 then leave i; out = out i||j; end; end
call check out, ' 11 21', 'named leave and iterate'
out = ''
do i = 1 to 2; i = i + 1; out = out i; end
call check out, ' 2', 'control variable changed in loop'
do i = 1.5 to 3; end
call check i, '3.5', 'decimal control variable'
x1 = 2
select
  when x1 = 1 then r = 'a'
  when x1 = 2 then r = 'b'
  otherwise r = 'c'
end
call check r, 'b', 'select'
select; when 0 then nop; otherwise r = 'o'; r = r'2'; end
call check r, 'o2', 'otherwise with several instructions'
if 1 then if 0 then r = 'x'; else r = 'inner else'
call check r, 'inner else', 'else binds to the inner if'
if 0 then nop
else do; r = 'do'; end
call check r, 'do', 'else do'

/* ---- routines ---- */
call check fact(10), '3628800', 'recursion'
call sub1 'a', , 'c'
call check result, '3 0 1 a c', 'call with omitted argument'
call check sub1(), '0 0 1  ', 'function without arguments'
g = 'global'
call check exposer(), 'global seen', 'procedure expose'
call check g, 'changed', 'exposed variable changed'
call check hidden(), 'LIT', 'procedure hides'
stem.1 = 'x'
call check exposestem(), 'x', 'expose a stem'
call check stem.2, 'set', 'exposed stem set'
lst = 'g'
call check exposelist(), 'changed', 'expose (list)'
call nores
call check symbol('result'), 'LIT', 'call without result drops RESULT'
call check rxlang2(21), '42', 'external routine'
call check rxlang2('src'), 'OS/2 FUNCTION', 'parse source of an external routine'
call rxlang2 'sub'
call check result, 'SUBROUTINE', 'external called as subroutine'
call check 'LENGTH'('abc'), '3', 'quoted function name'
call check sigl > 0, '1', 'sigl after call'

/* ---- parse ---- */
parse value 'a b c' with p1 p2 . 1 p3 +1
call check p1'|'p2'|'p3, 'a|b|a', 'parse words, placeholder, positions'
parse value '  a   b  c ' with p1 p2
call check '['p1']['p2']', '[a][  b  c ]', 'parse remainder'
parse value '  a  ' with p1
call check '['p1']', '[  a  ]', 'parse single target keeps blanks'
parse value 'k=v;z' with p1 '=' p2 ';' p3
call check p1 p2 p3, 'k v z', 'parse patterns'
str = 'abcdef'
parse var str 2 p1 +2 p2 =6 p3
call check p1'|'p2'|'p3, 'bc|de|f', 'parse positional'
parse var str p1 3 p2 1 p3
call check p1'|'p2'|'p3, 'ab|cdef|abcdef', 'parse backwards position'
d = ','
parse value 'a,b' with p1 (d) p2
call check p1 p2, 'a b', 'parse variable pattern'
parse value 'abc' with p1 'x' p2
call check p1'|'p2, 'abc|', 'parse pattern not found'
parse upper value 'Mixed' with p1
call check p1, 'MIXED', 'parse upper'
call two 'x y', 'z'
parse version v
call check word(v, 1), 'REXXSAA', 'parse version'
parse source s
call check word(s, 1) word(s, 2), 'OS/2 COMMAND', 'parse source'
p = 3
parse value 'abcdef' with 2 p1 +(p)
call check p1, 'bcd', 'parse +(variable)'

/* ---- the external data queue ---- */
queue 'q1'; push 'q0'; queue 'q2'
call check queued(), '3', 'queued'
pull p1; parse pull p2; pull p3
call check p1 p2 p3, 'Q0 q1 Q2', 'pull and parse pull'
newq = rxqueue('Create')
call check length(newq) > 0, '1', 'rxqueue create'
old = rxqueue('Set', newq)
queue 'in new'
call check queued(), '1', 'queue of the new queue'
parse pull p1
call check p1, 'in new', 'pull from the new queue'
call check rxqueue('Get'), translate(newq), 'rxqueue get'
call rxqueue 'Set', old
call check rxqueue('Delete', newq), '0', 'rxqueue delete'
call check rxqueue('Delete', newq), '9', 'rxqueue delete again'
queue 'via linein'
call check linein('QUEUE:'), 'via linein', 'linein queue'

/* ---- streams ---- */
f = 'RXLANG.TMP'
call SysFileDeleteIfAny f
call check lineout(f, 'one'), '0', 'lineout'
call lineout f, 'two'
call check stream(f, 'c', 'query size'), '10', 'query size'
call lineout f
call check stream(f, 's'), 'UNKNOWN', 'closed stream'
call check lines(f), '1', 'lines'
call check linein(f), 'one', 'linein'
call check linein(f), 'two', 'linein 2'
call check lines(f), '0', 'lines at end'
call check linein(f, 1), 'one', 'linein from line 1'
call check charin(f, 1, 3), 'one', 'charin'
call check chars(f), '7', 'chars'
call check charin(f, 6), 't', 'charin start'
call stream f, 'c', 'close'
call check charout(f, 'X', 1), '0', 'charout at position'
call stream f, 'c', 'close'
call check charin(f, 1, 3), 'Xne', 'charout overwrote'
call check stream(f, 'c', 'close'), 'READY:', 'stream close'
call check stream(f, 'c', 'open read'), 'READY:', 'stream open read'
call check stream(f, 'c', 'seek =6'), '6', 'stream seek'
call check linein(f), 'two', 'linein after seek'
call stream f, 'c', 'close'
call check translate(stream(f, 'c', 'query exists')) = translate(directory()'\'f) | translate(stream(f, 'c', 'query exists')) = translate(directory()f), '1', 'query exists'
call check length(stream(f, 'c', 'query datetime')), '18', 'query datetime'
call check stream('NOSUCH.FIL', 'c', 'query exists'), '', 'query exists missing'
'@del' f
call check stream(f, 'c', 'query exists'), '', 'deleted'
signal on notready name nr
call linein 'NOSUCH.FIL'
call check 'no', 'notready', 'notready raised'
nrdone:
signal off notready

/* ---- conditions ---- */
signal on syntax name syn1
interpret 'x2 = 1 +'
call check 'no', 'trapped', 'syntax in interpret'
syn1:
call check rc, '35', 'syntax rc'
call check condition('C') condition('I'), 'SYNTAX SIGNAL', 'condition()'
signal on syntax name syn2
x3 = 'a' + 1
syn2:
call check rc sigl, '41' (sigl), 'bad arithmetic'
signal on novalue name nov
x4 = undefinedvar
nov:
call check condition('D'), 'UNDEFINEDVAR', 'novalue description'
signal off novalue
call on error name err1
'@nosuchcommand_rxlang >nul 2>nul'
call check errseen, 'yes', 'call on error'
call check rc <> 0, '1', 'rc of a failing command'
call off error
'@echo off'
call check rc, '0', 'rc of a command'
address nosuchenv 'anything'
call check rc, '30', 'unknown environment'
call check condition('S'), 'OFF', 'condition state'

/* ---- interpret, value, misc ---- */
interpret 'y1 = 6 * 7'
call check y1, '42', 'interpret'
call check value('PATH',, 'OS2ENVIRONMENT') <> '', '1', 'environment'
call value 'RXLANGVAR', 'set', 'OS2ENVIRONMENT'
call check value('RXLANGVAR',, 'OS2ENVIRONMENT'), 'set', 'set environment'
'@set RXLANGCMD=fromcmd'
call check value('RXLANGCMD',, 'OS2ENVIRONMENT'), 'fromcmd', 'environment set by a command'
'@echo %RXLANGVAR%| rxqueue'
parse pull p1
call check strip(p1), 'set', 'environment seen by a command'
call check trace(), 'N', 'trace()'
call check trace('o'), 'N', 'trace() returns the old setting'
call check trace(), 'O', 'trace o'
trace n
call check rxfuncquery('SysLoadFuncs'), rxfuncquery('SysLoadFuncs'), 'rxfuncquery'
call check rxfuncadd('SysLoadFuncs', 'REXXUTIL', 'SysLoadFuncs') < 20, '1', 'rxfuncadd'
call check rxfuncquery('SysLoadFuncs'), '0', 'rxfuncquery registered'
call check rxfuncquery('NoSuchFunctionX'), '1', 'rxfuncquery not registered'
signal skip
call check 'no', 'skipped', 'signal skips'
skip:
call check sigl > 0, '1', 'signal sets sigl'

say 'RXLANG:' passed 'passed,' failed 'failed'
exit failed

check: procedure expose passed failed verbose
  parse arg got, want, what
  if got == want then do
    passed = passed + 1
    if verbose then say 'ok' what
  end
  else do
    failed = failed + 1
    say 'FAIL' what': got ['got'] expected ['want']'
  end
  return

fact: procedure
  parse arg n
  if n < 2 then return 1
  return n * fact(n - 1)

sub1:
  return arg() arg(2, 'e') arg(2, 'o') arg(1) arg(3)

exposer: procedure expose g
  r = g 'seen'
  g = 'changed'
  return r

hidden: procedure
  return symbol('g')

exposestem: procedure expose stem.
  stem.2 = 'set'
  return stem.1

exposelist: procedure expose (lst)
  return g

nores:
  return

two:
  parse arg t1 t2, t3
  call check t1'|'t2'|'t3, 'x|y|z', 'parse arg with two templates'
  return

SysFileDeleteIfAny:
  if stream(arg(1), 'c', 'query exists') <> '' then '@del' arg(1)
  return

nr:
  call check condition('C'), 'NOTREADY', 'notready trapped'
  signal nrdone

err1:
  errseen = 'yes'
  return
