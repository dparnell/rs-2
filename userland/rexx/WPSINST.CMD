/* WPSINST.CMD: rs2's Workplace Shell support for Object REXX.

   WPSINST +   registers the classes of REXXWPS.DLL with the shell: their
               loading starts the shell's REXX server, through which Object
               REXX programs reach the shell's objects (.WPS, .WPDESKTOP,
               .WPFOLDER and the other Workplace Shell classes)
   WPSINST -   deregisters them
   WPSINST ?   tells whether they are registered
   WPSINST     deregisters, then registers them again

   It needs Object REXX (SWITCHRX.CMD) and the shell running. */

parse version . level .
if level < 6 then do
  say 'WPSINST needs Object REXX: run SWITCHRX.CMD first.'
  exit 32
end

call rxfuncadd 'SysLoadFuncs', 'REXXUTIL', 'SysLoadFuncs'
call SysLoadFuncs

classes = 'WPZORXINIT WPZORX'
parse arg what .
what = strip(what)

select
  when what = '+' then exit add()
  when what = '-' then exit remove()
  when what = '?' then do
    if registered() then say 'The Object REXX Workplace Shell classes are registered.'
    else say 'The Object REXX Workplace Shell classes are not registered.'
    exit 0
  end
  when what = '' then do
    call remove
    exit add()
  end
  otherwise
    say 'Usage: WPSINST [+ | - | ?]'
    say '  +  register the Object REXX Workplace Shell classes'
    say '  -  deregister them'
    say '  ?  tell whether they are registered'
    say '  (nothing: deregister, then register them again)'
    exit 1
end

add: procedure expose classes
  bad = 0
  do i = 1 to words(classes)
    c = word(classes, i)
    /* loaded each time the shell starts */
    call SysIni 'USER', 'PM_Workplace:IplLoad', c, '2000'x
    if SysRegisterObjectClass(c, 'REXXWPS') then
      say c 'is registered.'
    else do
      say 'The shell could not register' c'.'
      bad = 1
    end
  end
  return bad

remove: procedure expose classes
  do i = 1 to words(classes)
    c = word(classes, i)
    call SysIni 'USER', 'PM_Workplace:IplLoad', c, 'DELETE:'
    if SysDeregisterObjectClass(c) then say c 'is deregistered.'
  end
  return 0

registered: procedure expose classes
  call SysQueryClassList 'list.'
  n = 0
  do i = 1 to list.0
    if wordpos(translate(word(list.i, 1)), classes) > 0 then n = n + 1
  end
  return n = words(classes)
