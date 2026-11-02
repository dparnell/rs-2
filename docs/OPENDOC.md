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

Not done yet (stubs that keep the tokens): event dispatching to parts
(ODDispatcher; ODSHELL's window does not route mouse and keys to facets
yet), the arbitrator beyond an owner table, menus (ODMenuBar records
items, no PM menu), help, status line and settings extensions, clipboard
and drag and drop (their storage units exist, nothing is exported),
links, undo, translation, name spaces, GPI regions for shapes
(GetRegion returns none; shapes are rectangles), nested drafts and
draft history, the window state's own persistence. OREXXSC.DLL needs the
Apple Event Manager, OSA and SOMUC.DLL. Calls with more than 12 words
(ODWindowState::RegisterWindow takes 13) must be made directly, not
through `somsup::call`.

Next steps: dispatch PM input to facets (HandleEvent with OS/2's ODEvent),
focus and menus through the arbitrator and window state, settings
notebook pages (ODSettingsExtension) so MMODPTS's parts can be opened and
played, GPI regions, then the Apple Event Manager for OREXXSC.
