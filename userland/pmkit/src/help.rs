//! Help instances made from a table of panel numbers, as OS/2's applets
//! make them from their HELPTABLE resources.

use alloc::vec::Vec;

use crate::consts::*;
use crate::sys::*;
use crate::{message, until_nul, z};

/// A window's help: its id, its extended help panel and (item, panel)
/// pairs for its menu items or controls.
pub struct HelpWindow<'a> {
    pub id: u16,
    pub ext: u16,
    pub items: &'a [(u16, u16)],
}

/// The help instance of a program.
pub struct Help {
    pub instance: u32,
    table: Vec<u8>,
    subtables: Vec<Vec<u16>>,
    title: Vec<u8>,
    library: Vec<u8>,
}

/// The help file on the HELP path or in \OS2\HELP of the boot drive.
pub fn find_help_file(name: &[u8]) -> Option<Vec<u8>> {
    let dir = [&[crate::boot_drive()][..], b":\\OS2\\HELP\0"].concat();
    let n = z(name);
    let mut buf = [0u8; 260];
    // SEARCH_ENVIRONMENT: the directories in HELP.
    if unsafe { DosSearchPath(2, b"HELP\0".as_ptr(), n.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0
        || unsafe { DosSearchPath(0, dir.as_ptr(), n.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0
    {
        return Some(until_nul(&buf).to_vec());
    }
    None
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.228"]
    fn DosSearchPath(flags: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;
}

impl Help {
    /// Make the help instance for `frame` from the windows' tables and the
    /// help file `file` (found on the HELP path); None without the file.
    pub fn create(hab: u32, frame: u32, file: &[u8], title: &[u8], windows: &[HelpWindow]) -> Option<&'static mut Help> {
        let lib = find_help_file(file)?;
        let mut subtables = Vec::new();
        for w in windows {
            // HELPSUBTABLE: words per entry, (item, panel) pairs, 0.
            let mut sub = alloc::vec![2u16];
            for &(id, panel) in w.items {
                sub.extend_from_slice(&[id, panel]);
            }
            sub.push(0);
            subtables.push(sub);
        }
        let h: &'static mut Help = alloc::boxed::Box::leak(alloc::boxed::Box::new(Help { instance: 0, table: Vec::new(), subtables, title: z(title), library: z(&lib) }));
        // HELPTABLE (packed): idAppWindow, phstHelpSubTable, idExtPanel;
        // a zero entry ends it.
        for (w, sub) in windows.iter().zip(h.subtables.iter()) {
            h.table.extend_from_slice(&w.id.to_le_bytes());
            h.table.extend_from_slice(&(sub.as_ptr() as u32).to_le_bytes());
            h.table.extend_from_slice(&w.ext.to_le_bytes());
        }
        h.table.extend_from_slice(&[0u8; 8]);
        let mut init = HelpInit {
            cb: core::mem::size_of::<HelpInit>() as u32,
            ret: 0,
            tutorial: core::ptr::null(),
            table: h.table.as_ptr(),
            table_module: 0,
            accel_module: 0,
            accel_id: 0,
            action_bar_id: 0,
            title: h.title.as_ptr(),
            show_panel_id: 0,
            library: h.library.as_ptr(),
        };
        let inst = unsafe { WinCreateHelpInstance(hab, &mut init) };
        if inst == 0 || init.ret != 0 {
            if inst != 0 {
                unsafe { WinDestroyHelpInstance(inst) };
            }
            return None;
        }
        if frame != 0 && unsafe { WinAssociateHelpInstance(inst, frame) } == 0 {
            unsafe { WinDestroyHelpInstance(inst) };
            return None;
        }
        h.instance = inst;
        Some(h)
    }

    /// Show a panel by its number.
    pub fn show(&self, panel: u16) {
        unsafe { WinSendMsg(self.instance, HM_DISPLAY_HELP, panel as u32, HM_RESOURCEID) };
    }

    pub fn send(&self, msg: u32, mp1: u32, mp2: u32) {
        unsafe { WinSendMsg(self.instance, msg, mp1, mp2) };
    }

    pub fn destroy(&mut self, frame: u32) {
        if self.instance != 0 {
            unsafe {
                WinAssociateHelpInstance(0, frame);
                WinDestroyHelpInstance(self.instance);
            }
            self.instance = 0;
        }
    }
}

/// Send a request to a help instance, or say that help is missing.
pub fn request(help: Option<&Help>, owner: u32, file: &[u8], title: &[u8], msg: u32, mp1: u32, mp2: u32) {
    match help {
        Some(h) => h.send(msg, mp1, mp2),
        None => {
            let t = [&b"Help is not available: the help file "[..], file, b" was not found."].concat();
            message(owner, &t, title, MB_OK | MB_INFORMATION);
        }
    }
}
