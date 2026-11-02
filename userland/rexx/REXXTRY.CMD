/* REXXTRY.CMD: try REXX instructions (rs2).

   REXXTRY instructions   runs the instructions and ends.
   REXXTRY                reads instructions, one line at a time, and runs
                          each line as it is entered; variables keep their
                          values from line to line. "?" shows help, EXIT
                          (or an end of input) ends.

   The instructions run with INTERPRET in this procedure, so they see and
   set its variables; the names this procedure uses itself begin with
   "RT_". */
signal on halt name rt_halted
parse arg rt_arg
parse source . . rt_me
rt_me = filespec('name', rt_me)
if rt_arg <> '' then do
  signal on syntax name rt_onearg
  interpret rt_arg
  exit
end
say rt_me': type REXX instructions to run them; ? for help, EXIT to end.'
rt_count = 0

rt_next:
  signal on syntax name rt_syntax
  do forever
    call charout , rt_me'> '
    if lines() = 0 then exit
    rt_line = linein()
    if strip(rt_line) = '' then iterate
    if strip(rt_line) = '?' then do
      call rt_help
      iterate
    end
    rt_count = rt_count + 1
    rc = 0
    interpret rt_line
    if rc <> 0 & datatype(rc, 'W') then say '  rc =' rc
  end

rt_syntax:
  say '  Error' rc 'in line' rt_count':' errortext(rc)
  signal rt_next

rt_onearg:
  say rt_me': error' rc':' errortext(rc)
  exit rc

rt_halted:
  say rt_me': halted.'
  exit

rt_help:
  say '  Each line is run as REXX instructions (separate several with ;).'
  say '  Expressions are shown with SAY:          say 2**10 date()'
  say '  Variables keep their values:             a = 5; say a * a'
  say '  Commands go to the current environment:  ''dir /w'''
  say '  A line ending in a comma does not continue: put a whole'
  say '  instruction (DO ... END) on one line.'
  say '  EXIT ends' rt_me'.'
  return
