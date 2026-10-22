//! Stack-aligning entry points (see sys.rs's `aligned!`): the exported
//! functions (`__a_`), the method implementations put in method tables
//! (`__m_`), the thunks' slow paths (`__s_`), and the varargs functions
//! (`__v_`), which pass a pointer to their variable arguments on.

use crate::aligned;
use crate::class as c;
use crate::id;
use crate::mgr as m;
use crate::object as o;
use crate::rt;
use crate::sys;

aligned! {
    // Exported functions.
    __a_SOMClassMgrNewClass => crate::SOMClassMgrNewClass, 2;
    __a_SOMClassNewClass => crate::SOMClassNewClass, 2;
    __a_SOMObjectNewClass => crate::SOMObjectNewClass, 2;
    __a_somAssert => crate::somAssert, 5;
    __a_somBeginPersistentIds => id::somBeginPersistentIds, 0;
    __a_somCheckArgs => crate::somCheckArgs, 0;
    __a_somCheckId => id::somCheckId, 1;
    __a_somCompareIds => id::somCompareIds, 2;
    __a_somConstructClass => crate::somConstructClass, 4;
    __a_somEndPersistentIds => id::somEndPersistentIds, 0;
    __a_somEnvironmentNew => crate::somEnvironmentNew, 0;
    __a_somIdFromString => id::somIdFromString, 1;
    __a_somParentResolve => crate::somParentResolve, 2;
    __a_somPrefixLevel => crate::somPrefixLevel, 1;
    __a_somRegisterId => id::somRegisterId, 1;
    __a_somResolve => crate::somResolve, 2;
    __a_somSaveMetrics => crate::somSaveMetrics, 0;
    __a_somSetExpectedIds => id::somSetExpectedIds, 1;
    __a_somStringFromId => id::somStringFromId, 1;
    __a_somTest => crate::somTest, 5;
    __a_somTestCls => crate::somTestCls, 4;
    __a_somTotalRegIds => id::somTotalRegIds, 0;
    __a_somUniqueKey => id::somUniqueKey, 1;
    __a_somVprintf => crate::somVprintf, 2;
    __a_somWriteMetrics => crate::somWriteMetrics, 2;
    __a_somDataResolve => crate::somDataResolve, 2;
    __a_somClassResolve => crate::somClassResolve, 2;
    __a_somBuildClass => crate::somBuildClass, 4;
    __a_somParentNumResolve => crate::somParentNumResolve, 3;
    __a_somCreateDynamicClass => crate::somCreateDynamicClass, 3;
    __a_somExceptionId => crate::somExceptionId, 1;
    __a_somExceptionValue => crate::somExceptionValue, 1;
    __a_somExceptionFree => crate::somExceptionFree, 1;
    __a_somSetException => crate::somSetException, 4;
    __a_somGetGlobalEnvironment => crate::somGetGlobalEnvironment, 0;
    __a_somIsObj => crate::somIsObj, 1;
    __a_somResolveByName => crate::somResolveByName, 2;
    __a_somStartCriticalSection => crate::somStartCriticalSection, 0;
    __a_somEndCriticalSection => crate::somEndCriticalSection, 0;
    __a_somApply => crate::somApply, 4;
    __a_somDataResolveChk => crate::somDataResolveChk, 2;
    __a_somAncestorResolve => crate::somAncestorResolve, 3;
    __a_somfixMsgTemplate => crate::somfixMsgTemplate, 2;
    __a_somGetClassFromMToken => crate::somGetClassFromMToken, 1;
    __a_somEnvironmentEnd => crate::somEnvironmentEnd, 0;
    __a_somAbnormalEnd => crate::somAbnormalEnd, 0;
    __a_somSetOutChar => crate::somSetOutChar, 1;
    __a_somRegisterClassLibrary => crate::somRegisterClassLibrary, 2;
    __a_somMainProgram => crate::somMainProgram, 0;
    __a_somUnregisterClassLibrary => crate::somUnregisterClassLibrary, 1;
    __a_somFreeThreadData => crate::somFreeThreadData, 0;
    __a_somResolveTerminal => crate::somResolveTerminal, 2;
    __a_somIdMarshal => crate::somIdMarshal, 2;
    __a_somMakeUserRdStub => crate::somMakeUserRdStub, 1;
    __a_somPCallResolve => crate::somPCallResolve, 3;

    // The replaceable procedures' defaults.
    __a_som_malloc => sys::som_malloc, 1;
    __a_som_calloc => sys::som_calloc, 2;
    __a_som_realloc => sys::som_realloc, 2;
    __a_som_free => sys::som_free, 1;
    __a_som_error => crate::som_error, 3;
    __a_som_out_char => crate::som_out_char, 1;
    __a_load_module => m::load_module, 6;
    __a_delete_module => m::delete_module, 1;
    __a_init_func_name => m::init_func_name, 0;
    __a_create_mutex => sys::create_mutex, 1;
    __a_request_mutex => sys::request_mutex, 1;
    __a_release_mutex => sys::release_mutex, 1;
    __a_destroy_mutex => sys::destroy_mutex, 1;
    __a_thread_id => sys::thread_id, 0;
    __a_no_method => crate::no_method, 0;

    // The thunks' slow paths.
    __s_resolve => rt::slow_resolve, 2;
    __s_data_resolve => rt::slow_data_resolve, 2;

    // SOMObject.
    __m_somInit => o::somInit, 1;
    __m_somUninit => o::somUninit, 1;
    __m_somFree => o::somFree, 1;
    __m_somDefaultVCopyInit => o::somDefaultVCopyInit, 3;
    __m_somGetClassName => o::somGetClassName, 1;
    __m_somGetClass => o::somGetClass, 1;
    __m_somIsA => o::somIsA, 2;
    __m_somRespondsTo => o::somRespondsTo, 2;
    __m_somIsInstanceOf => o::somIsInstanceOf, 2;
    __m_somGetSize => o::somGetSize, 1;
    __m_somDumpSelf => o::somDumpSelf, 2;
    __m_somDumpSelfInt => o::somDumpSelfInt, 2;
    __m_somPrintSelf => o::somPrintSelf, 1;
    __m_somDefaultConstVCopyInit => o::somDefaultConstVCopyInit, 3;
    __m_somDispatchV => o::somDispatchV, 4;
    __m_somDispatchL => o::somDispatchL, 4;
    __m_somDispatchA => o::somDispatchA, 4;
    __m_somDispatchD => o::somDispatchD, 4;
    __m_somDispatch => o::somDispatch, 4;
    __m_somClassDispatch => o::somClassDispatch, 5;
    __m_somCastObj => o::somCastObj, 2;
    __m_somResetObj => o::somResetObj, 1;
    __m_somDefaultInit => o::somDefaultInit, 2;
    __m_somDestruct => o::somDestruct, 3;
    __m_somPrivate => o::somPrivate, 1;
    __m_somDefaultCopyInit => o::somDefaultCopyInit, 3;
    __m_somDefaultConstCopyInit => o::somDefaultConstCopyInit, 3;
    __m_somDefaultAssign => o::somDefaultAssign, 3;
    __m_somDefaultConstAssign => o::somDefaultConstAssign, 3;
    __m_somDefaultVAssign => o::somDefaultVAssign, 3;
    __m_somDefaultConstVAssign => o::somDefaultConstVAssign, 3;

    // SOMClass.
    __m_somNew => c::somNew, 1;
    __m_somRenew => c::somRenew, 2;
    __m_somInitClass => c::somInitClass, 7;
    __m_somClassReady => c::somClassReady, 1;
    __m_somGetName => c::somGetName, 1;
    __m_somGetParent => c::somGetParent, 1;
    __m_somDescendedFrom => c::somDescendedFrom, 2;
    __m_somCheckVersion => c::somCheckVersion, 3;
    __m_somFindMethod => c::somFindMethod, 3;
    __m_somFindMethodOk => c::somFindMethodOk, 3;
    __m_somSupportsMethod => c::somSupportsMethod, 2;
    __m_somGetNumMethods => c::somGetNumMethods, 1;
    __m_somGetInstanceSize => c::somGetInstanceSize, 1;
    __m_somGetInstanceOffset => c::somGetInstanceOffset, 1;
    __m_somGetInstancePartSize => c::somGetInstancePartSize, 1;
    __m_somGetMethodIndex => c::somGetMethodIndex, 2;
    __m_somGetNumStaticMethods => c::somGetNumStaticMethods, 1;
    __m_somGetPClsMtab => c::somGetPClsMtab, 1;
    __m_somGetClassMtab => c::somGetClassMtab, 1;
    __m_somAddStaticMethod => c::somAddStaticMethod, 6;
    __m_somOverrideSMethod => c::somOverrideSMethod, 3;
    __m_somAddDynamicMethod => c::somAddDynamicMethod, 5;
    __m_class_private => c::private_stub, 1;
    __m_somGetApplyStub => c::somGetApplyStub, 2;
    __m_somFindSMethod => c::somFindSMethod, 2;
    __m_somFindSMethodOk => c::somFindSMethodOk, 2;
    __m_somGetMethodDescriptor => c::somGetMethodDescriptor, 2;
    __m_somGetNthMethodInfo => c::somGetNthMethodInfo, 3;
    __m_somSetClassData => c::somSetClassData, 2;
    __m_somGetClassData => c::somGetClassData, 1;
    __m_somNewNoInit => c::somNewNoInit, 1;
    __m_somRenewNoInit => c::somRenewNoInit, 2;
    __m_somGetInstanceToken => c::somGetInstanceToken, 1;
    __m_somGetMemberToken => c::somGetMemberToken, 3;
    __m_somGetMethodData => c::somGetMethodData, 3;
    __m_somOverrideMtab => c::somOverrideMtab, 1;
    __m_somGetMethodToken => c::somGetMethodToken, 2;
    __m_somGetParents => c::somGetParents, 2;
    __m_somGetPClsMtabs => c::somGetPClsMtabs, 1;
    __m_somInitMIClass => c::somInitMIClass, 9;
    __m_somGetVersionNumbers => c::somGetVersionNumbers, 3;
    __m_somLookupMethod => c::somLookupMethod, 2;
    __m_get_somInstanceDataOffsets => c::get_somInstanceDataOffsets, 2;
    __m_somRenewNoZero => c::somRenewNoZero, 2;
    __m_somRenewNoInitNoZero => c::somRenewNoInitNoZero, 2;
    __m_somAllocate => c::somAllocate, 2;
    __m_somDeallocate => c::somDeallocate, 2;
    __m_somGetRdStub => c::somGetRdStub, 2;
    __m_somGetNthMethodData => c::somGetNthMethodData, 3;
    __m_get_somDirectInitClasses => c::get_somDirectInitClasses, 2;
    __m_somDefinedMethod => c::somDefinedMethod, 2;
    __m_get_somDataAlignment => c::get_somDataAlignment, 1;
    __m_class_somFree => c::class_somFree, 1;
    __m_class_somDumpSelfInt => c::class_somDumpSelfInt, 2;

    // SOMClassMgr.
    __m_somFindClsInFile => m::somFindClsInFile, 5;
    __m_somFindClass => m::somFindClass, 4;
    __m_somClassFromId => m::somClassFromId, 2;
    __m_somRegisterClass => m::somRegisterClass, 2;
    __m_somUnregisterClass => m::somUnregisterClass, 2;
    __m_somLocateClassFile => m::somLocateClassFile, 4;
    __m_somLoadClassFile => m::somLoadClassFile, 5;
    __m_somUnloadClassFile => m::somUnloadClassFile, 2;
    __m_somGetInitFunction => m::somGetInitFunction, 1;
    __m_somMergeInto => m::somMergeInto, 2;
    __m_somGetRelatedClasses => m::somGetRelatedClasses, 2;
    __m_somSubstituteClass => m::somSubstituteClass, 3;
    __m_get_somInterfaceRepository => m::get_somInterfaceRepository, 1;
    __m_set_somInterfaceRepository => m::set_somInterfaceRepository, 2;
    __m_get_somRegisteredClasses => m::get_somRegisteredClasses, 2;
    __m_somBeginPersistentClasses => m::somBeginPersistentClasses, 1;
    __m_somEndPersistentClasses => m::somEndPersistentClasses, 1;
    __m_mgr_private => m::mgr_private, 1;
    __m_somRegisterClassLibrary => m::somRegisterClassLibrary, 3;
    __m_somJoinAffinityGroup => m::somJoinAffinityGroup, 3;
    __m_somUnregisterClassLibrary => m::somUnregisterClassLibrary, 2;
    __m_mgr_somDumpSelfInt => m::mgr_somDumpSelfInt, 2;
}

/// somPrintf(fmt, ...).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somPrintf() {
    core::arch::naked_asm!(
        "lea eax, [esp + 8]",
        "push eax",
        "push dword ptr [esp + 8]",
        "call {f}",
        "add esp, 8",
        "ret",
        f = sym __a_somVprintf,
    );
}

/// somLPrintf(level, fmt, ...).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somLPrintf() {
    core::arch::naked_asm!(
        "push dword ptr [esp + 4]",
        "call {p}",
        "add esp, 4",
        "lea eax, [esp + 12]",
        "push eax",
        "push dword ptr [esp + 12]",
        "call {f}",
        "add esp, 8",
        "ret",
        p = sym __a_somPrefixLevel,
        f = sym __a_somVprintf,
    );
}

/// somva_SOMObject_somDispatch(obj, retValue, methodId, ...): the
/// object's somDispatch with the variable arguments (which start with
/// the object) as its va_list.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somDispatch() {
    core::arch::naked_asm!(
        "lea eax, [esp + 16]",
        "push eax",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "push dword ptr [esp + 16]",
        "mov eax, dword ptr [{cd} + {slot}]",
        "call eax",
        "add esp, 16",
        "ret",
        cd = sym crate::SOMObjectClassData,
        slot = const 4 * (1 + 18),
    );
}

/// somva_SOMObject_somClassDispatch(obj, clsObj, retValue, methodId, ...).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somClassDispatch() {
    core::arch::naked_asm!(
        "lea eax, [esp + 20]",
        "push eax",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "mov eax, dword ptr [{cd} + {slot}]",
        "call eax",
        "add esp, 20",
        "ret",
        cd = sym crate::SOMObjectClassData,
        slot = const 4 * (1 + 19),
    );
}

macro_rules! dispatch_x {
    ($($name:ident => $idx:literal;)*) => {$(
        /// somva_SOMObject_somDispatchX(obj, methodId, descriptor, ...).
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!(
                "lea eax, [esp + 16]",
                "push eax",
                "push dword ptr [esp + 16]",
                "push dword ptr [esp + 16]",
                "push dword ptr [esp + 16]",
                "mov eax, dword ptr [{cd} + {slot}]",
                "call eax",
                "add esp, 16",
                "ret",
                cd = sym crate::SOMObjectClassData,
                slot = const 4 * (1 + $idx),
            );
        }
    )*};
}

dispatch_x! {
    __v_somDispatchV => 14;
    __v_somDispatchL => 15;
    __v_somDispatchA => 16;
    __v_somDispatchD => 17;
}
