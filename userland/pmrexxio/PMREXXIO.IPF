.* PMREXXIO.HLP: the help of the REXX input and output window (PMREXX),
.* compiled by crates/ipf when the image is built. The panel numbers are
.* those of the window's help table (userland/pmrexxio/src/tpl.rs).
:userdoc.
:title.PMREXX Help
:h1 res=1.PMREXX
:i1.PMREXX
:p.PMREXX runs a REXX procedure in a window of its own. What the
procedure writes with :hp2.SAY:ehp2., its trace and the output of the
commands it runs appear in the large area of the window; when the
procedure reads (:hp2.PULL:ehp2., :hp2.PARSE PULL:ehp2.) or waits at
an interactive trace pause, type the answer after :hp2.Input&colon.:ehp2.
and press Enter.
:p.Start it with :hp2.PMREXX [/T] [/Q] procedure [arguments]:ehp2..
:p.:hp2./T:ehp2. starts the procedure in interactive trace;
:hp2./Q:ehp2. closes the window when the procedure ends. The
procedure must begin with a comment (:hp2./*:ehp2.); without an
extension :hp2..CMD:ehp2. is tried too.
:p.The window stays when the procedure has ended: its output can be
saved, searched and copied, and the procedure run again
(:hp2.Restart:ehp2. in the :hp2.Trace:ehp2. menu).
:p.:link reftype=hd res=9001.Keys:elink.

:h1 res=2.File
:p.Saving the output and setting up the window.

:h1 res=3.Save
:p.Writes the output to the file it was last saved in; the first time,
:hp2.Save as:ehp2. asks for a file. Ctrl+S does the same.

:h1 res=4.Save as
:p.Writes the output to a file you choose.

:h1 res=9.Exit
:p.Closes the window and ends PMREXX. F3 does the same.

:h1 res=35.Properties
:p.The output's font, and whether a new run of the procedure keeps the
output of the last one.

:h1 res=18.Font
:p.Chooses the font of the output.

:h1 res=36.Append results
:p.When checked, the output of the procedure run again with
:hp2.Restart:ehp2. follows that of the last run; when not, the window
is cleared first.

:h1 res=10.Edit
:p.Copying and selecting text. Cut, copy, paste and delete act on the
input line when it has the focus, otherwise on the output (which can
only be copied from).

:h1 res=32.Cut
:p.Moves the selected text of the input line to the clipboard.

:h1 res=11.Copy
:p.Copies the selected text to the clipboard. Ctrl+Insert does the same.

:h1 res=12.Paste
:p.Inserts the clipboard's text into the input line. Shift+Insert does
the same.

:h1 res=13.Delete
:p.Removes the selected text of the input line.

:h1 res=14.Select all
:p.Selects all of the output.

:h1 res=34.Deselect all
:p.Removes the selection from the output.

:h1 res=37.Find
:p.Finds text in the output and selects it.

:h1 res=601.Find
:p.Type the text to find and select :hp2.OK:ehp2..
:hp2.Match case:ehp2. finds only text with the same capitals.

:h1 res=24.Trace
:p.Tracing and stopping the procedure.

:h1 res=17.Interactive trace
:p.Turns interactive trace on or off while the procedure runs. At each
pause, press Enter for the next clause, type REXX instructions to run
them, or use the items below.

:h1 res=28.Trace off
:p.Ends interactive trace and lets the procedure run on.

:h1 res=26.Next clause
:p.Runs the next clause (as Enter on an empty input line).

:h1 res=27.Repeat last clause
:p.Runs the last clause again (as typing an equal sign).

:h1 res=16.Restart
:p.Runs the procedure again once it has ended.

:h1 res=25.Halt procedure
:p.Stops the procedure (it gets the HALT condition). Ctrl+Break does
the same.

:h1 res=45.Help
:p.The help of PMREXX.

:h1 res=30.Help index
:p.The index of the help.

:h1 res=29.Using help
:p.How to use the help windows.

:h1 res=31.Keys help
:p.The keys of the window.

:h1 res=42.Product information
:p.Shows the version of PMREXX.

:h1 res=9001.Keys
:i1.keys
:dl compact tsize=18.
:dt.Enter
:dd.Sends the input line to the procedure.
:dt.Up arrow
:dd.Brings back the last line sent.
:dt.Tab
:dd.Moves between the input line and the output.
:dt.Ctrl+Break
:dd.Halts the procedure.
:dt.Ctrl+S
:dd.Saves the output.
:dt.F3
:dd.Ends PMREXX.
:dt.F1
:dd.Help.
:edl.

:h1 res=400.Font
:p.Choose the font, size and style of the output and select
:hp2.OK:ehp2..

:h1 res=600.Save as
:p.Choose the drive and directory, type the file name and select
:hp2.OK:ehp2..
:euserdoc.
