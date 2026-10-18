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
| `userland/pmgpi` → PMGPI.DLL | Exports (`Gpi*`, `Dev*`), presentation spaces bound to windows or memory bitmaps. |
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

## Milestones

1. `hello.exe` from the Toolkit samples (WinInitialize, message queue,
   WinRegisterClass, WinCreateStdWindow, WinGetMsg/WinDispatchMsg,
   WinBeginPaint, WinFillRect, GpiCharStringAt, WinMessageBox) shows its
   window on a desktop, can be moved with the mouse and closed.
2. Dialogs and standard controls from resources (`dialog.exe`,
   `template.exe`): WinDlgBox, buttons, entry fields, list boxes, static
   text, menus, accelerators.
3. GPI breadth (`graphic.exe`, `clipbrd.exe`, `jigsaw.exe`): paths, areas,
   bitmaps, fonts, transforms, clipboard.
4. A desktop shell: window list, program launcher (PMSHELL), then the
   Workplace Shell (SOM) much later.

## Testing

- Host unit tests in `crates/gpi` (render into memory and compare pixels)
  and `crates/pmwin` (window tree, region computation, message routing).
- QEMU runs of the Toolkit PM samples, checked with screenshots and with
  mouse/keyboard input through `xtask run --type` (`{mouse_move dx dy}`,
  `{mouse_button n}`).
