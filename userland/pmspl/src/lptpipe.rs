//! Printer ports opened by programs (`DosOpen("LPT1")`, DOS sessions'
//! printer output): while the spooler runs, the kernel connects each
//! such open to the spooler's port pipe (`\PIPE\SPOOLER\PORT`, kernel
//! `chardev::lpt_spool_open`) and the output becomes a PM_Q_RAW job on
//! the queue of the port's printer, as IBM's spooler takes it from the
//! printer device driver (its `KernelPipeThread`, SplFSOpen/SplFSClose).
//!
//! The kernel sends `LPTn\0program\0pid\0`; the answer is one byte, 0
//! when a printer prints to that port (the job is then made from what
//! follows, from the first byte written, and ends when the program
//! closes the port), 1 when the port is to be used directly (no printer
//! on it, or spooling disabled).

use alloc::vec::Vec;

use crate::ini::{self, Printer};
use crate::util::{eq, fields, trim, z};

const PIPE: &[u8] = b"\\PIPE\\SPOOLER\\PORT\0";
/// Connections served at once (one thread each).
const INSTANCES: u32 = 4;

/// Start the threads serving the port pipe.
pub fn start() {
    for _ in 0..INSTANCES {
        let mut tid = 0;
        unsafe { os2::DosCreateThread(&mut tid, pipe_thread, 0, 0, 64 * 1024) };
    }
}

extern "C" fn pipe_thread(_: u32) {
    let mut h = 0;
    // Duplex, byte stream, blocking.
    let rc = unsafe { os2::DosCreateNPipe(PIPE.as_ptr(), &mut h, 2, INSTANCES, 4096, 8192, 0) };
    if rc != 0 {
        return;
    }
    loop {
        if unsafe { os2::DosConnectNPipe(h) } != 0 {
            unsafe { os2::DosSleep(100) };
            continue;
        }
        serve(h);
        unsafe { os2::DosDisConnectNPipe(h) };
    }
}

/// The printer printing to `port`, if spooling is on.
fn spooled_printer(port: &[u8]) -> Option<Printer> {
    crate::jobs::locked(|| {
        if ini::get_str(ini::SPOOLER, b"SPOOL").is_some_and(|v| trim(fields(&v).first().copied().unwrap_or(b"1")) == b"0") {
            return None;
        }
        ini::printers().into_iter().find(|p| eq(&p.port, port) && !p.queues.is_empty())
    })
}

fn read(h: u32, buf: &mut [u8]) -> usize {
    let mut n = 0;
    if unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 {
        return 0;
    }
    n as usize
}

fn serve(h: u32) {
    // The request: three strings.
    let mut req = Vec::new();
    let mut buf = [0u8; 4096];
    while req.iter().filter(|&&c| c == 0).count() < 3 {
        let n = read(h, &mut buf[..1]);
        if n == 0 {
            return;
        }
        req.push(buf[0]);
    }
    let parts: Vec<&[u8]> = req.split(|&c| c == 0).collect();
    let (port, prog) = (parts[0], parts[1]);
    let Some(printer) = spooled_printer(port) else {
        let mut w = 0;
        unsafe { os2::DosWrite(h, [1u8].as_ptr(), 1, &mut w) };
        return;
    };
    let mut w = 0;
    unsafe { os2::DosWrite(h, [0u8].as_ptr(), 1, &mut w) };
    let queue = z(&printer.queues[0]);
    let datatype = b"PM_Q_RAW\0";
    let doc = if prog.is_empty() { z(port) } else { z(prog) };
    let dop: [u32; 4] = [queue.as_ptr() as u32, 0, 0, datatype.as_ptr() as u32];
    let mut hspl = 0;
    loop {
        let n = read(h, &mut buf);
        if n == 0 {
            break;
        }
        if hspl == 0 {
            hspl = crate::qm::SplQmOpen(b"*\0".as_ptr(), 4, dop.as_ptr());
            if hspl == 0 {
                break;
            }
            crate::qm::SplQmStartDoc(hspl, doc.as_ptr());
        }
        crate::qm::SplQmWrite(hspl, n as i32, buf.as_ptr());
    }
    if hspl != 0 {
        crate::qm::SplQmEndDoc(hspl);
        crate::qm::SplQmClose(hspl);
    }
}
