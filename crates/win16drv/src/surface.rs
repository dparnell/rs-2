//! The display surface a DOS session's VDM.EXE shares with its PM host
//! (PMVIOWIN): RS2DISP.DRV's screen and what goes with it, in named
//! shared memory `\SHAREMEM\RS2VDM\<pid in hex>.DSP` (the VDM's process
//! id). The VDM writes, the host reads; neither waits for the other.
//! The kernel wakes the host (RS2API 406 `Rs2VdmSurface`).
//!
//! | Offset | What |
//! |--------|------|
//! | 0000h | magic `RS2D`, width, height (ULONGs) |
//! | 000Ch | damage sequence: rectangles written so far |
//! | 0010h | palette generation, pointer generation |
//! | 0018h | pointer: x, y (its hot spot on the surface), hot x, hot y, width, height, bytes per mask row, shown |
//! | 0038h | window list generation, windows |
//! | 0040h | palette: 256 x 0x00RRGGBB |
//! | 0440h | pointer AND mask, then (0640h) XOR mask: 512 bytes each, rows top down |
//! | 0840h | damage ring: 64 x (x0, y0, x1, y1) LONGs, entry `seq % 64` written before seq is bumped |
//! | 0C40h | flags: bit 0 a seamless session (the host shows its windows, not its screen) |
//! | 1000h | seamless windows: 128 x 128 bytes, bottom of the z-order first (`WIN_*`) |
//! | 5000h | the host's command line for `REQ_RUN` (256 bytes, written by the host) |
//! | 10000h | pixels: width x height bytes, top row first |
//!
//! A window entry: hwnd (USHORT), flags (USHORT, `WF_*`), x, y, width,
//! height (LONGs, the Windows screen's coordinates), style (ULONG),
//! owner (USHORT), 2 bytes reserved, the title (100 bytes, NUL-terminated).
//!
//! The host's requests reach the VDM as input events (RS2API 407
//! `Rs2VdmRequest(sid, code, a, b)`, `REQ_*`).
//!
//! A host that finds the sequence more than 64 ahead of what it saw
//! redraws everything.

pub const MAGIC: u32 = 0x4432_5352;
pub const W: usize = 0x04;
pub const H: usize = 0x08;
pub const SEQ: usize = 0x0c;
pub const PAL_GEN: usize = 0x10;
pub const PTR_GEN: usize = 0x14;
pub const PTR_X: usize = 0x18;
pub const PTR_Y: usize = 0x1c;
pub const PTR_HOTX: usize = 0x20;
pub const PTR_HOTY: usize = 0x24;
pub const PTR_W: usize = 0x28;
pub const PTR_H: usize = 0x2c;
pub const PTR_WB: usize = 0x30;
pub const PTR_SHOWN: usize = 0x34;
pub const WIN_GEN: usize = 0x38;
pub const WIN_COUNT: usize = 0x3c;
pub const PALETTE: usize = 0x40;
pub const PTR_AND: usize = 0x440;
pub const PTR_XOR: usize = 0x640;
pub const PTR_MASK: usize = 0x200;
pub const RING: usize = 0x840;
pub const RING_LEN: u32 = 64;
pub const WINDOWS: usize = 0x1000;
pub const WINDOW_SIZE: usize = 128;
pub const MAX_WINDOWS: usize = 128;
pub const FLAGS: usize = 0x0c40;
pub const FL_SEAMLESS: u32 = 1;
pub const HOST_CMD: usize = 0x5000;
pub const HOST_CMD_LEN: usize = 256;
pub const PIXELS: usize = 0x10000;

/// Window entry fields.
pub const WIN_HWND: usize = 0;
pub const WIN_FLAGS: usize = 2;
pub const WIN_X: usize = 4;
pub const WIN_Y: usize = 8;
pub const WIN_W: usize = 12;
pub const WIN_H: usize = 16;
pub const WIN_STYLE: usize = 20;
pub const WIN_OWNER: usize = 24;
pub const WIN_TITLE: usize = 28;
pub const WIN_TITLE_LEN: usize = 100;
pub const WF_VISIBLE: u16 = 0x01;
pub const WF_MINIMIZED: u16 = 0x02;
pub const WF_MAXIMIZED: u16 = 0x04;
pub const WF_ACTIVE: u16 = 0x08;

/// Requests (`Rs2VdmRequest` codes): a = hwnd, except as noted.
pub const REQ_ACTIVATE: u32 = 1;
pub const REQ_CLOSE: u32 = 2;
/// Place, in two: the position (b = x | y << 16), then the size (b = w | h << 16).
pub const REQ_PLACE_POS: u32 = 3;
pub const REQ_PLACE_SIZE: u32 = 4;
pub const REQ_MINIMIZE: u32 = 5;
pub const REQ_RESTORE: u32 = 6;
pub const REQ_MAXIMIZE: u32 = 7;
/// Start the program at HOST_CMD (a: its length), or (a = -1) the one
/// REQ_RUN_TEXT gave.
pub const REQ_RUN: u32 = 8;
/// 8 more bytes of a command line in a and b (the kernel's way: it
/// cannot reach the surface).
pub const REQ_RUN_TEXT: u32 = 9;

/// The shared memory's size for a w x h surface.
pub const fn size(w: usize, h: usize) -> usize {
    PIXELS + w * h
}

/// The name of process `pid`'s surface, NUL-terminated.
pub fn name(pid: u32) -> [u8; 32] {
    let mut n = [0u8; 32];
    let pre = b"\\SHAREMEM\\RS2VDM\\";
    n[..pre.len()].copy_from_slice(pre);
    let mut i = pre.len();
    for k in (0..8).rev() {
        n[i] = b"0123456789ABCDEF"[(pid >> (k * 4)) as usize & 15];
        i += 1;
    }
    n[i..i + 4].copy_from_slice(b".DSP");
    n
}

/// Access to a mapped surface.
#[derive(Clone, Copy)]
pub struct Surface(pub *mut u8);

impl Surface {
    /// # Safety
    /// The surface must be mapped and `o` inside it.
    pub unsafe fn get(&self, o: usize) -> u32 {
        unsafe { (self.0.add(o) as *const u32).read_volatile() }
    }
    /// # Safety
    /// As for `get`.
    pub unsafe fn set(&self, o: usize, v: u32) {
        unsafe { (self.0.add(o) as *mut u32).write_volatile(v) }
    }
    /// # Safety
    /// As for `get`, `n` bytes.
    pub unsafe fn bytes(&self, o: usize, n: usize) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.0.add(o), n) }
    }
}
