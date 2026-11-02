//! Window, menu item, control and message numbers, as IBM's PMREXXIO
//! uses them: programs driving the I/O window (PMREXX) send these.

pub const RXHOST_WIN: u16 = 0x1100;
pub const RXIOM_FILE: u16 = 0x201;
pub const RXIOA_SAVE: u16 = 0x202;
pub const RXIOA_SAVEAS: u16 = 0x203;
pub const RXIOA_EXIT: u16 = 0x204;
pub const RXIOA_ABOUT: u16 = 0x205;
pub const RXIOA_RESTART: u16 = 0x206;
pub const RXIOM_EDIT: u16 = 0x106;
pub const RXIOA_COPY: u16 = 0x107;
pub const RXIOA_PASTE: u16 = 0x108;
pub const RXIOA_DELETE: u16 = 0x109;
pub const RXIOA_ALL: u16 = 0x10a;
pub const RXIOA_TRACE: u16 = 0x10d;
pub const RXIOA_FONTS: u16 = 0x10e;
pub const RXIOM_ACTIONS: u16 = 0x10f;
pub const RXIOA_HALT: u16 = 0x110;
pub const RXIOA_TRCSTEP: u16 = 0x111;
pub const RXIOA_TRCLAST: u16 = 0x112;
pub const RXIOA_TRCOFF: u16 = 0x113;
pub const RXIOM_HELP: u16 = 0x114;
pub const RXIOM_HELPHELP: u16 = 0x115;
pub const RXIOM_EXTHELP: u16 = 0x116;
pub const RXIOM_KEYSHELP: u16 = 0x117;
pub const RXIOM_INDEXHELP: u16 = 0x118;
pub const RXIOA_CUT: u16 = 0x124;
pub const RXIOA_DALL: u16 = 0x125;
pub const RXIOA_SEARCH: u16 = 0x126;
pub const RXIOA_PROPERTIES: u16 = 0x127;
pub const RXIOA_RESULTS: u16 = 0x128;

/// The output MLE and the input entry field.
pub const MLE_WNDW: u16 = 0x127;
pub const ID_EF1: u16 = 0x12a;

/// The Find dialog.
pub const PMREXX_SEARCH: u16 = 130;
pub const RX_SEARCH_TEXT: u16 = 131;
pub const RX_SEARCH_FIELD: u16 = 132;
pub const RX_CASEBOX: u16 = 133;
pub const PB_OK: u16 = 134;
pub const PB_CANCEL: u16 = 135;
pub const PB_HELP: u16 = 136;

/// Messages to the client (WM_USER + n).
pub const REXX_STARTPROC: u32 = 0x1001;
pub const REXX_ENDPROC: u32 = 0x1002;
pub const REXX_MAKE_VISIBLE: u32 = 0x1003;
pub const REXX_MAKE_INVISIBLE: u32 = 0x1004;
/// WM_COMMAND ids the I/O window sends its client (which a subclassing
/// program handles): the trace flag and the results setting changed.
pub const PMRXA_FLAGCHANGE: u32 = 0x1005;
pub const PMRXA_RESULTCHANGE: u32 = 0x1006;
/// Internal: output waiting (posted to the frame), a message box, a line
/// of output to add (mp1 a heap string).
pub const START_TIMER: u32 = 0x13e4;
pub const RXIOB_MSGBOX: u32 = 0x13e5;
pub const ADD_STDOUTWIN: u32 = 0x13e7;

/// The keys help panel (HM_QUERY_KEYS_HELP).
pub const RX_KEYSHELP: u32 = 9001;
