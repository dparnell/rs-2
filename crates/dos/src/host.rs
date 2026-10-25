//! What the virtual machine needs from the system it runs on.
//!
//! Paths passed to the host are full DOS paths (`C:\DIR\FILE.EXT`, upper
//! case, backslashes); the machine resolves relative paths, current
//! directories and devices itself. Errors are DOS error codes (2 file
//! not found, 3 path not found, 5 access denied, ...), which OS/2 shares.

use alloc::string::String;
use alloc::vec::Vec;

/// DOS open modes (the low byte of AL for 3Dh; access in bits 0-2,
/// sharing in 4-6).
#[derive(Clone, Copy, Debug)]
pub struct OpenMode(pub u8);

impl OpenMode {
    pub fn access(self) -> u8 {
        self.0 & 7
    }
    pub fn sharing(self) -> u8 {
        (self.0 >> 4) & 7
    }
}

/// How to create: what to do when the file exists (6Ch semantics).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Create {
    /// Create or truncate (3Ch).
    Truncate,
    /// Fail if it exists (5Bh).
    New,
}

/// One directory entry.
#[derive(Clone, Debug, Default)]
pub struct DirEntry {
    pub name: String,
    pub attr: u8,
    pub size: u32,
    /// DOS packed date and time.
    pub date: u16,
    pub time: u16,
}

/// Local date and time.
#[derive(Clone, Copy, Debug, Default)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub hundredths: u8,
    /// 0 = Sunday.
    pub weekday: u8,
}

/// Input from the user.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// A set-1 scan code byte (make or break, E0 prefixes as bytes), and
    /// the character the host's keyboard layout gives the key (0 = none or
    /// let the BIOS translate).
    Key { scan: u8, ch: u8 },
    /// Mouse motion in mickeys and the buttons (bit 0 left, 1 right, 2
    /// middle).
    Mouse { dx: i32, dy: i32, buttons: u8 },
    /// The mouse at a position given as a fraction of the screen (0-65535
    /// in each direction), from a windowed host.
    MouseAbs { x: u32, y: u32, buttons: u8 },
    /// The user asked to end the session (closing its window).
    Close,
    /// The seamless host asks Windows to do something (crate::vwin).
    Window(crate::vwin::Request),
}

/// The services of the system around the machine.
pub trait Host {
    // ---- Files ----
    fn open(&mut self, path: &str, mode: OpenMode) -> Result<u32, u16>;
    fn create(&mut self, path: &str, attr: u8, how: Create) -> Result<u32, u16>;
    fn close(&mut self, h: u32);
    fn read(&mut self, h: u32, buf: &mut [u8]) -> Result<usize, u16>;
    fn write(&mut self, h: u32, buf: &[u8]) -> Result<usize, u16>;
    /// Move the file pointer (whence 0 start, 1 current, 2 end); the new
    /// position.
    fn seek(&mut self, h: u32, off: i64, whence: u8) -> Result<u64, u16>;
    /// Truncate or extend the file to `size` bytes.
    fn set_size(&mut self, h: u32, size: u64) -> Result<(), u16>;
    /// (date, time) of an open file.
    fn file_time(&mut self, h: u32) -> Result<(u16, u16), u16>;
    fn set_file_time(&mut self, h: u32, date: u16, time: u16) -> Result<(), u16>;
    fn delete(&mut self, path: &str) -> Result<(), u16>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), u16>;
    fn mkdir(&mut self, path: &str) -> Result<(), u16>;
    fn rmdir(&mut self, path: &str) -> Result<(), u16>;
    /// Attributes of a file or directory (error 2/3 if it does not exist).
    fn get_attr(&mut self, path: &str) -> Result<u8, u16>;
    fn set_attr(&mut self, path: &str, attr: u8) -> Result<(), u16>;
    /// Every entry of a directory (`C:\DIR`), with `.` and `..` for
    /// subdirectories.
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, u16>;
    /// Whether drive `d` (0 = A:) exists.
    fn drive_exists(&mut self, d: u8) -> bool;
    /// (sectors per cluster, bytes per sector, free clusters, total
    /// clusters).
    fn disk_space(&mut self, d: u8) -> Result<(u16, u16, u32, u32), u16>;
    fn volume_label(&mut self, d: u8) -> Option<String> {
        let _ = d;
        None
    }
    /// The current directory of drive `d` when the machine starts
    /// (without the drive, no leading backslash: `OS2\MDOS`).
    fn initial_dir(&mut self, d: u8) -> String {
        let _ = d;
        String::new()
    }

    // ---- Time ----
    fn now(&mut self) -> DateTime;
    /// A monotonic clock in microseconds.
    fn micros(&mut self) -> u64;

    // ---- The user ----
    /// Take pending input events.
    fn poll(&mut self, out: &mut Vec<Event>);
    /// Sleep up to `us` microseconds, less if input arrives.
    fn wait(&mut self, us: u64);
    /// Show the screen (called when it changed, at most every few ms).
    fn present(&mut self, v: &crate::video::Vga, ram: &[u8]);
    /// The size of the screen RS2DISP.DRV (crate::disp) draws, asked
    /// when Windows enables it.
    fn display_size(&mut self) -> (u32, u32) {
        (800, 600)
    }
    /// Show RS2DISP.DRV's screen: the rectangles that changed since the
    /// last call (all of it the first time), the pointer in `d.pointer`.
    fn present_display(&mut self, d: &crate::disp::Display, damage: &[crate::disp::Rect]) {
        let _ = (d, damage);
    }
    /// Win-OS/2's top-level windows changed (seamless sessions,
    /// crate::vwin): bottom of the z-order first, and the active one.
    fn windows(&mut self, list: &[crate::vwin::Window], active: u16) {
        let _ = (list, active);
    }
    /// The PC speaker: `Some(hz)` sounding, `None` silent.
    fn speaker(&mut self, hz: Option<u32>) {
        let _ = hz;
    }
    /// Sound Blaster output: 8-bit unsigned mono PCM at `rate` Hz.
    fn pcm(&mut self, rate: u32, data: &[u8]) {
        let _ = (rate, data);
    }
    /// A diagnostic message.
    fn log(&mut self, msg: &str) {
        let _ = msg;
    }
    /// A ROM font of `height` (8, 14 or 16) pixels: 256 characters of
    /// `height` bytes. None: the machine's built-in shapes.
    fn font(&mut self, height: u8) -> Option<Vec<u8>> {
        let _ = height;
        None
    }
}
