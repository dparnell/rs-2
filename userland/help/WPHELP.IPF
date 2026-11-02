.* WPHELP.HLP: help for rs2's Workplace Shell (userland/pmwp, PMWP.DLL).
.*
.* Written for rs2, in rs2's own words. The res= numbers are the panel
.* numbers OS/2's shell and other programs ask for, so programs written
.* for OS/2 find the topics they expect. Panels of rs2's own use numbers
.* from 0x9000 on. The Help Index object merges the :i1. entries.
:userdoc.
:title.Workplace Shell Help
.*
.* ==== Using the desktop ====
:h1 res=0x8279 id=desktopuse.Using the desktop
:i1.desktop, using
:i1.Workplace Shell
:i1.getting started
:p.The desktop is the screen you see when rs2 has started. Everything on
it is an :hp2.object:ehp2.&colon. a folder, a program, a file, a drive, a
printer or a tool such as the Shredder. Each object is shown by an icon
with its title under it.
:p.You work with objects in the same few ways everywhere&colon.
:ul.
:li.Double-click an object with mouse button 1 to open it.
:li.Click an object with mouse button 2 to see its pop-up menu, the list
of what you can do with it.
:li.Drag an object with mouse button 2 to move, copy or link it, or to
drop it on another object (a printer, the Shredder, a program).
:li.Open an object's settings notebook (Open, then Settings, from its
pop-up menu) to change how it looks and behaves.
:eul.
:p.Click mouse button 2 on an empty part of the desktop for the
desktop's own menu&colon. there you find the Window list, Lockup now and
Shut down.
:p.The objects rs2 puts on the desktop are&colon.
:sl.
:li.:link reftype=hd res=0x1f4d.Drives:elink., one object per disk drive
:li.:link reftype=hd res=0x1f48.Command Prompts:elink., OS/2 and DOS
command lines
:li.Programs, the programs that come with rs2 and those you install
:li.:link reftype=hd res=0x1f42.Startup:elink., objects opened each time
the desktop starts
:li.:link reftype=hd res=0x4c4.System Setup:elink., where you set up the
system
:li.:link reftype=hd res=0x3d40.Templates:elink., for making new objects
:li.:link reftype=hd res=0x83bb.Assistance Center:elink., books and help
:li.Connections, with :link reftype=hd res=0x7549.Network:elink. and the
Printers folder
:li.:link reftype=hd res=0x2446.Minimized Window Viewer:elink.,
:link reftype=hd res=0x36c6.Window List:elink.,
:link reftype=hd res=0x4a6.Shredder:elink. and
:link reftype=hd res=0xfa1.Shut Down:elink.
:li.The :link reftype=hd res=0x7e90.LaunchPad:elink., the bar of buttons at
the bottom of the screen
:esl.
:p.See also :link reftype=hd res=0x9000.Using the mouse:elink.,
:link reftype=hd res=0x9001.Drag and drop:elink.,
:link reftype=hd res=0x7d7d.Objects:elink. and
:link reftype=hd res=0x3e8.Help information:elink..
.*
.* ==== Help information ====
:h1 res=0x3e8 id=helpinfo.Help information
:i1.help, getting
:i1.F1 key
:i1.Help Index
:p.There are several ways to get help in rs2&colon.
:ul.
:li.Press :hp2.F1:ehp2. in a window, a dialog or a settings page that
offers help. The help window shows the topic for where you are.
:li.Choose an item from a program's :hp2.Help:ehp2. menu. Most programs
have Help index, General help, Using help and Keys help.
:li.Open the :link reftype=hd res=0x37df.Help Index:elink. in the
Information folder of the Assistance Center. It lists the topics of every
help file on the system in one alphabetical list.
:li.Open the books in the :link reftype=hd res=0x7ea0.Information:elink.
folder, or type :hp2.VIEW:ehp2. and a book's name at a command prompt
(for example, VIEW CMDREF for the Command Reference, once it is
installed).
:li.At a command prompt, type :hp2.HELP:ehp2. for a list of commands,
HELP and a command's name for a line about it, or HELP and a message
number (HELP 2 or HELP SYS0002) for what a message means.
:eul.
:p.In the help window&colon.
:ul.
:li.Words in a different colour are links. Click one, or move to it with
Tab and press Enter, to see that topic.
:li.Esc or Backspace goes back to the topic you saw before.
:li.The Options menu shows the contents of the help file and the topics
you have viewed; the Services menu searches, prints and copies topics.
:eul.
:p.See also :link reftype=hd res=0x1374.Help window keys:elink..
.*
.* ==== Mouse and drag and drop (rs2's own panels) ====
:h1 res=0x9000 id=mouse_use.Using the mouse
:i1.mouse, using
:i1.mouse buttons
:i1.selecting objects
:p.The mouse has two jobs in the Workplace Shell. Mouse button 1 (usually
the left one) selects and opens; mouse button 2 (usually the right one)
shows menus and drags. You can swap them on the
:link reftype=hd res=0x44c.Mouse:elink. object's Setup page.
:dl tsize=24.
:dt.Click button 1
:dd.Select one object. Other selected objects in the window are
deselected.
:dt.Ctrl+click button 1
:dd.Add an object to the selection, or take it out.
:dt.Shift+click button 1
:dd.Select every object from the last one selected to this one.
:dt.Double-click button 1
:dd.Open the object in its default view.
:dt.Alt+click button 1
:dd.Change the object's title in place.
:dt.Click button 2
:dd.Show the pop-up menu of the object, or of the window when you click
an empty part of it.
:dt.Drag with button 2
:dd.Move, copy or link objects. See
:link reftype=hd res=0x9001.Drag and drop:elink..
:edl.
.*
:h1 res=0x9001 id=dragdrop.Drag and drop
:i1.drag and drop
:i1.dragging objects
:i1.dropping objects
:p.To drag an object, point to it, press and hold mouse button 2, move
the pointer to where you want the object and release the button. If
several objects are selected, they all go.
:p.What happens depends on where you drop and on the keys you hold&colon.
:dl tsize=22.
:dt.No key
:dd.The objects move to the folder. Files dropped on a folder on another
drive are copied.
:dt.Ctrl
:dd.The objects are copied.
:dt.Shift
:dd.The objects are moved.
:dt.Ctrl+Shift
:dd.A :link reftype=hd res=0x4b3.shadow:elink. of each object is made.
:edl.
:p.Dropping on other objects&colon.
:ul.
:li.On a program or program file&colon. the program starts with the
dropped files.
:li.On a printer&colon. the files are printed.
:li.On the :link reftype=hd res=0x4a6.Shredder:elink.&colon. the objects
are deleted, after you confirm.
:li.On a shadow or a drive&colon. the drop goes to the object it stands
for.
:li.On the :link reftype=hd res=0x7e90.LaunchPad:elink.&colon. a button for
the object is added.
:li.Dragging within the same icon view only moves the icon.
:eul.
:p.Dragging a :link reftype=hd res=0xfb6.template:elink. makes a new object
where you drop it. Colours, schemes and fonts are dragged from the
palettes in System Setup the same way.
:p.Files dragged from other programs can be dropped on folders too.
.*
.* ==== Objects ====
:h1 res=0x7d7d id=objects.Objects
:i1.objects
:i1.pop-up menu
:i1.object, title
:p.Everything on the desktop and in folders is an object. Objects of the
same kind belong to a class&colon. all folders behave alike, all program
objects behave alike, and so on. Some objects are files and directories
on a disk (folders, data files, program files); others exist only in the
desktop's records (program objects, shadows, the Shredder, the
LaunchPad).
:p.The pop-up menu of an object (mouse button 2, or Shift+F10) has
these items, as far as the object allows them&colon.
:dl tsize=20.
:dt.Open
:dd.The object's views&colon. Settings, and for folders and drives Icon
view, Tree view and Details view; Program for programs.
:link reftype=hd res=0x4ae.More:elink.
:dt.Find...
:dd.Folders and drives&colon. look for objects in them.
:link reftype=hd res=0x4b5.More:elink.
:dt.Refresh now
:dd.Folders&colon. show the folder's content as it is on disk now.
:dt.Rename
:dd.Change the title. :link reftype=hd res=0x84fc.More:elink.
:dt.Copy...
:dd.:link reftype=hd res=0xbec.Copy:elink. the object to a folder.
:dt.Move...
:dd.:link reftype=hd res=0x4b2.Move:elink. the object to a folder.
:dt.Create another...
:dd.:link reftype=hd res=0x4b0.Make a new object like this one:elink..
:dt.Create shadow...
:dd.:link reftype=hd res=0x4b3.Make a shadow:elink. of it in a folder.
:dt.Delete...
:dd.:link reftype=hd res=0x4b4.Delete:elink. the object.
:dt.Print
:dd.Data files&colon. :link reftype=hd res=0x4bb.print:elink. the file.
:edl.
:p.Items that do not apply are left out or shown greyed. An object can be
made so that it cannot be deleted, copied, moved or renamed; the desktop's
own objects such as System and the Shredder are made that way.
:p.A folder whose menus are set to :hp2.short menus:ehp2. (its Menu
page) leaves Rename, Copy, Move, Create another and Create shadow out of
the menus of its objects. Items you add on an object's Menu page appear
at the end of its menu.
:p.See also :link reftype=hd res=0x7d8a.Settings notebook:elink. and
:link reftype=hd res=0x1377.Object keys:elink..
.*
.* ==== Desktop ====
:h1 res=0xfa0 id=desktop.Desktop
:i1.desktop
:i1.desktop, menu
:i1.desktop, settings
:p.The desktop is a folder that fills the screen, behind every window.
Its content is the directory DESKTOP on the boot drive, plus the
desktop's own objects.
:p.Click mouse button 2 on an empty part of the desktop for its menu&colon.
:dl tsize=20.
:dt.Open
:dd.Settings, and the desktop as an Icon, Tree or Details view.
:dt.Find...
:dd.:link reftype=hd res=0x4b5.Find:elink. objects on the desktop and in
its folders.
:dt.Refresh now
:dd.:link reftype=hd res=0x7d0c.Read the desktop's content again:elink..
:dt.Sort
:dd.:link reftype=hd res=0x2422.Sort the icons:elink..
:dt.Arrange
:dd.:link reftype=hd res=0x4bf.Line up the icons:elink..
:dt.Window list
:dd.Show the :link reftype=hd res=0x36c6.Window List:elink..
:dt.Lockup now
:dd.:link reftype=hd res=0x7d89.Lock the system:elink..
:dt.Shut down...
:dd.:link reftype=hd res=0xfa1.Close everything and turn off:elink..
:edl.
:p.The desktop's settings notebook has these pages before the usual
folder pages&colon.
:ul.
:li.:hp2.Lockup:ehp2. (three pages)&colon. the lockup password, automatic
lockup and how the lockup screen looks.
:li.:hp2.Archive:ehp2.&colon. :hp2.Create archive at each system
startup:ehp2. copies OS2.INI, OS2SYS.INI, CONFIG.SYS, STARTUP.CMD and the
desktop directory to \OS2\ARCHIVES each time the desktop starts. The last
three archives are kept. To go back to one, copy its files back; rs2 has
no recovery screen at boot.
:li.:hp2.Desktop:ehp2.&colon. whether the desktop's settings are saved,
and what happens to objects dropped on the desktop from another folder
(they move, or a shadow is made).
:eul.
:p.The desktop's background colour is set on its Background page, or by
dragging a colour from the :link reftype=hd res=0x3cf1.Color
Palette:elink..
.*
.* ==== Folder ====
:h1 res=0x3d45 id=folder.Folder
:i1.folders
:i1.folder, settings
:i1.folder, background
:p.A folder holds other objects, including other folders. A folder on a
disk is a directory; its objects are the files and directories in it,
and the program objects and shadows you put there.
:p.Double-click a folder to open it in its default view. The folder's
menu (mouse button 2 on its icon) has Open with
:link reftype=hd res=0x7df4.Icon view:elink.,
:link reftype=hd res=0x7df5.Tree view:elink. and
:link reftype=hd res=0x7df6.Details view:elink., Find..., Refresh now and
the usual :link reftype=hd res=0x7d7d.object items:elink.. Mouse button 2
on an empty part of an open folder window gives the folder's menu with
Sort and Arrange.
:p.To make a new folder, drag the Folder template from the
:link reftype=hd res=0x3d40.Templates:elink. folder.
:p.The settings notebook of a folder has these pages&colon.
:dl tsize=14.
:dt.View
:dd.Three pages&colon. the view the folder opens in, how icons are
placed (as placed, flowed or in one column), their size, the font; the
tree view's lines and icons; the details view's columns.
:dt.Sort
:dd.The order of the objects. See :link reftype=hd res=0x2422.Sort:elink..
:dt.Include
:dd.Name patterns, such as *.TXT;*.DOC, that say which objects the
folder shows. Folders are always shown.
:dt.Background
:dd.The folder window's colour.
:dt.Menu
:dd.Your own items for the menus of the folder's objects, and long or
short menus.
:dt.File
:dd.Three pages&colon. the subject and real name; dates, size and
attributes; comments, key phrases and history.
:dt.Window
:dd.Whether opening the folder again shows the open window or a new one,
and whether opening an object from it closes the folder.
:dt.General
:dd.The title, and whether the folder is a template.
:edl.
:p.Folder menu bars are not available in rs2; the setting is kept.
.*
.* ==== Data file ====
:h1 res=0x3d46 id=datafile.Data file
:i1.data files
:i1.files, opening
:i1.associations
:i1.file type
:p.A data file is a file that holds information&colon. a text, a picture,
a document. Opening it (double-click) starts the program associated with
it, with the file. A data file is associated with a program by its
:hp2.type:ehp2. (the Type page) or by its name, for example *.TXT. A file
with no association opens in the System Editor.
:p.The pop-up menu has the usual :link reftype=hd res=0x7d7d.object
items:elink. and :link reftype=hd res=0x4bb.Print:elink., which prints the
file on the default printer.
:p.The settings notebook has these pages&colon.
:dl tsize=14.
:dt.Type
:dd.The file's types. Choose from the list of types the system knows.
:dt.Menu
:dd.Your own items for the file's pop-up menu.
:dt.File
:dd.Subject, real name, dates, size, attributes, comments, key phrases
and history.
:dt.Window
:dd.What happens when the file is opened while it is already open.
:dt.General
:dd.The title, and whether the file is a template.
:edl.
:p.When you change a file's extension by renaming it, you are asked
whether you meant to, since the extension can decide which program opens
it.
:p.To make a new empty data file, drag the Data File template from the
Templates folder.
.*
.* ==== Program ====
:h1 res=0x3d44 id=program.Program
:i1.program objects
:i1.programs, starting
:i1.programs, settings
:p.A program object starts a program. It is not the program file itself
but a reference to it, with the settings to start it with. Double-click it
to start the program; drop files on it to start the program with those
files.
:p.The settings notebook has these pages&colon.
:dl tsize=16.
:dt.Program
:dd.The path and file name of the program, its parameters and its
working directory. An asterisk (*) as the file name starts a command
prompt.
:dt.Session
:dd.How the program runs&colon. in a window or full screen, OS/2 or DOS.
:dt.Compatibility
:dd.rs2's own page&colon. the colours the program sees, the display's or
256, for older programs that need a palette.
:dt.Association
:dd.The file types and names (such as *.TXT) the program opens. Data
files of those types and names open with this program.
:dt.Window
:dd.Whether opening the object again starts another copy or shows the
running one.
:dt.General
:dd.The title, and whether the object is a template.
:edl.
:p.To make a program object, drag the Program template from the
:link reftype=hd res=0x3d40.Templates:elink. folder and fill in the
Program page. :link reftype=hd res=0x9002.Install OS/2 Applications:elink.
makes the program objects for the applications it installs.
.*
.* ==== Program file ====
:h1 res=0xfd7 id=progfile.Program file
:i1.program files
:i1.EXE files
:p.A program file is a file that can be run, such as an .EXE file or a
command file (.CMD). Double-click it to run it, or drop data files on it
to run it with them.
:p.Its pop-up menu's Open item has :hp2.Program:ehp2. (run it) and
Settings. Its settings notebook has an :hp2.Association:ehp2. page, like a
program object's, and the File, Window and General pages of other files.
:p.For a program you use often, make a
:link reftype=hd res=0x4b3.shadow:elink. or a
:link reftype=hd res=0x3d44.program object:elink. of it on the desktop, or
put it on the :link reftype=hd res=0x7e90.LaunchPad:elink..
.*
.* ==== Shadows ====
:h1 res=0x4b3 id=shadow.Shadows (Create shadow)
:i1.shadows
:i1.Create shadow
:i1.links
:p.A shadow is a stand-in for another object. It looks like the original
and opening it opens the original; dropping things on it drops them on
the original. You can have shadows of one object in many folders, and the
original stays where it is.
:p.Shadow titles are drawn in blue (light blue on dark backgrounds). When
the original is deleted, the shadow's title turns grey; you can then
delete the shadow.
:p.To make a shadow&colon.
:ul.
:li.Choose :hp2.Create shadow...:ehp2. from the object's pop-up menu,
then choose the folder the shadow goes in; or
:li.Hold Ctrl+Shift and drag the object to a folder.
:eul.
:p.Deleting a shadow deletes only the shadow, never the original.
.*
.* ==== Drives ====
:h1 res=0x1f4d id=drives.Drives
:i1.Drives folder
:i1.disks
:p.The Drives folder holds an object for each drive of the system&colon.
diskettes, hard disk partitions, CD-ROMs and network drives. Open a drive
object to see the files and directories on it.
:p.To work with one drive, see :link reftype=hd res=0x7d8c.Disk:elink..
.*
:h1 res=0x7d8c id=disk.Disk
:i1.drive objects
:i1.disk, details
:p.A drive object stands for one drive, such as C&colon.. Opening it
shows the drive's root directory as a folder, in icon, tree or details
view. Its pop-up menu has Open, with those views and Settings, and
Find..., which searches the whole drive.
:p.Its settings notebook has a :hp2.Details:ehp2. page with the file
system, the volume label, the serial number, the allocation units and
the free and used space, and Window and General pages.
:p.Checking and formatting disks is not on the drive's menu in rs2; use
the CHKDSK and FORMAT commands at a
:link reftype=hd res=0x1f48.command prompt:elink.. Files dropped on a drive
go to its root directory.
.*
.* ==== Shredder ====
:h1 res=0x4a6 id=shredder.Shredder
:i1.Shredder
:i1.deleting, Shredder
:p.The Shredder deletes what you drop on it. Drag an object (or several
selected objects) to it with mouse button 2; you are asked to confirm, as
for :link reftype=hd res=0x4b4.Delete:elink..
:p.Deleting with the Shredder is final&colon. rs2 has no wastebasket, so
deleted files cannot be brought back.
:p.Other programs can let you drop their items on the Shredder too; each
program decides what deleting its items means.
:p.The Shredder cannot be opened (its only view is its settings) and
stays where it is on the desktop.
.*
.* ==== Templates ====
:h1 res=0x3d40 id=templates.Templates
:i1.Templates folder
:p.The Templates folder holds a template of each kind of object you can
make&colon. a Data File, a Folder and a Program. A Printer template is in
the Printers folder in Connections.
:p.To make an object, drag a template to a folder, or open the template
to make one beside it. See :link reftype=hd res=0xfb6.Template:elink..
.*
:h1 res=0xfb6 id=template.Template
:i1.templates, using
:i1.new objects
:i1.objects, making
:p.A template is a pad of new objects. It is shown as its icon on a pad of
paper.
:ul.
:li.Drag a template to a folder (or the desktop) to make a new object
there.
:li.Open a template (double-click) to make a new object beside it.
:li.Choose :hp2.Create another...:ehp2. from any object's menu to make a
new object like it in a folder you choose.
:eul.
:p.The new object is a copy of the template. When its folder is open, its
title is ready to be typed over&colon. type a name and press Enter.
:p.Any object that can be copied can be made a template&colon. check
:hp2.Template:ehp2. on its General page. Uncheck it to make it an ordinary
object again.
.*
.* ==== System ====
:h1 res=0xfbe id=system.System
:i1.System object
:i1.confirmations
:i1.title clash
:i1.logo display time
:p.The System object, in :link reftype=hd res=0x4c4.System Setup:elink.,
holds settings for the whole desktop. Open it to see its pages&colon.
:dl tsize=16.
:dt.Confirmations
:dd.When the desktop asks before it acts&colon. deleting folders,
deleting objects, renaming extensions, copying and moving, and whether to
show progress while copying.
:dt.Title
:dd.What happens when an object is copied or moved to a folder that has
an object with the same title&colon. ask, rename the new one, or replace
the old one.
:dt.Window
:dd.What the minimize button does (hide the window, or put it in the
Minimized Window Viewer or on the desktop), window animation, and whether
opening an object shows the window already open.
:dt.Print Screen
:dd.Whether the Print Screen key is enabled. The setting is kept, but
rs2 does not print the screen yet.
:dt.Logo
:dd.How long programs show their logo windows&colon. not at all, until
clicked, or for a time.
:edl.
:p.The screen resolution is set with the Display program in System
Setup. rs2's System object has no Screen, Input or Country pages.
.*
.* ==== System Clock ====
:h1 res=0x482 id=clock.System Clock
:i1.clock
:i1.date and time
:i1.time, setting
:p.The System Clock shows the time in a window of its own. Open it from
System Setup.
:p.Its settings notebook has&colon.
:ul.
:li.:hp2.View:ehp2.&colon. an analog clock (with hands and a second hand)
or a digital one, showing the time, the date or both.
:li.:hp2.Date/Time:ehp2.&colon. the computer's date and time. Changing
them here sets the computer's clock.
:eul.
:p.Alarms are not available in rs2.
.*
.* ==== Keyboard ====
:h1 res=0x36bc id=keyboard.Keyboard
:i1.keyboard
:i1.repeat rate
:i1.cursor blink rate
:p.The Keyboard object in System Setup sets how the keyboard
behaves. Its :hp2.Timing:ehp2. page has&colon.
:ul.
:li.the repeat rate&colon. how fast a key held down repeats;
:li.the repeat delay&colon. how long before it starts repeating;
:li.the cursor blink rate.
:eul.
:p.The changes take effect at once and are used from then on, also
after the next start. Key mappings and special needs settings are not
available in rs2; the keyboard layout is set with KEYB and DEVINFO in
CONFIG.SYS.
.*
.* ==== Mouse ====
:h1 res=0x44c id=mouse.Mouse
:i1.mouse, settings
:i1.double-click speed
:i1.left-handed mouse
:p.The Mouse object in System Setup sets how the mouse behaves&colon.
:ul.
:li.:hp2.Timing:ehp2.&colon. the double-click time, how quickly two clicks
must follow each other to count as a double-click.
:li.:hp2.Setup:ehp2.&colon. right-handed or left-handed. Left-handed swaps
the buttons, so that mouse button 1 is the right button.
:eul.
:p.Pointer shapes and the other mouse pages of OS/2 are not available in
rs2. See also :link reftype=hd res=0x9000.Using the mouse:elink..
.*
.* ==== Sound ====
:h1 res=0x4a2 id=sound.Sound
:i1.sound
:i1.warning beep
:p.The Sound object in System Setup has a :hp2.Warning beep:ehp2. page&colon.
check it to have the computer beep when a program shows a warning or
you do something that is not possible; uncheck it for silence.
:p.Sounds for system events are not available in rs2.
.*
.* ==== Palettes ====
:h1 res=0x3cf1 id=colorpal.Color Palette
:i1.colours
:i1.Color Palette
:i1.background colour
:p.The Color Palette holds colours you can drag onto windows.
:ul.
:li.Drag a colour with mouse button 2 to a folder window or the desktop
to make it the background colour.
:li.Hold Shift while you drop to change the colour of the text instead.
:li.Dropped on another window, the colour becomes that window's
background (or, with Shift, its text colour), if the window uses
it.
:eul.
:p.To change the colours of every window at once, use the
:link reftype=hd res=0x17a6.Scheme Palette:elink..
.*
:h1 res=0x17a6 id=schemepal.Scheme Palette
:i1.colour schemes
:i1.Scheme Palette
:p.The Scheme Palette holds colour schemes&colon. sets of colours for
every part of a window (title bars, borders, menus, window backgrounds,
the desktop and so on).
:p.Drag a scheme with mouse button 2 and drop it on any window. The
scheme becomes the colours of the whole system, and every window is drawn
again in them. The scheme is remembered for the next start.
.*
:h1 res=0x1f82 id=fontpal.Font Palette
:i1.fonts
:i1.Font Palette
:p.The Font Palette holds fonts you can drag onto windows.
:ul.
:li.Dropped on a folder window, the font is used for the folder's
titles. The folder keeps it.
:li.Dropped on another window, that window and the windows in it use
the font.
:eul.
:p.A folder's font can also be set on its View page, whose Font... button
opens the Font Palette.
.*
.* ==== Help Index and Glossary ====
:h1 res=0x37df id=helpindex.Help Index
:i1.Help Index
:i1.index of help topics
:i1.searching help
:p.The Help Index lists the topics of all the help files on the system
(the files in the directories named by HELP in CONFIG.SYS), merged into
one alphabetical list. It is in the
:link reftype=hd res=0x7ea0.Information:elink. folder.
:ul.
:li.Type in the search field to go to the first entry that starts with
what you type.
:li.Click a letter tab on the right to jump to the entries starting with
that letter.
:li.Double-click an entry, or select it and press Enter, to see its
topic in a help window.
:eul.
:p.When the same entry belongs to several topics, the topic's title
follows it. Entries under another entry are indented.
:p.Help files installed later (with
:link reftype=hd res=0x9002.Install OS/2 Applications:elink.) appear the
next time the index is opened.
.*
:h1 res=0x13a7 id=glossary.Glossary
:i1.Glossary
:i1.terms
:p.The Glossary explains words used in OS/2 and rs2. It works like the
:link reftype=hd res=0x37df.Help Index:elink.&colon. open it, type the start
of a word or click a letter tab, and choose an entry to read its
meaning.
:p.It shows the glossary files in \OS2\HELP\GLOSS (the GLOSSARY setting
in CONFIG.SYS). If none are installed, the list is empty. The Glossary is
in the Online Information Overview folder of the Information folder.
.*
.* ==== Network, printing ====
:h1 res=0x7549 id=network.Network
:i1.network
:i1.Connections folder
:p.The Network folder is in the Connections folder. On OS/2 a LAN
requester fills it with the network groups and servers it finds.
:p.rs2 has no LAN requester yet, so the folder holds only what you put
in it, and its Network page says so. rs2 does have TCP/IP; network
printers are reached through LPR and IPP ports, which a printer's Output
port page sets up.
.*
:h1 res=0x3d72 id=spooler.Spooler
:i1.spooler
:i1.print jobs, spool
:p.The Spooler object in System Setup sets up the print spooler, which
keeps print jobs on disk until the printer takes them.
:ul.
:li.:hp2.Spool path:ehp2.&colon. the directory the jobs are kept in.
:li.:hp2.Print priority:ehp2.&colon. how much of the computer's time
printing gets.
:li.Its pop-up menu has :hp2.Enable spooler:ehp2. or :hp2.Disable
spooler:ehp2..
:eul.
:p.rs2 keeps the Disable and Print priority settings, but its spooler
always spools.
.*
:h1 res=0x3d6c id=printer.Printer object
:i1.printers
:i1.printing, printer object
:i1.print jobs
:i1.default printer
:p.A printer object stands for a printer queue. Printers are in the
Printers folder in Connections; each queue on the system has one.
:ul.
:li.Open a printer to see its jobs. Each job is an object&colon. its menu
can hold, release, print next or start it again, or delete it.
:li.Drop files on a printer to print them. PostScript files are sent as
they are; other files are printed as text.
:li.The printer's pop-up menu can hold or release the queue, delete all
jobs and make it the default printer.
:eul.
:p.Its settings notebook has these pages&colon.
:dl tsize=16.
:dt.Printer driver
:dd.The driver that prints, and its job properties (paper, quality and
the like). It also makes the printer the default one.
:dt.Output port
:dd.Where the output goes&colon. a parallel or serial port, a file, or a
network printer (LPR or IPP). Install new port adds one.
:dt.Queue options
:dd.Priority, the queue driver, and whether a job dialog is shown.
:edl.
:p.To add a printer, drag or open the Printer template in the Printers
folder&colon. give it a name, choose a driver and a port. Deleting a printer
deletes its queue.
.*
.* ==== URL objects ====
:h1 res=0x848d id=url.URL objects
:i1.URL objects
:i1.Internet objects
:i1.web pages
:p.A URL object holds the address of a page on the Internet. Opening it
starts the web browser with that page. The :hp2.rs2 on the
Internet:ehp2. object in the Assistance Center is one.
:p.The address is on the object's :hp2.URL:ehp2. settings page. A URL
object is a small file whose content is the address, so you can copy it
and send it like any file.
:p.If no web browser is set up, opening the object says so and shows
the address.
.*
.* ==== Minimized Window Viewer, Shut Down, Window List ====
:h1 res=0x2446 id=minviewer.Minimized Window Viewer
:i1.Minimized Window Viewer
:i1.minimized windows
:i1.windows, minimizing
:p.When you minimize a window, rs2 puts an icon for it in the Minimized
Window Viewer, a folder on the desktop. Double-click the icon to bring the
window back; its menu has Open, then Restore.
:p.Whether minimized windows go here, onto the desktop, or are simply
hidden is set on the Window page of the
:link reftype=hd res=0xfbe.System:elink. object. Hidden windows can be
found in the :link reftype=hd res=0x36c6.Window List:elink..
.*
:h1 res=0xfa1 id=shutdown.Shut Down
:i1.shutting down
:i1.restarting
:i1.turning off
:p.Shut down rs2 before you turn the computer off, so that programs can
save their work and everything is written to disk.
:p.Choose :hp2.Shut down...:ehp2. from the desktop's menu, open the Shut
Down object, or click Shut down on the LaunchPad. Then choose&colon.
:ul.
:li.:hp2.Shut down the computer:ehp2.&colon. turn it off; or
:li.:hp2.Restart the computer:ehp2.&colon. start rs2 again.
:eul.
:p.Every program is asked to close. A program with unsaved work may ask
you about it. If programs have not ended after a while, rs2 names them and
asks whether to end them anyway. Then the desktop is saved, the disks are
written and the computer is turned off or restarted. If it cannot be
turned off, a screen says when it is safe to switch off.
.*
:h1 res=0x36c6 id=winlist.Window List
:i1.Window List
:i1.switching programs
:i1.Ctrl+Esc
:p.The Window List shows the programs and windows that are running. Use
it to go to a window, including hidden and minimized ones, or to end a
program that has stopped responding.
:p.To show it, press :hp2.Ctrl+Esc:ehp2., open the Window List object on
the desktop, choose Window list from the desktop's menu, or click Window
list on the LaunchPad. Double-click an entry, or select it and choose
:hp2.Switch to:ehp2., to go to it; :hp2.End task:ehp2. closes the
program.
:p.If even the Window List cannot help, Ctrl+Alt+W shows rs2's rescue
screen, which lists the processes and can end one.
.*
.* ==== Folders on the desktop ====
:h1 res=0x1f42 id=startup.Startup folder
:i1.Startup folder
:i1.programs, at start
:p.The objects in the Startup folder are opened each time the desktop
starts. Put a shadow of a program or folder in it (Ctrl+Shift and drag)
to have it start every time.
:p.rs2 puts a shadow of the LaunchPad there. Take it out if you do not
want the LaunchPad.
.*
:h1 res=0x4c4 id=syssetup.System Setup folder
:i1.System Setup
:i1.settings, system
:p.System Setup holds the objects that set up the system&colon.
:sl.
:li.:link reftype=hd res=0xfbe.System:elink.
:li.:link reftype=hd res=0x482.System Clock:elink.
:li.:link reftype=hd res=0x3d72.Spooler:elink.
:li.:link reftype=hd res=0x36bc.Keyboard:elink.,
:link reftype=hd res=0x44c.Mouse:elink. and
:link reftype=hd res=0x4a2.Sound:elink.
:li.:link reftype=hd res=0x3cf1.Color Palette:elink.,
:link reftype=hd res=0x17a6.Scheme Palette:elink. and
:link reftype=hd res=0x1f82.Font Palette:elink.
:li.Display, rs2's program for the screen resolution and colours
:esl.
:p.The settings are kept in OS2.INI and used again at each start.
.*
:h1 res=0x7ea0 id=infofolder.Information folder
:i1.Information folder
:i1.books
:p.The Information folder, in the Assistance Center, holds the books and
help of the system&colon.
:ul.
:li.:hp2.Reference and Commands:ehp2.&colon. reference books such as the
Command Reference.
:li.:hp2.Tasks:ehp2.&colon. books about doing things.
:li.:hp2.Read Me:ehp2.&colon. the README file.
:li.The :link reftype=hd res=0x37df.Help Index:elink..
:li.:hp2.Online Information Overview:ehp2., with the
:link reftype=hd res=0x13a7.Glossary:elink..
:eul.
:p.Books you install with
:link reftype=hd res=0x9002.Install OS/2 Applications:elink. are put in
these folders. Double-click a book to read it.
.*
:h1 res=0x83bb id=assist.Assistance Center
:i1.Assistance Center
:p.The Assistance Center on the desktop gathers what helps you use
rs2&colon. the :link reftype=hd res=0x7ea0.Information:elink. folder, a
Troubleshooting folder, and :hp2.rs2 on the Internet:ehp2., a
:link reftype=hd res=0x848d.URL object:elink. for rs2's web page.
.*
:h1 res=0x1f48 id=prompts.Command Prompts
:i1.command prompts
:i1.OS/2 window
:i1.DOS window
:p.The Command Prompts folder starts command lines&colon.
:dl tsize=18.
:dt.OS/2 Window
:dd.An OS/2 command prompt in a window on the desktop.
:dt.OS/2 Full Screen
:dd.An OS/2 command prompt using the whole screen. Press Ctrl+Esc to get
back to the desktop.
:dt.DOS Window
:dd.A DOS command prompt in a window.
:dt.DOS Full Screen
:dd.A DOS command prompt using the whole screen.
:edl.
:p.Type EXIT to close a command prompt. Type HELP for a list of
commands.
.*
:h1 res=0xfa2 id=os2sys.OS/2 System folder
:i1.OS/2 System folder
:p.On OS/2, the OS/2 System folder gathers the system's folders. rs2's
desktop has no such folder&colon. the folders it would hold are on the
desktop itself&colon. :link reftype=hd res=0x4c4.System Setup:elink.,
:link reftype=hd res=0x1f48.Command Prompts:elink.,
:link reftype=hd res=0x1f42.Startup:elink.,
:link reftype=hd res=0x1f4d.Drives:elink. and the
:link reftype=hd res=0x2446.Minimized Window Viewer:elink..
.*
.* ==== Lockup ====
:h1 res=0x7d89 id=lockup.Lockup
:i1.lockup
:i1.password, lockup
:i1.screen saver
:p.Lockup protects your desktop while you are away. The screen is
covered and nothing can be done until the lockup password is typed.
:p.Choose :hp2.Lockup now:ehp2. from the desktop's menu. The first time,
you are asked for a password (1 to 15 characters, typed twice). To
unlock, type the password and press Enter.
:p.The Lockup pages of the desktop's settings notebook set&colon.
:ul.
:li.the password, or screen saver mode, in which any key unlocks;
:li.the lockup screen's background colour;
:li.automatic lockup after 1 to 99 minutes without use, and lockup each
time the system starts;
:li.whether the lockup covers the whole screen or most of it, and
auto-dim, which blanks the screen after two idle minutes.
:eul.
:p.The background picture setting is kept, but rs2 draws only the
colour.
.*
.* ==== Menu items ====
:h1 res=0x4b5 id=find.Find
:i1.Find
:i1.searching for objects
:p.Find looks for objects by name. Choose :hp2.Find...:ehp2. from the menu
of a folder, a drive or the desktop, or click Find on the LaunchPad.
:p.Type a name, or a pattern with wildcards&colon. * stands for any
characters and ? for any one character (for example, *.TXT). Check
:hp2.Search all subfolders:ehp2. to look in the folders inside too.
:p.The objects found are shown in a Find Results window. There they open,
show their menus and can be dragged as anywhere else. If nothing matches,
a message says so.
.*
:h1 res=0x4b0 id=createanother.Create another
:i1.Create another
:p.:hp2.Create another...:ehp2. makes a new object like the one whose menu
you used. Choose the folder for the new object and type its name, then
click Create.
:p.For a :link reftype=hd res=0xfb6.template:elink. this is the same as
dragging it. Drives and minimized windows cannot be copied this way.
.*
:h1 res=0x4b4 id=delete.Delete
:i1.Delete
:i1.deleting objects
:p.:hp2.Delete...:ehp2. on an object's menu, or the Delete key, deletes
the selected objects. A window lists what will be deleted; you can also
say there whether to confirm each object and each folder.
:p.For each object you are then asked&colon. Yes deletes it, Yes to all
deletes the rest without asking, No keeps it, and Cancel stops. Deleting a
folder deletes everything in it.
:p.Deleting is final&colon. there is no wastebasket. Deleting a shadow
deletes only the shadow. Some objects, such as the System object, cannot
be deleted. Whether you are asked is set on the Confirmations page of
the :link reftype=hd res=0xfbe.System:elink. object.
:p.See also :link reftype=hd res=0x4a6.Shredder:elink..
.*
:h1 res=0xbec id=copy.Copy
:i1.Copy
:i1.copying objects
:p.:hp2.Copy...:ehp2. copies the selected objects to a folder you choose.
A single object can get a new name on the way. You can also copy by
holding Ctrl and dragging.
:p.If the folder already has an object with the same name, rs2 asks you
whether to rename the copy, replace the other object, skip it or cancel,
or does what the Title page of the :link reftype=hd res=0xfbe.System:elink.
object says. Long copies show a progress window.
.*
:h1 res=0x4b2 id=move.Move
:i1.Move
:i1.moving objects
:p.:hp2.Move...:ehp2. moves the selected objects to a folder you choose.
You can also move by dragging&colon. within a drive objects move; hold
Shift to move to another drive (without it, files dropped on another drive
are copied).
:p.Shadows of the objects keep working after they move.
.*
:h1 res=0x84fc id=rename.Rename
:i1.Rename
:i1.titles, changing
:p.To change an object's title, choose :hp2.Rename:ehp2. from its menu,
press F2, or hold Alt and click its title. Type the new title and press
Enter (Esc keeps the old one). The title can also be changed on the
General page of its settings.
:p.For files and folders the title is the name on disk. On a drive that
only takes short names (FAT), the file gets a short name and the full title
is kept with it.
.*
:h1 res=0x4bf id=arrange.Arrange
:i1.Arrange
:i1.icons, arranging
:p.:hp2.Arrange:ehp2., on the menu of an open folder window or the
desktop, lines the icons up in rows from the top left, with no overlaps.
The new positions are kept.
:p.To keep icons in an order, use :link reftype=hd res=0x2422.Sort:elink.
with :hp2.Always maintain sort order:ehp2..
.*
:h1 res=0x2422 id=sort.Sort
:i1.Sort
:i1.sorting objects
:p.The :hp2.Sort:ehp2. menu of an open folder window puts its objects in
order by name, type, real name, size or last write date.
:p.The folder's Sort page sets the same thing, plus&colon.
:ul.
:li.:hp2.Where they were placed:ehp2.&colon. no sorting;
:li.:hp2.Always maintain sort order:ehp2.&colon. the folder keeps its
icons sorted whenever its content changes.
:eul.
.*
:h1 res=0x7d0c id=refresh.Refresh
:i1.Refresh
:p.:hp2.Refresh now:ehp2. (or F5 in a folder window) reads the folder's
content again, so that files made or deleted by programs, or from a
command prompt, are shown as they are.
.*
:h1 res=0x4ae id=open.Open
:i1.Open
:i1.views
:i1.opening objects
:p.Double-clicking an object opens it in its default view. The
:hp2.Open:ehp2. submenu of its menu lists all its views&colon.
:dl tsize=18.
:dt.Settings
:dd.The :link reftype=hd res=0x7d8a.settings notebook:elink..
:dt.Icon view
:dd.A folder's objects as icons. :link reftype=hd res=0x7df4.More:elink.
:dt.Tree view
:dd.A folder's folders as a tree. :link reftype=hd res=0x7df5.More:elink.
:dt.Details view
:dd.A folder's objects in a table. :link reftype=hd res=0x7df6.More:elink.
:dt.Program
:dd.Starts a program.
:edl.
:p.Some objects have a view of their own&colon. Shut down, Window list,
Restore (a minimized window), Index (Help Index and Glossary), Browser (a
URL object) and LaunchPad.
:p.When an object is already open, opening it again shows the open
window; its Window page can say to open a new one instead.
.*
:h1 res=0x4bb id=print.Print
:i1.Print
:i1.printing files
:p.:hp2.Print:ehp2. on a data file's menu prints the file on the default
printer. To use another printer, drag the file to that printer's object.
:p.PostScript files go to the printer as they are; other files print as
text. Printing a document with its formatting is done from the program
that made it.
:p.See :link reftype=hd res=0x3d6c.Printer object:elink..
.*
.* ==== Views ====
:h1 res=0x7df4 id=iconview.Icon view
:i1.icon view
:p.The icon view shows a folder's objects as icons with their titles.
Drag icons with mouse button 2 to place them; the folder remembers where.
:p.The folder's View page sets how icons are laid out (where they were
placed, flowed into columns, or in one column), their size (normal, small,
or none, titles only) and the font.
.*
:h1 res=0x7df5 id=treeview.Tree view
:i1.tree view
:p.The tree view shows a folder's folders as a tree. Click the small
button beside a folder, or press + and -, to show or hide the folders in
it; folders are read when you open them in the tree. A folder set to
show all objects in the tree view shows its other objects too.
:p.The folder's View page sets whether lines join the tree and the icon
size.
.*
:h1 res=0x7df6 id=detailsview.Details view
:i1.details view
:p.The details view shows a folder's objects in a table&colon. icon,
title, object class, real name, size and the date and time of the last
change. The folder's View page chooses which columns are shown.
.*
:h1 res=0x7d8a id=settings.Settings notebook
:i1.settings notebook
:i1.properties
:p.Each object has a settings notebook. Open it from the object's menu
(Open, then Settings). The tabs on the right are the pages; some tabs
have several pages, turned with the arrows at the bottom right.
:p.The pages depend on the kind of object. Every object has a
:hp2.General:ehp2. page with its title and the Template check box, and
most have a :hp2.Window:ehp2. page. See the topic for each object, for
example :link reftype=hd res=0x3d45.Folder:elink.,
:link reftype=hd res=0x3d46.Data file:elink. or
:link reftype=hd res=0x3d44.Program:elink..
:p.Use Alt+PgUp and Alt+PgDn to turn pages from the keyboard. Changes are
kept with the object.
.*
.* ==== LaunchPad ====
:h1 res=0x7e90 id=launchpad.LaunchPad
:i1.LaunchPad
:i1.drawers
:p.The LaunchPad is a bar of buttons for the things you use most. It
opens each time the desktop starts.
:ul.
:li.The action buttons show the Window list, start Find and Shut down.
:li.Click an object's button to open the object.
:li.Click the arrow above a button to open its drawer, a list of more
objects.
:li.Drop objects on the LaunchPad, on an arrow or into an open drawer to
add them.
:li.Drag the LaunchPad's empty space to move it.
:eul.
:p.Mouse button 2 on a button gives Open, Remove from LaunchPad,
LaunchPad settings and Close LaunchPad.
:p.Its settings have two pages&colon. :hp2.Options:ehp2. (vertical, small
icons, titles on the bar and in drawers, closing a drawer after opening an
object, floating on top, the title bar) and :hp2.Buttons:ehp2. (action
buttons as text, small icons, normal icons or none).
.*
.* ==== Keys ====
:h1 res=0x9003 id=keys.Keys
:i1.keys
:i1.keyboard shortcuts
:p.Most things can be done from the keyboard as well as with the
mouse&colon.
:ul.
:li.:link reftype=hd res=0x1375.System keys:elink.
:li.:link reftype=hd res=0x1376.Window keys:elink.
:li.:link reftype=hd res=0x1377.Object keys:elink.
:li.:link reftype=hd res=0x1014.Selection keys:elink.
:li.:link reftype=hd res=0x1374.Help window keys:elink.
:eul.
.*
:h2 res=0x1375 id=syskeys.System keys
:i1.system keys
:p.These keys work wherever you are&colon.
:dl tsize=18.
:dt.Ctrl+Esc
:dd.Show the Window List, to go to another program. In a full-screen
session it brings you back to the desktop.
:dt.Ctrl+Alt+W
:dd.Show rs2's rescue screen, which lists the processes and can end one
that hangs, when even the Window List cannot help.
:dt.F1
:dd.Help for where you are, if it is offered.
:edl.
.*
:h2 res=0x1376 id=winkeys.Window keys
:i1.window keys
:p.These keys work on the active window&colon.
:dl tsize=18.
:dt.Alt+Space
:dd.Show the window's system menu (also Shift+Esc). From it, Move and
Size move and size the window with the arrow keys.
:dt.F10
:dd.Go to the menu bar, and back.
:dt.Alt+F4
:dd.Close the window.
:dt.Alt+F5
:dd.Restore the window to its size before.
:dt.Alt+F9
:dd.Minimize the window.
:dt.Alt+F10
:dd.Maximize the window.
:dt.Alt+F11
:dd.Hide the window.
:dt.Tab, Shift+Tab
:dd.Go to the next or previous field of a window or dialog.
:dt.Alt+letter
:dd.Go to the item whose underlined letter that is.
:edl.
.*
:h2 res=0x1377 id=objkeys.Object keys
:i1.object keys
:p.These keys work on the object with the cursor in a folder window or
on the desktop&colon.
:dl tsize=18.
:dt.Enter
:dd.Open the object.
:dt.Shift+F10
:dd.Show the object's pop-up menu (with no object, the folder's).
:dt.F2
:dd.Rename the object.
:dt.Delete
:dd.Delete the selected objects.
:dt.F5
:dd.Refresh the folder.
:dt.F1
:dd.Help.
:edl.
.*
:h2 res=0x1014 id=selkeys.Selection keys
:i1.selection keys
:p.In a folder window&colon.
:dl tsize=18.
:dt.Arrow keys
:dd.Move to the next object and select it.
:dt.Home, End
:dd.Move to the first or last object.
:dt.PgUp, PgDn
:dd.Move a window's height up or down.
:dt.Space
:dd.Select the object with the cursor, or deselect it.
:dt.Ctrl+/
:dd.Select all objects.
:dt.Ctrl+\
:dd.Deselect all objects.
:dt.+ and -
:dd.In a tree view, show or hide the folders inside a folder; the left
and right arrows do the same.
:edl.
:p.With the mouse, Ctrl+click adds to the selection and Shift+click
selects a range.
.*
:h2 res=0x1374 id=helpkeys.Help window keys
:i1.help window keys
:p.In a help window&colon.
:dl tsize=18.
:dt.Tab, Shift+Tab
:dd.Move to the next or previous link.
:dt.Enter
:dd.Show the topic of the selected link.
:dt.Esc, Backspace
:dd.Go back to the previous topic.
:dt.Arrow keys, PgUp, PgDn, Home, End
:dd.Scroll the topic.
:dt.Ctrl+C
:dd.Show the contents.
:dt.Ctrl+S
:dd.Search the help.
:dt.Ctrl+H
:dd.Show the topics you have viewed.
:dt.Ctrl+N
:dd.Show the topic in a new window.
:dt.Ctrl+Insert
:dd.Copy the topic's text to the clipboard.
:dt.Ctrl+F, Ctrl+A
:dd.Copy the topic to a file, or add it to the end of one.
:dt.Ctrl+L
:dd.Show the libraries.
:dt.F11
:dd.Help index.
:dt.F2
:dd.General help.
:dt.F9
:dd.Keys help.
:dt.Shift+F10
:dd.Using help.
:edl.
:p.In a book, Ctrl+I shows the index, Ctrl+B sets a bookmark and F3
closes the book.
.*
.* ==== Installing applications (rs2's own panel) ====
:h1 res=0x9002 id=appinst.Install OS/2 Applications
:i1.installing applications
:i1.APPINST
:p.:hp2.Install OS/2 Applications:ehp2., in the Programs folder (or
APPINST at a command prompt), installs the applications of OS/2 Warp 4.52
that rs2 runs, from the OS/2 CD.
:p.Check the applications you want, or a whole category, and choose
Install. The files are unpacked to where OS/2 keeps them, and program
objects are made in folders in the Programs folder. Books go into the
Information folder; help files join the
:link reftype=hd res=0x37df.Help Index:elink..
:euserdoc.
