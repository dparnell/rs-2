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
queues, the input state) lives in a *shared data object* of PMWIN.DLL, the
same way OS/2 keeps it in shared segments. A process draws straight into
the framebuffer, clipped to its windows' visible regions. Cross-process
operations (WinSendMsg to another process's window, input routing) go
through the queues in shared memory and wake the receiving thread with an
event semaphore.

The logic lives in host-testable crates; the DLLs are thin:

| Crate / DLL | Contents |
|-------------|----------|
| `crates/gpi` | Rendering engine: presentation-space attributes, lines, boxes, arcs, areas, text with bitmap fonts, bitmaps and raster operations, clipping to rectangle lists. Draws into a `Surface` (32-bit pixels). No OS dependencies; unit-tested on the host. |
| `crates/pmwin` | Window system core: window tree, classes, z-order, visible-region computation, message queues, focus and activation, frame windows and their controls, standard window classes. Stores its state in a caller-provided arena so the same code works in the shared object and in host tests. |
| `userland/pmwin` → PMWIN.DLL | Exports (`Win*`), the shared data object, locking, the input thread, glue to the kernel services. |
| `userland/pmgpi` → PMGPI.DLL | Exports (`Gpi*`, `Dev*`) as forwarders to PMWIN, which implements them at ordinal 4000 + the PMGPI ordinal, since presentation spaces belong to windows. |
| `userland/pmshapi` etc. | Smaller DLLs: profile (OS2.INI) APIs, program list, PMCTLS, HELPMGR stubs. |

## Kernel services (module RS2API)

A kernel gate module, `RS2API`, holds rs2-private 32-bit services used by
rs2's own DLLs (applications never import it):

- `Rs2MapDisplay(PDISPLAYINFO)`: maps the framebuffer into the calling
  process (same linear address in every process, in the shared arena) and
  returns its address, width, height, stride and pixel format.
- `Rs2ReadInput(PINPUTEVENT, ulTimeout)`: raw input for the PM session:
  key events (scan code, character, shift state, make/break) and mouse
  events (absolute position in pixels, buttons, wheel).
- `Rs2SetPointer(...)`: hardware-style pointer drawing is done by PMWIN in
  software; this call only tells the kernel where it is, for a future
  hardware cursor.

The session manager gains a PM session type: the first PM program starts
(or joins) the PM session, which owns the framebuffer while it is in the
foreground and receives raw input instead of the text keyboard queue.
Switching to a text session and back makes the kernel post an event
semaphore so PMWIN repaints everything.

## Status

Milestone 1 works: the unmodified Toolkit `hello.exe` shows its window,
which can be moved by its title bar and closed, after which the system
returns to the text session. State is still per process (one PM
application at a time).

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
  `WinLoadMenu`, `WinCreateMenu`, `WinPopupMenu`.
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
  Unknown control classes become placeholder frames.
- Controls, each a registered class with its own window procedure:
  `WC_BUTTON` (push, default, check box, radio, 3-state and their
  automatic kinds, `BM_*`, `BN_CLICKED`), `WC_STATIC` (text with DT_*
  layout and mnemonics, group boxes, rectangles, frames, bitmaps),
  `WC_ENTRYFIELD` (editing, selection, insert/overwrite, scrolling,
  text limit, `ES_MARGIN`, `ES_READONLY`, `ES_UNREADABLE`, `EM_*`,
  `EN_*`), `WC_LISTBOX` (single and multiple selection, scrolling with a
  scroll-bar child, keyboard, `LM_*`, `LN_*`), `WC_SCROLLBAR` (arrows,
  paging, proportional slider tracking, `SBM_*`, `WM_VSCROLL`/
  `WM_HSCROLL`). `WM_QUERYDLGCODE` and `WM_MATCHMNEMONIC` tell the
  dialog manager about them.
- Double clicks (`WM_BUTTONnDBLCLK`), `WM_CREATE` with a CREATESTRUCT,
  `WinSetParent`, `WinWaitEventSem`.
- Threads: a recursive PMWIN lock (`src/lock.rs`) is taken by every
  export (in the generated trampolines) and released while a thread
  waits for messages or semaphores; only the first message queue's
  thread reads the kernel input; `WinSendMsg` to another thread's window
  is queued to that thread and waits for the reply (`template.exe`
  uses an object window on a second thread).
- `HELPMGR.DLL` (help instances are accepted, help shows nothing) and
  `PMCTLS.DLL` (`WinFileDlg`/`WinFontDlg` return as cancelled).

Not done for milestone 2: `WC_MLE`, combo boxes, spin buttons, sliders,
value sets, notebooks, containers, the clipboard, presentation
parameters (accepted and ignored), owner-drawn items, menu bitmaps,
minimizing; `controls/style.exe` needs the MLE, PMSHAPI and the
clipboard.

Milestone 3 (GPI breadth) works as far as the GPI is concerned: every
PMGPI import of the Toolkit samples `graphic.exe`, `clipbrd.exe`,
`jigsaw.exe` and `palette.exe` is implemented (they still need dialogs,
menus, the clipboard and HELPMGR/PMCTLS/PMSHAPI to run end to end;
`palette.exe` runs with stubs for those). `RS2\GPITEST.EXE` runs
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
   palettes; the clipboard is still to do.
4. A desktop shell: window list, program launcher (PMSHELL), then the
   Workplace Shell (SOM) much later.

## Testing

- Host unit tests in `crates/gpi` (render into memory and compare pixels;
  the Toolkit's sample bitmaps when `work/media` is present) and
  `crates/pmwin` (window tree, region computation, message routing).
- `RS2\GPITEST.EXE` (userland/gpitest): GPI self-checks printing
  PASS/FAIL and `gpitest done: N failure(s)`, then a window with every
  kind of primitive for a screenshot check (it also draws
  `C:\YOSEMITE.BMP` when present).
- QEMU runs of the Toolkit PM samples, checked with screenshots and with
  mouse/keyboard input through `xtask run --type` (`{mouse_move dx dy}`,
  `{mouse_button n}`).
