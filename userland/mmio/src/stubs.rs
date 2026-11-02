//! Entry points of IBM's MMIO that rs2 does not implement: compound
//! (BND) files, the MMPMMMIO.INI procedure list, CODECs and IBM's
//! internal helpers. They fail with MMIOERR_UNSUPPORTED_FUNCTION (NULL
//! for the calls that return a handle or procedure; the I/O procedures
//! answer MMIOERR_UNSUPPORTED_MESSAGE).

use crate::err;

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpCleanupInstance() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListCreate() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListAdd() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListGet() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListFind() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListInsert() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListChange() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListDelete() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListGetHeader() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpConvertAddressToHmmio() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpConvertHmmioToAddress() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpFreeHmmioEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGrowHmmioTable() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGenerateHmmio() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpAcquireMmioSem() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpDiscardMmioSem() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpReadIniFile() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpLoadDefIOProcs() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpUnloadDefIOProcs() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpRestoreInfoState() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpAllocateInternalBuffer() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpExpandMemFile() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn BNDIOProc() -> u32 {
    err::UNSUPPORTED_MESSAGE
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFOpen() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFClose() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFAddEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFDeleteEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFChangeEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFFindEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFAddElement() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFGetInfo() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFSetInfo() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFCopy() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpBuildCtoc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpCreateCtoc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpRewriteCtoc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetHeaderInfo() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpUpdateCtocHeader() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpChangeCtocHeader() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetRemainingSize() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpFindOpenCtoc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetFullPathName() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpAddCtocInstance() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpRemoveCtocInstance() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpLastModTime() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpOpenTempElement() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpCloseTempElement() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpTempName() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpRemoveSharingEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpAddSharingEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpCheckSharingMode() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpCheckHmmcfSharingMode() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpListTransferNode() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetFormatStringLength() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetFormatString() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetNLSData() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn CFIOProc() -> u32 {
    err::UNSUPPORTED_MESSAGE
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioIdentifyFileFormat() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

/// mmioDetermineSSIOProc(name, info, &fcc, element): the storage system
/// procedure of a compound name, as IBM's (in MMPM.DLL): the child
/// procedure the MMIOINFO names, else the extension of the part before
/// the first '+' ('|' with MMIOINFO flag 0x8000): "A.SU+B" gives 'SU  '
/// and the element "B". 0 when found, -1 when not.
#[unsafe(no_mangle)]
pub extern "C" fn mmioDetermineSSIOProc(name: *const u8, info: *const crate::MmioInfo, fcc: *mut u32, element: *mut u8) -> u32 {
    if fcc.is_null() || (name.is_null() && info.is_null()) {
        return 0x1969; // MMIOERR_INVALID_PARAMETER
    }
    unsafe { *fcc = 0 };
    let mut sep = b'+';
    if let Some(i) = unsafe { info.as_ref() } {
        if i.fccChildIOProc != 0 {
            unsafe { *fcc = i.fccChildIOProc };
            return 0;
        }
        if i.ulFlags & 0x8000 != 0 {
            sep = b'|';
        }
    }
    if name.is_null() {
        return u32::MAX;
    }
    let mut n = 0;
    while unsafe { *name.add(n) } != 0 && n < 260 {
        n += 1;
    }
    let s = unsafe { core::slice::from_raw_parts(name, n) };
    let Some(at) = s.iter().position(|&b| b == sep) else { return u32::MAX };
    let file = &s[..at];
    let Some(dot) = file.iter().rposition(|&b| b == b'.') else { return u32::MAX };
    let ext = &file[dot + 1..];
    if ext.is_empty() || ext.len() > 4 {
        return u32::MAX;
    }
    let mut f = [b' '; 4];
    for (i, &c) in ext.iter().enumerate() {
        f[i] = c.to_ascii_uppercase();
    }
    unsafe { *fcc = u32::from_le_bytes(f) };
    if !element.is_null() {
        let rest = &s[at + 1..];
        unsafe {
            core::ptr::copy_nonoverlapping(rest.as_ptr(), element, rest.len());
            *element.add(rest.len()) = 0;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioDetermineFFIOProc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioDetermineLastIOProc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpIsFccSS() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpIsFccFF() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioQueryIOProcModuleHandle() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpGetElementStart() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpChangeSharingEntry() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpLoadNextIOProc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpLoadIOProc() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioMigrateIniFile() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioIniFileCODEC() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioCFCompact() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioQueryCODECName() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmiohlpCFName() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioLoadCODECProc() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioQueryCODECNameLength() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioGetData() -> u32 {
    err::UNSUPPORTED_FUNCTION
}
