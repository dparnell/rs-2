/* ORXSOM.CMD: SOM classes from Object REXX (OREXXSOM as REXXSOM.DLL,
   SOMIR's interface repository through SET SOMIR). Prints one line per
   check and "ORXSOM: n passed, m failed" at the end. */
passed = 0
failed = 0

obj = .som~import('SOMObject')
call check obj~class~id \= '', '.SOM~import of SOMObject'
o = obj~new
call check o~somGetClassName = 'SOMObject', 'somGetClassName'
call check o~somGetSize = 4, 'somGetSize'
call check o~somRespondsTo('somGetSize') = 1, 'somRespondsTo with a somId argument'
call check o~somIsA(o~somGetClass) = 1, 'somIsA with an object argument'
cls = o~somGetClass
call check cls~somGetName = 'SOMObject', 'the class object: somGetName'
call check cls~somGetInstanceSize = 4, 'the class object: somGetInstanceSize'

/* A class imported by directive, and a subclass made in REXX */
m = .somclassmgr~new
call check m~somGetClassName = 'SOMClassMgr', '::class external SOM'

/* INOUT and OUT arguments: the DLF classes */
l = .som~import('DLFLong')~new
l~_set_value(42)
call check l~_get_value = 42, 'DLFLong'
s = .som~import('DLFString')~new
s~_set_value('hello')
call check s~_get_value = 'hello', 'DLFString'
d = .som~import('DLFDouble')~new
d~_set_value(1.5)
call check d~_get_value = 1.5, 'DLFDouble'

/* The interface repository itself */
ir = .som~import('Repository')~new
call check ir~somGetClassName = 'Repository', 'Repository'
def = ir~lookup_id('::SOMObject')
call check def~somGetClassName = 'InterfaceDef', 'Repository lookup_id gives an InterfaceDef'
call check def~_get_name = 'SOMObject', 'InterfaceDef _get_name'

say 'ORXSOM:' passed 'passed,' failed 'failed'
exit failed

check: procedure expose passed failed
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

::class somclassmgr external 'SOM SOMClassMgr'
