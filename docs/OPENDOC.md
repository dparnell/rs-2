# OpenDoc for OS/2

IBM's OpenDoc for OS/2 runtime (OPENDOC.DLL and friends) is not on the
Warp 4.52 CDs, but two programs on them are OpenDoc clients:

- `MMTEMP\MMODPTS.DLL` (disk 20; symbols in
  `debug/symbols/mm/applets/mmparts/mmodpts.sym`): the multimedia OpenDoc
  parts (MMBasePart and its subclasses MMAudioPart, MMVideoPart,
  MMImagePart, MMMIDIPart, MMSettingsExtension), which also install
  themselves with `RxInstallParts`;
- `OS2\DLL\OREXXSC.DLL` (disk 4; `debug/symbols/os2/dll/orexxsc.sym`):
  Object REXX's OpenDoc scripting component (REXXScriptingComponent, an
  OSA scripting component, and the Apple-event glue around it).

Both import module `OPENDOC` by ordinal only. rs2 builds its own
OPENDOC.DLL (`userland/opendoc`) with those ordinals. This file is the
inventory of what the clients need (studied with Ghidra:
`tools/ghidra/lxload.sh` + `DecompileAll.java` on both modules, with their
.sym files) and the plan and status of rs2's runtime.

## How the ordinals were decoded

OpenDoc is built on SOM: a client calls a method through the token in
the class's `<Class>ClassData` (`ClassData + 4 * (1 + i)` for the i-th
method in the class's IDL release order), builds subclasses with
`somBuildClass` after calling the parent's `<Class>NewClass`, makes
parent calls with `somParentNumResolve(<Sub>CClassData, 1, token)`, and
C++'s `new` checks the object against each ancestor's
`<Class>CClassData`. So each imported ordinal is a ClassData, a
CClassData, a NewClass or a plain function, and its role is plain from the
code.

- **The exports are in alphabetical order of their names**, three per
  class (`XCClassData`, `XClassData`, `XNewClass`): ODObject is 392-394,
  ODPart 410-412, ODPersistentObject 422-424, ODRefCntObject 443-445 (each
  shown by the methods called through it), and the classes between two
  known ones fill exactly the slots left (ODExtension 323-325, ODFacet,
  ODFacetIterator, ODFocusModule, ODFocusOwnerIterator, ODFocusSet
  338-340, ODFrame 344-346; the seven ODLink*/ODInfo classes between ODHelp
  and ODMenuBar; ODStorageSystem/Unit/UnitCursor/UnitRefIterator/UnitView
  464-478). `M_` metaclasses come first (M_ODPart 209-211),
  `OSAScriptingComponent` last (527-529). OS/2's OpenDoc splits several
  classes into a portable base and an OS/2 class: ODBaseCanvas,
  ODBaseDragAndDrop, ODBaseSession, ODBaseShape and ODBaseTransform sort
  before the rest (ODShape is ODObject -> ODRefCntObject -> ODBaseShape ->
  ODShape, as MMODPTS's `new ODShape` checks).
- **ODPart's release order** comes from MMBasePart's class: its static
  class information lists the 70 methods it overrides by name
  (`ODPart::Draw`...) and `MMBasePartNewClass` resolves the parent
  method of each, in reverse order, by token: pairing the two lists gives
  the token of 68 ODPart methods, 3 ODObject, 2 ODPersistentObject and 2
  ODRefCntObject methods (table below). The same pairing gives M_ODPart's
  five methods and three of ODSettingsExtension's.
- **Other classes' methods** are named from how the client uses the
  result and arguments, against OpenDoc's published interfaces (the
  ODStorageUnit, ODFrame, ODFacet, ODTransform... method lists): e.g.
  `ODFacet[0]` returns the frame passed to the part's own frame methods,
  `ODBaseTransform[18]/[19]` map a point both ways in TransformPoint and
  InvertPoint, `ODDraft[9]/[10]` bracket an `ODStorageUnit[17]` clone with
  a key. Where OpenDoc's published declaration order lines up with every
  use, rs2 takes it as the release order for the unused entries too
  (marked "assumed" below).

OREXXSC's ordinals 1-71 are functions (the Apple Event Manager calls:
AECoerceDesc, AECountItems, AECreateDesc, AECreateList,
AECreateAppleEvent, AEDisposeDesc, AEGetParamDesc, AEGetParamPtr, ...,
also alphabetical), 527-529 OSAScriptingComponent, and 1000-1046 a second
block of classes (an Apple-event terminology dictionary rooted at
SOMUC's somf_MCollectible: 1000-1002 is the class REXX's `SETAETE`
receives, its ancestors' CClassData are 1021, 1030 and 1033).

## What the clients import

### Classes (ClassData ordinal; CClassData is one less, NewClass one more)

| Ord | Class | Methods used (release index: name) | By |
| --- | --- | --- | --- |
| 204 | M_ODFacetWindow (name assumed; a SOMClass subclass) | 0: is this HWND a facet window (MMODPTS's focus library walks up parents with it) | MM |
| 210 | M_ODPart | 0 clsGetODPartHandlerName, 1 clsGetODPartHandlerDisplayName, 2 clsGetODPartKinds, 3 clsGetOLE2ClassId, 4 clsGetWindowsIconFileName (overridden by M_MMBasePart, which adds clsGetODPartHandlerCategories, ...FilenameFilters, ...FilenameTypes, ...KindName, ...KindDisplayName, ...CategoryDisplayName, GetWindowsIconFileName) | MM |
| 219 | ODArbitrator | 0 RequestFocusSet, 1 RequestFocus, 2 RelinquishFocusSet, 3 RelinquishFocus, 5 TransferFocusSet, 6 AcquireFocusOwner, 13 CreateFocusSet | MM |
| 222 | ODBaseCanvas | 1 GetPlatformCanvas(graphics system 3), 10 IsDynamic | MM |
| 231 | ODBaseDragAndDrop | 0 Clear, 1 GetContentStorageUnit, 2 StartDrag | MM |
| 234 | drag item iterator (sorted as ODBaseDrag...) | 0 First, 1 Next | MM |
| 264 | ODBaseSession | 0 GetArbitrator, 2 GetClipboard, 4 GetDragAndDrop, 13 GetWindowState, 30 Tokenize | MM |
| 267 | ODBaseShape | 2 GetBoundingBox, 3 SetRectangle, 15 Copy, 16 CopyFrom, 17 Transform | MM |
| 273 | ODBaseTransform | 0 GetType, 11 SetOffset, 18 TransformPoint, 19 InvertPoint | MM |
| 291 | ODClipboard | 1 Clear, 2 GetContentStorageUnit, 7 ExportClipboard, 11 (OS/2) register a platform format (name, format, info) | MM |
| 312 | ODDraft | 6 CreateStorageUnit, 7 AcquireStorageUnit, 9 BeginClone, 10 EndClone, 34 ReleasePart | MM |
| 315 | ODDragAndDrop | 3 CanEmbed/CanIncorporate (2 args), 7 GetDragAttributes | MM |
| 324 | ODExtension | (CClassData only: MMSettingsExtension's ancestry) | MM |
| 327 | ODFacet | 0 GetFrame, 10 AcquireClipShape, 13 ChangeActiveShape, 16 AcquireFrameTransform, 17 AcquireContentTransform, 18 AcquireWindowFrameTransform, 19 AcquireWindowContentTransform, 22 GetCanvas, 24 GetWindow, 44 (OS/2, int), 46 GetFacetHWND, 47/48 aggregate clip shapes | MM |
| 339 | ODFocusSet | 0 Add | MM |
| 345 | ODFrame | 0 AcquireContainingFrame, 2 AcquireWindow, 8 IsRoot, 18 AcquirePart, 20 GetPartInfo, 21 SetPartInfo, 22 GetViewType, 23 SetViewType, 24 ChangeViewType, 25 GetPresentation, 26 SetPresentation, 30 CreateFacetIterator, 32 AcquireFrameShape, 34 RequestFrameShape, 36 ChangeUsedShape, 43 SetDroppable, 44 IsDragging, 45 SetDragging, 50 Invalidate | MM |
| 348 | ODFrameFacetIterator | 0 First | MM |
| 351 | ODHelp (OS/2) | 1 DisplayHelp(file, panel), 2 DisplayHelpIndex(file) | MM |
| 375 | ODMenuBar (OS/2) | 0 Display, 7 SetMenuItemText, 11 CheckMenuItem, 12 EnableMenuItem, 13 AddMenuItemBefore, 14 AddMenuItemLast, 16 MenuItemExists, 17 IsItemEnabled, 18 IsItemChecked, 21 RemoveMenuItem | MM |
| 393 | ODObject | 1 HasExtension, 2 AcquireExtension, 3 ReleaseExtension | MM |
| 411 | ODPart | all 68 (table below) | MM |
| 423 | ODPersistentObject | 3 Externalize, 4 GetStorageUnit, 6 CloneInto | MM |
| 426 | ODPlatformCanvas (OS/2) | 1 GetPS(facet), 2 ReleasePS(facet) | MM |
| 444 | ODRefCntObject | 1 Acquire, 2 Release, 3 GetRefCount | MM |
| 450 | ODSession (OS/2) | 3 CreatePlatformWindow(frame flags), 4 GetHelp | MM |
| 453 | ODSettingsExtension | 17 InitSettingsExtension, 18 AddNotebookPages, 19 TitleChanged, 21 IconChanged, 23 show the settings | MM |
| 456 | ODShape (OS/2) | 0 InitShape, 1 GetRegion, 3 a copy of the region | MM |
| 462 | ODStatusLineExtension | 0 SetStatusLineText | MM |
| 468 | ODStorageUnit | 1 GetDraft, 2 GetSession, 3 Exists, 7 Focus, 11 GetID, 14 AddProperty, 15 AddValue, 16 Remove, 17 CloneInto, 18 CreateView, 22 GetCurrentValueType, 24 SetOffset, 25 GetOffset, 27 SetValue, 30 GetSize, 31 IsValidStorageUnitRef, 32 GetStrongStorageUnitRef, 36 RemoveStorageUnitRef, 37 GetIDFromStorageUnitRef, 43 SetPromiseValue, 47 Lock, 48 Unlock | MM |
| 477 | ODStorageUnitView | 1 GetStorageUnit, 12 GetProperty, 13 GetType, 18 SetValue, 23 GetStrongStorageUnitRef | MM |
| 486 | ODTransform (OS/2) | 0 InitTransform, 1 GetMATRIXLF | MM |
| 504 | ODWindow | 0 GetPlatformWindow, 5 IsActive, 6 Open, 9 Show, 19 GetID, 22 Select | MM |
| 510 | ODWindowState | 0 RegisterWindow, 1 RegisterWindowForFrame, 2 AcquireWindow(id), 16 CopyBaseMenuBar, 29 CopyBasePopup | MM |
| 528 | OSAScriptingComponent | 7-39 (17 OSA methods: OSACompile, OSAExecute, OSALoad, OSAStore, OSADisplay, OSAScriptError, ...: REXXScriptingComponent overrides them) | OREXXSC |
| 1001 | AETE dictionary class (name unknown) | 2, 6, 9, 12, 14 | OREXXSC |
| 1004, 1013, 1016, 1019, 1034, 1040, 1043, 1046 | further dictionary classes (names unknown) | a few each | OREXXSC |

CClassData-only imports (C++ `new` ancestry checks): 203, 266, 272, 323,
392, 443, 452, 455, 485, 1000, 1021, 1030, 1033. NewClass imports: 205,
211, 412, 454, 457, 487, 529, 1002.

### ODPart's release order (from MMBasePart's overrides)

0 InitPart, 1 InitPartFromStorage, 2 DisplayFrameAdded, 3
DisplayFrameRemoved, 4 DisplayFrameConnected, 5 DisplayFrameClosed, 6
AttachSourceFrame, 7 FrameShapeChanged, 8 ViewTypeChanged, 9
PresentationChanged, 10 SequenceChanged, 11 LinkStatusChanged, 12
ContainingPartPropertiesUpdated, 13 ReadPartInfo, 14 WritePartInfo, 15
ClonePartInfo, 16 Open, 17 Draw, 18 FacetAdded, 19 FacetRemoved, 20
GeometryChanged, 21 HighlightChanged, 22 CanvasChanged, 23 CanvasUpdated,
24 GetPrintResolution, 25 BeginRelinquishFocus, 26 CommitRelinquishFocus,
27 AbortRelinquishFocus, 28 FocusAcquired, 29 FocusLost, 30
ExternalizeKinds, 31 ChangeKind, 32 HandleEvent, 33 AdjustMenus, 34
CreateRootMenuBar, 35 UndoAction, 36 RedoAction, 37 DisposeActionState, 38
WriteActionState, 39 ReadActionState, 40 FulfillPromise, 41 DropCompleted,
42 DragEnter, 43 DragWithin, 44 DragLeave, 45 Drop, 46 CreateLink, 47
RevealLink, 48 LinkUpdated, 49-53 not overridden (rs2: reserved), 54
EmbeddedFrameUpdated, 55 EditInLinkAttempted, 56 RequestEmbeddedFrame, 57
RemoveEmbeddedFrame, 58 RequestFrameShape, 59 UsedShapeChanged, 60
AdjustBorderShape, 61 AcquireContainingPartProperties, 62 RevealFrame, 63
EmbeddedFrameSpec, 64 CreateEmbeddedFramesIterator, 65 IsRealPart, 66
GetRealPart, 67 ReleaseRealPart. ODObject: 0 InitObject, 1 HasExtension,
2 AcquireExtension, 3 ReleaseExtension. ODRefCntObject: 0
InitRefCntObject, 1 Acquire, 2 Release, 3 GetRefCount. ODPersistentObject:
0 InitPersistentObject, 1 InitPersistentObjectFromStorage, 2 ReleaseAll, 3
Externalize, 4 GetStorageUnit, 5 GetID, 6 CloneInto.

### Functions

| Ord | Function (name assumed) | Use |
| --- | --- | --- |
| 810 | StorageUnitGetValue(su, ev, size, buffer) | MMODPTS reads the focused value (28 calls) |
| 901 | ODDeregisterPartHandlerClass(className) | MMODPTS's `RxDeInstallParts` |
| 906 | ODRegisterPartHandlerClass(className, dllName, 1, 0) | MMODPTS's `RxInstallParts` |
| 3-71 | the Apple Event Manager (AECoerceDesc 3, AECountItems 5, AECreateDesc 7, AECreateList 8, AECreateAppleEvent 10, AEDisposeDesc 16, AEDuplicateDesc 18, AEGetAttributePtr 22, AEGetNthPtr 24, AEGetNthDesc 28, AEGetParamDesc 32, AEGetParamPtr 33, AEInstallEventHandler 38, AEPutDesc 50, AEPutParamDesc 51, AERemoveEventHandler 55, AESend 61, ...) | OREXXSC only |

MMODPTS also needs SW.DLL (IBM's multimedia "sw" utility DLL: string and
graphic button helpers), MMPARTS.DLL and PMWP, which rs2 has (SW and
MMPARTS are IBM's, loaded from the CD).

## rs2's OPENDOC.DLL (userland/opendoc)

Built with somsup like rs2's other SOM libraries: every class above (and
the ones between them in the ordinal order that a runtime needs) is a
real SOM class with IBM's parent, ordinals and release order, so IBM's
parts subclass them and call through their tokens.

Real behaviour (milestone 1):

- ODObject/ODRefCntObject/ODPersistentObject: reference counts,
  extensions (none by default), storage unit and ID.
- ODSession (ODBaseSession): the per-process service objects (arbitrator,
  window state, dispatcher, storage system, clipboard, drag and drop, name
  space manager, ...) and the type tokenizer (Tokenize/GetType/RemoveEntry).
- Storage: ODStorageSystem, ODContainer, ODDocument, ODDraft,
  ODStorageUnit (properties holding typed values, focus, offsets,
  strong/weak storage unit references, clone keys) and ODStorageUnitView;
  a document is saved to rs2's own container file format (below) and read
  back.
- Parts: ODPart's 68 methods with OpenDoc's default behaviour, M_ODPart,
  part handler registration (ordinals 901/906; OS2.INI application
  `OpenDoc:PartHandlers`), ODDraft CreatePart/AcquirePart/ReleasePart
  finding the part's class through SOM's class manager (loading its DLL).
- Layout: ODFrame (containing frame, part, part info, view type,
  presentation, frame/used shapes, internal transform, facets), ODFacet
  (frame, clip shape, external transform, canvas, embedded facets, the
  frame/content/window transforms composed up the facet tree), ODShape
  (rectangles, fixed point), ODTransform (offset and scale: TransformPoint,
  InvertPoint, compose, copy, GetMATRIXLF), ODCanvas.

Stubs (they exist with the right tokens and return defaults): the
arbitrator's focus model is a simple owner table; menus, help, status
line, settings, clipboard, drag and drop, links, undo, translation, the
dispatcher and the docshell's event loop; the Apple Event Manager
functions (they fail with `errAEEventNotHandled`) and the OSA/AETE
classes.

### Container file format (rs2's own)

A document file is: the 8 bytes `RS2ODOC1`, a u32 count of storage
units, then per storage unit its u32 ID, u32 property count and per
property a u16-length name, u32 value count and per value a u16-length
type name and a u32-length byte string. All integers little-endian.
Storage unit 1 is the draft properties (the root part's storage unit
reference is its `OpenDoc:Draft:RootPart` property). A storage unit
reference inside a value is the 4-byte ID of the storage unit it names
(strong references with the top bit clear, weak ones with it set).

## Events, focus and menus (milestone 2)

- **ODDispatcher and ODDispatchModule** (ordinals 296-301). OS/2's
  ODEvent is a QMSG (`hwnd, msg, mp1, mp2, time, ptl`), as MMODPTS's
  HandleEvent reads it (`event+4` the message, `event+8` mp1). The
  session's dispatcher has a standard module (an ODDispatchModule) and
  the modules and monitors added by event type (a PM message, 0 for all);
  Dispatch calls the monitors, then the standard module, then the added
  modules (newest first) until one handles the event. A document window's
  client passes every message but WM_PAINT/WM_SIZE through it.
- **The standard module:**
  - Mouse messages (0x70-0x79, 0x410-0x42f) go to the facet under the
    pointer: the deepest, frontmost facet whose clip shape contains the
    point through its window transform. While a frame holds the `Mouse`
    focus (IBM's parts take it on button-down and capture the mouse),
    they go to that frame's facet. When a part does not handle an event,
    it goes up the frame tree if the frame propagates events.
  - A button-down in a frame that is not the active one first activates
    it. The `Selection`, `Menu` and `Key` foci are taken from their owner
    (its part gets CommitRelinquishFocus) and each is announced to the
    frame's part with FocusAcquired, as an OpenDoc part activating itself
    does (MMBasePart's ActivateFrame calls its own FocusAcquired).
  - WM_CHAR goes to the `Key` focus frame. WM_COMMAND, WM_MENUSELECT and
    WM_INITMENU go to the `Menu` focus frame; WM_INITMENU first calls the
    part's AdjustMenus. WM_ACTIVATE activates the root frame when nothing
    is active.
- **The active frame** gets a border round its facets.
- **Painting.** Each facet draws with the paint PS saved and restored,
  and the default view matrix reset after it, because IBM's parts leave
  it focused on their facet.
- **What a facet shows.** Hit-testing and the border use what a facet
  shows: its clip shape, else its frame shape placed at the frame origin.
  IBM's parts draw from the frame origin whatever the origin of the frame
  shape they ask for.
- **ODMenuBar** is a real PM menu.
  - Menus and items are kept by ID: text, enabled, checked, MENUITEM
    styles, items before others. Menu 0 means the bar's own menus.
  - Display builds the PM action bar (WC_MENU, FID_MENU) on the frame
    window of the frame with the menu focus, and the menu is rebuilt when
    a shown bar changes.
  - The window state's base menu bar (`SetBaseMenuBar`, copied by
    `CopyBaseMenuBar`, as IBM's parts do in InitPart) is shown for a part
    that shows none of its own.
  - rs2's `AddMenuLast(menuID, title, part)` takes the menu's title
    string where IBM's takes a PM menu window.
- **Shapes** are unions of disjoint rectangles: Union, Intersect,
  Subtract and Outset are exact, so a shape can have holes or be an L.
  GetRegion and GetPlatformShape make a GPI region of the rectangles.
  Polygons are kept as their bounding boxes.

## IBM's multimedia parts in use

`ODSHELL C:\MM.OD - MMODPTS MMAudioPart C:\MMOS2\SOUNDS\DSK_STRT.WAV`
makes the embedded part from a data file, as a dropped file is made into
a part. Its storage unit names its editor and holds the file name as
OS/2 OpenDoc's file contents (`OpenDoc:Property:Contents`,
`OS2:Type:FileType`), and IBM's InitPartFromStorage imports it.

What the import and playing needed:

- **MMIO compound names.** MMBasePart keeps its media data in its storage
  unit, as the MMIO element `.SU+pid+part+Contents+IBM:MMPM2:RawData.WAV`,
  through its own I/O procedure for the FOURCC `SU`. MMPARTS's
  mmioCopyData finds that procedure with mmioDetermineSSIOProc. rs2's
  MMIO now implements it as IBM's (in MMPM.DLL): the MMIOINFO's child
  procedure, else the extension before the first `+` (`|` with MMIOINFO
  flag 0x8000), and the element after it. mmioOpen takes the same
  procedure for such names. MDM reads compound element names through
  MMIO.
- **mciCreatePlayFilePalette** (MCIAPI 25). IBM's parts play through
  this "play file palette" window. rs2's palette is its own (Play and
  Stop buttons) and answers IBM's messages (PFPalProc):
  - 0x1000 play, 0x1001 stop
  - 0x1002/0x1003 set and query the file
  - 0x1004/0x1005 set and query the title
  - 0x100c the optimal size, 0x100d enable

  The file plays on a device opened for it.

Results:

- **MMAudioPart.** Clicking the part plays DSK_STRT.WAV. The QEMU HD
  Audio capture (`--sound hda --sound-wav`) holds the sound (673,000
  samples, peaks at ±32,500).
  Screenshot: work/progress/opendoc/odshell-mmaudiopart-playing.png.
- **MMImagePart** shows FLOWER.JPG, and a click activates it
  (odshell-mmimagepart-active.png).
- **MMVideoPart** was not tried: there is no AVI file on the CDs to test
  with.

## Tests and status

- `RS2\ODTEST.EXE` (55 checks, all pass): a session and its type
  tokens; shapes and transforms; the storage unit API (properties,
  values, focus, offsets, insert, views, references, ODException for a
  missing property); a document with a root part and an embedded part
  (ODTEST's own ODPart subclass built as the SOM compiler would) in
  frames and facets (frame, content and window transforms, aggregate clip
  shape, ContainsPoint, the arbitrator), saved to `C:\ODTEST.OD`, closed,
  reopened: parts, frames, shapes, view types and the frame tree come
  back.
- `ODTEST MMODPTS` (12 checks, all pass; IBM's MMODPTS.DLL with SW.DLL,
  MMPARTS.DLL and the Open Class runtime in LIBPATH, from a local
  work/modules.txt): MMODPTS loads on rs2's OPENDOC.DLL, its classes
  build on rs2's ODPart and M_ODPart, `ODRegisterPartHandlerClass`
  registers MMAudioPart with the four kinds its metaclass reports
  (`IBM:MMPM2:AudioPartKind`, `<DRM_OS2FILE,DRF_WAVE>`, `CF_WAVE`,
  `...OS2:Kind:WAVE`), CreatePart by kind runs IBM's InitPart (storage
  unit focus/add/strong references, the arbitrator's focus set, window
  state, extensions), the draft is saved and reopened (InitPartFromStorage).
- `ODSHELL.EXE FILE [KIND [DLL CLASS]]` (`\OS2\ODSHELL.EXE`): a simple
  document shell. It opens or makes the document, registers a window with
  ODWindowState (OPENDOC.DLL makes the PM frame window, the root frame,
  the root facet on a canvas around the client window, and draws the
  facet tree on WM_PAINT into the paint PS), and saves the draft when the
  window closes. A new document's root part is ODSHELL's RS2TextPart with
  an embedded part of KIND: another RS2TextPart
  (work/progress/opendoc/odshell-embedded-part.png), or IBM's MMAudioPart,
  which negotiates its frame and draws itself
  (odshell-ibm-mmaudiopart.png).

Milestone 2 tests:

- **`ODTEST`: 71 checks, all pass.** New since milestone 1:
  - dispatch, focus and menus: a click in an embedded facet reaches its
    part and activates its frame (three FocusAcquired calls); WM_CHAR
    follows the key focus; a menu command goes to the menu-focus part,
    then to a dispatch module of ODTEST's own; events outside every facet
    are not dispatched; the menu bar API, copies and the base menu bar
  - four checks of shapes with holes
- **ODSHELL, driven with QEMU pointer and key input:**
  - A click activates the embedded part (its active border); typed keys
    edit the active part.
  - The Document/Edit/Help menus show, Edit/Cut reaches the active part,
    and Document/Save reaches the shell's own dispatch module.
  - Screenshots: work/progress/opendoc/odshell-click-type-active-border.png,
    odshell-edit-menu.png, odshell-after-cut.png.

## Settings, clipboard, drag and drop, help (milestone 3)

- **Settings** (`settings.rs`). ODSettingsExtension's ShowSettings
  (index 23) opens a PM notebook window and asks the extension for its
  pages with AddNotebookPages(notebook, flags, reserved) (index 18)
  through its token, so a subclass adds its own pages and calls its
  parent for the standard ones. IBM's MMSettingsExtension passes
  flags | 0x1f. rs2's standard pages, by flag:
  - 0x01 Name: the part's `OpenDoc:Metadata:Name`; Apply stores it and
    calls TitleChanged (19)
  - 0x02 Type, 0x04 View (view-type buttons calling ChangeViewType),
    0x08 Storage, 0x10 Presentation

  IBM's parts pass a facet (or a frame) to ShowSettings. ODSHELL's
  Selected/Settings (0x501a) opens the part's own extension
  ("OpenDoc Extension Info Settings"), else a plain ODSettingsExtension.
- **Menus and popups.** IBM's menu IDs are used: Edit 0x0e, Selected 0x11,
  Help 0x12. The ODMenuBar fixes that made MMODPTS's own items show:
  - OS/2's AddMenuItemLast/Before take the item ID from the MENUITEM.
  - IDs are USHORTs; callers leave garbage in the high word, so IDs are
    masked to 16 bits.

  Popups:
  - The window state's base popup (SetBasePopup) shows with WinPopupMenu
    at the pointer on a button-2 context menu (WM_CONTEXTMENU) that no
    part takes.
  - A popup's items are its top-level entries.

  Opening menus:
  - WM_INITMENU is 0x33. The active part's AdjustMenus runs as a menu
    opens, so MMODPTS enables, disables and renames its items.
  - A refresh while a menu is open updates the PM items in place
    (MM_SETITEMATTR/MM_SETITEMTEXT), so the open menu stays open.
- **The play palette** (MCIAPI). The state observer is called as IBM's
  PFPalSetState calls it: state 0 playing, 1 stopped. The palette also
  has:
  - Stop, enabled only while playing
  - a position slider (0-100) that follows the file while it plays and
    seeks when moved
  - 0x100a/0x100b to set and query the position

  At the end of the file the palette stops itself. MMAudioPart gets the
  notifications and answers with 0x1006.
- **Clipboard** (`clipboard.rs`). ODClipboard is on PM's clipboard:
  - ExportClipboard (and the clipboard's update) puts out:
    - the registered value types in their PM formats (RegisterClipboardFormat)
    - text as CF_TEXT
    - the contents unit and the units it refers to, in rs2's private
      format "rs2 OpenDoc storage units" (a tag, the root ID and the
      encoded units)
  - GetContentStorageUnit reads back what another program put there.
  - Storage unit references go through a per-unit reference table
    (`rs2:StorageUnitRefs`). An ODStorageUnitRef is an index into it, as
    in OpenDoc.
  - CloneInto copies the units that strong references name into the
    destination draft, once per clone key.

  ODSHELL's Edit menu:
  - Cut/Copy/Paste move RS2TextPart's text to and from other PM programs.
  - Copy part externalizes the active part and clones it into the
    clipboard.
  - Paste part clones it into the document and embeds it.
- **Drag and drop** (`dnd.rs`):
  - **Starting a drag.** StartDrag, and a button-2 drag (WM_BEGINDRAG) of
    an embedded part, run PMDRAG's DrgDrag. The part's frame goes as an
    `<DRM_OPENDOC,DRF_PART>` item.
  - **Dropping.** A document window's client answers DM_DRAGOVER and
    DM_DROP:
    - A part of the same document moves to the drop point.
    - A part from another document is cloned and embedded there.
    - An OS/2 file (`DRM_OS2FILE`) of a registered kind becomes a part.
      The kind comes from `OpenDoc:PartKinds`, with aliases WAV→WAVE,
      image extensions, MID→MIDI and AVI. Failing that, the file goes to
      MMODPTS's parts when they are registered.
  - **The part made from a file.** Its storage unit names the handler and
    holds the file name as `OpenDoc:Property:Contents`/`OS2:Type:FileType`,
    as IBM's parts import dropped files.
  - **Tried.** DSK_STRT.WAV dragged from a WPS folder (rs2's PMWP) onto
    ODSHELL becomes an IBM MMAudioPart at the drop point.
- **Help and the status line** (`help.rs`):
  - **Help.** ODHelp's DisplayHelp(file, panel), DisplayHelpIndex(file)
    and DisplayHelpUsingHelp use HELPMGR. There is one help instance per
    library, found on HELP and associated with the active window.
    MMODPTS's Help index (0x5022) and General help (0x5023) show
    MMPARTS.HLP. The file must be on the drive: the worktree's
    modules.txt has `work/unpacked/disk_14/MMTEMP/MMPARTS.HLP ->
    OS2\HELP\MMPARTS.HLP`.
  - **The status line.** The session has IBM's
    `OpenDoc:Extension:StatusLine`. MMODPTS acquires it in InitPart and
    calls it without checking, so it must exist.
    - ODStatusLineExtension's SetStatusLineText(text, frame) shows the
      text in a line at the bottom of the frame's document window.
    - The line is hidden while its text is empty.
    - MMODPTS puts its menu hints there on WM_MENUSELECT; so does ODSHELL
      for its own items.

Milestone 3 tests:

- **`ODTEST`: 89 checks, all pass.** New:
  - the settings notebook: a subclass's AddNotebookPages gets the
    notebook and its parent adds the standard pages
  - the clipboard: text out as CF_TEXT and back in; another program's
    text imported; a part with references cloned
  - drag and drop: DM_DRAGOVER/DM_DROP of a registered file kind embeds
    a part made from the file at the drop point; a part of the document
    dragged within it moves
  - the status line: SetStatusLineText shows and hides the line
  - ODHelp opens a library's index, and fails for a missing library
- **`ODTEST MMODPTS`: 12 checks, all pass.** MMIOTEST and AUDTEST pass.
- **ODSHELL, driven with QEMU input.** Screenshots in
  work/progress/opendoc/:
  - mmaudiopart-settings-notebook.png, textpart-settings-notebook.png
  - mmaudiopart-selected-menu.png
  - clipboard-text-pasted.png, clipboard-part-pasted.png
  - dnd-part-moved.png, dnd-wav-dropped-mmaudiopart.png
  - statusline-mmaudiopart-menu-hint.png, help-mmparts-index.png

Still stubs or missing:

- links, undo, translation and name spaces
- polygons as shapes
- nested drafts and draft history
- the window state's own persistence
- **Drag and drop:**
  - the drag image is the system's file pointer, not a picture of the
    part
  - rendering exchange (DM_RENDER) for parts dragged to other programs
    is not done
- **Status line:** it stays until the next text (IBM clears it when the
  `Statusline` focus moves); it overlaps the bottom of the root facet.
- **Selected menu:** IBM's "View as" items live in a submenu 0x501b that
  ODSHELL's flat Selected menu does not have (CheckMenuItem on it finds
  nothing).

OREXXSC.DLL needs the Apple Event Manager, OSA and SOMUC.DLL. Calls with
more than 12 words (ODWindowState::RegisterWindow takes 13) must be made
directly, not through `somsup::call`.

Next steps:

- the "View as" submenu (0x501b) as IBM's base menu has it
- rendering exchange for parts dropped on other programs
- MMVideoPart with an AVI file
- the Apple Event Manager for OREXXSC

## OREXXSC: Object REXX scripting (milestone 4)

### Inventory

OREXXSC.DLL (IBM's, disk 4 `OS2\DLL`, symbols in
`debug/symbols/os2/dll/orexxsc.sym`) defines one SOM class,
REXXScriptingComponent (a subclass of OSAScriptingComponent, made in its
SOMInitModule), and exports the native methods of the REXX classes that
script OpenDoc parts (ScriptableApp_Connect, _Disconnect, _Unknown,
_Novalue; SCList_, SCRange_, SCReference_, ObjectSpecifier_MakeAEDesc).
Its imports, read from its fixup records:

| Module | Ordinals | What |
|---|---|---|
| REXX | 16, 18, 30-36, 39, 42-46, 49, 50, 54, 55, 65, 68, 74, 75, 77-80, 92-104 | Object REXX's REXX.DLL (IBM's OREXX.DLL, made REXX.DLL by SWITCHRX): RexxTerminate, RexxSomInitialize, the native-method API (REXX_ARRAY_AT/PUT/SIZE/NEW/NEW1/NEW2, REXX_BUFFER_NEW, REXX_INTEGER_VALUE/NEW, REXX_DOUBLE, REXX_EXCEPT, REXX_INTEGER, REXX_ISINTEGER, REXX_ISASTRING, REXX_SEND, REXX_STRING_DATA/NEW, REXX_NIL, REXX_TRUE, REXX_ENVIRONMENT, REXX_LOCAL, REXX_UNSIGNED_INTEGER, REXX_ISDIRECTORY) and the OSA support calls (RexxCreateMethod 92, RexxRunMethod 93, RexxStoreMethod 94, RexxLoadMethod 95, RexxCreateDirectory 96, RexxRemoveDirectory 97, RexxDispose 98, RexxSetOSAInfo 99, RexxGetOSAInfo 100, RexxResultString 101, RexxGetSource 102, RexxCopyMethod 103, RexxValidObject 104) |
| REXXCRT | 59 free, 66 malloc, 97/154 _CRT_init/_CRT_term, 105, 112, 126 | IBM's C runtime DLL from the Object REXX package (on the CD) |
| DOSCALLS | 299 DosAllocMem, 304 DosFreeMem, 312 DosGetInfoBlocks | |
| som | 3, 14, 49, 50, 58 | somBuildClass, somParentNumResolve, the class data |
| somuc | `somf_MCollectibleCClassData` (by name) | SOMUC.DLL: the parent of the Apple-event terminology classes |
| OPENDOC | 3-71, 528/529, 1000-1046 | the Apple Event Manager, OSAScriptingComponent, the terminology classes |

**The Apple Event Manager** (the functions OREXXSC calls, their
arguments as it passes them; Apple's names where the use matches
Apple's published API):

| Ordinal | Function | Arguments |
|---|---|---|
| 3 | AECoerceDesc | desc, toType, result |
| 5 | AECountItems | list, *count |
| 7 | AECreateDesc | type, data, size, result |
| 8 | AECreateList | factoring, factoredSize, isRecord, result |
| 10 | AECreateAppleEvent | class, id, target, returnID, transactionID, result |
| 11 | CreateObjSpecifier | class, container, keyForm, keyData, dispose, result |
| 12 | (two arguments: a descriptor and a result: SCReference) | |
| 13 | CreateRangeDescriptor | start, stop, dispose, result |
| 16 | AEDisposeDesc | desc |
| 18 | AEDuplicateDesc | desc, result |
| 22 | AEGetAttributePtr | event, keyword, type, *type, data, max, *size |
| 24 | (OS/2: a descriptor's data) | desc, *type, data, max, *size |
| 28 | AEGetNthDesc | list, index, type, *keyword, result |
| 32 | AEGetParamDesc | event, keyword, type, result |
| 33 | AEGetParamPtr | event, keyword, type, *type, data, max, *size |
| 38 | AEInstallEventHandler | class, id, handler, refcon, isSystem |
| 42 | (three arguments: a descriptor, -1 (timeout?), a pointer) | |
| 50 | AEPutDesc | list, index, desc |
| 51 | AEPutParamDesc | event, keyword, desc |
| 55 | AERemoveEventHandler | class, id, handler, isSystem |
| 61 | AESend | event, reply, mode, priority, timeout, idle, filter |
| 66 | (OS/2: a descriptor's data size) | desc, *size |
| 67 | AESizeOfNthItem | list, index, *type, *size |
| 68 | AESizeOfParam | event, keyword, *type, *size |
| 71 | (three arguments, in OSAEventToRexxCode) | |

OREXXSC never looks inside an AEDesc (8 bytes: the descriptor type and
a data handle): it reads data only through 24 and 66.

**OSAScriptingComponent** (528). REXXScriptingComponent overrides 17
methods, and its own methods call its parent's through these tokens.
The indices are fixed by those calls: OSADoScript calls OSACompile (7),
OSAExecute (14) and OSADisplay (10), and OSALoadExecute calls OSALoad
(24). The other names are placed in Apple's OSA call names in
alphabetical order, which fits every fixed index:

- 7 OSACompile, 8 OSACompileExecute, 9 OSACopyID, 10 OSADisplay
- 11 OSADispose, 13 OSADoScript, 14 OSAExecute, 21 OSAGetScriptInfo
- 23 OSAGetSource, 24 OSALoad, 25 OSALoadExecute, 27 OSAScriptError
- 28 OSAScriptingComponentName, 34 OSASetScriptInfo
- 36 OSAStartRecording, 37 OSAStopRecording, 38 OSAStore

REXXScriptingComponent does the work through Object REXX:

- somInit calls RexxSomInitialize and RexxCreateDirectory (a directory
  named "OPENDOC" holds the script objects).
- OSACompile: RexxCreateMethod(dir, source RXSTRING, *id, *error info)
  and RexxSetOSAInfo(id, 'modf', mode).
- OSAExecute: RexxRunMethod(dir, id, *result id, *error info).
- OSADisplay: RexxResultString(id, RXSTRING), which allocates the text
  with DosAllocMem; OREXXSC frees it.
- OSAGetSource: RexxGetSource. OSAStore and OSALoad: RexxStoreMethod and
  RexxLoadMethod, data type 'orex' or 'scpt'.
- OSACopyID: RexxCopyMethod. OSADispose: RexxDispose.
- OSAGetScriptInfo/OSASetScriptInfo: RexxGetOSAInfo/RexxSetOSAInfo.
- OSAScriptError: 'errn' (the error number / 1000), 'errs' (the message),
  'errb' (the brief message), from the 6-word error information that
  RexxCreateMethod/RexxRunMethod fill.

Script IDs are Object REXX objects (REXXOBJECT handles).

**The terminology classes** (1000-1046): OREXXSC uses four CClassData
(1000, 1021, 1030, 1033), which it compares with an object's method
table to tell term kinds apart. It also uses ClassData 1001 (with
NewClass 1002), 1004, 1013, 1016, 1019, 1034, 1040, 1043 and 1046, which
it calls with methods 0-19. At three ordinals per class, that is 16
classes; their names are not known (IBM's OPENDOC.DLL is not on the
CDs). ScriptableApp_Connect makes an instance of 1001 and asks it to
read the target's terminology (its 'aete'); ScriptableApp_Unknown and
OSAEventToRexxCode turn REXX messages into Apple events and back with it.

**The REXX side.** OREXXSC builds REXX objects of classes it finds in
`.environment`: SCREFERENCE, SCRELATIVEPOSITION, OBJECTSPECIFIER (with
SETCONTAINER), SCLIST (INSERT), SCRANGE and SCRIPTABLEAPP (GETAETE,
SETAETE, GETNAME, SETNAME, GETCURRAPP, SETCURRAPP, SETENTRY,
GETDIRPARM, RESETDIRPARM). IBM keeps these in its Object REXX image
(REXX.IMG), whose code calls OREXXSC's exports as native methods.

**Native methods.** A native method is an exported function
`f(void **args)`:

- Called with NULL, it returns its signature: a zero-terminated byte
  string, the result type first. The codes are 2 object, 4 long,
  7 self and 0x10 an optional object (OREXXSC's: Connect 02 07 10,
  Unknown 02 07 02 02, Disconnect 04 07).
- Otherwise `args[0]` points to the result and `args[1..]` are the
  arguments in signature order.

### Plan

1. SOMUC.DLL, rs2's own (`userland/somuc`): somf_MCollectible and the
   utility classes.
2. The Apple Event Manager in OPENDOC.DLL: descriptors, lists, records,
   Apple events (attributes and parameters), coercion, event handlers
   and AESend to handlers in the process.
3. OSAScriptingComponent's methods in that order (errOSA... until a
   subclass overrides them).
4. OREXX.DLL: the OSA support calls (92-104), RexxSomInitialize and
   RexxTerminate, the native-method API, and native methods
   (`::METHOD name EXTERNAL 'LIBRARY dll entry'`).
5. ODTEST: SOMUC and the AE Manager with rs2's files only;
   `ODTEST OREXXSC` loads IBM's OREXXSC on rs2's OREXX (as REXX.DLL in
   a directory of its own) and compiles, runs, shows, stores and loads
   REXX scripts through OSA.
