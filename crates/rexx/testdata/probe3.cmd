/* probe 3: outputs to compare */
say .array~new~supplier
say .message~new(1, 'X')
say .method~new('a', 'nop')
say .stream~new('X.Y')
say .output
say .class
say .object
say .directory
say .list~new
say .queue~new
say .table~new
say .relation~new
say .set~new
say .bag~new
say .stem~new('S.')
say .supplier~new(.array~of(1), .array~of(2))
say .alarm
say .monitor~new(1)
say .environment
say .local
say 'abc'~class~id 12~class~id
say .array~of(1, 2)~class~id
x = .array~of(3, 1, 2)
say x~items x~size x~dimension
say 'x'~hasmethod('REVERSE') 'x'~hasmethod('FOO')
say .string~id .string~superclasses[1]~id
say .set~superclasses[1]~id .bag~superclasses[1]~id
say .array~new(5)~size .array~new(5)~items
say 'a b c'~subword(2) 'abc'~right(5) 'abc'~center(6)
say 12.5~format(4, 3) 1e3~format 'ab'~c2x 255~d2x '41'x~c2d
say 3 ~ '+' (4)
trace i
y = 'a'~reverse || 'b'
z = .array~of(1)~items + 1
trace o
do i = 1 to 2
  trace r
  w = i * 2
  trace o
end
call nest
say 'done'
exit
nest: procedure
  trace r
  if 1 then do
    a = 1
    if a then b = 2
    else b = 3
  end
  select
    when a = 1 then c = 1
    otherwise c = 2
  end
  trace o
  return
