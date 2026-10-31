//! System colours (SYSCLR_*) and colour index resolution.

use gpi::color::{self, Rgb};

pub const SYSCLR_WINDOW: i32 = -20;
pub const SYSCLR_WINDOWTEXT: i32 = -17;
pub const SYSCLR_BACKGROUND: i32 = -24;
pub const SYSCLR_ACTIVETITLE: i32 = -23;
pub const SYSCLR_INACTIVETITLE: i32 = -22;
pub const SYSCLR_ACTIVETITLETEXT: i32 = -28;
pub const SYSCLR_INACTIVETITLETEXT: i32 = -29;
pub const SYSCLR_WINDOWFRAME: i32 = -19;
pub const SYSCLR_ACTIVEBORDER: i32 = -15;
pub const SYSCLR_INACTIVEBORDER: i32 = -14;
pub const SYSCLR_DIALOGBACKGROUND: i32 = -34;
pub const SYSCLR_BUTTONLIGHT: i32 = -41;
pub const SYSCLR_BUTTONMIDDLE: i32 = -40;
pub const SYSCLR_BUTTONDARK: i32 = -39;
pub const SYSCLR_HELPBACKGROUND: i32 = -12;
pub const SYSCLR_HELPTEXT: i32 = -11;
pub const SYSCLR_HELPHILITE: i32 = -10;
pub const SYSCLR_HILITEBACKGROUND: i32 = -32;
pub const SYSCLR_HILITEFOREGROUND: i32 = -33;

/// RGB of a system colour (as set with WinSetSysColors, or the default).
pub fn sys(i: i32) -> Rgb {
    if let Some(&c) = crate::state::st().syscolors.get(&i) {
        return c;
    }
    default(i)
}

/// A system colour as rs2 starts (Warp 4's).
pub fn default(i: i32) -> Rgb {
    match i {
        SYSCLR_WINDOW => 0xffffff,
        SYSCLR_WINDOWTEXT => 0x000000,
        SYSCLR_BACKGROUND => 0x008080,
        SYSCLR_ACTIVETITLE => 0x000080,
        SYSCLR_INACTIVETITLE => 0x808080,
        SYSCLR_ACTIVETITLETEXT => 0xffffff,
        SYSCLR_INACTIVETITLETEXT => 0xcccccc,
        SYSCLR_WINDOWFRAME => 0x000000,
        SYSCLR_ACTIVEBORDER | SYSCLR_INACTIVEBORDER | SYSCLR_DIALOGBACKGROUND | SYSCLR_BUTTONMIDDLE => 0xcccccc,
        SYSCLR_BUTTONLIGHT => 0xffffff,
        SYSCLR_BUTTONDARK => 0x808080,
        SYSCLR_HELPBACKGROUND => 0xffffff,
        SYSCLR_HELPTEXT => 0x000000,
        SYSCLR_HELPHILITE => 0x008080,
        SYSCLR_HILITEBACKGROUND => 0x000080,
        SYSCLR_HILITEFOREGROUND => 0xffffff,
        // The rest as Warp 4's default scheme (OS2\VGA.RC, PM_Colors).
        -27 | -18 | -35 | -38 => 0x000000, // OUTPUTTEXT, MENUTEXT, ICONTEXT, BUTTONDEFAULT
        -26 => 0x000080,                    // WINDOWSTATICTEXT
        -46 => 0xa0a0a0,                    // MENUDISABLEDTEXT
        -36 | -37 | -48 | -50 => 0x808080,  // SHADOW, TITLEBOTTOM, SHADOWTEXT, SHADOWHILITEBGND
        -49 => 0xffff00,                    // SHADOWHILITEFGND
        -16 | -43 | -47 => 0xffffff,        // TITLETEXT, PAGEBACKGROUND, ENTRYFIELD
        -25 => 0xc0c0c0,                    // SCROLLBAR
        -30 => default(SYSCLR_ACTIVETITLE), // ACTIVETITLETEXTBGND
        -31 => default(SYSCLR_INACTIVETITLE),
        _ => 0xcccccc,
    }
}

/// RGB for a colour index as PM programs pass it: SYSCLR_* (-50..-4),
/// CLR_* (-3..15).
pub fn resolve(i: i32) -> Rgb {
    if (-50..=-6).contains(&i) { sys(i) } else { color::index_to_rgb(i) }
}
