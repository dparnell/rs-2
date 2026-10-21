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
pub extern "C" fn mmioIniFileHandler() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioIdentifyFileFormat() -> u32 {
    err::UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mmioDetermineSSIOProc() -> u32 {
    err::UNSUPPORTED_FUNCTION
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
