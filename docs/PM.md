# Presentation Manager in rs2

Most OS/2 applications are Presentation Manager (PM) programs. This
document describes how rs2 provides PM.

## Approach

As on OS/2, PM is a set of DLLs that run inside each application process:
PMWIN (windows, messages, input), PMGPI (graphics), PMSHAPI (profiles,
program list), PMCTLS, HELPMGR and friends. rs2 provides its own versions
of these DLLs, written in Rust and built with `elf2lx` into LX DLLs with the
original module names and export ordinals, so unmodified PM programs load
them through the normal loader.

PM state that all processes share (the window tree, window classes, message
queues, the input state) lives in shared memory of PMWIN.DLL, the same way
OS/2 keeps it in shared segments. A process draws straight into the
framebuffer, clipped to its windows' visible regions (there is no
compositor). Cross-process operations (WinSendMsg to another process's
window, input routing) go through the queues in shared memory and wake the
receiving thread with an event semaphore. A small PM server process owns
the desktop and the input; see [Processes and input](#processes-and-input).

The logic lives in host-testable crates; the DLLs are thin:

| Crate / DLL | Contents |
|-------------|----------|
| `crates/gpi` | Rendering engine (and `icon.rs`: the OS/2 pointer and icon formats, choosing an image for the display, AND/XOR masks, drawing): presentation-space attributes, lines, boxes, arcs, areas, text with bitmap fonts, bitmaps and raster operations, clipping to rectangle lists. Draws into a `Surface` (32-bit pixels). No OS dependencies; unit-tested on the host. |
| `crates/pmwin` | Window system core: window tree, z-order, visible and update regions, hit testing, the queues of threads to wake after invalidation; message queues with mouse-move coalescing and the not-responding bookkeeping; the MLE's text engine (`mle.rs`: layout with word wrap and tabs, editing, undo, import/export formats, search) and the arithmetic of spin buttons, sliders and value sets (`controls.rs`); the notebook's pages and geometry (`notebook.rs`); the container's layout for each view, hit testing, keyboard navigation, icon arrangement and selection rules (`container.rs`); where minimized frames go (`minimize.rs`); rs2's own system pointers and icons (`sysptr.rs`). Plain data structures, unit-tested on the host. |
| `userland/pmwin` → PMWIN.DLL | Exports (`Win*`, `Gpi*`), the shared state and its heap, locking, messaging, the built-in window classes, and the PM server's code (input routing, hung programs, window list). |
| `crates/viocell` | Text cells on pixels: renders VGA text cells (font, palette, blink, cursor, marked block) to 24-bit bitmaps; Copy/Copy All text and Paste keystrokes. Host-tested; for VIO windows now, AVIO later. |
| `userland/pmviowin` → `C:\OS2\PMVIOWIN.EXE` | The windows of windowed VIO sessions (see [Windowed VIO sessions](#windowed-vio-sessions-os2-window)). |
| `userland/pmsrv` → `C:\OS2\PMSRV.EXE` | The PM server process: only calls PMWIN's `Rs2PmServer` (private ordinal 9000; 9001, `Rs2ShellPosition`, gives PMSHAPI's `WinQueryTaskSizePos` the shell's next cascade position; 9002, `Rs2PmVioWindow`, registers a VIO window's client). |
| `userland/pmgpi` → PMGPI.DLL | Exports (`Gpi*`, `Dev*`) as forwarders to PMWIN, which implements them at ordinal 4000 + the PMGPI ordinal, since presentation spaces belong to windows. |
| `userland/pmctls` → PMCTLS.DLL | The standard file and font dialogs (`WinFileDlg`, `WinFontDlg`): ordinary PM dialogs, built from templates made in memory, run through PMWIN like an application's. |
| `userland/pmwp` → PMWP.DLL | The Workplace Shell calls programs use without it: `WinLoadFileIcon`, `WinSetFileIcon`, `WinFreeFileIcon`, forwarded to PMWIN (rs2-private ordinals 9004-9006). |
| `crates/inifile` | OS/2 binary INI files: parse (defensively) and write in IBM's layout, the program list records. Host-tested, also on IBM's INI files from the media. See [Profiles](#profiles-ini-files). |
| `userland/rs2shell` → `C:\OS2\RS2SHELL.EXE` | rs2's desktop shell (not the Workplace Shell): the desktop, folders, drives and files, program objects, shutdown. See [The desktop shell](#the-desktop-shell-rs2shell). Its icons are drawn in `crates/pmwin` (`shellicons.rs`). |
| `userland/pmshapi` etc. | Smaller DLLs: profile (OS2.INI) APIs, program list and task position (PMSHAPI), HELPMGR stubs, PMDRAG (no drag is ever in progress), PMMERGE (the entry points programs import from it directly), LIBUNI (locale objects). |

## Kernel services (module RS2API)

A kernel gate module, `RS2API`, holds rs2-private 32-bit services used by
rs2's own DLLs (applications never import it):

- `Rs2MapDisplay(PDISPLAYINFO)` (1): maps the framebuffer into the calling
  process (same linear address in every process, in the shared arena) and
  returns its address, width, height, stride and pixel format.
- `Rs2ReadInput(PINPUTEVENT, ulTimeout)` (2): raw input for the PM session:
  key events (scan code, character, shift state, make/break) and mouse
  events (absolute position in pixels, buttons, wheel).
  When a process that mapped the display ends, it reports an
  `EV_PROCEXIT` event (kind 4, `a` = pid) so that the PM server cleans
  up after it.
- `Rs2QueryPointer(PPOINTL)` (3): the pointer position.
- `Rs2DebugTrace(psz)` (4): a line on the serial port (debugging rs2's
  DLLs).
- `Rs2SetPointerShape(cx, cy, xHot, yHot, pulPixels)` (5): makes the mouse
  pointer a hardware cursor (0xAARRGGBB pixels, up to 64x64): a cursor
  resource on virtio-gpu's cursor queue, DEFINE_CURSOR on VMware SVGA II
  (whose driver takes the adapter over at the current mode when PM
  starts, for the FIFO). From then on the kernel's timer tick moves it to
  the pointer position itself and hides it while the PM session is not in
  the foreground. ERROR_NOT_SUPPORTED without such a display.
- `Rs2ShowPointer(fShow)` (6): shows or hides the hardware cursor.
- `Rs2PowerOff(ulAction)` (7): the end of a shutdown (after
  DosShutdown): commits the disks' write caches, then 0 turns the
  machine off (ACPI S5, then QEMU's and Bochs' ports) and returns
  ERROR_NOT_SUPPORTED if it is still running, 1 restarts it.

RS2API ordinals: 1-7 PM (above), 10-16 windowed VIO sessions (see
[Windowed VIO sessions](#windowed-vio-sessions-os2-window)), 100
performance counters, 200-221 sockets (docs/NETWORK.md).

Two kernel rules matter for PM: a thread inside a must-complete section
(DosEnterMustComplete) that is being killed finishes the section first
(it dies when it leaves it; a fatal exception still ends it at once), and
a mutex semaphore whose owner died is handed to the next requester with
ERROR_SEM_OWNER_DIED. DosKillProcess also lets a PM process end another PM
process (the window list).

The session manager gains a PM session type: the first PM program starts
(or joins) the PM session, which owns the framebuffer while it is in the
foreground and receives raw input instead of the text keyboard queue.
Switching to a text session and back makes the kernel queue a repaint
event so the PM server repaints everything. Switching away (Alt+Esc, the
text window list, Alt+Home on a VIO window, a pop-up) queues
`EV_BACKGROUND` (kind 5) first: the server points PMWIN's drawing
surface at a shadow buffer in the shared state until the repaint event
points it back at the framebuffer, so nothing PM draws in the meantime
lands on the text screen; for what PM drew in the moment before the
server saw the event, the kernel redraws the text screen a few times
during the first half second. In the PM session Ctrl+Esc goes to PM
(its window list); Alt+Esc still switches sessions.

## Processes and input

**Shared state.** PMWIN keeps everything in one named shared memory
object (`\SHAREMEM\PMWIN\STATE`, 64 MB reserved, committed as used)
mapped at the same address in every PM process; its heap is PMWIN's
global allocator, so the window tree, queues, classes, presentation
spaces and GPI objects are ordinary Rust collections that any process
can read and free. What belongs to one process carries its pid: private
window classes (and their procedure addresses), presentation spaces, GPI
objects (regions, bitmaps, DCs, palettes), accelerator tables. Window
procedures are only ever called on the thread that owns the window's
queue; a message for a window of another thread, in any process, goes
through that thread's queue.

**Locking.** One named mutex semaphore (`\SEM32\PMWIN\LOCK`) protects the
state; every export takes it (recursively) in its trampoline. It is never
held while application code runs: an application's window procedure is
called with the lock released (built-in procedures, which are PMWIN code,
keep it), and so are waits. The lock's sections are short and run inside
DosEnterMustComplete, so a process being ended (DosKillProcess, DosExit
from another thread) finishes its update of the shared state before its
thread dies; if a thread dies holding the lock anyway (an exception in
PMWIN), the next requester gets it with ERROR_SEM_OWNER_DIED and carries
on.

**Queues and waiting.** Every message queue has a shared event semaphore.
Posting to a queue, sending to it, or invalidating one of its windows
records the queue; when a thread lets go of the lock it posts the
semaphores of the queues it gave work to. WinGetMsg waits on its
semaphore (with a timeout for the next timer) instead of polling.
WinSendMsg to another thread queues the message and waits on the
sender's own semaphore for the reply, answering messages sent to it
meanwhile; if the receiver's process ends, the sender gets 0. Activation
and focus notifications (WM_ACTIVATE, WM_SETFOCUS, WM_SHOW, WM_DESTROY of
other threads' windows) are posted, never waited for.

**The PM server.** The first message queue in the system starts
`C:\OS2\PMSRV.EXE` (next to PMWIN's directory, detached). It owns the
desktop window (painting the background) and runs:

- an input thread at time-critical priority that reads raw input
  (`Rs2ReadInput`), moves the pointer, hit-tests the shared window tree
  and posts each event at once to the queue of the thread that owns the
  target window (WM_MOUSEMOVE coalesced with one still waiting at the end
  of that queue; WM_CHAR to the window that has the focus at that moment,
  so type-ahead goes where the focus was when the key was pressed). Mouse
  capture, the disabled owners of modal dialogs and activation on click
  are handled here; frame moves and sizes started by a title bar or
  sizing border are tracked here too (the thread that owns the frame only
  starts them). Nothing in the input path waits for an application.
- process clean-up: on `EV_PROCEXIT` (and from PMWIN's DLL termination in
  a process that ends normally) the process's windows, queues, timers,
  classes, presentation spaces, GPI objects and accelerator tables go;
  threads waiting for a reply from it get 0; the next frame is activated
  and what comes to light repaints.
- the window list (Ctrl+Esc): frames, full-screen sessions and the
  command prompt launchers.

The server never waits for an application: what it sends to another
thread's window is queued like a posted message and its result dropped.
When the last PM program has ended the server ends too, and with it the
PM session (the system returns to the text session).

**The pointer.** With virtio-gpu or VMware SVGA II the pointer is a
hardware cursor that the kernel moves (`Rs2SetPointerShape`), so it keeps
up even if every PM process stopped. Otherwise the server draws it in
software with a save-under; PMWIN code that draws on the screen (in any
process) hides it around each drawing operation, within one PMWIN call
under the lock, never across application code, so a stopped program
cannot leave it hidden. A program that hides the pointer with
WinShowPointer and then stops responding gets it shown again.

The pointer's shape is a pointer object (see [Pointers and
icons](#pointers-and-icons)): uploaded to the hardware cursor when it
changes (inverted pixels become black there), or drawn in software. When
the pointer comes to another window the input thread sets the arrow;
the window's `WM_MOUSEMOVE` processing may set another (`WinDefWindowProc`
sets the arrow, entry fields and MLEs the text pointer, sizing borders
the size pointers), so a window whose program does not respond shows
the arrow. Mouse messages carry `KC_SHIFT`, `KC_CTRL` and `KC_ALT` in
mp2's high word: the kernel puts the keyboard's shift state into each
mouse event (`Rs2ReadInput`'s flags, high byte).

## Windowed VIO sessions (OS/2 Window)

Text-mode (VIO) programs run in PM windows as on OS/2: `START /WIN CMD`,
DosStartSession with `SSF_TYPE_WINDOWABLEVIO`, or with
`SSF_TYPE_DEFAULT` for a program marked WINDOWCOMPAT started from PM or
from another windowed session (as OS/2 resolves the default type;
NOTWINDOWCOMPAT programs and programs started from a full-screen
session get a full-screen session, PM programs join PM).

**In the kernel** (`kernel/src/viowin.rs`, `kernel/src/api/viowin.rs`)
such a session is an ordinary session: its own `Screen` (cells, cursor,
mode, palette, blink, code page, font), keyboard queue with its input
mode, line editor and monitors, mouse queue and Ctrl+C/Ctrl+Break
target. Only the framebuffer is not its own: the screen is never drawn
there while the session is windowed. Every change of a windowed screen
bumps the screen's change counter and a global generation; the host
waits for the generation in the kernel. VioGetPhysBuf, VioScrLock and
VioScrUnLock, VioSavRedrawWait and VioModeWait fail in a windowed
session with ERROR_VIO_EXTENDED_SG (494, "The VIO call issued by a VIO
windowed program is not valid"), as on OS/2, where programs that need
them are NOTWINDOWCOMPAT and run full screen; VioSetMode accepts the
text modes (and refuses graphics modes, as it does everywhere in rs2).
The PIB and local info segment report process type 2 (VIO window) for
the text programs of a windowable session.

RS2API services for the host (32-bit, `_System`):

| Ordinal | Call |
|---------|------|
| 10 | `Rs2VioWait(PULONG pulGen, ULONG ulTimeout)`: wait until the generation differs from *pulGen (ms, -1 forever) and store it; registers the caller as the host |
| 11 | `Rs2VioQuerySessions(PRS2VIOSESSION a, ULONG cMax, PULONG pcFound, ULONG fAll)`: the windowable sessions (fAll: every session but PM's) as {sid, flags, screen change counter, leader pid, title[64]}; flags: 1 windowed now, 2 activate its window (reported once), 4 windowable, 8 PM, 0x10 foreground |
| 12 | `Rs2VioReadScreen(ULONG sid, PRS2VIOSTATE p, PUSHORT pCells, ULONG cb)`: state (change counter, columns, rows, cursor row/column, first/last cursor scan line, cell height, code page, flags: cursor shown, blink, windowed, mouse open, user font; font generation; 16 colours 0x00RRGGBB) and the cells |
| 13 | `Rs2VioQueryFont(ULONG sid, ULONG cy, PBYTE p, ULONG cb)`: the 256 glyphs at height 8, 14 or 16 for the session's code page (or its VioSetFont font) |
| 14 | `Rs2VioKey(ULONG sid, PINPUTEVENT p)`: a key as Rs2ReadInput reports it (character, scan code, extended flag, shift state, make/break): through the session's keyboard monitors, then its input mode (Ctrl+C, Ctrl+Break, Ctrl+S, PrtSc), to its queue; Alt+Home switches it to the full screen |
| 15 | `Rs2VioMouse(ULONG sid, LONG row, LONG col, ULONG fsButtons)`: the pointer over a cell: events into the session's mouse queue (and monitors) if it opened the mouse |
| 16 | `Rs2VioControl(ULONG sid, ULONG cmd, ULONG arg)`: 1 focus, 2 windowed (1) or full screen (0), 3 end the session (its processes are killed), 4 select any session, 5 register as the host |

**The host**, `C:\OS2\PMVIOWIN.EXE` (`userland/pmviowin`), is an
ordinary PM program. The kernel starts it (detached) when the first
windowed session appears and there is none; it ends with the last
windowed session unless started with `/SHELL`. It has one frame window
per windowable session (title from the session's title) and never waits
for a session:

- a watcher thread waits in `Rs2VioWait` and posts one message to the
  window thread, which reads the changed screens and invalidates the
  cells that changed (at most about 50 updates a second); a program that
  hangs or spins just stops changing its screen, and the window keeps
  painting, moving, sizing and closing;
- painting renders the cells with the kernel's own fonts (unscii 8x16,
  8x14, 8x8 for the session's code page), the session's palette and
  blink/intensity setting, the cursor shape (scaled to the font) blinking
  while the window has the focus, blinking characters, and the marked
  block inverted (crate `viocell`, host-tested, written so that AVIO can
  use it later), and draws them with GpiDrawBits;
- the window takes the size of the text mode (the session's cell height
  unless a font size was chosen) and follows mode changes (`MODE
  CO80,43`); when it is smaller than the screen, the scroll bars scroll it;
- the system menu adds Mark, Copy, Copy All, Paste, Fullscreen (Alt+Home)
  and Font 8 x 16 / 8 x 14 / 8 x 8. Dragging with mouse button 1 marks a
  block (unless the program opened the mouse, which then gets the mouse
  events); Mark marks with the arrow keys (Shift extends, Enter copies,
  Esc cancels). Copy and Copy All put CF_TEXT on the clipboard (trailing
  blanks dropped, CR LF line ends); Paste types CF_TEXT into the session
  (line ends as Enter);
- Close (Alt+F4, the system menu, End task in the window list) asks
  whether to end the programs in the window and kills them; the window
  goes when the session ends (also when its CMD runs EXIT).

**Keys** do not go through the host. PMWIN's input thread knows the
client windows (`Rs2PmVioWindow(hwnd, sid, fl)`, private ordinal 9002)
and hands a key for the one with the focus straight to the kernel with
`Rs2VioKey`, with the scan code, the extended-key flag, the full shift
state and the release, in the order typed; Alt+Space and Alt+F4 stay
with the frame, and an open menu or the host's mark mode gets keys as
WM_CHAR. Shift keys alone are not reported to the session (PM's raw
input has no events for them).

**Alt+Home** in a window switches the session to the full screen (the
window is hidden; the session is selected like any full-screen
session); Alt+Home there brings it back into its window and PM to the
front. Selecting a windowed session (DosSelectSession, the text window
list) activates its window in PM; Alt+Esc skips windowed sessions.

**The PM window list** (Ctrl+Esc) lists the frames (VIO windows among
them), the full-screen text sessions (switching to one selects it, End
task ends its processes) and two launchers: "Command prompt (window)"
and "Command prompt (full screen)" (DosStartSession of `%COMSPEC%`).

**Booting into PM with a command prompt window**: run the host as the
shell. In CONFIG.SYS

    SET COMSPEC=C:\OS2\CMD.EXE
    PROTSHELL=C:\OS2\PMVIOWIN.EXE /SHELL

(COMSPEC must name the command processor: it defaults to the shell). For
a development run, put `init=OS2\PMVIOWIN.EXE -- /SHELL` in
`work/cmdline.txt`. With `/SHELL` the host opens a command prompt window
when there is none at start and keeps running when the last window
closes (Ctrl+Esc offers new prompts). The default boot (`PROTSHELL=`
CMD.EXE) is unchanged.

## Programs that stop responding

A queue is *not responding* when something (a posted or sent message, or
something to paint) has been waiting for it longer than the hang time
without its thread coming for messages (WinGetMsg/WinPeekMsg); a thread
blocked in WinGetMsg is never hung. The hang time is 2000 ms by default
and set in CONFIG.SYS with

    SET PM_HANGTIME=milliseconds

(250 to 600000; the server reads it from its environment). For such a
queue the server, every 250 ms or when woken:

- marks it and repaints its frames' title bars grey with
  "(not responding)";
- paints a placeholder wherever its windows need painting: frames and
  the built-in controls (title bar, menus, min/max buttons, scroll bars,
  buttons...) through their own drawing code, application windows with a
  plain dialog-background fill. The update regions stay, so when the
  thread comes back it gets WM_PAINT for everything that was covered;
- handles clicks on its windows: the frame is activated and raised (the
  activation messages are posted to the program for later), the title bar
  moves it, the sizing border sizes it, the min/max buttons hide it; a
  click in a client window waits in the queue like any input. A capture
  held by one of its windows is dropped;
- lists it in the window list as "(not responding)", where End task ends
  the process (DosKillProcess; a program that responds gets SC_CLOSE
  instead) and Switch to shows and activates its frame.

When the thread reads its queue again the mark goes, the title bars
repaint, and the program catches up with its queued input and paints.
Keys typed meanwhile went to the window that had the focus when they were
typed, also if that was the stopped program's.

## Status

Milestone 1 works: the unmodified Toolkit `hello.exe` shows its window,
which can be moved by its title bar and closed, after which the system
returns to the text session. Several PM programs run at once (see
[Processes and input](#processes-and-input)), with cross-process window
order, activation and repainting.

Milestone 2 works: the Toolkit samples `hello.exe` (menu bar,
accelerators), `dialog.exe` and `template.exe` run unmodified, with
mouse and keyboard. What PMWIN has:

- Frames with their controls as real child windows laid out around the
  client: title bar (`WC_TITLEBAR`, moves the frame), system menu
  (`FID_SYSMENU`: Restore/Move/Size/Minimize/Maximize/Hide/Close/Window
  list, enabled to match the frame; double click closes), min/max
  buttons (see [Minimizing](#minimizing)), menu
  bar (`FCF_MENU`), frame scroll bars; sizing borders resize the frame.
  The frame passes `WM_COMMAND`, `WM_CONTROL`, scroll and menu
  notifications to its client.
- Resources through `DosGetResource`: menu templates (nested submenus),
  dialog templates, accelerator tables, string and message tables
  (`WinLoadString`, `WinLoadMessage`), bitmaps for `SS_BITMAP` statics.
- Menus (`WC_MENU`): menu bars, title-bar buttons and pull-down/pop-up
  menus (each submenu is a hidden menu window shown beside its item).
  Menu mode with the mouse (click or drag) and the keyboard (F10,
  Alt+mnemonic, arrows, Enter, Esc, letters), `WM_INITMENU`,
  `WM_MENUSELECT`, `WM_MENUEND`, `WM_COMMAND`/`WM_SYSCOMMAND`/`WM_HELP`
  from items, check marks, disabled items, separators, accelerator text
  after a tab; the MM_* messages (insert, delete, remove, query/set item,
  attributes, text, positions, select, start/end menu mode, rects);
  `WinLoadMenu`, `WinCreateMenu`, `WinPopupMenu`. As in PM, the focus
  window keeps the focus during menu mode (it gets no `WM_SETFOCUS`);
  the queue's keys go to the menu, also keys typed before the menu
  opened.
- Accelerators: tables of frames and of the queue are applied to WM_CHAR
  as the message loop takes it (`WinLoadAccelTable`,
  `WinCreateAccelTable`, `WinSetAccelTable`, `WinTranslateAccel`, ...),
  plus the system keys F10, Alt+F4, Alt+Space, Shift+Esc, Alt+F5,
  Alt+F9, Alt+F10 and Alt+F11; commands of
  disabled menu items are not sent.
- Dialogs: `WinLoadDlg`, `WinCreateDlg`, `WinProcessDlg` (modal loop,
  owner disabled; clicks on it bring up the dialog), `WinDlgBox`,
  `WinDismissDlg`, `WinDefDlgProc` (Tab/Shift+Tab, arrows within groups,
  Enter for the focused or default push button, Esc, mnemonics with or
  without Alt, `SC_CLOSE` as `DID_CANCEL`), `WM_INITDLG` with focus
  choice, `WinEnumDlgItem`, `WinSendDlgItemMsg`, `Win{Query,Set}DlgItem
  {Text,Short}`, `WinMapDlgPoints`. Template sizes are the area inside
  the frame controls; a dialog unit is 2x2 pixels (8x16 system font).
  Unknown control classes become placeholder frames. The actual owner of
  a dialog is the ancestor of `hwndOwner` that is a child of the
  dialog's parent (a client window's frame). `WinDefDlgProc` for a
  window that is not a dialog calls the window's class procedure (a
  control subclassed with a procedure that passes the rest to
  `WinDefDlgProc`, as the System Editor does).
- Focus and activation: activating a frame gives it the focus, which it
  passes on to `QWL_HWNDFOCUSSAVE` (the window that last had it, or one
  the application set) or its client, so the client hears that the
  focus came from the frame.
- Controls, each a registered class with its own window procedure:
  `WC_BUTTON` (push, default, check box, radio, 3-state and their
  automatic kinds, `BM_*`, `BN_CLICKED`), `WC_STATIC` (text with DT_*
  layout and mnemonics, group boxes, rectangles, frames, bitmaps),
  `WC_ENTRYFIELD` (editing, selection, insert/overwrite, scrolling,
  text limit, `ES_MARGIN`, `ES_READONLY`, `ES_UNREADABLE`, `EM_*`,
  `EN_*`), `WC_LISTBOX` (single and multiple selection, scrolling with a
  scroll-bar child, keyboard, `LM_*`, `LN_*`), `WC_SCROLLBAR` (arrows,
  paging, proportional slider tracking, `SBM_*`, `WM_VSCROLL`/
  `WM_HSCROLL`), `WC_MLE`, `WC_COMBOBOX`, `WC_SPINBUTTON`, `WC_SLIDER`,
  `WC_VALUESET` and `WC_NOTEBOOK` (see [Standard controls](#standard-controls)).
  `WM_QUERYDLGCODE` and `WM_MATCHMNEMONIC` tell the dialog manager
  about them; entry fields, MLEs, combo boxes and spin buttons use the
  clipboard (Shift+Del, Ctrl+Ins, Shift+Ins, `EM_CUT`/`COPY`/`PASTE`).
- Double clicks (`WM_BUTTONnDBLCLK`), `WM_CREATE` with a CREATESTRUCT,
  `WinSetParent`, `WinWaitEventSem`.
- Threads and processes: see [Processes and input](#processes-and-input)
  (`template.exe` uses an object window on a second thread).
  `WinMessageBox` is a modal dialog built from a template.
- `HELPMGR.DLL` (help instances are accepted, help shows nothing), the
  file and font dialogs of `PMCTLS.DLL` (see [Standard
  dialogs](#standard-dialogs)), the clipboard and atom tables (see [The
  clipboard](#the-clipboard)), `WinTrackRect` (rectangle tracking with
  the mouse or the keyboard), rectangle helpers (`WinSetRect`,
  `WinIntersectRect`, ...), `WinUpper` and the code page calls,
  `WinSetHook`/`WinReleaseHook` (hooks are recorded, not called).

Not done for milestone 2: presentation
parameters (accepted and ignored), owner-drawn items, menu bitmaps,
help windows, drag and drop.

The OS/2 System Editor (E.EXE) runs unmodified: see
[COMPATIBILITY.md](COMPATIBILITY.md#presentation-manager-programs).
rs2 ships its own E.EXE in its place (see [The System
Editor](#the-system-editor-eexe)).

Milestone 3 (GPI breadth) works as far as the GPI is concerned: every
PMGPI import of the Toolkit samples `graphic.exe`, `clipbrd.exe`,
`jigsaw.exe` and `palette.exe` is implemented; `palette.exe` and
`clipbrd.exe` run end to end. `RS2\GPITEST.EXE` runs
self-checks on memory bitmaps and shows the primitives in a window.
The GPI covers:

- Presentation spaces on window DCs (`WinOpenWindowDC`), memory DCs
  (`DevOpenDC` `OD_MEMORY`) or none (`GpiCreatePS`, `GpiAssociate`,
  `GpiResetPS`); a window PS follows its window's visible region, and
  `WinBeginPaint` can borrow it. `DevQueryCaps` describes a 24-bit,
  96 dpi display.
- Transforms: model and default viewing matrices (`TRANSFORM_REPLACE`,
  `ADD`, `PREEMPT`; `GpiTranslate`, `GpiScale`, `GpiRotate`), page units
  (`PU_PELS` .. `PU_TWIPS`) and page viewport, `GpiConvert`.
- Attributes in per-primitive bundles (`GpiSetAttrs`/`GpiQueryAttrs`),
  `AM_PRESERVE` and `GpiPop`, all foreground and background mixes.
- Lines with the `LINETYPE_*` styles and cosmetic widths, boxes with
  rounded corners, full, partial and three-point arcs with arc
  parameters, Bézier splines, fillets, markers (`MARKSYM_*`), pels.
- Areas (`BA_ALTERNATE`, `BA_WINDING`, boundaries) and paths: fill,
  stroke at the geometric width, outline, clip paths, `GpiPathToRegion`;
  the `PATSYM_*` patterns and bitmap pattern sets.
- Regions (y-x banded, with OR/AND/XOR/DIFF), clip regions and
  rectangles, `GpiPaintRegion`, `GpiFrameRegion`; `WinInvalidateRegion`,
  `WinQueryUpdateRegion`, `WinValidateRegion`, `WinScrollWindow`.
- Bitmaps: `GpiCreateBitmap`, bits in and out in 1/4/8/24 bpp with 1.x
  and 2.x headers (and RLE4/RLE8 input), `GpiLoadBitmap` from
  `RT_BITMAP` resources, `GpiBitBlt` (ternary ROPs, stretching,
  window↔memory), `GpiWCBitBlt`, `GpiDrawBits`, `WinDrawBitmap`.
- Colour: logical colour tables (`LCOLF_CONSECRGB`, `INDRGB`, `RGB`),
  RGB/index queries, palettes; on the true-colour display
  `GpiAnimatePalette` recolours what was drawn with the palette.
- Text: one font, the system font (8x16, from unscii), reported as
  System VIO; the standard outline faces (Helvetica, Times New Roman,
  Courier) are offered by `GpiQueryFonts` and drawn by scaling its
  glyphs to the character box, also as figures in paths (clip paths
  from text). Font metrics, text boxes, character positions,
  `GpiCharStringPos` with increments, opaque and clipped rectangles.

Not done: metafiles (`GpiLoadMetaFile` fails), retained segments,
printing (no printer DCs), rotated or sheared text, real outline fonts,
line ends, joins and mitre limits, `GpiPolygons` and
`GpiCreatePolygonRegion`.

Calling conventions: OS/2 compilers keep the stack 4-byte aligned while
Rust code assumes 16, so every PMWIN export goes through a generated
trampoline that aligns the stack (`tools/scripts/gen_pm_exports.py`, which
also writes `exports.def` with the Toolkit's ordinals).

## Standard controls

All are built-in PMWIN classes with their own window procedures,
reachable through `WinQueryClassInfo` and subclassing like the others;
what can be tested on the host is in `crates/pmwin`.

- `WC_MLE`, the multi-line entry field. The text keeps a line break as
  one LF, so an insertion point is a byte offset and a break counts as
  one character, as on OS/2; MLFIE_CFTEXT (CR LF), MLFIE_NOTRANS (LF)
  and MLFIE_WINFMT (CR CR LF for soft breaks) on import and export.
  Display lines are laid out per paragraph (word wrap at blanks, tab
  stops in pels) and laid out again only where an edit touched them.
  The font is the system font's glyphs scaled to the FATTRS cell
  (`lMaxBaselineExt`, half as wide for outline faces), so every face is
  monospaced. One level of undo (a typing run is one step; undoing
  again redoes). Messages `MLM_SETTEXTLIMIT` .. `MLM_QUERYIMPORTEXPORT`
  (import/export buffers, selection, insert and delete, line queries,
  format lengths, undo, fonts, colours, first character, cut, copy,
  paste and clear, refresh, search with select and replace-all, word
  wrap, tab stops, read-only, the changed flag, the format rectangle);
  notifications `MLN_CHANGE`, `MLN_SETFOCUS`/`MLN_KILLFOCUS`,
  `MLN_VSCROLL`/`MLN_HSCROLL`, `MLN_OVERFLOW` (text limit),
  `MLN_CLPBDFAIL`. Scroll bars are `WC_SCROLLBAR` children
  (`MLS_VSCROLL`/`MLS_HSCROLL`); `MLS_BORDER`, `MLS_IGNORETAB`,
  `MLS_DISABLEUNDO`, `MLS_READONLY`; mouse selection (drag, double
  click for a word, Shift+click), the CUA keys, Insert toggles
  overtype. `WinSetWindowText`/`WinQueryWindowText` work on the text
  (as CF_TEXT).
- `WC_COMBOBOX`: an entry field (`CBID_EDIT`) over a list box
  (`CBID_LIST`). `CBS_SIMPLE` shows the list all the time;
  `CBS_DROPDOWN` and `CBS_DROPDOWNLIST` (read-only entry) collapse the
  window to the entry and grow it to its template height, on top of its
  siblings, when the list drops down (the button, Alt+Down or F4; Up and
  Down step through the items). `LM_*` go to the list, `EM_*` to the
  entry; `CBM_SHOWLIST`, `CBM_HILITE`, `CBM_ISLISTSHOWING`; `CBN_*`
  notifications.
- `WC_SPINBUTTON`: a numeric range or an array of strings
  (`SPBM_SETARRAY`), wrapping at either end; arrows (held ones repeat),
  Up, Down, PgUp and PgDn, `SPBS_NUMERICONLY`, `SPBS_READONLY`,
  `SPBS_PADWITHZEROS`, justification; a master's arrows spin the
  servant (`SPBM_SETMASTER`) that last had the focus. `SPBM_*` and
  `SPBN_*`.
- `WC_SLIDER`: horizontal or vertical, home at either end, the
  increments of `SLDCDATA`, ticks (`SLM_SETTICKSIZE`), scale text,
  detents, arrow buttons, a ribbon strip, read-only, snapping; the arm
  follows the mouse (`SLN_SLIDERTRACK`) and the keyboard; `SLM_*` (the
  arm position in increments or pels, shaft and arm dimensions, tick
  positions) and `SLN_*`.
- `WC_VALUESET`: rows x columns of colour (RGB or index), text and icon
  or bitmap items (bitmaps are drawn as framed squares), item
  borders, disabled items; keyboard and mouse selection; `VM_*` and
  `VN_*`.
- `WC_NOTEBOOK`: pages in major sections (`BKA_MAJOR`) with minor
  sections (`BKA_MINOR`; the minor tabs of the top page's section
  show), back pages on any corner (`BKS_BACKPAGES*`), major tabs on any
  side (`BKS_MAJORTAB*`; minor tabs on the other back-page side), tab
  text alignment, square, rounded or polygon tabs (corners cut), a
  spiral binding drawn as rings, the status line (`BKA_STATUSTEXTON`,
  aligned by `BKS_STATUSTEXT*`) with the previous/next page buttons at
  its right end, and the Warp 4 tabbed dialog (`BKS_TABBEDDIALOG`: tabs
  on top, no back pages, `BKS_BUTTONAREA` and
  `BKM_SETNOTEBOOKBUTTONS` push buttons under the page whose commands
  go to the top page's window). Tabs are as wide as their text or
  `BKM_SETDIMENSIONS`; when they do not fit, the strip scrolls to the
  top page's tab. Page windows (`BKM_SETPAGEWINDOWHWND`, `BKM_SETPAGEINFO`
  with a window, a dialog template or a dialog resource, loaded at once
  or when first turned to) become the notebook's children; the top
  page's window shows, sized to the page with `BKA_AUTOPAGESIZE`, and
  the others are hidden. Clicks on tabs and page buttons, Alt+PgUp/PgDn
  from anywhere in the notebook (a page dialog passes keys it does not
  use to the notebook), and with the focus on the tabs PgUp/PgDn, the
  arrows (from tab to tab), Home/End, the tab mnemonics (`~1`, also
  through `WM_MATCHMNEMONIC` from the dialog), Tab into the page and
  Shift+Tab out of it; F1 and `WM_HELP` send `BKN_HELP`. `BKM_*`
  (insert, delete single/tab/all, turn, query page id/count/style/data/
  window/info, tab and status texts, tab bitmaps (their pixels are
  copied for painting), dimensions, `BKM_CALCPAGERECT` both ways,
  notebook and tab colours) and `BKN_PAGESELECTEDPENDING` (clearing
  `ulPageIdNew` refuses the turn), `BKN_PAGESELECTED`,
  `BKN_NEWPAGESIZE`, `BKN_PAGEDELETED`, `BKN_HELP`. Everything it
  paints lives in the shared state, so the PM server paints a hung
  program's notebook with the same code.
- `WC_CONTAINER`: records are application memory, as on OS/2:
  `CM_ALLOCRECORD` (RECORDCORE, or MINIRECORDCORE with
  `CCS_MINIRECORDCORE`, plus the application's bytes) and
  `CM_ALLOCDETAILFIELDINFO` take them from the application process's
  own heap, linked by `preccNextRecord`/`pNextFieldInfo`, and emphasis
  lives in each record's `flRecordAttr`. The control keeps only their
  addresses and order (roots, tree children, the icon view's z-order,
  the detail columns) in the shared state and reads the records only on
  its own thread, which is in the application's process; the PM server
  paints a plain placeholder for a container whose program hangs.
  Views: icon (`ptlIcon` positions, text centred below the icon, wrapped;
  `CCS_AUTOPOSITION` and `CM_ARRANGE` arrange rows from the top left),
  name and text (`CV_FLOW` fills columns), detail (`FIELDINFO` columns:
  strings, ULONG, `CDATE`, `CTIME`, icons; widths from `cxWidth` or the
  content; headings with `CA_DETAILSVIEWTITLES`; `CFA_SEPARATOR`,
  `CFA_HORZSEPARATOR`, alignment; the split bar is drawn after
  `pFieldInfoLast` but both sides scroll together) and tree (indent,
  expand/collapse buttons, `CA_TREELINE`, text/name/icon variants);
  `CV_MINI` for 16 x 16 icons; the container title with
  `CA_CONTAINERTITLE`, separator and alignment. Emphasis: selected
  (highlighted text, or row in the detail view), cursored (focus
  rectangle), in-use (hatched), source and target (dotted frame),
  disabled. Selection by the style: single, extended (click, Ctrl+click,
  Shift+click and Shift+arrows) and multiple (clicks toggle); the
  keyboard (arrows by geometry in every view, Home, End, PgUp, PgDn,
  Space, Enter, Ctrl+/ and Ctrl+\, + and - and left/right in the tree,
  Shift+F10, F1); scroll bars as needed (`CID_VSCROLL`/`CID_HSCROLL`).
  A double click or Enter is `CN_ENTER`; button 2 is `CN_CONTEXTMENU`
  (also `WM_CONTEXTMENU`), or, dragged over a record, `CN_INITDRAG`: if
  the owner does not take the drag (returns 0) the icon view moves the
  record, and the other selected ones, itself. Alt+click opens direct
  editing (`CN_BEGINEDIT`, `CN_REALLOCPSZ`, `CN_ENDEDIT`; an entry
  field with id `CID_MLE`, Enter keeps, Esc drops); `CM_OPENEDIT` and
  `CM_CLOSEEDIT` too. Messages: `CM_ALLOCRECORD`, `CM_FREERECORD`,
  `CM_INSERTRECORD`, `CM_INSERTRECORDARRAY` (order, parent, z-order),
  `CM_REMOVERECORD` (`CMA_FREE`, `CMA_INVALIDATE`, all), `CM_QUERYRECORD`
  (first, last, next, previous, parent, children; item or z-order),
  `CM_QUERYRECORDEMPHASIS`, `CM_SETRECORDEMPHASIS`, `CM_SETCNRINFO`/
  `CM_QUERYCNRINFO`, `CM_ARRANGE`, `CM_INVALIDATERECORD`,
  `CM_ERASERECORD`, the FIELDINFO messages, `CM_SORTRECORD` (the
  application's comparison runs without the PM lock; `pSortRecord` in
  CNRINFO sorts too), `CM_QUERYRECORDRECT`, `CM_QUERYRECORDFROMRECT`,
  `CM_QUERYVIEWPORTRECT`, `CM_SCROLLWINDOW`, `CM_SEARCHSTRING`,
  `CM_EXPANDTREE`/`CM_COLLAPSETREE`, `CM_FILTER`; notifications
  `CN_ENTER`, `CN_EMPHASIS` (for the user's changes), `CN_CONTEXTMENU`,
  `CN_INITDRAG`, `CN_SETFOCUS`/`CN_KILLFOCUS`, `CN_SCROLL`,
  `CN_EXPANDTREE`/`CN_COLLAPSETREE`, `CN_HELP` and the editing ones. Not
  done: owner draw (`CA_OWNERDRAW`, `CRA_OWNERDRAW`), bitmaps instead of
  icons (`CA_DRAWBITMAP`), separately scrolled split windows, the grid
  (`CM_SETGRIDINFO` ...), `CM_MOVETREE`, drag and drop between
  windows, rubber-band selection.

## Pointers and icons

Pointer and icon objects (`HPOINTER`) live in the shared state, so the
PM server can draw any process's icons (a minimized frame of a program
that hangs); a process's own go when it ends, system pointers belong to
nobody. `WinLoadPointer` reads RT_POINTER resources and
`WinLoadFileIcon` icon files and EAs in the OS/2 pointer format
(`gpi::icon`): one image or a `BA` array of them, monochrome (`IC`,
`PT`) or colour (`CI`, `CP`, an AND/XOR mask bitmap followed by the
colour bitmap), 1.x or 2.x headers, any bit count the bitmap code reads.
From an array PMWIN takes the 32 x 32 image (for `SV_CXICON` 32) with
the most colours made for any display or this one, else the nearest
larger one, scaled, and the 16 x 16 one for the mini icon (else the big
one scaled down). `WinCreatePointer` (a mask bitmap) and
`WinCreatePointerIndirect` (POINTERINFO) make pointers from the
caller's bitmaps, `WinQueryPointerInfo` gives the caller new bitmaps
back, `WinDestroyPointer`, `WinSetPointerOwner`. `WinDrawPointer`
draws with `DP_NORMAL`, `DP_MINI`, `DP_HALFTONED` and `DP_INVERTED`.
`WinQuerySysPointer` (with fLoad: a copy) returns rs2's own drawings
(`pmwin::sysptr`, nothing copied from IBM's): the arrow, text, wait,
move/size and the four sizing pointers, the application icon, the
information, question, error and warning icons, illegal, file, folder,
multiple files and program. `WinSetPointer`/`WinQueryPointer` set and
query the mouse pointer's shape. `WinQuerySysPointerData` and
`WinSetSysPointerData` are not supported.

Statics with `SS_ICON` (a resource id) and `SS_SYSICON` (an `SPTR_*`) show
their icon and take its size; `WinMessageBox` shows the system icon of
`MB_ICONQUESTION`, `MB_ICONEXCLAMATION`, `MB_ICONASTERISK` and
`MB_ICONHAND` left of its text; value sets draw `VIA_ICON` items. A
frame loads its icon with `FCF_ICON` (the frame's resource id), takes
another with `WM_SETICON` (`WM_QUERYICON` returns it) and shows the
mini icon on its system menu button.

**File icons** (PMWP.DLL): `WinLoadFileIcon` returns the file's `.ICON`
extended attribute, else for an executable (`.EXE`, `.DLL`) its
`RT_DEFAULTICON`, else `RT_POINTER` 1, else its first `RT_POINTER`
resource (read from the file with `os2_exe::res`, without loading it),
else rs2's folder, program or file icon; the file is read with the PM
lock released. `WinSetFileIcon` writes the `.ICON` EA from an icon file
(`ICON_FILE`), a resource (`ICON_RESOURCE`) or data (`ICON_DATA`), and
`ICON_CLEAR` removes it; `WinFreeFileIcon`.

## Minimizing

Minimizing a frame (its minimize button, the system menu, Alt+F9,
`WinSetWindowPos` with `SWP_MINIMIZE`) sends it
`WM_MINMAXFRAME` with the new position, hides its controls and client
and, as in classic PM, turns it into an icon with its title below at
the bottom of the desktop, in the first free place of rows from the
bottom left (or where it was last minimized; `pmwin::minimize`). The
frame gets `WS_MINIMIZED` (`WinQueryWindowPos` reports `SWP_MINIMIZE`),
goes to the bottom of the z-order and passes the activation on. The icon
is the frame's icon or rs2's application icon; a click activates it,
dragging moves it, a double click, Enter, Restore in the system menu,
Alt+F5, `SWP_RESTORE` or Switch to in the window list restore it (with
`WM_MINMAXFRAME`, maximized again if it was). Hide in the system menu
hides the frame. For programs that do not respond the PM server does
the same: their min button minimizes, a double click on the icon
restores, the icon is painted from the shared state.

A desktop shell can take minimized windows over: `Rs2SetMinimizeViewer
(hwnd)` (PMWIN private ordinal 9003; 0 goes back to desktop icons,
returns the previous viewer) makes minimizing hide the frame instead,
and the viewer gets `WM_RS2_MINWINDOW` (0x0ff8; mp1 = the frame, mp2 = 1
minimized, 0 restored); restoring works as before.

Other calls a shell uses: `Rs2SetDesktopWindow(hwnd)` (private ordinal
9007) keeps a desktop window below all others (see [The desktop
shell](#the-desktop-shell-rs2shell)); `WinSetWindowPos` with
`SWP_ZORDER` and `HWND_BOTTOM` sends a window below its siblings;
`WinWindowFromPoint` (the topmost showing child, or with fChildren the
deepest descendant, under a point) and `WinQueryWindowProcess`;
`WinSetPresParam`/`WinQueryPresParam`/`WinRemovePresParam` keep
`PP_FOREGROUNDCOLOR` and `PP_BACKGROUNDCOLOR` (RGB) per window, which
the container paints with (other presentation parameters are still
accepted and ignored).

## The clipboard

The clipboard and the atom tables are part of the shared PM state, so
every PM process sees the same ones. The clipboard is open for one
message queue at a time (`WinOpenClipbrd` waits, answering messages sent
to the caller meanwhile). Data handed to `WinSetClipbrdData` belongs to
PM, as on OS/2:

- `CFI_POINTER` formats (`CF_TEXT`, `CF_DSPTEXT`, registered formats):
  the giveable shared memory the application allocated is copied into
  the shared state (text up to its NUL, other formats the committed
  size `DosQueryMem` reports) and freed; `WinQueryClipbrdData` gives
  every process a pointer to that copy, which is mapped at the same
  address everywhere.
- `CF_BITMAP` (and `CF_DSPBITMAP`): the bitmap moves out of the
  setter's GPI objects into the clipboard; each process that asks gets
  its own bitmap handle with the picture, deleted when the clipboard is
  emptied.
- Other `CFI_HANDLE` data is kept as it is; data 0 is delayed
  rendering: the owner gets `WM_RENDERFMT` when someone asks for it.

`WinEmptyClipbrd` tells the owner (`WM_DESTROYCLIPBOARD`), closing
after a change tells the viewer (`WM_DRAWCLIPBOARD`);
`WinEnumClipbrdFmts`, `WinQueryClipbrdFmtInfo`,
`WinSetClipbrdOwner`/`Viewer` and their queries. Registered formats
come from the system atom table (`WinAddAtom` .. `WinQueryAtomUsage`,
integer atoms `#n`, private tables with `WinCreateAtomTable`). What a
process had open or owned is released when it ends.

## Standard dialogs

PMCTLS.DLL builds its dialogs as an application would: a dialog template
made in memory, `WinCreateDlg` and `WinProcessDlg`. A wrapper procedure
makes the FILEDLG/FONTDLG the `QWL_USER` window word before
`WM_INITDLG` reaches the application's procedure (`pfnDlgProc`), which
passes the rest to `WinDefFileDlgProc`/`WinDefFontDlgProc`.

- `WinFileDlg`: Open and Save as (`FDS_OPEN_DIALOG`,
  `FDS_SAVEAS_DIALOG`, `FDS_HELPBUTTON`, `FDS_APPLYBUTTON`,
  `FDS_MODELESS`): a file name field, file type and drive drop-down
  lists, the current directory's subdirectories (a double click enters
  one, `..` goes up) and files (a click fills in the name, a double
  click chooses it); names typed relative or absolute, a pattern
  changes the file list, a directory changes the directory; Open
  reports a missing file; `FDM_VALIDATE` lets the application refuse a
  name; `szFullFile`, `lReturn`, `sEAType`. The push buttons are where
  IBM's template has them, so applications that add one between OK and
  Cancel fit. Multiple selection, filtering by EA type and `FDM_FILTER`
  are not done.
- `WinFontDlg`: the system font and the outline faces (Helvetica, Times
  New Roman, Courier; normal and bold) in point sizes, outline,
  underline and strikeout, a preview in the chosen font; OK, Cancel,
  Apply, Reset, Help. The FONTDLG (`fAttrs`, `fxPointSize`, family
  name, `flStyle`, weight, em height) follows the choice as it changes,
  since applications' procedures read it when OK or Apply arrives.

## Profiles (INI files)

PMSHAPI's `Prf*` calls work on OS/2 binary INI files, shared by every
process in the system. The format logic is a host-tested `no_std`
crate, `crates/inifile` (parse, serialise, the program list records;
`cargo run -p inifile --example inidump FILE.INI` dumps a profile);
the DLL (`userland/pmshapi`) does the files, the sharing and the API.
PMSHAPI does not import PMWIN: text-mode programs load it too (REXXUTIL's
`SysIni`, IBM TELNET).

**Format.** Little-endian; offsets from the start of the file.

| Record | Size | Fields |
|--------|------|--------|
| header | 20 | `FFFFFFFF`, offset of the first application (0x14), file size, two reserved words (0) |
| application | 20 | offset of the next application (0: last), offset of its first key (0: none), reserved (0), name length with its NUL (u16), the same again (u16), offset of the name |
| key | 24 | offset of the next key (0: last), reserved (0), name length with NUL (u16), again (u16), offset of the name, value length (u16), again (u16), offset of the value |

IBM's writer puts each application record, then its name, then for
each key the key record, the key name and the value, in list order; an
empty profile is the header alone (first offset 0x14 = file size).
`inifile` writes exactly that layout: the IBM files on the media
written that way (`epw.ini`, `MMPMCONN.INI`, `MARKVIS\MODULES.INI`)
and the 39 KB OS2.INI IBM's MAKEINI builds from `INI.RC` come out byte
for byte the same after parsing and re-serialising; MAKEINI in turn
updates files rs2's PMSHAPI wrote without losing anything. Files IBM's writer
updated in place can hold stale records nothing links to (`vpd.ini`,
which also has non-zero reserved header words): readers follow the
links only. Values hold at most 65535 bytes. The reader checks every
offset and length, cuts chains that leave the file or loop, and keeps
what it could read; a file without the signature is "foreign" (not
opened by `PrfOpenProfile`, never overwritten), an empty file is an
empty profile.

**Names are case-sensitive.** The PM Programming Reference says so for
every Prf call ("There is no case-independent searching"; "This avoids
any code-page dependency"), and IBM's own names mix case
(`PM_Workplace:Location`, `PM_WorkPlace:Restart`); so `App`, `APP` and
`app` are three applications. (The in-memory PMSHAPI before this
compared case-insensitively.)

**Order.** Applications and keys keep the order of the file; new keys
and new applications are appended. IBM's writer behaves differently
for applications, as seen in MAKEINI, which carries its own copy of
the profile code: it prepends a new application and, reading a file,
builds its list in reverse, so every load and save reverses the order
of the applications (keys keep theirs and are appended). Application
order therefore means nothing in IBM's files; rs2 keeps it stable.

**Which files.** `HINI_USERPROFILE` and `HINI_SYSTEMPROFILE` are the
files named by `USER_INI` and `SYSTEM_INI` (`SET USER_INI=` in
CONFIG.SYS, as on OS/2) in the environment of the first process that
uses PMSHAPI, else `\OS2\OS2.INI` and `\OS2\OS2SYS.INI` on the boot
drive; they are created (empty) on first use. `HINI_PROFILE` (0)
searches the user profile, then the system profile (enumerations list
both, user entries first), and writes to the user profile.
`PrfOpenProfile` resolves the name to a full path, creates a missing
file, refuses the current user and system profiles and foreign files,
and returns a handle valid in the calling process; the same file can
be open under several handles. `PrfCloseProfile` fails for
`HINI_USERPROFILE`/`HINI_SYSTEMPROFILE` and unknown handles.
`PrfQueryProfile` returns both names (`cch` 0 asks for the length,
which includes the NUL; FALSE when a name was cut). `PrfReset` makes
another file the user profile for every process (created if missing);
the system profile name must stay the same.

**Calls.** As documented in the PM reference: `PrfQueryProfileData`
copies as much as fits, sets `*pulBufferMax` to the bytes copied and
returns FALSE if that was not all; `PrfQueryProfileString` copies the
value (or the default; 0 without either) with a NUL in the buffer's
last byte when it had to cut, and returns the bytes copied with the
NUL; `PrfQueryProfileInt` returns the leading decimal number (0 if the
value does not start with one, the default if the key is missing);
`PrfQueryProfileSize` gives a value's size or the buffer an
enumeration needs. With `pszApp` NULL the calls enumerate the
applications, with `pszKey` NULL the keys of one application, as
NUL-terminated names followed by a final NUL (the length reported
leaves out that final NUL; too small a buffer gets a cut list and
FALSE). Writing NULL data deletes a key, a NULL key deletes the
application, NULL application, key and data empty the profile; an
application whose last key is deleted goes too.

**Sharing.** One named mutex semaphore (`\SEM32\PMSHAPI\PRF`)
serialises all profile access in the system; every call holds it from
start to end in a must-complete section (a process being killed
finishes its update; if one dies holding it anyway, the next caller
gets ERROR_SEM_OWNER_DIED and carries on). A named shared memory object
(`\SHAREMEM\PMSHAPI\PRF`, 4 KB) holds the current user and system
profile names and 256 generation counters, one per hash of a file's
full path. Each process keeps the profiles it uses parsed in memory;
before each use it compares the file's counter and its size and time
stamp with what it saw and re-reads the file if either changed (the
time stamp catches files replaced by other means, such as COPY). Every
write goes to disk at once, still under the mutex, and bumps the
counter, so the next call in any process sees it and a process that
dies loses nothing. Writing through costs a whole-file rewrite per
call: 200 writes to a small profile take about 0.5-0.8 s under QEMU
(TCG); nothing is cached dirty, so there is nothing to flush at
`PrfCloseProfile` or process end.

**Safe replacement.** A profile `X.INI` is replaced the way IBM's
writer does it (traced in MAKEINI): the new contents go to `X.!!!`,
which is flushed (`DosResetBuffer`); then `X.INI` is renamed `X.###`,
`X.!!!` renamed `X.INI`, and `X.###` deleted (`DosMove` never
replaces a file; this works on FAT and HPFS). A crash leaves either
`X.INI`, or `X.###` and a complete `X.!!!`: when `X.INI` is missing,
`X.!!!` becomes the profile if it parses as a whole profile, else
`X.###` is renamed back. A read-only profile is read but not written.

**The program list.** `PrfCreateGroup`, `PrfAddProgram`,
`PrfChangeProgram`, `PrfQueryDefinition`, `PrfQueryProgramTitles`,
`PrfQueryProgramHandle`, `PrfRemoveProgram` and `PrfDestroyGroup` keep
groups and programs in a profile (the user profile for `HINI_PROFILE`),
so a desktop shell can read them from OS2.INI. The Toolkit's
os2386.lib has no 32-bit ordinals for `PrfCreateGroup` and
`PrfQueryProgramHandle`: they are at rs2-private ordinals 9000 and 9001
(`userland/pmshapi/exports.private`). Everything is in the application
`PM_RS2_ProgramList`:

- key `NextHandle`: the next handle to give out (4 bytes,
  little-endian), so a removed entry's handle is not reused;
- one key per group or program, named by its handle as eight
  upper-case hex digits (`0000000C`), in creation order (readers skip
  keys that are not eight hex digits); the value:

| Offset | Size | Contents |
|--------|------|----------|
| 0 | 4 | layout version, 1 |
| 4 | 4 | the group the entry belongs to; `SGH_ROOT` (0xFFFFFFFF) for a group |
| 8 | 4 | `PROGTYPE.progc` (`PROG_GROUP`, 5, for a group) |
| 12 | 4 | `PROGTYPE.fbVisible` (`SHE_VISIBLE` 0, `SHE_INVISIBLE` 1, `SHE_PROTECTED` 2) |
| 16 | 36 | `SWP swpInitial` (fl, cy, cx, y, x, hwndInsertBehind, hwnd, two reserved) as given |
| 52 | ... | six fields: title, executable, parameters, startup directory, icon, environment; each a u16 length (0xFFFF: the pointer was NULL) and that many bytes, no NUL; the environment holds its strings each with its NUL, without the list's final NUL |

`inifile::proglist` reads and writes these records. Queries put the
strings after the structures in the caller's buffer
(`PrfQueryDefinition`: PROGDETAILS, 72 bytes, then the strings, NULL
pointers for absent ones; `PrfQueryProgramTitles`: the PROGTITLE
array, 16 bytes each, then the titles; `SGH_ROOT` lists the groups);
called with no buffer they return the size needed (and the count),
with too small a buffer 0. `PrfCreateGroup` returns the existing group
of the same title; `PrfAddProgram` needs an existing group;
`PrfRemoveProgram` removes programs, `PrfDestroyGroup` a group with
its programs; `PrfQueryProgramHandle` matches the executable
case-insensitively, a name without a path in any directory.

**Not done.** WinGetLastError codes (PMSHAPI does not call PMWIN), the
`PL_ALTERED` broadcast after `PrfReset`, the 16-bit `Prf16*`/`Win16*`
entry points, `PrfQueryProgramCategory`, `WinStartApp` and the switch
list calls; the profile names in shared memory last while some process
has PMSHAPI loaded (afterwards `USER_INI`/`SYSTEM_INI` apply again, so
a `PrfReset` is forgotten); the program list is rs2's own layout, not
IBM's (Warp 4's Workplace Shell keeps its objects in `PM_Abstract:*`
and `PM_Workplace:*` instead).

## The desktop shell (RS2SHELL)

`C:\OS2\RS2SHELL.EXE` (`userland/rs2shell`) is rs2's own desktop: an
ordinary PM program, a stepping stone towards the Workplace Shell
(there is no SOM). It is the protected shell of the distributed system
(`PROTSHELL=C:\OS2\RS2SHELL.EXE` in CONFIG.SYS), so the kernel starts
it after CONFIG.SYS and starts it again if it ends. Only one runs (a
named mutex semaphore); without a graphics display it runs the
text-mode command processor (`OS2_SHELL`) instead. `RS2SHELL /MAKEINI`
only makes the default objects and ends.

**The desktop** is a frame without border or title covering the screen,
with a container (icon view) as its client's child. PMWIN keeps it
below every other window: `Rs2SetDesktopWindow(hwnd)` (PMWIN private
ordinal 9007; 0 ends it, returns the previous one) pins a top-level
window at the bottom of the z-order, so activating it does not raise
it and windows sent to the bottom (`HWND_BOTTOM`, minimized desktop
icons) go just above it. The window list leaves it out (no title). Its
colour is the container's `PP_BACKGROUNDCOLOR` presentation parameter
(white text on dark colours, `PP_FOREGROUNDCOLOR`), from OS2.INI. It
shows the visible program groups as folders (by default Command
Prompts and Programs), Drives, the Minimized Window Viewer, the Window
List (it opens PMWIN's window list, as Ctrl+Esc does), the program
objects of the hidden group "Desktop" and Shutdown.

**Folders** open in windows (title bar, sizing border, min/max buttons,
a View menu: icon, details or tree view, Arrange, Refresh, Close); each
remembers its place, size and view. Kinds:

- a program group: its program objects (icons from the program's
  `.ICON` EA or resources through `WinLoadFileIcon`; command prompts
  have rs2's own);
- Drives: one icon per drive (`DosQueryCurrentDisk`; CD icon for CDFS
  drives, labels from `DosQueryFSInfo`);
- a directory (a drive's root, then each subfolder in a new window):
  folders first, then files, with `WinLoadFileIcon` icons and, in the
  details view, size, date and time; directories are arranged, not
  positioned;
- the Minimized Window Viewer: the frames minimized meanwhile.

**Objects** open with a double click or Enter: folders open, program
objects start (`DosStartSession` in the object's working directory, or
the program's, with its parameters and session type; `*` is the
command processor), executables start (`.EXE`/`.COM` as their type
says, `.CMD`/`.BAT` in a command prompt window with `/K`), other files
open with the program associated with their extension (OS2.INI
`RS2_Associations`: TXT, DOC, LOG, ME, SYS, CFG, ... with the System
Editor, `C:\OS2\E.EXE`), minimized windows are restored. Mouse button 2 opens a pop-up
menu: on objects Open, Settings (the program settings notebook) or
Properties (a file's size, date, attributes), Open in editor, Rename
(direct editing, also F2 or Alt+click), Delete (with confirmation, also
the Del key; folders with their contents; Command Prompts and Programs
cannot be deleted); on a folder's background View, Arrange, Refresh,
Create program object, Create folder (a directory: a long name on HPFS,
`NEWFOLDR` on FAT), Open parent, Close; on the desktop Arrange,
Refresh, Create program object, Create folder (a program group), Window
list, System setup and Shut down. F5 refreshes.

**Dragging** with mouse button 2 (the container's `CN_INITDRAG`, then
the shell tracks the pointer with the mouse captured): within a folder
the icons move (positions are kept in OS2.INI at once); dropped on
another folder window or on a folder, drive or group icon, files and
directories move (`DosMove`, across drives copy and delete), program
objects move to the other group, and a program file dropped on the
desktop or a group becomes a program object.

**Program objects** are the program list in OS2.INI (see
[Profiles](#profiles-ini-files)), so programs using the `Prf*` program
list calls see them and the shell sees theirs (any visible group
appears on the desktop). The settings notebook (a `BKS_TABBEDDIALOG`
notebook with three page dialogs) edits the path and file name (Find...
opens the file dialog), parameters and working directory, the session
type (as the program says, OS/2 window, OS/2 full screen, Presentation
Manager) and the title, through `PrfAddProgram` or `PrfChangeProgram`.
Renaming a group makes a new group with the programs (the program list
has no call to rename one).

**What the shell keeps in OS2.INI** besides the program list:

| Application | Key | Value |
|-------------|-----|-------|
| `RS2_Desktop` | `Version` | `1` once the default objects have been made (the shell's MAKEINI step, rs2's own content, not IBM's INI.RC) |
| | `DesktopGroup`, `PromptsGroup`, `ProgramsGroup` | the group handles (hex) |
| | `Background` | desktop colour, `RRGGBB` |
| | `MinimizeTo` | `VIEWER` (default) or `DESKTOP` |
| `RS2_IconPos` | `FOLDER\|OBJECT` | `x y` (the container's `ptlIcon`); folders `DESKTOP`, `G` + group handle, `DRIVES`; objects `G`/`P` + handle, `DRIVES`, `DRIVE:C`, `SHUTDOWN`, ... |
| `RS2_Windows` | folder (`DIR:C:\OS2\` for directories) | `x y cx cy view` |
| `RS2_Associations` | extension (upper case) | the program |

The first start makes the groups Desktop (hidden), Command Prompts
(OS/2 Window, OS/2 Full Screen) and Programs (System Editor if E.EXE is
installed, Install rs2), the associations and the colour; a missing
piece is made again later.

**Minimized windows** go to the Minimized Window Viewer: the shell is
PMWIN's minimized window viewer (`Rs2SetMinimizeViewer`), so minimizing
hides the frame and the shell lists it; a double click restores it, the
pop-up menu also closes it. System setup can switch back to icons at
the bottom of the desktop (above the pinned desktop window).

**System setup** sets the desktop colour (six presets or `RRGGBB`) and
where minimized windows go.

**Shutting down** (the Shutdown object, the desktop's menu, or closing
the desktop) asks whether to shut down or restart, then: the text
sessions (full screen and windowed) end with their programs; every PM
program's frame gets `WM_SAVEAPPLICATION` and `WM_CLOSE`, as OS/2's
shutdown does, so a program may still ask about unsaved work (the
System Editor does); a status window counts the programs left. When
none is left (after 20 s the shell asks whether to end the rest with
`DosKillProcess`), `DosShutdown(0)` flushes and stops the file systems
(profiles are written through at once anyway) and `Rs2PowerOff` turns
the machine off or restarts it. If the machine cannot turn itself off,
a black screen says that it is safe to do so.

**Booting into the desktop** is the default of the distributed system
(dist/CONFIG.SYS) and of systems installed from it. Fallbacks to the
text-mode command prompt: press T while the boot loader waits (one
second; `bootwait=MS` on the kernel command line, `\RS2\CMDLINE`, sets
the wait, 0 none), which adds `textmode` to the kernel command line;
`textmode` there runs `OS2_SHELL` (CMD.EXE) instead of PROTSHELL; or
`PROTSHELL=C:\OS2\CMD.EXE` in CONFIG.SYS. From the desktop, Ctrl+Esc
offers command prompts in a window or full screen. For development,
`cargo xtask run --desktop` boots into the shell (`init=OS2\RS2SHELL.EXE`
in place of work/cmdline.txt's `init=`).

**Not done**: drag and drop with other programs (`DrgDrag`), copying
files (only moving), shadows, the tree view of a directory showing
subfolders, a Startup folder, templates, object settings other than
program objects', the lock-up, a real Workplace Shell object model.

## The System Editor (E.EXE)

`C:\OS2\E.EXE` is rs2's own System Editor (userland/e), written to
behave like OS/2's E (IBM's runs as well, see
[COMPATIBILITY.md](COMPATIBILITY.md#presentation-manager-programs)): a
standard frame titled with the file's full name (or "Untitled") whose
client holds a WC_MLE (System VIO 8 x 16, word wrap on, both scroll
bars), and E's menus built in memory:

- **File**: New, Open (WinFileDlg), Save, Save as (WinFileDlg with the
  file types Plain Text, OS/2 Command File, DOS Command File; the type
  chosen is written as the .TYPE EA), Autosave (a spin button: save
  after N changes). Save on a file without a .TYPE asks first, as E
  does ("Save notification": Type... or Save as...; the Type dialog
  sets the type and saves); a file's type is kept when it is rewritten.
  `E FILE` opens a file; a file that does not exist opens empty under
  its name. Text is read and written as CR LF (MLFIE_CFTEXT).
- **Edit**: Undo, Cut, Copy, Paste, Clear (enabled from the selection,
  MLM_QUERYUNDO and the clipboard at WM_INITMENU), Find (Ctrl+F, a
  modeless dialog: Find, Change then find, Change, Change all, case
  sensitive, wrap; MLM_SEARCH), Select all.
- **Options**: Set font (WinFontDlg, MLM_SETFONT), Set colors (16 named
  colours for text and background with a sample), Word wrap On/Off.
  These and the autosave settings are kept in OS2.INI
  (`SystemEditor`/`Settings`).
- **Help**: Help index, General help, Using help, Keys help (HM_*
  requests to the help instance) and Product information. The help
  instance (WinCreateHelpInstance, library `E.HLP`, a help table
  mapping the menu items to panels of the same numbers;
  WinAssociateHelpInstance with the frame) answers HM_QUERY_KEYS_HELP;
  the dialogs' Help buttons show their panels (HM_DISPLAY_HELP). There
  is no E.HLP yet.

Closing the window with unsaved changes asks "NAME has changed. Do you
want to save it?" (Save, Save as, Discard, Cancel, Help).
WM_SAVEAPPLICATION (sent by the shell at shutdown, before WM_CLOSE)
saves the options and asks the same question; the WM_CLOSE that
follows uses that answer instead of asking again (Cancel keeps the
window, and the shutdown waits).

`E /SELFTEST` checks the file and editing logic on a hidden window and
prints PASS/FAIL and `e selftest done: N failure(s)` (also on the
serial port): see [Testing](#testing).

**Not done**: drag and drop of files onto the window, the window's icon,
printing (OS/2's E has none either), the help file itself.

## Debugging

With `PMTRACE` set in a program's environment (`SET PMTRACE=1` before
starting it), PMWIN logs every PM call the program makes, with its
arguments, on the serial port (`[pid] tid WinSendMsg(f, 1d4, 0, 0)`).
Only the application's calls are logged, not PMWIN's own nested ones.
The export trampolines pass their index to the lock's entry
(`gen_pm_exports.py` writes the names table).

## Milestones

1. (done) `hello.exe` from the Toolkit samples (WinInitialize, message queue,
   WinRegisterClass, WinCreateStdWindow, WinGetMsg/WinDispatchMsg,
   WinBeginPaint, WinFillRect, GpiCharStringAt, WinMessageBox) shows its
   window on a desktop, can be moved with the mouse and closed.
2. (done) Dialogs and standard controls from resources (`dialog.exe`,
   `template.exe`): WinDlgBox, buttons, entry fields, list boxes, static
   text, menus, accelerators.
3. (GPI done) GPI breadth (`graphic.exe`, `clipbrd.exe`, `jigsaw.exe`,
   `palette.exe`): paths, areas, bitmaps, fonts, transforms, regions,
   palettes, the clipboard.
4. (done) The System Editor (E.EXE): the MLE, the clipboard across
   processes, combo boxes, spin buttons, sliders, value sets, the file
   and font dialogs; then rs2's own E.EXE (see [The System
   Editor](#the-system-editor-eexe)).
5. (done) A desktop shell: rs2's own RS2SHELL (see [The desktop
   shell](#the-desktop-shell-rs2shell)); the Workplace Shell (SOM) much
   later.

## Testing

- Host unit tests in `crates/gpi` (render into memory and compare pixels;
  the Toolkit's sample bitmaps when `work/media` is present) and
  `crates/pmwin` (window tree, region computation, message routing).
- `RS2\CTLTEST.EXE` (userland/ctltest): self-checks of the MLE, entry
  fields, combo boxes, spin buttons, sliders and value sets (messages
  and notifications), atoms and the clipboard (CF_TEXT, CF_BITMAP and a
  registered format, also read and replaced by a second process: it
  runs itself with `CHILD`), printing PASS/FAIL and `ctltest done: N
  failure(s)`.
- `RS2\SHELLTST.EXE` (userland/shelltest): self-checks of what a desktop
  shell needs: system pointers, pointer info and bitmaps, creating,
  copying and destroying pointers, the pointer's shape, file icons
  (programs, directories, the `.ICON` EA set and cleared with
  `WinSetFileIcon`), frame icons, minimize and restore (`WM_MINMAXFRAME`,
  positions, styles, the minimized window viewer) and the container
  (allocating, inserting, querying, emphasis, notifications from keys,
  sorting with an application comparison, searching, detail columns,
  the tree, removing and freeing), printing PASS/FAIL and `shelltest
  done: N failure(s)`, then a window with a container of icons and the
  system icons drawn normal, mini and halftoned (`SHELLTST QUIT` ends
  at once).
- `E /SELFTEST` (userland/e; `init=OS2\E.EXE -- /SELFTEST` in
  work/cmdline.txt): the System Editor's checks on a hidden window:
  a file opened into the MLE (LF inside, CR LF back out), edited,
  saved with a .TYPE and saved again keeping it, Find selecting a
  match, autosave after N changes, a 200 KB file's round trip, a
  missing file opening empty and saved, New, and WM_SAVEAPPLICATION /
  WM_CLOSE with and without a pending answer; printing PASS/FAIL and
  `e selftest done: N failure(s)`. Its dialogs and menus were checked
  against IBM's E with `--type` screenshots (`init=OS2\E.EXE --
  C:\README.TXT`; IBM's is `C:\IBM\E.EXE` in a development image), and
  from the `--dist --desktop` image: Programs > System Editor, and
  Shutdown with unsaved text (the question, Discard, the power-off).
- `RS2\NBTEST.EXE` (userland/nbtest): notebook self-checks (pages,
  texts, page data and info, page windows and a page dialog from a
  template, notifications, keys, clicks, sizes, deleting) printing
  PASS/FAIL and `nbtest done: N failure(s)`, then a window with a
  settings-style notebook and a tabbed dialog (`NBTEST QUIT` ends at
  once, `NBTEST HANG` stops reading its queue for 30 s first).
- `cargo test -p inifile` (the INI format; IBM's files from
  `work/media` when present, or from `$RS2_WORK`) and
  `RS2\INITEST.EXE` (userland/initest): profile self-checks printing
  PASS/FAIL and `initest done: N failure(s)`: the Prf calls with small
  buffers and defaults, enumerations, deletes, HINI_PROFILE's search
  order, PrfQueryProfile/PrfReset, recovery from `X.!!!`/`X.###`,
  read-only and foreign files, a second process reading and writing
  the same profiles (`CHILD`) and two writing at once (`WRITER`), the
  program list. `INITEST WRITE` and, after another boot with the same
  `--hdd` image (`cargo xtask disk [--hpfs]`, found by its label),
  `INITEST READ` check that a profile survives a reboot.
- `RS2\DESKTEST.EXE` (userland/desktest): self-checks of the desktop
  shell: its default objects (it runs `RS2SHELL /MAKEINI` first), a
  program object created, changed and removed through the program list
  calls the way the settings notebook does it, icon positions in
  `RS2_IconPos`, and PMWIN's `Rs2SetDesktopWindow` (z-order),
  `WinWindowFromPoint`, `WinQueryWindowProcess` and presentation
  parameter colours, printing PASS/FAIL and `desktest done: N
  failure(s)`.
- The desktop in QEMU with screenshots (`cargo xtask run --desktop`, or
  the dist ISO), driven by `--type-file` scripts of `{mouse_abs}`
  clicks: Drives > C: > OS2 in the icon and details views, an OS/2
  Window and the System Editor started from their objects, README.TXT
  opened in the editor by its association, a program object made with
  the settings notebook, icons dragged, a folder created and renamed, a
  file dragged into it and deleted, E.EXE minimized into the Minimized
  Window Viewer, Shutdown with E asking about its changes, and a second
  boot of the same disk (`cargo xtask disk --system`, `--hdd --boot-hdd`)
  showing the new object and the icons where they were left; T at the
  loader for the text-mode boot.
- `RS2\GPITEST.EXE` (userland/gpitest): GPI self-checks printing
  PASS/FAIL and `gpitest done: N failure(s)`, then a window with every
  kind of primitive for a screenshot check (it also draws
  `C:\YOSEMITE.BMP` when present).
- QEMU runs of the Toolkit PM samples, checked with screenshots and with
  mouse/keyboard input through `xtask run --type` (`{mouse_move dx dy}`,
  `{mouse_abs x y}`, `{mouse_button n}`, `{shot NAME}` for a screenshot
  as `work/NAME.png` at that point).
- VIO windows (QEMU runs with screenshots): IBM CMD.EXE started with
  `START /WIN CMD` from a full-screen CMD, from PM (the window list's
  launcher) and as the boot shell (`init=OS2\PMVIOWIN.EXE -- /SHELL`):
  DIR and scrolling output, typing, MODE CO80,43 and CO80,50 (the window
  follows; with the 8 x 16 font 50 rows do not fit and the scroll bars
  scroll), the font sizes, Ctrl+C, IBM TEDIT, `RS2\APITEST` inside a
  window (its console checks expect ERROR_VIO_EXTENDED_SG for
  VioGetPhysBuf and VioScrLock there), Alt+Home to the full screen and
  back, two and three windows at once with a REXX loop spinning in one,
  marking with the mouse and Copy, then Paste into E.EXE and back, Close
  with its confirmation, EXIT closing the window.
- `RS2\PMHANG.EXE` and `RS2\PMBUSY.EXE` (userland/pmhang): a window whose
  procedure never returns from a click (or the H key), blocked in
  DosSleep or spinning without system calls; button 2 stops it for six
  seconds only; X ends the process while a second thread keeps calling
  PMWIN. Started next to another program (a CMD file with
  `DETACH HELLO` and `DETACH PMHANG`), they check that the pointer keeps
  moving, the other program keeps getting keys and clicks, the stopped
  frame can be moved, sized, hidden and brought back, the window list
  ends it, and a program that comes back repaints.
