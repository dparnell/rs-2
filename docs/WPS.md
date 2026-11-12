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
| WPSH.IR, SOM.IR | `unpacked/disk_7/OS2/ETC` | interface repositories (rs2 makes its own: docs/formats/som-ir.md) |
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
| SOMU.DLL | 81 KB | SOM utility classes; PMWP uses only SOMMBeforeAfter (rs2's: [below](#rs2s-somtc-somu-and-somir)) |
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
   and Find, IBM's settings pages (View with the folder's font, icon
   arrangement and details columns; File, Type, Window, Association,
   Session, Details), deleting with IBM's confirmations and the lazy
   writer.
3. **The shell process**: done (see "The shell process" below). It is
   the default desktop (`PROTSHELL=C:\OS2\PMSHELL.EXE`; `xtask run
   --desktop`), with what RS2SHELL's desktop had: the Minimized Window
   Viewer, the Window List, System Setup and Shut Down.
   `PROTSHELL=C:\OS2\RS2SHELL.EXE` still starts RS2SHELL.
4. **Compatibility**: done as far as rs2 has what the classes stand
   for. The Toolkit's wpstutor.dll is registered in the running shell
   with WinRegisterObjectClass and works (its menu item, its own view,
   its settings, SHOWDESC.EXE showing the methods it is called through).
   The LaunchPad, Shredder and Network folder are there, and rs2's
   versions of WPCONFIG's System, System Clock, Mouse, Keyboard, Sound
   and palettes, and of WPPRINT's printer objects (see "What is not
   there" below for the rest).

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
  (`SOMInitModule`, then `<Class>NewClass` when that did not make the
  class, as IBM's does). Its methods are called
  through tokens, so a subclass such as the WPS's WPClassManager can
  override them. `somMergeInto` moves the registry and makes the target
  `SOMClassMgrObject`; the old manager forwards to it.

- **Abstract inheritance.** A parent whose `inherit_vars` bit is clear
  gives only its interface: none of its data (nor its ancestors', unless
  another parent passes them on), its methods (SOMObject's too) run a
  stub that raises SOMError 20059, and the class's initializers and
  destructor skip it; without the first parent's bit the class's own
  block and data come first. Data tokens for such a class raise 20109
  (twice, as IBM's). Instance sizes are where the last data ends (not
  rounded up); somGetInstanceOffset is 0. Overriding a method the class
  does not have fails somBuildClass (20169, class freed).
- **Casting.** somCastObj gives the object a cast class ("Dog->Animal",
  made by the private somCloneClass once per ancestor, not registered)
  whose methods are the ancestor's; somResetObj (or a cast to the true
  class) puts the true class back. These go through IBM's private
  SOMClass methods somCastObjCls, somResetObjCls, _get_somTrueClass and
  _get_somCastedClass.
- **Unregistering and unloading.** Registry entries count the
  registered classes that have the class as a parent or metaclass (it
  then cannot be unregistered: 1) and mark persistent classes
  (somBegin/EndPersistentClasses). The classes registered while
  somLoadClassFile loads a DLL, or somRegisterClassLibrary initializes
  one, form an affinity group led by the class that caused it
  (somGetRelatedClasses; somJoinAffinityGroup). somUnregisterClass frees
  the class object (SOMClass's somUninit clears its ClassData, so
  `<Class>NewClass` builds it again); the leader unloads its whole group
  and somUnloadClassFile frees the DLL (SOMDeleteModule; -2 when there
  was none). Registered classes are listed newest first.
- **The private methods.** SOMClass's somcPrivateN entries are IBM's
  real methods, under IBM's names: somGetMethodOffset,
  somSetMethodDescriptor, somCloneClass, somLookupTerminal,
  set_somDirectInitClasses, the instance init/destruction/assignment
  masks, the cast methods above, somRegLPMToken, somAddMethod,
  _get/_set_somCClassData, the class data order and entries (IBM's
  index stride, (n+1)*32, kept), _get_somDirectAssignClasses,
  setUserPCallDispatch, _get_somClassAllocate/Deallocate; SOMClassMgr's
  are somReleaseClasses and somRegisterThreadUsage.
- **Redispatch.** somGetRdStub makes a redispatch stub (it calls the
  object's somDispatch with a va_list) when the compiler gave none;
  somOverrideMtab routes every static method but somDispatch through
  them. somDispatch and somClassDispatch take the method from the
  class's somGetMethodData through its token and call somApply, and
  somGetClassName/somIsA/somRespondsTo ask the class through its tokens,
  so metaclasses can answer instead: SOMU.DLL's cooperative metaclasses
  (SOMMBeforeAfter with its sister classes, SOMMTraced) work.
  somCreateDynamicClass (`_Optlink` in IBM's DLL: className, parentName,
  file, major, minor) makes a data-less subclass that way.
- **The interface repository.** somLocateClassFile asks the repository's
  `lookup_modifier(class, "dllname")` once there is one (made from
  SOMIR.DLL's Repository, or when SOMIR is set), falling back to the
  class name (IBM's SOMIR gives the same answers there);
  _get_somInterfaceRepository makes the manager's repository on first use
  and hands out a copy. SOM's own methods have IBM's descriptors
  (`::SOMClass::somNew`).
- **_Optlink exports.** Besides somCreateDynamicClass, IBM's
  somSetOutChar, somfixMsgTemplate and somPCallResolve take their
  arguments in EAX, EDX, ECX. somSetOutChar sets the calling thread's
  output routine, which somPrintf & co. use while SOMOutCharRoutine is
  the default; it gets each string's terminating 0 too, as from IBM's
  somOutputString.
- **Classes in modules.** A descriptor's scope is its class's name
  without the module (`CosSecurityAuthn::Vault::instantiate_persona`
  overridden in `SOMSecurityAuthn::SOM_Vault`): IBM's SOMSEC.DLL needs it.

Not done: SOMObject's private somComputeForwardVisitMask and
somComputeReverseVisitMask; somSubstituteClass only redirects lookups
(IBM's also rewrites the replaced class's ClassData); a class built again
under a registered name replaces it (IBM's reuses the class object).

**Tests.** `RS2\SOMTEST.EXE` builds classes from layout-4 static class
information as compiler output does. It covers:
- emitted initializers and destructors, and legacy somInit;
- parent calls, data tokens, and a metaclass with class methods and data;
- multiple inheritance with a shared base, and a derived metaclass;
- ids, dispatch by name, and the class manager.

It also checks abstract inheritance, casting, unregistering (subclasses,
metaclasses, persistent classes, ClassData cleared and the class built
again, somUnloadClassFile) and somCreateDynamicClass. It passes on both
rs2's and IBM's SOM.DLL. Run it from a directory that holds IBM's DLL;
LIBPATH starts with `.`, but **each DLL needs its own boot**: rs2's
loader keeps using the SOM.DLL first loaded in the system, whatever the
next program's directory. IBM's SOM showed the destruction order and
the behaviour of all the above.

`SOMTEST BA` (with IBM's SOMU.DLL, SOMTC.DLL and SOMIR.DLL in the
directory) builds a class whose metaclass is derived from SOMU's
SOMMBeforeAfter and checks that its method runs between the before and
after methods. `SOMTEST IR class...` (with SOMIR set) shows the DLL
somLocateClassFile finds for each class. `SOMTEST SOMD` checks SOMD.DLL's
classes and SOMD_Init (rs2's stub or IBM's, which needs SOMEM, SOMS and
SOMSEC beside it). `SOMTEST IRW SET` changes a constant, a modifier and an
operation's mode in the last file SOMIR lists and saves it; `SOMTEST IRW
CHECK` (another process) finds the changes: run with IBM's SOMIR on one
side and rs2's on the other (two boots, the file on a test disk: `cargo
xtask disk`, `--hdd`), each reads the other's updates.

`SOMTEST LOAD dll class` loads a class library through the class
manager and makes an instance. IBM's SOMU.DLL classes (SOMUTStringId,
SOMMBeforeAfter, SOMMSingleInstance, SOMMTraced, SOMBuf) and SOMIR.DLL's
interface repository classes load and work on rs2's SOM. Freeing an
uninitialized repository object faults on both SOMs alike.


## rs2's SOMTC, SOMU and SOMIR

SOM's class libraries, binary compatible with IBM's (studied in IBM's DLLs
with Ghidra: there are no symbols for them, the class structures came from
each `somStaticClassInfo`): the same module names and ordinals, the same
exported class data (IBM's class data order, which for some classes is
not the IDL's release order: SOMMCooperative, SOMMTraced, Contained,
Container, SubRepository), each class's parents, metaclass, version and
instance data size, and IBM's calling conventions (a few exports are
VisualAge C++'s `_Optlink`: arguments in EAX, EDX, ECX). `userland/somsup`
is what they share: SOM.DLL's calls and the class building
(`somBuildClass` with layout-4 static class information, as compiler
output does; methods returning structures get a `somApRdInfo` saying so,
which rs2's SOM needs to make their tokens take the object after the
result's address).

| DLL | What it has |
|-----|-------------|
| SOMTC.DLL (`userland/somtc`) | TypeCodes: IBM's layout (`{ kind, alignment, 1, flags, parameters }`), tcNew/tcNewVL, tcKind, tcSize, tcAlignment (VisualAge's default packing: double 4), tcEqual, tcParameter, tcParmCount, tcCopy (one block), tcFree, tcSetAlignment (constants are never changed), tcPrint (IBM's text), tcSequenceNew, tcSeqFromListString, the `TC__` constants and the repository's description TypeCodes, the TCMalloc variables |
| SOMU.DLL (`userland/somu`) | All 16 classes and IBM's private SOMMProxy metaclasses: SOMMBeforeAfter, SOMMTraced, SOMMSingleInstance, the cooperative metaclasses, SOMBuf (IBM's gap buffer), somutMsgManager, SOMUTId/SOMUTStringId, SOMDesc, SOMArgList and SOMCommand; UUIDs, the SOMStartThread variables, the heap watcher (somWatchStart puts `somwatch*` into SOMMalloc & co.), the externalization functions |
| SOMIR.DLL (`userland/somir`) | The interface repository read from `.IR` files ([formats/som-ir.md](formats/som-ir.md)), the files `SOMIR` lists (rs2's CONFIG.SYS: `C:\OS2\ETC\SOM.IR`, rs2's own), and updated (saveIR) |
| SOMD.DLL (`userland/somd`) | DSOM's client library as a stub (below) |
| `\OS2\ETC\SOM.IR` | rs2's interface repository: the interfaces of SOM.DLL's, SOMU's, SOMIR's and SOMD's classes and the types they use (below) |

**Before/after metaclasses.** IBM's SOMMBeforeAfter is built on its
cooperative metaclass framework: SOMMCooperativeRedispatched's
somClassReady points the class's method table at redispatch stubs, the
stubs call somDispatch, and the metaclass's cooperative somDispatch calls
the before methods, the method (found in a "sister class", a copy made
with SOM's private class methods) and the after methods. rs2's SOMU keeps
the classes but does the work directly: SOMMCooperativeRedispatched's
somClassReady replaces each method of the class (except the dispatch
methods, and somInit/somUninit, which rs2's SOM keeps as legacy
initializers) with a stub (`push wrap; jmp ba_entry`). For an instance
whose class's metaclass descends from SOMMBeforeAfter the stub calls the
sommBeforeMethod implementations (`(class, ev, object, methodId, ap)`, ap
pointing at the object among the arguments), in IBM's dispatcher order
(sommSetBAMethodSeq: a metaclass's own, else its parents' left to right;
the after methods right to left), the method if every before method
returned TRUE, and the after methods of those that ran, in reverse, with
the result's address (EAX:EDX, or ST0 when a floating point value came
back). The method runs on the caller's arguments: the stub swaps the
return address for its own and keeps the real one on a per-thread stack.
SOMMTraced prints, in IBM's formats, `method( Class at 0x..., arguments
) called` and `method returns value` when `SOMM_TRACED` is empty or names
the class; the arguments and the result come from the interface
repository through SOMDesc (`somIsA( Traced at 0x00086aa8, SOMMTraced at
0x00086ab8 ) called`, `somGetClassName returns "Traced"`), else `, ...`.
A method without a descriptor is looked up as `IntroducingClass::method`,
the class from its token. IBM's SOMU works on rs2's SOM as on IBM's
(its framework uses SOM's private class methods: sister classes,
redispatch stubs), its SOMMTraced output the same as rs2's; nested calls
are indented by rs2's through somPrintf, by IBM's through the C library's
stdout (which shows them later, if at all).

**SOMMSingleInstance:** SOMMProxy::HiddenClassMeta's somNew and
somNewNoInit return the class's one instance, counting references;
somFree on it (put in the class's method table at somClassReady) frees it
with the last one, as IBM's.

**The interface repository.** A Repository (`somNew` on Repository, or
M_Repository's createRepository) reads the files once; one SubRepository
per file holds its top level. Definitions are made from their records
when first looked up and kept (somFree counts down a use, as IBM's does
until the last); getters return the object's own strings and TypeCodes;
`describe` returns a description whose value is one SOMMalloc'd block
(the structure, its strings and TypeCode copies), as IBM's. Lookups
follow IBM's internalNameLookup: `lookup_name` with levels 1 searches the
container, -1 also nested modules, interfaces and operations, both also
the enclosing scopes; an interface's contents include its base
interfaces' unless `exclude_inherited`; `within` of something in an
interface lists the interface and the interfaces derived from it.

**Updates.** Repository::saveIR writes what the `_set_` methods changed
back into the last file SOMIR lists, as IBM's saveToIR: a definition's
variable record is written again in space from the file's free list
(first fit, a larger block split) or at its end, its old record becomes
a free block at the head of the list; modes are rewritten in the fixed
record, a new name of the same length in the fixed record and its
container's contents (a longer name is not saved); the file is written
out whole. irEnableUpdates and irDisableUpdates do nothing, as IBM's.

**rs2's SOM.IR.** xtask writes `\OS2\ETC\SOM.IR` (xtask/src/somir.rs) from
`userland/somir/som-ir.txt`, a text form of the interfaces (one
definition per line, TypeCodes written out). `tools/scripts/gen_som_ir.py
REPO` makes the text from the Toolkit's IDL (som/include) of the classes
rs2 ships, read as SOM's compiler reads it (`__SOMIDL__`, `#include`,
`#pragma modifier`, foreign types from SOMFOREIGN typedefs with `impctx`)
and arranged as IBM's compiler puts it in a repository: an interface's
attributes become `_get_`/`_set_` OperationDefs (with an `attribute`
modifier) first, then come its operations, its other definitions and the
AttributeDefs; the attributes without `nodata` are its instance data. A
modules.txt line putting another file at `OS2\ETC\SOM.IR` replaces it
(development).

**SOMDesc and SOMArgList** describe a method from the repository (its
OperationDef, or the AttributeDef of a `_get_`/`_set_` accessor, the
interface's `callstyle`: idl 2, oidl 1) and take a call's arguments apart
with it (IBM's: the environment first for idl, floating point arguments 8
bytes). SOMDesc's argument sizes are the TypeCodes' sizes;
somutGetOperationModifier gives a modifier's value ("" for one without),
the ownership methods answer from `object_owns_result`,
`caller_owns_result`, `object_owns_parameters`, `caller_owns_parameters`
and `memory_management = corba` as IBM's (2 the object, 1 the caller, 3
unspecified); there is no description cache to flush.

**SOMCommand** is a method call written out as text (userland/somu
cmd.rs, IBM's format): the method's and the class's names (`%10d%s`
each), the target object, then the argument list as somutGetArgs writes
it: the count, then per argument a type letter as a `%10d` number and the
value (`b` char/octet, `s`/`S`/`l`/`L`/`B` as `%10d`, `f`/`d` as `%20f`,
`z` a string and `o` an object with their lengths). An object is written
by the first of three methods its class has (somutExtCommand's defaults:
somrGetObjName, sompGetPersistentIdString, somrGetState): `R` and its
name, `P` and its persistent id, or `L`, its class's name and version and
its state; only the `L` form is read back here (the others need the
replication and persistence frameworks). somutDoCommand calls the method
with somDispatch (somClassDispatch for another class).

**SOMD.DLL** (userland/somd) is DSOM's client library as a stub: nothing
rs2 ships uses distributed objects, but IBM's Object REXX imports it. It
has IBM's 138 exports and ordinals and builds, when loaded as IBM's does,
the 25 classes IBM exports class data for (SOMDObject, SOMDClientProxy
with its SOMDMetaproxy metaclass, ObjectMgr, SOMDObjectMgr, SOMDServer,
ORB, BOA, SOMOA, NVList, Request, Context, ImplRepository,
ImplementationDef, Principal, ...) with IBM's parents, metaclasses,
versions, instance sizes and class data order (`classes.rs`, generated
from a study of IBM's DLL; Principal's parent is SOMObject, not SOMSEC's
CosSecurityAuthn::Principal). Every method and procedure method raises
`StExcep::NO_IMPLEMENT` in the caller's environment and returns 0;
SOMD_Init makes SOMD_ObjectMgr (a SOMDObjectMgr), SOMD_ORBObject and
SOMD_ImplRepObject, SOMD_Uninit frees them; the string, byte order and
environment helpers work. IBM's SOMD.DLL imports PMWP.DLL, so Object REXX
processes had PMWIN attached; with rs2's they do not, and IBM's REXXUTIL
looks PMWIN up with DosQueryModuleHandle before loading it, which rs2's
kernel answers only for modules the process uses.

Not done: IBM's private methods (SOMIR's record allocator and its
saveToIR family, the persistence helpers) remain entries that return 0;
nothing outside IBM's own DLLs calls them.

**Tests.** `RS2\SOMTEST.EXE` checks TypeCodes (SOMTC), every SOMU
class's name, parent, class data and class data order, a before/after
metaclass (before and after called once, the result seen by the after
method, a before method returning FALSE skipping the method), SOMMTraced's
output, a single instance class, SOMBuf and UUIDs, and, when `SOMIR` is
set, SOMDesc's modifiers and ownership, an argument list written as text
and read back, a SOMCommand run, and the repository (rs2's or IBM's
SOM.IR: lookups, modifiers, descriptions, inherited operations, an
attribute, a TypeDef and a ConstantDef). Run from a directory holding
IBM's SOMTC, SOMU and SOMIR (with rs2's SOM), or IBM's SOM too, the same
checks pass with the same output. IBM's SOMEM.DLL (the event
manager, a SOMMSingleInstance class) loads and makes its SOMEEMan on
rs2's SOMU (`SOMTEST LOAD SOMEM SOMEEMan`). `tools/scripts/irdump.py`
prints an `.IR` file.

## rs2's Workplace Shell classes

`userland/pmwp` (PMWP.DLL) provides the 58 classes IBM's PMWP exports
that the Toolkit describes, M_WPMinWinViewer, IBM's WPMinWindow (which
the Toolkit does not describe), classes IBM keeps in other DLLs (WPUrl
and WPUrlFolder from WPINET.DLL, Mindex from MINXOBJ.DLL, WPCONFIG.DLL's
WPMouse, WPKeyboard, WPSound, WPPalette, WPColorPalette,
WPSchemePalette, WPFontPalette, WPSystem, WPClock, WPCountry, WPPower and
WPSpecialNeeds, WINCFG.DLL's WPWinConfig, WPPRINT.DLL's
WPPrinter, WPJob, WPPdr, WPQdr, WPPort and WPSpool, and PNP.DLL's
WPHwManager and WPDevice classes) and rs2's own
RS2Shutdown and RS2WindowList (and RS2System,
the System object of rs2's desktops before version 9, kept so that it
can wake to be replaced).

**Class tables.** `tools/scripts/gen_wps_classes.py` reads the Toolkit's
IDL (`wp*.idl` and `wincfg.idl`). It writes `src/classes_gen.rs` (each class's parents, metaclass,
version and methods in release order, plus index constants). It also
writes `src/classdata_gen.rs`: the exported `<Class>ClassData` and
`<Class>CClassData` variables and the `<Class>NewClass` functions, with
IBM's ordinals from `ordinals.txt`. WPImageFile is not in the Toolkit,
so WPBitmap derives from WPDataFile. The IDL's conditionals are read as
SOM's compiler for Intel reads them (`__SOMIDL__` defined, `_PPC_` not).
The script's `EXTRA` list adds the classes of IBM's other DLLs that
PMWP.DLL provides and the classes the IDL does not describe
(WPMinWindow and rs2's own, with no methods of their own), and
metaclasses the IDL describes come along with
their classes even where IBM exports no class data for them. An OIDL
metaclass without parents in the IDL (M_WPPrinter) gets its class's
parent's metaclass.

**Building.** `wps.rs` builds each class as SOM compiler output does:
somBuildClass with a layout-4 `somStaticClassInfo`, one static method
per release-order entry. Methods without an implementation return 0.
The rest comes from the class modules:

| Module | Classes |
|--------|---------|
| `obj.rs` | WPObject (title, style, icon (an `ICONFILE=` or `ICONRESOURCE=` icon is kept with the object's state and loaded again when it is restored), folder, default view, handle, object ID, setup strings, save/restore, copy, move, shadow; the lazy writer: wpSaveDeferred queues the object and the desktop's timer saves the queue a second later, shutdown saves what is left, and without a desktop objects save at once; the use list as IBM's: USEITEMs added with wpAddToObjUseList and found with wpFindUseItem, wpAllocMem's blocks, open views that wpSwitchTo and wpViewObject find; wpRegisterView gives a class's own view the object's title, icon and a window list entry; the open behavior, CCVIEW= and wpQuery/SetConcurrentView, else the System object's) and M_WPObject (wpclsNew, class titles, icons and views, objects from handles and locations; somClassReady calls wpclsInitData) |
| `delete.rs` | Deleting as IBM's PMWP does (studied with Ghidra): wpQueryConfirmations reads `PM_ControlPanel` ConfirmDelete, ConfirmSubDelete, ConfirmRenameFilesWithExt, ConfirmKeepAssoc, ConfirmCopyMoveEtc, DisplayProgressInd, AllowForceDelete and ConfirmForceDelete (all on when missing but ConfirmCopyMoveEtc); the Delete menu item, the Delete key and drops on the Shredder show the Delete Object(s) dialog (location, objects, Confirm on folder delete, Confirm on object delete), then call each object's wpDelete, whose wpConfirmDelete asks with IBM's texts (an object; a folder "and all of its contents"; a shadow, and what it is a shadow of) with Yes, Yes to all (with more objects to come), No (the default) and Cancel; a folder deletes its contents (wpDeleteContents, each asked about) before itself. The other confirmations: a data file renamed to another extension asks whether to keep it (IBM's texts for data and program files), a drop asks before copying or moving, and the copy and move progress window shows only with "Display progress indication dialog". WPShredder (`<WP_SHRED>`): objects dragged onto it are deleted that way once the drop is answered; other programs' items that offer DRM_DISCARD get DM_DISCARDOBJECT (DRR_ABORT stops the asking), as IBM's ShredderThread does; its only view is its settings, without a Window page |
| `system.rs` | WPCONFIG's WPSystem (`<WP_SYSTEM>`, studied with Ghidra; IBM's tabs and dialogs: Confirmations (IBM's five check boxes), Title (duplicate titles: prompt, auto-rename or replace, `PM_ControlPanel` NameClash, which copies and moves follow: filejob.rs renames or replaces without asking), Window (button appearance: hide or minimize button, `MinButtonType`, PMWIN's Rs2SetMinButtonType gives frames a hide button where they ask for a minimize button; minimize button behavior: hide the window, the viewer or the desktop, `HiddenMinWindows`; animation, `Animation` and SV_ANIMATION; object open behavior, ConcurrentView), Print Screen (`PrintScreen`, SV_PRINTSCREEN) and Logo (indefinite, none or timed in seconds and tenths, `LogoDisplayTime` in ms, as programs read it), Folder (three pages, WPCONFIG's dialogs 1307, 1303 and 2107: long or short menus and folder menu bars, `PM_Workplace` FolderMenus `LONG`/`SHORT` and FolderMenuBar `ON`/`OFF`, which open folder windows follow at once; the default folder view, DefFolderView `ICON`/`TREE`/`DETAILS`, for folders without a default view of their own; automatic close, `PM_Workplace:FolderAutoClose` Objects `NONE`/`SUBFOLDERS`/`ALL`), Input (WPCONFIG's PmInputDlgProc, dialog 1302: full window dragging, Alt+Tab switching window, VIO mouse actions, asynchronous focus change and its delay in tenths, type-ahead; each a system value (99-103, SV_LOCKSTARTINPUT) set at once with WM_SYSVALUECHANGED broadcast, and a ULONG in `PM_ControlPanel` FullWindowDrag, AltTabWindow, VioMouseActions, AsyncFocusChange, AFCSensitivity, LockStartInput; IBM's defaults off, on, on, on, 20, off; rs2's window manager acts on them: PMWIN's access.rs, see "The Input page in the window manager" below), Screen (`screen.rs`: the resolutions the display driver can switch to, RS2API Rs2QueryDisplayModes, and the colours, 16 million or 256; Apply switches at once with Rs2SetDisplayMode and keeps the new resolution only if Keep is chosen within 15 seconds, as the Display program does, saving it as CONFIG.SYS `VIDEO=WxH[x256]`; page 2, IBM's DMQS display type page, shows the display's resolution, colours, line length and whether its driver can switch; page 3, dialog 1226, only on a 256-colour screen: Workplace Shell palette aware, `PM_WorkPlace:Options` WPSPaletteAware) and Fonts (wpAddSystemFontSettingPages, rs2's page: the icon text font of folders and the desktop that have none of their own, `PM_SystemFonts` IconText, which open folder windows follow); M_WPSystem's wpclsQuerySetting/wpclsSetSetting with WPCONFIG's setting names and wpsystem.h's values; the shell gives PMWIN the settings when it starts) and WPClock (`<WP_CLOCK>`: a clock window, analog with hour and minute marks and a second hand, or digital, with the time, the date or both; its View page, kept with the object, and its Date/Time page, which sets the computer's clock with DosSetDateTime) |
| `syssetup.rs` | The System Setup classes IBM keeps in WPCONFIG.DLL and WINCFG.DLL that the modules below implement: their metaclasses' titles and rs2's own icons (pmwin::setupicons: a globe, a plug with a bolt, a key with a hand, a window in a window), and the entries that put Country (`<WP_CNTRY>`), Power (`<WP_POWER>`) and Special Needs (`<WP_SPECIALNEEDS>`) in System Setup once (`PM_Workplace` Rs2SystemSetup2), and WIN-OS/2 Setup (`<WP_WINCFG>`) once WIN-OS/2 is installed (Rs2WinOS2Setup), as IBM's INI.RC places Power and WIN-OS/2 Setup (Warp 3's placed Country; Warp 4's has GLOCALE's Locale, which rs2 also has) |
| `country.rs` | WPCountry (studied with Ghidra: cou_wpAddSettingsPages, CountryDlg, SetCountryData, WriteIntlWin): the Country page (the countries of rs2's NLS table by name; choosing one takes its conventions from DosQueryCtryInfo, as IBM's SetCountryData), Time (12- or 24-hour, separator, AM and PM strings), Date (order, separator) and Numbers (thousands and decimal separators, decimal places, list separator, leading zero, currency symbol and where it goes), with samples; Undo (as when the notebook opened) and Default (the country's own) on each; every change is written at once to OS2.INI `PM_National` (IBM's keys iCountry, iDate, iCurrency, iDigits, iTime, iLzero, s1159, s2359, sCurrency, sThousand, sDecimal, sDate, sTime, sList) and, with WIN-OS/2 installed, to WIN.INI's `[intl]` |
| `power.rs` | WPPower (the IDL's 33 methods in IBM's release order and its instance data first, as IBM lays it out): IBM's drives APM; rs2 has ACPI, which can turn the computer off but not suspend it. The Power page (power management on or off, wpSet/QueryPowerManagement, also `PM_ControlPanel` APM_BIOS_Enabled; confirm power state changes), the View page (refresh the status view automatically, every N seconds); the status view (OPEN_STATUS, its default view: power source, battery, power management, what it can do) and the menu's Status, Suspend and Power off (wpPowerOff asks when confirmations are on, then the shell shuts down and turns the computer off; wpSuspend says it cannot); wpChangePowerState with APM's states (3 off, 2 suspend); POWER= and CONFIRM= setup strings; the Resume and Geyserville pages are left out (APM wake-up events and SpeedStep) |
| `spneeds.rs` | Special Needs (WPSpecialNeeds, which IBM's Warp 4 no longer places, and the Keyboard object's Special Needs page, wpAddKeyboardSpecialNeedsPage; after IBM's SpecialDlgProc): on or off, the time-out after which it turns itself off without key presses, the acceptance delay, and the key repeat rate and delay while on. IBM's gives these to the keyboard driver (IMP32SetAIMProfile); rs2's to PMWIN as rs2-private system values 4000-4002 (its input thread ignores keys let go before the acceptance delay, see below) and the repeat to KBD$; kept in `PM_ControlPanel` (SpecialNeeds, SpecialNeedsTimeOut, SpecialNeedsAccept, SpecialNeedsRepeatRate, SpecialNeedsRepeatDelay) and applied when the shell starts |
| `wincfg.rs` | WIN-OS/2 Setup (WPWinConfig, studied with Ghidra in WINCFG.DLL: win_wpAddSettingsPages, WinDDEDlgProc, WinSessionDlgProc, ProgCatFromSessionButtons), only where WIN-OS/2 is installed (`PM_INSTALL` WINOS2_LOCATION): the Data Exchange page (DDE and clipboard public or private, OS2.INI `WINOS2` PM_DDESupport and PM_ClipboardSupport; kept: rs2's Win-OS/2 has no clipboard or DDE bridge yet, WINOS2.md) and the WIN-OS/2 Session page (the session type of WIN-OS/2 programs that do not choose one, PM_Global31SessionType: full screen PROG_31_ENH, a window in the common session PROG_31_ENHSEAMLESSCOMMON or in one of its own PROG_31_ENHSEAMLESSVDM; fast load, PM_Global31FastLoad, kept; WIN-OS/2 settings..., the DOS Properties dialog on PM_GlobalWindows31Settings, which rs2's launcher gives every Win-OS/2 session under the program's own settings); IBM's Session page for WIN-OS/2 3.0 shows the 3.1 page |
| `screen.rs` | The System object's Screen pages and Fonts page (see `system.rs`) |
| `connections.rs` | The LAN requester's objects ([LAN.md](LAN.md)): WPNetwork (`<WP_NETWORK>`: starts the requester, holds a WPNetgrp per workgroup; its Network page shows the requester's state), WPNetgrp (`\\GROUP`: the group's servers), WPServer (`\\SERVER`: its disk shares and print queues), WPSharedDir (`\\SERVER\SHARE`: a folder on the UNC name, which shows the share's files; Assign drive / Assign port and Unassign in its menu, the Drives folder refreshed) |
| `printer.rs` | WPPrinter on rs2's spooler ([PRINTING.md](PRINTING.md)): a printer object is a queue (QUEUENAME=, wpQuery/SetPrinterName, saved with the object; its title is the queue's description). It opens on its job view: the queue's jobs as job objects (WPJob, printobj.rs) in a view window whose title carries IBM's status (queue held, number of jobs), as icons or (View page) details: document, status, job id, size, date and time submitted, brought up to date at the View page's refresh interval (two seconds by default); the window's own pop-up menu is the printer's (Hold or Release, Delete all jobs, Set default). IBM's pages: View (physical name, default view Detail or Icon, the details to display, refresh interval; WPPRTMRI's dialog 20700), Printer driver (the installed drivers' objects, the default one, Job properties: DevPostDeviceModes, kept with the queue; Default printer), Output port (port objects: choosing one sets the printer's port, opening one edits its settings; Output to file; Install new port), Queue options (priority, the queue driver objects, job dialog before print, printer-specific format, print while spooling: PRQINFO3's fsType) and Print options (dialog 20600: the separator file, typed or found with Find file..., and the start and stop times in hours, minutes and AM/PM; equal times print at any time). Files dropped on it print (PostScript as it is, other files as text), as data files' Print menu item does on the default printer; jobs dragged from another printer's job view move to it (wpMoveJobObject: SplCopyJob, SplDeleteJob); opening or dragging the template shows IBM's Create a printer (name, driver, port or Output to file; Install new printer driver), which makes a queue and a printer (Cancel drops the object); deleting the object deletes its queue; when the shell starts every queue gets an object (`<RS2_PRINTER_queue>`) in the Printers folder |
| `printobj.rs` | WPPRINT's other classes (the IDL describes them): WPJob (transient, one per spool job: titled with its document and status; Change status (Hold, Release, Start again, Print next) and Delete in its menu; Submission data and Queue options pages (date, time, owner, size, comment; job id, position, priority, status); wpDeleteJob, wpHoldJob, wpReleaseJob, wpPrintJobNext, wpStartJobAgain, wpQueryPrintObject, wpQueryJobId, wpQueryJobFile, wpQueryJobType), WPPdr (a printer driver's device, from `PM_SPOOLER_DD`; opening it shows its printer properties), WPQdr (a queue processor, SplEnumQueueProcessor), WPPort (a spooler port; opening it edits its settings: FILE's output file, LPR's host;queue;port, IPP's host;path;port, COM's line settings), WPSpool (`<WP_SPOOL>` in System Setup, desktop version 10: Spool path, `PM_SPOOLER` DIR; Print priority, `PRIORITY`; Enable/Disable spooler in its menu, `SPOOL`), the Create a printer, Install New Port (LPRn, IPPn, FILEn) and Install New Printer Driver dialogs |
| `desktop.rs` | WPDesktop's own (IBM's `desk_` methods and lockup code, studied with Ghidra): the Lockup tab's three pages (password typed twice or screen saver mode; the lockup's background colour and its image (LOCKUPBACKGROUND=, LockupBitmap: an OS/2 or Windows .BMP, a bitmap array's first image, or any picture MMIO's image I/O procedures read: JPEG, GIF, PCX, TIFF, Targa; `image.rs`) drawn normal (centred), tiled or scaled N by N (IBM's ids 306-308 and the scale, kept in LockupOptions); automatic lockup after 1-99 idle minutes, lock on startup, partial or full screen, auto-dim), kept in OS2.INI `PM_Lockup` as IBM keeps them (`LockupOptions`, IBM's 46-byte record with the password XORed with a key byte; `LockupScreenSaverMode`; `LockupBitmap`), the setup strings AUTOLOCKUP, LOCKUPTIMEOUT, LOCKUPONSTARTUP, LOCKUPFULLSCREEN, LOCKUPAUTODIM, LOCKUPSCREENSAVERMODE, LOCKUPBACKGROUND and the Toolkit's wpQuery/Set methods; the lockup itself ("Lockup now" in the desktop's menu, WinLockupSystem/WinUnlockSystem (PMWP 283/282, from other processes through ipc.rs), the idle timeout through PMWIN's Rs2QueryIdleTime, lock on startup): a system-modal window (PMWIN gives it all input; Ctrl+Esc does nothing) over the screen or most of it, a padlock and IBM's texts; typing the password and Enter unlocks, a wrong one says so; auto-dim blanks it after two idle minutes; without a password the password dialog comes first. The Archive page (create an archive at each startup: OS2.INI, OS2SYS.INI, CONFIG.SYS, STARTUP.CMD and the desktop directory, its files with their extended attributes, into `\OS2\ARCHIVES\nnnnn`, three kept, ARCHIVES.LOG; the Recovery Choices settings, kept in `PM_Workplace:Archive`, are PMSHELL's: see "The shell process") and the Desktop page (Save Desktop properties, `PM_Workplace` PreserveDTSettings; the default drop operation, wpQuery/SetDefaultDropOp, kept with the desktop) |
| `usermenu.rs` | Menu pages (IBM's MenuDlgProc, Menu2DlgProc, fs_wpAddUserItemsToPopupMenu, studied with Ghidra): WPFileSystem's wpAddFileMenuPage (the user's items and cascade or conditional cascade menus on an object's pop-up menu, each item a program (a path, or a program object's ID) started with the object's path; IBM's ids from WPMENUID_USER; kept with the object's state) and wpAddUserItemsToPopupMenu; WPFolder's wpAddFolderMenu2Page (long or short menus, wpSet/QueryMenuStyle: short menus leave out Rename, Copy, Move, Create another and Create shadow for the folder's objects; folder menu bars, wpSet/QueryMenuBarVisibility, which the folder's windows follow at once; the desktop has none unless set) and wpAddFolderSelfClosePage (Window, page 2: opening an object from the folder's window closes it never, for subfolders only or for all objects; by default as `PM_Workplace:FolderAutoClose` Objects says) |
| `pages.rs` | IBM's settings pages that the other modules do not add: Window (WPObject: the object open behavior; WPCONFIG's objects, the Shredder, Shut Down and Window List leave it out, SETTINGS_PAGE_REMOVED), the three File pages of WPFileSystem (the subject in `.SUBJECT` and the physical name; dates, flags changed at once, sizes, a folder's objects; comments, key phrases and history in multi-valued `.COMMENTS`, `.KEYPHRASES`, `.HISTORY`), WPDataFile's Type page (`.TYPE`, wpQuery/SetType; IBM's standard types and those OS2.INI associates), the Association page of programs and program files (`PMWP_ASSOC_TYPE` and `PMWP_ASSOC_FILTER`, ASSOCTYPE= and ASSOCFILTER=, wpQuery/SetAssociationType and -Filter; data files open with their types' programs, then their names'), WPProgram's Session page (OS/2 or DOS, full screen or window), WPProgram's Compatibility page (rs2's own: the colours the program sees, the display's or 256; see `abs.rs`) and a drive's Details page (file system, label, serial number, allocation units and bytes, from DosQueryFSInfo) |
| `abs.rs` | WPAbstract, WPProgram (EXENAME, PARAMETERS, STARTUPDIR, PROGTYPE; rs2's RS2COLORS=256 or DEFAULT, the Compatibility page's colours, saved with the object and given to the program as `RS2COLORS=256` in its environment, which wpQueryProgDetails reports in pszEnvironment and wpSetProgDetails reads back; wpQuery/SetProgDetails; opening starts the program, also from a data file or a dropped file, with that environment), WPShadow (SHADOWID; shows and opens its object), WPDisk (a drive: its settings are its own, its other views and its default view are its root folder's; its menu's Check disk... and Format disk... start rs2's PMCHKDSK and PMFORMAT for it) |
| `fs.rs` | WPFileSystem (real name, renaming, file icons, moving and copying files), WPDataFile (opened with the PMWP_ASSOC_FILTER program, else the System Editor), WPProgramFile, WPFolder (content: the directory, without hidden files, and the abstract objects in it), WPDrives (a WPDisk per drive), the desktop |
| `icons.rs` | rs2's own icons (pmwin::shellicons, as RS2SHELL shows them) for WPDrives, WPDisk (hard disk or CD-ROM), WPDesktop, command prompts and the Command Prompts folder (by its object ID, where IBM's INI.RC sets an icon resource); a folder's .ICON EA still comes first |
| `minwin.rs` | WPMinWinViewer (`<WP_VIEWER>`; its content is the minimized windows) and WPMinWindow (transient, one per window PMWIN minimizes to the viewer: WM_RS2_MINWINDOW to the window set with Rs2SetMinimizeViewer, the desktop's; opening one restores its window). `RS2_Desktop` `MinimizeTo` (as for RS2SHELL) says whether windows go to the viewer or the desktop |
| `rs2obj.rs` | rs2's own abstract classes for what IBM's shell keeps on the LaunchPad: RS2Shutdown (opening it shuts down), RS2WindowList (the window list) |
| `launchpad.rs` | WPLaunchPad (IBM's setup strings FPOBJECTS, DRAWEROBJECTS, LPCLOSEDRAWER, LPACTIONSTYLE, LPVERTICAL, LPTEXT, LPDRAWERTEXT, LPSMALLICONS, LPHIDECTLS, LPFLOAT, and its query/set methods): the action buttons (Window list, Find, Shut down; as text, small or normal icons, or none), object buttons with drawers above them, objects that open on a click, drops that add objects to the bar or a drawer, Remove from LaunchPad, moving by dragging; the Options and Buttons pages. rs2's own: LPAUTOHIDE ("Hide automatically": a second after the pointer leaves, the bar goes off the nearest screen edge, leaving a 3-pixel strip that brings it back on top); rs2 makes it with LPFLOAT=NO (and once turns floating off on desktops made earlier: PM_Workplace Rs2LaunchPadNoFloat); Close LaunchPad takes its shadow out of the Startup folder and sets PM_Workplace Rs2LaunchPadClosed (new desktop versions do not put it back), opening it puts the shadow back |
| `setup.rs` | WPCONFIG's settings objects: WPMouse (Timing: the double-click time; Setup: right- or left-handed), WPKeyboard (Timing: the repeat rate and delay through KBD$'s KBD_SETTYPAMATICRATE, the cursor blink rate), WPSound (Warning beep), WPColorPalette (colours dragged onto a folder become its background, with Shift its text; onto other windows PP_BACKGROUNDCOLOR or PP_FOREGROUNDCOLOR) WPSchemePalette (a scheme dropped on any window becomes the system's colours, WinSetSysColors) and WPFontPalette (a font dropped on a folder becomes its font, kept as WPFolder key 6 and set on its views as PP_FONTNAMESIZE; on another window, that window's PP_FONTNAMESIZE). The settings are kept in OS2.INI as IBM's PM keeps them (`PM_ControlPanel` DoubleClickSpeed, SwapMouseButton, CursorBlinkRate, KeyRepeatRate, KeyRepeatDelay, Beep; `PM_Colors` "R G B" by colour name) and applied when the shell starts |
| `viewset.rs` | A folder's View tab, IBM's three pages: wpAddFolderView1Page (the view it opens in, the icon view's format: as placed, flowed, non-flowed; icon size: normal, small, invisible; the folder's font: Font... opens the Font Palette, whose fonts dropped on the folder set it, as on IBM's page, and System font drops it), wpAddFolderView2Page (tree view lines and icon size), wpAddFolderView3Page (the details view's columns: Icon, Title, Object class, Real name, Size, Last write date and time) |
| `arrange.rs` | The View menu's Arrange submenu (IBM's ProcessCnrViewMenuItems, CnrViewSetupUndoData and PMWPMRI's menu 101, studied with Ghidra): Standard (733, as Arrange, 309), From top, left, right and bottom (734-737: the view's grid filled from that edge), Perimeter (739: round the edges, then inwards), Selected horizontally and vertically (740, 741: the selected icons in a row or column from the first); before an arrange or a sort each view keeps where its icons were (one undo per view, as IBM's), and its menus offer Undo arrange (310) or Undo sort (319) |
| `lazy.rs` | Pickup and Drop (IBM's ModifyLazyDragMenuItems, CnrOwner_InitPickup, QueryDropOps, CnrOwner_DropNotify): Pick up (188) on an object's menu picks it (with the others selected) up with PMDRAG's DrgLazyDrag, its records halftoned (CRA_PICKED); menus of targets that take them then have Drop (189) with the operations the target's wpDragOver allows (Copy 190, Move 191, Create shadow 192; the default checked) and Cancel drag (193), or Cancel drag alone; a Drop calls DrgLazyDrop and the view's DM_DROP runs wpDragOver/wpDrop as a mouse drop; DM_DROPNOTIFY puts the objects down; a source view that closes cancels the drag. The Folder pull-down has Pick up and Drop for the folder itself |
| `clip.rs` | Paste (IBM's fdr_wpFilterMenu, PasteToFolderDlgProc, EnumerateObjectClasses, df_wpRenderFromClipboard, ClipboardAddClassAssoc/EnumClassAssoc): Paste... (715) is offered by folders whenever the clipboard holds data; its dialog takes the new object's name, the clipboard format and a class associated with the format (M_WPDataFile's wpclsAddClipboardAssoc and wpclsEnumClipboardClasses, class names as system atoms; text with WPDataFile, bitmaps with WPBitmap, metafiles with WPMet), makes the object (wpclsNew) and fills it (wpRenderFromClipboard, which hands the clipboard's data to wpSetContentsFromHandle: the text, a 24-bit .BMP, the saved metafile). rs2's own object clipboard (IBM's shell has none): Ctrl+C and Ctrl+X (Ctrl+Insert, Shift+Delete) in a folder window put the selected objects on the clipboard (format `RS2_WPSObjects`: the operation and their handles; CF_TEXT: their paths, for programs that paste text), Ctrl+V (Shift+Insert) or Paste copies or moves them into the folder |
| `image.rs` | Pictures through MMIO for the lockup: any file an installed image I/O procedure knows, opened with translation and read as an OS/2 bitmap (the MMIMAGEHEADER, then the rows); MMIO.DLL is loaded when first needed |
| `filejob.rs` | Copying and moving file system objects as the user asks (drops, Copy..., Move...): a worker thread with a progress window and the name collision dialog (Rename with a free name, Replace, Skip, Cancel), ported from RS2SHELL; the target folder dialog of Copy..., Move..., Create shadow... and Create another...; the objects follow on the shell's thread when a job ends (paths, handles, folders; a moved or renamed folder's objects too; copies lose the original's object ID) |
| `sortinc.rs` | Folders' Sort page (name, type, real name, size, last write date, or as placed; always maintain sort order, setup ALWAYSSORT) and Include page (patterns matched against titles and real names; folders always shown), the view's Sort menu; SHOWALLINTREEVIEW=YES (a folder's tree view shows every object, not only folders) is kept with them in fs.rs |
| `templates.rs` | Templates (TEMPLATE style, the General page's check box): opening one makes a new object beside it, dragging one makes it at the drop, Create another... makes one where the user says; the new object's title is edited in place |
| `find.rs` | Find... on folders, drives and the desktop: a name pattern, all subfolders; a worker thread walks the directories, and a Find Results window shows the objects (and the matching abstract objects of awake folders) |
| `mindex.rs` | Mindex (IBM's MINXOBJ.DLL, studied with Ghidra): the Help Index and Glossary. INDEX= names an environment variable (HELP, GLOSSARY) whose directories hold the help files (`*.HLP` starting "HS"), or the files themselves separated by `+`; opening merges their indexes (sorted, an entry several panels share followed by the panel's title, as MindexAppendDupIndexTitle does) into a window with a search field; choosing an entry shows its panel through a help instance and HM_SHOW_TOC; letter tabs down the right jump to the entries starting with that letter, and second-level entries are indented under theirs |
| `hwmgr.rs` | PNP.DLL's Hardware Manager (WPHwManager, `<WP_HWMGR>` in System Setup) and its device classes (WPDevice; WPDevAudio, WPDevBus, WPDevCDRom, WPDevCPU, WPDevDisplay, WPDevDiskette, WPDevHarddrive, WPDevKeyboard, WPDevMemory, WPDevMouse, WPDevParallel, WPDevPeriph, WPDevSerial, WPDevTape, WPDevTimer): see [The Hardware Manager](#the-hardware-manager) |
| `url.rs` | WPUrl (IBM's WPINET.DLL): a data file whose content is the URL (URL=, the URL page; `*.URL` files); opening starts the browser OS2.INI's WPURLDEFAULTSETTINGS names (DefaultBrowserExe, DefaultParameters, DefaultWorkingDir), else rs2's own WebRS2 (OS2\APPS\WEB.EXE, [BROWSER.md](BROWSER.md)), else says none is set up; a globe icon |
| `clsmgr.rs` | WPClassManager (merged in as SOM's class manager; finds classes in the DLLs registered in OS2SYS.INI's PM_Objects ClassTable) |
| `api.rs` | WinCreateObject, WinQueryObject, WinSetObjectData, WinDestroyObject, WinOpenObject, WinSaveObject, WinMove/CopyObject, WinCreateShadow, WinQueryObjectPath, WinRegister/Deregister/Enum/ReplaceObjectClass |

**Persistence** follows IBM's layout of OS2.INI:
- Abstract objects are under `PM_Abstract:Objects`, and the folders they
  are in under `PM_Abstract:FldrContent`.
- File-system handles are under `PM_Workplace:Handles`, and object IDs
  under `PM_Workplace:Location`. The table is the truth about a file
  system handle: a number is given out under a system-wide mutex
  (`\SEM32\RS2WPS\HANDLES`), a handle finds the object of the path the
  table gives it, and an awake object whose number the table has given
  to another path takes a new one. (Before, a program's WinQueryObject
  of a path while the shell was starting could take the number the
  shell had just given another folder: `SysOpenObject('C:\MMOS2\SOUNDS')`
  opened the Multimedia Templates folder.)
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

`WPSTEST SHELL cmd; cmd...`, as the boot's first program (`cargo xtask
run --init "RS2\WPSTEST.EXE SHELL OPEN <WP_SYSTEM> 2"`), starts PMSHELL,
waits for its server and runs commands against the running shell: OPEN
id view, SET id setup, LOCK, UNLOCK password, HOLDQ queue, JOB queue
name (a raw job), SLEEP n. The screenshots of the pages, the lockup, the
job view and Create a printer were taken that way (with `--type-file`
for the clicks).

`WPSTEST` also checks the classes and methods added for phases 2 and
4: the confirmations' defaults, a folder deleted with its contents
(wpDelete), the Shredder, System, System Clock and Network classes, a
printer's queue name (saved, and woken again by `WPSTEST CHECK`), the
use list (wpAllocMem, wpFreeMem, wpAddToObjUseList, wpFindUseItem,
wpDeleteFromObjUseList), CCVIEW=, types (wpSetType, wpQueryType) and
associations (wpSet/QueryAssociationType and -Filter), and the
program's colours (RS2COLORS=256 and DEFAULT in its environment, and
256 woken again by `WPSTEST CHECK`), the System object's settings
(wpclsQuery/SetSetting: NameClash, LogoDisplayTime and its default,
PrintScreen and Animation with their system values, the button
appearance and minimize behavior, as `PM_ControlPanel` keeps them), the
desktop's lockup and drop settings (wpIsCurrentDesktop, the lockup
methods, the setup strings, IBM's `LockupOptions` record,
wpQueryLockupBackground, wpSet/QueryDefaultDropOp) and the Spooler
object. It also checks the folder menu settings (FolderMenuBar and
FolderMenus through wpclsSetSetting, as `PM_Workplace` keeps them, a
folder's own setting winning, none for the desktop), a folder window's
menu bar (five pull-downs; Edit filled as it drops; taken away and
given back by wpSetMenuBarVisibility) and that a file system handle
follows OS2.INI when its number was given to another path.

`WPSTEST CLASS dll class` registers a class DLL and makes an object of
it. The Toolkit's samples, compiled for IBM's PMWP, work as subclasses
of rs2's classes:
- Car (WPDataFile): its methods and instance data, and its metaclass's
  class title loaded in wpclsInitData;
- TextFolder (WPFolder);
- WPSTutorial (WPDataFile, which overrides about a hundred methods).

`WPSTEST REGISTER dll class`, with PMSHELL running, registers the DLL
in the shell (WinRegisterObjectClass through ipc.rs) and puts an object
of the class on the desktop, as the samples' INSTALL.CMD does. With
WPSTUTOR.DLL in `C:\OS2\DLL` and the sample's SHOWDESC.EXE in the
shell's current directory (`C:\`; the tutorial starts it with DosExecPgm
and writes method ids to it over the named pipe
`\PIPE\SHOWDESC\NAME.QUE`): SHOWDESC shows the methods as the shell
calls them, the object's menu has the sample's Open tutorial item,
double-clicking opens its view (the default view its metaclass names),
a second double-click brings the same view to the front (wpSwitchTo
through the use list), and its settings show IBM's pages. That needed
the use list, wpRegisterView, and PMWIN placing a frame made by
WinCreateWindow with FCF_SHELLPOSITION in its frame data.

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
  (`arrange.rs`) lays out every icon again in one of IBM's directions
  and keeps the result, which Undo arrange or Undo sort takes back.
- **Tree views** show the folder's folders (and drives' root folders,
  and the folders shadows point to). Each folder opens on demand, in
  place of a placeholder record.
- **Shadows' titles** are owner-drawn in the shadow colour: blue, light
  blue on dark backgrounds, grey when the original is gone.
- **Folder settings** add the View tab's three pages (`viewset.rs`)
  and a Background page (a colour with presets,
  `wpAddFolderBackgroundPage`). The colour is the folder's own state
  and applies to its open views; without one a folder shows the colour
  scheme's window colour and the desktop rs2's teal or the scheme's
  background.
- **Folder menu bars** (Warp 4's, IBM's menu 926 and ShlDisplayFolderMenu
  studied with Ghidra): folder windows (not the desktop) have Folder,
  Edit, View, Selected and Help with IBM's ids (719-723), filled each
  time one drops (WM_INITMENU). Folder: Open as (icon, tree, details
  view), Properties, Open parent, Create another, Move, Copy, Create
  shadow, Delete, Pick up and Drop (`lazy.rs`) of the folder itself.
  Edit: the same for the selected objects (enabled when every one
  allows it), Paste (into the folder, when the clipboard holds data:
  `clip.rs`), Select
  all (Ctrl+/), Deselect all (Ctrl+\\), Find. View: Icon, Tree and Details
  view (WPMENUID_CHANGETO*: the window becomes that view, in its place),
  Sort, the Arrange submenu, Undo arrange or Undo sort, Refresh now.
  Selected: the first selected object's
  pop-up menu without the Edit items, its class's and the user's items
  added (wpModifyPopupMenu, wpAddUserItemsToPopupMenu); its choices go
  to every selected object. Help: index, general, using help, keys.
  Whether a folder has one is its Menu page's setting, else the System
  object's (`PM_Workplace` FolderMenuBar); changing either adds or takes
  away the bars of open windows, the frame growing or shrinking by the
  bar as IBM's wpSetMenuBarVisibility does.
- **Keyboard:** Delete deletes (as the Delete menu item: delete.rs), F2
  renames, F5 refreshes, and Shift+F10 shows the menu; Ctrl+C, Ctrl+X
  and Ctrl+V copy, cut and paste objects (`clip.rs`).
- **Settings notebooks** have IBM's tabs in IBM's order: a folder's
  View, Sort, Include, Background, Menu (two pages), File (three pages),
  Window (two pages) and General; the desktop's Lockup (three pages),
  Archive and Desktop before those; a data file's Type, Menu, File,
  Window, General; a program's Program, Session, Association, Window,
  General; a drive's Details, Window, General; the System object's
  Confirmations, Title, Window, Print Screen, Logo, Folder (three
  pages), Input, Screen (256 colours only), General; a printer's
  View, Printer driver, Output port, Queue options, Print options,
  Window, General.

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
If a Workplace Shell is already running, it ends. As on OS/2 there are
two PMSHELL processes: the first (PROTSHELL's) runs the shell as a
second one with `WORKPLACE_PROCESS=YES` in its environment and starts
it again when it ends abnormally (see "IBM's multimedia classes"
below); the rescue screen lists both.

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
    Viewer, Window List, System Setup (`<WP_CONFIG>`: System
    (`<WP_SYSTEM>`), System Clock (`<WP_CLOCK>`), the Spooler
    (`<WP_SPOOL>`), Keyboard, Mouse, Sound,
    the Color, Scheme and Font Palettes, and Display where it exists),
    Shut Down, the Shredder (`<WP_SHRED>`, LOCKEDINPLACE), Connections
    (`<WP_CONNECTIONSFOLDER>`: Network, `<WP_NETWORK>`, and Printers,
    `<WP_PRINTERSFOLDER>`, with a printer template, `<WP_NEWPRINTER>`, as
    in IBM's INI.RC; Drives stays on the desktop), Templates (Data File,
    Folder, Program) and the Assistance Center as IBM's INI.RC lays it
    out (`<WP_ASSISTANCE>`: Information, a tree view showing all its
    objects, with Reference and Commands, Tasks and Read Me, which holds
    a shadow of `README.TXT`; Troubleshooting), where APPINST puts the
    books, the Help Index (`<WP_MINDEX>`), Online Information Overview
    with the Glossary (`<WP_GLOSS>`), and "rs2 on the Internet" (a WPUrl
    to the project's page), then the LaunchPad (`<WP_LAUNCHPAD>`: Drives,
    OS/2 Window with OS/2 Full Screen in its drawer, the System Editor,
    the Assistance Center) with its shadow in Startup. `PM_Workplace`
    `Rs2Desktop` holds the version made (10: the Spooler; 12: rs2's applets in
    Utilities, Games and System Setup, [PM.md](PM.md#rs2s-pm-applets)); a desktop of an earlier
    version gets the entries again, and PRESERVEOLD keeps what it has
    (objects are matched by their OBJECTID, so none is made twice).
    Version 9 also deletes rs2's earlier System object (`<RS2_SYSTEM>`,
    RS2System) for IBM's class: DELETE removes an object even when it was
    made non-deleteable. Titles break lines at `^`, as in INI.RC.
    Checked in QEMU: `WPSTEST OLDDESKTOP` (OS2.INI says version 8, an
    old RS2System object made), PMSHELL ended from the rescue screen and
    started again, then `WPSTEST DUPES` (each desktop folder there once,
    by its title; the Shredder made once; RS2System replaced).
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

**The Recovery Choices** (`userland/pmshell/src/recovery.rs`) are
rs2's counterpart of OS/2's Alt+F1 screen. IBM's loader shows it, from
the list the shell writes (`\OS2\BOOT\ALTF1MID.SCR`, a `CONFIG.n` per
archive: IBM's DoArchiveWork and WriteAltF1Screen, studied with Ghidra).
rs2's boot loader runs from the EFI partition and does not read the
system's files, so it only passes the wish on: R (or F1) while it waits
for T adds `recovery` to the kernel command line, and the kernel gives
the boot programs `RS2RECOVERY=YES`. The first PMSHELL then shows a
text-mode screen before PM starts, when asked so or when the Archive
page's "Display recovery choices at each system startup" is set, for its
timeout (0: until a key): Enter or Esc start the current desktop, a
number restores that archive (newest first) and C runs a command prompt
first. Restoring copies the archive's OS2.INI and OS2SYS.INI to `\OS2`,
CONFIG.SYS and STARTUP.CMD to the root, and its copy of the desktop
directory (files and extended attributes, which hold the objects'
settings) in place of the desktop; what they replace is kept in the
archive location's `REPLACED` directory. A restored CONFIG.SYS that
differs restarts the system, so that it is the one in use. The second
PMSHELL does not get `RS2RECOVERY`.

The desktop's pop-up menu adds **Window list**, **Lockup now** and
**Shut down**. When the shell starts it makes the Archive page's archive,
starts the automatic lockup's timer and locks at once with "Lock on
startup".
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

### The Input page in the window manager

PMWIN acts on the System object's Input page (system values 99-103)
and on Special Needs (rs2-private 4000-4002), in `userland/pmwin/src/access.rs`:

- **Full window drag** (99, IBM's default off): without it, moving or
  sizing a frame with the mouse shows an inverted outline that follows
  the pointer, and the window moves (or takes its new size) when the
  button goes up.
- **Alt+Tab switching window** (100, default on): Alt+Tab activates the
  next window (frames of programs with a title bar, visible or
  minimized, in z-order); with the switching window on, a box in the
  middle of the screen shows the window it would switch to (icon and
  title) while Alt is held, each Tab (Shift+Tab: back) moving on, and
  the switch happens when Alt goes up (the kernel now passes PM the Alt
  key's release; PMWIN keeps it from programs). Alt+Esc stays the
  kernel's (the next session).
- **VIO mouse actions** (101, default on): off, the mouse no longer
  marks text in VIO windows (PMVIOWIN; the menu's Mark still does).
- **Asynchronous focus change** (102, default on) and its delay (103,
  tenths of a second, default 20): a thread that takes longer than the
  delay to come for its input is treated as not responding (PMWIN
  then activates, moves and sizes its windows itself), instead of only
  after PM_HANGTIME.
- **Special Needs**: with it on, a key counts only when held down for the
  acceptance delay (a press is held back until a repeat or the release
  shows it was held long enough; a shorter one is dropped; Shift, Ctrl,
  Alt and the lock keys always count), and it turns itself off after
  the time-out without key presses.

Checked in QEMU (`WPSTEST SHELL OPEN <WP_CONFIG> 1; OPEN <WP_SYSTEM> 2;
OPEN <WP_POWER> 131` with `--type "{keydown alt}{tab}...{keyup alt}"`
and pointer drags): the switching window going through the windows and
the switch when Alt went up; a title bar drag showing the outline and
the window moving at the end; with Special Needs on, quick taps of a
and b in an OS/2 window dropped and x and y held for 0.7 s typed (the
default acceptance delay, 250 ms).

## The Hardware Manager

IBM keeps the Hardware Manager in PNP.DLL (its IDL: `wphwmgr.idl`,
`wpdevice.idl` and one file per device class); rs2 has its classes in
PMWP.DLL, with the Toolkit's release orders, and a PNP.DLL whose
exports (IBM's ordinals, by name too) forward to PMWP's private ordinals
12000 + IBM's (`userland/pnp`), as WPCONFIG.DLL's do. IBM's PNP.DLL,
RMINFO.DLL and RMVIEW.EXE were studied with Ghidra.

**The resource manager.** IBM's RESOURCE.SYS keeps a tree of nodes
(adapters, the devices on them, logical devices, drivers, detected
hardware) with the resources each holds; programs read it through
RMINFO.DLL, which opens `RESMGR$` and makes IOCtls of category 0x80.
rs2's kernel provides the same device (`kernel/src/resmgr.rs`), with
IBM's structures (rmbase.h's RMNODE, ADAPTERSTRUCT, DEVICESTRUCT,
LDEVSTRUCT, DRIVERSTRUCT, RESOURCELIST, as RMVIEW reads them):

| IOCtl | Parameters | Data |
|-------|------------|------|
| 0 | | ULONG: RM's entry point for 16-bit drivers (0) |
| 1 GetNodeInfo | RMHANDLE, linear address of the data | RMNodeSize, RMNODE, the node's structure, strings and resource list (linear addresses) |
| 2 EnumNodes | USHORT: 0 physical, 1 logical, 2 drivers, 3/4 detected | NumEntries (all of them, also when they do not fit), { RMHANDLE, depth } each |

The tree is built when first asked for, from what the kernel knows:
the processors (`CPU_n` and CPUID's name), the memory, the I/O APICs,
the PCI functions under their buses (a bridge's bus under it) with
their BARs (sized when the bus is scanned at boot, before any driver
starts) and their routed interrupt, the disks and CD drives on their
controllers (`HD_n`, `CDROM_n`), an ISA bus under the PCI-to-ISA bridge
with the ACPI namespace's standard devices that have a hardware ID and
fixed current resources (`_CRS` that is data: keyboard controller,
PS/2 mouse port, serial and parallel ports that answer, real-time
clock, HPET, diskette controller), the PC devices ACPI leaves out (PIC,
DMA controller, timer, RTC, keyboard controller), logical disks and
ports, and the kernel's drivers (by module: AHCI, NVME, VIRTIO, COM,
KBD, ...) owning their adapters. Names follow IBM's (`CPU_`, `PCI_`,
`ISA_`, `HD_`, `DSKT_`, `KBD_`, `MOUSE_`), as the Hardware Manager picks
classes by them.

`OS2\DLL\RMINFO.DLL` (`userland/rminfo`) is rs2's: IBM's names and
ordinals, _Optlink entries (EAX, EDX, ECX) on a 16-byte aligned stack:
RMEnumNodes, RMGetNodeInfo, RMConvertID (EISA IDs to text and back),
RMGetSnoopLevel and RMSetSnoopLevel (the detection levels, in
`\OS2\BOOT\PREVIOUS.DAT` as IBM keeps them) and RMSaveDetectedData
(nothing detected); the drivers' calls fail. IBM's RMVIEW.EXE runs on
it, and on IBM's RMINFO.DLL (which opens `"RESMGR$ "`, blank-padded:
rs2's device names now drop trailing blanks): all its views but the
detected ones.

**The classes.** WPHwManager is a folder (`<WP_HWMGR>` in System Setup,
desktop version 15) whose default view is its details view, as IBM's.
Populating it reads the physical view and the drivers and makes a
transient object for each adapter and device it does not show yet
(UNIQUEID= the node's handle, PARENTID= the node a level up before it),
of the class IBM's DetermineNameAndClass picks: by an adapter's base
type and subtype (hard disk or diskette controller, display, audio,
memory, serial, parallel, timer, system peripheral, keyboard, mouse),
by its name (`CPU_` a processor, `ISA_`/`PCI_`/`MCA_` a bus), by a
device's type and name (`HD_`, `DSKT_`, CD-ROM, `MOUSE_`, `KBD_`,
audio); planar chipset devices get none. The title is the node's
description. The details view shows each device's type, IRQs, DMA
channels, I/O port and memory ranges (IBM adds the four columns with
wpclsQueryDetailsInfo; rs2's views take them from `hwmgr.rs`); the tree
view shows the devices under what they are on, all open. A device
opens on its settings: Device (the device type and the driver: module,
description, vendor, version, date), IRQ, DMA, I/O and Memory (a list
each; IBM's page order), General. WPDevice's private methods are IBM's
(wpDevicePrivate1 is wpGetUniqueID, 2 wpGetParentID, 3-5 the node's
type, base type and subtype, 22-26 the driver's file, name, vendor,
version and date, 28-29 wpSetUniqueID/wpSetParentID); the getters and
those setters are there. Icons are rs2's own (`pmwin::hwicons`).

Checked in QEMU: RMVIEW's views (`/P /D /L /IRQ /IOA /MEM /DMA /HW`),
with rs2's and with IBM's RMINFO.DLL; the Hardware Manager's details
and tree views (with NVMe, virtio, HD Audio and e1000 devices too) and
a serial port's Device and I/O pages (`WPSTEST SHELL OPEN <WP_HWMGR>
102`, then double-clicks).

## rs2's multimedia classes

`mm.rs` gives PMWP the multimedia classes, with the names of IBM's
MMPARTS.DLL ones (MINSTALL's list) so that scripts and programs naming
them find them: MMDataFile (WPDataFile) and its sound classes (MMAudio;
MMWAV, MMVOC, MM_AU, MMAU, MMAIF, MMIFF, MMSNDF), MMMIDI, the video
classes (MMVideo; MMAVI, MMFLI, MMMPG, MMMOV) and the image classes
(MMImage; MMJPG, MMGIF, MMPCX, MMTIFF, MMTarga, MMDIB, MMPhotoCD, MMAVC,
MMMMotion); MMFolder; MMDevice, MMCDDevice, MMLVDDevice and MMVolume
(WPAbstract). They introduce no methods (`gen_wps_classes.py`'s EXTRA,
with the metaclass M_MMDataFile).

- **Files:** M_MMDataFile gives each data file class IBM's instance
  filter and type (MMWAV `*.WAV` and `WAV,Digital Audio`, MMJPG
  `*.JPG,*.JPEG,*.JPE`, ...), so files take the classes by the shell's
  instance types and filters (IBM's AddInstanceTypeAndFilter, below), and
  the class icons: rs2's own (`pmwin::mmicons`: a sound page, a MIDI
  page, a film strip, a picture; the Multimedia folder, Volume Control,
  the CD Player), unless a file has its own.
- **Views:** opening a data file starts rs2's media player
  (`%MMBASE%\MPPM.EXE` with the file, MMPM.md); the Open submenu has
  Player (Viewer for images) and, for sounds, Editor (WEPM, the Digital
  Audio editor). MMCDDevice opens the CD Player (CDPM), MMVolume Volume
  Control (MMVOLUME), MMDevice and MMLVDDevice Multimedia Setup (STPM).
  Program objects of those applets show the same icons.
- **Loading:** desktop version 13 puts the classes in the class table
  for PMWP and loads them, so they are ready (their filters in effect)
  whenever the shell starts. When another DLL is registered for one of
  these names (IBM's MMPARTS, by WinRegisterObjectClass or INSTMMVW.CMD)
  the class manager loads that one instead (clsmgr.rs); rs2's are then
  not built, from the next start of the shell.
- **The Multimedia folder** (desktop version 13): `<MMPM2_FOLDER>`
  (MMFolder) in Programs with Media Player, Digital Audio, Multimedia
  Setup (`<MMPM2_SETUP>`), CD Player (MMCDDevice, `<MMPM2_CDPLAYER>`),
  Volume Control (MMVolume, `<MMPM2_VOLUME>`) and a shadow of the Sound
  object (`<MMPM2_SNDSHAD>`); Multimedia Templates
  (`<MMPM2_MMTEMPLATEFOLDER>`) in Templates with a Wave Audio (MMWAV)
  template, whose new files open in the editor as new sounds. IBM's
  object IDs are kept where MMPM/2 has the object.

Checked in QEMU (screenshots): the Multimedia folder; a folder of a WAV,
an AU, an AVI and a JPEG, each with its class's icon; double-clicking
the JPEG shows it in the player; a sound's menu.

## IBM's multimedia classes (MMPARTS)

IBM's MMPARTS.DLL (MMPM/2's Workplace Shell classes; the files and the
players in [MMPM.md](MMPM.md#mmpartsdll-and-mppm)) is a third-party
class DLL like wpstutor, much larger and built with VisualAge C++'s Open
Class library. What it needed of rs2's shell, each as IBM's PMWP and
PMSHELL do it (studied in Ghidra):

- **The workplace process.** MMPARTS's SOMInitModule makes its classes
  only when `WORKPLACE_PROCESS=YES` is in the environment. IBM's first
  PMSHELL (the PROTSHELL one) runs only the message loop; PMWP's
  StartWorkplaceExe starts `%RUNWORKPLACE%` (PMSHELL again) with its
  environment plus `WORKPLACE_PROCESS=YES`, and that one calls
  ShlStartWorkplace. rs2's PMSHELL does the same: without the variable
  it runs the shell as a second PMSHELL (or `%RUNWORKPLACE%`) and waits
  for it, starting it again when it ends abnormally (a trap, or ended
  from the rescue screen), and while another shell runs it waits for
  that one to end. Programs the shell starts get the variable as
  `WORKPLACE__PROCESS=NO` (IBM's BuildEnv), so a program that loads a
  class DLL (MPPMNEW loads MMPARTS) does not take itself for the shell.
- **The exception guard.** PMWP exports `ShlSaveEnv` (213) and
  `ShlExceptionHandler` (212): `if (ShlSaveEnv(&rec, ShlExceptionHandler))
  { ... DosUnsetExceptionHandler(&rec); } else { trapped }`. ShlSaveEnv
  puts the record (prev, handler, then EBP, EBX, EDI, ESI, ESP and the
  return address) at the head of the thread's handler chain and returns
  1; on an access violation the handler goes back there (ShlRestoreEnv)
  and ShlSaveEnv returns 0, unless `SHELLEXCEPTIONHANDLER` says OFF;
  with `PMWPDEBUG` the trap's address, its callers and the registers are
  written out. A kill signal to the shell saves what the lazy writer
  holds. The kernel forgets the dispatch of a handler that jumped out
  so (its records are above the stack pointer).
- `vhwndWorkplaceWindow` (466, the shell's object-call window, which
  MMPARTS waits for before it configures itself) and `vhwndHelpInstance`
  (279, 0: rs2's objects show help through their own instances);
  `WinWaitForShell` (512: the desktop created, opened, populated; with
  WWFS_QUERY only asked).
- **Registered classes load when the shell starts**, as on OS/2, after
  the first start's `PM_InstallClass`: their DLLs set themselves up and
  their instance types and filters take effect. A class whose DLL is
  missing or does not load stays out and the shell goes on (its files
  wake as plain data files); WinRegisterObjectClass refuses a class whose
  DLL does not load.
- **The classes of files** (IBM's AddInstanceTypeAndFilter, called from
  M_WPFileSystem's wpclsInitData, and FsParseClassInfo): a file system
  class's wpclsQueryInstanceType and wpclsQueryInstanceFilter
  (comma-separated) are noted when it is ready, unless its parent says
  the same, the class registered last first, separately for folders and
  files. A file whose `.CLASSINFO` names no class, or WPDataFile, wakes
  as the class of its first `.TYPE` that a class claims, else of the
  class whose filter matches its name (MMWAV: `*.WAV`, types `WAV,Digital
  Audio`), else as before.
- **Class icons from wpclsQueryIconData** (an ICONINFO: an icon
  resource, an icon file or icon file data; IBM's M_WPObject's
  wpclsQueryIcon makes and keeps the pointer): a class's own icon, and a
  file's when it has no `.ICON` (MMPARTS's sound files show its WAV
  icon).

MMPARTS's classes that matter for files override no settings pages (a
sound file's notebook is WPDataFile's, as on OS/2); they override the
pop-up menu, wpOpen, the default view, drag items, titles, icons and
the instance types and filters. Opening a WAV file's default view opens
MMPARTS's audio editor (see MMPM.md for what works in it).

**Installing it.** IBM's MINSTALL (FOURMEG.SCR) registers 31 classes from
MMPARTS (MMDataFile, MMImage, MMAudio, MMVideo, MMMIDI, MMTarga, MMTIFF,
MMGIF, MMPCX, MMPhotoCD, MMAVC, MMMMotion, MMDIB, MMDevice, MMCDDevice,
MMLVDDevice, MMVolume, MMVOC, MM_AU, MMAIF, MMIFF, MMSNDF, MMAVI, MMFLI,
MMMPG, MMMOV, MMJPG, MMWAV, MMFolder, MMFinder) and replaces WPBitmap
and WPShadow by MMBitmap and MMShadow (from MMBIT.DLL and MMSHD.DLL,
forwarders into MMPARTS) through `PM_InstallClassReplacement`; its
INSTMMVW.CMD does the same with REXX's SysRegisterObjectClass. On rs2
the classes are registered with WinRegisterObjectClass (WPSTEST
`REGISTER`, or REXX), and take the place of rs2's own classes of the
same names ("rs2's multimedia classes" above) from the next start of the
shell; the Multimedia folder (`<MMPM2_FOLDER>`) is rs2's.

**Class replacement** is IBM's: OS2.INI's `PM_Workplace:ReplaceList`
holds, under the class replaced, the classes replacing it (each
NUL-terminated, the last in effect); WinReplaceObjectClass adds or
removes one (in the shell's process when called from another) and
applies it at once (SOM's somSubstituteClass), `PM_InstallClassReplacement`
entries (INI.RC's, MINSTALL's) go into the list when the shell starts,
and the list is applied then, where the replacing class loads. Files of
PMWP's own classes are found through the class manager, so MMBitmap
stands in for WPBitmap. MMBitmap is one of the classes MMPARTS's
SOMInitModule does not make: as IBM's, SOM's default SOMLoadModule
then calls the DLL's `<Class>NewClass`.

**Test.** `cargo xtask run --init "RS2\WPSTEST.EXE SHELL MMPARTS"` with
IBM's files staged in work/modules.txt (MMPARTS.DLL in `\MMOS2\DLL`,
OS2OM30, OS2OOOC and OS2OOR3U in `\OS2\DLL`, SW, MMPMCRTS, DIVE and
MMPTMRI in `\MMOS2\DLL`, a WAV in `\MMOS2\SOUNDS`; LIBPATH with
`C:\MMOS2\DLL`, `SET MMBASE=C:\MMOS2;`): a class from a missing DLL is
refused and the shell goes on; MMAudio and MMWAV registered, an MMAudio
object made on the desktop (its `.CLASSINFO` names MMAudio), the WAV
file woken and saved as MMWAV, the object deleted; MMBitmap registered
and WPBitmap replaced by it (a .BMP file wakes as MMBitmap), the
replacement undone. Without MMPARTS.DLL
its checks are skipped. WPSTEST SHELL also has `REGISTER dll class` and
`CREATE class title location setup` (with `|` for `;`).

## Object REXX in the shell (REXXWPS)

IBM's Object REXX scripts the Workplace Shell with OREXXWPS.DLL
(installed as REXXWPS.DLL), a class DLL that WPSINST.CMD registers
(WPZORXINIT and WPZORX, also under `PM_Workplace:IplLoad`). Its
SOMInitModule starts Object REXX in the shell's process and runs the
tokenized WPSINIT.WPS (the WPS server, `.WPS` in the global
environment), WPCLS.IMP (the classes WPObject, WPFolder, WPAbstract,
WPTransient, WPProgram and WPDesktop as REXX proxies) and WPREXX.IMP
(WPZORXINIT, a WPTransient whose `OPEN=DEFAULT` makes the server's
object window). REXX programs in other processes then send their WPS
messages to the shell's REXX threads, which call the objects' methods
there and hand back object handles. What it needs of rs2's shell, each
as IBM's PMWP does it (studied in Ghidra):

- **WPCONFIG.DLL.** REXXWPS names IBM's System Setup class library among
  its modules. rs2 has those classes in PMWP; its WPCONFIG.DLL
  (`userland/wpconfig`) exports IBM's names and ordinals as forwarders to
  PMWP's private ordinals 9101-9154 (the NewClass functions, ClassData
  and CClassData of WPSystem, WPClock, WPMouse, WPKeyboard, WPSound and
  the palettes, and their metaclasses).
- **Handles of transient objects** (IBM's trn_wpQueryHandle): the
  object's number (counted from 1) in the low word and its top byte, the
  transient class's tag 4 between them; wpclsObjectFromHandle finds the
  object while it lives. Transients are still not saved.
- **`OPEN=DEFAULT` / `OPEN=SETTINGS`** in setup strings (IBM's
  ShlOpenObjectOnceInitialized): the object is locked and the view opens
  on the shell's PM thread (a message to the shell's object window), so
  objects made from other threads open their views where the shell's
  windows live.
- SOM's redispatch stubs (REXX overrides methods with somOverrideSMethod
  and the method's somGetRdStub) and the interface repository.

**Test.** `tests/rexx/ORXWPS.CMD`, with Object REXX staged
(COMPATIBILITY.md): `cargo xtask run --init "RS2\WPSTEST.EXE SHELL RUN
C:\CMD.EXE /C ORXWPS.CMD"`. It registers the classes, then makes a folder
on the desktop and a program object in it, retitles and sets them up and
deletes them, all from a CMD process. WPREXX.IMP's wpOpen of WPZORXINIT
reports `Could not find routine "WPUSER"` in the shell (not found yet).

## What is not there

Compared with IBM's System Setup (INI.RC's `<WP_CONFIG>`) and its other
WPCONFIG and WPPRINT objects, rs2 leaves out, for now:
- the System object's display type page shows the display rather than
  listing monitor types (rs2's drivers report their own resolutions,
  without refresh rates), and its Fonts page has only the icon text font
  (IBM's Sys*FontDlgProc pages set the device, video and text-mode fonts
  of DBCS systems); the Input page's type-ahead (SV_LOCKSTARTINPUT) is
  kept but not acted on;
- Power cannot suspend (rs2's ACPI only turns the computer off) and
  reads no batteries; WIN-OS/2 Setup's data exchange and fast load are
  kept but have nothing to act on yet (WINOS2.md); Special Needs has no
  sticky keys (IBM's has none either in Warp 4's page) and its time-out
  only applies when the next key comes; Touch and
  Selective Install (IBM's installer's);
- printing: remote printers (WPRPrinter objects) are not made: a print
  queue on a server is a WPSharedDir whose Assign port redirects LPT1-3
  to it (NET USE LPTn), and a local printer object on that port prints
  there ([LAN.md](LAN.md)); Start again cannot stop a job that is
  printing; the Spooler's Print priority is kept, and its Disable only
  sends programs' port output (DosOpen of LPT1, DOS sessions) to the
  port (PM programs' jobs still spool); the Print options page has no
  Input bin and no "only jobs that exceed size limits", the View page's
  Network job view (local queues only);
- the desktop: the Recovery Choices offer the archives and a command
  prompt, not IBM's other choices (VGA mode, another CONFIG.SYS); the
  Drop submenu has no Create from template and Create program object
  (templates dropped make new objects through Copy); Paste's dialog lists
  only the classes associated with a format (no "Become" page);
- the network objects have no pages of their own (wpAddNetworkPage,
  wpAddServerPage, wpAddResourcePage), no Access Control or logon
  dialogs, and IBM's network icons (they show folder icons).
- the Hardware Manager: no hardware detection page (IBM's wpAddSnoop1Page
  sets the snoop level of the next boots' detection; rs2 detects nothing
  at boot, so the detected views are empty and its levels are only kept
  by RMSetSnoopLevel), no "next boot" resources to change, and the
  drivers are the kernel's modules (no driver files, dates or
  parameters). The resource manager knows the devices the kernel found:
  USB devices behind xHCI, a USB keyboard or mouse, and resources ACPI
  computes in methods (`_CRS` methods, `_STA`) are not in its tree;
  interrupts are the PCI functions' routed lines, also when a driver
  uses MSI.
