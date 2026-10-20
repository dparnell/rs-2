rs2 - an OS/2 Warp 4.52 compatible operating system written in Rust
====================================================================

This disc boots on UEFI computers (and virtual machines). It starts a
live system with the rs2 desktop: Presentation Manager with rs2's own
desktop shell.

The desktop
-----------
Double click an icon (or select it and press Enter) to open it:

  Command Prompts   OS/2 Window (a command prompt in a window) and
                    OS/2 Full Screen
  Programs          installed programs (and "Install rs2")
  Drives            one icon per drive; a drive opens its root folder,
                    folders open in new windows, programs run, text
                    files open in the System Editor when it is installed
  Minimized Window Viewer   windows you minimized
  Window List       the running programs (also Ctrl+Esc)
  Shutdown          closes all programs and turns the computer off

Mouse button 2 opens a pop-up menu: Open, Settings, Rename, Delete on
objects; Arrange, Refresh, Create program object, System setup and
Shut down on the desktop. Drag icons with mouse button 2; their places
are kept. Drag a file onto a folder to move it there. F2 renames, Del
deletes (after asking), F5 refreshes.

The text-mode command prompt
----------------------------
Press T while the boot loader says "Press T for a text-mode boot" to get
the command prompt instead of the desktop. On an installed system,
CONFIG.SYS chooses: PROTSHELL=C:\OS2\RS2SHELL.EXE is the desktop,
PROTSHELL=C:\OS2\CMD.EXE the command prompt. From the desktop,
Ctrl+Esc offers "Command prompt (full screen)", and Alt+Esc switches
between full-screen sessions and the desktop.

Installing
----------
To install rs2 on a hard disk, open Programs > Install rs2, or type
INSTALL at a command prompt. The installer erases the chosen disk,
creates an EFI system partition and copies the system to it. Remove the
disc and restart afterwards.

Programs for OS/2 (16-bit NE and 32-bit LX executables) can be run from
the desktop or the command line. Many system services are still being
written; see the project's ROADMAP for the current state.
