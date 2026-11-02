.* ICONEDIT.HLP: help for rs2's Icon Editor (userland/iconedit,
.* OS2\ICONEDIT.EXE).
.*
.* Written for rs2. The res= numbers are the panels the editor's help
.* table asks for (the numbers OS/2's Icon Editor uses). xtask compiles
.* this file with crates/ipf into OS2\HELP\ICONEDIT.HLP.
:userdoc.
:title.Icon Editor Help
.*
.* ---- Overview ----
:h1 res=20086 id=general.Icon Editor
:i1.Icon Editor
:i1.icons
:i1.pointers
:i1.bitmaps
:p.The Icon Editor draws the pictures of the system&colon. :hp2.icons:ehp2.
(the pictures of objects and programs), :hp2.pointers:ehp2. (the shapes of
the mouse pointer) and :hp2.bitmaps:ehp2. (pictures of any size, such as
desktop backgrounds). Start it from the Utilities folder, open an icon,
pointer or bitmap file from a folder, or type :hp2.ICONEDIT:ehp2. at a
command prompt followed by the name of a file.
:p.The window has three parts&colon.
:dl tsize=18.
:dt.Status area
:dd.Along the top. The mouse shows the colors its two buttons paint with;
next to it is the figure at its real size. The text tells the size of the
form you are editing, where the pen is, the pen's size, the hot spot, the
kind of figure, which form of how many you are editing and the device the
form is for. It also tells you when a mode such as selecting or color
filling is on.
:dt.Edit area
:dd.The figure enlarged, one square for each pel. Press mouse button 1 to
paint with button 1's color, button 2 to paint with button 2's color, and
move the mouse with the button held down to draw.
:dt.Palette
:dd.Down the right side. Click a color with mouse button 1 to give it to
button 1 (it is framed in red), with button 2 to give it to button 2 (framed
in green). Double-click a color to change it. Icons and pointers also have
:hp2.Screen:ehp2. and :hp2.Inverse:ehp2.&colon. where the figure is painted
with Screen, whatever is behind it shows through; where it is painted with
Inverse, the colors behind it are inverted.
:edl.
:p.A figure can have several :link reftype=hd refid=forms.forms:elink.,
versions of the same picture for different screens and sizes.
:p.See also :link reftype=hd res=64025.the keys of the Icon Editor:elink. and
:link reftype=hd refid=formats.file formats:elink..
.*
:h2 id=forms.Forms and devices
:i1.forms
:i1.devices
:p.OS/2 shows icons and pointers at different sizes depending on the
screen&colon. 32 by 32 pels on most screens, 40 by 40 on 1024 by 768 screens
with large fonts, and half those sizes in small icon views. A file can hold
a :hp2.form:ehp2. for each, in two colors (black and white) or in color.
The system picks the form that suits the screen best.
:p.A new icon gets the recommended forms of the list of predefined devices;
you edit one form at a time. Use :link reftype=hd res=20041.Device List:elink.
to see the forms of the figure, to edit another one, to add forms and to
delete them. A form you add is drawn from the form you are editing, made
larger or smaller as needed, so you only have to touch it up.
.*
:h2 id=formats.File formats
:i1.file formats
:p.The Icon Editor reads and writes OS/2's files&colon.
:ul.
:li.icons (.ICO), pointers (.PTR) and bitmaps (.BMP);
:li.files with one picture and files with several forms (bitmap arrays);
:li.the formats of OS/2 1.1, 1.2 and 2.0, and 2.0 files compressed with
run-length encoding;
:li.2, 16, 256 colors and true color (24 bits per pel).
:eul.
:p.When you save a file with Save as, the Save Options window lets you
choose the format and which forms to write.
.*
.* ---- File menu ----
:h2 res=20001 id=file.File menu
:i1.File menu
:p.The File menu starts new figures, opens and saves files.
.*
:h3 res=20007.New
:p.Starts a new figure. Choose :hp2.Icon:ehp2., :hp2.Pointer:ehp2. or
:hp2.Bitmap:ehp2.; for a bitmap also type its width and height in pels (up to
1024). Icons and pointers get the recommended forms of the predefined
devices; a bitmap gets one 16 color form. If the current figure has changes
you are asked whether to save them first.
.*
:h3 res=20008.Open
:p.Opens an icon, pointer or bitmap file. If the current figure has changes
you are asked whether to save them first.
.*
:h3 res=20009.Next
:p.Opens the next file of the same kind (by its extension) in the directory
of the current file, in alphabetic order. Use it to go through a directory of
icons one after the other.
.*
:h3 res=20010.Save
:p.Saves the figure in its file, in the format it was read in. A new figure is
saved as with Save as.
.*
:h3 res=20011.Save as
:p.Saves the figure under another name. After you choose the name, the
:link reftype=hd res=20122.Save Options:elink. window asks for the format
and the forms to save.
.*
.* ---- Edit menu ----
:h2 res=20002 id=edit.Edit menu
:i1.Edit menu
:p.The Edit menu works on the :hp2.selection:ehp2., a rectangle of the form
you mark with Select, or on the whole form when nothing is selected.
.*
:h3 res=20013.Undo
:p.Takes back the last change. Choose Undo again to put the change back.
.*
:h3 res=20014.Select
:p.Lets you mark a rectangle&colon. after choosing Select, press a mouse
button on one corner and move to the opposite corner. The selection is
framed with a dotted line and its size shows in the status area. Press Esc
to remove it.
.*
:h3 res=20015.Select all
:p.Selects the whole form.
.*
:h3 res=20016.Cut
:p.Copies the selection to the clipboard and clears it.
.*
:h3 res=20017.Copy
:p.Copies the selection to the clipboard, both as a bitmap other programs
can paste and in the Icon Editor's own form, which keeps the Screen and
Inverse areas.
.*
:h3 res=20018.Paste
:p.Puts the clipboard's picture at the top left corner of the selection (or
of the form) at its own size. Colors that the form does not have become the
nearest color of its palette.
.*
:h3 res=20019.Clear
:p.Clears the selection&colon. to Screen in icons and pointers, to white in
bitmaps.
.*
:h3 res=20020.Stretch paste
:p.Puts the clipboard's picture into the selection, made larger or smaller
to fill it.
.*
:h3 res=20021.Fill
:p.Fills the selection with mouse button 1's color.
.*
:h3 res=20022.Flip horizontal
:p.Turns the selection over from left to right.
.*
:h3 res=20023.Flip vertical
:p.Turns the selection upside down.
.*
:h3 res=20024.Circle
:p.Draws a circle, or an ellipse, inside the selection with mouse button 1's
color and the current pen.
.*
.* ---- Palette menu ----
:h2 res=20003 id=palette.Palette menu
:i1.Palette menu
:i1.colors
:p.The palette is the list of colors a form uses&colon. two for a black and
white form, 16 or 256 for color forms. True color forms can use any color;
for them the palette holds 16 colors to paint with.
.*
:h3 res=20027.Load default palette
:p.Gives the form the default palette for its number of colors again (the one
set with Set default palette, or the system's).
.*
:h3 res=20028.Open
:p.Loads a palette saved with Save from a palette file (.PAL).
.*
:h3 res=20029.Save
:p.Saves the palette in its palette file (or asks for a name the first time).
.*
:h3 res=20078.Save as
:p.Saves the palette in a palette file under a new name.
.*
:h3 res=20031.Edit color
:p.Changes mouse button 1's palette color. You can also double-click a color
in the palette. See :link reftype=hd res=20084.Edit color:elink..
.*
:h3 res=20033.Swap colors
:p.Exchanges the palette entries of the colors of the two mouse buttons.
.*
:h3 res=20032.Preserve figure
:p.Exchanges the two palette entries but keeps the picture as it is.
.*
:h3 res=20042.Don't preserve figure
:p.Exchanges the two colors in the picture too&colon. what was painted with
one now has the other.
.*
:h3 res=20034.Set default palette
:p.Makes the form's palette the default for new forms with that number of
colors.
.*
.* ---- Options menu ----
:h2 res=20004 id=options.Options menu
:i1.Options menu
:p.The Options menu changes how the editor works. The choices are kept from
one session to the next when Save state on exit is on.
.*
:h3 res=20035.Test
:p.Turns test mode on and off. In test mode the mouse pointer takes the
shape of the figure while it is over the edit area, so you can see how an
icon or pointer really looks, its hot spot included.
.*
:h3 res=20036.Grid
:p.Shows or hides the lines between the pels of the edit area.
.*
:h3 res=20037.X background
:p.Marks the pels painted with Screen with an X, so they stand out from a
color that looks the same.
.*
:h3 res=20038.Draw straight
:p.Keeps lines drawn with the mouse straight&colon. the first move decides
whether the line runs across or down.
.*
:h3 res=20039.Pen size
:p.Sets the size of the pen, from one pel to 9 by 9 pels.
.*
:h3 res=20085.Pen sizes
:p.Choose the size of the square pen. Use Set pen shape for pens of other
shapes.
.*
:h3 res=20040.Preferences
:p.Settings of the editor itself&colon. Safe prompting, Suppress warnings,
Save state on exit, Display status area, Reset options and modes, and Save
profile, which keeps the options now.
.*
:h3 res=20047.Safe prompting
:p.When on, the editor asks before it throws away changes and before it
replaces an existing file.
.*
:h3 res=20048.Suppress warnings
:p.When on, the editor does not warn about large files, unusual extensions
and palettes without black.
.*
:h3 res=20049.Save state on exit
:p.When on, the options are kept in the user profile when the editor ends.
.*
:h3 res=20090.Display status area
:p.Shows or hides the status area at the top of the window.
.*
:h3 res=20050.Reset options and modes
:p.Sets every option back to its default and leaves any mode.
.*
:h3 res=20025.Set pen shape
:p.Lets you shape the pen with the cursor keys&colon. Left and Right make it
narrower and wider, Down and Up lower and higher. Press Enter or Esc when
done.
.*
:h3 res=20026.Hotspot
:p.The hot spot is the pel of a pointer that points&colon. where the mouse
clicks. Choose Hotspot, then click the pel. The hot spot is marked with a
red cross; its position shows in the status area.
.*
.* ---- Device menu ----
:h2 res=20005 id=device.Device menu
:i1.Device menu
:p.The Device menu works with the forms of the figure and the list of
predefined devices.
.*
:h3 res=20041.List
:p.Shows the forms of the figure. See :link reftype=hd res=20098.Device
List:elink..
.*
:h3 res=20044.Edit predefined
:p.Changes the list of predefined devices. See
:link reftype=hd res=20119.Edit Predefined Device Forms:elink..
.*
.* ---- Tools menu ----
:h2 res=20006 id=tools.Tools menu
:i1.Tools menu
.*
:h3 res=20045.Color fill
:p.Fills an area of one color&colon. choose Color fill, then click a pel with
mouse button 1 or 2. The pel and all the pels of its color that touch it,
across or down, get that button's color.
.*
:h3 res=20046.Find color
:p.Finds a color in the palette&colon. choose Find color, then click a pel
with mouse button 1 or 2. That button gets the pel's color.
.*
.* ---- Dialogs ----
:h2 res=20058 id=newdlg.New Figure
:p.Choose the kind of figure. For a bitmap type its width and height in pels.
Choose :hp2.OK:ehp2. to start it or :hp2.Cancel:ehp2. to keep the current
figure.
.*
:h2 res=20122 id=saveopts.Save Options
:p.Choose the format of the file&colon.
:dl tsize=12.
:dt.1.1
:dd.OS/2 1.1&colon. one form only (the first one selected).
:dt.1.2
:dd.OS/2 1.2&colon. all the forms selected, with the 1.x headers every OS/2
version reads.
:dt.2.0
:dd.OS/2 2.0&colon. all the forms selected, with the 2.0 headers.
:dt.packed 2.0
:dd.Like 2.0 with 16 and 256 color pels compressed.
:edl.
:p.The format the file was read in is marked with < and >. Then select the
forms to save in the list (all are selected at first) and choose
:hp2.Save:ehp2..
.*
:h2 res=20084 id=colordlg.Edit color
:i1.Edit color
:p.Set the color with the scroll bars or type the values&colon. red, green
and blue from 0 to 255, or with :hp2.HSV:ehp2. hue (0 to 359), saturation
and value (0 to 100). The box shows the color. Every pel of the old color
takes the new one.
:p.:hp2.Important:ehp2. and :hp2.Dynamic:ehp2. are kept for palette managers;
the Icon Editor does not use them.
:p.Icons and pointers need a black entry in their palette&colon. the Screen
and Inverse areas are written with it.
.*
:h2 res=20098 id=devlist.Device List
:p.The list shows the forms of the figure, with their size, colors and
device.
:dl tsize=10.
:dt.Add
:dd.Adds a form for another device (see :link reftype=hd res=20101.Add
Device:elink.).
:dt.Delete
:dd.Deletes the selected form (a figure keeps at least one).
:dt.View
:dd.Shows the selected form in the edit area.
:dt.Up, Down
:dd.Move the selected form in the list; the order is the order in the file.
:dt.OK
:dd.Edits the selected form.
:edl.
.*
:h2 res=20101 id=adddev.Add Device
:p.Select a predefined device and choose :hp2.OK:ehp2. to add a form for it.
The new form is drawn from the form you are editing. :hp2.Customize:ehp2.
lets you describe a device of your own (see
:link reftype=hd res=20108.Customize Device:elink.).
.*
:h2 res=20108 id=custom.Customize Device
:p.Describe the form to add&colon. the size of the screen it is for (0 by 0
for any screen), its resolution, the size of the picture in pels and its
colors.
.*
:h2 res=20119 id=predef.Edit Predefined Device Forms
:p.The predefined devices are the forms new figures get and the choices of
Add Device. Choose a device in the list, then change&colon.
:ul.
:li.its sizes&colon. of icons, of pointers, of bitmaps (as a percentage of
the first form's size), of the screen and its resolution;
:li.its colors;
:li.whether a form added for it keeps the colors of the form it is drawn
from (Inherited) or gets the default palette (Predefined);
:li.whether new icons and pointers get it (Recommended).
:eul.
:p.:hp2.New:ehp2. adds a device, a copy of the one shown; :hp2.Delete:ehp2.
deletes the device shown. The list is kept in the user profile.
.*
.* ---- Help ----
:h2 res=64022 id=helpmenu.Help menu
:i1.Help menu
:dl tsize=22.
:dt.Help index
:dd.The help topics in alphabetic order.
:dt.General help
:dd.What the Icon Editor is and how to use it.
:dt.Using help
:dd.How to use the help windows.
:dt.Keys help
:dd.The keys of the Icon Editor.
:dt.Product information
:dd.The program's name and version.
:edl.
:h3 res=64026.Help index
:p.Shows the help topics in alphabetic order.
:h3 res=64024.General help
:p.Explains what the Icon Editor is for and how its window works.
:h3 res=64023.Using help
:p.Explains how to use the help windows.
.*
:h2 res=64025 id=keys.Keys
:i1.keys
:p.Besides the shortcut keys shown in the menus&colon.
:dl tsize=22.
:dt.Arrow keys
:dd.Move the pen location (a red frame in the edit area).
:dt.Space bar
:dd.Paints at the pen location with mouse button 1's color.
:dt.Enter
:dd.Paints at the pen location with mouse button 2's color.
:dt.Esc
:dd.Leaves the current mode (select, hot spot, color fill, find color, pen
shape) and removes the selection.
:dt.Ctrl+1 ... Ctrl+9
:dd.Pen sizes.
:dt.Alt+Backspace
:dd.Undo.
:edl.
.*
:h2 res=64096 id=product.Product information
:p.Shows the name and version of the Icon Editor.
:euserdoc.
