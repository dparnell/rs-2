# The Workplace Shell: what is on the disk, and how rs2 can provide it

This is a study of IBM's Workplace Shell (WPS) as shipped on the Warp
4.52 CD, made to plan rs2's own. It covers the modules, the SOM object
model they are built on, the class tree, persistence and start-up.
IBM's files are only studied (under `work/`), never shipped.

rs2 today has its own desktop, RS2SHELL.EXE. It is not object-oriented
but covers the everyday desktop (see ROADMAP, Milestone 4). rs2's
PMWP.DLL holds only the calls programs use without a WPS:
WinCreateObject for program objects and the file icon calls.

## Sources

| What | Where (under `work/`) | Use |
|------|-----------------------|-----|
| PMWP.DLL, the WPS itself | `unpacked/disk_9/OS2/DLL` | behaviour; Ghidra |
| pmwp.sym (4399 symbols) | `media/client/os2image/debug/symbols/os2/dll` | function names: `obj_wpSaveState`, `fdr_wpOpen`, ... |
| SOM.DLL, SOMU.DLL, SOMIR.DLL | `unpacked/disk_7/OS2/DLL` | the SOM runtime |
| som.sym (529 symbols) | `media/client/os2image/debug/symbols/INSTALL/os2/dll` | |
| Toolkit 4.5: `idl/wp*.idl`, `h/wp*.h` | `media/boot/toolkit/os2tk45` | every WPS class's methods, **release order** (the ClassData layout), types |
| Toolkit `som/include/somapi.h`, `somcls.idl`, `somobj.idl`, `somcm.idl` | same | `somStaticClassInfo`, ClassData, SOMObject/SOMClass/SOMClassMgr |
| Toolkit `samples/wps/wpstutor` (with a built `wpstutor.dll`) | same | a third-party WPS class DLL: a binary-compatibility test |
| Toolkit `som/bin/sc.exe` | same | IBM's SOM compiler (for more test classes) |
| WPSH.IR, SOM.IR | `unpacked/disk_7/OS2/ETC` | interface repositories |
| INI.RC, INISYS.RC | `unpacked/disk_2/OS2`, `disk_1` | the default desktop: 266 `PM_InstallObject` entries; MAKEINI builds OS2.INI from them (it already runs on rs2) |

The Ghidra projects are made with `tools/ghidra/lxload.sh PMWP.DLL
pmwp.sym` and `SOM.DLL som.sym`; `tools/ghidra/lxdecomp.sh` decompiles
by name.

## Modules

| Module | Size | Role |
|--------|------|------|
| PMSHELL.EXE | 6 KB | The process: WinInitialize, WinCreateMsgQueue, then PMWP's `ShlStartWorkplace` (@211) and `MessageLoopProc` (@284); `ShlSaveEnv`/`ShlExceptionHandler` (@213/@212) guard it |
| PMWP.DLL | 1 MB | All the base classes (below), the desktop, folders, settings notebooks, drag and drop, the object thread, the lazy writer; 379 exports |
| PMWPMRI.DLL | 54 KB | PMWP's texts, menus, dialogs |
| SOM.DLL | 92 KB | The SOM 2.1 kernel: SOMObject, SOMClass, SOMClassMgr, somBuildClass, method resolution, ids, the environment; imports only DOSCALLS, MSG and NLS |
| SOMU.DLL | 81 KB | SOM utility classes; PMWP uses only SOMMBeforeAfter |
| WPCONFIG.DLL | 268 KB | The System Setup objects (System, Keyboard, Mouse, Sound, Country, Font and Colour Palettes, ...) |
| WPPRINT.DLL | 367 KB | Printer, Job, Port, Queue and Printer Driver objects |
| BDWPCLS.DLL, BDCALL32.DLL | | Bidirectional-language pages (called by PMWP) |
| SEAMLESS.DLL, PMVIOP.DLL | | WIN-OS/2 and VIO program settings |

PMWP imports 435 functions: PMWIN 151, DOSCALLS 98, PMGPI 39, PMMERGE 32,
PMDRAG 30, PMSHAPI 25, som 22, plus a few from bdwpcls, SEAMLESS, NLS,
LIBUNI, UCONV, QUECALLS and SESMGR.

PMWP uses 22 functions from SOM.DLL:

- Memory: SOMMalloc, SOMCalloc, SOMRealloc, SOMFree.
- Errors and tracing: SOMError, SOM_TraceLevel, somPrintf.
- The three base classes: SOMObjectNewClass and SOMObjectClassData, SOMClassNewClass and SOMClassClassData, SOMClassMgrNewClass, SOMClassMgrClassData and SOMClassMgrObject.
- Ids: somIdFromString, somStringFromId, somBeginPersistentIds and somEndPersistentIds.
- Class building and resolution: somEnvironmentNew, somBuildClass, somParentNumResolve, somDataResolve.

### Running IBM's WPS on rs2

The IBM modules can be staged and started with PMSHELL. Loading them
stops on missing exports, about 95 in all:

- PMMERGE: 77. Most are needed by PMVIOP and SEAMLESS (GRE entries, the Win16 heap). PMWP itself needs a few private entries, such as WinSwitchToTaskManager, She32SystemShutdown, Start32SystemExecutables and Win32HAppFromPID.
- PMCTLS: 4 (the NLS container strings).
- PMGPI: 3.
- PMSHAPI: 3.
- PMWIN: 8.

PMWP also talks to a "shell window" in PMWIN (`WinSendMsg(vhwndShellWindow,
0x1014)`) and uses PMMERGE-private calls during start-up. IBM's WPS is
therefore tied to IBM's PM internals, not only to the documented API.

## SOM 2.1 as the WPS uses it

Every class is built at run time by its `<Class>NewClass(major, minor)`.
The function first makes its parent class and metaclass, then calls
`somBuildClass(inherit_vars, &sci, major, minor)` with a static
`somStaticClassInfo` (somapi.h): method table entries (smt), overrides
(omt), instance data size, the parents' ids, and pointers to the class's
`<Class>ClassData` and `<Class>CClassData`. Afterwards it calls
`somParentNumResolve(parentMtabs, n, token)` once for each overridden
method, to store the parent's implementation for `parent_` calls. For
WPAbstract that is 16 overrides; for WPFileSystem it is 34.

**ClassData is the binary interface.** `<Class>ClassData` is an exported
structure:

```text
{ classObject, token for method 1, token for method 2, ... }
```

The tokens are in the IDL's `releaseorder`. Callers, including other
classes and third-party class DLLs, invoke a method with
`(*WPObjectClassData.wpSaveData)(self, ...)`. A **method token is
executable**: somBuildClass generates a thunk for each method
(somMakeThunk, from the prototype `somProtoThunk`):

```text
mov ecx,[esp+4]      ; the object
mov edx,[ecx]        ; its method table
jmp [edx+offset]     ; the method's slot
```

Instance data is reached the same way. `<Class>CClassData` holds a data
token that is a thunk:

```text
mov eax,[esp+4]
add eax,offset
ret
```

For example, `obj_wpQueryTitle` is `(*dataToken)(self)->core->title`
(+4, then +0x14).

Objects are `{ mtab*, instance data... }`; an mtab starts with the
class object, the class name and the instance size, followed by the
method slots. Classes are objects too: each WPS class has a metaclass
(`M_WPObject`, ...) with its own class methods, the `wpcls*` methods
(68 of them).

An rs2 SOM.DLL must produce the same ClassData layouts, mtab layout and
thunks. Third-party WPS classes (wpstutor; later XWorkplace-style
extensions) are compiled against the Toolkit headers. They call through
those tokens and subclass PMWP's classes via `somBuildClass` with IBM's
`somStaticClassInfo` (layout versions 1-3).

## Class tree (PMWP.DLL)

The tree below was recovered from the 41 `*NewClass` functions; each
class also has an `M_` metaclass.

```text
SOMObject
└─ WPObject
   ├─ WPAbstract
   │  ├─ WPProgram
   │  ├─ WPShadow ── WPNetLink
   │  ├─ WPDisk
   │  ├─ WPLaunchPad
   │  └─ WPShredder
   ├─ WPFileSystem
   │  ├─ WPDataFile
   │  │  ├─ WPIcon, WPPointer, WPMet, WPPif
   │  │  ├─ WPImageFile ── WPBitmap
   │  │  └─ WPProgramFile ── WPCommandFile
   │  └─ WPFolder
   │     ├─ WPDesktop, WPDrives, WPRootFolder, WPStartup, WPTemplates
   │     ├─ WPFindFolder, WPMinWinViewer, WPProgramGroup, WPVault
   │     └─ WPNetgrp, WPNetwork, WPServer, WPSharedDir
   ├─ WPTransient
   │  ├─ WPCnrView ── WPDiskCV, WPFolderCV
   │  ├─ WPFilter ── WPFinder
   │  └─ WPMinWindow
   └─ WPObjectBA
SOMClassMgr ── WPClassManager
```

pmwp.sym names 491 distinct `wp*` methods and 68 `wpcls*` methods. The
symbol prefixes name the implementing classes, for example:

| Prefix | Class | Methods |
|--------|-------|---------|
| `obj_` | WPObject | 155 |
| `fdr_` | WPFolder | 131 |
| `fs_` | WPFileSystem | 76 |
| `ref_` | WPShadow (reference) | 57 |
| `df_` | WPDataFile | 47 |
| `pgm_` | WPProgram | 42 |
| `lnchpd_` | WPLaunchPad | 42 |

WPCONFIG.DLL and WPPRINT.DLL add the System Setup and printer classes on
top of these.

## Persistence

- **Abstract objects** (programs, shadows, the Launchpad, ...) live in
  OS2.INI under `PM_Abstract:Objects` (one key per object handle), with
  `PM_Abstract:Icons` and `PM_Abstract:FldrContent` (which folder holds
  which abstract objects).
- **File-system objects** are files and directories. Their state is in
  EAs: `.CLASSINFO` holds the class and saved instance data, plus
  `.LONGNAME`, `.TYPE`, `.ICON` and `.ICONPOS` for folders. A drive's
  root has no directory entry to hold EAs: as in IBM's shell, they are
  on the hidden file `WP ROOT. SF` in it (rs2's FAT takes spaces inside
  8.3 names, as OS/2's does).
- **Object handles**: `PM_Workplace:Handles0/1` and
  `PM_Workplace:ActiveHandles` (the "HFS nodes": a path→handle table
  restored by `RestoreHFSNodes`).
- **Object IDs** (`<WP_DESKTOP>`, `<WP_DRIVES>`, ...) are in
  `PM_Workplace:Location`.
- `PM_Workplace:FolderPos` holds window positions.
- `PM_Workplace:Startup` and `PM_WorkPlace:Restart` hold what to reopen.
- `PM_InstallObject` and `PM_InstallClass` (from INI.RC) list what to
  create on the first start.
- **Saving**: `wpSaveState` calls `wpSaveLong`, `wpSaveString` and
  `wpSaveData(self, class name, key, buffer, size)`. A lazy-writer
  thread flushes to the INI file and EAs. `wpRestoreState` reads the
  values back when an object is made awake.

## Start-up (`ShlStartWorkplace`)

1. Reads the colours, backgrounds and text visibility from
   `PM_Colors`, `PM_SystemBackground` and `PM_WorkPlace:Options`.
2. Creates the class-table mutex and the lazy-writer and
   "wait for shell" event semaphores.
3. Restores the handle tables (`RestoreHFSNodes`).
4. Loads PMWPMRI and registers the classes (`ShlInitClassInfo`, from
   `PM_Objects`/`ClassTable` in OS2SYS.INI, which WinRegisterObjectClass writes).
5. Starts the object thread and a wheel-watcher thread.
6. Pumps messages until the desktop window exists.

The object thread makes `<WP_DESKTOP>` awake; on the first boot it also
processes `PM_InstallObject`. The Desktop folder then opens and starts
`PM_Workplace:Startup`.

## Plan

**Option A: run IBM's WPS on rs2.** This means implementing the missing
exports and PMWIN's shell-window protocol. It would give a reference to
compare against, but it depends on undocumented PMMERGE internals and
ships nothing of rs2's own. It is worth doing only partly: enough for
SOM.DLL to load (it already can, since it needs only DOSCALLS, MSG and
NLS), so IBM's SOM can be checked against rs2's.

**Option B (recommended): rs2's own SOM and WPS, binary compatible.**
The work is in phases; each phase is usable on its own:

1. **SOM runtime** (done; see "rs2's SOM.DLL" below).
2. **Core classes in rs2's PMWP.DLL**: done, with the object model and
   its views (see "rs2's Workplace Shell classes" below), with copying
   and moving (progress, name collisions), Sort and Include, templates
   and Find. Still to do:
   - the other view pages (fonts, icon arrangement, details columns);
   - a lazy writer.
3. **The shell process**: done (see "The shell process" below). It is
   the default desktop (`PROTSHELL=C:\OS2\PMSHELL.EXE`; `xtask run
   --desktop`), with what RS2SHELL's desktop had: the Minimized Window
   Viewer, the Window List, System Setup and Shut Down.
   `PROTSHELL=C:\OS2\RS2SHELL.EXE` still starts RS2SHELL.
4. **Compatibility**: wpstutor.dll registered with WinRegisterObjectClass
   and working; then the remaining classes (Launchpad, Shredder,
   Network); then rs2 versions of
   WPCONFIG and WPPRINT (System Setup and printers).

Phase 2 can start with generating the ClassData layouts and method lists
from the IDL files.

## rs2's SOM.DLL

`userland/som` builds SOM.DLL with IBM's 98 ordinals, variables and
class data structures. It imports only DOSCALLS.

| File | Contents |
|------|----------|
| `rt.rs` | Class records, method table layout, token and data thunks, resolution, the initializer/destructor/assignment controls |
| `env.rs` | Bootstrapping SOMObject, SOMClass, SOMClassMgr (and SOMParentDerivedMetaclass); `somBuildClass` for `somStaticClassInfo` layouts 1-4; derived metaclasses |
| `object.rs`, `class.rs`, `mgr.rs` | The three classes' methods (release order as in somobj/somcls/somcm.idl) |
| `id.rs` | Registered ids |
| `lib.rs` | Exported variables and functions; `fmt.rs` is somPrintf's formatter |

How it matches IBM's runtime (all checked in IBM's SOM.DLL with Ghidra
and pmwp.sym/som.sym):

- **Tokens are code.** A method token's thunk loads the object's method
  table and compares the word at the introducing class's usual block
  position with the class marker. When they match it jumps through the
  slot; otherwise it calls a resolver, which finds the block (this is
  the multiple-inheritance case). Methods returning structures take the
  object at `[esp+8]`. Data tokens work the same way and return the
  address of the class's data. The word before a token points to its
  description (IBM keeps a locator there too).
- **Method tables** start with `somMethodTab`'s nine public words. Each
  class has a block: a marker, then its methods in release order. The
  blocks of a class's leftmost ancestors keep their positions in every
  subclass.
- **Initialization** follows SOM 2.1's protocol. `somNew` calls the
  object's `somDefaultInit` with a copy of the class's `somInitCtrl`
  (the mask, `somInitInfo` entries of 0x1c bytes, the info size).
  Emitted initializers (PMWP's `obj_somDefaultInit`, ...) advance it and
  call their parents' initializers when their mask bytes say so.
  A class that does not define somDefaultInit, somDestruct, or the
  copy/assign operators gets SOMObject's generic walkers.
  Overrides of `somInit`/`somUninit` become the class's legacy
  initializer/uninitializer, which the generic walkers call; they are
  not put in the method table, as in IBM's SOM.
- **Initialization order:** depth first, parents left to right, each
  class at its first occurrence. **Destruction** mirrors it: parents
  right to left, a shared ancestor destroyed last.
- **Ids** are `char **`. Registering rewrites `*id` to point to
  `{0, name, ...}`; names compare without case, and a leading `::` is
  ignored.
- **Metaclasses:** when the explicit metaclass and the parents'
  metaclasses do not form a single chain, a derived metaclass with all
  of them as parents is made.
- **The class manager** loads class DLLs through `SOMLoadModule`
  (`SOMInitModule`, or `<Class>NewClass`). Its methods are called
  through tokens, so a subclass such as the WPS's WPClassManager can
  override them. `somMergeInto` moves the registry and makes the target
  `SOMClassMgrObject`; the old manager forwards to it.

Not done: `inherit_vars` without a bit for a parent (abstract
inheritance) is treated as inheriting everything. Class unloading
(`somUnloadClassFile`) only unregisters. `somCastObj`/`somResetObj`
return FALSE, and somCreateDynamicClass returns NULL.

**Tests.** `RS2\SOMTEST.EXE` builds classes from layout-4 static class
information as compiler output does. It covers:
- emitted initializers and destructors, and legacy somInit;
- parent calls, data tokens, and a metaclass with class methods and data;
- multiple inheritance with a shared base, and a derived metaclass;
- ids, dispatch by name, and the class manager.

It passes on both rs2's and IBM's SOM.DLL. Run it from a directory that
holds IBM's DLL; LIBPATH starts with `.`. IBM's SOM showed the
destruction order, and that it requires method descriptors and an
`inherit_vars` bit per parent.

`SOMTEST LOAD dll class` loads a class library through the class
manager and makes an instance. IBM's SOMU.DLL classes (SOMUTStringId,
SOMMBeforeAfter, SOMMSingleInstance, SOMMTraced, SOMBuf) and SOMIR.DLL's
interface repository classes load and work on rs2's SOM. Freeing an
uninitialized repository object faults on both SOMs alike.


## rs2's Workplace Shell classes

`userland/pmwp` (PMWP.DLL) provides the 58 classes IBM's PMWP exports
that the Toolkit describes, M_WPMinWinViewer, IBM's WPMinWindow (which
the Toolkit does not describe) and rs2's own RS2Shutdown, RS2WindowList
and RS2System.

**Class tables.** `tools/scripts/gen_wps_classes.py` reads the Toolkit's
IDL. It writes `src/classes_gen.rs` (each class's parents, metaclass,
version and methods in release order, plus index constants). It also
writes `src/classdata_gen.rs`: the exported `<Class>ClassData` and
`<Class>CClassData` variables and the `<Class>NewClass` functions, with
IBM's ordinals from `ordinals.txt`. WPImageFile is not in the Toolkit,
so WPBitmap derives from WPDataFile. The script's `EXTRA` list adds the
classes the IDL does not describe (WPMinWindow and rs2's own, with no
methods of their own), and metaclasses the IDL describes come along with
their classes even where IBM exports no class data for them.

**Building.** `wps.rs` builds each class as SOM compiler output does:
somBuildClass with a layout-4 `somStaticClassInfo`, one static method
per release-order entry. Methods without an implementation return 0.
The rest comes from the class modules:

| Module | Classes |
|--------|---------|
| `obj.rs` | WPObject (title, style, icon, folder, default view, handle, object ID, setup strings, save/restore, delete, copy, move, shadow) and M_WPObject (wpclsNew, class titles, icons and views, objects from handles and locations; somClassReady calls wpclsInitData) |
| `abs.rs` | WPAbstract, WPProgram (EXENAME, PARAMETERS, STARTUPDIR, PROGTYPE; wpQuery/SetProgDetails; opening starts the program), WPShadow (SHADOWID; shows and opens its object), WPDisk (a drive: its settings are its own, its other views and its default view are its root folder's) |
| `fs.rs` | WPFileSystem (real name, renaming, file icons, moving and copying files), WPDataFile (opened with the PMWP_ASSOC_FILTER program, else the System Editor), WPProgramFile, WPFolder (content: the directory, without hidden files, and the abstract objects in it), WPDrives (a WPDisk per drive), the desktop |
| `icons.rs` | rs2's own icons (pmwin::shellicons, as RS2SHELL shows them) for WPDrives, WPDisk (hard disk or CD-ROM), WPDesktop, command prompts and the Command Prompts folder (by its object ID, where IBM's INI.RC sets an icon resource); a folder's .ICON EA still comes first |
| `minwin.rs` | WPMinWinViewer (`<WP_VIEWER>`; its content is the minimized windows) and WPMinWindow (transient, one per window PMWIN minimizes to the viewer: WM_RS2_MINWINDOW to the window set with Rs2SetMinimizeViewer, the desktop's; opening one restores its window). `RS2_Desktop` `MinimizeTo` (as for RS2SHELL) says whether windows go to the viewer or the desktop |
| `rs2obj.rs` | rs2's own abstract classes for what IBM's shell keeps on the LaunchPad and in WPCONFIG: RS2Shutdown (opening it shuts down), RS2WindowList (the window list), RS2System (a Window page: where minimized windows go) |
| `filejob.rs` | Copying and moving file system objects as the user asks (drops, Copy..., Move...): a worker thread with a progress window and the name collision dialog (Rename with a free name, Replace, Skip, Cancel), ported from RS2SHELL; the target folder dialog of Copy..., Move..., Create shadow... and Create another...; the objects follow on the shell's thread when a job ends (paths, handles, folders; a moved or renamed folder's objects too; copies lose the original's object ID) |
| `sortinc.rs` | Folders' Sort page (name, type, real name, size, last write date, or as placed; always maintain sort order, setup ALWAYSSORT) and Include page (patterns matched against titles and real names; folders always shown), the view's Sort menu |
| `templates.rs` | Templates (TEMPLATE style, the General page's check box): opening one makes a new object beside it, dragging one makes it at the drop, Create another... makes one where the user says; the new object's title is edited in place |
| `find.rs` | Find... on folders, drives and the desktop: a name pattern, all subfolders; a worker thread walks the directories, and a Find Results window shows the objects (and the matching abstract objects of awake folders) |
| `clsmgr.rs` | WPClassManager (merged in as SOM's class manager; finds classes in the DLLs registered in OS2SYS.INI's PM_Objects ClassTable) |
| `api.rs` | WinCreateObject, WinQueryObject, WinSetObjectData, WinDestroyObject, WinOpenObject, WinSaveObject, WinMove/CopyObject, WinCreateShadow, WinQueryObjectPath, WinRegister/Deregister/Enum/ReplaceObjectClass |

**Persistence** follows IBM's layout of OS2.INI:
- Abstract objects are under `PM_Abstract:Objects`, and the folders they
  are in under `PM_Abstract:FldrContent`.
- File-system handles are under `PM_Workplace:Handles`, and object IDs
  under `PM_Workplace:Location`.
- File-system objects keep their state in the `.CLASSINFO` EA.
- On a file system without long names (FAT) the file gets a short real
  name, and the title goes in the `.LONGNAME` EA, also when only the
  case differs.

The values in the state record are rs2's own format (wps.rs).

**Where objects live.** The objects run in the process that calls
`Rs2WpsStart` (PMWP.9100), which will be the shell. In such a process
WinCreateObject makes real objects. Elsewhere it still adds program
objects to RS2SHELL's program list, as before.

**Tests.** `RS2\WPSTEST.EXE` makes a folder on the desktop, a program,
a data file, a subfolder and a shadow. It changes, moves and copies
them and checks OS2.INI. `WPSTEST CHECK` then runs as a new process:
it wakes the objects from OS2.INI and their EAs, checks them, and
deletes them.

`WPSTEST CLASS dll class` registers a class DLL and makes an object of
it. The Toolkit's samples, compiled for IBM's PMWP, work as subclasses
of rs2's classes:
- Car (WPDataFile): its methods and instance data, and its metaclass's
  class title loaded in wpclsInitData;
- TextFolder (WPFolder);
- WPSTutorial (WPDataFile, which overrides about a hundred methods).

Set `PMWPDEBUG` to trace object creation.

**Views** (`view.rs`, `menu.rs`, `book.rs`, `dnd.rs`) run in the
shell's process, on its PM thread.
- **Folder windows** are a frame with a WC_CONTAINER in icon, details
  or tree view. Each record keeps its object. WPDesktop's view covers
  the screen below every other window.
- **Settings notebooks** are a WC_NOTEBOOK that `wpAddSettingsPages`
  fills through `wpInsertSettingsPage`. A PAGEINFO names either a dialog
  in a class DLL or a ready window. rs2's own pages are General (the
  title) and WPProgram's Program page.
- **Pop-up menus** follow IBM's sequence with IBM's menu ids:
  `wpDisplayMenu` → `wpFilterPopupMenu` → `wpModifyPopupMenu` (classes
  add items from their DLLs with `wpInsertPopupMenuItems`, also into the
  Open submenu) → `wpMenuItemSelected`.
- **Direct editing** of a title calls `wpSetTitle`.
- **Drag and drop** goes through `wpFormatDragItem`, `wpDragOver` and
  `wpDrop`:
  - Folders move, copy or link (shadow) objects, and take other
    programs' DRM_OS2FILE files.
  - Programs start with the dropped files.
  - Shadows and drives pass drops on to their objects.
  - Icons dragged within their own view move.
- Open views follow changes: `wpAddToContent` and `wpDeleteFromContent`
  add and remove records, and `wpSetTitle` and `wpSetIcon` update them.
- **Icon positions** are kept in the folder's `.ICONPOS` EA (rs2's
  records: x, y and a name, the file's name or `#` with an abstract
  object's handle). An `@` record holds the height the positions were
  measured at, so icons keep their distance from the top when the
  window changes size. New objects go to free grid places; Arrange
  lays out every icon again and keeps the result.
- **Tree views** show the folder's folders (and drives' root folders,
  and the folders shadows point to). Each folder opens on demand, in
  place of a placeholder record.
- **Shadows' titles** are owner-drawn in the shadow colour: blue, light
  blue on dark backgrounds, grey when the original is gone.
- **Folder settings** add a View page (open in icon, details or tree
  view) and a Background page (a colour with presets).
  `wpAddFolderView1Page` and `wpAddFolderBackgroundPage` provide them.
  The colour is the folder's own state and applies to its open views.
- **Keyboard:** Delete deletes (after asking), F2 renames, F5 refreshes,
  and Shift+F10 shows the menu.

`RS2\WPSHELL.EXE` starts PM and the shell's objects, opens the desktop
and runs the message loop. It is a test shell until the shell process
(phase 3) exists:
- `WPSHELL DEMO` makes a folder with a program, a file and a shadow, and
  opens its views and the program's settings.
- `WPSHELL OPEN` opens only the folder.
- `WPSHELL CAR` puts the Toolkit's Car sample on the desktop. Its menu
  items ("Beep horn", "Open car" in Open) and its Dashboard and Horn
  Beep pages come from CAR.DLL.

Checked in QEMU with screenshots and pointer input
(`--type "{mouse_abs x y}{mouse_button n}{shot NAME}"`): menus, a file
dragged from a folder to the desktop, and a double-click starting a
program.

## The shell process

`OS2\PMSHELL.EXE` (`userland/pmshell`) does what IBM's PMSHELL does:
it starts PM, calls PMWP's `ShlStartWorkplace(hab, hmq)` (ordinal 211)
and runs the message loop. Without PM it starts the command prompt.
If a Workplace Shell is already running, it ends.

`ShlStartWorkplace` (`shell.rs`) does the following:
- **Builds the classes.** PMWP's classes are also built whenever the
  class manager is asked for one.
- **Makes the first start's objects** (`install.rs`). These are OS2.INI's
  `PM_InstallClass` and `PM_InstallObject` entries, as MAKEINI writes
  them from INI.RC; they are removed once done:
  - The key is `title;class;location;flags` and the value is the setup
    string. `?:` stands for the boot drive.
  - The flags are PRESERVEOLD, UPDATE, REPLACE, UPDATEONLY and DELETE,
    applied against an existing object with the same OBJECTID.
  - Objects whose folder is made later wait for it.
  - The very first time, rs2's own entries come first: the desktop,
    Drives, Command Prompts (OS/2 Window, OS/2 Full Screen), Programs
    (the System Editor with its ASSOCFILTER, the application installer,
    Install rs2, where their files exist; the installer's program is on
    the live media only, so an installed system drops its object), Startup, the Minimized Window
    Viewer, Window List, System Setup (`<WP_CONFIG>`: System, and
    Display where it exists), Shut Down and Templates (Data File,
    Folder, Program). `PM_Workplace` `Rs2Desktop` holds the version made
    (3); a desktop of an earlier version gets
    the entries again, and PRESERVEOLD keeps what it has. Titles break
    lines at `^`, as in INI.RC.
- **Starts the server** for other processes' object calls (`ipc.rs`):
  - A caller takes the mutex `\SEM32\RS2WPS\CALL` and writes its
    request into `\SHAREMEM\RS2WPS.MEM`.
  - It posts `WM_WPS_CALL` to the shell's object window. Posting needs
    no message queue, so text-mode programs can call too.
  - It waits on the event `\SEM32\RS2WPS\DONE`, which the shell posts
    when the call has run on its PM thread.
  - WinCreateObject, WinQueryObject, WinSetObjectData,
    WinDestroyObject, WinOpenObject, WinSaveObject, WinMove/CopyObject,
    WinCreateShadow and WinRegister/DeregisterObjectClass go this way.
  - Without a shell running, WinCreateObject still adds programs to
    RS2SHELL's program list.
- **Opens the desktop.**
- **Opens the Startup folder's objects** (`<WP_START>`, a WPStartup),
  once per PM session (the system atom `WPS_STARTUP`).
  `SET RESTARTOBJECTS=NO` turns this off.

The desktop's pop-up menu adds **Window list** and **Shut down**.
Shutting down (`shutdown.rs`, RS2SHELL's sequence) asks first, then:
- ends the text sessions;
- sends every program WM_SAVEAPPLICATION and WM_CLOSE, and waits,
  offering after 20 s to end those still running;
- runs DosShutdown, then powers off (ACPI S5) or restarts.

`WinShutdownSystem` (ordinal 149) does the same without asking.

**Checked in QEMU** with screenshots and pointer input:
- the first start's desktop;
- an OS/2 Window opened from Command Prompts running `WPSTEST REMOTE`,
  whose object calls ran in the shell and whose object appeared on the
  desktop;
- shutdown through the desktop menu, as far as the power-off;
- the Startup folder (`WPSTEST STARTUP`, then PMSHELL in the same boot:
  the System Editor started).