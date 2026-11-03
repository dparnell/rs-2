//! OSAScriptingComponent (ordinals 527-529): the base class of OpenDoc's
//! scripting components (IBM's REXXScriptingComponent in OREXXSC.DLL).
//!
//! The method names are the ones REXXScriptingComponent overrides
//! (`OSAScriptingComponent::OSACompile`...), at the indices its calls to
//! its parent fix (docs/OPENDOC.md); the others are Apple's OSA call
//! names in the same alphabetical order (assumed). The base class has no
//! language: its calls fail with errOSABadSelector, except the composite
//! calls, which a subclass gets made from its own: OSADoScript
//! (compile, execute, display), OSACompileExecute and OSALoadExecute.

use crate::rt::*;
use crate::*;

pub const ERR_BAD_SELECTOR: i32 = -1754;
pub const ERR_SYSTEM: i32 = -1750;

mod ix {
    pub const COMPILE: usize = 7;
    pub const DISPLAY: usize = 10;
    pub const DISPOSE: usize = 11;
    pub const EXECUTE: usize = 14;
    pub const LOAD: usize = 24;
}

fn osa(i: usize, args: &[usize]) -> i32 {
    call(cd!(OSAScriptingComponentClassData), i, args) as i32
}

methods! {
    fn BadSelector0(this: usize, ev: usize) -> usize { ERR_BAD_SELECTOR as usize }
    fn BadSelector1(this: usize, ev: usize, a: usize) -> usize { ERR_BAD_SELECTOR as usize }
    fn BadSelector2(this: usize, ev: usize, a: usize, b: usize) -> usize { ERR_BAD_SELECTOR as usize }
    fn BadSelector3(this: usize, ev: usize, a: usize, b: usize, c: usize) -> usize { ERR_BAD_SELECTOR as usize }
    fn BadSelector4(this: usize, ev: usize, a: usize, b: usize, c: usize, d: usize) -> usize { ERR_BAD_SELECTOR as usize }
    fn BadSelector5(this: usize, ev: usize, a: usize, b: usize, c: usize, d: usize, e: usize) -> usize { ERR_BAD_SELECTOR as usize }
    // OSADoScript(source, context, desiredType, modeFlags, *text)
    fn DoScript(this: usize, ev: usize, source: usize, context: usize, ty: usize, mode: usize, text: usize) -> usize {
        let mut id = 0usize;
        let mut r = osa(ix::COMPILE, &[this, ev, source, mode, &mut id as *mut usize as usize]);
        if r == 0 {
            let mut result = 0usize;
            r = osa(ix::EXECUTE, &[this, ev, id, context, mode, &mut result as *mut usize as usize]);
            if r == 0 {
                r = osa(ix::DISPLAY, &[this, ev, result, ty, mode, text]);
                osa(ix::DISPOSE, &[this, ev, result]);
            }
            osa(ix::DISPOSE, &[this, ev, id]);
        }
        r as usize
    }
    // OSACompileExecute(source, context, modeFlags, *resultID)
    fn CompileExecute(this: usize, ev: usize, source: usize, context: usize, mode: usize, result: usize) -> usize {
        let mut id = 0usize;
        let mut r = osa(ix::COMPILE, &[this, ev, source, mode, &mut id as *mut usize as usize]);
        if r == 0 {
            r = osa(ix::EXECUTE, &[this, ev, id, context, mode, result]);
            osa(ix::DISPOSE, &[this, ev, id]);
        }
        r as usize
    }
    // OSALoadExecute(scriptData, context, modeFlags, *resultID)
    fn LoadExecute(this: usize, ev: usize, data: usize, context: usize, mode: usize, result: usize) -> usize {
        let mut id = 0usize;
        let mut r = osa(ix::LOAD, &[this, ev, data, mode, &mut id as *mut usize as usize]);
        if r == 0 {
            r = osa(ix::EXECUTE, &[this, ev, id, context, mode, result]);
            osa(ix::DISPOSE, &[this, ev, id]);
        }
        r as usize
    }
}

pub static OSA_M: &[(&str, M)] = &[
    ("OSAAddStorageType", BadSelector2::t),
    ("OSAAvailableDialectCodeList", BadSelector1::t),
    ("OSAAvailableDialects", BadSelector1::t),
    ("OSACoerceFromDesc", BadSelector4::t),
    ("OSACoerceToDesc", BadSelector5::t),
    ("OSAScriptingComponent_m5", BadSelector0::t),
    ("OSAScriptingComponent_m6", BadSelector0::t),
    ("OSACompile", BadSelector3::t),
    ("OSACompileExecute", CompileExecute::t),
    ("OSACopyID", BadSelector2::t),
    ("OSADisplay", BadSelector4::t),
    ("OSADispose", BadSelector1::t),
    ("OSADoEvent", BadSelector4::t),
    ("OSADoScript", DoScript::t),
    ("OSAExecute", BadSelector4::t),
    ("OSAExecuteEvent", BadSelector4::t),
    ("OSAGenericToRealID", BadSelector2::t),
    ("OSAGetActiveProc", BadSelector2::t),
    ("OSAGetCreateProc", BadSelector2::t),
    ("OSAGetCurrentDialect", BadSelector1::t),
    ("OSAGetDialectInfo", BadSelector3::t),
    ("OSAGetScriptInfo", BadSelector3::t),
    ("OSAGetSendProc", BadSelector2::t),
    ("OSAGetSource", BadSelector3::t),
    ("OSALoad", BadSelector3::t),
    ("OSALoadExecute", LoadExecute::t),
    ("OSAMakeContext", BadSelector3::t),
    ("OSAScriptError", BadSelector3::t),
    ("OSAScriptingComponentName", BadSelector1::t),
    ("OSASetActiveProc", BadSelector2::t),
    ("OSASetCreateProc", BadSelector2::t),
    ("OSASetCurrentDialect", BadSelector1::t),
    ("OSASetDefaultTarget", BadSelector1::t),
    ("OSASetResumeDispatchProc", BadSelector2::t),
    ("OSASetScriptInfo", BadSelector3::t),
    ("OSASetSendProc", BadSelector2::t),
    ("OSAStartRecording", BadSelector1::t),
    ("OSAStopRecording", BadSelector1::t),
    ("OSAStore", BadSelector4::t),
];
