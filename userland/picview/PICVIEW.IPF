.* PICVIEW.HLP: help for rs2's Picture Viewer (userland/picview,
.* OS2\APPS\PICVIEW.EXE).
.*
.* Written for rs2. The res= numbers are the panels the viewer's help
.* table asks for (the numbers OS/2's Picture Viewer uses, so the
.* desktop's references to them work). xtask compiles this file with
.* crates/ipf into OS2\HELP\PICVIEW.HLP.
:userdoc.
:title.Picture Viewer Help
.*
.* ---- General help (the main window's extended help) ----
:h1 res=7200 id=general.Picture Viewer
:i1.Picture Viewer
:i1.metafiles
:i1.spool files
:p.The Picture Viewer shows pictures kept as :hp2.metafiles:ehp2. (files
ending in .MET) and :hp2.spool files:ehp2. (files ending in .SPL, the
print jobs the spooler keeps for a printer). A metafile holds a picture as
drawing instructions rather than dots, so it can be shown at any size
without losing detail; a spool file is a metafile with one or more pages.
:p.To look at a picture, open it from a folder (the Picture Viewer is the
program for .MET files), drop it on the Picture Viewer object, choose
:link reftype=hd res=7202.Open:elink. from the File menu, or type
:hp2.PICVIEW:ehp2. and the file's name at a command prompt.
:p.The picture is drawn as large as the window allows, with its
proportions kept. Make the window larger to see more detail. The title bar
shows the file's name, and for a picture of several pages the page shown.
:p.With the Picture Viewer you can also&colon.
:ul compact.
:li.:link reftype=hd res=7204.print:elink. a picture;
:li.:link reftype=hd res=7203.save:elink. it in another file;
:li.copy it to the :link reftype=hd res=7206.clipboard:elink. for other
programs, or show a picture from there;
:li.show several pictures one after the other, a
:link reftype=hd res=7250.sequence:elink..
:eul.
:p.The Picture Viewer remembers the size and place of its window and the
:link reftype=hd res=7214.options:elink. you choose.
:p.Typed at a command prompt, :hp2.PICVIEW /P:ehp2. followed by a file name
prints the picture on the default printer without showing a window, and
:hp2.PICVIEW /S:ehp2. starts the window where the system places new
windows instead of where it was last time.
.*
:h2 res=7244 id=keys.Keys
:i1.keys
:p.These keys work in the Picture Viewer&colon.
:dl tsize=22 compact.
:dt.Ctrl+O
:dd.Open a picture.
:dt.Ctrl+S
:dd.Save the picture in another file.
:dt.Shift+Print Screen
:dd.Print the picture.
:dt.Shift+Delete
:dd.Cut the picture to the clipboard.
:dt.Ctrl+Insert
:dd.Copy the picture to the clipboard.
:dt.Shift+Insert
:dd.Show the picture on the clipboard.
:dt.Ctrl+F, Page Down
:dd.The next page.
:dt.Ctrl+B, Page Up
:dd.The page before.
:dt.Ctrl+N
:dd.The next picture of a sequence.
:dt.Ctrl+Q
:dd.End a sequence.
:dt.F1
:dd.Help for the item selected.
:dt.F3
:dd.Close the Picture Viewer.
:edl.
.*
.* ---- File menu ----
:h2 res=7201 id=file.File menu
:i1.File menu
:p.The File menu has&colon.
:dl tsize=12 compact.
:dt.Open
:dd.Show one picture or a sequence of pictures.
:dt.Save as
:dd.Keep the picture shown in a metafile.
:dt.Print
:dd.Print the picture.
:edl.
.*
:h3 res=7202 id=open.Open
:i1.opening pictures
:p.Choose :hp2.Open:ehp2. to show another picture, or several one after the
other. The Open window appears; see
:link reftype=hd res=7222.Picture Viewer - Open:elink..
.*
:h3 res=7203 id=saveas.Save as
:i1.saving pictures
:p.Choose :hp2.Save as:ehp2. to keep the picture shown in a metafile of its
own, for example a picture you took from the clipboard. For a spool file,
the page shown is saved. See
:link reftype=hd res=7231.Picture Viewer - Save As:elink..
.*
:h3 res=7204 id=print.Print
:i1.printing pictures
:p.Choose :hp2.Print:ehp2. to print the picture on the default printer
(the printer object marked as the default in the Printers folder). Every
page of a spool file is printed. Each page is made as large as the paper
allows, its proportions kept.
:p.A window asks how many copies to print; see
:link reftype=hd res=7236.Picture Viewer - Print:elink.. If no printer is set
up, the Picture Viewer says so.
.*
.* ---- Edit menu ----
:h2 res=7206 id=edit.Edit menu
:i1.Edit menu
:i1.clipboard
:p.The Edit menu moves pictures between the Picture Viewer and other
programs through the clipboard. A picture goes there as a metafile, which
programs such as the Clipboard Viewer show and drawing programs can paste.
:dl tsize=8 compact.
:dt.Cut
:dd.Put the picture on the clipboard and clear the window.
:dt.Copy
:dd.Put the picture on the clipboard.
:dt.Paste
:dd.Show the metafile on the clipboard.
:edl.
.*
:h3 res=7207 id=cut.Cut
:p.Choose :hp2.Cut:ehp2. to put the picture shown on the clipboard and clear
the window.
.*
:h3 res=7208 id=copy.Copy
:p.Choose :hp2.Copy:ehp2. to put the picture shown on the clipboard. The
picture stays in the window.
.*
:h3 res=7209 id=paste.Paste
:p.Choose :hp2.Paste:ehp2. to show the picture on the clipboard. It is
available only when the clipboard holds a metafile. The title bar then
says :hp2.Clipboard:ehp2. in place of a file name; use
:link reftype=hd res=7203.Save as:elink. to keep the picture in a file.
.*
.* ---- Page menu ----
:h2 res=7210 id=page.Page menu
:i1.Page menu
:i1.pages
:p.A spool file can hold several pages (a document printed by a program
is kept so until it is printed). The Page menu moves between them; the
title bar shows the page shown and how many there are.
:dl tsize=12 compact.
:dt.Forward
:dd.The next page.
:dt.Backward
:dd.The page before.
:dt.Go to page
:dd.A page by its number.
:edl.
.*
:h3 res=7211 id=forward.Forward
:p.Choose :hp2.Forward:ehp2. to show the next page.
.*
:h3 res=7212 id=backward.Backward
:p.Choose :hp2.Backward:ehp2. to show the page before.
.*
:h3 res=7213 id=goto.Go to page
:p.Choose :hp2.Go to page:ehp2. to show a page by its number. See
:link reftype=hd res=7239.Picture Viewer - Page:elink..
.*
.* ---- Options menu ----
:h2 res=7214 id=options.Options menu
:i1.Options menu
:p.The Options menu decides how the picture fits the window and what is
behind it. The Picture Viewer keeps your choices for the next time.
:dl tsize=26 compact.
:dt.Fit picture in window
:dd.Make the drawing itself as large as the window allows.
:dt.Fit page in window
:dd.Show the whole page the picture was drawn on.
:dt.Default background
:dd.Show the picture on the window color.
:dt.Black background
:dd.Show the picture on black.
:dt.Code page
:dd.The country PIF pictures come from.
:edl.
.*
:h3 res=7215 id=fitpicture.Fit picture in window
:p.Choose :hp2.Fit picture in window:ehp2. to show the part of the page
the picture covers as large as the window allows. Empty margins of the
page are left out. This is the choice the Picture Viewer starts with.
.*
:h3 res=7216 id=fitpage.Fit page in window
:p.Choose :hp2.Fit page in window:ehp2. to show the whole page the picture
was drawn on, margins and all, as it would be printed.
.*
:h3 res=7217 id=defback.Default background
:p.Choose :hp2.Default background:ehp2. to draw the picture on the color
of windows.
.*
:h3 res=7218 id=blackback.Black background
:p.Choose :hp2.Black background:ehp2. to draw the picture on black, for
pictures made for a dark background.
.*
:h3 res=7219 id=codepage.Code page
:p.Choose :hp2.Code page:ehp2. to tell where PIF pictures (pictures in the
Picture Interchange Format) come from, so that their text can be read in
the right code page. See
:link reftype=hd res=7240.Picture Viewer - Code Page:elink..
:note.The Picture Viewer of rs2 does not read PIF files yet; the choice is
kept for when it does.
.*
.* ---- Sequence menu ----
:h2 res=7250 id=sequence.Sequence menu
:i1.Sequence menu
:i1.slide show
:p.When you choose several files in the Open window, the Picture Viewer
shows them one after the other&colon. a :hp2.sequence:ehp2.. Shown
interactively, each picture stays until you choose :hp2.Next picture:ehp2.;
shown automatically, the next one follows after the delay you chose.
.*
:h3 res=7220 id=next.Next picture
:p.Choose :hp2.Next picture:ehp2. to show the next picture of the
sequence. After the last one the sequence ends.
.*
:h3 res=7221 id=quit.Quit sequence
:p.Choose :hp2.Quit sequence:ehp2. to end the sequence. The picture shown
stays in the window.
.*
.* ---- Help menu ----
:h2 res=6422 id=helpmenu.Help menu
:i1.Help menu
:p.The Help menu has&colon.
:dl tsize=22 compact.
:dt.Help index
:dd.The help topics in alphabetic order.
:dt.General help
:dd.What the Picture Viewer does and how to use it.
:dt.Using help
:dd.How to use the help windows.
:dt.Keys help
:dd.The keys of the Picture Viewer.
:dt.Product information
:dd.The program's name and version.
:edl.
.*
:h3 res=6423 id=usinghelp.Using help
:p.Choose :hp2.Using help:ehp2. to learn how the help windows work.
.*
:h3 res=6424 id=generalhelp.General help
:p.Choose :hp2.General help:ehp2. for an overview of the Picture Viewer.
.*
:h3 res=6425 id=keyshelp.Keys help
:p.Choose :hp2.Keys help:ehp2. for the keys of the Picture Viewer.
.*
:h3 res=6426 id=helpindex.Help index
:p.Choose :hp2.Help index:ehp2. to see the help topics in alphabetic order.
.*
:h3 res=6496 id=product.Product information
:p.Shows the name and version of the Picture Viewer.
.*
.* ---- The windows ----
:h2 res=7222 id=opendlg.Picture Viewer - Open
:i1.Open window
:p.Use this window to choose the pictures to show.
:p.:hp2.Current path:ehp2. is the directory whose files are listed. Choose
another drive in :hp2.Drives:ehp2.; double-click a directory in
:hp2.Directories:ehp2. to go into it, or :hp2...:ehp2. to go up.
:p.:hp2.Files:ehp2. lists the files of the directory that match the kind
shown. The buttons :hp2.*.MET:ehp2., :hp2.*.PIF:ehp2. and :hp2.*.SPL:ehp2.
list metafiles, PIF files or spool files. You can also type a pattern such
as :hp2.*.MET:ehp2. or a directory in :hp2.File name:ehp2. and press Enter.
:p.Select one file, or several (with Ctrl or Shift while you click), and
choose :hp2.Open:ehp2.; or type the file's name. Several files are shown as
a sequence&colon.
:dl tsize=18 compact.
:dt.Interactively
:dd.Each picture stays until you choose Next picture in the Sequence menu
(Ctrl+N).
:dt.Automatically
:dd.The next picture follows after :hp2.Delay:ehp2. seconds (1 to 999).
:edl.
:p.Choose :hp2.Cancel:ehp2. to close the window without opening anything.
At most 50 files make a sequence.
.*
:h2 res=7231 id=savedlg.Picture Viewer - Save As
:i1.Save As window
:p.Use this window to save the picture in a file. Choose the drive and the
directory as in the Open window, then type the file's name and choose
:hp2.Save:ehp2.. A name without an extension gets .MET. If the file
exists, the Picture Viewer asks before replacing it.
.*
:h2 res=7236 id=printdlg.Picture Viewer - Print
:i1.copies
:p.Type the number of copies to print (1 to 99) and choose :hp2.OK:ehp2..
The picture goes to the default printer as one print job.
.*
:h2 res=7239 id=pagedlg.Picture Viewer - Page
:p.Type the number of the page to show and choose :hp2.OK:ehp2.. The
window says how many pages there are.
.*
:h2 res=7240 id=cpdlg.Picture Viewer - Code Page
:i1.PIF files
:p.A PIF file keeps its text in the code page of the country it was made
in. Select that country in the list and choose :hp2.OK:ehp2.;
:hp2.Default:ehp2. selects the usual choice again. Check :hp2.Use an EBCDIC
code page for metafile text:ehp2. for pictures whose text was written on a
host computer.
:euserdoc.
