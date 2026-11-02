# REXX

rs2 has its own classic REXX: the language of IBM's OS/2 REXX (SAA
Procedures Language 2.0, `PARSE VERSION` gives `REXXSAA 4.00 3 Feb
1999` as IBM's does), its application programming interface and its
utilities, binary compatible with Warp 4.52's so that OS/2 programs that
use the REXX API, IBM's 16-bit CMD.EXE among them, run on it. Nothing of
IBM's REXX is shipped; the modules were studied in Ghidra (with the
`.sym` files of the media) and the language in REXX.INF, and every
behaviour the test suite checks was checked on IBM's REXX first. rs2
also has its own [Object REXX](#object-rexx) (IBM's Object REXX 6.00:
classes, messages, directives, activities, SOM and Workplace Shell
scripting), installed beside classic REXX; SWITCHRX.CMD makes it the
system's REXX.

| Module | Source | Image base | Role |
|--------|--------|------------|------|
| REXX.DLL | `userland/rexx`, `crates/rexx` | 0x15800000 | The interpreter: RexxStart, RexxVariablePool, RexxSetHalt/RexxSetTrace/RexxResetTrace; the 16-bit REXXSAA, RXVAR, RXHALTSET, RXTRACESET, RXTRACERESET, RXBREAKCLEANUP |
| REXXAPI.DLL | `userland/rexxapi` | 0x15900000 | Queues, subcommand handlers, exits, function packages, the macro space (IBM's 48 entries, 32- and 16-bit) |
| REXXUTIL.DLL | `userland/rexxutil` | 0x15940000 | The 85 Sys* functions ([below](#rexxutil)) |
| PMREXXIO.DLL | `userland/pmrexxio` | 0x159C0000 | PMREXX's I/O window |
| REXXINIT.DLL | `userland/rexxinit` | 0x15A00000 | Keeps the API's shared state alive |
| RXQUEUE.EXE, RXSUBCOM.EXE | `userland/rexxcmds` | | [below](#rexx-tools) |
| PMREXX.EXE | `userland/pmrexx` | | [below](#rexx-tools) |
| REXXTRY.CMD | `userland/rexx/REXXTRY.CMD` | | Tries REXX instructions (rs2's own program) |
| REX.MSG, REXH.MSG | `userland/msg/REX*.TXT` | | Messages and their help, built by xtask (crates/msgf) |
| OREXX.DLL | `userland/orexx` (`userland/rexx` built with `cfg(orexx)`), `crates/rexx` | 0x15800000 | [Object REXX](#object-rexx): the module REXX, REXX.DLL once SWITCHRX.CMD switches |
| OREXUTIL.DLL | `userland/rexxutil` | 0x15940000 | REXXUTIL for Object REXX (the same DLL as module REXXUTIL) |
| OREXXSOM.DLL | `userland/rexxsom`, `userland/rxsom` | 0x15A40000 | SOM classes from Object REXX (module REXXSOM) |
| OREXXWPS.DLL | `userland/rexxwps`, `userland/rxsom` | 0x15A80000 | Scripting the Workplace Shell (module REXXWPS) |
| REXXC.EXE | `userland/rexxcmds/src/bin/rexxc.rs` | | Object REXX's tokenizer |
| SWITCHRX.CMD, WPSINST.CMD | `userland/rexx/` | | Switch the system's REXX; register the Workplace Shell classes |
| OREX.MSG, OREXH.MSG | `userland/msg/OREX*.TXT` | | Object REXX's messages (REX.MSG, REXH.MSG once switched) |

## The interpreter (crates/rexx)

`crates/rexx` is a `no_std` crate with no OS dependencies: everything
that touches the system goes through its `Host` trait (output, the
terminal, commands, the queue, files, external routines, the
environment, the clock, messages). REXX.DLL implements the host on OS/2
calls; the crate's tests (`cargo test -p rexx`) use a host in memory.
Strings are byte strings, one character per byte.

- **Clauses.** The tokenizer splits the program into clauses at
  semicolons and line ends (a comma ending a line continues the clause;
  comments nest; Ctrl+Z ends the text). A clause it cannot tokenize (an
  invalid hexadecimal string, a character that is not allowed) keeps its
  error, raised when the clause is reached, as IBM's REXX does: the
  clauses before it run, and SIGNAL ON SYNTAX can trap it. An unended
  comment, and structural errors (DO without END, ELSE without IF...)
  fail the whole program before it starts.
- **Execution.** A program is a flat list of instructions; DO/END,
  IF/THEN/ELSE and SELECT/WHEN/OTHERWISE are linked by index, and each
  routine activation runs the list with a program counter and a control
  stack, which is what SIGNAL needs (it may jump into the middle of
  anything). INTERPRET parses the string as a program of its own, run in
  the same activation; as IBM's, its ITERATE and LEAVE see only loops in
  the interpreted string (error 28 otherwise), and its clauses carry the
  INTERPRET's line number.
- **Arithmetic** is decimal, to NUMERIC DIGITS significant digits
  (rounded half up), with FUZZ for comparisons and SCIENTIFIC or
  ENGINEERING exponential notation; results are formatted by REXX's
  rules (plain unless the integer part would need more than DIGITS
  digits or the fraction more than twice DIGITS). Additions and
  multiplications keep trailing zeros (`1.50+1` is `2.50`); divisions
  and powers drop them (`1.0**2` is `1`, as IBM's).
- **Variables.** Each PROCEDURE has a pool; EXPOSE (also of stems,
  compound variables and `(list)`) links names to the caller's. Stems
  keep their default value; a tail dropped while the stem has one reads
  as its own name again.
- **Conditions.** SIGNAL ON and CALL ON for ERROR, FAILURE, HALT,
  NOTREADY (CALL ON), NOVALUE and SYNTAX (SIGNAL ON), with NAME;
  CONDITION('C'/'D'/'I'/'S'); CALL ON handlers run after the clause with
  the trap delayed. An untrapped error ends the whole program with a
  traceback: the failing clause, the message, and the clause of every
  active call (`    26 +++     Call lvl2;`).
- **Built-in functions**: all of OS/2 REXX's (ABBREV ... XRANGE, X2B, X2C,
  X2D), the stream functions, CONDITION, DIRECTORY, ENDLOCAL/SETLOCAL,
  FILESPEC, RXFUNCADD/RXFUNCDROP/RXFUNCQUERY (0 done, 1 not), RXQUEUE,
  BEEP, VALUE with the OS2ENVIRONMENT selector, and the DBCS functions
  (on a single-byte system). DATE takes B D E M N O S U W (IBM's 4.00
  has no C or J: error 40); TIME takes C E H L M N R S, `L` with
  microseconds.
- **Streams.** CHARIN, CHAROUT, CHARS, LINEIN, LINEOUT, LINES and STREAM
  (OPEN [READ|WRITE], CLOSE, SEEK =/</+/-, QUERY EXISTS/SIZE/DATETIME,
  'S' and 'D' states READY/NOTREADY/ERROR/UNKNOWN). Streams open on first
  use (reading opens read-only; writing appends), stay open until closed
  or the program ends; lines end at CR LF or LF, Ctrl+Z ends text files.
  STDIN, STDOUT, STDERR (with or without a colon) are the standard
  handles; `QUEUE:` reads the external data queue. LINES and CHARS on a
  pipe or redirected input read one character ahead to see the end; on a
  device they are 1.
- **TRACE** A, C, E, F, I, L, N (the default), O, R and S, `?`
  (interactive: a pause after each traced clause, `=` runs the clause
  again, other input runs as instructions) and `!` (commands not run).
  The trace output is IBM's: clauses are rebuilt from their tokens
  (keywords capitalised, symbols as first spelt in the program, tokens
  one blank apart: `     6 *-*   y = 'a''b' 'c' || X;`), indented by
  nesting and call depth, results as `       >>>     "3"` and the
  intermediates `>V> >L> >C> >F> >O> >P> >.>`; commands show
  `    33 >>>       "@echo hi"` and a nonzero RC `       +++     RC(1041)`.

## REXX.DLL

- **RexxStart** runs a procedure from a file (the default extension
  `.CMD`; the name as given, else along PATH for external routines),
  from memory (`instore[0]` holds the source; `instore[1]` gets an image,
  which is rs2's own: the source behind a marker) or from the macro
  space. The initial environment is the one given, else the file's
  extension (`CMD`). Arguments may be omitted (null strings). The result
  goes into the caller's RXSTRING when it fits, else into memory from
  DosAllocMem; `rc` gets it when it is a whole number that fits a SHORT.
  An error ends the call with the negative error number (CMD's
  ERRORLEVEL then shows 65501 for error 35, as with IBM's). A missing
  file gives -3.
- **Exits** named in the exit list (RXFNC, RXCMD, RXMSQ, RXSIO, RXHLT,
  RXTRC, RXINI, RXTER) are called through REXXAPI's RexxCallExit with
  IBM's packed parameter blocks (RXFNCCAL_PARM: flags byte, then name,
  queue, argument list and result at offsets 1, 7, 0x0F, 0x13).
  Without an RXSIO exit SAY writes to handle 1, trace and error output
  to handle 2, PULL reads handle 0.
- **Commands** go to the environment's subcommand handler through
  RexxCallSubcom (`ADDRESS ENV.DLL` names the DLL too). As in IBM's
  `sys_command`: a nonzero handler return is RC, else the result
  string; the handler's flags give ERROR or FAILURE; an environment
  nobody registered gives RC 30 and FAILURE, except CMD, whose commands
  then run through `%COMSPEC% /C` (REXX run by a program that is not
  CMD); CMD's SYS1041 (not a command) is a FAILURE, so the default TRACE
  N shows it.
- **External routines**: the macro space's "before" functions, the RXFNC
  exit, function packages (RexxCallFunction; a nonzero return is error
  40), programs on disk (`name.CMD` in the current directory, then
  PATH), the "after" macros; else error 43.
- **RexxVariablePool** works while the thread's interpreter is in a
  callback (an exit, a function package, a subcommand handler):
  RXSHV_SET/FETCH/DROPV (names as given) and SYSET/SYFET/SYDRO
  (symbolic), NEXTV, PRIV (PARM, PARM.n, SOURCE, VERSION, QUENAME);
  FETCH with a null buffer allocates the value (DosAllocMem); flags NEWV,
  LVAR, TRUNC, BADN, MEMFL, BADF; RXSHV_NOAVL (144) when no procedure is
  running.
- **Halt and trace.** RexxSetHalt halts the procedure at its next clause
  (HALT condition or error 4); RexxSetTrace turns on interactive tracing
  (`?R`), RexxResetTrace turns tracing off; with thread id 0 all REXX
  threads of the process. Ctrl+Break and Ctrl+C (the XCPT_SIGNAL
  exception) halt the running procedure.
- **Stack.** The interpreter runs on a 16 MB stack of its own per thread
  (committed as touched): callers' stacks can be small and REXX
  recursion goes deep (2000 nested calls before error 11).
- **The environment**: VALUE(name, value, 'OS2ENVIRONMENT') and
  SETLOCAL/ENDLOCAL change the process's environment block (in place,
  else a new one the PIB then points to), so commands run afterwards see
  the change; rs2's CMD takes its variables back from the block after a
  procedure and around each command it runs for it.
- **16-bit entry points**: REXXSAA converts 16-bit RXSTRINGs (far
  pointers), 6-byte RXSYSEXIT entries and the result as IBM's
  `RexxStart32` does; RXVAR converts SHVBLOCKs. IBM's 16-bit CMD.EXE
  loads REXX, gets RXBREAKCLEANUP and REXXSAA, registers "CMD" with
  REXXAPI's RXSUBCOMREGISTER, and runs `.CMD` procedures on rs2's REXX.

## REXXAPI.DLL and REXXINIT.DLL

- **Queues** (RexxCreateQueue, RexxDeleteQueue, RexxAddQueue,
  RexxPullQueue, RexxQueryQueue) are system-wide: one named shared
  memory object (2 MB, DosSubAllocMem) behind one named mutex holds them.
  Names are 1 to 250 characters A-Z 0-9 `. ? ! _`, kept upper case;
  RexxCreateQueue without a name, or with one that exists (the duplicate
  flag set), makes `S<session>Q<counter>`; `SESSION` is the caller's
  session queue (the session number from `_RXQSESSION`, else the
  process's session) and always exists. RexxPullQueue with the wait flag
  waits on a named event semaphore for an entry; entries carry their
  time stamp; a null or short buffer gets memory from DosAllocMem. The
  return codes are IBM's (5 bad name, 9 no such queue, 8 empty...).
- **Registrations**: DLL registrations of functions, subcommand handlers
  and exits are system-wide (shared memory); registrations of entry
  points in an executable belong to the process. Names are matched
  without regard to case, and a DLL entry name is tried as given, then
  upper case (REXXUTIL registers `SysCls` for its export `SYSCLS`).
  Registering a function name that exists returns 10 and keeps the old
  one; a second subcommand or exit registration of a name returns 10 too
  (RXSUBCOM_DUP). RexxQuerySubcom/RexxQueryExit return the registration's
  8-byte user area. Deregistering a DLL registration made with drop
  authority RXSUBCOM_NONDROP from another process gives 40.
- **Calls**: RexxCallFunction(name, argc, argv, rc, result, queue),
  RexxCallSubcom(env, dll, command, flags, rc, result) and
  RexxCallExit(name, dll, function, subfunction, parameters) are IBM's
  undocumented entries with IBM's arguments (from REXXAPI's
  disassembly), so Object REXX's OREXX.DLL can use them too.
  Registrations made through the 16-bit API (RXSUBCOMREGISTER with an
  SCBLOCK, RXFUNCTIONREGISTER) are called as 16-bit procedures through
  `thunk16::call16`, with 16-bit RXSTRINGs.
- **The macro space** (RexxAddMacro ... RexxReorderMacro,
  RexxExecuteMacroFunction) is system-wide and keeps the macros' REXX
  source; RexxSaveMacroSpace writes rs2's own file format
  (`RS2 REXX MACROSPACE 1`), so files saved by IBM's REXX (its
  tokenized images) give RXMACRO_SIGNATURE_ERROR.
- **REXXINIT.DLL** (RxApiInit, dllinit) sets up the shared state when
  loaded; rs2's kernel loads it at boot through SYSDLLS.EXE, which stays
  resident, so queues outlive the programs that use them (`dir |
  rxqueue` and a later procedure that reads the queue).

## REXXUTIL

rs2's REXXUTIL.DLL (`userland/rexxutil`) is the REXX utility package
with IBM Warp 4.52's 85 functions under IBM's ordinals and upper-case
export names. `SysLoadFuncs` registers them, in IBM's order and under
their mixed-case names, with RexxRegisterFunctionDll ("REXXUTIL");
`SysDropFuncs` drops them. Each function follows IBM's REXXUTIL, studied
in Ghidra: the same argument checks (REXX error 40 for an incorrect
call), result strings ("0"/"-1" for the stem functions, "1"/"0" for the
Workplace functions, return codes elsewhere, "ERROR:" from SysIni) and
stem layouts. SysFileTree lines are `m/dd/yy hh:mmA size ADHRS name`, or
`yy/mm/dd/hh/mm` with T and `yyyy-mm-dd hh:mm:ss` with L; it takes the
options F/D/B/O/S/T/L and `+-*` attribute masks. SysFileSearch takes C/N
and splits lines at 4095 characters. SysGetKey returns an extended key
as 00 or E0 followed by its scan code. SysSleep takes up to three
decimals. The file functions write stems with RXSHV_SET under
upper-cased names; the SysStem functions use symbolic names, so a stem
may be given in any case. Results longer than the interpreter's 256-byte
buffer are returned in memory from DosAllocMem. The profile, Workplace
Shell, switch list and shutdown functions load PMWIN, PMSHAPI and PMWP
at run time and call them by ordinal, so programs that use none of them
never load PM. Known differences from IBM's: SysNationalLanguageCompare
on characters above 127 (IBM's indexes the collating table with a
signed character), and SysQueryClassList returns the number of classes
where IBM's returns a stale stack address.

## REXX tools

- **RXQUEUE.EXE** and **RXSUBCOM.EXE** (`userland/rexxcmds`) behave as
  IBM's: RXQUEUE copies standard input into a queue (named, `%RXQUEUE%`,
  else SESSION; /FIFO, /LIFO, /CLEAR; lines end at CR, LF or CR LF, cut
  at 4096 characters, Ctrl+Z ends the input); errors are REX.MSG
  messages 119-124 or OSO001.MSG 87/1003 on standard error, with return
  code 0. RXSUBCOM REGISTER/DROP/QUERY/LOAD return the REXXAPI call's
  code (rc); usage errors are REX0115-0118/0125 with rc -1.
- **PMREXX.EXE** and **PMREXXIO.DLL** (`userland/pmrexx`,
  `userland/pmrexxio`, help `userland/pmrexxio/PMREXXIO.IPF`) run a
  procedure in a PM window. RexxCreateIOWindow puts handles 0-2 on pipes
  read and written by two threads, so SAY and PULL use the standard
  handles. The PMREXXIO RXSIO exit answers trace pauses (RXSIODTR) and
  shows the window at reads (RXSIOTRD). PMREXX's RXINI exit sets trace
  for /T; Halt and Trace call RexxSetHalt/RexxSetTrace/RexxResetTrace for
  the REXX thread. Menu ids, WM_USER messages, the exit name and OS2.INI
  keys ("PMREXX Interface": UserFont, WindowState) are IBM's, so IBM's
  PMREXX.EXE runs on rs2's DLL.
- **REXXTRY.CMD** runs the instructions given on its command line, or
  reads instructions line by line and runs each (variables keep their
  values; `?` shows help, EXIT or the end of input ends). It is rs2's own
  program, not IBM's.

## Messages

`userland/msg/REX.TXT` and `REXH.TXT` are MKMSGF sources with the
numbers and meanings REXX programs and tools expect (1-49 the REXX
errors, 100 the "Error n running FILE, line L:" frame, 102-113 month
names, 114 the interactive-trace banner, 115-125 RXSUBCOM and RXQUEUE,
126 interactive-trace errors) and rs2's own wording. xtask compiles them
with crates/msgf into `\OS2\SYSTEM\REX.MSG` and `REXH.MSG` (MSG_FILES).
An error report is message `n` with message 100 inserted as `%1`
(`REX0035: Error 35 running C:\X.CMD, line 3: The expression is not
valid`), written with DosPutMessage (wrapped at 80 columns, as IBM's);
REXX.DLL carries the same texts for when REX.MSG is missing. ERRORTEXT
returns the message without its prefix.

## Differences from IBM's REXX

- IBM's REXX stores a procedure's tokenized form in its `.CMD` file's
  extended attributes (REXX.METACONTROL, REXX.LITERALPOOL,
  REXX.VARIABLEBUF, REXX.TOKENSIMAGE) and runs that when the source is
  unchanged; rs2's always runs the source and neither reads nor writes
  them. In-memory images (RexxStart's `instore[1]`) and saved macro
  spaces are rs2's own formats.
- RexxSetHalt/RexxSetTrace reach only threads of the calling process.
- 16-bit exits (registered through RXEXITREGISTER) are not called.

## Object REXX

rs2's Object REXX is the language of IBM's Object REXX for OS/2 6.00
(`PARSE VERSION` gives `OBJREXX 6.00 18 May 1999`), taken from OREXX.INF
(the reference), IBM's module export tables and message file (numbers),
and runs of IBM's Object REXX on rs2 that the tests compare with.
Nothing of IBM's is shipped: the interpreter, the class library, the
DLLs, the messages (rs2's wording, IBM's numbers) and the tools are
rs2's own.

**One interpreter.** Object REXX is a superset of classic REXX, so it is
the same crate: `Interp::new` runs classic REXX as before, `Interp::new_oo`
Object REXX (the `w` field holds the objects; the lexer and parser take
an `oo` flag for `~`, `~~`, `[`, `]`, environment symbols and the new
instructions; Object REXX's code is reached only through the table of
entry points its world holds, so classic REXX.DLL does not link it).
Values are strings or references to objects
(`crates/rexx/src/value.rs`); variable pools and stems are shared (`Rc`) so
that a method's pool can have the object's variables as its parent
(EXPOSE) and a stem can be passed by reference (USE ARG). Classic REXX
behaves as before: RXLANG passes on both (with the expected differences:
the version, and Object REXX's one-blank `QUERY DATETIME`).

- **Programs and directives** (`oo.rs`): a program is split at its
  directives into the main code and code units (`parse::sections`).
  `::CLASS` (PUBLIC, SUBCLASS, MIXINCLASS, INHERIT, METACLASS, EXTERNAL),
  `::METHOD` (CLASS, ATTRIBUTE, PRIVATE, UNGUARDED, PROTECTED),
  `::ROUTINE` (PUBLIC), `::REQUIRES` (the file is loaded and its main code
  run once; its public classes and routines are seen). Classes are made
  superclasses first, then each gets INIT. Functions are found as IBM
  documents: internal labels, built-in functions, ::ROUTINEs, public
  ones of required files, then external ones.
- **Messages**: `r~msg:scope(args)`, `r~~msg`, `r[args]`, `r~msg = v`,
  `r[i] = v`, message instructions (RESULT set or dropped). The method
  search goes through the object's own methods (SETMETHOD, ENHANCED), its
  class and the superclasses (mixins first, shared ancestors last), class
  methods of class objects then their metaclass; UNKNOWN, else NOMETHOD,
  else error 97.1. Operators with an object on the left are messages to
  it (.Object knows only the comparisons, so `.nil + 1` is error 97.1 as
  in IBM's); a string on the left takes the object's STRING.
- **Instructions**: USE ARG, EXPOSE, GUARD ON/OFF [WHEN], REPLY, RAISE
  (with ADDITIONAL, ARRAY, DESCRIPTION, RETURN, EXIT, PROPAGATE: as IBM's,
  RAISE without RETURN raises in the caller of the program, method or
  ::ROUTINE), FORWARD (TO, MESSAGE, ARGUMENTS, ARRAY, CLASS, CONTINUE),
  `DO name OVER collection`, SIGNAL/CALL ON NOMETHOD, NOSTRING, LOSTDIGITS,
  ANY and USER conditions; CONDITION('O') and 'A'.
- **Classes** (`classes.rs`): .Object, .Class, .String (every string
  method, the operators), .Array (several dimensions), .List, .Queue,
  .Directory, .Table, .Relation, .Set, .Bag, .Stem, .Supplier, .Message,
  .Method, .Monitor, .Stream, .Alarm, and .environment, .local (.STDIN,
  .STDOUT, .STDERR and the .INPUT, .OUTPUT, .ERROR monitors), .nil, .true,
  .false, .methods. CHANGESTR and COUNTSTR are built-in functions.
- **Activities.** START, REPLY (the rest of the method on a new
  activity), alarms and the waits of GUARD WHEN and Message~RESULT go
  through the host: OREXX.DLL runs activities on threads that take turns
  under one lock per process (every 64 clauses and while waiting), and
  RexxStart waits for a program's activities before it returns. Guarded
  methods keep the object to their activity. A host without activities
  (the default) runs a started message at once and a replying method to
  its end before its sender goes on.
- **Errors** are IBM's numbers and subcodes; untrapped, an error leaves
  the methods and routines it happened in (any of them may trap it with
  SIGNAL ON SYNTAX) and is reported as IBM's Object REXX does: the
  clauses as written, innermost first, then
  `REX0042E: Error 42 running C:\X.CMD line 8:  <message 42>` and
  `REX0413E: Error 42.3:  <message 413>` (the secondary messages are
  `userland/msg/OREX.TXT`, which the crate reads too; `msgs.rs` has
  IBM's subcode-to-message numbers). Traces show clauses as written,
  indented by their nesting.

**OREXX.DLL** is REXX.DLL built with `cfg(orexx)` (`userland/orexx`): the
same API and ordinals as IBM's OREXX.DLL for RexxStart, RexxVariablePool,
the halt and trace calls and the 16-bit entries, plus
RexxTranslateProgram (ordinal 24), which REXXC uses. Its LX module name is
REXX (xtask's `LX_NAMES`), so it becomes REXX.DLL by renaming. IBM's other
OREXX.DLL exports (the native-method API, RexxCreateMethod...) are not
there: no program on the CD uses them.

**REXXC.EXE** (`REXXC program [output] [/S]`) checks a program with
RexxTranslateProgram and writes rs2's tokenized form: the program behind
a `/**/` line and a Ctrl+Z (so CMD runs it as REXX and TYPE shows
nothing), scrambled. REXX.DLL runs such files (RexxStart, external
routines, ::REQUIRES); IBM's tokenized images are not read.

**REXXSOM.DLL** (`userland/rexxsom`): `.SOM~import('Class')` gives a SOM
class (the class manager finds or loads it; the interface repository's
classes once the repository is made), `::CLASS x EXTERNAL 'SOM Class'`
the same, `class~new` is somNew. A message to a SOM object calls the
method of that name in any case (somGetNthMethodInfo, somResolveByName)
with a signature from `userland/rxsom` (SOMObject's, SOMClass's,
SOMClassMgr's, the repository's) or REXXSOM's (the DLF classes): strings,
numbers, somIds, objects, doubles; INOUT/OUT arguments take a DLF object.
The DLF classes (DLFLong, DLFULong, DLFShort, DLFUShort, DLFFloat,
DLFDouble, DLFChar, DLFOctet, DLFBoolean, DLFString, DLFObject) are SOM
classes with IBM's names and class data exports. Methods not in the
tables give error 98.931 (rs2 does not read signatures from the interface
repository yet).

**REXXWPS.DLL** (`userland/rexxwps`) and **WPSINST.CMD**: WPSINST
registers the classes WPZORXINIT and WPZORX (WPAbstract subclasses) with
the running shell; their loading starts a server thread in the shell on
`\PIPE\RS2REXXWPS`. In REXX programs, `.WPS`, `.WPDESKTOP` and the
shell's classes by name (`.WPFOLDER`...) are objects whose messages the
server runs on the shell's SOM objects (an object is its address in the
shell); `class~new(title, setup, folder, lock)` is wpclsNew. The
Workplace Shell's method signatures are in `userland/rexxwps`.

**Differences from IBM's Object REXX.** IBM's keeps its class library
as REXX code in REXX.IMG and a tokenized image of each program in its
extended attributes; rs2's needs neither (and does not read IBM's REXXC
output). OREXX.DLL has no native-method API (REXX_* exports,
RexxCreateMethod, RexxSendMessage...), so IBM's OREXXSC.DLL (OpenDoc) and
other DLLs written against it do not load on it. SOM method signatures
come from rs2's tables, not the interface repository. SAY writes to
standard output whatever `.OUTPUT` is set to. A stream object shares the
open stream of its name with the stream functions. Objects collected by
reference counting: cycles are kept until the program ends, and UNINIT is
not run.

**Switching.** rs2 installs classic REXX as the system's REXX and Object
REXX beside it. `SWITCHRX.CMD` (rs2's own; `SWITCHRX Y` without asking)
renames the files: REXX.DLL, REXXUTIL.DLL, REX.MSG and REXH.MSG of the
REXX that is installed to C... (classic) or their O... names, and the
other REXX's to the system names (OREXXSOM.DLL and OREXXWPS.DLL to
REXXSOM.DLL and REXXWPS.DLL). Which REXX is installed is read from the
files (CREXX.DLL: Object REXX is). Programs already running keep the
REXX they loaded: the switch takes effect when the system restarts.

## Tests

`tests/rexx/RXLANG.CMD` (with `RXLANG2.CMD`, an external routine): 286
checks of the language and the built-in functions (arithmetic and
formatting, comparisons, every built-in function, stems and DROP,
control structures, procedures and EXPOSE, external routines, PARSE,
the queue and RXQUEUE, streams and STREAM, conditions, commands and RC,
INTERPRET, VALUE, the environment, TRACE(), RXFUNC*). It prints the
failures and `RXLANG: n passed, m failed` (`RXLANG V` lists every
check). It passes on IBM's REXX (IBM's CMD, REXX DLLs from the current
directory) and on rs2's REXX with IBM's CMD and with rs2's CMD:
`cargo xtask run --init "CMD.EXE /C RXLANG.CMD"`, with
`tests/rexx/rxlang.cmd -> RXLANG.CMD` and
`tests/rexx/rxlang2.cmd -> RXLANG2.CMD` in work/modules.txt.
`tests/rexx/RXUTEST.CMD` (REXXUTIL) and `RXQTEST.CMD` (RXQUEUE,
RXSUBCOM) are described above; `cargo test -p rexx` runs the
interpreter's own tests on the host.

Object REXX: `tests/rexx/ORXTEST.CMD` (52 checks, with `ORXREQ.CMD`),
`ORXSOM.CMD` (14, SOM), `ORXWPS.CMD` (8, the Workplace Shell: run with
`RS2\WPSTEST.EXE SHELL RUN C:\OS2\CMD.EXE /C ORXWPS.CMD`) and rs2's own
`ORXLANG.CMD` (105: message syntax, classes and mixins, private methods,
SETMETHOD, DEFINE, ENHANCED, UNKNOWN and NOMETHOD, the collections,
conditions and RAISE, messages and activities, the environment, streams)
pass on rs2's Object REXX with no IBM REXX files; ORXLANG passes on
IBM's too. The host tests (`crates/rexx/src/tests_oo.rs`) run ORXTEST's
language checks, ORXLANG (activities on threads), and
`crates/rexx/testdata/probe2.cmd`, whose output is compared with IBM's
(`probe2.out`, IBM's message texts left out). To run a program on
Object REXX without switching: put OREXX.DLL (as REXX.DLL), OREXUTIL.DLL
(as REXXUTIL.DLL), OREXXSOM.DLL (as REXXSOM.DLL) and OREX.MSG (as
REX.MSG) in a directory, make it the current one, add it to DPATH and run
`CMD /C program` there (LIBPATH starts with `.`).
