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
| `userland/pmshapi` etc. | Smaller DLLs: profile (OS2.INI) APIs, program list and task position (PMSHAPI), HELPMGR stubs, PMDRAG (no drag is ever in progress), PMMERGE (the entry points programs import from it directly), LIBUNI and UCONV (locales and code page conversion, see [Locales](#locales-libuni-and-uconv)). |

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
- `HELPMGR.DLL` (see [The help manager](#the-help-manager-helpmgrdll-and-viewexe)), the
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

`C:\OS2\E.EXE` is rs2's own System Editor (userland/e). Its behaviour
follows OS/2's E, studied in IBM's binary with Ghidra (`e.sym` from the
debug symbols) and in its resource DLL (EHXDLMRI.DLL); the code, texts
and templates are rs2's own. IBM's E runs as well (see
[COMPATIBILITY.md](COMPATIBILITY.md#presentation-manager-programs)).

- **Window**: a standard frame (class `EHXMAIN`, task list entry) titled
  with the file's full name or "Untitled" (after the program's own title
  and " - " when the frame has one); the client holds a WC_MLE (id
  0x200, both scroll bars, no border). The window comes back where it
  was (OS2.INI `WINDOWPOSITION`, written on exit), else where the shell
  puts programs (WinQueryTaskSizePos).
- **Menus** with E's ids (File 0x400: New 0x402, Open 0x401, Save 0x403,
  Save as 0x404, Autosave 0x406; Edit 0x410: Undo 0x411, Cut 0x422, Copy
  0x423, Paste 0x424, Clear 0x425, Find 0x426, Select all 0x427; Options
  0x428: Set font 0x42a, Set colors 0x42b, Word wrap 0x42c with On 0x433
  and Off 0x434; Help 0x42d: Help index 0x42f, General help 0x431, Using
  help 0x42e, Keys help 0x430, Product information 0x408). WM_INITMENU
  enables them: Autosave needs a file name; Cut/Copy/Clear a selection;
  Paste text on the clipboard; Find and Select all some text; Undo names
  what MLM_QUERYUNDO reports ("Undo typing", "Redo paste", ...). As in
  E, a menu command ends the undo history (MLM_RESETUNDO). Ctrl+F (also
  with Shift) is Find and F3 closes the window (command 0x409); the
  editing keys are the MLE's own. A failed MLE edit command says so
  ("Clipboard Error").
- **Files**: `E [file]`, File > Open (WinFileDlg, Help shows panel
  0x1043), New. A file that is not there opens empty under its name.
  Text is CR LF (MLFIE_CFTEXT); E writes a Ctrl+Z end-of-file mark after
  it and drops one when reading. NUL characters in a file are shown as
  blanks after asking ("Continue loading file" / "Stop loading file").
  Read and write errors offer Retry. Save with no changes says there is
  nothing to save.
- **File types (.TYPE EA)**, decided before a save as E does: on a file
  system without EAs nothing; once a save in this session dealt with
  the file's type, nothing more; **Save as** and a new file through it:
  the default type (Plain Text, or what the Type dialog chose last,
  OS2.INI `USDEFAULTTYPEEA`); a malformed .TYPE: replaced by the
  default; a type the editor knows (Plain Text, OS/2 Command File, DOS
  Command File; EAT_ASCII, EAT_MVST or EAT_MVMT): kept; **no type**:
  "Save notification" (Type... opens the Type dialog, whose Set saves
  with that type; Save as...; Cancel); **types it does not know**: a
  question whether to add "Plain Text" (Yes keeps them and adds it, No
  saves as they are, Save as..., Cancel). Saving for autosave titles
  these "Autosave notification", and Cancel there turns autosave off.
  Save as (WinFileDlg with the type list, a Type... button and Help
  0x1072) asks before replacing a file.
- **Closing**: WM_CLOSE (the close button, F3, the shell's shutdown,
  which also sends WM_SAVEAPPLICATION first) with unsaved changes beeps
  and asks "NAME has changed. Do you want to save it?" (Save, disabled
  for Untitled, and Save as stay in the question until the file is
  written; Discard empties the window; Cancel keeps it). A WM_QUIT from
  outside asks the same after the message loop ends; Cancel then calls
  WinCancelShutdown and the loop goes on.
- **Autosave** (File > Autosave: a check box and a spin button, 1 to
  9999 changes; Set only with a valid number): MLN_CHANGE counts
  changes, and after N of them File > Save runs.
- **Find** (Ctrl+F, modeless): Find (default once there is text),
  Change, then find, Change and Change all (enabled after a match was
  found), Case sensitive and Wrap (kept in OS2.INI); the texts stay for
  the next time; nothing found says so ("Find Notification").
- **Set font**: WinFontDlg with Apply (both OK and Apply set the font
  and tab stops of 8 average character widths; `FONTATTRS`,
  `FXPTSIZE`). **Set colors**: foreground and background from 16 named
  colours and Default, samples of normal and selected text, Apply (then
  Cancel is the default button) and Set (`ULFGNDCOLOR`,
  `ULBGNDCOLOR`). **Word wrap** On/Off (`BWRAPSTYLE`).
- **Settings** are OS/2's E's: OS2.INI application `EHXMAIN`, keys
  `IAUTOSAVE`, `BAUTOSAVEON`, `ULFGNDCOLOR`, `ULBGNDCOLOR`, `BWRAPSTYLE`,
  `BCASESENSEARCH`, `BWRAPSEARCH`, `USDEFAULTTYPEEA` (numbers as
  strings), `FONTATTRS`, `FXPTSIZE`, `WINDOWPOSITION` (binary).
- **Help**: E.HLP is looked for on the HELP path and in `\OS2\HELP` of
  the boot drive (then OS/2's EHXHP.HLP, whose panels have the same
  numbers). Found, it gets a help instance (WinCreateHelpInstance with
  E's help table: the frame's subtable maps the menu items, and one per
  dialog, by E's dialog ids, maps its controls to the dialog's panel;
  WinAssociateHelpInstance with the frame). The Help menu sends
  HM_HELP_INDEX, HM_EXT_HELP, HM_DISPLAY_HELP (Using help) and
  HM_KEYS_HELP (E answers HM_QUERY_KEYS_HELP); dialogs' Help buttons
  show their panel (HM_DISPLAY_HELP). Without a help file Help says it
  is not available. rs2's own E.HLP is written in IPF in
  `userland/e/E.IPF` (one panel per res= number of the help table, in
  rs2's own words) and compiled by xtask with `crates/ipf`'s compiler
  (`ipf::compile`; by hand: `cargo run -p ipf --example ipfc E.IPF
  E.HLP`) into `OS2\HELP\E.HLP` of the development and dist images.

`E /SELFTEST` checks the file and editing logic on a hidden window and
prints PASS/FAIL and `e selftest done: N failure(s)` (also on the
serial port): see [Testing](#testing).

**Not done**: dragging files onto the window (E takes a dropped file),
the window's icon, Undo restoring a font or colours (rs2's MLE
does not undo those), the Set tabs dialog E has but does not offer.

## The help manager (HELPMGR.DLL) and VIEW.EXE

HELPMGR.DLL (userland/helpmgr) is rs2's help manager, VIEW.EXE
(userland/view) its book viewer. Both follow IBM's, studied in
HELPMGR.DLL, VIEW.EXE and PMMERGE.DLL with Ghidra (`helpmgr.sym`,
`view.sym` and `pmmerge.sym` from the debug symbols name every function;
`tools/ghidra/lxload.sh` and `lxdecomp.sh`). The help files are read by
`crates/ipf` (format, text and layout rules from IBM's `IOLoad*`,
`FormatCell` and bitmap code; tested on every INF/HLP file on the media;
described in [formats/ipf-structures.md](formats/ipf-structures.md),
[formats/ipf-text.md](formats/ipf-text.md) and
[formats/ipf-bitmaps.md](formats/ipf-bitmaps.md)).

**Requests from F1 to a panel**, as in IBM's PMWIN and HELPMGR:

- The system accelerator table has F1 as WM_HELP (as PMMRGRES.DLL's
  table 1 does). WM_HELP from controls, menus (MIS_HELP) and
  accelerators goes to the owner; WinDefWindowProc sends it on to the
  parent, so it ends at a frame.
- A frame given WM_HELP (IBM's `FrameProcessHelp`) calls the help hooks
  (HK_HELP) with the focus window's rectangle: HLPM_WINDOW (topic: its
  owner's id) for controls of dialogs, else HLPM_FRAME (topic: the
  frame's id), the subtopic being the focus window's id (a combination
  box's entry field stands for the box). F1 in an open menu ends menu
  mode and calls the hooks with HLPM_MENU, the pull-down's id and the
  item's. The hooks of the thread's queue are called newest first until
  one returns TRUE; the chain is read again after each, since OS/2's E
  installs its own hook that creates the help instance on the first F1,
  releases itself and leaves the request to the help manager's.
- The help system commands go to the help instance of the frame (or of a
  frame above it or, at the desktop, owning it): SC_HELPEXTENDED as
  HM_EXT_HELP, SC_HELPINDEX as HM_HELP_INDEX, SC_HELPKEYS as HM_KEYS_HELP,
  SC_SWITCHPANELIDS as HM_SET_SHOW_PANEL_ID; the client gets the command
  when there is no help instance. Frames keep their help instance
  (WM_SETHELPINFO, WM_QUERYHELPINFO) and say they are frames
  (WM_QUERYFRAMEINFO).
- WinCreateHelpInstance opens the help libraries of HELPINIT's
  pszHelpLibraryName (blank-separated; along HELP, then BOOKSHELF) and
  the external databases they name, and returns an object window (class
  `HMHelpInstance`). The first instance of a process sets the help hook
  on its queue (IBM's `HOOKProc`).
- The hook (IBM's `HelpProcessor`) takes requests for the active window
  of its own process: it finds the help instance up the frames
  (HAPIFindWndWithHelp), the help table entry of the active window's id
  (HELPTABLE in memory, or RT_HELPTABLE/RT_HELPSUBTABLE resources) and
  the subtable entry of the subtopic (`HOOKSearchHelpTable`; FID_CLIENT
  means the extended help panel). An unknown subtopic is sent to the
  application as HM_HELPSUBITEM_NOT_FOUND, and the extended help panel
  shows unless it returns TRUE; HM_EXT_HELP_UNDEFINED and HM_ERROR as
  IBM's.
- HM_DISPLAY_HELP (resource id or panel name, local then global names),
  HM_EXT_HELP, HM_HELP_INDEX, HM_HELP_CONTENTS, HM_KEYS_HELP
  (HM_QUERY_KEYS_HELP to the application), HM_SET_ACTIVE_WINDOW,
  HM_LOAD_HELP_TABLE, HM_CREATE_HELP_TABLE, HM_SET_HELP_WINDOW_TITLE,
  HM_SET_SHOW_PANEL_ID, HM_REPLACE_HELP_FOR_HELP, HM_SET_HELP_LIBRARY_NAME
  and HM_DISMISS_WINDOW; HM_NOTIFY (OPEN_PAGE, OPEN_TOC, OPEN_INDEX) to
  the active window. HM_DISPLAY_HELP 0 ("Using help") shows the
  replacement panel, else the contents: IBM's own help library
  (HMHELP.HLP) is not shipped.

**The help window** is a frame on the right of the screen titled with the
help window title and the panel's: the panel (laid out by `crates/ipf`
for the window's width and laid out again when it changes), a vertical
scroll bar, and Previous, Search..., Index and Contents push buttons.
Panels draw in the fonts of the file's font table (below), styles as
logical fonts' fsSelection (real italic, bold, underscore), colours of
`:hp4.`/`:color.` and links in SYSCLR_HELPHILITE;
bitmaps are decompressed by `crates/ipf` and drawn with GpiDrawBits
(stretched for `fit`). Up/Down/PgUp/PgDn/Home/End scroll; Tab and
Shift+Tab move a box from link to link, Enter or a click follows it, Esc
(or Backspace, or Previous) goes back. Links lead to panels of the file
or, by resource id, any library; footnotes and panels without a
contents entry open in a footnote window over the help window (Esc
closes it); inform links send HM_INFORM to the application; launch links
start the program (found along PATH); links into external databases
open the panel there. Contents (the visible TOC entries, indented by
level), Index (level-2 entries indented as IBM's) and Search (the panels
having every word typed, from the file's search table) are lists in the
same window; Enter or a double click opens the panel. `:hide.` text shows
when IPF_KEYS has its key.

**Fonts.** A `:font.` selects an entry of the file's font table (face,
point height and width); dynamic data adds its own (`DdfSetFont`,
numbers 16 and up, "default" being entry 0). IBM's help manager matches
each to the nearest *image* font of the face at the display's resolution
(FontFindFontMetrics: closest lMaxBaselineExt and average width). rs2
takes the nearest of the point sizes IBM's image fonts come in (Helv and
Tms Rmn 8-24, Courier 8-12), at least the system font's 16 pixels, drawn
with rs2's scaled faces (Helvetica, Times New Roman, Courier:
GpiCreateLogFont outline and GpiSetCharBox); System faces and entry 0
are the system font. `:hp1.`-`:hp7.` are the font's logical fonts with
fsSelection italic, bold and underscore, which GPI simulates (slanted,
one pel wider, underlined; GpiSetCharShear slants outline fonts too).

**Dynamic data (DDF).** The Ddf* functions (HELPMGR 74-88) build a
buffer of cell text as IBM's (`bhddf.c`): words written out as escape
0x66 with no autospacing (0xFE one space), links by panel name (0x65) or
number (0x1D), inform links, styles, colours (DdfSetColor's mapping of
PM colours), fonts (0x67), lists (DdfBeginList/DdfListItem with margin
escapes 0x02/0x11/0x12/0x22), text as it is with an alignment
(DdfSetFormat/DdfSetTextAlign: escapes 0x1A/0x1B), the application's
bitmaps (0x68, placed as `:artwork.`) and metafiles (0x69, a box of the
rectangle: GpiPlayMetaFile draws nothing yet). A handle is valid until
the help manager has read it. When a panel with `:ddf res=N.` is laid
out, the help manager sends HM_QUERY_DDF_DATA (mp1 the panel window, mp2
N) to the HM_SET_OBJCOM_WINDOW window, else the application's (IBM's
MscAppInterface: HM_SET_ACTIVE_WINDOW's, else the associated frame),
lays out the text where the escape is and frees the handle; it keeps
the text for the panel until HM_INVALIDATE_DDF_DATA (mp1 a count, 0
all, mp2 the ids) asks for it again. A reply that is not a DDF posts
HM_ERROR 0x2014. Frames send messages they do not process
(0x213-0x406, WM_USER and up) to their client as IBM's frame procedure
does, so applications get HM_ messages in their client.

**Windows of their own (viewports).** A link's data (`vpx=`, `vpy=`,
`vpcx=`, `vpcy=`, `group=`, `viewport`, `auto`, `dependent`) or the
target heading's (`x=`, `y=`, `width=`, `height=`, `group=`) opens the
panel in a viewport: a frame inside the help window, placed in the
area above the buttons as IBM's PnlConvertUnit/PnlConvertCord compute
it (characters, percent, pixels, points, or left/right/top/bottom/
center; 0xFFFF keeps the window's value), following the help window's
size. A panel with a size, a link asking for a viewport, or a group
whose viewport is open goes to a viewport; a group's panels replace
each other in one viewport. `auto` links open their panel when the
panel holding them is shown (inform links tell the application, launch
links start the program); viewports close when the panel they were
opened from changes (all of them for the help window's own panel, the
dependent ones for a viewport's) or with Esc. The help window's own
panel is placed where its heading puts it. `:acviewport.` loads the
DLL, calls the procedure with an ACVP (hab, hmq, object id, the panel
window as parent and owner) and the object info, and places the frame
it returns by the tag's window data (IBM's MscStartObject); HM_ messages
sent to a panel window go to the help instance, and HM_QUERY answers
HMQW_COVERPAGE, HMQW_VIEWPORT, HMQW_OBJCOM_WINDOW, HMQW_INSTANCE,
HMQW_ACTIVEVIEWPORT and HMQW_GROUP_VIEWPORT.

**Message boxes.** WinMessageBox's MB_HELP adds a Help button
(MBID_HELP, BS_HELP | BS_NOPOINTERFOCUS, as IBM's MboxInit) and the box
has idWindow as its id, so Help and F1 reach the help hook as HLPM_WINDOW
help for that id.

**VIEW.EXE** `book[+book...] [topic]` opens the books (along BOOKSHELF,
then HELP; `.INF` added when there is no extension) at the first panel
whose title (else index entry) has the topic, else at the contents, and
ends when the window is closed. IBM's VIEW is a launcher handing books
to a VIEWDOC server through shared memory; rs2's calls the help
manager's `Rs2ViewBook` (private ordinal 200) instead.

IBM's VIEW.EXE and VIEWDOC.EXE run on rs2's HELPMGR too. VIEW zeroes a
0x164-byte block (IBM's instance structure) and calls HELPMGR's
unpublished ordinals on it: HMSetHelpDatabase (37) opens the books,
IOLoadAllTOC (28) and IOLoadAllIndex (29) put arrays of entries where
VIEW reads them (+0x76/+0x78: `char *title, u8 level, u16 flags`, 0x800
not in the contents; +0x68/+0x6A: `char *text`, TOC index at +8, flags
at +10), it matches the topic with FindPattern, FindWildPattern,
FindTokenPattern, MscStriCmp and MscStrniCmp (33-35, 46, 47; GetStrToken
36), IOFree* (30-32) close them, MscAppendINF (45) expands its argument
(an environment variable naming books, `.INF` added). VIEWDOC opens the
books with WinCreateBookInstance (26) and sends the instance
HM_HELP_CONTENTS, 0x235 (show TOC entry mp1), 0x236 (bring the window
up) and 0x1000 (its window, which gets 0x1003 with the instance when
the book's window closes); window word 0 of the instance points at a
block with the help window's frame at +0x26, as VIEWDOC reads it. The
help manager's window procedures and entry points switch to a stack of
their own when less than 64 KB of the caller's is left (VIEWDOC's
stack is 8 KB). HELPMGR's other unpublished ordinals (38-44: MscCleanUp
and path helpers) are not there; nothing on the media imports them.

Not done: the Print button, the help window's menu (Services,
Options), bookmarks and the library list, metafiles of dynamic data,
the viewport style word (title bar, scroll bars, rules: viewports always
have a title bar and a vertical scroll bar), HMQW_INDEX/TOC/SEARCH/
VIEWPAGES/LIBRARY, and the panel placement of HM_DISPLAY_HELP and
HM_HELP_CONTENTS by heading window data beyond the help window's own
panel.

## Locales (LIBUNI and UCONV)

LIBUNI.DLL (userland/libuni) has OS/2's Unicode locale calls: locale
objects, items and values (`UniQueryLocaleItem`/`Value`/`Info`),
`UniStrftime`, locale lists, language and country names, user locales
(`UniMakeUserLocale`, `UniSetUserLocaleItem`, `UniDeleteUserLocale`,
`UniCompleteUserLocale`), character classification
(`UniQueryCharType`, `UniQueryAlpha`...) and the `UniStr*` string
functions. It follows IBM's LIBUNI and LOCALE.DLL, studied with Ghidra
(`libuni.sym`, `locale.sym`); the data and logic are in `crates/uls`
(no_std, host-tested with `cargo test -p uls`).

- **System locales** (what IBM compiles into LOCALE.DLL) are rs2's own:
  `tools/scripts/gen_locales.py` writes `crates/uls/src/data.rs`, 41
  locales (`posix`, `univ` and 39 language/country pairs) with all 136
  items (LOCI_*), as NUL-terminated UCS-2 in one pool. Country
  conventions come from gen_nls.py's table (the same as rs2's
  COUNTRY.SYS data); names of days, months, languages and countries are
  written in the script. As in IBM's data, x items (`xLocaleToken`,
  `xWinLocale`) are hex digits without `0x`.
- **User locales** are new locales and overrides of system ones (an
  override keeps the system locale's name). They live in
  `<boot>:\LANGUAGE\LOCALE\USER.LOC` in IBM's text format:
  `[codepage 850]`, then per locale `[locale NAME BASE]` and lines
  `    itemName (padded to 19) = "value"`, only for items that differ from
  the base; characters outside printable ASCII are code page 850 bytes
  (Latin-1 letters) or `[hex]`, and `[` is `[[`. A process reads the file
  when it first needs it and again when it creates a locale object or
  lists locales with nothing unsaved; `UniCompleteUserLocale` writes it
  (creating the directory). IBM keeps them in shared memory instead.
- **Locale objects** hold a locale name per category; an empty spec
  takes `LC_ALL`, `LC_category` or `LANG`, and without them the locale
  of the system country (IBM's installation sets LANG; rs2's does not).
  `UniQueryLocaleObject(LC_ALL)` gives the six names separated by
  blanks, as IBM's does. Items of system locales are returned from the
  static pool (`UniFreeMem` leaves them alone); others are allocated.
- **Export 151, `Locale`**, is the table LOCALE.DLL's `locale_init`
  returns and IBM's LIBUNI exports unchanged: the highest item number,
  the pool's address range and functions taking their arguments in EAX
  and EDX (`locale_name`, `locale_uniname`, `locale_token`,
  `locale_item`, `locale_value`, `locale_data`, `locale_itemname`,
  `locale_itemnum`, `locale_itemcategory`). GLOCALE.EXE calls them
  directly (userland/libuni/src/table.rs).

UCONV.DLL (userland/uconv) converts between UCS-2 and the single-byte
code pages of rs2's NLS tables (`UniCreateUconvObject` with `IBM-nnn`,
or the process code page for an empty name or one with only modifiers
such as `@path=no`); `UniUconvToUcs`/`FromUcs`, `UniStrToUcs`/`FromUcs`,
`UniMapCpToUcsCp`. Double-byte code pages are not converted.

Not done: `UniStrfmon`, `UniStrptime`, `UniStrtod`, transform and
attribute objects, collation beyond case folding (`UniStrcoll`,
`UniStrxfrm`), language and country names in languages other than
English and the language itself.

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
  work/cmdline.txt): the System Editor's checks on a hidden window: a
  file opened into the MLE (its Ctrl+Z dropped, LF inside, CR LF back
  out), edited and saved (with the Ctrl+Z), the .TYPE rules (a known
  type kept, Save as giving the default type, unknown types kept or
  "Plain Text" added, malformed and single-ASCII values, no second
  question after a save), Find (case, wrap), autosave after N changes,
  a 200 KB file's round trip, a missing file opening empty, the EHXMAIN
  settings, New, and closing (WM_CLOSE, F3) without changes; printing
  PASS/FAIL and `e selftest done: N failure(s)`. Its menus and dialogs
  were compared with IBM's E in `--type` screenshots (`init=OS2\E.EXE
  -- C:\README.TXT`; IBM's is `C:\IBM\E.EXE` in a development image),
  and checked from the `--dist --desktop` image: Programs > System
  Editor, and Shutdown with unsaved text (the question, Cancel, the
  shell's offer to end it).
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
- The help manager: `cargo test -p ipf` (the IBM files on the media:
  structures, rendered text of the System Editor's help and REXX.INF,
  every panel laid out and every bitmap decoded), and QEMU runs with
  screenshots (`--init "OS2\VIEW.EXE CMDREF"`, `--init "IBM\E.EXE"`
  with IBM's EHXHP.HLP, CMDREF.INF and REXX.INF in work/modules.txt):
  the contents, a panel with lists, a syntax diagram bitmap and links,
  Tab and Enter on a footnote link (the footnote window), Esc back,
  Search, and E's F1 and Help menu. `RS2\HMTEST.EXE` (userland/hmtest,
  with HMTEST.HLP from HMTEST.IPF) checks the rest: `HMTEST 1` dynamic
  data (Tab to the inform link and Enter: the data is asked for again),
  `HMTEST 2` fonts and styles, `HMTEST 5` viewports (groups, dependent,
  auto, heading-placed), `HMTEST /MB` a message box with MB_HELP (F1).
  `--init "OS2\VIEW.EXE IPF IPF-Controlled"` shows the Toolkit's IPF
  sample (samples/pm/ipf: IPF.INF to OS2\BOOK, IPF.DLL to OS2\DLL in
  work/modules.txt) with IPF.DLL's application-controlled viewport;
  `--init "IBM\VIEW.EXE CMDREF XCOPY"` IBM's VIEW and VIEWDOC (from
  disk 2, in C:\IBM).
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
