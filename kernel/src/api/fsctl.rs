//! DosFSCtl and DosFSAttach: requests routed to a file system driver by
//! file handle, path or FSD name.
//!
//! rs2's file systems are built into the kernel: the generic functions
//! (FSCTL_ERROR_INFO, FSCTL_MAX_EASIZE, FSCTL_GET_NEXT_ROUTE_NAME) are
//! answered here. FSD-private functions (0x8000 and up: the HPFS cache
//! controls of CACHE.EXE, the JFS functions of CACHEJFS, DEFRAGFS,
//! EXTENDFS, CLRBBLKS) get ERROR_INVALID_FUNCTION, which is what an FSD
//! without that function returns; the programs then report that the file
//! system does not support the operation.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::fs::{FileSystem, Handle};

const ERROR_INVALID_FSD_NAME: u32 = 252;
const ERROR_ALREADY_ASSIGNED: u32 = 85;

const FSCTL_HANDLE: u32 = 1;
const FSCTL_PATHNAME: u32 = 2;
const FSCTL_FSDNAME: u32 = 3;

const FSCTL_ERROR_INFO: u32 = 1;
const FSCTL_MAX_EASIZE: u32 = 2;
const FSCTL_GET_NEXT_ROUTE_NAME: u32 = 3;
const FSCTL_DAEMON_QUERY: u32 = 4;

/// The file system names a request can be routed to (FSCTL_FSDNAME),
/// as DosQueryFSAttach reports them.
const FSD_NAMES: &[&str] = &["FAT", "HPFS", "CDFS"];

fn fs_name(fs: &Arc<dyn FileSystem>) -> String {
    String::from(fs.name()).to_ascii_uppercase()
}

/// Find the file system a request goes to.
fn route(c: &Ctx, method: u32, proute: u32, hfile: u32) -> Result<String, u32> {
    match method {
        FSCTL_HANDLE => match super::file::get(c, hfile) {
            Some(Handle::File(f)) => crate::fs::fs_of(&f.path).map(|fs| fs_name(&fs)),
            Some(Handle::Drive(_)) => Ok(String::from("FAT")),
            Some(_) => Err(err::INVALID_FUNCTION), // devices and pipes have no FSD
            None => Err(err::INVALID_HANDLE),
        },
        FSCTL_PATHNAME => {
            let path = c.read_str(proute, 260).ok_or(err::INVALID_ADDRESS)?;
            let p = super::file::resolve(c, &path)?;
            crate::fs::fs_of(&p).map(|fs| fs_name(&fs))
        }
        FSCTL_FSDNAME => {
            let name = c.read_str(proute, 64).ok_or(err::INVALID_ADDRESS)?.to_ascii_uppercase();
            if FSD_NAMES.contains(&name.as_str()) {
                Ok(name)
            } else {
                Err(ERROR_INVALID_FSD_NAME)
            }
        }
        _ => Err(err::INVALID_LEVEL),
    }
}

/// The caller's buffers: data, its size, where its length goes; same for
/// the parameters. 16-bit callers have USHORT lengths.
struct Io {
    data: u32,
    cb_data: u32,
    pcb_data: u32,
    parm: u32,
    cb_parm: u32,
    pcb_parm: u32,
    short: bool,
}

impl Io {
    fn set_len(&self, c: &Ctx, at: u32, n: u32) {
        if at != 0 {
            if self.short { c.write_u16(at, n as u16) } else { c.write_u32(at, n) };
        }
    }

    fn reply(&self, c: &Ctx, data: &[u8]) -> u32 {
        self.set_len(c, self.pcb_data, data.len() as u32);
        if (data.len() as u32) > self.cb_data {
            return err::BUFFER_OVERFLOW;
        }
        if !data.is_empty() && !c.write(self.data, data) {
            return err::INVALID_ADDRESS;
        }
        0
    }
}

fn fsctl(c: &Ctx, io: &Io, func: u32, method: u32, proute: u32, hfile: u32) -> u32 {
    if func == FSCTL_GET_NEXT_ROUTE_NAME {
        // ROUTENAMEBUF in the data buffer: handle in, next handle and name out.
        let h = c.read_u32(io.data).unwrap_or(0) as usize;
        let Some(name) = FSD_NAMES.get(h) else { return err::NO_MORE_ITEMS };
        let mut b = Vec::new();
        b.extend_from_slice(&(h as u32 + 1).to_le_bytes());
        b.extend_from_slice(name.as_bytes());
        b.push(0);
        return io.reply(c, &b);
    }
    let fsd = match route(c, method, proute, hfile) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match func {
        FSCTL_ERROR_INFO => {
            // The FSD's text for an error code in the parameters: rs2's
            // file systems use only the system's messages.
            let _ = (io.parm, io.cb_parm);
            io.set_len(c, io.pcb_parm, 0);
            io.reply(c, &[0])
        }
        FSCTL_MAX_EASIZE => {
            // EASIZEBUF { USHORT cbMaxEASize; ULONG cbMaxEAListSize }.
            let (one, list): (u16, u32) = match fsd.as_str() {
                "CDFS" => (0, 0),
                _ => (65535, 65536),
            };
            let mut b = Vec::new();
            b.extend_from_slice(&one.to_le_bytes());
            b.extend_from_slice(&list.to_le_bytes());
            io.reply(c, &b)
        }
        FSCTL_DAEMON_QUERY => {
            // No FSD daemon threads to start.
            io.reply(c, &[0, 0, 0, 0])
        }
        _ => {
            log_once(c, &fsd, func);
            err::INVALID_FUNCTION
        }
    }
}

static SEEN: crate::sync::SpinLock<Vec<(String, u32)>> = crate::sync::SpinLock::new(Vec::new());

fn log_once(c: &Ctx, fsd: &str, func: u32) {
    let mut s = SEEN.lock();
    if !s.iter().any(|(n, f)| n == fsd && *f == func) {
        s.push((String::from(fsd), func));
        crate::println!("[DosFSCtl {:#x} for {} from {}: not supported]", func, fsd, c.process.exe.name);
    }
}

/// DosFSCtl(pData, cbData, pcbData, pParms, cbParms, pcbParms, function,
/// pszRoute, hFile, method)
pub fn dos_fsctl(c: &mut Ctx) -> u32 {
    let io = Io {
        data: c.arg(0),
        cb_data: c.arg(1),
        pcb_data: c.arg(2),
        parm: c.arg(3),
        cb_parm: c.arg(4),
        pcb_parm: c.arg(5),
        short: false,
    };
    fsctl(c, &io, c.arg(6), c.arg(9), c.arg(7), c.arg(8))
}

/// 16-bit DosFSCtl(pData, cbData, pcbData, pParms, cbParms, pcbParms,
/// function, pszRoute, hFile, method, reserved)
pub fn dos16_fsctl(c: &mut Ctx) -> u32 {
    let io = Io {
        data: c.ptr(0),
        cb_data: c.arg(1),
        pcb_data: c.ptr(2),
        parm: c.ptr(3),
        cb_parm: c.arg(4),
        pcb_parm: c.ptr(5),
        short: true,
    };
    let (func, proute, hfile, method) = (c.arg(6), c.ptr(7), c.arg(8), c.arg(9));
    fsctl(c, &io, func, method, proute, hfile)
}

/// DosFSAttach(pszDevice, pszFilesystem, pData, cbData, flag): attaching
/// installable file systems or remote drives (NFS, LAN) needs an FSD,
/// which rs2 does not load. Detaching a built-in drive is not allowed.
pub fn dos_fs_attach(c: &mut Ctx) -> u32 {
    let (pdev, pfs, flag) = if c.is16 { (c.ptr(0), c.ptr(1), c.arg(4)) } else { (c.arg(0), c.arg(1), c.arg(4)) };
    let Some(dev) = c.read_str(pdev, 260) else { return err::INVALID_ADDRESS };
    let fsd = c.read_str(pfs, 64).unwrap_or_default().to_ascii_uppercase();
    match flag {
        0 => {
            // FS_ATTACH
            if dev.len() == 2 && dev.ends_with(':') {
                let d = dev.as_bytes()[0].to_ascii_uppercase();
                if (b'A'..=b'Z').contains(&d) && crate::fs::drive(d - b'A').is_some() {
                    return ERROR_ALREADY_ASSIGNED;
                }
            }
            if FSD_NAMES.contains(&fsd.as_str()) { err::NOT_SUPPORTED } else { ERROR_INVALID_FSD_NAME }
        }
        1 => err::INVALID_DRIVE, // FS_DETACH
        2 | 3 => err::NOT_SUPPORTED, // spooler devices
        _ => err::INVALID_LEVEL,
    }
}
