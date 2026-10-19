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
| `crates/gpi` | Rendering engine: presentation-space attributes, lines, boxes, arcs, areas, text with bitmap fonts, bitmaps and raster operations, clipping to rectangle lists. Draws into a `Surface` (32-bit pixels). No OS dependencies; unit-tested on the host. |
| `crates/pmwin` | Window system core: window tree, z-order, visible and update regions, hit testing, the queues of threads to wake after invalidation; message queues with mouse-move coalescing and the not-responding bookkeeping; the MLE's text engine (`mle.rs`: layout with word wrap and tabs, editing, undo, import/export formats, search) and the arithmetic of spin buttons, sliders and value sets (`controls.rs`); the notebook's pages and geometry (`notebook.rs`). Plain data structures, unit-tested on the host. |
| `userland/pmwin` → PMWIN.DLL | Exports (`Win*`, `Gpi*`), the shared state and its heap, locking, messaging, the built-in window classes, and the PM server's code (input routing, hung programs, window list). |
| `crates/viocell` | Text cells on pixels: renders VGA text cells (font, palette, blink, cursor, marked block) to 24-bit bitmaps; Copy/Copy All text and Paste keystrokes. Host-tested; for VIO windows now, AVIO later. |
| `userland/pmviowin` → `C:\OS2\PMVIOWIN.EXE` | The windows of windowed VIO sessions (see [Windowed VIO sessions](#windowed-vio-sessions-os2-window)). |
| `userland/pmsrv` → `C:\OS2\PMSRV.EXE` | The PM server process: only calls PMWIN's `Rs2PmServer` (private ordinal 9000; 9001, `Rs2ShellPosition`, gives PMSHAPI's `WinQueryTaskSizePos` the shell's next cascade position; 9002, `Rs2PmVioWindow`, registers a VIO window's client). |
| `userland/pmgpi` → PMGPI.DLL | Exports (`Gpi*`, `Dev*`) as forwarders to PMWIN, which implements them at ordinal 4000 + the PMGPI ordinal, since presentation spaces belong to windows. |
| `userland/pmctls` → PMCTLS.DLL | The standard file and font dialogs (`WinFileDlg`, `WinFontDlg`): ordinary PM dialogs, built from templates made in memory, run through PMWIN like an application's. |
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

RS2API ordinals: 1-6 PM (above), 10-16 windowed VIO sessions (see
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
  buttons (maximize and restore work, minimize and hide are inert), menu
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
  plus the system keys F10, Alt+F4, Alt+Space and Shift+Esc; commands of
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

Not done for milestone 2: containers, presentation
parameters (accepted and ignored), owner-drawn items, icons and
pointers from resources, menu bitmaps, minimizing, help windows, drag
and drop.

The OS/2 System Editor (E.EXE) runs unmodified: see
[COMPATIBILITY.md](COMPATIBILITY.md#presentation-manager-programs).

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
  or bitmap items (icons and bitmaps are drawn as framed squares), item
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
   and font dialogs.
5. A desktop shell: window list, program launcher (PMSHELL), then the
   Workplace Shell (SOM) much later.

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
- `RS2\NBTEST.EXE` (userland/nbtest): notebook self-checks (pages,
  texts, page data and info, page windows and a page dialog from a
  template, notifications, keys, clicks, sizes, deleting) printing
  PASS/FAIL and `nbtest done: N failure(s)`, then a window with a
  settings-style notebook and a tabbed dialog (`NBTEST QUIT` ends at
  once, `NBTEST HANG` stops reading its queue for 30 s first).
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
