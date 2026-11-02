//! NETCLI: checks of rs2's TCP/IP applications (docs/NETWORK.md). Runs
//! FTPD, TFTPD and TELNETD, then FTP, TFTP, LPR, LPQ, LPRM, FINGER, REXEC
//! and RSH with scripted input, against those servers (127.0.0.1) and
//! against tools/scripts/sockhost.py and nethost.py on the host, and
//! compares what arrives. Prints PASS/FAIL lines and a total; then keeps
//! the servers running for `NETCLI [seconds]` (90) so nethost.py's
//! clients can reach FTPD and TELNETD.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
use os2::path::cstr;
use os2::*;
os2::entry!(main);

static mut FAILS: u32 = 0;
static mut PASSES: u32 = 0;

fn check(ok: bool, what: &str) {
    if ok {
        unsafe { PASSES += 1 };
        outln(&alloc::format!("PASS {what}"));
    } else {
        unsafe { FAILS += 1 };
        outln(&alloc::format!("FAIL {what}"));
    }
}

fn write_text(path: &str, s: &str) {
    if let Ok(hf) = create_file(path, false) {
        let _ = write_file(hf, s.as_bytes());
        unsafe { DosClose(hf) };
    }
}

/// Run `prog args` with standard input from `input` (a file) and
/// standard output and error to `output`; `wait` = run to the end, else
/// leave it running. Returns the output when it waited.
fn run(prog: &str, args: &str, input: Option<&str>, output: &str, wait: bool) -> String {
    let inp = input.unwrap_or("NUL");
    let c = cstr(inp);
    let (mut hin, mut act) = (0, 0);
    unsafe { DosOpen(c.as_ptr(), &mut hin, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE | OPEN_FLAGS_NOINHERIT, core::ptr::null_mut()) };
    let hout = create_file(output, false).unwrap_or(0);
    unsafe { DosSetFHState(hout, OPEN_FLAGS_NOINHERIT) };
    let mut saved = [0xffff_ffffu32; 3];
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut argv: Vec<u8> = prog.bytes().collect();
    argv.push(0);
    argv.extend_from_slice(args.as_bytes());
    argv.extend_from_slice(&[0, 0]);
    let p = cstr(prog);
    unsafe {
        for (h, s) in saved.iter_mut().enumerate() {
            DosDupHandle(h as u32, s);
            DosSetFHState(*s, OPEN_FLAGS_NOINHERIT);
        }
        let mut t = 0;
        DosDupHandle(hin, &mut t);
        for h in [1u32, 2] {
            let mut t = h;
            DosDupHandle(hout, &mut t);
        }
        let mut fail = [0u8; 64];
        DosExecPgm(fail.as_mut_ptr(), 64, if wait { 0 } else { 1 }, argv.as_ptr(), core::ptr::null(), &mut res, p.as_ptr());
        for (h, s) in saved.iter().enumerate() {
            let mut t = h as u32;
            DosDupHandle(*s, &mut t);
            DosClose(*s);
        }
        DosClose(hin);
        DosClose(hout);
    }
    if !wait {
        return String::new();
    }
    let out: String = read_file(output).unwrap_or_default().iter().map(|&b| b as char).collect();
    outln(&alloc::format!("---- {prog} {args}"));
    os2::print(&out);
    out
}

fn same(a: &str, b: &str) -> bool {
    match (read_file(a), read_file(b)) {
        (Some(x), Some(y)) => {
            if x != y {
                outln(&alloc::format!("  {a}: {} bytes, {b}: {} bytes", x.len(), y.len()));
            }
            x == y
        }
        _ => {
            outln(&alloc::format!("  {a} or {b} missing"));
            false
        }
    }
}

fn mkdir(p: &str) {
    let c = cstr(p);
    unsafe { DosCreateDir(c.as_ptr(), core::ptr::null_mut()) };
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let linger: u32 = a.first().and_then(|v| v.parse().ok()).unwrap_or(90);
    let d = "C:\\NETT";
    mkdir(d);
    mkdir("C:\\NETT\\TFTP");
    mkdir("C:\\MPTN");
    mkdir("C:\\MPTN\\ETC");
    // Test data: 300,000 pseudo-random bytes and a text file.
    let mut data = Vec::with_capacity(300_000);
    let mut x: u32 = 12345;
    for _ in 0..300_000 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        data.push((x >> 16) as u8);
    }
    if let Ok(hf) = create_file("C:\\NETT\\DATA.BIN", false) {
        let _ = write_file(hf, &data);
        unsafe { DosClose(hf) };
    }
    let mut text = String::new();
    for i in 0..200 {
        text.push_str(&alloc::format!("line {i} of the text file for FTP and TFTP\r\n"));
    }
    write_text("C:\\NETT\\TEXT.TXT", &text);
    write_text(&etc_path("TRUSERS"), "user: rs2 secret\r\nrw: C:\\NETT\r\n\r\nuser: anonymous\r\nr: C:\\NETT\r\n");

    // The servers.
    run("C:\\OS2\\FTPD.EXE", "-p 21", None, "C:\\NETT\\FTPD.LOG", false);
    run("C:\\OS2\\TFTPD.EXE", "C:\\NETT\\TFTP -l", None, "C:\\NETT\\TFTPD.LOG", false);
    run("C:\\OS2\\TELNETD.EXE", "", None, "C:\\NETT\\TELNETD.LOG", false);
    unsafe { DosSleep(1500) };

    // FTP against rs2's FTPD.
    write_text(
        "C:\\NETT\\FTP1.TXT",
        "user rs2 secret\r\nbinary\r\ncd C:/NETT\r\npwd\r\nput C:\\NETT\\DATA.BIN up.bin\r\nget up.bin C:\\NETT\\DOWN.BIN\r\npassive\r\nget up.bin C:\\NETT\\DOWNP.BIN\r\nascii\r\nput C:\\NETT\\TEXT.TXT text2.txt\r\nmkdir sub\r\nrename text2.txt sub/t.txt\r\nls sub\r\ndir\r\nsize up.bin\r\ndelete sub/t.txt\r\nrmdir sub\r\nquit\r\n",
    );
    let o = run("C:\\OS2\\FTP.EXE", "-n 127.0.0.1", Some("C:\\NETT\\FTP1.TXT"), "C:\\NETT\\FTP1.OUT", true);
    check(o.contains("230 User rs2 logged in"), "FTP -> FTPD: login");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\up.bin"), "FTP -> FTPD: put (active, binary)");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\DOWN.BIN"), "FTP -> FTPD: get (active, binary)");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\DOWNP.BIN"), "FTP -> FTPD: get (passive, binary)");
    check(o.contains("257 \"C:/NETT\"") && o.contains("213 300000"), "FTP -> FTPD: pwd, size");
    check(o.contains("t.txt") && o.contains("250 RMD"), "FTP -> FTPD: mkdir, rename, ls, delete, rmdir");
    check(o.to_ascii_uppercase().contains("DOWN.BIN"), "FTP -> FTPD: dir");
    write_text("C:\\NETT\\FTP2.TXT", "user anonymous me@rs2\r\nput C:\\NETT\\TEXT.TXT nope.txt\r\nuser rs2 wrong\r\nquit\r\n");
    let o = run("C:\\OS2\\FTP.EXE", "-n 127.0.0.1 21", Some("C:\\NETT\\FTP2.TXT"), "C:\\NETT\\FTP2.OUT", true);
    check(o.contains("230 User anonymous") && o.contains("550 Permission denied") && o.contains("530 Login incorrect"), "FTP -> FTPD: anonymous is read-only, wrong password refused");

    // FTP against the host's server (sockhost.py, 10.0.2.2:2121).
    write_text(
        "C:\\NETT\\FTP3.TXT",
        "user rs2 x\r\nbinary\r\nput C:\\NETT\\DATA.BIN fromrs2.bin\r\nget fromrs2.bin C:\\NETT\\HDOWN.BIN\r\npassive\r\nget fromrs2.bin C:\\NETT\\HDOWNP.BIN\r\nls\r\nquit\r\n",
    );
    let o = run("C:\\OS2\\FTP.EXE", "-n 10.0.2.2 2121", Some("C:\\NETT\\FTP3.TXT"), "C:\\NETT\\FTP3.OUT", true);
    check(o.contains("230"), "FTP -> host: login");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\HDOWN.BIN"), "FTP -> host: put and get (active)");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\HDOWNP.BIN"), "FTP -> host: get (passive)");

    // TFTP against rs2's TFTPD.
    write_text(
        "C:\\NETT\\TFTP1.TXT",
        "binary\r\nput C:\\NETT\\DATA.BIN t.bin\r\nget t.bin C:\\NETT\\TDOWN.BIN\r\nblksize 1428\r\nget t.bin C:\\NETT\\TDOWN2.BIN\r\nascii\r\nput C:\\NETT\\TEXT.TXT t.txt\r\nget t.txt C:\\NETT\\TDOWN.TXT\r\nget nosuch.bin C:\\NETT\\X.BIN\r\nstatus\r\nquit\r\n",
    );
    let o = run("C:\\OS2\\TFTP.EXE", "127.0.0.1", Some("C:\\NETT\\TFTP1.TXT"), "C:\\NETT\\TFTP1.OUT", true);
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\TFTP\\t.bin"), "TFTP -> TFTPD: put (octet)");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\TDOWN.BIN"), "TFTP -> TFTPD: get (octet, 512-byte blocks)");
    check(same("C:\\NETT\\DATA.BIN", "C:\\NETT\\TDOWN2.BIN"), "TFTP -> TFTPD: get (blksize 1428)");
    check(same("C:\\NETT\\TEXT.TXT", "C:\\NETT\\TDOWN.TXT"), "TFTP -> TFTPD: put and get (netascii)");
    check(o.contains("Error code 1"), "TFTP -> TFTPD: missing file reported");

    // LPR, LPQ, LPRM against the host's LPD (10.0.2.100:515).
    let o = run("C:\\OS2\\LPR.EXE", "-p rawq -s 10.0.2.100 C:\\NETT\\TEXT.TXT", None, "C:\\NETT\\LPR.OUT", true);
    check(o.contains("sent to rawq"), "LPR -> host LPD");
    let o = run("C:\\OS2\\LPQ.EXE", "-p rawq -s 10.0.2.100 -l", None, "C:\\NETT\\LPQ.OUT", true);
    check(o.contains("Printer: rawq") && o.contains("dfA"), "LPQ -> host LPD");
    let o = run("C:\\OS2\\LPRM.EXE", "-prawq -s10.0.2.100 -a", None, "C:\\NETT\\LPRM.OUT", true);
    check(o.contains("dequeued"), "LPRM -> host LPD");
    let o = run("C:\\OS2\\LPQ.EXE", "rawq@10.0.2.100", None, "C:\\NETT\\LPQ2.OUT", true);
    check(o.contains("no entries"), "LPQ printer@server: queue empty after LPRM");

    // FINGER, REXEC, RSH against the host (10.0.2.100).
    let o = run("C:\\OS2\\FINGER.EXE", "rs2@10.0.2.100", None, "C:\\NETT\\FINGER.OUT", true);
    check(o.contains("rs2 finger test: rs2"), "FINGER -> host");
    let o = run("C:\\OS2\\REXEC.EXE", "10.0.2.100 -l rs2 -p secret -n dir C:\\", None, "C:\\NETT\\REXEC.OUT", true);
    check(o.contains("rexec ran: dir C:\\"), "REXEC -> host");
    let o = run("C:\\OS2\\REXEC.EXE", "10.0.2.100 -l rs2 -p wrong -n dir", None, "C:\\NETT\\REXEC2.OUT", true);
    check(o.contains("Login incorrect"), "REXEC -> host: wrong password refused");
    let o = run("C:\\OS2\\RSH.EXE", "10.0.2.100 -l bob -n ver", None, "C:\\NETT\\RSH.OUT", true);
    check(o.contains("rsh ran: ver as bob"), "RSH -> host");

    // TCPCFG2's settings (the command-line form; the notebook saves the same way).
    let o = run("C:\\OS2\\TCPCFG2.EXE", "NETWORK=10.0.2.15/24,10.0.2.2,10.0.2.3 HOSTNAME=rs2box DOMAIN=example.org", None, "C:\\NETT\\TCPCFG.OUT", true);
    let cfg: String = read_file("C:\\CONFIG.SYS").unwrap_or_default().iter().map(|&b| b as char).collect();
    let res: String = read_file(&etc_path("RESOLV2")).unwrap_or_default().iter().map(|&b| b as char).collect();
    check(
        o.contains("saved") && cfg.contains("NETWORK=10.0.2.15/24,10.0.2.2,10.0.2.3\r\n") && cfg.contains("SET HOSTNAME=rs2box\r\n") && res.contains("domain example.org") && res.contains("nameserver 10.0.2.3"),
        "TCPCFG2: NETWORK=, SET HOSTNAME= and RESOLV2 written",
    );
    let o = run("C:\\OS2\\TCPCFG2.EXE", "/QUERY", None, "C:\\NETT\\TCPCFG2.OUT", true);
    check(o.contains("NETWORK=10.0.2.15/24,10.0.2.2,10.0.2.3") && o.contains("HOSTNAME=rs2box") && o.contains("now: lan0: 10.0.2.15"), "TCPCFG2 /QUERY: settings and the interface");
    let o = run("C:\\OS2\\TCPCFG2.EXE", "NETWORK=10.0.2.300/24", None, "C:\\NETT\\TCPCFG3.OUT", true);
    check(o.contains("not valid"), "TCPCFG2: a bad address is refused");
    run("C:\\OS2\\TCPCFG2.EXE", "NETWORK=dhcp HOSTNAME= DOMAIN= NAMESERVERS=", None, "C:\\NETT\\TCPCFG4.OUT", true);

    let (p, f) = unsafe { (PASSES, FAILS) };
    outln(&alloc::format!("NETCLI: {p} passed, {f} failed"));
    outln(&alloc::format!("NETCLI: servers stay up for {linger} seconds"));
    unsafe { DosSleep(linger * 1000) };
    outln("NETCLI: done");
    let log = read_file("C:\\NETT\\FTPD.LOG").unwrap_or_default();
    os2::write(&log);
    let log = read_file("C:\\NETT\\TELNETD.LOG").unwrap_or_default();
    os2::write(&log);
    f
}
