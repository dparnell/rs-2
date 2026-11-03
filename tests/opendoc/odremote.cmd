/* ODREMOTE.CMD: drive a running ODSHELL from another process (Object REXX) */
call time 'R'
do while time('E') < 25; end
shell = .ScriptableApp~new~connect('ODSHELL')
shell~set(text~of(part~index(1)), .directory~new~~setentry('TO', 'Set by ODREMOTE.CMD, run by CMD'))
shell~play
say 'part 1:' shell~get(text~of(part~index(1)))
say 'part 2:' shell~get(name~of(part~index(2)))
