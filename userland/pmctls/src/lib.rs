//! PMCTLS.DLL: the standard file and font dialogs.
//!
//! The dialogs are not implemented yet: they return at once as if the
//! user had pressed Cancel, which applications handle as usual.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

mod exports;

const DID_CANCEL: i32 = 2;

unsafe extern "C" {
    #[link_name = "PMWIN.910"]
    fn PmWinDefDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// WinFileDlg(hwndParent, hwndOwner, pfild): FILEDLG has cbSize, fl,
/// ulUser, lReturn.
#[unsafe(no_mangle)]
pub extern "C" fn WinFileDlg(_parent: u32, _owner: u32, fild: *mut i32) -> u32 {
    if fild.is_null() {
        return 0;
    }
    unsafe { fild.add(3).write_unaligned(DID_CANCEL) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDefFileDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { PmWinDefDlgProc(hwnd, msg, mp1, mp2) }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinFreeFileDlgList(_list: u32) -> u32 {
    1
}

/// WinFontDlg(hwndParent, hwndOwner, pfntd): FONTDLG has cbSize, hpsScreen,
/// hpsPrinter, pszTitle, pszPreview, pszPtSizeList, pfnDlgProc,
/// pszFamilyname, fxPointSize, fl, flFlags, flType, flTypeMask,
/// flStyle, flStyleMask, clrFore, clrBack, ulUser, lReturn.
#[unsafe(no_mangle)]
pub extern "C" fn WinFontDlg(_parent: u32, _owner: u32, fntd: *mut i32) -> u32 {
    if fntd.is_null() {
        return 0;
    }
    unsafe { fntd.add(18).write_unaligned(DID_CANCEL) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDefFontDlgProc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { PmWinDefDlgProc(hwnd, msg, mp1, mp2) }
}
