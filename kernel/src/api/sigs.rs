//! Argument signatures of the 16-bit `_Pascal` APIs.
//!
//! One character per argument, left to right: `w` = 16-bit word, `d` =
//! 32-bit value, `p` = 16:16 far pointer. The gate needs these to find the
//! arguments and to pop them on return (`retf n`).

const DOSCALLS: &[(u16, &str)] = &[
    (2, "wwppw"),      // DosCWait
    (3, ""),           // DosEnterCritSec
    (5, "ww"),         // DosExit
    (6, ""),           // DosExitCritSec
    (7, "wp"),         // DosExitList
    (8, "pp"),         // DosGetInfoSeg
    (9, "wpw"),        // DosGetPrty
    (10, "ww"),        // DosKillProcess
    (11, "wwww"),      // DosSetPrty
    (12, "p"),         // DosPTrace
    (13, "w"),         // DosHoldSignal
    (14, "pppww"),     // DosSetSigHandler
    (15, "wwww"),      // DosFlagProcess
    (16, "ppw"),       // DosMakePipe
    (20, "dd"),        // DosSemSetWait
    (22, "ppd"),       // DosMuxSemWait
    (23, "d"),         // DosCloseSem
    (24, "wpp"),       // DosCreateSem
    (25, "pp"),        // DosOpenSem
    (26, "w"),         // DosResumeThread
    (27, "w"),         // DosSuspendThread
    (28, "p"),         // DosSetDateTime
    (29, "ddp"),       // DosTimerAsync
    (30, "ddp"),       // DosTimerStart
    (31, "w"),         // DosTimerStop
    (32, "d"),         // DosSleep
    (33, "p"),         // DosGetDateTime
    (34, "wpw"),       // DosAllocSeg
    (35, "wpp"),       // DosAllocShrSeg
    (36, "pp"),        // DosGetShrSeg
    (37, "wwp"),       // DosGiveSeg
    (38, "ww"),        // DosReallocSeg
    (39, "w"),         // DosFreeSeg
    (40, "wwpww"),     // DosAllocHuge
    (41, "p"),         // DosGetHugeShift
    (42, "www"),       // DosReallocHuge
    (43, "wp"),        // DosCreateCSAlias
    (44, "pwpp"),      // DosLoadModule
    (45, "wpp"),       // DosGetProcAddr
    (46, "w"),         // DosFreeModule
    (47, "pp"),        // DosGetModHandle
    (48, "wwp"),       // DosGetModName
    (49, "p"),         // DosGetMachineMode
    (50, "ww"),        // DosBeep
    (51, ""),          // DosCLIAccess
    (52, "pww"),       // DosDevConfig
    (53, "ppwww"),     // DosDevIOCtl
    (56, "w"),         // DosBufReset
    (57, "pd"),        // DosChDir
    (58, "wdwp"),      // DosChgFilePtr
    (59, "w"),         // DosClose
    (60, "pd"),        // DosDelete
    (61, "wp"),        // DosDupHandle
    (62, "wpp"),       // DosFileLocks
    (63, "w"),         // DosFindClose
    (64, "ppwpwpd"),   // DosFindFirst
    (65, "wpwp"),      // DosFindNext
    (66, "pd"),        // DosMkDir
    (67, "ppd"),       // DosMove
    (68, "wd"),        // DosNewSize
    (69, "wwww"),      // DosPortAccess
    (70, "pppdwwwd"),  // DosOpen
    (71, "wpp"),       // DosQCurDir
    (72, "pp"),        // DosQCurDisk
    (73, "wp"),        // DosQFHandState
    (74, "wwpw"),      // DosQFileInfo
    (75, "ppd"),       // DosQFileMode
    (76, "wwpw"),      // DosQFSInfo
    (77, "wpp"),       // DosQHandType
    (78, "p"),         // DosQVerify
    (80, "pd"),        // DosRmDir
    (81, "w"),         // DosSelectDisk
    (82, "ww"),        // DosSetFHandState
    (83, "wwpw"),      // DosSetFileInfo
    (84, "pwd"),       // DosSetFileMode
    (85, "w"),         // DosSetMaxFH
    (86, "w"),         // DosSetVerify
    (89, "wpp"),       // DosSetVec
    (90, "wwwp"),      // DosSysTrace
    (91, "pp"),        // DosGetEnv
    (92, "p"),         // DosGetVersion
    (94, "p"),         // DosGetPID
    (95, "pppdwwdpd"), // DosOpen2
    (97, "wwpw"),      // DosSetFSInfo
    (98, "pwpwd"),     // DosQPathInfo
    (99, "pwpwwww"),   // DosDevIOCtl2
    (104, "pwpwwd"),   // DosSetPathInfo
    (120, "w"),        // DosError
    (121, "w"),        // DosGetSeg
    (122, "w"),        // DosLockSeg
    (123, "w"),        // DosUnlockSeg
    (126, "wp"),       // DosSizeSeg
    (127, "p"),        // DosMemAvail
    (129, "wpwpw"),    // DosPhysicalDisk
    (130, "wpp"),      // DosGetCp
    (134, "ww"),       // DosSendSignal
    (137, "wpwp"),     // DosRead
    (138, "wpwp"),     // DosWrite
    (139, "wppp"),     // DosErrClass
    (140, "dd"),       // DosSemRequest
    (141, "d"),        // DosSemClear
    (142, "dd"),       // DosSemWait
    (143, "d"),        // DosSemSet
    (144, "pwwpppp"),  // DosExecPgm
    (145, "ppp"),      // DosCreateThread
    (146, "www"),      // DosSubSet
    (147, "wpw"),      // DosSubAlloc
    (148, "www"),      // DosSubFree
    (149, "wpppwp"),   // DosReadAsync
    (150, "wpppwp"),   // DosWriteAsync
    (151, "wpppw"),    // DosSearchPath
    (152, "pp"),       // DosScanEnv
    (153, "ww"),       // DosSetCp
    (155, "wwwp"),     // DosGetResource
    (156, "wp"),       // DosGetPPID
    (157, "p"),        // DosCallback
    (160, "w"),        // DosR2StackRealloc
    (161, "pd"),       // DosFSRamSemRequest
    (162, "p"),        // DosFSRamSemClear
    (163, "pp"),       // DosQAppType
    (164, "ww"),       // DosSetProcCp
    (166, "wpw"),      // DosQSysInfo
    (181, "pppwwd"),   // DosFSAttach
    (182, "pwwppd"),   // DosQFSAttach
    (183, "pwppwpwpwwd"), // DosFSCtl
    (184, "ppwpwpwd"), // DosFindFirst2
    (185, "ppd"),      // DosMkDir2
    (186, "wpwp"),     // DosFileIO
    (187, "w"),        // DosFindNotifyClose
    (188, "ppwpwpwdd"), // DosFindNotifyFirst
    (189, "wpwpd"),    // DosFindNotifyNext
    (191, "wpppw"),    // DosEditName
    (201, "ppwd"),     // DosCopy
    (203, "pd"),       // DosForceDelete
    (204, "wpdpdpdd"), // DosEnumAttribute
    (206, "d"),        // DosShutdown
    (207, "wwwp"),     // DosGetResource2
    (208, "p"),        // DosFreeResource
    (214, "p"),        // DosMaxPathLen
    (215, "p"),        // DosPageSize
    (423, ""),         // DosFlatToSel: register based (EAX in and out)
    (424, ""),         // DosSelToFlat: register based (EAX in and out)
    (567, "pppddwwdpp"), // Dos16OpenL (from CMD.EXE call sites)
    (568, "wddcp"),    // Dos16SetFilePtrL (LONGLONG as two dwords)
    (570, "wdd"),      // Dos16SetFileSizeL
    (690, "p"),        // DosSMSetTitle
    (712, "p"),        // DosSMPMPresent
    (998, "pd"),       // DosSetExtLIBPATH
    (999, "pd"),       // DosQueryExtLIBPATH
];

// VIOCALLS, KBDCALLS and MOUCALLS are generated from bsesub.h by
// tools/scripts/gen_sigs16.py.
const VIOCALLS: &[(u16, &str)] = &[
    (1, "w"), // VioEndPopUp
    (2, "pw"), // VioGetPhysBuf
    (3, "pw"), // VioGetAnsi
    (5, "ww"), // VioSetAnsi
    (6, ""), // VioDeRegister
    (7, "wwwwwpw"), // VioScrollUp
    (8, "w"), // VioPrtSc
    (9, "ppw"), // VioGetCurPos
    (10, "pwwww"), // VioWrtCellStr
    (11, "pw"), // VioPopUp
    (12, "wwwwwpw"), // VioScrollRt
    (13, "pwwww"), // VioWrtCharStr
    (15, "www"), // VioSetCurPos
    (18, "w"), // VioScrUnLock
    (19, "pww"), // VioWrtTTY
    (21, "pw"), // VioGetMode
    (22, "pw"), // VioSetMode
    (23, "wpw"), // VioScrLock
    (24, "ppwww"), // VioReadCellStr
    (25, "wpw"), // VioSavRedrawWait
    (26, "pwwww"), // VioWrtNAttr
    (27, "pw"), // VioGetCurType
    (28, "www"), // VioSavRedrawUndo
    (29, "pw"), // VioGetFont
    (30, "ppwww"), // VioReadCharStr
    (31, "ppw"), // VioGetBuf
    (32, "pw"), // VioSetCurType
    (33, "pw"), // VioSetFont
    (35, "www"), // VioModeUndo
    (37, "wpw"), // VioModeWait
    (40, "wpw"), // VioGetCp
    (42, "www"), // VioSetCp
    (43, "www"), // VioShowBuf
    (44, "wwwwwpw"), // VioScrollLf
    (45, "ppdd"), // VioRegister
    (46, "wpw"), // VioGetConfig
    (47, "wwwwwpw"), // VioScrollDn
    (48, "pwwwpw"), // VioWrtCharStrAtt
    (49, "pw"), // VioGetState
    (50, "w"), // VioPrtScToggle
    (51, "pw"), // VioSetState
    (52, "pwwww"), // VioWrtNCell
    (53, "pwwww"), // VioWrtNChar
    (70, "ppddd"), // VioGlobalReg (RETF 20 in DOSCALL1)
    (75, "pwww"), // VioCheckCharType
];

const KBDCALLS: &[(u16, &str)] = &[
    (1, "pw"), // KbdSetCustXt
    (3, "dpw"), // KbdGetCp
    (4, "pww"), // KbdCharIn
    (5, "www"), // KbdSetCp
    (7, "w"), // KbdSynch
    (8, "ppd"), // KbdRegister
    (9, "ppww"), // KbdStringIn
    (10, "pw"), // KbdGetStatus
    (11, "pw"), // KbdSetStatus
    (12, "ww"), // KbdGetFocus
    (13, "w"), // KbdFlushBuffer
    (14, "pw"), // KbdXlate
    (17, "w"), // KbdClose
    (18, "w"), // KbdFreeFocus
    (20, ""), // KbdDeRegister
    (21, ""), // KbdSetFgnd
    (22, "pw"), // KbdPeek
    (23, "p"), // KbdOpen
    (24, "pw"), // KbdGetHWID
    (25, "pw"), // KbdSetHWID
];

const MOUCALLS: &[(u16, &str)] = &[
    (1, "ppw"), // MouGetPtrShape
    (2, "ppw"), // MouSetPtrShape
    (3, "pw"), // MouGetNumMickeys
    (6, "pw"), // MouGetScaleFact
    (7, "w"), // MouFlushQue
    (8, "pw"), // MouGetNumButtons
    (9, "w"), // MouClose
    (11, "pw"), // MouSetScaleFact
    (13, "pw"), // MouGetNumQueEl
    (14, ""), // MouDeRegister
    (15, "pw"), // MouGetEventMask
    (16, "pw"), // MouSetEventMask
    (17, "pp"), // MouOpen
    (18, "pw"), // MouRemovePtr
    (19, "pw"), // MouGetPtrPos
    (20, "ppw"), // MouReadEventQue
    (21, "pw"), // MouSetPtrPos
    (22, "pw"), // MouGetDevStatus
    (23, "w"), // MouSynch
    (24, "ppd"), // MouRegister
    (25, "pw"), // MouSetDevStatus
    (26, "w"), // MouDrawPtr
    (27, "p"), // MouInitReal
    (29, "pw"), // MouGetThreshold
    (30, "pw"), // MouSetThreshold
];

const MSG: &[(u16, &str)] = &[
    (7, "pwppp"),    // DosIQueryMessageCP
    (1, "wwp"),      // DosPutMessage
    (2, "pwpwwppp"), // DosTrueGetMessage
    (3, "pwpwpwp"),  // DosInsMessage
];

const NLS: &[(u16, &str)] = &[
    (1, "wpp"),  // DosCaseMap
    (2, "wppp"), // DosGetCollate
    (3, "wppp"), // DosGetCtryInfo
    (4, "wpp"),  // DosGetDBCSEv
];

const SESMGR: &[(u16, &str)] = &[
    (8, "wwd"),  // DosStopSession
    (9, "wd"),   // DosSelectSession
    (14, "wp"),  // DosSetSession
    (17, "ppp"), // DosStartSession
];

const QUECALLS: &[(u16, &str)] = &[
    (1, "wpppwwpd"), // DosReadQueue
    (2, "w"),        // DosPurgeQueue
    (3, "w"),        // DosCloseQueue
    (4, "wp"),       // DosQueryQueue
    (5, "wppppwpd"), // DosPeekQueue
    (6, "wwwpw"),    // DosWriteQueue
    (7, "ppp"),      // DosOpenQueue
    (8, "pwp"),      // DosCreateQueue
];

/// Argument bytes popped by 16-bit entries of DOSCALL1, measured from the
/// `RETF n` of each entry point (tools/ghidra/RetfScan.java). Used when no
/// typed signature is known, so the gate can at least return correctly.
pub fn retf_bytes(ordinal: u16) -> Option<u8> {
    super::sigs_retf::RETF
        .binary_search_by(|e| e.0.cmp(&ordinal))
        .ok()
        .map(|i| super::sigs_retf::RETF[i].1)
}

pub fn lookup(module: u8, ordinal: u16) -> Option<&'static [u8]> {
    let table = match module {
        0 => DOSCALLS,
        1 => VIOCALLS,
        2 => KBDCALLS,
        3 => MOUCALLS,
        4 => MSG,
        5 => NLS,
        6 => SESMGR,
        7 => QUECALLS,
        _ => return None,
    };
    table.iter().find(|e| e.0 == ordinal).map(|e| e.1.as_bytes())
}

/// Byte size of a signature character.
pub fn size(c: u8) -> u32 {
    if c == b'w' { 2 } else { 4 }
}
// `c` is a 32-bit count/flags value (same size as `d`).
