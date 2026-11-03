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

unsafe extern "C" {
    #[link_name = "PMSPL.393"]
    fn SplCreateQueue(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32;
    #[link_name = "TCPIP32.145"]
    fn rexec(host: *mut *mut u8, port: i32, user: *const u8, pass: *const u8, cmd: *const u8, fd2: *mut i32) -> i32;
    #[link_name = "PMSPL.392"]
    fn SplEnumJob(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
}

/// A spooler queue with no printer: its jobs stay queued.
fn make_queue(name: &[u8]) {
    let mut q = Vec::new();
    q.extend_from_slice(&(name.as_ptr() as u32).to_le_bytes());
    for v in [5u16, 0, 0, 0] {
        q.extend_from_slice(&v.to_le_bytes());
    }
    for p in [0u32, b"PMPRINT\0".as_ptr() as u32, 0, b"LPD test queue\0".as_ptr() as u32] {
        q.extend_from_slice(&p.to_le_bytes());
    }
    q.extend_from_slice(&[0, 0, 0, 0]);
    for p in [0u32, 0, 0] {
        q.extend_from_slice(&p.to_le_bytes());
    }
    let rc = unsafe { SplCreateQueue(core::ptr::null(), 3, q.as_ptr(), q.len() as u32) };
    if rc != 0 && rc != 2154 {
        outln(&alloc::format!("NETCLI: SplCreateQueue: error {rc}"));
    }
}

/// (job count, documents) of a queue.
fn queue_jobs(name: &[u8]) -> (u32, String) {
    let mut buf = alloc::vec![0u8; 8192];
    let (mut ret, mut total, mut need) = (0, 0, 0);
    unsafe { SplEnumJob(core::ptr::null(), name.as_ptr(), 2, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) };
    let mut docs = String::new();
    for i in 0..ret as usize {
        let p = u32::from_le_bytes([buf[i * 28 + 24], buf[i * 28 + 25], buf[i * 28 + 26], buf[i * 28 + 27]]);
        if p != 0 {
            let c = unsafe { core::ffi::CStr::from_ptr(p as *const core::ffi::c_char) };
            docs.extend(c.to_bytes().iter().map(|&b| b as char));
            docs.push(';');
        }
    }
    (ret, docs)
}

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
    run_env(prog, args, input, output, wait, &[])
}

/// `run` with more environment variables.
fn run_env(prog: &str, args: &str, input: Option<&str>, output: &str, wait: bool, extra: &[&str]) -> String {
    let mut envb: Vec<u8> = Vec::new();
    for e in netapps::rcmd::environment() {
        let k = e.split('=').next().unwrap_or("");
        if extra.iter().any(|x| x.split('=').next().is_some_and(|xk| xk.eq_ignore_ascii_case(k))) {
            continue;
        }
        envb.extend_from_slice(e.as_bytes());
        envb.push(0);
    }
    for e in extra {
        envb.extend_from_slice(e.as_bytes());
        envb.push(0);
    }
    envb.push(0);
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
        DosExecPgm(fail.as_mut_ptr(), 64, if wait { 0 } else { 1 }, argv.as_ptr(), envb.as_ptr(), &mut res, p.as_ptr());
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
    make_queue(b"LPDQ\0");
    write_text(&etc_path("RHOSTS"), "127.0.0.1\r\n10.0.2.2 hostuser\r\n");
    run("C:\\OS2\\REXECD.EXE", "", None, "C:\\NETT\\REXECD.LOG", false);
    run("C:\\OS2\\RSHD.EXE", "", None, "C:\\NETT\\RSHD.LOG", false);
    run("C:\\OS2\\TALKD.EXE", "", None, "C:\\NETT\\TALKD.LOG", false);
    run("C:\\OS2\\LPD.EXE", "-b", None, "C:\\NETT\\LPD.LOG", false);
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

    // LPR, LPQ, LPRM against rs2's LPD (127.0.0.1): jobs land in the
    // spooler queue LPDQ.
    let o = run("C:\\OS2\\LPR.EXE", "-p lpdq -s 127.0.0.1 -J lprjob C:\\NETT\\TEXT.TXT", None, "C:\\NETT\\LPR2.OUT", true);
    unsafe { DosSleep(500) };
    let (n, docs) = queue_jobs(b"LPDQ\0");
    check(o.contains("sent to lpdq") && n >= 1 && docs.contains("lprjob"), "LPR -> LPD: the job is in the spooler queue");
    let o = run("C:\\OS2\\LPQ.EXE", "-p lpdq -s 127.0.0.1", None, "C:\\NETT\\LPQ3.OUT", true);
    check(o.contains("lprjob") && o.to_ascii_lowercase().contains("lpdq"), "LPQ -> LPD: the queue's jobs");
    let o = run("C:\\OS2\\LPRM.EXE", "-p lpdq -s 127.0.0.1 -a", None, "C:\\NETT\\LPRM2.OUT", true);
    check(o.contains("dequeued") && !queue_jobs(b"LPDQ\0").1.contains("lprjob"), "LPRM -> LPD: the job is removed");

    // REXEC, RSH and rexec() against rs2's REXECD and RSHD.
    let o = run("C:\\OS2\\REXEC.EXE", "127.0.0.1 -l rs2 -p secret -n echo rexecd works", None, "C:\\NETT\\REXEC3.OUT", true);
    check(o.contains("rexecd works"), "REXEC -> REXECD: the command's output");
    let o = run("C:\\OS2\\REXEC.EXE", "127.0.0.1 -l rs2 -p wrong -n echo no", None, "C:\\NETT\\REXEC4.OUT", true);
    check(o.contains("Login incorrect") && !o.contains("echo no"), "REXEC -> REXECD: a wrong password is refused");
    write_text("C:\\NETT\\SORTIN.TXT", "pear\r\napple\r\nfig\r\n");
    let o = run("C:\\OS2\\RSH.EXE", "127.0.0.1 -l bob sort", Some("C:\\NETT\\SORTIN.TXT"), "C:\\NETT\\RSH2.OUT", true);
    check(o.contains("apple\r\nfig\r\npear"), "RSH -> RSHD: standard input to the command (sort)");
    let o = run("C:\\OS2\\RSH.EXE", "127.0.0.1 -l bob -n set user", None, "C:\\NETT\\RSH3.OUT", true);
    check(o.to_ascii_uppercase().contains("USER=BOB"), "RSH -> RSHD: USER is the local user");
    {
        let mut host = *b"127.0.0.1\0";
        let mut hp = host.as_mut_ptr();
        let mut fd2 = -1i32;
        let s = unsafe { rexec(&mut hp, 512u16.to_be() as i32, b"rs2\0".as_ptr(), b"secret\0".as_ptr(), b"echo via rexec call & echo to stderr 1>&2\0".as_ptr(), &mut fd2) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        if s >= 0 {
            let so = Sock(s as u32);
            let mut b = [0u8; 512];
            while let Ok(n) = so.recv(&mut b) {
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&b[..n]);
            }
            if fd2 >= 0 {
                let se = Sock(fd2 as u32);
                while let Ok(n) = se.recv(&mut b) {
                    if n == 0 {
                        break;
                    }
                    err.extend_from_slice(&b[..n]);
                }
            }
        }
        let o: String = out.iter().map(|&b| b as char).collect();
        let e: String = err.iter().map(|&b| b as char).collect();
        outln(&alloc::format!("---- rexec(): [{}] [{}]", o.trim(), e.trim()));
        check(o.contains("via rexec call") && e.contains("to stderr"), "rexec() (TCPIP32) -> REXECD: output and standard error");
    }

    // TALK between two users through TALKD.
    write_text("C:\\NETT\\TALKA.TXT", "hello bob\r\n");
    write_text("C:\\NETT\\TALKB.TXT", "hi alice\r\n");
    run_env("C:\\OS2\\TALK.EXE", "bob@127.0.0.1", Some("C:\\NETT\\TALKA.TXT"), "C:\\NETT\\TALKA.OUT", false, &["USER=alice"]);
    unsafe { DosSleep(1500) };
    let ob = run_env("C:\\OS2\\TALK.EXE", "alice@127.0.0.1", Some("C:\\NETT\\TALKB.TXT"), "C:\\NETT\\TALKB.OUT", true, &["USER=bob"]);
    let mut oa = String::new();
    for _ in 0..40 {
        oa = read_file("C:\\NETT\\TALKA.OUT").unwrap_or_default().iter().map(|&b| b as char).collect();
        if oa.contains("Exiting") {
            break;
        }
        unsafe { DosSleep(500) };
    }
    outln(&alloc::format!("---- TALK alice:\r\n{oa}"));
    check(ob.contains("hello bob") && oa.contains("hi alice"), "TALK <-> TALK through TALKD: both sides see the other's text");

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
    for l in ["LPD", "REXECD", "RSHD", "TALKD"] {
        let log = read_file(&alloc::format!("C:\\NETT\\{l}.LOG")).unwrap_or_default();
        os2::write(&log);
    }
    f
}
