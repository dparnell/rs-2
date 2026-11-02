.* START.INF: "Start Here", a short book for new users of rs2.
.*
.* Written for rs2, in rs2's own words. xtask compiles it with
.* crates/ipf into a book that VIEW shows.
:userdoc.
:title.Start Here
.*
:h1 res=1 id=welcome.Welcome to rs2
:i1.rs2
:i1.welcome
:p.Welcome! rs2 is a new operating system that works like OS/2 Warp 4.52.
It is written from scratch in Rust, and it runs OS/2 programs&colon. text
mode programs at the command prompt, Presentation Manager programs on the
desktop, and many DOS and Windows 3.1 programs too.
:p.This short book shows you around&colon.
:ul.
:li.:link reftype=hd res=2.The desktop:elink.
:li.:link reftype=hd res=3.Starting programs:elink.
:li.:link reftype=hd res=4.Command prompts:elink.
:li.:link reftype=hd res=5.Finding help:elink.
:li.:link reftype=hd res=6.Installing applications:elink.
:li.:link reftype=hd res=7.Shutting down:elink.
:eul.
:p.Use the arrows of the book window, or Ctrl+C for the contents, to move
around. Esc takes you back.
.*
:h1 res=2 id=desktop.The desktop
:i1.desktop
:i1.objects
:i1.mouse
:p.Everything on the desktop is an :hp2.object:ehp2.&colon. folders,
programs, files, drives, printers and tools. You do the same few things
with all of them&colon.
:dl tsize=24.
:dt.Double-click (button 1)
:dd.Open the object.
:dt.Click button 2
:dd.Show its pop-up menu&colon. open, rename, copy, move, delete and more.
:dt.Drag with button 2
:dd.Move it to a folder. Hold Ctrl to copy, or Ctrl+Shift to make a
shadow (a link).
:dt.Open, then Settings
:dd.Change how the object looks and behaves.
:edl.
:p.Some places to visit&colon.
:ul.
:li.:hp2.Drives:ehp2.&colon. your disks and their files.
:li.:hp2.Programs:ehp2.&colon. the System Editor and the programs you
install.
:li.:hp2.System Setup:ehp2.&colon. the mouse, keyboard, colours, fonts,
display and clock.
:li.:hp2.Templates:ehp2.&colon. drag one out to make a new folder, file or
program object.
:li.The :hp2.LaunchPad:ehp2.&colon. the bar of buttons at the bottom.
Drop your favourite objects on it.
:eul.
:p.Click button 2 on an empty part of the desktop for the Window list,
Lockup now and Shut down.
.*
:h1 res=3 id=programs.Starting programs
:i1.programs, starting
:i1.Window List
:p.To start a program, double-click its object in the Programs folder, on
the desktop or on the LaunchPad. You can also double-click a program file
(an .EXE) in a drive's folders, or a data file to open it with its
program.
:p.Each program gets a window. To switch between programs, press
:hp2.Ctrl+Esc:ehp2. for the Window List and double-click the one you
want. Minimized windows go to the :hp2.Minimized Window Viewer:ehp2. on the
desktop.
:p.To have a program start every time, put a shadow of it in the
:hp2.Startup:ehp2. folder.
:note.If a program stops responding, use End task in the Window List. If
even that does not help, Ctrl+Alt+W shows rs2's rescue screen.
.*
:h1 res=4 id=prompts.Command prompts
:i1.command prompt
:i1.CMD
:p.The :hp2.Command Prompts:ehp2. folder on the desktop starts command
lines&colon. an OS/2 window, OS/2 full screen, a DOS window or DOS full
screen.
:p.At the prompt you type commands and press Enter. A few to start with&colon.
:dl tsize=16.
:dt.DIR
:dd.List the files in the current directory.
:dt.CD name
:dd.Change to another directory.
:dt.COPY a b
:dd.Copy files.
:dt.E name
:dd.Edit a text file in the System Editor.
:dt.HELP
:dd.List the commands.
:dt.EXIT
:dd.Close the command prompt.
:edl.
:p.From a full-screen session, Ctrl+Esc brings you back to the desktop.
.*
:h1 res=5 id=help.Finding help
:i1.help
:i1.F1
:i1.Help Index
:i1.VIEW
:i1.HELP command
:p.Help is never far away&colon.
:ul.
:li.Press :hp2.F1:ehp2. in a program or dialog for help with what is in
front of you, or use the program's Help menu.
:li.Open the :hp2.Help Index:ehp2. (Assistance Center, Information) for
an alphabetical list of every help topic on the system. Type a word to
jump to it.
:li.Read the books in the :hp2.Information:ehp2. folder, or type
:hp2.VIEW:ehp2. and a book's name at a command prompt. For example,
VIEW CMDREF opens the Command Reference once it is installed.
:li.At a command prompt, :hp2.HELP:ehp2. lists the commands and
:hp2.HELP DIR:ehp2. tells you about one. When a command prints a message
such as SYS0002, type :hp2.HELP 2:ehp2. to find out what it means and
what to do.
:eul.
.*
:h1 res=6 id=install.Installing applications
:i1.APPINST
:i1.installing applications
:p.rs2 comes with a few programs of its own. Many more, from the OS/2
Warp 4.52 CD, can be added with :hp2.Install OS/2 Applications:ehp2. in
the Programs folder (or type APPINST at a command prompt).
:ol.
:li.Put the OS/2 CD in the drive.
:li.Start Install OS/2 Applications.
:li.Check the applications you want, or a whole category&colon. games,
productivity tools, books, REXX and more.
:li.Choose Install.
:eol.
:p.The programs appear in folders in the Programs folder. Books go to the
Information folder, and help files join the Help Index.
:p.Other OS/2 programs can be copied to the disk and started from a
command prompt or a program object; drag the Program template from the
Templates folder to make one.
.*
:h1 res=7 id=shutdown.Shutting down
:i1.shutting down
:i1.Lockup
:p.Always shut down rs2 before you turn off the computer, so that your
work and the desktop are saved and everything is written to disk.
:ol.
:li.Click button 2 on an empty part of the desktop and choose
:hp2.Shut down...:ehp2. (or open the Shut Down object, or click Shut down
on the LaunchPad).
:li.Choose Shut down the computer, or Restart the computer.
:li.Answer any program that asks about unsaved work.
:eol.
:p.rs2 turns the computer off, or tells you when it is safe to do so.
:p.Going away for a while? Choose :hp2.Lockup now:ehp2. from the desktop's
menu instead&colon. the screen locks until you type your password.
:p.Enjoy rs2!
:euserdoc.
