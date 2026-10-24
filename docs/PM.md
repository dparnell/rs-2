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
| `crates/gpi` | Rendering engine (and `icon.rs`: the OS/2 pointer and icon formats, choosing an image for the display, AND/XOR masks, drawing): presentation-space attributes, lines, boxes, arcs, areas, text with bitmap and outline fonts (TrueType and Type 1, anti-aliased), bitmaps and raster operations, clipping to rectangle lists. Draws into a `Surface` (32-bit pixels). No OS dependencies; unit-tested on the host. |
| `crates/pmwin` | Window system core: window tree, z-order, visible and update regions, hit testing, the queues of threads to wake after invalidation; message queues with mouse-move coalescing and the not-responding bookkeeping; the MLE's text engine (`mle.rs`: layout with word wrap and tabs, editing, undo, import/export formats, search) and the arithmetic of spin buttons, sliders and value sets (`controls.rs`); the notebook's pages and geometry (`notebook.rs`); the container's layout for each view, hit testing, keyboard navigation, icon arrangement and selection rules (`container.rs`); where minimized frames go (`minimize.rs`); rs2's own system pointers and icons (`sysptr.rs`). Plain data structures, unit-tested on the host. |
| `userland/pmwin` → PMWIN.DLL | Exports (`Win*`, `Gpi*`), the shared state and its heap, locking, messaging, the built-in window classes, and the PM server's code (input routing, hung programs, window list). |
| `crates/viocell` | Text cells on pixels: renders VGA text cells (font, palette, blink, cursor, marked block) to 24-bit bitmaps; Copy/Copy All text and Paste keystrokes. Host-tested; for VIO windows now, AVIO later. |
| `userland/pmviowin` → `C:\OS2\PMVIOWIN.EXE` | The windows of windowed VIO sessions (see [Windowed VIO sessions](#windowed-vio-sessions-os2-window)). |
| `userland/pmsrv` → `C:\OS2\PMSRV.EXE` | The PM server process: only calls PMWIN's `Rs2PmServer` (private ordinal 9000; 9001, `Rs2ShellPosition`, gives PMSHAPI's `WinQueryTaskSizePos` the shell's next cascade position; 9002, `Rs2PmVioWindow`, registers a VIO window's client). |
| `userland/pmgpi` → PMGPI.DLL | Exports (`Gpi*`, `Dev*`) as forwarders to PMWIN, which implements them at ordinal 4000 + the PMGPI ordinal, since presentation spaces belong to windows. |
| `userland/pmctls` → PMCTLS.DLL | The standard file and font dialogs (`WinFileDlg`, `WinFontDlg`): ordinary PM dialogs, built from templates made in memory, run through PMWIN like an application's. |
| `userland/pmwp` → PMWP.DLL | The Workplace Shell calls programs use without it: `WinLoadFileIcon`, `WinSetFileIcon`, `WinFreeFileIcon`, forwarded to PMWIN (rs2-private ordinals 9004-9006); `WinStoreWindowPos`/`WinRestoreWindowPos`; `WinCreateObject` for program objects. |
| `userland/pmspl` → PMSPL.DLL | The spooler API (`Spl*`) of a system without printers (see [Status](#status)). |
| `userland/registry` → REGISTRY.DLL, `userland/pmwinx` → PMWINX.DLL | The registry (Win32 registry calls) and Open32's forwarders to it (see [Open32 and the registry](#open32-and-the-registry-pmwinx-registry)). |
| `userland/pmbidi` → PMBIDI.DLL, `userland/thailib` → THAILIB.DLL | Bidirectional language support of a system without bidirectional code pages (see [Bidirectional languages](#bidirectional-languages-pmbidi)). |
| `crates/inifile` | OS/2 binary INI files: parse (defensively) and write in IBM's layout, the program list records. Host-tested, also on IBM's INI files from the media. See [Profiles](#profiles-ini-files). |
| `userland/rs2shell` → `C:\OS2\RS2SHELL.EXE` | rs2's desktop shell (not the Workplace Shell): the desktop, folders, drives and files, program objects, shutdown. See [The desktop shell](#the-desktop-shell-rs2shell). Its icons are drawn in `crates/pmwin` (`shellicons.rs`). |
| `userland/pmshapi` etc. | Smaller DLLs: profile (OS2.INI) APIs, program list and task position (PMSHAPI), PMDRAG (direct manipulation: see [Drag and drop](#drag-and-drop-pmdrag)), PMMERGE (IBM PMMERGE's ordinals, see below), LIBUNI and UCONV (locales and code page conversion, see [Locales](#locales-libuni-and-uconv)). The help manager is HELPMGR.DLL (see [The help manager](#the-help-manager-helpmgrdll-and-viewexe)). |

## Kernel services (module RS2API)

A kernel gate module, `RS2API`, holds rs2-private 32-bit services used by
rs2's own DLLs (applications never import it):

- `Rs2MapDisplay(PDISPLAYINFO)` (1): maps the framebuffer (on a plain
  firmware framebuffer a RAM back buffer) into the calling process (same
  linear address in every process, in the shared arena) and returns its
  address, width, height, stride and pixel format.
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

- `Rs2UpdateDisplay(pul, c)` (20): PM drew in `c` rectangles (left,
  top, right, bottom in framebuffer pixels, rows from the top); see
  [The screen](#the-screen-damage-and-display-modes).
- `Rs2QueryDisplay(PDISPLAYINFO)` (21): the display as `Rs2MapDisplay`
  describes it, plus a sixth ULONG, the number of mode changes so far.
- `Rs2QueryDisplayModes(pul, cMax, pcModes)` (22): the resolutions the
  driver can switch to, as (width, height) pairs.
- `Rs2SetDisplayMode(cx, cy)` (23): switch the resolution under PM
  ((0, 0): the display's preferred size); ERROR_NOT_SUPPORTED without a
  driver that can.
- `Rs2QueryProcesses(pBuf, cb)` (24), `Rs2QueryThreadReport(pBuf, cb)`
  (25), `Rs2KillProcess(pid)` (26): for the rescue screen (see
  [Programs that stop responding](#programs-that-stop-responding)).

RS2API ordinals: 1-7 and 20-26 PM (above), 10-16 windowed VIO sessions
(see [Windowed VIO sessions](#windowed-vio-sessions-os2-window)), 100
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

## The screen: damage and display modes

PM processes draw into memory the kernel maps for them
(`Rs2MapDisplay`): a back buffer in RAM, sized for the largest mode the
display driver offers (VMware SVGA II, virtio-gpu) or for the screen (a
plain firmware framebuffer); when that is more than a quarter of the
free memory, for the current mode (then PM can only switch to modes that
fit in it). Without the memory for one they draw straight into
the framebuffer: VMware SVGA II's VRAM or virtio-gpu's guest-RAM
backing.

**Damage.** The screen surface (gpi's `Surface`) notes every pixel it
writes in a damage list in the shared state (`gpi::Damage`: at most 16
rectangles; a new one joins the one it adds least area to when that is
little or the list is full). When a thread lets go of the PM lock,
PMWIN hands the list to the kernel (`Rs2UpdateDisplay`) unless it is
drawing into the shadow buffer of a background PM session. The kernel
copies those areas from the back buffer to the framebuffer then, while
the thread still holds the lock, so the screen only shows what PM
finished drawing (VMware shows what is written to its framebuffer as it
happens: drawing there directly, every window flickered as it was
erased and painted). While drawing is going on PMWIN holds the list
back, for at most 30 ms: while windows still have to paint (a shown
window with an update region whose thread is not hung, or a
WinBeginPaint not yet ended), and while a thread that drew has not
waited since (in WinGetMsg, WinWaitEventSem and the like). A window
moved, opened or brought to the top appears with everything under and
in it painted, and what a program draws in steps appears when it is
done (Klondike, picking up a card, hides the card's window, lets the
table paint over it and draws the card it drags in its place: the table
showed through for a moment). When PM gets the screen back from a text session the
kernel copies all of the back buffer. The kernel also adds the list to
its own (`display::damage`, also fed with the rows the text screens
draw), and about 60 times a second the driver sends only those areas to
the display: an UPDATE per rectangle on SVGA II, a TRANSFER_TO_HOST_2D
and a RESOURCE_FLUSH on virtio-gpu. Measured in QEMU at 1024x768 with
`dispstat` on the kernel command line (pixels sent per second, logged
every 5 s): an idle desktop went from 24.5 million (the whole screen 31
times a second) to 0; dragging a folder window around sends 1-3
million.

**Changing the resolution.** `Rs2SetDisplayMode` switches the driver
(virtio-gpu makes a new resource on the same backing; SVGA II programs
the mode in the same VRAM) and queues `EV_MODE` (6). The server's
input thread takes the new geometry (`Rs2QueryDisplay`; it also
compares the mode generation on every pass, in case the event was
missed), points the surface at it, sizes the desktop window, gives the
pinned desktop window (the shell's) and windows that covered the old
screen the new one, re-maximizes maximized frames, brings frames that
ended up off the screen back, posts `WM_SIZE` to the desktop window and
repaints everything; the pointer is kept on the screen. When the host
changes a virtio-gpu display (`VIRTIO_GPU_EVENT_DISPLAY`) the kernel
queues `EV_HOSTSIZE` (7) and the server asks for the display's new
preferred size.

**Display settings.** `C:\OS2\DISPLAY.EXE` lists the modes
(`Rs2QueryDisplayModes`); Apply switches, and the new mode stays only if
Keep is pressed within 15 seconds (Revert, closing the window or the
time running out bring the old one back). A kept mode is saved as
`VIDEO=WxH` in the boot drive's CONFIG.SYS; the kernel applies it after
reading CONFIG.SYS at boot (it wins over the loader's `video=`, which
picks among the firmware's modes). `DISPLAY WxH` switches and saves
without asking. A plain firmware framebuffer cannot change modes.

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

**The rescue screen (Ctrl+Alt+W).** For when the window list cannot help
(the desktop itself stuck, or PM's lock held so nothing in PM moves), in
the spirit of OS/2's WatchCat. `OS2\RESCUE.EXE` (userland/rescue), started
detached by `RUN=` in CONFIG.SYS, waits on the event semaphore
`\SEM32\RS2\RESCUE`; the kernel posts it when Ctrl+Alt+W is pressed,
before the key would go to PM. It shows a text pop-up (VioPopUp) over
whatever is on the screen:

- the processes (`Rs2QueryProcesses`: pid, parent, threads, P for PM
  programs, L for the holder of `\SEM32\PMWIN\LOCK`); a process that
  keeps PM's lock all through half a second is named at the top, since
  everything in PM waits for it;
- E ends the chosen process after Y (`Rs2KillProcess`: any process, not
  only a descendant). Ending PMSHELL restarts the desktop (the kernel
  starts PROTSHELL again); ending the lock's holder frees PM (the next
  thread gets ERROR_SEM_OWNER_DIED and carries on);
- S appends a report to `RS2HANG.TXT` on the boot drive: the processes
  and every thread's state and kernel stack (`Rs2QueryThreadReport`, what
  Ctrl+Alt+F12 writes on the serial port), then PM's own view: RESCUE
  posts `\SEM32\RS2\PMSTATE`, the PM server's input thread (it takes
  PM's lock at least every 100 ms) writes RS2PM.TXT and posts
  `\SEM32\RS2\PMSTATED` (PMWIN's `report.rs`): the capture, focus and
  active windows, each message queue (waiting for messages, waiting for
  the reply to a message it sent and to which window, in a modal loop
  such as `WinProcessDlg` or `WinTrackRect`, tracking a menu, not
  responding) and the top-level windows in z-order (class, title,
  position, visible, disabled, minimized, needing paint, queue, owner).
  No answer within 3 seconds is reported too: a thread keeps PM's lock;
- F5 refreshes, Esc ends the pop-up and PM repaints.

## The switch list

The switch list (`WinAddSwitchEntry`, `WinChangeSwitchEntry`,
`WinQuerySwitchEntry`, `WinRemoveSwitchEntry`, `WinQuerySwitchHandle`,
`WinQuerySwitchList`, `WinSwitchToProgram`) is PM state kept by PMWIN
(rs2-private ordinals 9010-9016); PMSHAPI's entry points call them
through the process's PMWIN (PMSHAPI does not import it: text-mode
programs load PMSHAPI too). Entries are checked as IBM's PMWIN does
(visibility flags, the caller's process when idProcess is 0, titles cut
at 60 characters); a frame created with `FCF_TASKLIST` gets an entry
whose title follows the frame's until the program sets one (an entry
the program adds for the frame replaces it). Entries go when their
process ends, a frame's own entry with the frame. The window list
(Ctrl+Esc) shows frames by their entry's title, leaves out
`SWL_INVISIBLE` entries and also lists entries without a frame of their
own.

## PMMERGE ordinals

On OS/2 Warp 4, PMWIN, PMGPI and PMSHAPI forward most entry points to
PMMERGE.DLL, and some programs (OS/2's Klondike and Chess) import
PMMERGE's ordinals directly. rs2's PMMERGE exports, for every one of
those that rs2 implements, IBM's PMMERGE ordinal as an entry that jumps
to rs2's PMWIN or PMSHAPI export: `userland/pmmerge/ibm-forwards.txt`
(IBM's map, from crates/os2-exe's `lximage` "forward" lines) and
`tools/scripts/gen_pmmerge_forwards.py`, which `gen_pm_exports.py` runs
after regenerating PMWIN's or PMSHAPI's exports.

## 16-bit PM programs (the OS/2 1.x API)

16-bit PM programs (OS/2 1.x and early 2.x ones such as Seek and Scan and
PMCHKDSK: 16-bit code, NE or LX) import PMWIN, PMGPI, PMSHAPI and HELPMGR
ordinals below 700 (and some PMMERGE ones): `_pascal far` functions
whose integers are 16-bit where the 32-bit API has 32-bit ones, whose
pointers are 16:16, and whose structures (SWP, QMSG, CREATESTRUCT,
WNDPARAMS, OWNERITEM, TRACKINFO, CLASSINFO, SWCNTRL, HELPINIT ...) have
16-bit fields. IBM implements them in PMMERGE as 16-bit stubs into a
common thunk; rs2's are:

- **Entries.** PMWIN has them all (`src/win16.rs`): PMWIN ordinals as
  IBM's, the 16-bit GPI at 4000 + the PMGPI ordinal (as the 32-bit GPI).
  PMGPI and PMMERGE export LX forwarders to them (elf2lx writes
  `NAME=MODULE.ordinal` export lines as forwarder entries). PMSHAPI
  (`shapi16.rs`: Prf16*, the Win16 profile calls on HINI_PROFILE, the
  switch list) and HELPMGR (`help16.rs`) have their own. The 16-bit
  entries are exported under IBM's upper-case names (`WINCREATEWINDOW`,
  the 32-bit one is `WinCreateWindow`): 16-bit programs look names up so
  (Seek and Scan finds `WINCREATEHELPINSTANCE` with DosGetProcAddr) and
  the loader matches case-sensitively first.
- **Argument types.** `userland/pmwin/ibm-16bit.txt` lists every 16-bit
  entry's arguments, read from the descriptor IBM's 16-bit stubs pass to
  their thunk in EDX (low nibble the count, then 2 bits per argument from
  the last: 32-bit, far pointer, unsigned or signed 16-bit).
  `tools/scripts/gen_win16.py` makes `src/win16_table.rs` from it: the
  ~350 entries whose arguments only need widening and flat pointers call
  the 32-bit export through one table-driven thunk; about 40 have
  handlers (messages, QMSG, SWP, procedures, clipboard selectors, hooks,
  dialogs). Run it before `gen_pm_exports.py` (which appends each DLL's
  `exports.extra`).
- **Thunks** (`userland/thunk16`). `entries16_pascal!` makes the 16-bit
  stubs (`mov ax, N; call common; retf SIZE`) and the common code: it
  switches to a 32-bit stack of the thread, one per nesting depth (a
  16-bit window procedure called from PMWIN may call PMWIN again), and
  records the caller's SS:SP and DS in spare TIB fields (`fs:[0x30]`,
  `fs:[0x38]`; rs2's TIB blocks are 0x40 apart and use 0x30), which every
  DLL's thunks share. `call16` calls 16-bit code from 32-bit code below
  that SS:SP, with that DS (a thread that never ran 16-bit code gets a
  16-bit stack of its own).
- **Window procedures and hooks** (`src/cb16.rs`). Inside PMWIN every
  procedure is a 32-bit address: a 16-bit procedure becomes one of 256
  32-bit stubs (per process) that converts the message and calls it on
  the 16-bit stack; a 32-bit procedure handed to 16-bit code (the old
  procedure from WinSubclassWindow, QWP_PFNWP, CLASSINFO) one of 128
  16-bit stubs that calls it. Each turns back into the original when it
  goes the other way, so subclassing and unsubclassing round-trip. `msg::call`
  runs the 32-bit stubs as application code (without the PM lock). Hooks
  are called the same way (see [Hooks](#hooks)); their structures are
  passed as they are, except the help hook's.
- **Messages.** As IBM's PMMERGE (a table by message number), the 32-bit
  form is the canonical one: messages from 16-bit code (WinSendMsg,
  WinPostMsg, WinDefWindowProc, WinDispatchMsg ...) are converted when
  they come in and converted again for a 16-bit procedure: plain pointers
  (list box and menu item texts, MLE buffers, RECTLs), and copies of SWP,
  CREATESTRUCT, WNDPARAMS, OWNERITEM, TRACKINFO and QMSG structures,
  copied back after the call. WM_INITDLG and WM_CONTROL parameters and
  application messages pass as they are.

**Not done.** The 16-bit heap (WinCreateHeap, WinAllocMem ...),
WinCatch/WinThrow, DDE, WinCallMsgFilter, the 16-bit program list calls,
16-bit GPI calls whose structures differ (bitmap headers are passed as
they are), posted messages carrying converted structures, the class
"thunk procedures" (WinSetClassThunkProc). WIN16BUILDPTRHANDLE (PMWIN
310, which PMSHLTKT imports) returns 0. CTLTEST checks the entries, a
16-bit window procedure and the conversions (`userland/ctltest/src/pm16.rs`).

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
  notifications and application messages (`WM_USER` and up; IBM's frame
  passes everything it does not process) to its client. It lays out its
  controls as IBM's does, by asking its window procedure:
  `WM_QUERYFRAMECTLCOUNT`, then `WM_FORMATFRAME` with an SWP array (the
  client last) that it applies with the SWPs' `SWP_SHOW`/`SWP_HIDE`, so
  a subclassed frame adds controls of its own (OS/2's Icon Editor puts
  its status line and palette there). `WM_CALCFRAMERECT`
  (`WinCalcFrameRect`) converts between frame and client rectangles. A
  frame created with `FCF_SHELLPOSITION` stays without a size (its
  client gets no `WM_SIZE`) until it is first shown, as on OS/2;
  `FCF_TASKLIST` gives it a switch entry (see [The switch
  list](#the-switch-list)). As in IBM's PMWIN, the frame's window
  procedure makes a window a frame when it gets `WM_CREATE` with the
  FRAMECDATA (`WinCreateStdWindow` and dialog frames pass one), so a
  class an application registers with WC_FRAME's procedure (from
  `WinQueryClassInfo`) and creates with `WinCreateWindow` gets its frame
  controls too (EPM's edit frames); a child with id `FID_CLIENT` is the
  client however it was made. The built-in classes have IBM's window
  words (the frame 0xAC bytes, the controls 8); frames answer
  `QWS_XRESTORE`..`QWS_CYRESTORE` and `QWS_XMINIMIZE`/`QWS_YMINIMIZE`.
- The text cursor (`WinCreateCursor`, `WinShowCursor`,
  `WinDestroyCursor`, `WinQueryCursorInfo`), with IBM's rules
  (`crates/pmwin/src/cursor.rs`): one in the system, created hidden
  (one `WinShowCursor(TRUE)` shows it), drawn by inverting its rectangle
  in the window's visible region (solid, `CURSOR_FRAME` one border wide,
  `CURSOR_HALFTONE` every other pixel; the cx or cy 0 means the border
  width), `CURSOR_SETPOS` moves it, `CURSOR_FLASH` blinks it at
  `SV_CURSORRATE` through `WM_TIMER` `TID_CURSOR` (0xFFFF), which
  `WinDefWindowProc` handles; `WinBeginPaint` hides it while its window
  paints and `WinEndPaint` shows it again; it goes with its window.
- DDE: see [Dynamic data exchange](#dynamic-data-exchange-dde).
- `WinStoreWindowPos`/`WinRestoreWindowPos` (PMWP.DLL) keep a window's
  position, size, minimized/maximized state, restore and minimize
  positions and sizing border in OS2.INI in the Workplace Shell's record
  (`crates/pmwin/src/winpos.rs`), scaled when the screen size changed;
  presentation parameters are not stored, and those of a record OS/2
  wrote are skipped.
- PMSPL.DLL is the spooler API of a system without printers: queue,
  device, job, driver and port lists are empty, named ones are "not
  found" (`NERR_QNotFound` etc.), `SplQmOpen` fails. Programs that look
  for printers at start (EPM, the system log viewer, MarkVision, the
  printer utilities) load and say there are none.
- Metafiles and retained segments: see [Metafiles and
  segments](#metafiles-and-segments).
- `WinSetClipbrdViewer` sends the new viewer `WM_DRAWCLIPBOARD` at once
  (mp1 the clipboard owner), as `WinCloseClipbrd` does after a change.
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
  `WinDefDlgProc`, as the System Editor does). A template whose window
  is not a frame creates that class (PMREXXIO's input line is an entry
  field), subclassed by the dialog procedure.
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
  `WinIntersectRect`, ...), `WinUpper` and the code page calls (per
  queue: `WinSetCp`), hooks (see [Hooks](#hooks)), owner drawing (see
  [Owner drawing](#owner-drawing)), `WinFlashWindow`,
  `WinGetMaxPosition`/`WinGetMinPosition`, `WinQueryObjectWindow`.

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
- Text: the system font and the faces of [Fonts](#fonts); outline
  faces (Helvetica, Times New Roman, Courier, their bold and italic)
  are rendered at the character box's size (the box is the em square;
  other proportions, shear and paths scale the glyphs instead, also as
  figures in paths: clip paths from text); the image faces (Helv,
  Tms Rmn, System Proportional, WarpSans, System Monospaced) at
  lMaxBaselineExt from the same outlines, or IBM's bitmaps where
  installed; a face's own bold and italic are used, others simulated.
  `GpiLoadFonts` loads the image
  fonts of a font file (a resource DLL: `RT_FONTDIR`, `RT_FONT` in
  OS/2's format) for the process; `GpiQueryFonts` offers them with
  `QF_PRIVATE`, `GpiCreateLogFont` picks one by lMatch or by face and
  size, and they draw with their own proportional glyphs (Klondike's
  CARDSYM.FON). Font metrics, text boxes, character positions,
  `GpiCharStringPos` with increments, opaque and clipped rectangles.
- `hdc | 1` used as a presentation space draws on that DC itself (as in
  IBM's GPI, which hands such handles to the engine; DC handles are
  even); `DevQueryCaps` also takes a presentation space. `WinFillRect`,
  `WinDrawText`, `WinDrawBitmap`, `WinDrawPointer` and `WinDrawBorder`
  draw on a memory PS's bitmap. `CLR_TRUE`/`CLR_FALSE` are white and
  black.
- `WinDrawText`: one line, `DT_QUERYEXTENT` (measures only),
  `DT_WORDBREAK` (the words that fit; returns the characters processed,
  for callers that loop), `DT_MNEMONIC`, `DT_HALFTONE`, `DT_ERASERECT`,
  `DT_TEXTATTRS`. `WinDrawBorder` (the four bands as IBM's engine
  splits them, `DB_INTERIOR`, `DB_AREAATTRS`, `DB_STANDARD`,
  `DB_DLGBORDER`, Warp 4's raised/depressed/trough/fence/field
  borders), `WinGetSysBitmap` (the `SBMP_*` images drawn as rs2's
  controls look), `WinGetKeyState` (virtual keys as the input thread saw
  them), `WinBeginEnumWindows`/`WinGetNextWindow`/`WinEndEnumWindows`,
  `WinQueryClassName`. Timers run at most at the 31 ms clock rate.

Not done: printing (no printer DCs), rotated or sheared text, real
outline fonts, line ends, joins and mitre limits, `GpiPolygons` and
`GpiCreatePolygonRegion`.

### Metafiles and segments

GPI calls can be kept as GOCA drawing orders (the format of the
Toolkit's pmord.h): in retained segments and in metafiles.
`crates/gpi` has the order codec (`goca.rs`: the 1-byte, 2-byte, long
and very long orders, coordinates in the short or long format, laid out
as IBM's PMGPI builds them) and the metafile format (`metafile.rs`),
both tested on the host with the OS/2 metafiles on the CD
(`graphic.met`, `prtsamp.met`, `AAAAA.MET`) when `work/` has them.
PMWIN's `gpiseg.rs` does the rest:

- Recording: each drawing and attribute call records its order (an
  attribute set in `AM_PRESERVE` mode becomes "push and set", so
  `GpiPop` plays back) into the open segment of a PS in `DM_RETAIN` or
  `DM_DRAWANDRETAIN` mode (`GpiSetDrawingMode`, `GpiOpenSegment`,
  `GpiCloseSegment`; `DM_RETAIN` does not draw), and onto the metafile
  of a PS associated with a metafile DC (`DevOpenDC` `OD_METAFILE` or
  `OD_METAFILE_NOQUERY`). Calls that are not drawing orders go to a
  metafile as IBM's escape orders (0xFE 0xD5 with an identifier, the
  data as IBM's `PlayEscape` reads it): `GpiCreateLogFont`,
  `GpiCreateLogColorTable`, `GpiDeleteSetId`, `GpiSetCp`,
  `GpiSetDefaultViewMatrix`, `GpiErase`, `GpiIntersectClipRectangle`,
  `GpiExcludeClipRectangle`, `GpiSetDefAttrs`, `GpiSetDefArcParams`,
  `GpiSetDefViewingLimits`. `GpiSetAttrs` records a bundle element
  (type `0xFD00 | primitive`) of individual attribute orders.
- Segments: `GpiDrawChain`, `GpiDrawSegment`, `GpiDrawFrom` (each segment
  starts from the default attributes, current position (0,0) and its
  segment transform), segment attributes (`GpiSetSegmentAttrs`,
  `GpiSetInitialSegmentAttrs`: chained, visible, ...), names, deleting,
  `GpiGetData` (whole orders from an offset, long format) and
  `GpiPutData` (orders executed as GPI calls). The call segment order
  draws another segment. A retained segment drawn on a metafile PS
  becomes a segment of the metafile (OS/2's PMPIC.DLL builds metafiles
  so). Correlation, pick, editing and dynamic segments are not done.
- Metafiles: `DevCloseDC` of a metafile DC builds the metafile and
  returns its handle. A metafile handle stands for the bytes of a .MET
  file (`GpiQueryMetaFileLength`/`Bits`, `GpiSetMetaFileBits`, which
  can grow it), loaded and saved as is (`GpiLoadMetaFile` checks that
  it starts with Begin Document; `GpiSaveMetaFile` deletes the handle,
  as IBM's does); `GpiCopyMetaFile`, `GpiDeleteMetaFile`. Metafiles
  live in the shared PM state: one put on the clipboard as
  `CF_METAFILE` becomes the system's (see [The
  clipboard](#the-clipboard)).
- The metafile layout is IBM's: Begin Document (a description triplet),
  Begin Resource Group, the colour attribute table (entries or RGB
  mode), Begin Graphics Object, its object environment group (Map
  Colour Attribute Table, a Map Coded Font per logical font: name,
  lcid, font descriptor, code page, face), the Graphics Data Descriptor
  (picture descriptor with the page units and size, the coordinate
  format, the default viewing transform) and the orders in Graphics
  Data fields of at most 32 KB split at order boundaries, each segment
  opened by a Begin Segment order.
- `GpiPlayMetaFile` turns the orders back into GPI calls, so playing
  draws, records into another metafile or into retained segments
  (`DM_RETAIN`: the metafile's segments are kept, named from
  `PMF_SEGBASE`) and collects boundary data like the calls themselves.
  Options as IBM's: `RES_RESET` resets the PS to the metafile's page
  units and size, `LT_ORIGINALVIEW` takes its default viewing
  transform, `LC_LOADDISC` creates its logical fonts, `CTAB_REPLACE`
  its colour table, `SUP_SUPPRESS` draws nothing; the description is
  returned. Each segment starts with the default attributes. Images
  (`GpiImage`, 1 bpp) play; bitmaps (`GpiBitBlt` into a metafile, the
  bitmap resources of IBM's metafiles) are not recorded or played.
- Also there now: `GpiSetViewingTransformMatrix` (a transform between
  the model and default viewing transforms), `GpiSetDefAttrs` and
  `GpiQueryDefAttrs` (what a reset gives), `GpiSetDefViewingLimits`
  (kept, not applied), `GpiSetSegmentTransformMatrix`,
  `WinCpTranslateString`/`Char` (rs2's code page tables) and PMMERGE's
  `AAB32GetReg`/`AAB32SetReg` (ordinals 5000/5001, rs2's names: a
  per-thread block in which IBM's DLLs note an error and its severity,
  and the error `WinGetLastError` returns).

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
  The font is the monospaced font's glyphs (Liberation Mono) drawn at
  its own size, or scaled to the FATTRS cell (`lMaxBaselineExt`, half
  as wide for outline faces), so every face is monospaced. One level of undo (a typing run is one step; undoing
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
  `CN_EXPANDTREE`/`CN_COLLAPSETREE`, `CN_HELP` and the editing ones.
  Direct manipulation as IBM's container does it: `DM_DRAGOVER` becomes
  `CN_DRAGOVER` to the owner (CNRDRAGINFO: the DRAGINFO and the record
  under the pointer, or none over empty space; the owner's answer is
  the container's), with target emphasis on that record meanwhile;
  `DM_DRAGLEAVE`, `DM_DROP` (the record at the drop position) and
  `DM_DROPHELP` become `CN_DRAGLEAVE`, `CN_DROP` and `CN_DROPHELP`.
  `CA_OWNERDRAW`: after drawing a record's text (icon, name, text and
  tree views) the container sends the owner `WM_DRAWITEM` (OWNERITEM
  with `idItem` `CMA_TEXT`, hItem a CNRDRAWITEMINFO), which may paint
  over it. Not done: owner draw of icons and detail cells
  (`CRA_OWNERDRAW`), bitmaps instead of icons (`CA_DRAWBITMAP`),
  separately scrolled split windows, the grid (`CM_SETGRIDINFO` ...),
  `CM_MOVETREE`, `CN_DRAGAFTER` (ordered views), emphasis of the whole
  container as a target, lazy drag (`CN_PICKUP`), rubber-band
  selection.

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
presentation parameters (see [Presentation
parameters](#presentation-parameters)).

**System values and colours.** `WinSetSysValue` sets the values a user
changes: SV_SWAPBUTTON (the right button becomes button 1), SV_DBLCLKTIME
(how close two clicks must be to make a double-click), SV_ALARM (whether
`WinAlarm` sounds), SV_CURSORRATE, the scroll rates and the beeps'
frequencies and durations. `WinSetSysColors` (LCOLF_INDRGB or
LCOLF_CONSECRGB, LCOL_RESET) sets system colours; every window is
redrawn, and the controls (buttons, entry fields, list boxes, menus,
notebooks, containers and the rest) draw with the system colours
(SYSCLR_DIALOGBACKGROUND, BUTTONLIGHT, BUTTONDARK, WINDOW, WINDOWTEXT,
HILITEBACKGROUND, HILITEFOREGROUND). Both live in the shared PM state;
the Workplace Shell's Mouse, Keyboard, Sound and Scheme Palette objects
set them and keep them in OS2.INI (docs/WPS.md).

## Presentation parameters

Presentation parameters are kept as IBM's PMWIN keeps them
(`userland/pmwin/src/pp.rs`; PMMERGE's `GetpPresParam`,
`QueryPresParam`, `XSetPresParam`, `XNotifyPresParamChanged`): each window
has a list of parameters of any id (the `PP_*` colours and fonts,
`PP_USER` and up for applications), from `WinSetPresParam`,
`WinCreateWindow`'s PRESPARAMS and the PRESPARAMS of dialog template
items. A window without a parameter takes its owner's, up the owner chain
(`QPF_NOINHERIT` asks the window alone); `WinQueryPresParam` returns the
first of its two ids found, with `QPF_ID1COLORINDEX`/`QPF_ID2COLORINDEX`
turning a colour index into RGB. An RGB colour and its index form
(`PP_FOREGROUNDCOLOR`/`PP_FOREGROUNDCOLORINDEX` ...) replace each other.
Setting or removing a parameter sends `WM_PRESPARAMCHANGED` to the window
and to the windows that take it from there (owned windows without their
own) and repaints them. `PP_FONTNAMESIZE` ("10.Helv", "12.Times New
Roman.Bold") is the font the window and those below it paint and measure
their text in (`draw::font_of`).

The built-in controls draw in the colours IBM's controls ask for (their
`*QueryPresParams` functions, `CtlQueryColor`): an RGB parameter, its
index form, or the system colour.

| Control | Parameters |
|---------|------------|
| Static texts, group boxes, rectangles | `PP_FOREGROUNDCOLOR`, `PP_BACKGROUNDCOLOR`, `PP_DISABLEDFOREGROUNDCOLOR` |
| Buttons | `PP_FOREGROUNDCOLOR`, `PP_BACKGROUNDCOLOR`, `PP_DISABLEDFOREGROUNDCOLOR`, `PP_DISABLEDBACKGROUNDCOLOR`, `PP_BORDERLIGHTCOLOR`, `PP_BORDERDARKCOLOR`, `PP_BORDERDEFAULTCOLOR` |
| Entry fields, list boxes, combination boxes, spin buttons | `PP_FOREGROUNDCOLOR`, `PP_BACKGROUNDCOLOR`, the disabled pair, `PP_HILITEFOREGROUNDCOLOR`, `PP_HILITEBACKGROUNDCOLOR` (the selection) |
| MLEs | `PP_FOREGROUNDCOLOR`, `PP_BACKGROUNDCOLOR` (unless `MLM_SETTEXTCOLOR`/`MLM_SETBACKCOLOR` set others), `PP_DISABLEDFOREGROUNDCOLOR` |
| Menus | `PP_MENUFOREGROUNDCOLOR`, `PP_MENUBACKGROUNDCOLOR`, `PP_MENUHILITEFGNDCOLOR`, `PP_MENUHILITEBGNDCOLOR`, `PP_MENUDISABLEDFGNDCOLOR` |
| Title bars (a frame's) | `PP_ACTIVECOLOR`, `PP_ACTIVETEXTFGNDCOLOR`, `PP_INACTIVECOLOR`, `PP_INACTIVETEXTFGNDCOLOR` |
| Notebooks | `PP_BACKGROUNDCOLOR`, `PP_PAGEBACKGROUNDCOLOR`, `PP_MAJORTAB*`, `PP_MINORTAB*` (`BKM_SETNOTEBOOKCOLORS` first) |
| Containers | `PP_FOREGROUNDCOLOR`, `PP_BACKGROUNDCOLOR` |
| Value sets, sliders | `PP_BACKGROUNDCOLOR` |

Not done: the frame border's colours, scroll bars, the container's other
colours, the BIDI parameters (`PP_BIDI_*`), `PP_FONTHANDLE`.

## Bidirectional languages (PMBIDI)

OS/2's Arabic and Hebrew support is PMBIDI.DLL (calls for the
bidirectional attributes of processes, windows and presentation spaces,
the keyboard's national layer, and X/Open's layout objects, done by
BDCALL32 in the bidirectional editions) and PMMERGE's language calls
(`WinSetLangInfo`, `WinQueryLangInfo`, `WinSetKbdLayer`,
`WinQueryKbdLayer`, `WinSetLangViewer`, `WinQueryLangViewer`, ordinals
3500-3505; PMBIDI's Win32 entries forward there). rs2's code pages are not
bidirectional (`WinQueryCpType` says so), so rs2's PMBIDI
(`userland/pmbidi`) transforms text into the same text, as IBM's does for
text without bidirectional characters (`layout_object_transform` and
`layout_object_editshape` copy, with identity index maps and level 0;
`setvalues`/`getvalues` keep the values), and the attribute calls keep
what they are given: per window (`LI_BD_WND_ATTR`, `LI_BD_CLIP_ATTR`; the
window is asked first with IBM's messages 0xBD0-0xBD3), per process, per
presentation space (`Gpi32SetBidiAttr`, which the WarpCenter calls). The
keyboard has no national layer; `WinSetKbdLayer` keeps the layer and
tells the language viewer. THAILIB.DLL (`userland/thailib`) has the two
Thai keyboard layer calls BDVIEWER uses. PMXTEST checks them; TEXTCON1
runs on them.

## Hooks

`WinSetHook`/`WinReleaseHook` and the calls of the hooks follow IBM's
PMWIN (studied in PMMERGE: `WSetHook`, `GetHookToCall` and the
`Call*Hook` functions; `userland/pmwin/src/hooks.rs`). Each kind has one
chain: the hooks of message queues (newest first), then the system hooks
(hmq NULLHANDLE, newest first), which need the module that holds the hook
procedure. A hook is called on the thread whose work it hooks, so a
system hook runs in every PM process: its module is loaded into a process
the first time one of its hooks is called there (DLL code has the same
address everywhere). The chain is read again after each hook (a hook may
set or release hooks). Hooks run without the PM lock; the PM server calls
none but the journal playback hook. Where they are called:

| Hook | Called |
|------|--------|
| `HK_SENDMSG` | in the sender, before a sent message goes anywhere (SMHSTRUCT, `fInterTask`); it may change the message |
| `HK_INPUT` | for every message `WinGetMsg`/`WinPeekMsg` retrieves (posted, input, timer, paint), with `PM_REMOVE` or `PM_NOREMOVE`; TRUE takes the message |
| `HK_MSGFILTER` | `WinCallMsgFilter`, and the modal loops: `WinProcessDlg` (`MSGF_DIALOGBOX`), `WinMessageBox` (`MSGF_MESSAGEBOX`), `WinTrackRect` (`MSGF_TRACK`); TRUE: the message is not dispatched |
| `HK_JOURNALRECORD` | as a keyboard or mouse message (the system queue's: mouse moves, buttons, `WM_CHAR`) is taken off a queue; played-back input is not recorded |
| `HK_JOURNALPLAYBACK` | by the PM server on a thread of its own (IBM's `GetAndProcessJrnMsg`): the hook gives the next message and how long to wait, the message is processed as hardware input (keys to the focus, mouse messages at their position), then the hook skips to the next; hardware input waits meanwhile |
| `HK_HELP` | `WM_HELP` at a frame, F1 in a menu, PMWIN 288 (see [The help manager](#the-help-manager-helpmgrdll-and-viewexe)) |
| `HK_LOADER` | `WinLoadLibrary`, `WinDeleteLibrary`, `WinLoadProcedure`, `WinDeleteProcedure` (`LHK_*`); TRUE: the hook's result |
| `HK_REGISTERUSERMSG` | `WinRegisterUserMsg`, `WinRegisterUserDatatype` |
| `HK_FINDWORD` | `WinDrawText` with `DT_WORDBREAK`: where the word that does not fit begins (IBM's `CchLineBreak`) |
| `HK_CODEPAGECHANGED` | `WinSetCp` changing a queue's code page |
| `HK_WINDOWDC` | `WinGetPS` and `WinReleasePS` |
| `HK_DESTROYWINDOW` | each window `WinDestroyWindow` destroys, before its `WM_DESTROY` |
| `HK_CHECKMSGFILTER` | which queued messages a filter takes (in place of the message range) |
| `HK_MSGINPUT` | before the queue: a source of input messages (pen) |

`HK_MSGCONTROL`, the program list hooks, `HK_LOCKUP` and `HK_FLUSHBUF`
are recorded only (IBM's PMWIN does not call the first; rs2 has no
lockup or program list hooks to call). `RS2\PMXTEST.EXE` checks them all,
the system hooks with `PMXHOOK.DLL` in a second process and in the PM
server.

## Dynamic data exchange (DDE)

`WinDdeInitiate` sends `WM_DDE_INITIATE` to every top-level frame but
the client's (IBM's `BuildPwndList` with only frames), each with a
DDEINIT of its own; frames pass it, and the other DDE messages, to their
client (IBM's frame procedure passes messages 0x7B-0xBA but
`WM_QUERYCONVERTPOS`). `WinDdeRespond` sends `WM_DDE_INITIATEACK`,
`WinDdePostMsg` posts a DDESTRUCT. The DDEINIT and DDESTRUCT blocks are
giveable shared memory given to the receiving process
(`DosGiveSharedMem`); the sender's access is dropped unless
`DDEPM_NOFREE`, and the receiver frees the block or sends it on.
rs2's queues do not fill up, so `DDEPM_RETRY` never waits. PMXTEST holds
a conversation with a server in another process (`PMXTEST DDESERVER`):
initiate, request, poke, execute, terminate. PMDDEML (the DDE management
library) is not there: no program on the OS/2 CD uses it but IBM's
PMWINX, which rs2 replaces.

## Owner drawing

The controls ask their owner to draw as IBM's do (PMMERGE's
`LboxDrawItems`, `MenuDrawMiOwner`, `ButtonPaint`; PMCTLS's
`VSSendOwnerdraw`, `DrawSliderArm`, `DrawTab`): `WM_DRAWITEM` (mp1 the
control's id, mp2 an OWNERITEM: hwnd, hps, fsState, fsAttribute,
fsStateOld, fsAttributeOld, rclItem, idItem, hItem) on a presentation
space clipped to the item and in RGB mode; what the owner does not draw
(it returns FALSE) is drawn as usual.

- List boxes (`LS_OWNERDRAW`): `WM_MEASUREITEM` when the list box is
  made gives the items' height; each item on view is the owner's
  (fsState and fsStateOld the selection, idItem the index, hItem the item
  handle); an item the owner draws but leaves either state set in is
  inverted.
- Menus (`MIS_OWNERDRAW`): `WM_MEASUREITEM` (mp2 an OWNERITEM; the owner
  sets rclItem's xRight and yTop to the item's size) before the menu is
  laid out, `WM_DRAWITEM` with the item's style as fsState and its
  attributes (`MIA_HILITED` while highlighted) as fsAttribute; an item
  left `MIA_HILITED` is inverted. The notifications go up the chain of
  menus to the first owner that is not a menu.
- Buttons (`BS_USERBUTTON`): `WM_CONTROL` (id, `BN_PAINT`) with a
  USERBUTTON (hwnd, hps, fsState, fsStateOld: `BDS_HILITED`,
  `BDS_DISABLED`, `BDS_DEFAULT`); the button draws nothing itself.
- Value sets: `VS_OWNERDRAW` gives the owner the background
  (`VDA_BACKGROUND`), `VIA_OWNERDRAW` items their cell (`VDA_ITEM`,
  hItem the row and column).
- Sliders (`SLS_OWNERDRAW`): the background, the shaft, the ribbon strip
  and the arm (`SDA_*`); the arm's rclItem is in the slider's
  coordinates (IBM draws the arm into a bitmap of its own).
- Notebooks: a tab with neither text nor a bitmap (idItem `BKA_TAB`,
  hItem the page ID, fsAttribute `BKA_MAJOR` or `BKA_MINOR`).
- Containers (`CA_OWNERDRAW`): the records' text (see [Standard
  controls](#standard-controls)).

## Open32 and the registry (PMWINX, REGISTRY)

IBM's Open32 (PMWINX.DLL, the Win32 API on OS/2) is installed with the
Feature Installer (the CD's `FI\DAX`); programs on the CD use only its
registry calls, which IBM's PMWINX forwards to REGISTRY.DLL (ordinals
541-555). rs2's PMWINX holds just those forwarders, and rs2's
REGISTRY.DLL has the Win32 registry calls (the A forms) at IBM's
ordinals. IBM's REGISTRY keeps its keys in `\OS2\SYSTEM\SYSTEM.DAT` and
`USER.DAT`, binary files mapped into shared memory with a server
process; rs2 keeps them in two profiles (OS/2 INI files through PMSHAPI,
shared by all processes and written through): `\OS2\SYSTEM\REGSYS.INI`
for `HKEY_LOCAL_MACHINE` (and `HKEY_DYN_DATA`) and `REGUSER.INI` for
`HKEY_USERS`. A key is a profile application named by its path ("HKLM\
SOFTWARE\IBM", upper case: names are case-insensitive) with a key `K`
holding the name as given; a value is a key `V` + its name in upper case
holding the type, the name as given and the data. The predefined keys are
Open32's handles (`HKEY_LOCAL_MACHINE` 0xFFFFFFEF ... `HKEY_CLASSES_ROOT`
0xFFFFFFEC; the Win32 values are taken too) and map as in IBM's
REGISTRY: `HKEY_CLASSES_ROOT` is `HKEY_LOCAL_MACHINE\SOFTWARE\Classes`,
`HKEY_CURRENT_USER` is `HKEY_USERS\.default`, `HKEY_CURRENT_CONFIG` is
`HKEY_LOCAL_MACHINE\CONFIG\0001`. Keys are deleted with their subkeys.
Not there: the W calls, IBM's keys that map values to INI files
(`OS2_INI_FILE_NAME`), security and notification. PMXTEST checks the calls
through PMWINX; the Registry Editor (REGEDIT2.EXE) runs on them.

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
- `CF_METAFILE` (and `CF_DSPMETAFILE`): the metafile becomes the
  system's, every process gets the same handle and can play it; it is
  deleted when the clipboard is emptied.
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

## Fonts

PM draws text anti-aliased from outline fonts, rendered by rs2's own
code in `crates/gpi`: `ttf.rs` reads TrueType (head, maxp, hhea, hmtx,
loca, glyf with composite glyphs, cmap formats 4 and 12, OS/2, name)
and holds the rasteriser (each edge's signed area accumulated per pel,
as font-rs does; a 0.8 gamma so thin stems keep their weight);
`type1.rs` reads Type 1 (.PFB: eexec, Subrs, CharStrings; the
charstring operators with seac accents, flex and hint replacement).
There is no hinting: the scale is nudged so the x-height is whole pels
and the baseline sits on a pel boundary. A face (`font::Source`) keeps
the sizes made from it (up to 32), each a `Font` whose glyphs are
coverage maps for code page 850; text blends them over what is there
(other mixes take pels at half coverage).

`userland/pmwin/src/fonts.rs` chooses the files, the first time a PM
process starts (the fonts live in the shared state, so every process
uses the same rendered glyphs):

| Use | IBM's, if installed | Otherwise (shipped with rs2) |
|-----|----------------------|------------------------------|
| System font (controls, menus, titles) | WarpSans 9 pt for 96 dpi (`OS2\DLL\DSPRES.DLL`, resource type 1000) | Liberation Sans at 13 pels |
| Monospaced (MLEs) | | Liberation Mono at 13 pels |
| Helvetica, Times New Roman, Courier (and Bold, Italic, Bold Italic) | `PSFONTS\HELV*.PFB`, `TNR*.PFB`, `COUR*.PFB` | `PSFONTS\LIBSANS*.TTF`, `LIBSERIF*`/`LIBSERF*`/`LIBSRFBI`, `LIBMONO*`/`LIBMONBI` |
| Image faces | `DSPRES.DLL`, `HELV.FON`, `TIMES.FON`, `COURIER.FON`, `COURIERI.FON`, `SYSMONO.FON`, offered to every process as if loaded with `GpiLoadFonts` | drawn from the outline faces |

The Liberation fonts are under the SIL Open Font License 1.1
(`PSFONTS\LIBERATN.TXT`). IBM's come from the OS/2 CD through APPINST
("OS/2 fonts"); rs2 never ships them. VIO windows keep the kernel's
unscii cells, and the pointer and icon glyphs unscii (`font::vio`).

A font name ("points.face[.Bold][.Italic]", `fonts::by_name`) is IBM's
bitmap of that point size for 96 dpi where installed, else the outline
face at points x 96 / 72 pels. Controls measure text in pels: entry fields (scrolling, the cursor,
selection, hit testing), wrapping (`draw::wrap`), mnemonic underlines,
container icon titles (at least 75 pels, SV_CXICONTEXTWIDTH) and
details columns. Dialog units stay 2x2 pels whatever the font.

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
`PL_ALTERED` broadcast after `PrfReset`, the 16-bit program list calls
(`Prf16AddProgram` and the like; the 16-bit profile calls are there,
see "16-bit PM programs"), `PrfQueryProgramCategory`, `WinStartApp`;
the profile names in shared memory last while some process
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
- the Minimized Window Viewer: the frames minimized meanwhile;
- the Startup folder (a program group): see below.

Besides their own objects, the desktop, groups and directories show
the shadows kept in them (see Shadows below).

**The tree view** (View > Tree view) of Drives and directory windows
is the container's tree (`CV_TREE`, mini icons, tree lines) of folders
only: Drives shows the drives, a directory shows itself, open, with
its subfolders. A record whose folder has subfolders gets a placeholder
child, so it has its + button; expanding it (`CN_EXPANDTREE`) replaces
the placeholder with the subfolders, each checked for subfolders of
its own (`MUST_HAVE_DIRECTORY` search) — so a tree reads one level
ahead, on demand. Enter or a double click opens a folder in its own
window; folders in the tree take drops.

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
(direct editing, also F2 or Alt+click), Copy..., Move..., Create
shadow... (files, folders and program objects; shadows of every kind
of object), Delete (with confirmation, also the Del key; folders with
their contents; Command Prompts, Programs and Startup cannot be
deleted); on shadows Original: Locate (opens the folder of the object,
selecting it), Copy, Move and Delete shadow; on a folder's background View, Arrange, Refresh,
Create program object, Create folder (a directory: a long name on HPFS,
`NEWFOLDR` on FAT), Open parent, Close; on the desktop Arrange,
Refresh, Create program object, Create folder (a program group), Window
list, System setup and Shut down. F5 refreshes.

**Copying and moving** (the Copy... and Move... menu items, or
dragging): the target folder dialog lists the desktop and the program
groups (for program objects and shadows) or the drives and, folder by
folder, the directories (for files; `[..]` goes up; a path can be
typed), with the new name for one object. Files and folders are copied
and moved on a worker thread (with its own message queue, posting to
the window thread): each file with `DosCopy`, so its EAs, attributes
and dates come along; directories are made one by one with their EAs
(`DosEnumAttribute`, `FIL_QUERYEASFROMLIST`, `DosSetPathInfo`) so each
file is reported; a move within a drive is `DosMove`, across drives a
copy and a delete. A progress window (shown when the job takes longer
than half a second) names the file and the target and shows files and
megabytes done of the total, with a bar; Cancel stops after the
current file. When the target has an object of that name (also when
copying into the same folder) the **name collision dialog** asks, as
the Workplace Shell's does: Rename (a free name is offered: `NAME
(2).EXT` on file systems with long names, `NAME~2.EXT` on FAT),
Replace (for a folder: the one there with everything in it), Skip or
Cancel. Errors ask whether to go on. The shell's own code for this is
`fileops.rs` (no windows; DESKTEST includes it) and `copy.rs`.
Program objects are copied (`PrfAddProgram`) or moved (added, then
removed) between the desktop and groups.

**Shadows** (Create shadow..., Ctrl+Shift+drag, or a file dropped on
the desktop or a group) stand for a file or folder, a program object,
a group, a drive, Drives, the Minimized Window Viewer, the Window List
or Shutdown; they live in OS2.INI (`RS2_Shadows`, below) by the folder
they are in and their object's path or object id. A shadow shows its
object's title and icon with the title in the shadow colour (the
container's `CA_OWNERDRAW`: blue, light blue on dark backgrounds),
opens its object, and is broken (grey title, a plain icon; opening it
offers to delete it) when the object is gone. The shell keeps shadows
with their objects when it renames or moves files and folders (also
for shadows kept in a moved directory), moves program objects or
renames groups, and deletes them with their objects.

**The Startup folder** (a visible program group, made on first start
or by the profile update) starts its program objects, and opens the
objects of its shadows, when the desktop starts for the first time
after the system starts, as the Workplace Shell's does. A shell
started again in the same session does not (the system atom
`RS2SHELL_STARTUP`, which lives as long as PM's shared state, marks
it done); `SET RESTARTOBJECTS=NO` turns it off. `RS2SHELL /STARTUP`
does only this and ends (DESKTEST uses it).

**Dragging** with mouse button 2 goes through PMDRAG as the Workplace
Shell's does (see [Drag and drop](#drag-and-drop-pmdrag)): the
container's `CN_INITDRAG` makes a DRAGINFO with an item per selected
object and calls `DrgDrag` (until button 2 goes up; Esc cancels); the
dragged records have source emphasis. Files and folders are
`<DRM_OS2FILE,DRF_UNKNOWN>` items with their directory and name
(`hstrContainerName`, `hstrSourceName`) and a type by extension (Plain
Text, Executable, ...), so any program that takes files can use them;
a target that asks for a file to be rendered (`DM_RENDER` with
`hstrRenderToName`) gets a copy there and `DM_RENDERCOMPLETE`. The
shell's other objects are `<DRM_OBJECT,DRF_OBJECT>` items with their
object id. As a target (the containers' `CN_DRAGOVER` and `CN_DROP`)
the shell takes files from any program (`DRM_OS2FILE` with a directory
and a name) and its own objects, with the operation the user holds
down (Ctrl copies, Shift moves, Ctrl+Shift makes shadows) or the
Workplace Shell's default: files move within a drive and are copied to
another; icons dropped in their own folder move there (positions are
kept in OS2.INI at once); program objects move between the desktop
and groups; a program file dropped on the desktop or a group becomes a
program object and other files become shadows there (at the drop
point); files dropped on a program object, or on a program file,
start it with their names as parameters; folders, drives and groups
(their icons, shadows of them, the folders of a tree) take drops into
them. Every item's source gets `DM_ENDCONVERSATION`.

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
| `RS2_Desktop` | `Version` | `5` once the default objects have been made (the shell's MAKEINI step, rs2's own content, not IBM's INI.RC; older versions get what is new) |
| | `DesktopGroup`, `PromptsGroup`, `ProgramsGroup`, `StartupGroup` | the group handles (hex) |
| | `NextShadow` | the next shadow id (hex) |
| | `Background` | desktop colour, `RRGGBB` |
| | `MinimizeTo` | `VIEWER` (default) or `DESKTOP` |
| `RS2_IconPos` | `FOLDER\|OBJECT` | `x y` (the container's `ptlIcon`); folders `DESKTOP`, `G` + group handle, `DRIVES`; objects `G`/`P` + handle, `DRIVES`, `DRIVE:C`, `SHUTDOWN`, `S` + shadow id, ...; `FOLDER\|@HEIGHT` the height they are from (icons keep their distance from the top when a folder or the screen changes height) |
| `RS2_Windows` | folder (`DIR:C:\OS2\` for directories) | `x y cx cy view` |
| `RS2_Associations` | extension (upper case) | the program |
| `RS2_Shadows` | `S` + id (8 hex digits) | the folder (`DESKTOP`, `G` + group handle, `DIR:` + directory), a tab, the object: a full path or an object id (`P`/`G` + handle, `DRIVES`, `DRIVE:C`, `MINIMIZED`, `WINLIST`, `SHUTDOWN`) |

The first start makes the groups Desktop (hidden), Command Prompts
(OS/2 Window, OS/2 Full Screen), Programs (System Editor if E.EXE is
installed, Install rs2) and Startup (empty), the associations and the
colour; a missing piece is made again later.

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
`cargo xtask run --desktop` boots into the Workplace Shell
(`init=OS2\PMSHELL.EXE` in place of work/cmdline.txt's `init=`;
`--init OS2\RS2SHELL.EXE` boots RS2SHELL, the earlier desktop).

### Drag and drop (PMDRAG)

PMDRAG.DLL (`userland/pmdrag`) does direct manipulation as IBM's does
(its code is in IBM's PMCTLS.DLL; studied in Ghidra with the debug
symbols):

- String handles are atoms of the system atom table (a string starting
  with `!` or `#` gets a 0xFF byte in front so it is not an integer
  atom), so every process sees them: `DrgAddStrHandle`,
  `DrgQueryStrName[Len]`, `DrgDeleteStrHandle`,
  `DrgDeleteDraginfoStrHandles` (each of the items' handles once).
- A DRAGINFO is giveable and gettable shared memory (the same address
  in every process): a 0x2C-byte header (magic 0xF00E0E0F, the items'
  address) in front of the DRAGINFO and its DRAGITEMs.
  `DrgAllocDraginfo`/`DrgReallocDraginfo`; a target gets at it with
  `DrgAccessDraginfo` (`DosGetSharedMem`) and lets go with
  `DrgFreeDraginfo` (`DosFreeMem`; it goes away when the last process
  has), which fails for the source while its drag runs
  (PMERR_SOURCE_SAME_AS_TARGET), so a source that is its own target
  frees it once, after `DrgDrag`; `DrgPushDraginfo` gives it to a
  window's process. DRAGTRANSFERs are the same behind a header with
  magic 0xE0E00F0F; `DrgSendTransferMsg` (`DM_RENDER`,
  `DM_RENDERPREPARE`) and `DrgPostTransferMsg` (retrying while the
  queue is full) give them to the receiver first.
- `DrgDrag(hwndSource, pdinfo, pdimg, cdimg, vkTerminate, pRsvd)` is
  modal: it captures the mouse for the source window and reads the
  thread's messages itself (dispatching the others) until the
  terminating button goes up (`VK_ENDDRAG`: `WM_ENDDRAG`, or button 2
  up) or Esc. Over each window under the pointer (`WinWindowFromPoint`
  with children) it sends `DM_DRAGOVER` (the DRAGINFO and the pointer
  position) and the source `DM_DRAGOVERNOTIFY` with the answer; a
  window that answered `DOR_NEVERDROP` gets no more until the pointer
  leaves it; leaving a window sends `DM_DRAGLEAVE`; F1 posts
  `DM_DROPHELP` and ends the drag. The operation (`usOperation`) is
  `DO_DEFAULT`, or `DO_COPY`, `DO_MOVE`, `DO_LINK` with Ctrl, Shift,
  Ctrl+Shift held down (at the start and as the keys change; another
  key held, `DO_UNKNOWN`); with the default the target must name an
  operation (copy, move, link or create a program object) or one of its
  own above `DO_UNKNOWN`. At the end a window that answered `DOR_DROP`
  gets `DM_DROP` (sent), with `usOperation` set to the operation it
  named if the user chose none, and `DrgDrag` returns it; else 0.
  `WM_QUIT` during a drag ends it and is posted again.
- The pointer shows the first drag image's icon (`DRG_ICON`) over
  windows that take the drop, `SPTR_ILLEGAL` elsewhere;
  `DrgSetDragPointer` replaces it while the pointer is over the same
  window, `DrgSetDragImage` changes the icon. (IBM's draws the images,
  bitmaps and polygons too, on the screen beside the pointer.)
- RMF strings and types (`crates/pmwin/src/drag.rs`, host-tested):
  `<mechanism,format>` pairs and `(m1,m2)x(f1,f2)` cross products,
  blanks ignored, the first pair native; type lists with the first the
  true type: `DrgVerifyRMF` (NULL matches any), `DrgVerifyNativeRMF`,
  `DrgQueryNativeRMF[Len]`, `DrgVerifyType`, `DrgVerifyTrueType`,
  `DrgVerifyTypeSet`, `DrgQueryTrueType[Len]`.
- `DrgQueryDragStatus`, `DrgQueryDraginfoPtr`,
  `DrgQueryDraginfoPtrFromHwnd`, `DrgQueryDraginfoPtrFromDragitem`
  (the header in front of the item), `DrgGetPS`/`DrgReleasePS`.

The System Editor takes a file dropped on its window (see below); the
desktop shell drags and takes drops as described above.

**Not done** (PMDRAG): lazy drag (`DrgLazyDrag`, `DrgLazyDrop`,
`DrgCancelLazyDrag` fail), `DrgDragFiles` and `DrgAcceptDroppedFiles`
(they fail), drag images drawn beside the pointer.

**Not done** (shell): templates, object settings other than program
objects' (and a file's properties), the lock-up, a real Workplace
Shell object model (the desktop is not a directory: files dropped on
it become shadows, not moved there), shadows kept in a directory
folder are rs2's (in OS2.INI, not abstract objects in the directory),
undoing a move, deleting into a trash can (there is no shredder or
trash can), drag and drop with the keyboard (Shift+F10 menus do it).

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

**A file dropped on the window** (one item, a `DRM_OS2FILE` file with
its directory and name, dropped with the default operation or Ctrl)
opens in it, after asking about unsaved changes as File > Open does;
the MLE is subclassed for `DM_DRAGOVER` and `DM_DROP`, the opening
happens after the drop (posted), and the source hears
`DM_ENDCONVERSATION` (`drop.rs`).

**Not done**: the window's icon, Undo restoring a font or colours (rs2's MLE
does not undo those), the Set tabs dialog E has but does not offer.

## Installing OS/2's applications (APPINST.EXE)

`C:\OS2\APPINST.EXE` (userland/appinst; "Install OS/2 Applications" in
the desktop's Programs folder) installs the OS/2 applications rs2 runs
(see [COMPATIBILITY.md](COMPATIBILITY.md)) from the OS/2 Warp 4.52 client
CD. rs2 ships none of IBM's files; this puts the user's own copies where
OS/2 itself keeps them.

- **The catalogue** is `userland/appinst/apps.txt`, built into the
  program: categories (Games, Productivity, REXX, Command line tools,
  Information) with their applications, each with its files (the target
  path and the CD's packed file that holds it), the program objects to
  make and what else it needs (PM REXX needs REXX; the command line
  tools need OS/2's message files). An application is shown as installed
  when all its files are on the boot drive.
- **The window**: the source (the first drive with `\OS2IMAGE`, or
  `APPINST X:`), Refresh, a tree of the categories with check boxes
  (checking a category checks all its applications, a partly checked one
  shows a filled box; each application can be checked by itself; mouse:
  the arrow opens and closes a category, the box or a double click
  checks; keys: arrows, PgUp/PgDn, Home/End, Space, +/-), a description
  of the item with the cursor, a status line, a progress bar, Install
  and Close.
- **Installing** runs on a second thread: each file is unpacked from its
  packed file on the CD with `crates/os2-pack` (files stored as they are,
  like TEDIT.EXE, are copied), written with its date, time, attributes
  and extended attributes (`DosSetPathInfo` levels 1 and 2), the
  directories made as needed, each packed file read once. Then the
  program objects are made, a folder per category ("Games",
  "Productivity", "REXX", "Information"; objects already there are
  updated). With the Workplace Shell running they are Workplace Shell
  objects (WinCreateObject, which goes to the shell's process): the
  folders in the Programs folder, the books in the Assistance Center's
  folders (Reference and Commands, Tasks, Troubleshooting), with IBM's
  INI.RC object IDs and places where it has them (`<WP_GAMES>`,
  `<WP_TOOLS>`, `<WP_INFO>`; `<WP_KLDK>`, `<WP_CHESS>`, `<WP_EPM>`,
  `<WP_CMDREF>`...; apps.txt's last fields). With RS2SHELL
  they are program groups, and the program broadcasts the registered
  message `RS2_ProgramListChanged` to the top-level frames, on which
  RS2SHELL refills its desktop and program group folders. When an application
  lives in `\OS2\APPS` and CONFIG.SYS's `SET PATH` or `LIBPATH` lacks
  `C:\OS2\APPS`, it is added (Solitaire finds its card font along the
  PATH) and the message says to restart; rs2's own CONFIG.SYS has it.
  A boot drive that cannot be written (rs2 running from the CD) is
  reported before anything is done.

The live image's FAT partition is only as large as its files, so
applications are installed on an installed system (or a larger USB
stick); a full disk shows as files that "cannot be written".

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
  replacement panel (HM_REPLACE_HELP_FOR_HELP), else the first panel of
  rs2's own help for help, OS2\HELP\HMHELP.HLP (userland/helpmgr/
  HMHELP.IPF; IBM's HMHELP.HLP is not shipped), as does F1 in a help
  window. The help window is activated again once the request's message
  is done, so a request from an application's menu leaves it active.

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
(or Backspace, or Previous) goes back; Esc with nothing to go back to
closes the window. Links lead to panels of the file
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
  runs itself with `CHILD`), the window manager calls in
  `ctltest/src/wm.rs` (the text cursor, frames of a class derived from
  WC_FRAME, `WinSetMultWindowPos`, `WinStoreWindowPos`/
  `WinRestoreWindowPos`, DDE, a new clipboard viewer's
  `WM_DRAWCLIPBOARD`, `WinQueryQueueInfo`, PMSPL), printing PASS/FAIL
  and `ctltest done: N failure(s)`.
- `RS2\PMXTEST.EXE` (userland/pmxtest, with `RS2\PMXHOOK.DLL` from
  userland/pmxhook): self-checks of hooks of every kind PMWIN calls (queue
  hooks, and system hooks in PMXHOOK.DLL called in a second process,
  `PMXTEST CHILD`, and journal playback in the PM server; `PMXTEST
  RECORD` with keys from `--type` checks journal recording), a DDE
  conversation with a server in another process (`PMXTEST DDESERVER`),
  the registry through PMWINX, and owner drawing (list box, menu, user
  button, value set, slider, notebook tab), printing PASS/FAIL and
  `pmxtest done: N failure(s)`. `PMXTEST WINS [PROGRAM]` (starting
  PROGRAM first) lists the window tree on the serial port (class, id,
  text, position, style), to see what another program made.
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
  `RS2_IconPos`, PMWIN's `Rs2SetDesktopWindow` (z-order),
  `WinWindowFromPoint`, `WinQueryWindowProcess` and presentation
  parameter colours; the Startup folder (an object started by
  `RS2SHELL /STARTUP`, and not again in the same session); shadows
  (made, listed, following a rename and a moved directory, broken,
  removed with their object); the shell's file operations (a tree with
  EAs on a file and a directory copied with each file reported,
  replaced, refused inside itself, stopped, moved, deleted; free names);
  PMDRAG (string handles, DRAGINFO and DRAGITEM calls, the RMF and type
  checks, and a `DrgDrag` from its window onto a container in a second
  process, `DESKTEST DROPTARGET`: `CN_DRAGOVER` with `DOR_DROP`/
  `DO_COPY` seen in `DM_DRAGOVERNOTIFY`, the drop, one item read from
  its directory and name, one rendered by the source through
  `DM_RENDER`/`DM_RENDERCOMPLETE`, `DM_ENDCONVERSATION` for both, a drag
  cancelled with Esc), printing PASS/FAIL and `desktest done: N
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
  PASS/FAIL and `gpitest done: N failure(s)` (among them metafiles made
  on a metafile DC, played, copied, rebuilt with `GpiSetMetaFileBits`,
  saved and loaded; retained segments in `DM_RETAIN` and
  `DM_DRAWANDRETAIN`, drawn by `GpiDrawChain`, read with `GpiGetData`
  and played with `GpiPutData`, drawn onto a metafile;
  `WinCpTranslateString`), then a window with every kind of primitive
  for a screenshot check (it also draws `C:\YOSEMITE.BMP` when
  present).
- The application installer: a dist system disk (`cargo xtask disk
  --system --dist` with dist/manifest.txt as work/modules.txt) booted
  with the OS/2 client CD (`--hdd IMG --boot-hdd --iso CD.ISO`): the
  installer from the Programs folder finds the CD on D:, the Games
  category and PM REXX (bringing REXX) installed, the Games and REXX
  folders appearing on the desktop, and after a restart Solitaire
  started from the Games folder.
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
