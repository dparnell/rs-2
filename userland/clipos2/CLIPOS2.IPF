.* CLIPOS2.HLP: help for rs2's Clipboard Viewer (userland/clipos2,
.* OS2\CLIPOS2.EXE).
.*
.* Written for rs2. The res= numbers are the panels the viewer's help
.* table asks for (the numbers OS/2's Clipboard Viewer uses). xtask
.* compiles this file with crates/ipf into OS2\HELP\CLIPOS2.HLP.
:userdoc.
:title.Clipboard Viewer Help
.*
:h1 res=21221 id=general.Clipboard Viewer
:i1.Clipboard Viewer
:i1.clipboard
:p.The Clipboard Viewer shows what is on the clipboard. Programs put text,
pictures and other data on the clipboard when you choose :hp2.Copy:ehp2.
or :hp2.Cut:ehp2., and take it from there when you choose
:hp2.Paste:ehp2.. While the viewer is open it follows every change&colon.
the window always shows what a paste would give.
:p.The title bar names the format of the data shown, for example
:hp2.Clipboard (Text):ehp2. or :hp2.Clipboard (Metafile):ehp2.. When the
clipboard is empty the title says :hp2.Clipboard (empty):ehp2..
:p.The viewer shows&colon.
:ul.
:li.text, line by line;
:li.bitmaps, at their size in pels;
:li.metafiles (pictures such as those of the Picture Viewer), at the size
they were drawn for;
:li.data the program that put it there draws itself.
:eul.
:p.When the data is larger than the window, use the scroll bars or the
cursor keys and Page Up and Page Down to see the rest.
:p.A program often puts the same data on the clipboard in several formats.
The viewer shows the one most suited for display; to see another, use
:link reftype=hd res=21218.Render:elink. in the Display menu.
:p.Only one Clipboard Viewer can run at a time.
.*
:h2 res=21214 id=file.File menu
:i1.File menu
:p.:hp2.Exit:ehp2. closes the Clipboard Viewer. The data on the clipboard
stays there.
.*
:h2 res=21217 id=display.Display menu
:i1.Display menu
:p.:hp2.Render:ehp2. lists the formats of the data on the clipboard so you
can choose the one to show.
.*
:h2 res=21218 id=render.Render
:i1.Render
:p.Choose :hp2.Render:ehp2. to see the data on the clipboard in another of
its formats.
:p.The Render window lists the formats the clipboard holds. Select one and
choose :hp2.OK:ehp2. (or double-click it). The viewer then shows that
format until the clipboard changes to data without it.
.*
:h2 res=21219 id=exit.Exit
:p.Choose :hp2.Exit:ehp2. to close the Clipboard Viewer.
.*
:h2 res=21224 id=renderdlg.Render - Available formats
:i1.formats
:p.The list shows each format the data on the clipboard is in&colon.
:dl tsize=20.
:dt.Text
:dd.Plain text.
:dt.Display Text
:dd.Text meant only for showing, put there by programs whose own format
cannot be shown.
:dt.BITMAP
:dd.A picture made of pels.
:dt.Display BITMAP
:dd.A bitmap meant only for showing.
:dt.Metafile
:dd.A picture made of drawing orders, which can be drawn at any size.
:dt.Display Metafile
:dd.A metafile meant only for showing.
:edl.
:p.Other formats are the programs' own; the list shows their names.
.*
:h3 res=21222 id=renderok.OK
:p.Shows the format selected.
.*
:h3 res=21223 id=rendercancel.Cancel
:p.Closes the window without changing what is shown.
.*
:h2 res=64022 id=helpmenu.Help menu
:i1.Help menu
:p.The Help menu has&colon.
:dl tsize=22.
:dt.Help index
:dd.An alphabetic list of the help topics.
:dt.General help
:dd.What the Clipboard Viewer is for and how to use it.
:dt.Using help
:dd.How to use the help itself.
:dt.Product information
:dd.The program's name and version.
:edl.
.*
:h2 res=64027 id=usinghelp.Using help
:p.Choose :hp2.Using help:ehp2. to learn how the help windows work.
.*
:h2 res=64044 id=helpindex.Help index
:p.Choose :hp2.Help index:ehp2. to see the help topics in alphabetic order.
.*
:h2 res=64096 id=product.Product information
:p.Shows the name and version of the Clipboard Viewer.
:euserdoc.
