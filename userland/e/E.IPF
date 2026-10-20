.* E.HLP: help for rs2's System Editor (userland/e, OS2\E.EXE).
.*
.* Written for rs2. The res= numbers are the panels E's help table asks
.* for (help_table() in src/main.rs), the same numbers OS/2's E uses, so
.* either help file works with either editor. xtask compiles this file
.* with crates/ipf into OS2\HELP\E.HLP.
:userdoc.
:title.System Editor Help
.*
.* ---- Overview (General help; the frame's extended help) ----
:h1 res=0x1030 id=general.System Editor
:i1.System Editor
:i1.E
:p.The System Editor is a small editor for plain text files&colon. the
CONFIG.SYS file, command files, notes and other files made of lines of
text. It edits one file in a window; start it more than once to work on
several files at the same time.
:p.To start it, open the System Editor object, open a text file from a
folder, or type :hp2.E:ehp2. at a command prompt, followed by the name of a
file if you like.
:p.The window's title shows the name of the file, or :hp2.Untitled:ehp2. for
a new file that has not been saved yet. The text area works like the other
entry fields of the system&colon. type to insert text, select text with the
mouse or with the Shift key and the cursor keys, and use the menus for
everything else.
:p.The menus are&colon.
:ul.
:li.:link reftype=hd res=0x1320.File:elink. to open, start and save files
:li.:link reftype=hd res=0x1330.Edit:elink. to undo, move and find text
:li.:link reftype=hd res=0x1340.Options:elink. to set the font, the colors
and word wrap
:li.:link reftype=hd res=0xfa16.Help:elink. for this information
:eul.
:p.See also :link reftype=hd res=0x1031.the keys of the System Editor:elink.
and :link reftype=hd refid=types.saving and file types:elink..
:p.The System Editor remembers your settings from one session to the next&colon.
its window position, the font, the colors, word wrap, autosave and the
choices of the Find window.
.*
.* ---- Keys ----
:h2 res=0x1031 id=keys.Keys
:i1.keys
:i1.shortcut keys
:p.These keys work in the System Editor's window&colon.
:dl tsize=22.
:dt.Ctrl+F
:dd.Find or change text (Edit, Find).
:dt.F3
:dd.Close the window, asking first if there are unsaved changes.
:dt.Alt+Backspace
:dd.Undo the last change.
:dt.Shift+Delete
:dd.Cut the selected text to the clipboard.
:dt.Ctrl+Insert
:dd.Copy the selected text to the clipboard.
:dt.Shift+Insert
:dd.Paste the text on the clipboard.
:dt.Delete
:dd.Delete the selected text, or the character after the cursor.
:dt.Backspace
:dd.Delete the character before the cursor.
:dt.Insert
:dd.Switch between inserting and replacing characters.
:dt.Shift+cursor keys
:dd.Select text.
:dt.Home, End
:dd.Go to the start or the end of the line.
:dt.Ctrl+Home, Ctrl+End
:dd.Go to the start or the end of the file.
:dt.Page Up, Page Down
:dd.Move a window of text up or down.
:dt.F1
:dd.Help for the menu choice or window you are in.
:dt.F10
:dd.Go to the menu bar.
:edl.
.*
.* ---- File ----
:h2 res=0x1320 id=file.File menu
:i1.File menu
:p.The :hp2.File:ehp2. menu opens and saves files&colon.
:ul.
:li.:link reftype=hd res=0x1050.New:elink.
:li.:link reftype=hd res=0x1040.Open:elink.
:li.:link reftype=hd res=0x1060.Save:elink.
:li.:link reftype=hd res=0x1070.Save as:elink.
:li.:link reftype=hd res=0x1080.Autosave:elink.
:eul.
:p.To close the System Editor, press F3 or close its window.
If the file has changes that are not saved, the editor asks what to do with
them first (see :link reftype=hd res=0x1360.File Changed:elink.).
.*
:h3 res=0x1050.New
:i1.new file
:p.:hp2.New:ehp2. starts a new, empty file called :hp2.Untitled:ehp2.. If the
file you were editing has unsaved changes, you are asked whether to save
them first.
:p.Give the new file a name when you save it with :hp2.Save as:ehp2..
.*
:h3 res=0x1040.Open
:i1.opening a file
:p.:hp2.Open:ehp2. replaces the file in the window with another one. If the
file you were editing has unsaved changes, you are asked whether to save
them first. Then the Open window lets you choose the file.
.*
:h3 res=0x1043.Open window
:p.Choose the file to edit&colon.
:ol.
:li.Choose the drive and the directory, or type a full name in the file name
field.
:li.Select the file in the list of files, or type its name.
:li.Select :hp2.Open:ehp2..
:eol.
:p.The System Editor reads the whole file. A file that contains NUL
characters is not plain text; the editor asks before loading it (see
:link reftype=hd res=0x13ba.NUL characters:elink.).
.*
:h3 res=0x1060.Save
:i1.saving a file
:p.:hp2.Save:ehp2. writes the text back to its file, replacing what was
there. A new file that has no name yet is saved with :hp2.Save as:ehp2.
instead.
:p.Before saving, the editor looks at the file's type and may ask about it;
see :link reftype=hd refid=types.saving and file types:elink..
:p.Lines are saved ending in a carriage return and a line feed, with an end-of-file
mark (Ctrl+Z) after the last line, as OS/2 text files usually are.
.*
:h3 res=0x1070.Save as
:i1.Save as
:i1.renaming a file
:p.:hp2.Save as:ehp2. saves the text under a new name, which becomes the name
of the file in the window. Use it to name a new file or to keep a copy of
the file as it was before your changes.
.*
:h3 res=0x1072.Save as window
:p.Choose where to save the file&colon.
:ol.
:li.Choose the drive and the directory.
:li.Type the file's name in the file name field.
:li.Select :hp2.Type...:ehp2. if you want to set the file's
:link reftype=hd res=0x13a6.type:elink..
:li.Select :hp2.Save:ehp2..
:eol.
:p.If a file with that name is there already, it is replaced.
:note.On a FAT drive, a file name is at most eight characters, with an
extension of at most three after a period.
.*
:h3 res=0x1080.Autosave
:i1.autosave
:p.:hp2.Autosave:ehp2. saves the file for you every time you have made a
number of changes, so that little is lost if something goes wrong.
.*
:h3 res=0x1082.Autosave window
:dl tsize=22.
:dt.Autosave on
:dd.Check this to have the file saved automatically.
:dt.Number of changes
:dd.How many changes to make between saves.
:dt.Set
:dd.Use these settings. They are kept for the next time.
:dt.Cancel
:dd.Close the window without changing anything.
:edl.
:note.Autosave saves only files that have a name. Save a new file with
:hp2.Save as:ehp2. first.
.*
:h3 res=0x1100.Close
:p.Press F3 to close the System Editor's window. If the file has unsaved
changes, you are asked whether to save them first.
.*
.* ---- Edit ----
:h2 res=0x1330 id=edit.Edit menu
:i1.Edit menu
:p.The :hp2.Edit:ehp2. menu works on the text&colon.
:ul.
:li.:link reftype=hd res=0x1110.Undo:elink.
:li.:link reftype=hd res=0x1120.Cut:elink., :link reftype=hd res=0x1130.Copy:elink.
and :link reftype=hd res=0x1140.Paste:elink.
:li.:link reftype=hd res=0x1150.Clear:elink.
:li.:link reftype=hd res=0x1160.Find:elink.
:li.:link reftype=hd res=0x1170.Select all:elink.
:eul.
:p.Cut, Copy and Clear work on the selected text. To select text, drag the
mouse over it with mouse button 1 held down, or hold down Shift while moving
the cursor.
.*
:h3 res=0x1110.Undo
:i1.undo
:p.:hp2.Undo:ehp2. takes back the last change to the text. The menu choice
says what it will undo, such as :hp2.Undo typing:ehp2. or :hp2.Undo
paste:ehp2.. Choosing it again redoes the change.
:p.Only changes made since the last menu choice can be undone.
.*
:h3 res=0x1120.Cut
:i1.cut
:p.:hp2.Cut:ehp2. removes the selected text and puts it on the clipboard,
from where you can paste it somewhere else in this file or in another
program.
.*
:h3 res=0x1130.Copy
:i1.copy
:p.:hp2.Copy:ehp2. puts a copy of the selected text on the clipboard. The
text stays where it is.
.*
:h3 res=0x1140.Paste
:i1.paste
:i1.clipboard
:p.:hp2.Paste:ehp2. inserts the text on the clipboard at the cursor. If text
is selected, the clipboard's text replaces it.
.*
:h3 res=0x1150.Clear
:i1.deleting text
:p.:hp2.Clear:ehp2. deletes the selected text without putting it on the
clipboard. The Delete key does the same.
.*
:h3 res=0x1160.Find
:i1.finding text
:i1.changing text
:p.:hp2.Find:ehp2. opens the Find window to look for text in the file and,
if you like, to change it to other text. The window stays open while you
work in the file, so you can find the next place at any time.
.*
:h3 res=0x1161.Find window
:dl tsize=22.
:dt.Find
:dd.The text to look for.
:dt.Change to
:dd.The text to put in its place. Leave it empty to delete the text found.
:dt.Case sensitive
:dd.Find only text with the same capital and small letters.
:dt.Wrap
:dd.After the end of the file, go on looking from its start.
:dt.Find (button)
:dd.Select the next place after the cursor where the text is.
:dt.Change, then find
:dd.Change the text found, then find the next place.
:dt.Change
:dd.Change the text found.
:dt.Change all
:dd.Change every place where the text is, from the cursor to the end of the
file (the whole file with :hp2.Wrap:ehp2.).
:dt.Cancel
:dd.Close the Find window.
:edl.
:p.The change buttons work once text has been found. If the text is not in
the file, a message says so.
.*
:h3 res=0x1170.Select all
:i1.selecting text
:p.:hp2.Select all:ehp2. selects all the text of the file, for example to copy
it to the clipboard.
.*
.* ---- Options ----
:h2 res=0x1340 id=options.Options menu
:i1.Options menu
:p.The :hp2.Options:ehp2. menu changes how the text looks&colon.
:ul.
:li.:link reftype=hd res=0x1180.Set font:elink.
:li.:link reftype=hd res=0x1190.Set colors:elink.
:li.:link reftype=hd res=0x1310.Word wrap:elink.
:eul.
:p.These settings change only how the text is shown, not the file, and are
kept for the next time you use the System Editor.
.*
:h3 res=0x1180.Set font
:i1.font
:p.:hp2.Set font:ehp2. chooses the font the text is shown in.
.*
:h3 res=0x1181.Font window
:p.Choose the font's name, size and style; the sample shows how text will
look.
:dl tsize=22.
:dt.OK
:dd.Use the font and close the window.
:dt.Apply
:dd.Use the font and keep the window open to try others.
:dt.Cancel
:dd.Close the window.
:edl.
.*
:h3 res=0x1190.Set colors
:i1.colors
:p.:hp2.Set colors:ehp2. chooses the colors of the text and of the
background.
.*
:h3 res=0x1191.Set Colors window
:dl tsize=22.
:dt.Foreground
:dd.The color of the text.
:dt.Background
:dd.The color behind the text.
:dt.Color sample
:dd.How text and selected text will look.
:dt.Set
:dd.Use the colors and close the window.
:dt.Apply
:dd.Use the colors and keep the window open.
:dt.Cancel
:dd.Close the window.
:edl.
:p.:hp2.Default:ehp2. is black text on white.
.*
:h3 res=0x1310.Word wrap
:i1.word wrap
:p.With :hp2.Word wrap:ehp2. on, lines longer than the window are shown on
several rows, broken between words. With it off, long lines go past the
right edge of the window and you scroll to see them.
:p.Word wrap changes only the display&colon. the file keeps its lines as they
are.
.*
.* ---- Help ----
:h2 res=0xfa16 id=help.Help menu
:i1.Help menu
:p.The :hp2.Help:ehp2. menu shows this information&colon.
:ul.
:li.:link reftype=hd res=0xfa1a.Help index:elink.
:li.:link reftype=hd res=0xfa18.General help:elink.
:li.:link reftype=hd res=0xfa17.Using help:elink.
:li.:link reftype=hd res=0xfa19.Keys help:elink.
:li.:link reftype=hd res=0xfa60.Product information:elink.
:eul.
:p.Press F1 at any time for help on the menu choice or the window you are in.
.*
:h3 res=0xfa1a.Help index
:p.:hp2.Help index:ehp2. lists the subjects of the System Editor's help in
alphabetical order. Select one to read about it.
.*
:h3 res=0xfa18.General help
:p.:hp2.General help:ehp2. explains what the System Editor is for and how to
use it.
.*
:h3 res=0xfa17.Using help
:p.:hp2.Using help:ehp2. explains how to use the help windows themselves.
.*
:h3 res=0xfa19.Keys help
:p.:hp2.Keys help:ehp2. lists the keys you can use in the System Editor.
.*
:h3 res=0xfa60.Product information
:i1.product information
:i1.version
:p.:hp2.Product information:ehp2. shows the System Editor's version.
:p.This System Editor is part of rs2, a re-implementation of OS/2 Warp
written from scratch. It is free software, under the MIT or the Apache 2.0
licence.
.*
.* ---- Saving and file types ----
:h2 id=types.Saving and file types
:i1.file types
:i1.type of a file
:p.OS/2 keeps a file's :hp1.type:ehp1. with the file, in its extended
attributes. The type says what the file holds, and the desktop uses it to
choose the program that opens the file. The System Editor knows three
types&colon.
:dl tsize=22.
:dt.Plain Text
:dd.Text such as notes or documents.
:dt.OS/2 Command File
:dd.A batch file for OS/2 command prompts, such as a .CMD file or CONFIG.SYS.
:dt.DOS Command File
:dd.A batch file for DOS, such as AUTOEXEC.BAT.
:edl.
:p.When you save a file, the editor looks at its type&colon.
:ul.
:li.If the file has no type, it asks you to choose one
(see :link reftype=hd res=0x1392.No type:elink.).
:li.If the file's types are none of these three, it offers to add Plain Text
(see :link reftype=hd res=0x139c.Unknown type:elink.).
:eul.
:p.:hp2.Save as:ehp2. gives a new file the type last chosen in the
:link reftype=hd res=0x13a6.Type:elink. window.
.*
:h3 res=0x1360.File Changed
:i1.unsaved changes
:p.The file in the window has changes that have not been saved, and what
you asked for would lose them&colon. closing the window, opening another
file, starting a new one or shutting the system down. Choose&colon.
:dl tsize=22.
:dt.Save
:dd.Save the changes, then go on.
:dt.Save as
:dd.Save the changes under a new name, then go on.
:dt.Discard
:dd.Throw the changes away and go on.
:dt.Cancel
:dd.Go back to the file without doing anything. When the system is shutting
down, Cancel stops the shutdown.
:edl.
.*
:h3 res=0x1392.No type
:p.The file you are saving has no type yet.
:dl tsize=22.
:dt.Type...
:dd.Choose a type in the :link reftype=hd res=0x13a6.Type:elink. window;
the file is then saved with it.
:dt.Save as...
:dd.Save the file under another name instead.
:dt.Cancel
:dd.Do not save the file.
:edl.
.*
:h3 res=0x139c.Unknown type
:p.The file you are saving has a type the System Editor does not know.
:dl tsize=22.
:dt.Yes
:dd.Add the type Plain Text to the types the file has, and save it.
:dt.No
:dd.Save the file with the types it has now.
:dt.Save as...
:dd.Save the file under another name instead.
:dt.Cancel
:dd.Do not save the file.
:edl.
.*
:h3 res=0x13a6.Type
:p.Choose the type of the file from the list and select :hp2.Set:ehp2.. The
type you choose is also used for new files saved with :hp2.Save
as:ehp2. from then on. :hp2.Cancel:ehp2. closes the window without a change.
.*
:h3 res=0x13ba.NUL characters
:i1.NUL characters
:i1.binary files
:p.The file you are opening contains NUL characters (bytes with the value
zero), so it is probably not a text file.
:dl tsize=22.
:dt.Continue loading file
:dd.Load it anyway, with blanks in place of the NUL characters. If you save
it, the file is written with blanks there.
:dt.Stop loading file
:dd.Do not load the file.
:edl.
:euserdoc.
