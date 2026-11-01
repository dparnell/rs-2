//! SOMDesc, SOMArgList and SOMCommand: method descriptions from the
//! interface repository, argument lists built from them, and method calls
//! written out as text (IBM's DSOM and persistence frameworks use them;
//! SOMMTraced prints arguments with them). Not done yet: their methods
//! fail (-1), so a trace shows ", ..." for the arguments, and the
//! externalization functions report failure.

use somsup::{SomId, methods};

/// A method that fails (-1), whatever its arguments.
#[unsafe(naked)]
pub extern "C" fn fail() {
    core::arch::naked_asm!("mov eax, -1", "ret");
}

pub static DESC_METHODS: &[(&str, usize)] = &[
    ("somutSetSomDesc", 0),
    ("somutSetSomDesc2", 0),
    ("somutGetNumArgs", 0),
    ("somutGetRetSize", 0),
    ("somutGetRetInfo", 0),
    ("somutGetArgSize", 0),
    ("somutGetArgInfo", 0),
    ("somutGetContextIds", 0),
    ("somutGetCallStyle", 0),
    ("somutIsOnewayAck", 0),
    ("somutFlushCache", 0),
    ("somutFlushEntry", 0),
    ("somutGetOperationModifier", 0),
    ("_get_somutReturnResultOwner", 0),
    ("somutGetArgOwner", 0),
    ("somutGetArgIndex", 0),
];

pub static ARGLIST_METHODS: &[(&str, usize)] = &[
    ("somutSetArgDesc", 0),
    ("somutGetArgDesc", 0),
    ("somutAddInteger1Arg", 0),
    ("somutAddInteger2Arg", 0),
    ("somutAddUInteger2Arg", 0),
    ("somutAddInteger4Arg", 0),
    ("somutAddUInteger4Arg", 0),
    ("somutAddFloat4Arg", 0),
    ("somutAddFloat8Arg", 0),
    ("somutAddStringArg", 0),
    ("somutAddFStringArg", 0),
    ("somutAddSomTokenArg", 0),
    ("somutAddBooleanArg", 0),
    ("somutGetInteger1Arg", 0),
    ("somutGetInteger2Arg", 0),
    ("somutGetUInteger2Arg", 0),
    ("somutGetInteger4Arg", 0),
    ("somutGetUInteger4Arg", 0),
    ("somutGetFloat4Arg", 0),
    ("somutGetFloat8Arg", 0),
    ("somutGetStringArg", 0),
    ("somutGetFStringArg", 0),
    ("somutGetSomTokenArg", 0),
    ("somutGetBooleanArg", 0),
    ("somutGetNumArgsList", 0),
    ("somutGetArgList", 0),
    ("somutSetArgList", 0),
    ("somutGetArgs", 0),
    ("somutSetArgs", 0),
    ("somutAddSomEnvArg", 0),
];

pub static COMMAND_METHODS: &[(&str, usize)] = &[
    ("somutSetCommand", 0),
    ("somutSetClassCommand", 0),
    ("somutGetExtRep", 0),
    ("somutSetExtRep", 0),
    ("somutDoCommand", 0),
];

extern "C" fn nothing(_o: usize) {}

methods! {
    __m_desc_init => nothing, 1;
    __m_desc_uninit => nothing, 1;
    __m_args_init => nothing, 1;
    __m_args_uninit => nothing, 1;
    __m_cmd_init => nothing, 1;
    __m_cmd_uninit => nothing, 1;
}

/// The varargs entries (somva_SOMArgList_somutSetArgs, ...): fail.
#[unsafe(naked)]
pub extern "C" fn __v_set_args() {
    core::arch::naked_asm!("mov eax, -1", "ret");
}
#[unsafe(naked)]
pub extern "C" fn __v_set_command() {
    core::arch::naked_asm!("mov eax, -1", "ret");
}

/// SOMMTraced: print a call's arguments (FALSE: they cannot be
/// described).
pub fn trace_args(_obj: usize, _mid: SomId, _ap: usize) -> bool {
    false
}

pub fn trace_result(_obj: usize, _mid: SomId, _ret: usize) -> bool {
    false
}

// ---- The externalization functions (_Optlink; somExternalizeCommandInit varargs) ----

pub extern "C" fn somutInternalizeObject() -> i32 {
    -1
}
pub extern "C" fn somutExternalizeObject() -> i32 {
    -1
}
pub extern "C" fn somutIntCommand() -> i32 {
    -1
}
pub extern "C" fn somutExtCommand() -> i32 {
    -1
}

somsup::optlink! {
    __o_somutInternalizeObject => somutInternalizeObject, 0;
    __o_somutExternalizeObject => somutExternalizeObject, 0;
    __o_somutIntCommand => somutIntCommand, 0;
    __o_somutExtCommand => somutExtCommand, 0;
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somExternalizeCommandInit() {
    core::arch::naked_asm!("mov eax, -1", "ret");
}
