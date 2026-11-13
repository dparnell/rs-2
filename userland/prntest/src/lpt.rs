//! `PRNTEST LPT [n]`: the parallel port driver itself (kernel
//! parport.rs), without the spooler: the category 5 IOCtls on LPTn and
//! output to the port. Run it where no spooler serves the port (as the
//! init program); QEMU's `-parallel file:F` then holds what was printed,
//! which must be SI ESC 0 (PRT_SETFRAMECTL 132/8) followed by the test
//! pattern (`pattern()`: "RS2 LPT TEST\r\n" and 16 times the bytes
//! 0-255, 4110 bytes in all).

use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::*;

use crate::check;

/// What the test writes after the frame control bytes.
pub fn pattern() -> Vec<u8> {
    let mut v = b"RS2 LPT TEST\r\n".to_vec();
    for _ in 0..16 {
        v.extend(0..=255u8);
    }
    v
}

/// DosDevIOCtl category 5: (rc, data returned).
fn ioctl(hf: u32, f: u32, data: &[u8], out: usize) -> (u32, Vec<u8>) {
    let mut buf = alloc::vec![0u8; data.len().max(out)];
    buf[..data.len()].copy_from_slice(data);
    let mut parm = [0u8; 1];
    let (mut pl, mut dl) = (1u32, buf.len() as u32);
    // A buffer with a zero maximum length must be NULL (ERROR_INVALID_PARAMETER
    // otherwise, as IBM's kernel checks).
    let pdata = if buf.is_empty() { null_mut() } else { buf.as_mut_ptr() };
    let rc = unsafe { DosDevIOCtl(hf, 5, f, parm.as_mut_ptr(), 1, &mut pl, pdata, buf.len() as u32, &mut dl) };
    buf.truncate(if rc == 0 { (dl as usize).min(buf.len()) } else { 0 });
    (rc, buf)
}

pub fn run(n: u8) {
    let name = [b'L', b'P', b'T', b'0' + n, 0];
    let port = core::str::from_utf8(&name[..4]).unwrap_or("LPT");
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe { DosOpen(name.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYNONE, null_mut()) };
    if !check(&alloc::format!("DosOpen {port}"), rc == 0) {
        return;
    }
    // No pop-ups: errors come back to the program.
    unsafe { DosError(0) };

    let (rc, st) = ioctl(hf, 0x66, &[], 1);
    println!("{port} status {:#04x}", st.first().copied().unwrap_or(0));
    check("PRT_GETPRINTERSTATUS: selected, not busy, no error or timeout", rc == 0 && st.len() == 1 && st[0] & 0x99 == 0x90);

    let (rc, t) = ioctl(hf, 0x6e, &[], 2);
    check("PRT_QUERYIRQTIMEOUT: 120 seconds (PRINT01's default)", rc == 0 && t == [120, 0]);
    let (rc, _) = ioctl(hf, 0x4e, &[5, 0], 0);
    let (rc2, t) = ioctl(hf, 0x6e, &[], 2);
    check("PRT_SETIRQTIMEOUT 5, read back", rc == 0 && rc2 == 0 && t == [5, 0]);

    let (rc, r) = ioctl(hf, 0x64, &[], 1);
    check("PRT_GETINFINITERETRY: off", rc == 0 && r == [0]);
    let (rc, _) = ioctl(hf, 0x44, &[1], 0);
    let (rc2, r) = ioctl(hf, 0x64, &[], 1);
    check("PRT_SETINFINITERETRY 1, read back", rc == 0 && rc2 == 0 && r == [1]);
    let (rc, _) = ioctl(hf, 0x44, &[2], 0);
    check("PRT_SETINFINITERETRY 2: ERROR_INVALID_PARAMETER", rc == 87);
    let (rc, _) = ioctl(hf, 0x44, &[0], 0);
    check("PRT_SETINFINITERETRY 0", rc == 0);

    let (rc, fr) = ioctl(hf, 0x62, &[], 2);
    check("PRT_GETFRAMECTL: 80 characters per line, 6 lines per inch", rc == 0 && fr == [80, 6]);
    let (rc, _) = ioctl(hf, 0x42, &[100, 6], 0);
    check("PRT_SETFRAMECTL 100/6 (not one of the four): ERROR_INVALID_PARAMETER", rc == 87);
    let (rc, _) = ioctl(hf, 0x42, &[132, 8], 0);
    let (rc2, fr) = ioctl(hf, 0x62, &[], 2);
    check("PRT_SETFRAMECTL 132/8, read back", rc == 0 && rc2 == 0 && fr == [132, 8]);

    let (rc, f) = ioctl(hf, 0x69, &[], 4);
    check("PRT_QUERYACTIVEFONT: the printer's own (0, 0)", rc == 0 && f == [0, 0, 0, 0]);
    let (rc, _) = ioctl(hf, 0x6a, &[0, 0, 0, 0], 0);
    check("PRT_VERIFYFONT 0/0", rc == 0);
    let (rc, _) = ioctl(hf, 0x48, &[0xd2, 0x04, 5, 0], 0);
    check("PRT_ACTIVATEFONT 1234/5 (not available): device error 0xFF03", rc == 0xff03);
    let mut cp = [0u32; 2];
    let mut cb = 0u32;
    unsafe { DosQueryCp(8, cp.as_mut_ptr(), &mut cb) };
    let sys = cp[0] as u16;
    let (rc, _) = ioctl(hf, 0x48, &sys.to_le_bytes().iter().chain(&[0, 0]).copied().collect::<Vec<u8>>(), 0);
    let (rc2, f) = ioctl(hf, 0x69, &[], 4);
    check(&alloc::format!("PRT_ACTIVATEFONT {sys}/0 (a prepared code page), read back"), rc == 0 && rc2 == 0 && f[..2] == sys.to_le_bytes() && f[2..] == [0, 0]);

    let (rc, _) = ioctl(hf, 0x4d, b"rs2 parallel port test\0", 0);
    check("PRT_SETPRINTJOBTITLE", rc == 0);

    let (rc, m) = ioctl(hf, 0x72, &[], 1);
    check("PRT_QUERYCOMMMODE: compatibility", rc == 0 && m == [0]);
    let (rc, _) = ioctl(hf, 0x52, &[1], 0);
    let (rc2, m) = ioctl(hf, 0x72, &[], 1);
    check("PRT_SETCOMMMODE nibble, read back", rc == 0 && rc2 == 0 && m == [1]);
    let (rc, _) = ioctl(hf, 0x52, &[9], 0);
    check("PRT_SETCOMMMODE 9: ERROR_INVALID_PARAMETER", rc == 87);
    let (rc_ecp, _) = ioctl(hf, 0x52, &[4], 0);
    let (rc_fifo, _) = ioctl(hf, 0x53, &[1], 0);
    println!("{port}: ECP mode {}, FIFO transfers {}", if rc_ecp == 0 { "available" } else { "not available" }, if rc_fifo == 0 { "available" } else { "not available" });
    check("ECP mode and FIFO transfers: both or neither (an ECP chip)", (rc_ecp == 0) == (rc_fifo == 0) && (rc_ecp == 0 || rc_ecp == 87));
    let (rc, _) = ioctl(hf, 0x52, &[0], 0);
    let (rc2, _) = ioctl(hf, 0x53, &[0], 0);
    let (rc3, x) = ioctl(hf, 0x73, &[], 1);
    check("PRT_SETCOMMMODE compatibility, PRT_SETDATAXFERMODE programmed I/O, read back", rc == 0 && rc2 == 0 && rc3 == 0 && x == [0]);
    let (rc, _) = ioctl(hf, 0x53, &[2], 0);
    check("PRT_SETDATAXFERMODE 2 (DMA): ERROR_INVALID_PARAMETER", rc == 87);

    let (rc, id) = ioctl(hf, 0x74, &[], 256);
    if rc == 0 {
        println!("{port} device ID: {}", id.iter().skip(2).map(|&c| c as char).collect::<alloc::string::String>());
        check("PRT_QUERDEVICEID: length prefix", id.len() >= 2 && u16::from_be_bytes([id[0], id[1]]) as usize >= id.len().min(2));
    } else {
        println!("{port}: no IEEE 1284 device ID (rc {rc})");
        check("PRT_QUERDEVICEID without a 1284 peripheral: ERROR_BAD_COMMAND", rc == 22);
    }

    let (rc, _) = ioctl(hf, 0x7f, &[], 0);
    check("an unknown printer function: ERROR_INVALID_FUNCTION", rc == 1);
    let (rc, _) = ioctl(hf, 0x21, &[], 2);
    check("PRT_QUERYJOBHANDLE on the port itself (no spooler): ERROR_INVALID_FUNCTION", rc == 1);
    let mut d = [0u8; 2];
    let (mut pl, mut dl) = (0u32, 2u32);
    let rc = unsafe { DosDevIOCtl(hf, 1, 0x61, null_mut(), 0, &mut pl, d.as_mut_ptr(), 2, &mut dl) };
    check("category 1 on a printer port: ERROR_INVALID_CATEGORY", rc == 117);

    // Output: the frame control bytes went out with PRT_SETFRAMECTL; then
    // the pattern in pieces.
    let data = pattern();
    let mut total = 0u32;
    let mut ok = true;
    for chunk in data.chunks(1000) {
        let mut w = 0u32;
        let rc = unsafe { DosWrite(hf, chunk.as_ptr(), chunk.len() as u32, &mut w) };
        ok &= rc == 0 && w == chunk.len() as u32;
        total += w;
    }
    check(&alloc::format!("DosWrite of {} bytes to {port}", data.len()), ok && total == data.len() as u32);
    let (rc, st) = ioctl(hf, 0x66, &[], 1);
    check("PRT_GETPRINTERSTATUS after the output: no timeout", rc == 0 && st.len() == 1 && st[0] & 0x01 == 0);

    let (rc, _) = ioctl(hf, 0x46, &[], 0);
    let (rc2, fr) = ioctl(hf, 0x62, &[], 2);
    check("PRT_INITPRINTER: the frame is 80/6 again", rc == 0 && rc2 == 0 && fr == [80, 6]);
    // The defaults back for the next program.
    ioctl(hf, 0x4e, &[120, 0], 0);
    ioctl(hf, 0x48, &[0, 0, 0, 0], 0);
    unsafe { DosClose(hf) };
}

fn ms_count() -> u32 {
    let mut v = 0u32;
    unsafe { DosQuerySysInfo(14, 14, &mut v, 4) };
    v
}

/// `PRNTEST LPT n STALL`, with `lptstall=2000,4000` on the kernel command
/// line (the port reports "out of paper" for 4 s after every 2000
/// bytes): a write ends on the timeout with ERROR_OUT_OF_PAPER and the
/// status says timeout; with infinite retry the rest waits the stalls
/// out. The host file holds the pattern once.
pub fn stall(n: u8, popup: bool) {
    let name = [b'L', b'P', b'T', b'0' + n, 0];
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe { DosOpen(name.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYNONE, null_mut()) };
    if !check("DosOpen", rc == 0) {
        return;
    }
    if !popup {
        unsafe { DosError(0) };
    }
    ioctl(hf, 0x4e, &[2, 0], 0);
    ioctl(hf, 0x44, &[0], 0);
    let data = pattern();
    let t0 = ms_count();
    let mut w = 0u32;
    let rc = unsafe { DosWrite(hf, data.as_ptr(), data.len() as u32, &mut w) };
    let dt = ms_count().wrapping_sub(t0);
    println!("stalled write: rc {rc}, {w} of {} bytes, {dt} ms", data.len());
    check("a printer out of paper past the timeout: ERROR_OUT_OF_PAPER, part written, after about 2 s", rc == 28 && w > 0 && (w as usize) < data.len() && (1500..4000).contains(&dt));
    let (rc2, st) = ioctl(hf, 0x66, &[], 1);
    println!("status {:#04x}", st.first().copied().unwrap_or(0));
    check("PRT_GETPRINTERSTATUS: timeout, out of paper, busy", rc2 == 0 && st.len() == 1 && st[0] & 0xa1 == 0x21);
    ioctl(hf, 0x44, &[1], 0);
    let rest = &data[w as usize..];
    let t0 = ms_count();
    let mut w2 = 0u32;
    let rc = unsafe { DosWrite(hf, rest.as_ptr(), rest.len() as u32, &mut w2) };
    let dt = ms_count().wrapping_sub(t0);
    println!("infinite retry: rc {rc}, {w2} of {} bytes, {dt} ms", rest.len());
    check("infinite retry: the rest written after the stalls, no error", rc == 0 && w2 as usize == rest.len() && dt >= 1500);
    let (_, st) = ioctl(hf, 0x66, &[], 1);
    check("PRT_GETPRINTERSTATUS: the timeout bit cleared", st.len() == 1 && st[0] & 0x01 == 0);
    ioctl(hf, 0x44, &[0], 0);
    ioctl(hf, 0x4e, &[120, 0], 0);
    unsafe { DosClose(hf) };
}
