.* CMDREF.INF: rs2's Command Reference.
.*
.* Written for rs2; it takes the place of OS/2's Command Reference and
.* describes the commands rs2 provides (CMD.EXE's internal commands in
.* userland/cmd and the programs in userland/utils and elsewhere), as they
.* behave in rs2. Compile it with crates/ipf (ipfc example) into
.* OS2\BOOK\CMDREF.INF; VIEW CMDREF topic opens the panel whose title
.* (else an index entry) contains the topic, so every command panel is
.* titled with the command's name and its other names are index entries.
.*
.* Titles are searched as substrings in book order: keep the words of the
.* chapter titles free of command names (no "redirection", "several",
.* "overview" ...), and keep PATH before DPATH, SET before SETLOCAL and so
.* on.
:userdoc.
:title.rs2 Command Reference
.*
.* ==== About this book ====
:h1 res=100 id=about.About this book
:i1.Command Reference
:i1.commands, list of
:p.This book describes the commands you can type at an rs2 command prompt
or put in a batch file&colon. the commands built into the command
interpreter, CMD.EXE, and the programs that come with rs2.
:p.Each command has a panel of its own, named after the command. To go
straight to it from a command prompt, type :hp2.VIEW CMDREF:ehp2. followed
by the name of the command, for example&colon.
:sl.
:li.:hp4.VIEW CMDREF XCOPY:ehp4.
:esl.
:p.The book has these parts&colon.
:ul.
:li.:link reftype=hd refid=syntax.How to read the syntax:elink. explains the
way commands are written in this book.
:li.:link reftype=hd refid=using.Using commands:elink. explains the command
line&colon. file names, wildcards, environment variables, pipes, output to
files and how the system finds programs.
:li.:link reftype=hd refid=batch.Batch files:elink. explains command files
and REXX procedures.
:li.:link reftype=hd refid=errors.Errors and messages:elink. explains the
SYSnnnn messages and how to get more information about them.
:li.:link reftype=hd refid=commands.Commands:elink. lists every command.
:eul.
.*
.* ==== Syntax conventions ====
:h1 res=110 id=syntax.How to read the syntax
:i1.syntax conventions
:p.The syntax of each command is shown on lines like this one&colon.
:sl.
:li.:hp4.DIR:ehp4. [drive:][path][name] [/W] [/B] [/S] [/P] [/A]
:esl.
:p.Read it this way&colon.
:dl tsize=16.
:dt.UPPER CASE
:dd.Words in capitals are typed as they are shown, although the system does
not care whether you type them in capitals or small letters.
:dt.lower case
:dd.Words in small letters stand for something you supply, such as the name
of a file or a drive letter.
:dt.[ ]
:dd.What is between square brackets may be left out. Do not type the
brackets.
:dt.|
:dd.A vertical bar separates choices; type one of them. Do not type the bar
(except where the panel says it is the pipe character).
:dt....
:dd.Three dots mean that the item before them may be repeated.
:dt./X
:dd.A slash and a letter is a switch, which changes the way the command
works. Switches may be typed in capitals or small letters.
:edl.
:p.In the parameter lists, :hp1.drive:ehp1. means a drive letter with its
colon, such as C&colon., and :hp1.path:ehp1. means a list of directory names
separated by backslashes, such as \OS2\HELP. A path that starts with a
backslash starts at the root directory of the drive; any other path starts
at the current directory.
.*
.* ==== Using commands ====
:h1 res=120 id=using.Using commands
:i1.command line
:i1.command prompt
:p.When rs2 shows a command prompt, it is waiting for you to type a
command. Type the command and its parameters, then press Enter. The command
interpreter runs it and shows the prompt again when it has finished.
:p.A command is either an :hp2.internal command:ehp2., one that the command
interpreter carries out itself (such as DIR, COPY or SET), or the name of a
program or batch file (such as XCOPY, CHKDSK or a .CMD file of your own).
Internal commands are always available; programs are looked for along the
PATH.
:p.The command name and its parameters are separated by blanks. Commas,
semicolons and tabs also separate parameters. Put a parameter in double
quotation marks if it contains blanks, for example a file name such as
"My Notes.TXT".
:p.Some commands may be typed directly in front of their parameters without
a blank&colon. :hp4.CD\:ehp4., :hp4.CD..:ehp4., :hp4.ECHO.:ehp4. and
:hp4.DIR/W:ehp4. work as you would expect.
:p.To change the current drive, type its letter and a colon, for example
:hp4.A&colon.:ehp4..
:p.See also&colon.
:ul.
:li.:link reftype=hd refid=names.File names and wildcards:elink.
:li.:link reftype=hd refid=envvars.Environment variables:elink.
:li.:link reftype=hd refid=pipes.Pipes and output to files:elink.
:li.:link reftype=hd refid=separators.Command separators:elink.
:li.:link reftype=hd refid=search.How programs are found:elink.
:li.:link reftype=hd refid=configsys.CONFIG.SYS:elink.
:li.:link reftype=hd refid=cmdexe.Command interpreter:elink.
:eul.
.*
:h2 res=121 id=names.File names
:i1.wildcards
:i1.file names
:i1.global file-name characters
:p.A file is named by its drive, its path and its name&colon. for example
C&colon.\OS2\HELP\CMDREF.INF. If you leave out the drive, the current drive
is used; if you leave out the path, the current directory of that drive is
used.
:p.On HPFS and JFS drives names may be long and may contain blanks and
several periods; on FAT drives names have up to eight characters, a period
and up to three characters of extension. Capitals and small letters name the
same file.
:p.Two special directory names are available everywhere&colon. a single
period (.) means the current directory, two periods (..) the directory above
it.
:p.Many commands accept :hp2.wildcards:ehp2. in a file name, to work on
several files at once&colon.
:dl tsize=6.
:dt.*
:dd.Stands for any number of characters, including none. *.TXT means every
file whose name ends in .TXT; * alone means every file.
:dt.?
:dd.Stands for any one character. REPORT?.TXT matches REPORT1.TXT and
REPORTS.TXT.
:edl.
:p.DIR, COPY, DEL, TYPE, ATTRIB, XCOPY, FOR and IF EXIST accept wildcards.
REN and MOVE do not.
.*
:h2 res=122 id=envvars.Environment variables
:i1.environment variables
:i1.variables
:p.The :hp2.environment:ehp2. is a list of names and values that every
program inherits from the program that started it. The command interpreter
keeps its own copy, which you change with
:link reftype=hd refid=set.SET:elink., and gives it to every program it
runs.
:p.Variables that rs2 uses include&colon.
:dl tsize=16.
:dt.PATH
:dd.The directories searched for programs and batch files.
:dt.DPATH
:dd.The directories some programs search for their data files.
:dt.PROMPT
:dd.The command prompt (see PROMPT).
:dt.COMSPEC
:dd.The command interpreter that other programs start.
:dt.HELP
:dd.Where the help files are.
:dt.BOOKSHELF
:dd.Where VIEW looks for books such as this one.
:dt.HOSTNAME
:dd.The name of the computer on the network.
:edl.
:p.Three names are not variables but settings of the command interpreter's
process&colon. BEGINLIBPATH and ENDLIBPATH (directories searched for DLLs
before and after the LIBPATH of CONFIG.SYS) and LIBPATHSTRICT. SET changes
them, and the programs started afterwards inherit them.
:p.In a command, :hp2.%name%:ehp2. is replaced by the value of the variable
before the command runs; a name that is not set is replaced by nothing. For
example&colon.
:sl.
:li.:hp4.ECHO %PATH%:ehp4.
:li.:hp4.SET PATH=%PATH%;D&colon.\TOOLS:ehp4.
:esl.
:p.To put a percent sign in a command, type it twice&colon. %%.
.*
:h2 res=123 id=pipes.Pipes and output to files
:i1.pipes
:i1.redirecting input and output
:i1.standard input and output
:p.Every program has a :hp2.standard input:ehp2. (normally the keyboard),
a :hp2.standard output:ehp2. and a :hp2.standard error output:ehp2.
(normally both the screen). These are handles 0, 1 and 2. On the command
line you can send them elsewhere&colon.
:dl tsize=16.
:dt.> file
:dd.Write the output to the file, replacing what was in it.
:dt.>> file
:dd.Add the output to the end of the file (creating it if needed).
:dt.< file
:dd.Read the input from the file.
:dt.2> file
:dd.Write the error messages to the file. Any handle number may be put in
front of > or <.
:dt.2>&amp.1
:dd.Send handle 2 where handle 1 goes (so that errors go to the same file
as the output). Write it after the redirection of handle 1.
:dt.a | b
:dd.A :hp2.pipe:ehp2.&colon. the output of command a becomes the input of
command b. Several commands may be joined this way.
:edl.
:p.Examples&colon.
:sl.
:li.:hp4.DIR /S > FILES.LST:ehp4.
:li.:hp4.DIR | SORT | MORE:ehp4.
:li.:hp4.CHKDSK C&colon. > REPORT.TXT 2>&amp.1:ehp4.
:li.:hp4.SORT < NAMES.TXT >> SORTED.TXT:ehp4.
:esl.
:p.Put a file name with blanks in double quotation marks. To use one of the
characters < > | &amp. as an ordinary character, put a caret (^) in front of
it, or put it inside double quotation marks.
:p.Programs that work well at the end of a pipe are
:link reftype=hd refid=find.FIND:elink.,
:link reftype=hd refid=sort.SORT:elink. and
:link reftype=hd refid=more.MORE:elink..
.*
:h2 res=124 id=separators.Command separators
:i1.command separators
:i1.conditional commands
:p.One line may hold more than one command&colon.
:dl tsize=16.
:dt.a &amp. b
:dd.Run a, then b.
:dt.a &amp.&amp. b
:dd.Run b only if a succeeded (ended with return code 0).
:dt.a || b
:dd.Run b only if a failed (ended with a return code other than 0).
:edl.
:p.Example&colon.
:sl.
:li.:hp4.MD \BACKUP &amp.&amp. XCOPY \WORK \BACKUP /S:ehp4.
:esl.
.*
:h2 res=125 id=search.How programs are found
:i1.search path
:i1.file extensions
:p.When the command word is not an internal command, the command
interpreter looks for a file of that name, first in the current directory
and then in each directory of the PATH variable, in order. If the name has
no extension, it tries the extensions .COM, .EXE, .CMD and .BAT, in that
order, in each directory. If the name contains a drive or a path, only that
place is searched.
:p.What happens next depends on the file&colon.
:ul.
:li.An OS/2 program (.EXE or .COM) runs in the same session, and the
command interpreter waits for it to end. Its return code is kept for IF
ERRORLEVEL and the &amp.&amp. and || separators.
:li.A .CMD file runs as a
:link reftype=hd refid=batch.batch file:elink., or as a
:link reftype=hd refid=rexx.REXX procedure:elink. if its first line starts
with /*.
:li.A DOS program, and any .BAT file, runs in a DOS session of its own.
:eul.
:p.If nothing is found, the message SYS1041 says that the name is not a
command, program or batch file.
:p.To run a program in another session, or without waiting for it, use
:link reftype=hd refid=start.START:elink. or
:link reftype=hd refid=detach.DETACH:elink..
.*
:h2 res=126 id=configsys.CONFIG.SYS
:i1.CONFIG.SYS
:i1.LIBPATH
:i1.PROTSHELL
:p.CONFIG.SYS, in the root directory of the drive rs2 starts from, sets up
the system when it starts. Each line is a statement; lines that start with
REM are comments. Changes take effect when the system is started again. You
can edit it with the System Editor&colon. :hp4.E C&colon.\CONFIG.SYS:ehp4..
:p.The statements rs2 reads include&colon.
:dl tsize=16.
:dt.SET name=value
:dd.Puts a variable in the environment that every program starts with, for
example SET PATH= and SET PROMPT=.
:dt.LIBPATH=
:dd.The directories searched for dynamic link libraries (DLLs).
:dt.PROTSHELL=
:dd.The program that is started as the user interface&colon. PMSHELL.EXE
for the desktop, or CMD.EXE for a text-mode command prompt.
:dt.RUN=
:dd.A program started in the background while the system starts.
:dt.VIDEO=
:dd.The screen resolution (written by DISPLAY).
:edl.
:p.Other statements found in OS/2's CONFIG.SYS (device drivers and the like)
are accepted and ignored where rs2 has no use for them.
.*
:h2 res=127 id=cmdexe.Command interpreter
:i1.CMD.EXE
:i1.command interpreter
:i1.OS/2 window
:p.CMD.EXE is the command interpreter&colon. it shows the prompt, reads
commands and runs them. Opening an OS/2 window or OS/2 full screen session
starts one, and you can start another from a command prompt.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.CMD:ehp4. [/C command | /K command | command]
:esl.
:dl tsize=12.
:dt./C command
:dd.Run the command (and the batch file it names, to its end), then end.
The return code is the command's.
:dt./K command
:dd.Run the command, then go on reading commands.
:dt.command
:dd.Without /C or /K, the same as /K.
:edl.
:p.Use :link reftype=hd refid=exit.EXIT:elink. to end a command
interpreter.
.*
.* ==== Batch files ====
:h1 res=130 id=batch.Batch files
:i1.batch files
:i1.command files
:i1.labels
:p.A :hp2.batch file:ehp2. is a text file of commands with the
extension .CMD. Type its name (with or without .CMD) to run it&colon. the command
interpreter carries out its lines one after another, as if you had typed
them.
:p.Things to know about batch files&colon.
:ul.
:li.Each line is shown on the screen before it runs, unless
:link reftype=hd refid=echo.ECHO OFF:elink. has been used. A line that
starts with @ is never shown, so the first line is often
:hp4.@ECHO OFF:ehp4..
:li.A line that starts with a colon is a :hp2.label:ehp2., a place to go to
with :link reftype=hd refid=goto.GOTO:elink.; it is not run. Labels are
compared without regard to case.
:li.:link reftype=hd refid=rem.REM:elink. lines are comments.
:li.Running another batch file by its name ends the first one. To come back
afterwards, use :link reftype=hd refid=call.CALL:elink..
:li.:link reftype=hd refid=exit.EXIT:elink. ends the batch file and the
command interpreter that runs it.
:li.A Ctrl+Z character ends the file.
:li.If the first line starts with /*, the file is a
:link reftype=hd refid=rexx.REXX procedure:elink. instead.
:eul.
:p.The commands made for batch files are
:link reftype=hd refid=call.CALL:elink.,
:link reftype=hd refid=echo.ECHO:elink.,
:link reftype=hd refid=endlocal.ENDLOCAL:elink.,
:link reftype=hd refid=extproc.EXTPROC:elink.,
:link reftype=hd refid=for.FOR:elink.,
:link reftype=hd refid=goto.GOTO:elink.,
:link reftype=hd refid=if.IF:elink.,
:link reftype=hd refid=pause.PAUSE:elink.,
:link reftype=hd refid=rem.REM:elink.,
:link reftype=hd refid=setlocal.SETLOCAL:elink. and
:link reftype=hd refid=shift.SHIFT:elink.. Every other command can be used
too.
:p.An example, BACKUP.CMD&colon.
:sl.
:li.@ECHO OFF
:li.REM Copy a directory tree to drive D&colon.
:li.IF "%1"=="" GOTO NOARG
:li.XCOPY %1 D&colon.\BACKUP\%1 /S /E
:li.IF ERRORLEVEL 1 ECHO Some files were not copied.
:li.GOTO END
:li.&colon.NOARG
:li.ECHO Type BACKUP followed by the name of a directory.
:li.&colon.END
:esl.
.*
:h2 res=131 id=params.Batch file parameters
:i1.%0 - %9
:i1.replaceable parameters
:p.The words typed after the name of a batch file are its
:hp2.parameters:ehp2.. In the batch file, %1 stands for the first one, %2 for
the second and so on up to %9; %0 is the name of the batch file itself. A
parameter that was not given is replaced by nothing.
:p.If you type :hp4.COPYALL REPORTS A&colon.:ehp4., then in COPYALL.CMD
%1 is REPORTS and %2 is A&colon..
:p.Parameters are separated by blanks, commas or semicolons; put one in
double quotation marks to include blanks in it.
:p.For more than nine parameters, use
:link reftype=hd refid=shift.SHIFT:elink.. Environment variables can be used
too, as %name%; see
:link reftype=hd refid=envvars.Environment variables:elink.. A percent sign
in a batch file is written %%.
.*
:h2 res=132 id=rexx.REXX procedures
:i1.REXX
:i1.procedures, REXX
:p.A .CMD file whose first line starts with /* (the beginning of a REXX
comment) is a :hp2.REXX procedure:ehp2.. The command interpreter hands it to
the REXX interpreter (REXX.DLL) instead of reading it line by line. REXX is
a programming language with variables, loops, arithmetic and string
functions.
:p.A line of a REXX procedure that REXX does not understand as one of its
own instructions is passed back to the command interpreter as a command
(the CMD environment). Its return code is in the REXX variable RC.
:p.An example, HELLO.CMD&colon.
:sl.
:li./* Greet the user */
:li.SAY 'What is your name?'
:li.PULL name
:li.SAY 'Hello,' name
:li.'DIR /W'
:esl.
:p.REXX must be installed for REXX procedures to run. If it is not, the
file runs as an ordinary batch file.
.*
.* ==== Errors and messages ====
:h1 res=140 id=errors.Errors and messages
:i1.messages
:i1.SYSnnnn messages
:i1.return codes
:p.When a command cannot do what you asked, it shows a message that starts
with :hp2.SYS:ehp2. and a number, for example&colon.
:sl.
:li.SYS0002: The system cannot find the file specified.
:esl.
:p.The number identifies the message. To see more about a message, what
caused it and what to do about it, type :hp4.HELP:ehp4. and the number,
with or without SYS and leading zeros&colon.
:sl.
:li.:hp4.HELP 2:ehp4.
:li.:hp4.HELP SYS1041:ehp4.
:esl.
:p.The texts come from the message files OSO001.MSG and OSO001H.MSG in
C&colon.\OS2\SYSTEM. If they are not installed, commands show a short text
of their own and HELP cannot explain the message.
:p.Every command also ends with a :hp2.return code:ehp2.&colon. 0 when it
succeeded, another number when it did not (often the number of the
message). Batch files can test it with
:link reftype=hd refid=if.IF ERRORLEVEL:elink.; the command separators
&amp.&amp. and || test it too.
:p.Messages you may often see&colon.
:dl tsize=10.
:dt.SYS0002
:dd.The file was not found.
:dt.SYS0003
:dd.The path (a directory) was not found.
:dt.SYS0005
:dd.Access was denied, for example to a read-only file.
:dt.SYS0015
:dd.The drive does not exist.
:dt.SYS0087
:dd.A parameter is not correct.
:dt.SYS1003
:dd.The syntax of the command is not correct.
:dt.SYS1039
:dd.A batch file has no label of that name (GOTO).
:dt.SYS1041
:dd.The name is not a command, program or batch file.
:edl.
.*
.* ==== Commands ====
:h1 res=200 id=commands.Commands
:i1.commands
:p.rs2's commands, in alphabetical order. Commands marked
:hp2.(internal):ehp2. are carried out by the command interpreter itself;
the others are programs in C&colon.\OS2 (or C&colon.\OS2\INSTALL for the
installers).
:sl.
:li.:link reftype=hd refid=ansi.ANSI:elink. (internal)
:li.:link reftype=hd refid=appinst.APPINST:elink.
:li.:link reftype=hd refid=attrib.ATTRIB:elink.
:li.:link reftype=hd refid=call.CALL:elink. (internal)
:li.:link reftype=hd refid=cd.CD, CHDIR:elink. (internal)
:li.:link reftype=hd refid=chcp.CHCP:elink. (internal)
:li.:link reftype=hd refid=chkdsk.CHKDSK:elink.
:li.:link reftype=hd refid=cls.CLS:elink. (internal)
:li.:link reftype=hd refid=copy.COPY:elink. (internal)
:li.:link reftype=hd refid=date.DATE:elink. (internal)
:li.:link reftype=hd refid=del.DEL, ERASE:elink. (internal)
:li.:link reftype=hd refid=detach.DETACH:elink. (internal)
:li.:link reftype=hd refid=dir.DIR:elink. (internal)
:li.:link reftype=hd refid=display.DISPLAY:elink.
:li.:link reftype=hd refid=dpath.DPATH:elink. (internal)
:li.:link reftype=hd refid=e.E:elink. (System Editor)
:li.:link reftype=hd refid=echo.ECHO:elink. (internal)
:li.:link reftype=hd refid=endlocal.ENDLOCAL:elink. (internal)
:li.:link reftype=hd refid=exit.EXIT:elink. (internal)
:li.:link reftype=hd refid=extproc.EXTPROC:elink. (internal)
:li.:link reftype=hd refid=find.FIND:elink.
:li.:link reftype=hd refid=for.FOR:elink. (internal)
:li.:link reftype=hd refid=format.FORMAT:elink.
:li.:link reftype=hd refid=goto.GOTO:elink. (internal)
:li.:link reftype=hd refid=help.HELP:elink.
:li.:link reftype=hd refid=host.HOST:elink.
:li.:link reftype=hd refid=hostname.HOSTNAME:elink.
:li.:link reftype=hd refid=if.IF:elink. (internal)
:li.:link reftype=hd refid=ifconfig.IFCONFIG:elink.
:li.:link reftype=hd refid=install.INSTALL:elink.
:li.:link reftype=hd refid=keyb.KEYB:elink.
:li.:link reftype=hd refid=keys.KEYS:elink. (internal)
:li.:link reftype=hd refid=label.LABEL:elink.
:li.:link reftype=hd refid=md.MD, MKDIR:elink. (internal)
:li.:link reftype=hd refid=mode.MODE:elink.
:li.:link reftype=hd refid=more.MORE:elink.
:li.:link reftype=hd refid=move.MOVE:elink. (internal)
:li.:link reftype=hd refid=netstat.NETSTAT:elink.
:li.:link reftype=hd refid=path.PATH:elink. (internal)
:li.:link reftype=hd refid=pause.PAUSE:elink. (internal)
:li.:link reftype=hd refid=ping.PING:elink.
:li.:link reftype=hd refid=pminstal.PMINSTAL:elink.
:li.:link reftype=hd refid=prompt.PROMPT:elink. (internal)
:li.:link reftype=hd refid=rd.RD, RMDIR:elink. (internal)
:li.:link reftype=hd refid=rem.REM:elink. (internal)
:li.:link reftype=hd refid=ren.REN, RENAME:elink. (internal)
:li.:link reftype=hd refid=set.SET:elink. (internal)
:li.:link reftype=hd refid=setlocal.SETLOCAL:elink. (internal)
:li.:link reftype=hd refid=shift.SHIFT:elink. (internal)
:li.:link reftype=hd refid=sort.SORT:elink.
:li.:link reftype=hd refid=start.START:elink. (internal)
:li.:link reftype=hd refid=sysdlls.SYSDLLS:elink.
:li.:link reftype=hd refid=time.TIME:elink. (internal)
:li.:link reftype=hd refid=tree.TREE:elink.
:li.:link reftype=hd refid=type.TYPE:elink. (internal)
:li.:link reftype=hd refid=ver.VER:elink. (internal)
:li.:link reftype=hd refid=verify.VERIFY:elink. (internal)
:li.:link reftype=hd refid=view.VIEW:elink.
:li.:link reftype=hd refid=vol.VOL:elink. (internal)
:li.:link reftype=hd refid=xcopy.XCOPY:elink.
:esl.
.*
.* ---- ANSI ----
:h2 res=202 id=ansi.ANSI
:i1.ANSI
:i1.escape sequences
:p.Reports that ANSI screen control is on.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.ANSI:ehp4. [ON | OFF]
:esl.
:p.rs2's command sessions always understand ANSI escape sequences (the
characters Esc [ followed by a code, which move the cursor, clear the screen
and set colors), so ANSI always answers that they are on; ON and OFF change
nothing.
:p.Related&colon. :link reftype=hd refid=prompt.PROMPT:elink. ($E puts an
Esc character in the prompt), :link reftype=hd refid=cls.CLS:elink..
.*
.* ---- APPINST ----
:h2 res=204 id=appinst.APPINST
:i1.APPINST
:i1.applications, installing
:i1.games, installing
:p.Installs the OS/2 applications that rs2 can run (such as the games and
the productivity programs) from an OS/2 Warp 4.52 CD onto the boot drive.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.APPINST:ehp4. [drive:]
:esl.
:dl tsize=10.
:dt.drive:
:dd.The drive that holds the CD. Without it, APPINST looks for the CD
itself.
:edl.
:p.APPINST opens a window that lists the applications by category. Check a
category to choose all its applications, or check them one at a time; an
application that needs another (PM REXX needs REXX, for example) brings it
along. Select :hp2.Install:ehp2. to copy the files; a bar shows the
progress.
:p.Afterwards the applications have program objects&colon. with the
Workplace Shell, a folder per category in the Programs folder. If an
application needs C&colon.\OS2\APPS on the PATH or LIBPATH, APPINST adds it
to CONFIG.SYS; restart the system to use it.
:p.Related&colon. :link reftype=hd refid=install.INSTALL:elink..
.*
.* ---- ATTRIB ----
:h2 res=206 id=attrib.ATTRIB
:i1.ATTRIB
:i1.attributes of files
:i1.read-only files
:i1.hidden files
:p.Shows or changes the attributes of files.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.ATTRIB:ehp4. [+R | -R] [+A | -A] [+S | -S] [+H | -H] [files ...]
:esl.
:dl tsize=10.
:dt.+
:dd.Turns an attribute on.
:dt.-
:dd.Turns an attribute off.
:dt.R
:dd.Read-only&colon. the file cannot be changed or deleted.
:dt.A
:dd.Archive&colon. the file has changed since it was last backed up.
:dt.S
:dd.System&colon. a file of the operating system.
:dt.H
:dd.Hidden&colon. DIR does not list the file.
:dt.files
:dd.The files to show or change. Wildcards may be used. Without it, all the
files in the current directory.
:edl.
:p.Without + or -, ATTRIB lists the files with their attributes (hidden and
system files included). Several letters may follow one sign, as in
:hp4.ATTRIB +RH SECRET.TXT:ehp4.. The dates of the files are not changed.
:p.Related&colon. :link reftype=hd refid=dir.DIR:elink. (DIR /A shows hidden
files).
.*
.* ---- CALL ----
:h2 res=208 id=call.CALL
:i1.CALL
:p.Runs a batch file from another batch file and then continues with the
next line of the first one.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.CALL:ehp4. name [parameters]
:esl.
:dl tsize=12.
:dt.name
:dd.The batch file (.CMD) to run, found as other commands are.
:dt.parameters
:dd.Its parameters, %1 to %9 in the called batch file.
:edl.
:p.Without CALL, running a batch file from a batch file ends the first one.
If the name is not a batch file, CALL simply runs the command.
:p.Related&colon. :link reftype=hd refid=batch.Batch files:elink.,
:link reftype=hd refid=exit.EXIT:elink..
.*
.* ---- CD ----
:h2 res=210 id=cd.CD
:i1.CD
:i1.CHDIR
:i1.current directory
:p.Shows or changes the current directory. CHDIR is another name for CD.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.CD:ehp4. [drive:]
:li.:hp4.CD:ehp4. [drive:]path
:esl.
:dl tsize=10.
:dt.drive:
:dd.Alone, shows the current directory of that drive.
:dt.path
:dd.The directory to make current. .. is the directory above; \ is the root
directory.
:edl.
:p.Without parameters, CD shows the current drive and directory. Each drive
has its own current directory; changing it on another drive does not change
the current drive (type the drive letter and a colon for that).
:p.Examples&colon.
:sl.
:li.:hp4.CD \OS2\HELP:ehp4.
:li.:hp4.CD..:ehp4.
:li.:hp4.CD\:ehp4.
:esl.
:p.Related&colon. :link reftype=hd refid=md.MD:elink.,
:link reftype=hd refid=rd.RD:elink., :link reftype=hd refid=dir.DIR:elink..
.*
.* ---- CHCP ----
:h2 res=212 id=chcp.CHCP
:i1.CHCP
:i1.code pages
:p.Shows the code page in use, or switches to another prepared code page.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.CHCP:ehp4. [codepage]
:esl.
:dl tsize=12.
:dt.codepage
:dd.The number of a code page prepared for the system (such as 437 or 850).
:edl.
:p.Without a number CHCP shows the active code page and the prepared ones.
With a number it switches this session, its screen and keyboard to that code
page. A code page that is not prepared gives message SYS1767.
:p.Related&colon. :link reftype=hd refid=keyb.KEYB:elink..
.*
.* ---- CHKDSK ----
:h2 res=214 id=chkdsk.CHKDSK
:i1.CHKDSK
:i1.checking disks
:i1.disk errors
:p.Checks a disk for errors and reports how its space is used.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.CHKDSK:ehp4. [drive:] [/F] [/V] [/C] [/P]
:esl.
:dl tsize=10.
:dt.drive:
:dd.The drive to check. Without it, the current drive.
:dt./F
:dd.Repairs the errors found (FAT drives). The drive is locked while CHKDSK
works, so no files on it may be open.
:dt./V
:dd.Lists the files and directories as they are checked.
:dt./C
:dd.Accepted for compatibility; CHKDSK always checks.
:dt./P
:dd.Reports in the form the Presentation Manager disk checker reads (one
message number and its values per line). It is not meant to be typed.
:edl.
:p.CHKDSK reports the file system of the drive first. FAT drives are checked
fully and can be repaired with /F. HPFS drives get a check of their directory
tree and JFS drives a check of all their structures; on those CHKDSK reports
problems but does not repair them.
:p.Related&colon. :link reftype=hd refid=format.FORMAT:elink.,
:link reftype=hd refid=vol.VOL:elink..
.*
.* ---- CLS ----
:h2 res=216 id=cls.CLS
:i1.CLS
:i1.clearing the screen
:p.Clears the screen.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.CLS:ehp4.
:esl.
:p.The prompt appears again at the top of the screen.
.*
.* ---- COPY ----
:h2 res=218 id=copy.COPY
:i1.COPY
:i1.copying files
:i1.joining files
:p.Copies files, or joins several files into one.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.COPY:ehp4. source [destination]
:li.:hp4.COPY:ehp4. file1+file2[+...] destination
:esl.
:dl tsize=14.
:dt.source
:dd.The files to copy. Wildcards may be used; a directory means all the
files in it.
:dt.destination
:dd.Where to copy them&colon. a directory (the copies keep their names) or a
file name. Without it, the current directory.
:dt.+
:dd.Joins the files named, one after the other, into the destination file.
:edl.
:p.A file in the destination that has the same name is replaced. A file
cannot be copied onto itself. When several files are copied, their names are
listed; at the end COPY says how many files it copied. Switches such as /V
are accepted and ignored.
:p.COPY does not copy subdirectories; use
:link reftype=hd refid=xcopy.XCOPY:elink. for a whole directory tree.
:p.Examples&colon.
:sl.
:li.:hp4.COPY *.TXT A&colon.:ehp4.
:li.:hp4.COPY PART1.TXT+PART2.TXT WHOLE.TXT:ehp4.
:esl.
:p.Related&colon. :link reftype=hd refid=move.MOVE:elink.,
:link reftype=hd refid=xcopy.XCOPY:elink..
.*
.* ---- DATE ----
:h2 res=220 id=date.DATE
:i1.DATE
:p.Shows the date.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.DATE:ehp4.
:esl.
:p.DATE shows the day of the week and the date (month, day, year). rs2
takes the date from the computer's clock; DATE does not change it, and any
parameters are ignored.
:p.Related&colon. :link reftype=hd refid=time.TIME:elink.,
:link reftype=hd refid=prompt.PROMPT:elink. ($D).
.*
.* ---- DEL ----
:h2 res=222 id=del.DEL
:i1.DEL
:i1.ERASE
:i1.deleting files
:p.Deletes files. ERASE is another name for DEL.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.DEL:ehp4. files [files ...]
:esl.
:dl tsize=10.
:dt.files
:dd.The files to delete. Wildcards may be used. A directory means all the
files in it (the directory itself stays; see RD).
:edl.
:p.Before deleting all the files of a directory (* or *.*, or a directory
name), DEL asks :hp2.Are you sure (Y/N)?:ehp2.; type Y to go ahead.
Switches are accepted and ignored. Read-only files are not deleted; turn the
attribute off first with ATTRIB -R.
:p.Deleted files cannot be brought back by rs2.
:p.Related&colon. :link reftype=hd refid=rd.RD:elink.,
:link reftype=hd refid=attrib.ATTRIB:elink..
.*
.* ---- DETACH ----
:h2 res=224 id=detach.DETACH
:i1.DETACH
:i1.background programs
:p.Starts a program in the background, without a session of its own, and
returns to the prompt at once.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.DETACH:ehp4. program [parameters]
:esl.
:p.DETACH shows the process identification number of the new process. The
program cannot use the keyboard or the screen; send its output to a file if
you want to keep it.
:p.Related&colon. :link reftype=hd refid=start.START:elink..
.*
.* ---- DIR ----
:h2 res=226 id=dir.DIR
:i1.DIR
:i1.listing files
:p.Lists the files and subdirectories in a directory.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.DIR:ehp4. [drive:][path][name] ... [/W] [/B] [/S] [/P] [/A]
:esl.
:dl tsize=10.
:dt.name
:dd.The files to list; wildcards may be used. Without it, everything in the
directory.
:dt./W
:dd.Wide&colon. names only, five to a line; directories in square brackets.
:dt./B
:dd.Bare&colon. names only, one to a line, without the heading and totals.
:dt./S
:dd.Also lists the subdirectories, and theirs (with /B, the full names).
:dt./P
:dd.Pauses after each screen of the long listing; press Enter to go on.
:dt./A
:dd.Also lists hidden and system files.
:edl.
:p.The ordinary listing starts with the volume label and serial number, then
gives the date, time, size and name of each entry (<DIR> for a directory),
and ends with the number of files, the bytes they use and the bytes free on
the drive.
:p.Switches may be put together (:hp4.DIR /W/S:ehp4.) or typed straight after
DIR (:hp4.DIR/W:ehp4.).
:p.Related&colon. :link reftype=hd refid=tree.TREE:elink.,
:link reftype=hd refid=attrib.ATTRIB:elink.,
:link reftype=hd refid=names.File names and wildcards:elink..
.*
.* ---- DISPLAY ----
:h2 res=228 id=display.DISPLAY
:i1.DISPLAY
:i1.screen resolution
:p.Shows and changes the screen resolution of the desktop.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.DISPLAY:ehp4.
:li.:hp4.DISPLAY:ehp4. WxH[x256 | x16M]
:esl.
:dl tsize=10.
:dt.WxH
:dd.Switches to the resolution W by H pixels and keeps it, without asking.
:dt.x256
:dd.256 colors from a palette (some older programs need this).
:dt.x16M
:dd.16 million colors.
:edl.
:p.Without parameters DISPLAY opens a window listing the resolutions the
display can use. After :hp2.Apply:ehp2., choose :hp2.Keep:ehp2. within 15
seconds or the old resolution comes back.
:p.A kept resolution is saved as VIDEO= in CONFIG.SYS and used at the next
start. A change of colors takes effect at the next start.
:p.Related&colon. :link reftype=hd refid=mode.MODE:elink. (text screens).
.*
.* ---- E ----
:h2 res=232 id=e.E
:i1.E
:i1.System Editor
:i1.editing files
:p.Starts the System Editor, to edit a text file in a window.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.E:ehp4. [file]
:esl.
:dl tsize=10.
:dt.file
:dd.The file to edit. If it does not exist, the editor starts with an empty
file of that name.
:edl.
:p.The System Editor edits files made of lines of text, such as CONFIG.SYS
and batch files. Its own help (F1) describes its menus and keys.
:p.Related&colon. :link reftype=hd refid=type.TYPE:elink..
.*
.* ---- ECHO ----
:h2 res=234 id=echo.ECHO
:i1.ECHO
:i1.@ (echo off)
:p.Shows a message, or turns the showing of batch file commands on or off.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.ECHO:ehp4. [ON | OFF]
:li.:hp4.ECHO:ehp4. message
:li.:hp4.ECHO.:ehp4.
:esl.
:dl tsize=10.
:dt.ON
:dd.Batch file lines are shown, with the prompt, before they run.
:dt.OFF
:dd.Batch file lines are not shown.
:dt.message
:dd.Text to show.
:edl.
:p.ECHO alone tells whether echo is on or off. ECHO with a period straight
after it (ECHO.) shows an empty line.
:p.A line that starts with @ is not shown, whatever the ECHO setting. When a
batch file ends, ECHO goes back to what it was before the batch file
started.
:p.Related&colon. :link reftype=hd refid=batch.Batch files:elink.,
:link reftype=hd refid=rem.REM:elink..
.*
.* ---- ENDLOCAL ----
:h2 res=236 id=endlocal.ENDLOCAL
:i1.ENDLOCAL
:p.Brings back the environment, current drive and current directory saved by
the last SETLOCAL.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.ENDLOCAL:ehp4.
:esl.
:p.Without an earlier SETLOCAL, ENDLOCAL does nothing.
:p.Related&colon. :link reftype=hd refid=setlocal.SETLOCAL:elink..
.*
.* ---- EXIT ----
:h2 res=238 id=exit.EXIT
:i1.EXIT
:i1.closing a command prompt
:p.Ends the command interpreter.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.EXIT:ehp4. [code]
:esl.
:dl tsize=10.
:dt.code
:dd.The return code to end with. Without it, the return code of the last
command.
:edl.
:p.Typed at a prompt in an OS/2 window, EXIT closes the window. In a batch
file, EXIT ends the batch file and the command interpreter that runs it.
:p.Related&colon. :link reftype=hd refid=cmdexe.Command interpreter:elink..
.*
.* ---- EXTPROC ----
:h2 res=240 id=extproc.EXTPROC
:i1.EXTPROC
:p.Names another program to run a batch file.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.EXTPROC:ehp4. program [parameters]
:esl.
:p.On OS/2, EXTPROC as the first line of a batch file hands the file to
another batch processor. rs2 accepts the line and ignores it&colon. the
batch file is run by the command interpreter itself.
.*
.* ---- FIND ----
:h2 res=242 id=find.FIND
:i1.FIND
:i1.searching files for text
:p.Finds the lines of files, or of its input, that contain a string.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.FIND:ehp4. [/V] [/C] [/N] [/I] "string" [files ...]
:esl.
:dl tsize=10.
:dt."string"
:dd.The text to find, in double quotation marks.
:dt.files
:dd.The files to search. Without them, FIND reads its standard input, so it
can be used at the end of a pipe.
:dt./V
:dd.Shows the lines that do :hp2.not:ehp2. contain the string.
:dt./C
:dd.Shows only how many lines match.
:dt./N
:dd.Puts the line number, in square brackets, in front of each line.
:dt./I
:dd.Ignores the difference between capitals and small letters.
:edl.
:p.Each file's lines are shown after a line with its name. The return code is
0 if a line matched and 1 if none did.
:p.Examples&colon.
:sl.
:li.:hp4.FIND /I "libpath" C&colon.\CONFIG.SYS:ehp4.
:li.:hp4.DIR /B | FIND ".CMD":ehp4.
:esl.
:p.Related&colon. :link reftype=hd refid=sort.SORT:elink.,
:link reftype=hd refid=more.MORE:elink., :link reftype=hd refid=pipes.Pipes
and output to files:elink..
.*
.* ---- FOR ----
:h2 res=244 id=for.FOR
:i1.FOR
:i1.loops
:p.Runs a command once for each item of a set.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.FOR:ehp4. %%x :hp4.IN:ehp4. (set) :hp4.DO:ehp4. command
:esl.
:dl tsize=10.
:dt.%%x
:dd.The variable, a single letter. In a batch file write it with two percent
signs; at a prompt one is enough (%x).
:dt.set
:dd.The items, separated by blanks. An item with wildcards stands for the
files it matches.
:dt.command
:dd.The command to run, in which %%x is replaced by each item in turn.
:edl.
:p.Examples&colon.
:sl.
:li.:hp4.FOR %%F IN (*.TXT) DO TYPE %%F:ehp4.
:li.:hp4.FOR %D IN (A B C) DO MD %D:ehp4.
:esl.
:p.Related&colon. :link reftype=hd refid=batch.Batch files:elink..
.*
.* ---- FORMAT ----
:h2 res=246 id=format.FORMAT
:i1.FORMAT
:i1.file systems
:i1.FAT, HPFS, JFS
:p.Creates a new, empty file system on a disk partition.
:hp2.Everything on it is lost.:ehp2.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.FORMAT:ehp4. drive: [/FS&colon.FAT | /FS&colon.FAT32 | /FS&colon.HPFS
| /FS&colon.JFS] [/V&colon.label] [/Q]
:esl.
:dl tsize=12.
:dt.drive:
:dd.The drive to format. It cannot be the drive the system started from.
:dt./FS&colon.type
:dd.The file system&colon. FAT, FAT32, HPFS or JFS. Without it, the drive's
present file system, or FAT.
:dt./V&colon.label
:dd.The volume label for the new file system. Without it, FORMAT asks for one
at the end.
:dt./Q
:dd.Quick&colon. does not read the whole partition first to check it.
:edl.
:p.If the drive has a volume label, FORMAT first asks you to type it, then
asks you to confirm. The drive is locked while it is formatted, so no files
on it may be in use. /ONCE, /L and /BS are accepted and ignored.
:p.Related&colon. :link reftype=hd refid=chkdsk.CHKDSK:elink.,
:link reftype=hd refid=label.LABEL:elink..
.*
.* ---- GOTO ----
:h2 res=248 id=goto.GOTO
:i1.GOTO
:p.Continues a batch file at a label.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.GOTO:ehp4. label
:esl.
:dl tsize=10.
:dt.label
:dd.The name of a label, a line of the batch file that starts with a colon.
The colon may be left out in the GOTO command.
:edl.
:p.If the batch file has no such label, message SYS1039 appears and the
batch file ends.
:p.Related&colon. :link reftype=hd refid=if.IF:elink.,
:link reftype=hd refid=batch.Batch files:elink..
.*
.* ---- HELP ----
:h2 res=250 id=help.HELP
:i1.HELP
:p.Gives help on commands and on messages.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.HELP:ehp4.
:li.:hp4.HELP:ehp4. command
:li.:hp4.HELP:ehp4. [SYS]nnnn
:esl.
:dl tsize=12.
:dt.command
:dd.Shows a line about what the command does.
:dt.nnnn
:dd.Shows message nnnn and its explanation&colon. what caused it and what to
do.
:edl.
:p.HELP alone lists the commands. For the full description of a command,
use this book&colon. :hp4.VIEW CMDREF command:ehp4..
:p.Related&colon. :link reftype=hd refid=errors.Errors and messages:elink.,
:link reftype=hd refid=view.VIEW:elink..
.*
.* ---- HOST ----
:h2 res=252 id=host.HOST
:i1.HOST
:i1.name resolution
:i1.DNS
:p.Looks up a computer on the network by its name or address.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.HOST:ehp4. name | address
:esl.
:p.Given a name, HOST shows the computer's addresses; given an address (such
as 10.0.2.2), its name. It looks in the HOSTS file first, then asks the name
servers (DNS).
:p.Related&colon. :link reftype=hd refid=ping.PING:elink.,
:link reftype=hd refid=hostname.HOSTNAME:elink..
.*
.* ---- HOSTNAME ----
:h2 res=254 id=hostname.HOSTNAME
:i1.HOSTNAME
:p.Shows the name of this computer on the network.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.HOSTNAME:ehp4.
:esl.
:p.The name comes from the HOSTNAME environment variable; without one it is
rs2. To change it, put SET HOSTNAME=name in CONFIG.SYS and restart.
:p.Related&colon. :link reftype=hd refid=ifconfig.IFCONFIG:elink..
.*
.* ---- IF ----
:h2 res=256 id=if.IF
:i1.IF
:i1.ERRORLEVEL
:i1.EXIST
:p.Runs a command only if a condition holds.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.IF:ehp4. [NOT] :hp4.ERRORLEVEL:ehp4. n command
:li.:hp4.IF:ehp4. [NOT] :hp4.EXIST:ehp4. file command
:li.:hp4.IF:ehp4. [NOT] string1==string2 command
:esl.
:dl tsize=16.
:dt.NOT
:dd.Runs the command if the condition does not hold.
:dt.ERRORLEVEL n
:dd.True if the last command's return code is n or more.
:dt.EXIST file
:dd.True if the file exists; with wildcards, if any file matches.
:dt.string1==string2
:dd.True if the two strings are the same, capitals and small letters
counting. Put them in double quotation marks to compare empty strings or
strings with blanks.
:dt.command
:dd.Any command.
:edl.
:p.Because ERRORLEVEL means :hp2.n or more:ehp2., test the highest numbers
first. Examples&colon.
:sl.
:li.:hp4.IF EXIST REPORT.TXT TYPE REPORT.TXT:ehp4.
:li.:hp4.IF "%1"=="" GOTO USAGE:ehp4.
:li.:hp4.IF NOT ERRORLEVEL 1 ECHO It worked.:ehp4.
:esl.
:p.Related&colon. :link reftype=hd refid=goto.GOTO:elink.,
:link reftype=hd refid=separators.Command separators:elink..
.*
.* ---- IFCONFIG ----
:h2 res=258 id=ifconfig.IFCONFIG
:i1.IFCONFIG
:i1.network interfaces
:i1.IP address
:p.Shows the network interfaces and their settings.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.IFCONFIG:ehp4. [interface | -a]
:esl.
:dl tsize=12.
:dt.interface
:dd.Only that interface, such as lan0 or lo.
:dt.-a
:dd.All interfaces (the same as no parameter).
:edl.
:p.For each interface IFCONFIG shows its address, network mask, flags,
hardware address, gateway, name servers and how many packets went in and
out. rs2 sets the interfaces up when it starts (by DHCP, or as CONFIG.SYS
says); IFCONFIG does not change them.
:p.Related&colon. :link reftype=hd refid=netstat.NETSTAT:elink.,
:link reftype=hd refid=ping.PING:elink..
.*
.* ---- INSTALL ----
:h2 res=260 id=install.INSTALL
:i1.INSTALL
:i1.installing rs2
:i1.updating rs2
:p.Installs rs2 on a hard disk, or updates an rs2 already there.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.INSTALL:ehp4. [/LIST] [/DISK&colon.n] [/NEW[&colon.FAT32 |
&colon.HPFS | &colon.JFS] | /UPDATE] [/YES] [/NORESTART]
:esl.
:dl tsize=14.
:dt./LIST
:dd.Lists the hard disks and what is on them, and ends.
:dt./DISK&colon.n
:dd.The disk to install on, by its number in the /LIST list.
:dt./NEW&colon.FAT32
:dd.Erases the disk and makes one FAT32 partition for everything.
:dt./NEW&colon.HPFS
:dd.Erases the disk and makes a FAT32 boot partition and an HPFS partition
for the system.
:dt./NEW&colon.JFS
:dd.The same with a JFS partition for the system.
:dt./UPDATE
:dd.Updates the rs2 on the disk, keeping your settings and files.
:dt./YES
:dd.Does not ask for confirmation.
:dt./NORESTART
:dd.Does not restart the computer at the end.
:edl.
:p.Without switches INSTALL asks which disk to use, whether to update or
erase an rs2 it finds there, and, for a new installation, which layout.
/NEW and /UPDATE cannot be used together. /? shows the switches.
:p.Related&colon. :link reftype=hd refid=pminstal.PMINSTAL:elink.,
:link reftype=hd refid=appinst.APPINST:elink..
.*
.* ---- KEYB ----
:h2 res=262 id=keyb.KEYB
:i1.KEYB
:i1.keyboard layout
:p.Shows or changes the keyboard layout.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.KEYB:ehp4. [layout [codepage]]
:li.:hp4.KEYB /L:ehp4.
:esl.
:dl tsize=12.
:dt.layout
:dd.The country's keyboard&colon. US, UK, GR (Germany), FR, BE, NL, IT, SP
(Spain), LA (Latin America), PO (Portugal), SG (Swiss German), SF (Swiss
French), SV (Sweden), SU (Finland), DK, NO, IS (Iceland), CF (Canadian
French), TR or PL.
:dt.codepage
:dd.The code page to use with it.
:dt./L
:dd.Lists the layouts.
:edl.
:p.Without parameters KEYB shows the current layout and code page.
:p.Related&colon. :link reftype=hd refid=chcp.CHCP:elink.,
:link reftype=hd refid=mode.MODE:elink. (typing rate).
.*
.* ---- KEYS ----
:h2 res=264 id=keys.KEYS
:i1.KEYS
:i1.command recall
:p.On OS/2, turns command recall on or off.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.KEYS:ehp4. [ON | OFF | LIST]
:esl.
:p.rs2 accepts KEYS and its parameters and does nothing; its command prompt
does not keep a list of earlier commands.
.*
.* ---- LABEL ----
:h2 res=266 id=label.LABEL
:i1.LABEL
:i1.volume label
:p.Shows or changes the volume label of a disk.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.LABEL:ehp4. [drive:][label]
:esl.
:dl tsize=10.
:dt.drive:
:dd.The drive. Without it, the current drive.
:dt.label
:dd.The new label, up to 11 characters. It is stored in capitals.
:edl.
:p.Without a label, LABEL shows the present label and serial number and asks
for a new one; press Enter to keep the old one. A label cannot contain
* ? / \ | . , ; &colon. + = [ ] < > or ".
:p.Related&colon. :link reftype=hd refid=vol.VOL:elink.,
:link reftype=hd refid=format.FORMAT:elink..
.*
.* ---- MD ----
:h2 res=268 id=md.MD
:i1.MD
:i1.MKDIR
:i1.creating directories
:p.Creates directories. MKDIR is another name for MD.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.MD:ehp4. [drive:]path [...]
:esl.
:dl tsize=10.
:dt.path
:dd.The directory to create. The directories above it must exist already.
Several may be named.
:edl.
:p.Related&colon. :link reftype=hd refid=rd.RD:elink.,
:link reftype=hd refid=cd.CD:elink..
.*
.* ---- MODE ----
:h2 res=270 id=mode.MODE
:i1.MODE
:i1.screen lines
:i1.serial ports
:i1.typing rate
:p.Shows and changes the screen mode, the keyboard rate and the settings of
serial ports.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.MODE:ehp4. [CON]
:li.:hp4.MODE:ehp4. display[,rows]
:li.:hp4.MODE CON:ehp4. [COLS=c] [LINES=l] [RATE=r] [DELAY=d]
:li.:hp4.MODE COM:ehp4.n[&colon.]
:li.:hp4.MODE COM:ehp4.n[&colon.]baud[,parity[,databits[,stopbits]]]
[option=value ...]
:esl.
:p.:hp2.The screen:ehp2.
:dl tsize=12.
:dt.display
:dd.40, 80 or 132 columns; CO40 or CO80 (color), BW40 or BW80, MONO.
:dt.rows
:dd.25, 43, 50 or 60 lines.
:dt.COLS=, LINES=
:dd.Columns and lines.
:edl.
:p.MODE alone, or MODE CON, shows the columns, rows and colors of the
screen. Example&colon. :hp4.MODE CO80,50:ehp4..
:p.:hp2.The keyboard:ehp2.
:dl tsize=12.
:dt.RATE=r
:dd.How fast a held key repeats, 1 to 32.
:dt.DELAY=d
:dd.How long a key must be held before it repeats, 1 to 4 quarters of a
second.
:edl.
:p.:hp2.Serial ports:ehp2.
:dl tsize=12.
:dt.COMn
:dd.The port, such as COM1. Alone, MODE shows its settings.
:dt.baud
:dd.The speed in bits per second, such as 9600.
:dt.parity
:dd.N (none), O (odd), E (even), M (mark) or S (space).
:dt.databits
:dd.The number of data bits, 5 to 8.
:dt.stopbits
:dd.1, 1.5 or 2.
:dt.TO=
:dd.ON or OFF&colon. infinite time-out.
:dt.XON=
:dd.ON or OFF&colon. XON/XOFF flow control.
:dt.IDSR=, ODSR=
:dd.ON or OFF&colon. input and output DSR handshaking.
:dt.OCTS=
:dd.ON or OFF&colon. output CTS handshaking.
:dt.DTR=
:dd.ON, OFF or HS (handshaking).
:dt.RTS=
:dd.ON, OFF, HS (handshaking) or TOG (toggling).
:edl.
:p.Example&colon. :hp4.MODE COM1&colon.9600,N,8,1 XON=ON:ehp4.. Parallel
ports and diskette drives cannot be set with MODE in rs2.
:p.Related&colon. :link reftype=hd refid=display.DISPLAY:elink.,
:link reftype=hd refid=keyb.KEYB:elink..
.*
.* ---- MORE ----
:h2 res=272 id=more.MORE
:i1.MORE
:i1.screen at a time
:p.Shows its input one screen at a time.
:p.:hp2.Syntax:ehp2.
:sl.
:li.command | :hp4.MORE:ehp4.
:li.:hp4.MORE:ehp4. < file
:esl.
:p.After each screen MORE shows -- More --; press a key to see the next one.
:p.Related&colon. :link reftype=hd refid=type.TYPE:elink.,
:link reftype=hd refid=pipes.Pipes and output to files:elink..
.*
.* ---- MOVE ----
:h2 res=274 id=move.MOVE
:i1.MOVE
:i1.moving files
:p.Moves a file or directory to another directory on the same drive, or
renames it.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.MOVE:ehp4. source destination
:esl.
:dl tsize=14.
:dt.source
:dd.The file or directory to move. Wildcards cannot be used.
:dt.destination
:dd.Its new place and name. A name without a path stays in the source's
directory, so the file is only renamed.
:edl.
:p.Files cannot be moved to another drive (message SYS0017); copy them and
delete the originals instead.
:p.Example&colon. :hp4.MOVE \WORK\REPORT.TXT \DONE\REPORT.TXT:ehp4.
:p.Related&colon. :link reftype=hd refid=ren.REN:elink.,
:link reftype=hd refid=copy.COPY:elink..
.*
.* ---- NETSTAT ----
:h2 res=276 id=netstat.NETSTAT
:i1.NETSTAT
:i1.network connections
:i1.routing table
:p.Shows the state of the network&colon. connections, routes, interfaces and
counters.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.NETSTAT:ehp4. [-a] [-t] [-u] [-r] [-i] [-s]
:esl.
:dl tsize=8.
:dt.-a
:dd.All sockets, including those listening and UDP and raw sockets.
:dt.-t
:dd.TCP sockets only.
:dt.-u
:dd.UDP sockets only.
:dt.-r
:dd.The routing table.
:dt.-i
:dd.The interfaces.
:dt.-s
:dd.Counters of the protocols.
:edl.
:p.Without options NETSTAT lists the TCP connections. The options may also
be written with a slash.
:p.Related&colon. :link reftype=hd refid=ifconfig.IFCONFIG:elink.,
:link reftype=hd refid=ping.PING:elink..
.*
.* ---- PATH ----
:h2 res=278 id=path.PATH
:i1.PATH
:p.Shows or sets the list of directories searched for programs and batch
files.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.PATH:ehp4. [[drive:]path[;[drive:]path ...]]
:li.:hp4.PATH ;:ehp4.
:esl.
:p.PATH alone shows the list. With directories, separated by semicolons, it
replaces the list; PATH ; empties it, so that only the current directory is
searched. The list is the PATH environment variable; the setting lasts until
the command interpreter ends. To keep a change, edit the SET PATH= line of
CONFIG.SYS.
:p.Example&colon. :hp4.PATH %PATH%;D&colon.\TOOLS:ehp4.
:p.Related&colon. :link reftype=hd refid=dpath.DPATH:elink.,
:link reftype=hd refid=search.How programs are found:elink..
.*
.* ---- DPATH (after PATH, so that VIEW CMDREF PATH finds PATH) ----
:h2 res=230 id=dpath.DPATH
:i1.DPATH
:i1.data path
:p.Shows or sets the list of directories that programs search for their data
files.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.DPATH:ehp4. [[drive:]path[;[drive:]path ...]]
:li.:hp4.DPATH ;:ehp4.
:esl.
:p.DPATH works as PATH does, on the DPATH environment variable. Programs
that open a data file by its name alone may look for it in these
directories when it is not in the current one.
:p.Related&colon. :link reftype=hd refid=path.PATH:elink.,
:link reftype=hd refid=set.SET:elink..
.*
.* ---- PAUSE ----
:h2 res=280 id=pause.PAUSE
:i1.PAUSE
:p.Waits until Enter is pressed.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.PAUSE:ehp4.
:esl.
:p.PAUSE shows :hp2.Press Enter to continue . . .:ehp2. and waits. Use it in
a batch file to give the user time to read something or to change a
diskette. To stop the batch file instead, press Ctrl+C.
:p.Related&colon. :link reftype=hd refid=echo.ECHO:elink..
.*
.* ---- PING ----
:h2 res=282 id=ping.PING
:i1.PING
:i1.testing the network
:p.Sends echo requests to a computer on the network and shows the replies.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.PING:ehp4. [-c count] [-s size] [-w milliseconds] host
:esl.
:dl tsize=18.
:dt.host
:dd.The computer's name or address.
:dt.-c count
:dd.How many requests to send. -n may be used instead.
:dt.-s size
:dd.The number of data bytes in each request. -l may be used instead.
:dt.-w milliseconds
:dd.How long to wait for each reply.
:edl.
:p.Each reply shows how long it took. A host that does not reply may be
switched off, unreachable or not answering echo requests.
:p.Related&colon. :link reftype=hd refid=host.HOST:elink.,
:link reftype=hd refid=ifconfig.IFCONFIG:elink..
.*
.* ---- PMINSTAL ----
:h2 res=284 id=pminstal.PMINSTAL
:i1.PMINSTAL
:p.Installs or updates rs2 on a hard disk, in a window.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.PMINSTAL:ehp4.
:esl.
:p.PMINSTAL does the same work as INSTALL&colon. in its window choose the
disk, whether to update the rs2 on it or to erase it, and for a new
installation the layout (one FAT32 partition, or a FAT32 boot partition with
the system on HPFS). The window shows the progress.
:p.Related&colon. :link reftype=hd refid=install.INSTALL:elink..
.*
.* ---- PROMPT ----
:h2 res=286 id=prompt.PROMPT
:i1.PROMPT
:i1.command prompt, changing
:p.Changes the command prompt.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.PROMPT:ehp4. [text]
:esl.
:p.The text is shown as the prompt; a dollar sign and a letter in it stand
for something else&colon.
:dl tsize=6.
:dt.$P
:dd.The current drive and directory.
:dt.$N
:dd.The current drive letter.
:dt.$D
:dd.The date.
:dt.$T
:dd.The time.
:dt.$V
:dd.The version of rs2.
:dt.$R
:dd.The return code of the last command.
:dt.$I
:dd.The help line at the top of the screen.
:dt.$G
:dd.The > character.
:dt.$L
:dd.The < character.
:dt.$B
:dd.The | character.
:dt.$Q
:dd.The = character.
:dt.$A
:dd.The &amp. character.
:dt.$C
:dd.The ( character.
:dt.$F
:dd.The ) character.
:dt.$S
:dd.A blank.
:dt.$H
:dd.A backspace (erases the character before it).
:dt.$E
:dd.The Esc character, for ANSI escape sequences.
:dt.$_
:dd.A new line.
:dt.$$
:dd.The $ character.
:edl.
:p.PROMPT alone goes back to the standard prompt, $i[$p]. The prompt is the
PROMPT environment variable; to keep a change, edit the SET PROMPT= line of
CONFIG.SYS.
:p.Example&colon. :hp4.PROMPT $N$G:ehp4. gives a prompt such as C>.
:p.Related&colon. :link reftype=hd refid=set.SET:elink.,
:link reftype=hd refid=ansi.ANSI:elink..
.*
.* ---- RD ----
:h2 res=288 id=rd.RD
:i1.RD
:i1.RMDIR
:i1.removing directories
:p.Removes directories. RMDIR is another name for RD.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.RD:ehp4. [drive:]path [...]
:li.:hp4.RD /S:ehp4. [/Q] [drive:]path [...]
:esl.
:dl tsize=10.
:dt.path
:dd.The directory to remove. Without /S, it must be empty. Several may be
named.
:dt./S
:dd.Removes the directory with all its files and subdirectories, read-only,
hidden and system files included. RD asks first.
:dt./Q
:dd.With /S, does not ask.
:edl.
:p.The current directory cannot be removed.
:p.Related&colon. :link reftype=hd refid=md.MD:elink.,
:link reftype=hd refid=del.DEL:elink..
.*
.* ---- REM ----
:h2 res=290 id=rem.REM
:i1.REM
:i1.comments
:p.Marks a comment in a batch file or in CONFIG.SYS.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.REM:ehp4. [text]
:esl.
:p.The line does nothing. With ECHO ON it is shown like other lines; put @ in
front of it, or use ECHO OFF, to hide it.
:p.Related&colon. :link reftype=hd refid=batch.Batch files:elink..
.*
.* ---- REN ----
:h2 res=292 id=ren.REN
:i1.REN
:i1.RENAME
:i1.renaming files
:p.Renames a file or directory. RENAME is another name for REN.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.REN:ehp4. [drive:][path]name newname
:esl.
:dl tsize=10.
:dt.name
:dd.The file or directory to rename. Wildcards cannot be used.
:dt.newname
:dd.Its new name. It stays in the same directory unless a path is given
(then the file is moved, as MOVE does).
:edl.
:p.Example&colon. :hp4.REN \WORK\DRAFT.TXT FINAL.TXT:ehp4.
:p.Related&colon. :link reftype=hd refid=move.MOVE:elink..
.*
.* ---- SET ----
:h2 res=294 id=set.SET
:i1.SET
:p.Shows, sets or removes environment variables.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.SET:ehp4.
:li.:hp4.SET:ehp4. name
:li.:hp4.SET:ehp4. name=[value]
:esl.
:dl tsize=12.
:dt.name
:dd.Alone, shows the variables whose names start with it.
:dt.name=value
:dd.Sets the variable. Names are stored in capitals; the value is kept as
typed.
:dt.name=
:dd.Removes the variable.
:edl.
:p.SET alone lists all the variables. Changes last until the command
interpreter ends and are passed to the programs it starts; for lasting
settings, use SET lines in CONFIG.SYS.
:p.SET BEGINLIBPATH=, SET ENDLIBPATH= and SET LIBPATHSTRICT= change the
directories searched for DLLs by this command interpreter and the programs it
starts; see :link reftype=hd refid=envvars.Environment variables:elink..
:p.Related&colon. :link reftype=hd refid=path.PATH:elink.,
:link reftype=hd refid=prompt.PROMPT:elink.,
:link reftype=hd refid=setlocal.SETLOCAL:elink..
.*
.* ---- SETLOCAL ----
:h2 res=296 id=setlocal.SETLOCAL
:i1.SETLOCAL
:p.Saves the environment, the current drive and the current directory, so
that a batch file can change them and ENDLOCAL can bring them back.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.SETLOCAL:ehp4.
:esl.
:p.SETLOCAL and ENDLOCAL may be nested&colon. each ENDLOCAL brings back what
the matching SETLOCAL saved.
:p.Related&colon. :link reftype=hd refid=endlocal.ENDLOCAL:elink.,
:link reftype=hd refid=set.SET:elink..
.*
.* ---- SHIFT ----
:h2 res=298 id=shift.SHIFT
:i1.SHIFT
:p.Moves the parameters of a batch file down by one.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.SHIFT:ehp4.
:esl.
:p.After SHIFT, %1 is what %2 was, %2 what %3 was, and so on; %9 gets the
tenth parameter. %0 stays the name of the batch file. Use it to handle more
than nine parameters, or any number of them in a loop.
:p.Example&colon.
:sl.
:li.&colon.NEXT
:li.IF "%1"=="" GOTO END
:li.TYPE %1
:li.SHIFT
:li.GOTO NEXT
:li.&colon.END
:esl.
:p.Related&colon. :link reftype=hd refid=params.Batch file parameters:elink..
.*
.* ---- SORT ----
:h2 res=300 id=sort.SORT
:i1.SORT
:i1.sorting lines
:p.Sorts the lines of its input and writes them out.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.SORT:ehp4. [/R] [/+n] [< file]
:esl.
:dl tsize=10.
:dt./R
:dd.Sorts in reverse order.
:dt./+n
:dd.Sorts by the text from column n on (the first column is 1).
:edl.
:p.Capitals and small letters sort together.
:p.Examples&colon.
:sl.
:li.:hp4.SORT < NAMES.TXT > SORTED.TXT:ehp4.
:li.:hp4.DIR /B | SORT /R:ehp4.
:esl.
:p.Related&colon. :link reftype=hd refid=find.FIND:elink.,
:link reftype=hd refid=more.MORE:elink..
.*
.* ---- START ----
:h2 res=302 id=start.START
:i1.START
:i1.sessions, starting
:p.Starts a program or command in a new session.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.START:ehp4. ["title"] [/K | /C | /N] [/F | /B] [/FS | /WIN | /PM]
[command [parameters]]
:esl.
:dl tsize=10.
:dt."title"
:dd.The title of the session in the Window List. Without it, the name of the
program.
:dt./K
:dd.Runs the command in a new command interpreter, which stays when the
command ends.
:dt./C
:dd.Runs the command in a new command interpreter, which ends with it. This
is what happens to a command or batch file without /K or /N.
:dt./N
:dd.Starts the program directly, without a command interpreter. An .EXE
file without /K is always started this way.
:dt./F
:dd.The new session comes to the foreground (the default).
:dt./B
:dd.The new session starts in the background.
:dt./FS
:dd.A full-screen session.
:dt./WIN
:dd.A session in a window.
:dt./PM
:dd.A Presentation Manager program.
:edl.
:p.Without a command, START opens a new command prompt session titled OS/2
Window. /DOS, /I, /MAX, /MIN and /PGM are accepted and ignored.
:p.Example&colon. :hp4.START "Build" /K MAKE.CMD:ehp4.
:p.Related&colon. :link reftype=hd refid=detach.DETACH:elink..
.*
.* ---- SYSDLLS ----
:h2 res=304 id=sysdlls.SYSDLLS
:i1.SYSDLLS
:p.Loads DLLs and keeps them loaded.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.SYSDLLS:ehp4. module [module ...]
:esl.
:p.SYSDLLS loads each module (running its start-up code) and then waits
forever, so that the modules, and anything they started, stay in memory.
It is meant to be started with RUN= in CONFIG.SYS or with DETACH, for DLLs
such as REXX's that serve other programs. It ends at once if none of the
modules could be loaded.
.*
.* ---- TIME ----
:h2 res=306 id=time.TIME
:i1.TIME
:p.Shows the time.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.TIME:ehp4.
:esl.
:p.TIME shows the hours, minutes, seconds and hundredths of a second. rs2
takes the time from the computer's clock; TIME does not change it, and any
parameters are ignored.
:p.Related&colon. :link reftype=hd refid=date.DATE:elink.,
:link reftype=hd refid=prompt.PROMPT:elink. ($T).
.*
.* ---- TREE ----
:h2 res=308 id=tree.TREE
:i1.TREE
:i1.directory tree
:p.Shows the directories under a directory, each indented under the one
that holds it.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.TREE:ehp4. [drive:][path] [/F]
:esl.
:dl tsize=10.
:dt.path
:dd.Where to start. Without it, the current directory.
:dt./F
:dd.Lists the files in each directory too.
:edl.
:p.Related&colon. :link reftype=hd refid=dir.DIR:elink. (DIR /S).
.*
.* ---- TYPE ----
:h2 res=310 id=type.TYPE
:i1.TYPE
:i1.showing a file
:p.Shows the contents of text files.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.TYPE:ehp4. file [file ...]
:esl.
:dl tsize=10.
:dt.file
:dd.The file to show. Wildcards may be used; several files are shown one
after the other.
:edl.
:p.A Ctrl+Z character ends the text. To read a long file a screen at a time,
use :hp4.TYPE file | MORE:ehp4..
:p.Related&colon. :link reftype=hd refid=more.MORE:elink.,
:link reftype=hd refid=e.E:elink..
.*
.* ---- VER ----
:h2 res=312 id=ver.VER
:i1.VER
:i1.version
:p.Shows the version of rs2.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.VER:ehp4.
:esl.
:p.rs2 reports version 4.52, the version of OS/2 it carries on from.
.*
.* ---- VERIFY ----
:h2 res=314 id=verify.VERIFY
:i1.VERIFY
:p.Tells whether data written to disks is read back to check it.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.VERIFY:ehp4. [ON | OFF]
:esl.
:p.rs2 does not read written data back, so VERIFY always reports that it is
off; ON and OFF change nothing.
.*
.* ---- VIEW ----
:h2 res=316 id=view.VIEW
:i1.VIEW
:i1.books, reading
:i1..INF files
:p.Opens an online book, such as this one.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.VIEW:ehp4. book[+book ...] [topic]
:esl.
:dl tsize=10.
:dt.book
:dd.The name of the book. Without an extension, .INF is added; without a
path, the book is looked for in the directories of the BOOKSHELF variable.
Several books joined with + open as one.
:dt.topic
:dd.A word to look for&colon. the book opens at the first panel whose title,
or else whose index entry, contains it. Without it, the book opens at its
contents.
:edl.
:p.Examples&colon.
:sl.
:li.:hp4.VIEW CMDREF:ehp4.
:li.:hp4.VIEW CMDREF COPY:ehp4.
:esl.
:p.Closing the book's window ends VIEW.
:p.Related&colon. :link reftype=hd refid=help.HELP:elink..
.*
.* ---- VOL ----
:h2 res=318 id=vol.VOL
:i1.VOL
:i1.serial number
:p.Shows the volume label and serial number of a disk.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.VOL:ehp4. [drive:]
:esl.
:dl tsize=10.
:dt.drive:
:dd.The drive. Without it, the current drive.
:edl.
:p.Related&colon. :link reftype=hd refid=label.LABEL:elink.,
:link reftype=hd refid=dir.DIR:elink..
.*
.* ---- XCOPY ----
:h2 res=320 id=xcopy.XCOPY
:i1.XCOPY
:i1.copying directories
:p.Copies files, and with /S whole directory trees.
:p.:hp2.Syntax:ehp2.
:sl.
:li.:hp4.XCOPY:ehp4. source [destination] [/S] [/E]
:esl.
:dl tsize=14.
:dt.source
:dd.The files to copy&colon. a directory (all its files) or a file name with
wildcards.
:dt.destination
:dd.The directory to copy them into; it is created if needed. Without it,
the current directory.
:dt./S
:dd.Also copies the subdirectories, with the files in them that match.
:dt./E
:dd.With /S, also creates the subdirectories that have no matching files.
:edl.
:p.XCOPY lists each file as it copies it, and at the end how many files were
copied. Files of the same name in the destination are replaced.
:p.Example&colon. :hp4.XCOPY C&colon.\WORK D&colon.\BACKUP\WORK /S /E:ehp4.
:p.Related&colon. :link reftype=hd refid=copy.COPY:elink..
:euserdoc.
