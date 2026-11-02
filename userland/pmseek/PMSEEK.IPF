.* PMSEEK.HLP: help for rs2's Seek and Scan Files (userland/pmseek,
.* OS2\APPS\PMSEEK.EXE).
.*
.* Written for rs2. The res= numbers are the panels the program's help
.* table asks for (the numbers OS/2's Seek and Scan Files uses). xtask
.* compiles this file with crates/ipf into OS2\HELP\PMSEEK.HLP.
:userdoc.
:title.Help for Seek and Scan Files
.*
:h1 res=21247 id=general.Seek and Scan Files
:i1.Seek and Scan Files
:i1.searching for files
:p.Seek and Scan Files finds files on your drives by their names and, if
you like, by text they contain. The files found are listed with the date
and time they were last changed, their size and their full name; from the
list you can open a file in an editor, run a program or type a command for
it.
:p.To search&colon.
:ol.
:li.Type the name of the files to find in :hp2.File name to search
for:ehp2.. Use :hp2.*:ehp2. for any characters and :hp2.?:ehp2. for any one
character, for example :hp2.*.CMD:ehp2. or :hp2.REPORT??.TXT:ehp2.. A
directory may come first (:hp2.\OS2\*.EXE:ehp2.) and so may a drive
(:hp2.D:\*.INI:ehp2., which searches only that drive).
:li.To find only files holding some text, type it in :hp2.Text to search
for:ehp2..
:li.Choose the drives to search.
:li.Select :hp2.Search:ehp2..
:eol.
:p.The line above the list tells what is being searched, and how many files
were found and how long it took when the search ends. You can go on working
while the search runs; :hp2.Stop:ehp2. ends it early.
:p.The choices in the :link reftype=hd res=21259.Options:elink. menu decide
whether subdirectories are searched, whether the lines holding the text are
listed, whether capitals matter and whether the list is cleared before each
search.
.*
:h2 res=21239 id=keys.Keys
:i1.keys
:p.These keys work in the Seek and Scan Files window&colon.
:dl tsize=18.
:dt.Enter
:dd.Starts the search (or, in the list, opens the file).
:dt.F2
:dd.Saves the list in its file (File, Save).
:dt.F4
:dd.Clears the list.
:dt.F5
:dd.Starts the search.
:dt.F6
:dd.Stops the search.
:dt.Ctrl+Insert
:dd.Copies the selected line to the clipboard.
:dt.F1
:dd.Shows help for the field or menu item you are on.
:edl.
.*
:h2 res=21230 id=name.File name to search for
:p.Type the name of the files to find. :hp2.*:ehp2. stands for any number
of characters and :hp2.?:ehp2. for one character&colon. :hp2.*.*:ehp2. finds
every file, :hp2.*.TXT:ehp2. every file ending in .TXT.
:p.Put a directory in front of the name to search only there (and below it,
when subdirectories are searched), for example :hp2.\OS2\HELP\*.HLP:ehp2..
With a drive letter in front (:hp2.C:\*.SYS:ehp2.) only that drive is
searched, whatever drives are checked.
.*
:h2 res=21231 id=text.Text to search for
:p.Leave this empty to list every file whose name matches. Type some text
to list only the files that contain it. The text is looked for in each line
of the file; whether capitals matter is set by :hp2.Ignore case:ehp2. in the
Options menu.
.*
:h2 res=21238 id=editor.Editor filespec
:p.The editor that :hp2.Open:ehp2. starts with the selected file. It is the
System Editor, :hp2.E.EXE:ehp2., unless you type another program here. A
program without a directory is looked for along the PATH. :hp2.Set
defaults:ehp2. in the Options menu keeps your choice.
.*
:h2 res=21260 id=allfixed.All fixed disks
:p.Check this to search all the hard disks of the computer. Diskette, CD
and network drives are not included; check their letters to search them
too.
.*
:h2 res=21261 id=drives.Drives
:p.Each check box stands for one drive of the computer. Check the drives to
search. Checking :hp2.All fixed disks:ehp2. checks every hard disk.
.*
:h2 res=21249 id=list.Files found
:p.Each file found has a line with the date and time it was last changed,
its size in bytes and its full name. With :hp2.Display found text:ehp2.
on, the lines of the file holding the text follow, each starting with an
arrow.
:p.Select a line to work with its file (the Selected menu, Open). A double
click opens the file in the editor.
.*
:h2 res=21250 id=search.Search
:p.Starts searching. The list is cleared first when :hp2.Clear on
search:ehp2. is checked in the Options menu.
.*
:h2 res=21245 id=stop.Stop
:p.Ends the search that is running. The files found so far stay in the
list.
.*
:h2 res=21252 id=helpbtn.Help
:p.Shows help for the Seek and Scan Files window.
.*
:h2 res=21253 id=filemenu.File menu
:i1.File menu
:p.:hp2.Save:ehp2. writes the list into its file (PMSEEK.LST unless you
chose another with Save as). :hp2.Save as:ehp2. asks for the file's name
first.
.*
:h2 res=21254 id=save.Save
:p.Writes every line of the list into the list file, one line each.
.*
:h2 res=21255 id=saveas.Save as
:p.Asks for the name of the file to write the list into, then writes it.
The name is kept for the next Save.
.*
:h2 res=21256 id=saveasdlg.Save As
:p.Type the name of the file to write the list into and select
:hp2.Save:ehp2.. A file of that name is replaced.
.*
:h3 res=21257 id=saveasok.Save
:p.Writes the list into the file named.
.*
:h3 res=21246 id=cancel.Cancel
:p.Closes the window without doing anything.
.*
:h2 res=34463 id=selmenu.Selected menu
:i1.Selected menu
:p.What to do with the file of the line selected in the list&colon.
:hp2.Open:ehp2. it in the editor, :hp2.Process:ehp2. it (run it) or type a
:hp2.Command:ehp2. for it.
.*
:h2 res=21233 id=open.Open
:p.Starts the editor named in :hp2.Editor filespec:ehp2. with the selected
file.
.*
:h2 res=21234 id=process.Process
:p.Runs the selected file&colon. a program (.EXE, .COM) is started, any
other file is given to the command processor, which runs command files
(.CMD).
.*
:h2 res=21235 id=command.Command
:p.Asks for a command line, filled in with the selected file's name, and
runs it in an OS/2 window through the command processor.
.*
:h2 res=21243 id=commanddlg.File Command
:p.Type the command to run, for example :hp2.COPY:ehp2. in front of the
file's name and a target after it, and select :hp2.OK:ehp2..
.*
:h3 res=21244 id=commandok.OK
:p.Runs the command.
.*
:h2 res=21258 id=editmenu.Edit menu
:i1.Edit menu
:p.:hp2.Copy:ehp2. puts the selected line on the clipboard;
:hp2.Clear list:ehp2. empties the list.
.*
:h2 res=21236 id=copy.Copy
:p.Copies the selected line of the list to the clipboard as text.
.*
:h2 res=21251 id=clear.Clear list
:p.Removes every line from the list.
.*
:h2 res=21259 id=options.Options menu
:i1.Options menu
:p.Each choice is on when it has a check mark; select it to change it.
:hp2.Set defaults:ehp2. keeps the choices and the editor for the next
time.
.*
:h2 res=21240 id=subdirs.Search subdirectories
:p.When checked, the search goes through every directory below the one it
starts in; otherwise only that directory is searched.
.*
:h2 res=21241 id=foundtext.Display found text
:p.When checked, the lines of each file that hold the text are listed below
the file (at most 50 for one file).
.*
:h2 res=21242 id=ignorecase.Ignore case
:p.When checked, capital and small letters are the same to the text search.
.*
:h2 res=21232 id=clearonsearch.Clear on search
:p.When checked, the list is emptied each time a search starts; otherwise
new files are added below the ones already there.
.*
:h2 res=21237 id=defaults.Set defaults
:p.Keeps the Options menu's choices, the editor and the list file's name,
so that Seek and Scan Files starts with them next time.
.*
:h2 res=64022 id=helpmenu.Help menu
:i1.Help menu
:p.The Help menu has&colon.
:dl tsize=22.
:dt.Help index
:dd.An alphabetic list of the help topics.
:dt.General help
:dd.What Seek and Scan Files does and how to use it.
:dt.Using help
:dd.How to use the help itself.
:dt.Keys help
:dd.The keys of the window.
:dt.Product information
:dd.The program's name and version.
:edl.
.*
:h2 res=64023 id=usinghelp.Using help
:p.Choose :hp2.Using help:ehp2. to learn how the help windows work.
.*
:h2 res=64024 id=generalhelp.General help
:p.Choose :hp2.General help:ehp2. for an overview of Seek and Scan Files.
.*
:h2 res=64025 id=keyshelp.Keys help
:p.Choose :hp2.Keys help:ehp2. to see the keys of the window.
.*
:h2 res=64026 id=helpindex.Help index
:p.Choose :hp2.Help index:ehp2. to see the help topics in alphabetic order.
.*
:h2 res=64096 id=product.Product information
:p.Shows the name and version of Seek and Scan Files.
:euserdoc.
