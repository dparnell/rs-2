//! Hard errors: DosError and the "drive not ready" pop-up.
//!
//! When a file system operation fails with a hard error (not ready, write
//! protected, CRC, seek, sector not found, read/write fault, general
//! failure, wrong disk) and the process has not disabled hard-error
//! pop-ups with DosError(FERR_DISABLEHARDERR), a pop-up asks whether to
//! retry the operation, return the error to the program, or end the
//! program, as OS/2 does. The devices of mounted volumes are wrapped by
//! [`HardErrDev`], which retries the transfer on "Retry".

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::fs::block::BlockDev;
use crate::sync::SpinLock;

/// DosError flags by process: bit 0 FERR_ENABLEHARDERR, bit 1
/// FERR_DISABLEEXCEPTION. Absent = 1 (pop-ups on).
static FLAGS: SpinLock<BTreeMap<u32, u32>> = SpinLock::new(BTreeMap::new());

fn current_pid() -> u32 {
    crate::task::sched::current_opt().and_then(|t| t.process()).map_or(0, |p| p.pid)
}

/// DosError(fl): 16-bit (USHORT) and 32-bit (ULONG) forms.
pub fn dos_error(c: &mut Ctx) -> u32 {
    let fl = c.arg(0);
    if fl & !3 != 0 {
        return err::INVALID_PARAMETER;
    }
    FLAGS.lock().insert(c.process.pid, fl);
    0
}

/// Whether process `pid` wants hard-error pop-ups.
pub fn enabled(pid: u32) -> bool {
    FLAGS.lock().get(&pid).is_none_or(|f| f & 1 != 0)
}

/// Whether process `pid` wants pop-ups for unhandled exceptions.
pub fn exception_popups(pid: u32) -> bool {
    FLAGS.lock().get(&pid).is_none_or(|f| f & 2 == 0)
}

pub fn process_ended(pid: u32) {
    FLAGS.lock().remove(&pid);
}

fn is_hard(e: u32) -> bool {
    matches!(e, 19..=31 | 34)
}

fn text(e: u32) -> &'static str {
    match e {
        19 => "The media is write protected.",
        20 => "The system cannot find the device specified.",
        21 => "The drive is not ready.",
        22 => "The device does not recognize the command.",
        23 => "Data error (cyclic redundancy check).",
        24 => "The program issued a command but the command length is incorrect.",
        25 => "The drive cannot locate a specific area or track on the disk.",
        26 => "The specified disk or diskette cannot be accessed.",
        27 => "The drive cannot find the sector requested.",
        28 => "The printer is out of paper.",
        29 => "The system cannot write to the specified device.",
        30 => "The system cannot read from the specified device.",
        31 => "A device attached to the system is not functioning.",
        _ => "The wrong diskette is in the drive.",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Retry,
    /// Return the error to the program.
    Fail,
    /// End the program.
    End,
}

const CHOICES: [(&str, Action); 3] =
    [("Return error code to program", Action::Fail), ("Retry the operation", Action::Retry), ("End the program", Action::End)];

/// Ask the user what to do about hard error `e` on drive `drive`.
pub fn popup(e: u32, drive: Option<u8>) -> Action {
    let pid = current_pid();
    if pid == 0 || !enabled(pid) || crate::session::popup_owner(pid) {
        return Action::Fail;
    }
    if crate::session::popup_begin(true, false).is_err() {
        return Action::Fail;
    }
    let title = {
        let sid = crate::session::current();
        crate::session::list().into_iter().find(|(i, _)| *i == sid).map_or(String::from("OS/2 Window"), |(_, t)| t)
    };
    let prog = crate::task::sched::current_opt().and_then(|t| t.process()).map(|p| p.exe.name.clone()).unwrap_or_default();
    let mut sel = 1;
    let action = loop {
        draw(e, drive, &title, &prog, sel);
        let Some(k) = crate::kbd::get_key_from(crate::session::POPUP_SID, true) else { break Action::Fail };
        match (k.ch.to_ascii_uppercase(), k.scan) {
            (b'R', _) => break Action::Retry,
            (b'C', _) | (0x1b, _) => break Action::Fail,
            (b'E', _) => break Action::End,
            (b'\r', _) => break CHOICES[sel].1,
            (0 | 0xe0, 0x48) => sel = (sel + CHOICES.len() - 1) % CHOICES.len(),
            (0 | 0xe0, 0x50) => sel = (sel + 1) % CHOICES.len(),
            _ => {}
        }
    };
    let _ = crate::session::popup_end();
    if action == Action::End {
        if let Some(p) = crate::task::sched::current_opt().and_then(|t| t.process()) {
            p.exit(1, 0xffff);
        }
    }
    action
}

fn draw(e: u32, drive: Option<u8>, title: &str, prog: &str, sel: usize) {
    const BOX: u16 = 0x4f00;
    const HI: u16 = 0x2f00;
    crate::screen::with_session(crate::session::POPUP_SID, |s| {
        for r in 0..s.rows {
            for c in 0..s.cols {
                s.put(r, c, 0x0720);
            }
        }
        let (top, left, w) = (6usize, 6usize, 68usize);
        let mut lines: Vec<(String, u16)> = Vec::new();
        lines.push((String::new(), BOX));
        lines.push((alloc::format!("Session Title: {title}"), BOX));
        lines.push((String::new(), BOX));
        lines.push((alloc::format!("SYS{e:04}: {}", text(e)), BOX));
        if let Some(d) = drive {
            lines.push((alloc::format!("Drive {}:", (b'A' + d) as char), BOX));
        }
        if !prog.is_empty() {
            lines.push((alloc::format!("Program: {prog}"), BOX));
        }
        lines.push((String::new(), BOX));
        for (i, (t, _)) in CHOICES.iter().enumerate() {
            lines.push((alloc::format!("  {t}"), if i == sel { HI } else { BOX }));
        }
        lines.push((String::new(), BOX));
        lines.push((String::from("R = Retry   C = Return error   E = End   Enter = selected"), BOX));
        lines.push((String::new(), BOX));
        for (i, (t, attr)) in lines.iter().enumerate() {
            let b = t.as_bytes();
            for c in 0..w {
                let ch = if c >= 2 { b.get(c - 2).copied().unwrap_or(b' ') } else { b' ' };
                s.put(top + i, left + c, attr | ch as u16);
            }
        }
        s.set_cursor(None);
    });
}

/// Run `op`, asking about hard errors. `drive` is only looked up when an
/// error happens: reads run under file system locks.
fn retry<T>(drive: impl Fn() -> Option<u8>, mut op: impl FnMut() -> Result<T, u32>) -> Result<T, u32> {
    loop {
        match op() {
            Err(e) if is_hard(e) => match popup(e, drive()) {
                Action::Retry => continue,
                Action::Fail => return Err(e),
                Action::End => return Err(err::INTERRUPT),
            },
            r => return r,
        }
    }
}

/// Block devices wrapped by [`wrap`]: address of the wrapper and the drive
/// it was mounted as (0xff until known).
static WRAPPED: SpinLock<Vec<(usize, u8)>> = SpinLock::new(Vec::new());

/// A wrapped device was mounted as drive `d`.
pub fn note_drive(dev: usize, d: u8) {
    if let Some(e) = WRAPPED.lock().iter_mut().find(|e| e.0 == dev) {
        e.1 = d;
    }
}

/// A volume's device whose hard errors go through the pop-up.
pub struct HardErrDev(Arc<dyn BlockDev>);

/// Wrap the device of a volume (once).
pub fn wrap(dev: Arc<dyn BlockDev>) -> Arc<dyn BlockDev> {
    let addr = Arc::as_ptr(&dev) as *const () as usize;
    if WRAPPED.lock().iter().any(|e| e.0 == addr) {
        return dev;
    }
    let w: Arc<dyn BlockDev> = Arc::new(HardErrDev(dev));
    WRAPPED.lock().push((Arc::as_ptr(&w) as *const () as usize, 0xff));
    w
}

impl HardErrDev {
    fn drive(&self) -> Option<u8> {
        let me = self as *const HardErrDev as *const () as usize;
        WRAPPED.lock().iter().find(|e| e.0 == me).map(|e| e.1).filter(|&d| d != 0xff)
    }
}

impl BlockDev for HardErrDev {
    fn sector_size(&self) -> usize {
        self.0.sector_size()
    }
    fn sector_count(&self) -> u64 {
        self.0.sector_count()
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        retry(|| self.drive(), || self.0.read(lba, buf))
    }
    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        retry(|| self.drive(), || self.0.write(lba, buf))
    }
    fn flush(&self) -> Result<(), u32> {
        self.0.flush()
    }
}

impl Drop for HardErrDev {
    fn drop(&mut self) {
        let me = self as *const HardErrDev as *const () as usize;
        WRAPPED.lock().retain(|e| e.0 != me);
    }
}
