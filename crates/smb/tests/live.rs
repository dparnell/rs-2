//! The client against tools/scripts/smbhost.py over real TCP: each test
//! starts its own server on a free port (skipped when python3 is missing).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use smb::{connect, Credentials, Disposition, Ea, Env, OpenArgs, Options, Session, SetBasic, Transport};

struct Tcp(TcpStream);

impl Transport for Tcp {
    fn send(&mut self, d: &[u8]) -> smb::Result<()> {
        self.0.write_all(d).map_err(|_| smb::Error::Io(32))
    }
    fn recv(&mut self, b: &mut [u8]) -> smb::Result<usize> {
        self.0.read(b).map_err(|_| smb::Error::Io(54))
    }
}

struct HostEnv;

impl Env for HostEnv {
    fn now(&self) -> u64 {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
        smb::FILETIME_1970 + d.as_nanos() as u64 / 100
    }
    fn random(&self, b: &mut [u8]) {
        let mut x = std::time::Instant::now().elapsed().as_nanos() as u64 ^ 0x9e3779b97f4a7c15 ^ (b.as_ptr() as u64);
        for v in b.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = x as u8;
        }
    }
}

static PORT: AtomicU16 = AtomicU16::new(0);

struct Server {
    child: Child,
    port: u16,
    nbport: u16,
    dir: PathBuf,
    spool: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(self.dir.parent().unwrap());
    }
}

fn server(extra: &[&str]) -> Option<Server> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = root.join("tools/scripts/smbhost.py");
    let base = 20000 + (std::process::id() % 2000) as u16 * 10;
    let n = PORT.fetch_add(2, Ordering::SeqCst);
    let (port, nbport) = (base + n, base + n + 1);
    let tmp = std::env::temp_dir().join(format!("smbtest-{}-{}", std::process::id(), n));
    let dir = tmp.join("data");
    let spool = tmp.join("spool");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::create_dir_all(&spool).ok()?;
    std::fs::write(dir.join("hello.txt"), b"Hello from the host\r\n").ok()?;
    std::fs::create_dir_all(dir.join("sub")).ok()?;
    std::fs::write(dir.join("sub/a.dat"), b"a").ok()?;
    let child = Command::new("python3")
        .arg("-I")
        .arg(&script)
        .args(["--port", &port.to_string(), "--nbport", &nbport.to_string(), "--nbns-port", "0"])
        .arg("--share")
        .arg(format!("DATA={}", dir.display()))
        .arg("--printer")
        .arg(format!("LPT={}", spool.display()))
        .args(["--user", "rs2:secret"])
        .args(extra)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some(Server { child, port, nbport, dir, spool });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("smbhost.py did not start on port {port}")
}

fn login(s: &Server, password: &str, o: impl FnOnce(&mut Options)) -> smb::Result<Box<dyn Session>> {
    let mut opt = Options {
        server: "SMBHOST".into(),
        calling: "RS2".into(),
        cred: Credentials { user: "rs2".into(), password: password.into(), domain: "WORKGROUP".into(), workstation: "RS2".into() },
        ..Default::default()
    };
    o(&mut opt);
    let port = if opt.netbios { s.nbport } else { s.port };
    let t = TcpStream::connect(("127.0.0.1", port)).unwrap();
    t.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    connect(Box::new(Tcp(t)), Box::new(HostEnv), &opt)
}

fn exercise(s: &Server, sess: &mut dyn Session) {
    eprintln!("{} on port {}", sess.dialect(), s.port);
    let shares = sess.shares().unwrap();
    assert!(shares.iter().any(|x| x.name == "DATA" && x.typ == 0), "{shares:?}");
    assert!(shares.iter().any(|x| x.name == "LPT" && x.typ == 1), "{shares:?}");
    assert_eq!(sess.tree_connect("NOSUCH").unwrap_err().os2(), smb::status::ERROR_BAD_NET_NAME);
    let t = sess.tree_connect("DATA").unwrap();
    let tid = t.id;

    // Listing and wildcards.
    let all = sess.list(tid, "", "*").unwrap();
    let names: Vec<_> = all.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"hello.txt") && names.contains(&"sub"), "{names:?}");
    assert!(all.iter().find(|e| e.name == "sub").unwrap().attrs.is_dir());
    assert_eq!(sess.list(tid, "", "*.txt").unwrap().len(), 1);
    assert!(sess.list(tid, "", "*.none").unwrap().is_empty());
    assert_eq!(sess.list(tid, "sub", "*").unwrap()[0].name, "a.dat");

    // Read a host file.
    let (f, a, _) = sess.open(tid, "HELLO.TXT", &OpenArgs::read()).unwrap();
    assert_eq!(a.size, 21);
    assert_eq!(sess.read(tid, f, 0, 100).unwrap(), b"Hello from the host\r\n");
    assert!(sess.read(tid, f, 21, 100).unwrap().is_empty());
    sess.close(tid, f).unwrap();

    // Write a large file in max_io pieces and read it back.
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let mut w = OpenArgs::read();
    w.write = true;
    w.disposition = Disposition::OverwriteIf;
    w.directory = Some(false);
    let (f, _, _) = sess.open(tid, "sub\\big.bin", &w).unwrap();
    let step = sess.max_io() as usize;
    let mut off = 0;
    while off < big.len() {
        let n = sess.write(tid, f, off as u64, &big[off..(off + step).min(big.len())]).unwrap() as usize;
        assert!(n > 0);
        off += n;
    }
    sess.flush(tid, f).unwrap();
    assert_eq!(sess.stat(tid, f).unwrap().size, big.len() as u64);
    let mut back = Vec::new();
    loop {
        let d = sess.read(tid, f, back.len() as u64, step as u32).unwrap();
        if d.is_empty() {
            break;
        }
        back.extend_from_slice(&d);
    }
    assert!(back == big);
    sess.set_size(tid, f, 1000).unwrap();
    sess.close(tid, f).unwrap();
    assert_eq!(std::fs::read(s.dir.join("sub/big.bin")).unwrap(), &big[..1000]);

    // Create exclusive fails on an existing file.
    let mut c = w.clone();
    c.disposition = Disposition::Create;
    assert!(sess.open(tid, "hello.txt", &c).is_err());

    // Directories, rename, delete.
    sess.mkdir(tid, "newdir").unwrap();
    assert!(sess.mkdir(tid, "newdir").is_err());
    sess.rename(tid, "sub\\big.bin", "newdir\\moved.bin").unwrap();
    assert!(s.dir.join("newdir/moved.bin").exists());
    assert_eq!(sess.rmdir(tid, "newdir").unwrap_err().os2(), smb::status::ERROR_DIR_NOT_EMPTY);
    sess.delete(tid, "newdir\\moved.bin").unwrap();
    assert_eq!(sess.delete(tid, "newdir\\moved.bin").unwrap_err().os2(), smb::status::ERROR_FILE_NOT_FOUND);
    sess.rmdir(tid, "newdir").unwrap();
    assert!(!s.dir.join("newdir").exists());
    assert_eq!(sess.stat_path(tid, "nodir\\x").unwrap_err().os2(), smb::status::ERROR_PATH_NOT_FOUND);

    // Attributes and times.
    sess.set_basic_path(tid, "hello.txt", &SetBasic { attr: Some(smb::attr::READONLY | smb::attr::HIDDEN), ..Default::default() }).unwrap();
    let a = sess.stat_path(tid, "hello.txt").unwrap();
    assert_eq!(a.attr & 3, 3, "{a:?}");
    sess.set_basic_path(tid, "hello.txt", &SetBasic { attr: Some(0), written: smb::FILETIME_1970 + 10_000_000 * 86400 * 365 * 20, ..Default::default() }).unwrap();
    let a = sess.stat_path(tid, "hello.txt").unwrap();
    assert_eq!(a.attr & 3, 0);
    assert_eq!(a.written / 10_000_000, (smb::FILETIME_1970 + 10_000_000 * 86400 * 365 * 20) / 10_000_000);

    // Extended attributes.
    let ea = Ea { flags: 0, name: b".LONGNAME".to_vec(), value: b"\xfd\xff\x05\x00hello".to_vec() };
    sess.set_eas(tid, "hello.txt", &[ea.clone()]).unwrap();
    assert_eq!(sess.eas(tid, "hello.txt").unwrap(), vec![ea]);
    assert!(sess.stat_path(tid, "hello.txt").unwrap().ea_size > 0);

    let fs = sess.fs_size(tid).unwrap();
    assert!(fs.total_units > 0 && fs.bytes_per_sector > 0);
    sess.echo().unwrap();
    sess.tree_disconnect(tid).unwrap();

    // Printing.
    let p = sess.tree_connect("LPT").unwrap();
    let mut w = OpenArgs::read();
    w.read = false;
    w.write = true;
    w.disposition = Disposition::OverwriteIf;
    let (f, _, _) = sess.open(p.id, "rs2job", &w).unwrap();
    sess.write(p.id, f, 0, b"%!PS\nshowpage\n").unwrap();
    sess.close(p.id, f).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let jobs: Vec<_> = std::fs::read_dir(&s.spool).unwrap().collect();
    assert_eq!(jobs.len(), 1);
    sess.logoff().unwrap();
}

#[test]
fn smb2_session() {
    let Some(s) = server(&[]) else { return };
    let mut sess = login(&s, "secret", |_| {}).unwrap();
    assert_eq!(sess.dialect(), "SMB 2.1");
    assert_eq!(sess.server_domain(), "WORKGROUP");
    exercise(&s, &mut *sess);
}

#[test]
fn smb202_netbios_signed() {
    let Some(s) = server(&["--max-dialect", "0202", "--require-signing"]) else { return };
    let mut sess = login(&s, "secret", |o| o.netbios = true).unwrap();
    assert_eq!(sess.dialect(), "SMB 2.0.2");
    exercise(&s, &mut *sess);
    let mut sess = login(&s, "secret", |o| o.smb2_only = true).unwrap();
    sess.echo().unwrap();
}

#[test]
fn wrong_password() {
    let Some(s) = server(&[]) else { return };
    let e = login(&s, "wrong", |_| {}).err().unwrap();
    assert_eq!(e.os2(), smb::status::ERROR_INVALID_PASSWORD);
}

#[test]
fn smb1_session() {
    let Some(s) = server(&["--smb1"]) else { return };
    let mut sess = login(&s, "secret", |_| {}).unwrap();
    assert_eq!(sess.dialect(), "NT LM 0.12");
    let servers = sess.servers(None, 0xFFFF_FFFF).unwrap();
    assert!(servers.iter().any(|x| x.name == "SMBHOST"), "{servers:?}");
    exercise(&s, &mut *sess);
}

#[test]
fn smb1_signed() {
    let Some(s) = server(&["--smb1", "--require-signing"]) else { return };
    let mut sess = login(&s, "secret", |o| o.netbios = true).unwrap();
    exercise(&s, &mut *sess);
}

#[test]
fn guest() {
    let Some(s) = server(&["--guest"]) else { return };
    let mut sess = login(&s, "", |o| o.cred.user.clear()).unwrap();
    assert!(sess.shares().unwrap().len() >= 2);
}
