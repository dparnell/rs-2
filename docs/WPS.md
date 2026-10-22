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
  `.LONGNAME`, `.TYPE`, `.ICON` and `.ICONPOS` for folders.
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

1. **SOM runtime** (`userland/som` → SOM.DLL, IBM ordinals): SOMObject,
   SOMClass, SOMClassMgr, `somBuildClass` with the full
   `somStaticClassInfo`, mtabs, method and data thunks,
   `somParentNumResolve`, ids, `somResolveByName`, `somApply` and the
   environment. Test on the host where possible, then on rs2 against
   IBM's SOM.DLL, running the same small class DLLs built with the
   Toolkit's `sc.exe`.
2. **Core classes in rs2's PMWP.DLL**: WPObject, WPAbstract,
   WPFileSystem, WPDataFile, WPFolder, WPProgram, WPShadow, WPDesktop,
   WPDrives, WPDisk, WPRootFolder, WPTransient and WPClassManager. They
   need ClassData exported in IDL release order, persistence in OS2.INI
   and `.CLASSINFO` EAs, object IDs, setup strings, WinCreateObject,
   WinSetObjectData and WinOpenObject, views on WC_CONTAINER, pop-up
   menus, settings notebooks, and drag and drop. RS2SHELL's code
   (copying, shadows, the tree view, DrgDrag) carries over.
3. **The shell process**: a PMSHELL-compatible start-up building the
   desktop from INI.RC/MAKEINI on the first boot, the Startup folder,
   shutdown. RS2SHELL stays the default until this works; the switch is
   `RUNWORKPLACE`.
4. **Compatibility**: wpstutor.dll registered with WinRegisterObjectClass
   and working; then the remaining classes (Templates, Find, Launchpad,
   Shredder, Minimized Window Viewer, Network); then rs2 versions of
   WPCONFIG and WPPRINT (System Setup and printers).

Phase 1 is self-contained and can run in parallel with preparatory
phase 2 work: generating the ClassData layouts and method lists from the
IDL files.
