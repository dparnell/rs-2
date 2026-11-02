/* RXUTEST.CMD: the REXX utility functions (REXXUTIL.DLL). Each check
   prints one line, then "RXUTEST: n passed, m failed". The expectations
   are IBM REXXUTIL's behaviour; the test runs against rs2's REXXUTIL
   and IBM's alike. Works in C:\RXUT (created and removed again).
   Argument WPS adds the Workplace Shell functions (the shell must run). */
parse upper arg opts
passed = 0; failed = 0
d = 'C:\RXUT'

call RxFuncAdd 'SysLoadFuncs', 'REXXUTIL', 'SysLoadFuncs'
call SysLoadFuncs
call check result = '', 'SysLoadFuncs returns ""'
call check call40('SysLoadFuncs 1') = 40, 'SysLoadFuncs with an argument: error 40'
call check SysUtilVersion() = '2.00', 'SysUtilVersion' SysUtilVersion()
v = SysOS2Ver()
call check datatype(v, 'N') & pos('.', v) = length(v) - 2, 'SysOS2Ver' v
call check SysVersion() = 'OS/2' v, 'SysVersion' SysVersion()
b = SysBootDrive()
call check length(b) = 2 & right(b, 1) = ':' & datatype(left(b, 1), 'U'), 'SysBootDrive' b
call check call40('SysUtilVersion 1') = 40, 'SysUtilVersion with an argument: error 40'

/* Directories and files */
call check SysMkDir(d) = 0, 'SysMkDir'
call check SysMkDir(d) <> 0, 'SysMkDir of an existing directory' SysMkDir(d)
call check SysMkDir(d'\SUB') = 0, 'SysMkDir of a subdirectory'
call check call40('SysMkDir') = 40, 'SysMkDir without arguments: error 40'
call make d'\A.TXT', 'hello'
call make d'\B.DAT', 'one', 'two'
call make d'\SUB\C.TXT', 'deep'
call check SysSetFileDateTime(d'\A.TXT', '2001-02-03', '04:05:06') = 0, 'SysSetFileDateTime'
call check SysGetFileDateTime(d'\A.TXT') = '2001-02-03 04:05:06', 'SysGetFileDateTime' SysGetFileDateTime(d'\A.TXT')
call check SysGetFileDateTime(d'\A.TXT', 'W') = '2001-02-03 04:05:06', 'SysGetFileDateTime W'
call check call40("SysGetFileDateTime '"d"\A.TXT', 'X'") = 40, 'SysGetFileDateTime X: error 40'
call check SysGetFileDateTime(d'\NONE.TXT') = -1, 'SysGetFileDateTime of a missing file'
call check SysSetFileDateTime(d'\NONE.TXT') = -1, 'SysSetFileDateTime of a missing file'
call check SysSetFileDateTime(d'\B.DAT', '1975-01-01') = -1, 'SysSetFileDateTime before 1980'
call check SysSetFileDateTime(d'\B.DAT', 'garbage') = -1, 'SysSetFileDateTime with a bad date'
call check SysSetFileDateTime(d'\B.DAT', , '13:07:08') = 0, 'SysSetFileDateTime of the time only'
call check SysSetFileDateTime(d'\SUB\C.TXT', '1999-12-31', '00:30:00') = 0, 'SysSetFileDateTime 1999'

/* SysFileTree */
call check SysFileTree(d'\A.TXT', 'f.') = 0 & f.0 = 1, 'SysFileTree one file' f.0
call check f.1 = ' 2/03/01   4:05a           7  A----  C:\RXUT\A.TXT', 'SysFileTree line "'f.1'"'
call SysFileTree d'\A.TXT', 'f.', 'T'
call check f.1 = '01/02/03/04/05           7  A----  C:\RXUT\A.TXT', 'SysFileTree T "'f.1'"'
call SysFileTree d'\A.TXT', 'f.', 'L'
call check f.1 = '2001-02-03 04:05:06           7  A----  C:\RXUT\A.TXT', 'SysFileTree L "'f.1'"'
call SysFileTree d'\B.DAT', 'f.'
call check subword(f.1, 2, 1) = '1:07p' & word(f.1, 3) = 10, 'SysFileTree afternoon "'f.1'"'
call SysFileTree d'\SUB\C.TXT', 'f.'
call check left(f.1, 17) = '12/31/99   0:30a ', 'SysFileTree midnight hour "'f.1'"'
call SysFileTree d'\*', 'f.', 'O'
call check f.0 = 3, 'SysFileTree * files and directories' f.0
call SysFileTree d'\*', 'f.', 'FO'
call check f.0 = 2 & f.1 = 'C:\RXUT\A.TXT' & f.2 = 'C:\RXUT\B.DAT', 'SysFileTree F O' f.1 f.2
call SysFileTree d'\*', 'f.', 'DO'
call check f.0 = 1 & f.1 = 'C:\RXUT\SUB', 'SysFileTree D O' f.1
call SysFileTree d'\', 'f.', 'BO'
call check f.0 = 3, 'SysFileTree of a directory with a backslash' f.0
call SysFileTree d'\*.TXT', 'f.', 'FSO'
call check f.0 = 2 & f.1 = 'C:\RXUT\A.TXT' & f.2 = 'C:\RXUT\SUB\C.TXT', 'SysFileTree S' f.1 f.2
call SysFileTree d, 'f.', 'DO'
call check f.0 = 1 & f.1 = 'C:\RXUT', 'SysFileTree of the directory itself' f.1
old = directory()
call directory d
call SysFileTree '*.TXT', 'g.', 'FO'
call check g.0 = 1 & g.1 = 'C:\RXUT\A.TXT', 'SysFileTree relative' g.1
call SysFileTree 'SUB\*', 'g.', 'FO'
call check g.0 = 1 & g.1 = 'C:\RXUT\SUB\C.TXT', 'SysFileTree relative path' g.1
call SysFileTree 'C:*.DAT', 'g.', 'FO'
call check g.0 = 1 & g.1 = 'C:\RXUT\B.DAT', 'SysFileTree drive-relative' g.1
call directory old
call SysFileTree d'\*', 'f.', 'FO', '-****'
call check f.0 = 0, 'SysFileTree attribute mask -****' f.0
call SysFileTree d'\A.TXT', 'f.', 'F', '+****', '***+*'
call check right(f.1, 22) = 'A--R-  C:\RXUT\A.TXT', 'SysFileTree sets read-only "'f.1'"'
call SysFileTree d'\*', 'f.', 'FO', '***+*'
call check f.0 = 1 & f.1 = 'C:\RXUT\A.TXT', 'SysFileTree finds read-only files' f.0
call SysFileTree d'\A.TXT', 'f.', 'F', , '-**-*'
call check right(f.1, 22) = '-----  C:\RXUT\A.TXT', 'SysFileTree clears attributes "'f.1'"'
call SysFileTree d'\NOTHERE\*', 'f.'
call check f.0 = 0 & result = 0, 'SysFileTree of a missing directory'
call check call40("SysFileTree 'x'") = 40, 'SysFileTree with one argument: error 40'
call check call40("SysFileTree 'x', 'f.', 'Q'") = 40, 'SysFileTree bad option: error 40'
call check call40("SysFileTree 'x', 'f.', ''") = 40, 'SysFileTree empty option: error 40'
call check call40("SysFileTree 'x', 'f.', 'F', '++++++'") = 40, 'SysFileTree long mask: error 40'
call check call40("SysFileTree 'x', 'f.', 'F', '+x'") = 40, 'SysFileTree bad mask: error 40'

/* SysFileSearch */
call make d'\S.TXT', 'first line', 'the Needle here', 'no match', 'NEEDLE again'
call check SysFileSearch('needle', d'\S.TXT', 'r.') = 0 & r.0 = 2, 'SysFileSearch' r.0
call check r.1 = 'the Needle here' & r.2 = 'NEEDLE again', 'SysFileSearch lines'
call SysFileSearch 'NEEDLE', d'\S.TXT', 'r.', 'C'
call check r.0 = 1 & r.1 = 'NEEDLE again', 'SysFileSearch C' r.0
call SysFileSearch 'needle', d'\S.TXT', 'r.', 'N'
call check r.0 = 2 & r.1 = '2 the Needle here' & r.2 = '4 NEEDLE again', 'SysFileSearch N' r.1
call check SysFileSearch('x', d'\NONE.TXT', 'r.') = 3, 'SysFileSearch of a missing file'
call check call40("SysFileSearch '', 'x', 'r.'") = 40, 'SysFileSearch empty target: error 40'

/* SysTempFileName, SysSearchPath, SysWildCard */
t = SysTempFileName(d'\T???.TMP')
call check length(t) = 16 & left(t, 9) = 'C:\RXUT\T' & right(t, 4) = '.TMP' & datatype(substr(t, 10, 3), 'W'), 'SysTempFileName' t
t = SysTempFileName(d'\U##', '#')
call check length(t) = 11 & datatype(right(t, 2), 'W'), 'SysTempFileName with a filler' t
call check SysTempFileName(d'\X') = '', 'SysTempFileName without filler characters'
call check SysTempFileName('C:\RXUNONE\X??') = '', 'SysTempFileName in a missing directory'
call check call40("SysTempFileName 'x??', 'ab'") = 40, 'SysTempFileName long filler: error 40'
p = SysSearchPath('PATH', 'CMD.EXE')
call check right(p, 8) = '\CMD.EXE', 'SysSearchPath' p
call check SysSearchPath('PATH', 'RXUNONE.EXE') = '', 'SysSearchPath of a missing file'
call check SysSearchPath('DPATH', 'OSO001.MSG', 'N') <> '', 'SysSearchPath N'
call check SysWildCard('readme.txt', '*.bak') = 'readme.bak', 'SysWildCard' SysWildCard('readme.txt', '*.bak')
call check SysWildCard('a.b', '?x.*') = 'ax.b', 'SysWildCard ?' SysWildCard('a.b', '?x.*')

/* Extended attributes */
call check SysPutEA(d'\B.DAT', 'RXU.ONE', 'first value') = 0, 'SysPutEA'
call check SysPutEA(d'\B.DAT', 'RXU.TWO', 'x') = 0, 'SysPutEA another'
call check SysGetEA(d'\B.DAT', 'RXU.ONE', 'ea') = 0 & ea = 'first value', 'SysGetEA' ea
call check SysGetEA(d'\B.DAT', 'RXU.NONE', 'ea') = 0 & ea = '', 'SysGetEA of a missing EA'
call check SysQueryEAList(d'\B.DAT', 'eal.') = 0, 'SysQueryEAList'
names = ''
do i = 1 to eal.0; names = names translate(eal.i); end
call check wordpos('RXU.ONE', names) > 0 & wordpos('RXU.TWO', names) > 0, 'SysQueryEAList names' names
call check SysPutEA(d'\B.DAT', 'RXU.TWO', '') = 0, 'SysPutEA empty deletes'
call check SysGetEA(d'\NONE.TXT', 'X', 'ea') <> 0, 'SysGetEA of a missing file'

/* Drives */
di = SysDriveInfo(b)
call check word(di, 1) = b & datatype(word(di, 2), 'W') & datatype(word(di, 3), 'W'), 'SysDriveInfo' di
call check word(di, 2) <= word(di, 3), 'SysDriveInfo free <= total'
call check substr(di, 3, 2) = '  ', 'SysDriveInfo layout "'di'"'
call check call40("SysDriveInfo 'CX'") = 40, 'SysDriveInfo bad drive: error 40'
call check wordpos(b, SysDriveMap()) > 0, 'SysDriveMap' SysDriveMap()
call check word(SysDriveMap(b, 'USED'), 1) = b, 'SysDriveMap from the boot drive'
call check wordpos(b, SysDriveMap('A', 'FREE')) = 0, 'SysDriveMap FREE' SysDriveMap('A', 'FREE')
call check call40("SysDriveMap 'C', 'BOGUS'") = 40, 'SysDriveMap bad mode: error 40'
call check SysFileSystemType(b) = 'FAT', 'SysFileSystemType' SysFileSystemType(b)

/* Profiles */
ini = d'\T.INI'
call check SysIni(ini, 'App1', 'K1', 'V1') = '', 'SysIni set'
call check SysIni(ini, 'App1', 'K2', 'V2') = '', 'SysIni set another'
call check SysIni(ini, 'App2', 'K3', 'V3') = '', 'SysIni set in another application'
call check SysIni(ini, 'App1', 'K1') = 'V1', 'SysIni query' SysIni(ini, 'App1', 'K1')
call check SysIni(ini, 'App1', 'NOKEY') = 'ERROR:', 'SysIni query of a missing key'
call SysIni ini, 'ALL:', 'apps.'
call check apps.0 = 2 & apps.1 = 'App1' & apps.2 = 'App2', 'SysIni ALL: applications' apps.0
call SysIni ini, 'App1', 'ALL:', 'keys.'
call check keys.0 = 2 & keys.1 = 'K1' & keys.2 = 'K2', 'SysIni ALL: keys' keys.0
call check SysIni(ini, 'App1', 'K1', 'DELETE:') = '', 'SysIni delete a key'
call check SysIni(ini, 'App1', 'K1') = 'ERROR:', 'SysIni deleted key is gone'
call check SysIni(ini, 'App2', 'DELETE:') = '', 'SysIni delete an application'
call SysIni ini, 'ALL:', 'apps.'
call check apps.0 = 1, 'SysIni application deleted' apps.0
call check SysIni('USER', 'RXUTEST', 'Key', 'user value') = '', 'SysIni USER set'
call check SysIni(, 'RXUTEST', 'Key') = 'user value', 'SysIni default profile query'
call check SysIni('USER', 'RXUTEST', 'DELETE:') = '', 'SysIni USER delete'
call check SysIni('USER', 'RXUTEST', 'Key') = 'ERROR:', 'SysIni USER deleted'
call check call40('SysIni') = 40, 'SysIni without arguments: error 40'
call check call40("SysIni 'x', 'ALL:'") = 40, 'SysIni ALL: without a stem: error 40'

/* Stems */
s.0 = 5; s.1 = 'pear'; s.2 = 'Apple'; s.3 = 'banana'; s.4 = 'apple2'; s.5 = 'Cherry'
call check SysStemSort('s.') = 0, 'SysStemSort'
call check s.1 s.2 s.3 s.4 s.5 = 'Apple Cherry apple2 banana pear', 'SysStemSort ascending' s.1 s.2 s.3 s.4 s.5
call SysStemSort 's', 'D'
call check s.1 s.2 s.3 s.4 s.5 = 'pear banana apple2 Cherry Apple', 'SysStemSort descending' s.1 s.2 s.3 s.4 s.5
call SysStemSort 's.', 'A', 'I'
call check s.1 s.2 s.3 s.4 s.5 = 'Apple apple2 banana Cherry pear', 'SysStemSort case-insensitive' s.1 s.2 s.3 s.4 s.5
call SysStemSort 's.', 'D', 'C', 2, 4
call check s.1 s.2 s.3 s.4 s.5 = 'Apple banana apple2 Cherry pear', 'SysStemSort range' s.1 s.2 s.3 s.4 s.5
u.0 = 3; u.1 = 'x3b'; u.2 = 'y1c'; u.3 = 'z2a'
call SysStemSort 'u.', 'A', 'C', , , 2, 2
call check u.1 u.2 u.3 = 'y1c z2a x3b', 'SysStemSort column' u.1 u.2 u.3
call SysStemSort 'u.', 'A', 'C', , , 3
call check u.1 u.2 u.3 = 'z2a x3b y1c', 'SysStemSort from a column' u.1 u.2 u.3
call SysStemSort 'u.', 'D', 'I', , , 2, 3
call check u.1 u.2 u.3 = 'x3b z2a y1c', 'SysStemSort column case-insensitive' u.1 u.2 u.3
call check call40("SysStemSort 's.', 'X'") = 40, 'SysStemSort bad order: error 40'
call check call40("SysStemSort 's.', 'A', 'C', 4, 2") = 40, 'SysStemSort end before start: error 40'
call check call40("SysStemSort 's.', 'A', 'C', 9") = 40, 'SysStemSort start past the end: error 40'
x = SysStemSort('nostem.')
call check x = 0, 'SysStemSort of a stem without .0 does nothing' x
call check SysStemInsert('s.', 2, 'new') = 0 & s.0 = 6 & s.2 = 'new' & s.3 = 'banana' & s.6 = 'pear', 'SysStemInsert' s.0
call check SysStemInsert('s.', 7, 'last') = 0 & s.0 = 7 & s.7 = 'last', 'SysStemInsert at the end'
call check call40("SysStemInsert 's.', 0, 'x'") = 40, 'SysStemInsert at 0: error 40'
call check call40("SysStemInsert 's.', 9, 'x'") = 40, 'SysStemInsert past the end: error 40'
call check SysStemDelete('s.', 2) = 0 & s.0 = 6 & s.2 = 'banana' & symbol('s.7') = 'LIT', 'SysStemDelete' s.0
call check SysStemDelete('s.', 5, 2) = 0 & s.0 = 4 & s.4 = 'Cherry', 'SysStemDelete two' s.0
call check call40("SysStemDelete 's.', 4, 2") = 40, 'SysStemDelete past the end: error 40'
call check SysStemCopy('s.', 'c.') = 0 & c.0 = 4 & c.1 = 'Apple' & c.4 = 'Cherry', 'SysStemCopy' c.0
call check SysStemCopy('s.', 'c.', 2, 2, 2, 'I') = 0 & c.0 = 6 & c.2 = 'banana' & c.3 = 'apple2' & c.4 = 'banana', 'SysStemCopy insert' c.0 c.2 c.3 c.4
call check SysStemCopy('s.', 'c.', 1, 6, 2) = 0 & c.0 = 7 & c.6 = 'Apple' & c.7 = 'banana', 'SysStemCopy past the end' c.0
drop n.
call check SysStemCopy('s.', 'n.', 3) = 0 & n.0 = 2 & n.1 = 'apple2' & n.2 = 'Cherry', 'SysStemCopy to a new stem' n.0
call check call40("SysStemCopy 's.', 'c.', 1, 1, 1, 'X'") = 40, 'SysStemCopy bad option: error 40'
call check dumpvars(d'\VARS.TXT') = 0, 'SysDumpVariables to a file'
vars = charin(d'\VARS.TXT', 1, chars(d'\VARS.TXT'))
call stream d'\VARS.TXT', 'c', 'close'
call check pos("Name=ALPHA, Value='1'"'0d0a'x, vars) > 0 & pos("Name=BETA, Value='two words'"'0d0a'x, vars) > 0, 'SysDumpVariables lines'

/* Semaphores */
h = SysCreateEventSem()
call check handle(h), 'SysCreateEventSem' h
call check SysPostEventSem(h) = 0, 'SysPostEventSem'
call check SysWaitEventSem(h, 0) = 0, 'SysWaitEventSem posted'
call check SysResetEventSem(h) = 0, 'SysResetEventSem'
call check SysWaitEventSem(h, 0) = 640, 'SysWaitEventSem times out' SysWaitEventSem(h, 0)
call check SysCloseEventSem(h) = 0, 'SysCloseEventSem'
call check SysCloseEventSem(h) <> 0, 'SysCloseEventSem twice'
call check call40("SysCloseEventSem 'x'") = 40, 'SysCloseEventSem bad handle: error 40'
h = SysCreateEventSem('\SEM32\RXUTEST')
h2 = SysCreateEventSem('\SEM32\RXUTEST')
call check handle(h) & handle(h2), 'SysCreateEventSem named, twice' h h2
call SysCloseEventSem h2; call SysCloseEventSem h
m = SysCreateMutexSem()
call check handle(m), 'SysCreateMutexSem' m
call check SysRequestMutexSem(m, 0) = 0, 'SysRequestMutexSem'
call check SysReleaseMutexSem(m) = 0, 'SysReleaseMutexSem'
call check SysReleaseMutexSem(m) <> 0, 'SysReleaseMutexSem not owned'
call check SysCloseMutexSem(m) = 0, 'SysCloseMutexSem'

/* Process, code pages, national language support */
call check SysSetPriority(2, 0) = 0, 'SysSetPriority'
call check call40('SysSetPriority 5, 0') = 40, 'SysSetPriority bad class: error 40'
call check call40('SysSetPriority 2, 32') = 40, 'SysSetPriority bad delta: error 40'
call check datatype(SysProcessType(), 'W'), 'SysProcessType' SysProcessType()
cp = SysQueryProcessCodePage()
call check datatype(cp, 'W') & cp > 0, 'SysQueryProcessCodePage' cp
call check SysSetProcessCodePage(cp) = 0, 'SysSetProcessCodePage'
call check SysMapCase('abc xyz 123') = 'ABC XYZ 123', 'SysMapCase' SysMapCase('abc xyz 123')
call check SysNationalLanguageCompare('apple', 'banana') = -1, 'SysNationalLanguageCompare less'
call check SysNationalLanguageCompare('b', 'a') = 1, 'SysNationalLanguageCompare greater'
call check SysNationalLanguageCompare('same', 'same') = 0, 'SysNationalLanguageCompare equal'
call check SysNationalLanguageCompare('abc', 'abcd') = -1, 'SysNationalLanguageCompare shorter'
call check length(SysGetCollate()) = 256, 'SysGetCollate'
call check call40("SysMapCase 'x', 'y'") = 40, 'SysMapCase bad country: error 40'
fh = SysAddFileHandle(0)
call check datatype(fh, 'W') & fh >= 20, 'SysAddFileHandle' fh
call check SysAddFileHandle(5) = fh + 5, 'SysAddFileHandle 5' SysAddFileHandle(0)
call check SysSetFileHandle(fh + 10) = 0, 'SysSetFileHandle'
call check call40('SysSetFileHandle 1') = 40, 'SysSetFileHandle 1: error 40'
call check SysSetExtLIBPATH(d, 'B') = 0, 'SysSetExtLIBPATH'
call check SysQueryExtLIBPATH('B') = d, 'SysQueryExtLIBPATH' SysQueryExtLIBPATH('B')
call check SysSetExtLIBPATH('', 'B') = 0 & SysQueryExtLIBPATH('B') = '', 'SysSetExtLIBPATH empty'
call check call40("SysQueryExtLIBPATH 'X'") = 40, 'SysQueryExtLIBPATH X: error 40'
call check SysWaitNamedPipe('\PIPE\RXUNONE', 0) <> 0, 'SysWaitNamedPipe of a missing pipe' SysWaitNamedPipe('\PIPE\RXUNONE', 0)

/* Time */
call check SysSleep(0.1) = 0, 'SysSleep fraction'
call check SysSleep(0) = 0, 'SysSleep 0'
call check call40("SysSleep 'abc'") = 40, 'SysSleep abc: error 40'
call check call40("SysSleep '1.x'") = 40, 'SysSleep 1.x: error 40'
call check call40("SysSleep '1234567890'") = 40, 'SysSleep 10 digits: error 40'
call SysElapsedTime 'R'
call SysSleep 0.3
e = SysElapsedTime()
call check datatype(e, 'N') & e >= 0.2 & e < 5 & length(e) - pos('.', e) = 6, 'SysElapsedTime' e
call check SysElapsedTime('R') >= e & SysElapsedTime() < e, 'SysElapsedTime reset'
call check call40("SysElapsedTime 'X'") = 40, 'SysElapsedTime X: error 40'

/* Messages */
msg = SysGetMessage(2)
call check length(msg) > 0, 'SysGetMessage' strip(msg)
call check SysGetMessage(2, 'OSO001.MSG') = msg, 'SysGetMessage with the file'
call check call40("SysGetMessage 'x'") = 40, 'SysGetMessage bad number: error 40'

/* Macro space */
call make d'\MAC.CMD', '/* macro */', "return 'macro' arg(1)"
call check SysAddRexxMacro('RXUMAC', d'\MAC.CMD') = 0, 'SysAddRexxMacro'
call check SysQueryRexxMacro('RXUMAC') = 'B', 'SysQueryRexxMacro' SysQueryRexxMacro('RXUMAC')
call check rxumac('ok') = 'macro ok', 'calling a macro'
call check SysReorderRexxMacro('RXUMAC', 'A') = 0 & SysQueryRexxMacro('RXUMAC') = 'A', 'SysReorderRexxMacro'
call check SysSaveRexxMacroSpace(d'\MAC.RXL') = 0, 'SysSaveRexxMacroSpace'
call check SysDropRexxMacro('RXUMAC') = 0 & SysQueryRexxMacro('RXUMAC') = '', 'SysDropRexxMacro'
call check SysLoadRexxMacroSpace(d'\MAC.RXL') = 0 & SysQueryRexxMacro('RXUMAC') = 'A', 'SysLoadRexxMacroSpace'
call check SysClearRexxMacroSpace() = 0 & SysQueryRexxMacro('RXUMAC') = '', 'SysClearRexxMacroSpace'
call check SysDropRexxMacro('RXUMAC') <> 0, 'SysDropRexxMacro of a missing macro'
call check call40("SysAddRexxMacro 'x', 'y', 'Q'") = 40, 'SysAddRexxMacro bad order: error 40'

/* Function packages */
call check SysLoadLibrary('REXXUTIL', 'SysUtilVersion') = 0, 'SysLoadLibrary'
call check SysLoadLibrary('RXUNONE') = 1, 'SysLoadLibrary of a missing DLL'

/* The text screen */
cls = SysCls()
p1 = SysCurPos(3, 10)
call charout , 'RXUTEST-SCREEN'
p2 = SysCurPos()
t1 = SysTextScreenRead(3, 10, 14)
t2 = SysTextScreenRead(3, 0)
say
call check cls = 0, 'SysCls'
call check p1 = '0 0', 'SysCurPos after SysCls' p1
call check p2 = '3 24', 'SysCurPos after output' p2
call check t1 = 'RXUTEST-SCREEN', 'SysTextScreenRead' t1
call check length(t2) > 14, 'SysTextScreenRead to the end' length(t2)
ts = SysTextScreenSize()
call check words(ts) = 2 & word(ts, 1) >= 25 & word(ts, 2) >= 80, 'SysTextScreenSize' ts
call check SysCurState('OFF') = 0 & SysCurState('on') = 0, 'SysCurState'
call check call40("SysCurState 'x'") = 40, 'SysCurState x: error 40'
call check call40('SysCurPos 1') = 40, 'SysCurPos one argument: error 40'
call check call40('SysCurPos -1, 0') = 40, 'SysCurPos negative: error 40'

/* The Workplace Shell (only with the shell running) */
if wordpos('WPS', opts) > 0 then do
  call check SysQueryClassList('cl.') = 0 & cl.0 > 10, 'SysQueryClassList' cl.0
  call check SysCreateObject('WPFolder', 'RXU Folder', '<WP_DESKTOP>', 'OBJECTID=<RXU_FOLDER>', 'R') = 1, 'SysCreateObject'
  call check SysSetObjectData('<RXU_FOLDER>', 'TITLE=RXU Renamed') = 1, 'SysSetObjectData'
  call check SysDestroyObject('<RXU_FOLDER>') = 1, 'SysDestroyObject'
  call check SysDestroyObject('<RXU_FOLDER>') = 0, 'SysDestroyObject of a missing object'
  call check SysQuerySwitchList('sw.') = 0 & sw.0 > 0, 'SysQuerySwitchList' sw.0
  call check SysWaitForShell('DESKTOPCREATED', 'QUERY') = 1, 'SysWaitForShell'
end

/* Clean up */
call check SysFileDelete(d'\A.TXT') = 0, 'SysFileDelete'
call check SysFileDelete(d'\A.TXT') = 2, 'SysFileDelete of a missing file'
call check SysRmDir(d'\SUB') <> 0, 'SysRmDir of a directory with files'
call SysFileTree d'\*', 'f.', 'FSO'
do i = 1 to f.0; call SysFileDelete f.i; end
call check SysRmDir(d'\SUB') = 0, 'SysRmDir'
call check SysRmDir(d) = 0, 'SysRmDir of the work directory'
call check SysDropFuncs() = '', 'SysDropFuncs'
call check call40('SysUtilVersion') = 43, 'functions dropped'
say 'RXUTEST:' passed 'passed,' failed 'failed'
exit 0

/* Write lines to a new file. */
make: procedure
  parse arg file
  call SysFileDelete file
  do i = 2 to arg()
    call lineout file, arg(i)
  end
  call lineout file
  return

/* The REXX error number a call raises (0 if none). */
call40: procedure expose s. u.
  signal on syntax name call40.raised
  interpret 'call' arg(1)
  return 0
call40.raised:
  return rc

/* A semaphore handle: a number (above 2**31 for shared ones). */
handle: return arg(1) <> '' & verify(arg(1), '0123456789') = 0

dumpvars: procedure
  alpha = 1
  beta = 'two words'
  return SysDumpVariables(arg(1))

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
