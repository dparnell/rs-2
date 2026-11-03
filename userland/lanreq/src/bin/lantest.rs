//! LANTEST: checks of the LAN requester against an SMB server
//! (tools/scripts/smbhost.py; tools/scripts/lantest.sh runs it all).
//! `LANTEST [server [share [printshare [user password]]]]`, by default
//! SMBHOST DATA LPT rs2 secret. Prints PASS/FAIL lines and a summary;
//! leaves FROMRS2.BIN (the pattern of `pattern()`) and a print job on the
//! server for the host side to compare.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use utils::files;
os2::entry!(main);

static mut FAILS: u32 = 0;
static mut PASSES: u32 = 0;

fn check(name: &str, ok: bool, detail: &str) {
    if ok {
        unsafe { PASSES += 1 };
        utils::outln(&alloc::format!("PASS {name}"));
    } else {
        unsafe { FAILS += 1 };
        utils::outln(&alloc::format!("FAIL {name} {detail}"));
    }
}

/// The test pattern: `n` bytes of a 32-bit LCG (the host side makes the same).
fn pattern(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect()
}

fn rc_of(r: Result<(), u32>) -> u32 {
    r.err().unwrap_or(0)
}

fn fs_info(d: &str) -> (u16, String, String) {
    let name = cstr(d);
    let mut buf = [0u8; 256];
    let mut cb = buf.len() as u32;
    if unsafe { DosQueryFSAttach(name.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } != 0 {
        return (0, String::new(), String::new());
    }
    let typ = u16::from_le_bytes([buf[0], buf[1]]);
    let cb_name = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    let cb_fsd = u16::from_le_bytes([buf[4], buf[5]]) as usize;
    let cb_data = u16::from_le_bytes([buf[6], buf[7]]) as usize;
    let at = 8 + cb_name + 1;
    let fsd: String = buf[at..at + cb_fsd].iter().map(|&b| b as char).collect();
    let dat: String = buf[at + cb_fsd + 1..at + cb_fsd + 1 + cb_data].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    (typ, fsd, dat)
}

unsafe extern "C" {
    #[link_name = "NETAPI32.146"]
    fn Net32WkstaGetInfo(server: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u32) -> u32;
    #[link_name = "NETAPI32.128"]
    fn Net32UseEnum(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u32, ptotal: *mut u32) -> u32;
    #[link_name = "NETAPI32.129"]
    fn Net32UseGetInfo(server: *const u8, name: *const u8, level: u32, buf: *mut u8, cb: u32, ptotal: *mut u32) -> u32;
    #[link_name = "NETAPI32.122"]
    fn Net32ShareEnum(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u32, ptotal: *mut u32) -> u32;
    #[link_name = "NETAPI32.101"]
    fn Net32ServerEnum2(server: *const u8, level: u32, buf: *mut u8, cb: u32, pread: *mut u32, ptotal: *mut u32, typ: u32, domain: *const u8) -> u32;
    #[link_name = "NETAPI32.39"]
    fn Net32ConfigGet2(server: *const u8, r: *const u8, comp: *const u8, param: *const u8, buf: *mut u8, cb: u32, plen: *mut u32) -> u32;
}

/// The C string a flat pointer in a NETAPI32 result points at.
fn ptr_str(b: &[u8], at: usize) -> String {
    let p = u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as *const u8;
    unsafe { lanreq::netapi::str_at(p) }.unwrap_or_default()
}

fn netapi32_checks(server: &str, share: &str, unc: &str) {
    let mut buf = alloc::vec![0u8; 4096];
    let (mut read, mut total) = (0u32, 0u32);
    let rc = unsafe { Net32WkstaGetInfo(core::ptr::null(), 10, buf.as_mut_ptr(), 4096, &mut total) };
    check("Net32WkstaGetInfo 10", rc == 0 && ptr_str(&buf, 0) == lanreq::status().computer && ptr_str(&buf, 8) == "WORKGROUP", &alloc::format!("{rc}"));
    let rc = unsafe { Net32UseEnum(core::ptr::null(), 1, buf.as_mut_ptr(), 4096, &mut read, &mut total) };
    let local: String = buf[..9].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    check("Net32UseEnum 1", rc == 0 && read >= 1 && local == "X:" && ptr_str(&buf, 10).eq_ignore_ascii_case(unc), &alloc::format!("{rc} {read} {local}"));
    let rc = unsafe { Net32UseEnum(core::ptr::null(), 1, buf.as_mut_ptr(), 20, &mut read, &mut total) };
    check("Net32UseEnum small buffer", rc == 234 && read == 0 && total >= 1, &alloc::format!("{rc} {read} {total}"));
    let rc = unsafe { Net32UseGetInfo(core::ptr::null(), cstr("X:").as_ptr(), 1, buf.as_mut_ptr(), 4096, &mut total) };
    check("Net32UseGetInfo", rc == 0 && u16::from_le_bytes([buf[20], buf[21]]) == 0, &alloc::format!("{rc}"));
    let rc = unsafe { Net32UseGetInfo(core::ptr::null(), cstr("Q:").as_ptr(), 1, buf.as_mut_ptr(), 4096, &mut total) };
    check("Net32UseGetInfo missing", rc == 2250, &alloc::format!("{rc}"));
    let srv = cstr(&alloc::format!("\\\\{server}"));
    let rc = unsafe { Net32ShareEnum(srv.as_ptr(), 1, buf.as_mut_ptr(), 4096, &mut read, &mut total) };
    let names: Vec<String> = (0..read as usize).map(|i| buf[i * 20..i * 20 + 13].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()).collect();
    check("Net32ShareEnum 1", rc == 0 && names.iter().any(|n| n.eq_ignore_ascii_case(share)), &alloc::format!("{rc} {names:?}"));
    let rc = unsafe { Net32ServerEnum2(core::ptr::null(), 1, buf.as_mut_ptr(), 4096, &mut read, &mut total, 0xFFFF_FFFF, core::ptr::null()) };
    let names: Vec<String> = (0..read as usize).map(|i| buf[i * 26..i * 26 + 16].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()).collect();
    check("Net32ServerEnum2 1", rc == 0 && names.iter().any(|n| n.eq_ignore_ascii_case(server)), &alloc::format!("{rc} {names:?}"));
    let rc = unsafe { Net32ServerEnum2(core::ptr::null(), 0, buf.as_mut_ptr(), 4096, &mut read, &mut total, 0x8000_0000, core::ptr::null()) };
    check("Net32ServerEnum2 domains", rc == 0 && read >= 1 && buf.starts_with(b"WORKGROUP\0"), &alloc::format!("{rc} {read}"));
    let rc = unsafe { Net32ConfigGet2(core::ptr::null(), core::ptr::null(), cstr("requester").as_ptr(), cstr("computername").as_ptr(), buf.as_mut_ptr(), 4096, &mut total) };
    check("Net32ConfigGet2", rc == 0 && buf.starts_with(b"RS2TEST\0"), &alloc::format!("{rc}"));
}

fn main(s: &os2::Startup) -> u32 {
    let a = utils::args(s);
    let arg = |i: usize, d: &str| a.get(i).cloned().unwrap_or_else(|| String::from(d));
    let (server, share, pshare, user, pw) = (arg(0, "SMBHOST"), arg(1, "DATA"), arg(2, "LPT"), arg(3, "rs2"), arg(4, "secret"));
    let unc = alloc::format!("\\\\{server}\\{share}");

    // The requester.
    let r = lanreq::ensure_started(&[]);
    check("requester starts", r.is_ok(), &alloc::format!("{r:?}"));
    let st = lanreq::status();
    check("requester status", st.started && !st.computer.is_empty(), &alloc::format!("{st:?}"));
    check("logon", rc_of(lanreq::logon(&user, &pw, "")) == 0, "");
    check("logged on user", lanreq::status().user == user, "");

    // Browsing.
    let servers = lanreq::servers(None).unwrap_or_default();
    check("server list", servers.iter().any(|x| x.name.eq_ignore_ascii_case(&server)), &alloc::format!("{servers:?}"));
    let shares = lanreq::shares(&server);
    let ok = shares.as_ref().is_ok_and(|v| v.iter().any(|x| x.name.eq_ignore_ascii_case(&share) && x.typ == 0) && v.iter().any(|x| x.name.eq_ignore_ascii_case(&pshare) && x.typ == 1));
    check("share list", ok, &alloc::format!("{shares:?}"));
    check("unknown server", lanreq::shares("NOSUCHSRV").is_err(), "");

    // NET USE X:.
    check("use add", rc_of(lanreq::use_add("X", &unc, None, None)) == 0, "");
    check("use add twice", rc_of(lanreq::use_add("X", &unc, None, None)) == 85, "");
    check("use bad share", rc_of(lanreq::use_add("Y", &alloc::format!("\\\\{server}\\NOSUCH"), None, None)) == 67, "");
    let uses = lanreq::uses().unwrap_or_default();
    check("use list", uses.iter().any(|u| u.local == "X:" && u.remote.eq_ignore_ascii_case(&unc)), &alloc::format!("{uses:?}"));
    netapi32_checks(&server, &share, &unc);
    let (typ, fsd, data) = fs_info("X:");
    check("fs attach", typ == 4 && fsd == "LAN" && data.eq_ignore_ascii_case(&unc), &alloc::format!("{typ} {fsd} {data}"));

    // Files the host put there.
    let hello = files::read_file("X:\\HELLO.TXT");
    check("read host file", hello.as_deref() == Ok(b"Hello from the host\r\n".as_slice()), &alloc::format!("{hello:?}"));
    let unc_hello = files::read_file(&alloc::format!("{unc}\\hello.txt"));
    check("read by UNC name", unc_hello == hello, "");
    let host = files::read_file("X:\\HOSTDATA.BIN").unwrap_or_default();
    check("read 5 MB host file", host == pattern(5 * 1024 * 1024, 7), &alloc::format!("len {}", host.len()));

    // A large file written and read back.
    let big = pattern(5 * 1024 * 1024 + 123, 42);
    check("write 5 MB", files::write_file("X:\\FROMRS2.BIN", &big).is_ok(), "");
    let st = files::status("X:\\FROMRS2.BIN");
    check("size", st.as_ref().is_some_and(|s| s.cb_file as usize == big.len()), "");
    check("read back", files::read_file("X:\\FROMRS2.BIN").is_ok_and(|d| d == big), "");

    // Directories and wildcards.
    let rc = unsafe { DosCreateDir(cstr("X:\\SUB1").as_ptr(), core::ptr::null_mut()) };
    check("mkdir", rc == 0, &alloc::format!("{rc}"));
    for n in ["A.TXT", "B.TXT", "C.DAT"] {
        let _ = files::write_file(&alloc::format!("X:\\SUB1\\{n}"), n.as_bytes());
    }
    let f = files::find_all("X:\\SUB1\\*.TXT", 0);
    check("find *.TXT", f.len() == 2, &alloc::format!("{}", f.len()));
    let f = files::find_all("X:\\SUB1\\?.*", 0);
    check("find ?.*", f.len() == 3, &alloc::format!("{}", f.len()));
    let f = files::find_all("\\\\.\\none\\*", 0);
    check("find bad UNC", f.is_empty(), "");
    let f = files::find_all(&alloc::format!("{unc}\\SUB1\\*"), 0x10);
    check("find by UNC", f.len() == 3, &alloc::format!("{}", f.len()));
    let f = files::find_all("X:\\*", 0x10);
    check("find directories", f.iter().any(|x| x.name.eq_ignore_ascii_case("SUB1") && x.attr & 0x10 != 0), "");

    // Rename, delete, rmdir.
    let rc = unsafe { DosMove(cstr("X:\\SUB1\\C.DAT").as_ptr(), cstr("X:\\SUB1\\D.DAT").as_ptr()) };
    check("rename", rc == 0 && files::exists("X:\\SUB1\\D.DAT") && !files::exists("X:\\SUB1\\C.DAT"), &alloc::format!("{rc}"));
    let rc = unsafe { DosDeleteDir(cstr("X:\\SUB1").as_ptr()) };
    check("rmdir non-empty fails", rc != 0, "");
    for n in ["A.TXT", "B.TXT", "D.DAT"] {
        let rc = unsafe { DosDelete(cstr(&alloc::format!("X:\\SUB1\\{n}")).as_ptr()) };
        check("delete", rc == 0, &alloc::format!("{n} {rc}"));
    }
    let rc = unsafe { DosDelete(cstr("X:\\SUB1\\A.TXT").as_ptr()) };
    check("delete missing", rc == 2, &alloc::format!("{rc}"));
    let rc = unsafe { DosDeleteDir(cstr("X:\\SUB1").as_ptr()) };
    check("rmdir", rc == 0 && !files::exists("X:\\SUB1"), &alloc::format!("{rc}"));

    // Attributes.
    let _ = files::write_file("X:\\ATTR.TXT", b"attributes");
    let rc = files::set_info("X:\\ATTR.TXT", 0x01 | 0x02, 0, 0);
    let st = files::status("X:\\ATTR.TXT");
    check("set read-only+hidden", rc == 0 && st.as_ref().is_some_and(|s| s.attr & 3 == 3), &alloc::format!("{rc}"));
    check("read-only refuses writes", files::write_file("X:\\ATTR.TXT", b"x").is_err(), "");
    let (d, t) = files::packed(&files::now());
    let rc = files::set_info("X:\\ATTR.TXT", 0x20, 0x5a21, 0x6000);
    let st = files::status("X:\\ATTR.TXT");
    check("clear attributes, set date", rc == 0 && st.as_ref().is_some_and(|s| s.attr & 3 == 0 && s.fdate_write == 0x5a21), &alloc::format!("{rc} {d:x} {t:x}"));

    // Extended attributes.
    let ea = files::Ea { flags: 0, name: b".SUBJECT".to_vec(), value: b"\xfd\xff\x04\x00test".to_vec() };
    let rc = files::write_eas("X:\\ATTR.TXT", core::slice::from_ref(&ea));
    let back = files::read_eas("X:\\ATTR.TXT").unwrap_or_default();
    check("extended attributes", rc == 0 && back.iter().any(|e| e.name == ea.name && e.value == ea.value), &alloc::format!("{rc} {}", back.len()));
    let _ = unsafe { DosDelete(cstr("X:\\ATTR.TXT").as_ptr()) };

    // DosCopy both ways.
    let rc = unsafe { DosCopy(cstr("X:\\FROMRS2.BIN").as_ptr(), cstr("C:\\LANCOPY.BIN").as_ptr(), 1) };
    check("copy from share", rc == 0 && files::read_file("C:\\LANCOPY.BIN").is_ok_and(|d| d == big), &alloc::format!("{rc}"));
    let rc = unsafe { DosCopy(cstr("C:\\LANCOPY.BIN").as_ptr(), cstr(&alloc::format!("{unc}\\COPY2.BIN")).as_ptr(), 1) };
    check("copy to share by UNC", rc == 0 && files::read_file("X:\\COPY2.BIN").is_ok_and(|d| d == big), &alloc::format!("{rc}"));
    let _ = unsafe { DosDelete(cstr("X:\\COPY2.BIN").as_ptr()) };
    let _ = unsafe { DosDelete(cstr("C:\\LANCOPY.BIN").as_ptr()) };

    // Free space.
    let mut fsa = FsAllocate::default();
    let rc = unsafe { DosQueryFSInfo(24, 1, &mut fsa as *mut _ as *mut u8, core::mem::size_of::<FsAllocate>() as u32) };
    check("free space", rc == 0 && fsa.units > 0, &alloc::format!("{rc}"));

    // Open files keep a NET USE /D from going through.
    let h = utils::open_read("X:\\FROMRS2.BIN");
    check("use del with open file", rc_of(lanreq::use_del("X", false)) == lanreq::NERR_OPEN_FILES, "");
    if let Ok(h) = h {
        unsafe { DosClose(h) };
    }

    // Printing through LPT2.
    let rc = rc_of(lanreq::use_add("LPT2", &alloc::format!("\\\\{server}\\{pshare}"), None, None));
    check("use LPT2", rc == 0, &alloc::format!("{rc}"));
    let job = b"%!PS-Adobe-3.0\n%%Title: rs2 LANTEST\n/Helvetica findfont 24 scalefont setfont 72 720 moveto (rs2 LAN printing) show showpage\n%%EOF\n";
    let r = files::write_file("LPT2", job);
    check("print to LPT2", r.is_ok(), &alloc::format!("{r:?}"));
    check("use del LPT2", rc_of(lanreq::use_del("LPT2", false)) == 0, "");

    check("use del", rc_of(lanreq::use_del("X:", false)) == 0, "");
    check("drive gone", !files::exists("X:\\FROMRS2.BIN"), "");
    check("use del again", rc_of(lanreq::use_del("X:", false)) == lanreq::NERR_USE_NOT_FOUND, "");
    // A deviceless connection.
    check("use UNC", rc_of(lanreq::use_add("", &unc, None, None)) == 0, "");
    check("use del UNC", rc_of(lanreq::use_del(&unc, false)) == 0, "");

    let (p, f) = unsafe { (PASSES, FAILS) };
    utils::outln(&alloc::format!("LANTEST: {p} passed, {f} failed"));
    if f == 0 { 0 } else { 1 }
}
