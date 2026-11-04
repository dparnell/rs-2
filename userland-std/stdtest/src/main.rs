//! STDTEST: Rust's standard library on rs2 (rs2libc). Runs the sections
//! named on the command line (default: all but `host`):
//!
//! - `threads`: Mutex/Condvar/RwLock/Barrier/channels/scoped threads,
//!   thread-locals and their destructors, park/unpark, sleeping
//! - `fs`: files and directories on C: (write, read, append, seek, set_len,
//!   metadata, read_dir, rename, copy, remove_dir_all, canonicalize, errors)
//! - `time`, `env` (arguments, variables, current_exe/current_dir)
//! - `panic`: catch_unwind, drops during unwinding, a panicking thread
//! - `mem`: a large HashMap and Vec, then the address space limit (how many
//!   MB can be allocated in 16 MB pieces; more than 512 with high memory)
//! - `net`: loopback TCP (1 MB echoed), UDP, a refused connection, timeouts,
//!   name lookup
//! - `process`: Command with piped output (CMD /C ECHO)
//! - `host HOST:PORT`: an HTTP GET from a host server (tools/scripts:
//!   `python3 -m http.server`)
//!
//! Prints `PASS name` / `FAIL name: why` per check and a summary.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime};

use rs2libc as _;

struct T {
    pass: usize,
    fail: usize,
}

impl T {
    fn check(&mut self, name: &str, ok: bool, why: impl std::fmt::Display) {
        if ok {
            self.pass += 1;
            println!("PASS {name}");
        } else {
            self.fail += 1;
            println!("FAIL {name}: {why}");
        }
    }
    fn ok<E: std::fmt::Debug>(&mut self, name: &str, r: Result<bool, E>) {
        match r {
            Ok(b) => self.check(name, b, "wrong result"),
            Err(e) => self.check(name, false, format!("{e:?}")),
        }
    }
}

fn threads(t: &mut T) {
    // Mutex under contention.
    let counter = Arc::new(Mutex::new(0u32));
    let hs: Vec<_> = (0..8)
        .map(|_| {
            let c = counter.clone();
            std::thread::spawn(move || {
                for _ in 0..10000 {
                    *c.lock().unwrap() += 1;
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let n = *counter.lock().unwrap();
    t.check("mutex 8 threads x 10000", n == 80000, n);

    // Condvar ping-pong.
    let pair = Arc::new((Mutex::new(0u32), Condvar::new()));
    let p2 = pair.clone();
    let h = std::thread::spawn(move || {
        let (m, c) = &*p2;
        for i in 0..1000 {
            let mut v = m.lock().unwrap();
            while *v != 2 * i + 1 {
                v = c.wait(v).unwrap();
            }
            *v += 1;
            c.notify_all();
        }
    });
    {
        let (m, c) = &*pair;
        for i in 0..1000 {
            let mut v = m.lock().unwrap();
            while *v != 2 * i {
                v = c.wait(v).unwrap();
            }
            *v += 1;
            c.notify_all();
        }
    }
    h.join().unwrap();
    t.check("condvar ping-pong 1000", *pair.0.lock().unwrap() == 2000, "");

    // Condvar timeout.
    let start = Instant::now();
    let (m, c) = (Mutex::new(()), Condvar::new());
    let (_g, r) = c.wait_timeout(m.lock().unwrap(), Duration::from_millis(200)).unwrap();
    let el = start.elapsed();
    t.check("condvar wait_timeout 200 ms", r.timed_out() && el >= Duration::from_millis(190) && el < Duration::from_secs(2), format!("{el:?}"));

    // RwLock: readers together, then a writer.
    let rw = Arc::new(RwLock::new(5));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let rw = rw.clone();
            std::thread::spawn(move || *rw.read().unwrap())
        })
        .collect();
    let sum: i32 = readers.into_iter().map(|h| h.join().unwrap()).sum();
    *rw.write().unwrap() += 1;
    t.check("rwlock", sum == 20 && *rw.read().unwrap() == 6, sum);

    // Channels.
    let (tx, rx) = mpsc::channel();
    for k in 0..4 {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for i in 0..2500u64 {
                tx.send(k * 10000 + i).unwrap();
            }
        });
    }
    drop(tx);
    let got: Vec<u64> = rx.iter().collect();
    t.check("mpsc 4 senders x 2500", got.len() == 10000, got.len());
    let (stx, srx) = mpsc::sync_channel::<u32>(1);
    let h = std::thread::spawn(move || (0..100).map(|_| srx.recv().unwrap()).sum::<u32>());
    for i in 0..100 {
        stx.send(i).unwrap();
    }
    t.check("sync_channel(1)", h.join().unwrap() == 4950, "");

    // Barrier.
    let b = Arc::new(Barrier::new(5));
    let hits = Arc::new(AtomicUsize::new(0));
    let hs: Vec<_> = (0..5)
        .map(|_| {
            let (b, hits) = (b.clone(), hits.clone());
            std::thread::spawn(move || {
                hits.fetch_add(1, Ordering::SeqCst);
                b.wait();
                hits.load(Ordering::SeqCst)
            })
        })
        .collect();
    let all = hs.into_iter().all(|h| h.join().unwrap() == 5);
    t.check("barrier 5", all, "");

    // Scoped threads borrowing the stack.
    let mut data = vec![1u64; 1000];
    std::thread::scope(|s| {
        for chunk in data.chunks_mut(250) {
            s.spawn(move || chunk.iter_mut().for_each(|x| *x *= 3));
        }
    });
    t.check("thread::scope", data.iter().sum::<u64>() == 3000, "");

    // Thread-locals and their destructors.
    struct D(Arc<AtomicUsize>);
    impl Drop for D {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    thread_local! {
        static TL: std::cell::RefCell<Option<D>> = const { std::cell::RefCell::new(None) };
        static N: std::cell::Cell<u32> = const { std::cell::Cell::new(7) };
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let hs: Vec<_> = (0..4)
        .map(|i| {
            let d = drops.clone();
            std::thread::spawn(move || {
                N.with(|n| n.set(n.get() + i));
                TL.with(|t| *t.borrow_mut() = Some(D(d)));
                N.with(|n| n.get())
            })
        })
        .collect();
    let vals: Vec<u32> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    t.check("thread_local values", vals == [7, 8, 9, 10] && N.with(|n| n.get()) == 7, format!("{vals:?}"));
    t.check("thread_local destructors", drops.load(Ordering::SeqCst) == 4, drops.load(Ordering::SeqCst));

    // park/unpark.
    let flag = Arc::new(AtomicUsize::new(0));
    let f2 = flag.clone();
    let h = std::thread::spawn(move || {
        while f2.load(Ordering::SeqCst) == 0 {
            std::thread::park();
        }
    });
    std::thread::sleep(Duration::from_millis(50));
    flag.store(1, Ordering::SeqCst);
    h.thread().unpark();
    h.join().unwrap();
    t.check("park/unpark", true, "");

    let name = std::thread::Builder::new().name("worker".into()).stack_size(256 << 10).spawn(|| {
        std::thread::current().name().map(String::from)
    });
    let ok = name.map(|h| h.join().unwrap());
    t.check("Builder name+stack", matches!(&ok, Ok(Some(n)) if n == "worker"), format!("{ok:?}"));

    // Many short-lived threads (TLS selectors and blocks are recycled).
    let mut ok = true;
    for _ in 0..200 {
        ok &= std::thread::spawn(|| 1).join().unwrap() == 1;
    }
    t.check("200 threads in turn", ok, "");

    let start = Instant::now();
    std::thread::sleep(Duration::from_millis(100));
    let el = start.elapsed();
    t.check("sleep 100 ms", el >= Duration::from_millis(100) && el < Duration::from_millis(1000), format!("{el:?}"));
    let p = std::thread::available_parallelism();
    t.check("available_parallelism", p.is_ok(), format!("{p:?}"));
    println!("     ({} CPUs)", p.map(|n| n.get()).unwrap_or(0));
}

fn fs(t: &mut T, root: &str) {
    let dir = format!("{root}/STDTEST.TMP");
    let _ = std::fs::remove_dir_all(&dir);
    t.ok("create_dir_all", std::fs::create_dir_all(format!("{dir}/SUB/DEEP")).map(|_| true));
    let file = format!("{dir}/DATA.BIN");
    let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + i / 256) as u8).collect();
    t.ok("write 100 KB", std::fs::write(&file, &data).map(|_| true));
    t.ok("read back", std::fs::read(&file).map(|r| r == data));
    t.ok("metadata len", std::fs::metadata(&file).map(|m| m.len() == 100_000 && m.is_file()));
    t.ok("metadata dir", std::fs::metadata(&dir).map(|m| m.is_dir()));
    t.ok(
        "modified time",
        std::fs::metadata(&file).and_then(|m| m.modified()).map(|m| {
            let age = SystemTime::now().duration_since(m).unwrap_or_default();
            age < Duration::from_secs(120)
        }),
    );
    let r = (|| -> std::io::Result<bool> {
        let mut f = std::fs::OpenOptions::new().append(true).open(&file)?;
        f.write_all(b"tail")?;
        drop(f);
        let mut f = std::fs::File::open(&file)?;
        f.seek(SeekFrom::End(-4))?;
        let mut b = [0; 4];
        f.read_exact(&mut b)?;
        Ok(&b == b"tail")
    })();
    t.ok("append + seek", r);
    let r = (|| -> std::io::Result<bool> {
        let f = std::fs::OpenOptions::new().write(true).open(&file)?;
        f.set_len(10)?;
        drop(f);
        Ok(std::fs::metadata(&file)?.len() == 10)
    })();
    t.ok("set_len", r);
    let r = (|| -> std::io::Result<bool> {
        let mut f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(format!("{dir}/RW.TXT"))?;
        write!(f, "line one\nline two\n")?;
        f.seek(SeekFrom::Start(0))?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        Ok(s == "line one\nline two\n")
    })();
    t.ok("read+write file", r);
    t.ok("copy", std::fs::copy(&file, format!("{dir}/COPY.BIN")).map(|n| n == 10));
    t.ok("rename", std::fs::rename(format!("{dir}/COPY.BIN"), format!("{dir}/SUB/MOVED.BIN")).map(|_| true));
    t.ok("rename over existing", std::fs::rename(format!("{dir}/RW.TXT"), format!("{dir}/DATA.BIN")).map(|_| true));
    let names: Result<Vec<String>, _> =
        std::fs::read_dir(&dir).and_then(|d| d.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned())).collect());
    let mut names = names.unwrap_or_default();
    names.sort();
    t.check("read_dir", names == ["DATA.BIN", "SUB"], format!("{names:?}"));
    let types: Vec<bool> = std::fs::read_dir(format!("{dir}/SUB"))
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).collect())
        .unwrap_or_default();
    t.check("file_type", types.len() == 2 && types.iter().filter(|&&d| d).count() == 1, format!("{types:?}"));
    let e = std::fs::File::open(format!("{dir}/MISSING.TXT"));
    t.check("NotFound", matches!(&e, Err(e) if e.kind() == std::io::ErrorKind::NotFound), format!("{e:?}"));
    let e = std::fs::create_dir(&dir);
    t.check("AlreadyExists", matches!(&e, Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists), format!("{e:?}"));
    let e = std::fs::remove_dir(&dir);
    t.check("remove_dir not empty", e.is_err(), format!("{e:?}"));
    let c = std::fs::canonicalize(format!("{dir}/SUB/../DATA.BIN"));
    t.check(
        "canonicalize",
        matches!(&c, Ok(p) if p.to_string_lossy().eq_ignore_ascii_case(&format!("{}\\STDTEST.TMP\\DATA.BIN", root.trim_end_matches('/')))),
        format!("{c:?}"),
    );
    let before = std::env::current_dir();
    let r = std::env::set_current_dir(&dir).and_then(|_| std::fs::read_to_string("DATA.BIN"));
    t.check("set_current_dir + relative path", matches!(&r, Ok(s) if s.starts_with("line one")), format!("{r:?}"));
    if let Ok(b) = before {
        let _ = std::env::set_current_dir(b);
    }
    t.ok("remove_dir_all", std::fs::remove_dir_all(&dir).map(|_| true));
    t.check("gone", std::fs::metadata(&dir).is_err(), "");
}

fn time(t: &mut T) {
    let a = Instant::now();
    let mut x = 0u64;
    for i in 0..1_000_000 {
        x = x.wrapping_add(i * i);
    }
    let b = Instant::now();
    t.check("Instant monotonic", b >= a, format!("{x}"));
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH);
    t.check("SystemTime after 2024", matches!(now, Ok(d) if d.as_secs() > 1_704_067_200), format!("{now:?}"));
    println!("     (unix time {:?})", now.map(|d| d.as_secs()));
}

fn env(t: &mut T, args: &[String]) {
    t.check("args[0]", args.first().is_some_and(|a| a.to_ascii_uppercase().contains("STDTEST")), format!("{args:?}"));
    unsafe { std::env::set_var("STDTEST_VAR", "value 1") };
    t.check("set_var/var", std::env::var("STDTEST_VAR").as_deref() == Ok("value 1"), "");
    t.check("vars has it", std::env::vars().any(|(k, v)| k == "STDTEST_VAR" && v == "value 1"), "");
    unsafe { std::env::remove_var("STDTEST_VAR") };
    t.check("remove_var", std::env::var("STDTEST_VAR").is_err(), "");
    t.check("PATH", std::env::var("PATH").is_ok(), "");
    let exe = std::env::current_exe();
    t.check("current_exe", matches!(&exe, Ok(p) if p.to_string_lossy().to_ascii_uppercase().ends_with("STDTEST.EXE")), format!("{exe:?}"));
    let cwd = std::env::current_dir();
    t.check("current_dir", matches!(&cwd, Ok(p) if p.to_string_lossy().len() >= 3), format!("{cwd:?}"));
}

fn panics(t: &mut T) {
    std::panic::set_hook(Box::new(|_| {}));
    let n = std::hint::black_box(42);
    let r = std::panic::catch_unwind(|| panic!("boom {n}"));
    let msg = r.as_ref().err().and_then(|e| e.downcast_ref::<String>().cloned().or(e.downcast_ref::<&str>().map(|s| s.to_string())));
    t.check("catch_unwind", msg.as_deref() == Some("boom 42"), format!("{msg:?}"));
    struct Flag<'a>(&'a AtomicUsize);
    impl Drop for Flag<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let n = AtomicUsize::new(0);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _a = Flag(&n);
        let _b = Flag(&n);
        let v: Vec<u32> = Vec::new();
        v[3]
    }));
    t.check("drops while unwinding", r.is_err() && n.load(Ordering::SeqCst) == 2, n.load(Ordering::SeqCst));
    let h = std::thread::spawn(|| -> u32 { panic!("in a thread") });
    t.check("panicking thread joins Err", h.join().is_err(), "");
    let r = std::panic::catch_unwind(|| {
        let r = std::panic::catch_unwind(|| panic!("inner"));
        assert!(r.is_err());
        7
    });
    t.check("nested catch_unwind", r.ok() == Some(7), "");
    let _ = std::panic::take_hook();
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.348"]
    fn DosQuerySysInfo(first: u32, last: u32, buf: *mut u32, cb: u32) -> u32;
}

/// QSV_VIRTUALADDRESSLIMIT (MB), 512 if unknown.
fn virtual_address_limit() -> u32 {
    let mut v = 0u32;
    match unsafe { DosQuerySysInfo(30, 30, &mut v, 4) } {
        0 => v,
        _ => 512,
    }
}

fn mem(t: &mut T) {
    let mut m = HashMap::new();
    for i in 0..1_000_000u32 {
        m.insert(i, i.wrapping_mul(2654435761));
    }
    let ok = (0..1_000_000u32).step_by(997).all(|i| m[&i] == i.wrapping_mul(2654435761));
    t.check("HashMap 1M entries", ok && m.len() == 1_000_000, m.len());
    drop(m);
    let mut v: Vec<u8> = Vec::new();
    let r = v.try_reserve_exact(100 << 20);
    if r.is_ok() {
        v.resize(100 << 20, 0xa5);
        v[(100 << 20) - 1] = 1;
    }
    t.check("Vec 100 MB", r.is_ok() && v[12345] == 0xa5 && v[(100 << 20) - 1] == 1, format!("{r:?}"));
    drop(v);
    // How much address space can be had in 16 MB pieces?
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    loop {
        let mut b = Vec::new();
        if b.try_reserve_exact(16 << 20).is_err() || blocks.len() >= 256 {
            break;
        }
        b.push(1);
        blocks.push(b);
    }
    let mb = blocks.len() * 16;
    let high = blocks.iter().filter(|b| b.as_ptr() as usize >= 0x2000_0000).count() * 16;
    println!("     (allocated {mb} MB in 16 MB blocks before failing, {high} MB of them above 512 MB)");
    t.check("at least 256 MB of address space", mb >= 256, mb);
    // With high memory (VIRTUALADDRESSLIMIT above 512) the allocator gets
    // more than the low arena's 512 MB can hold.
    let limit = virtual_address_limit();
    if limit > 512 {
        t.check("more than 512 MB with high memory", mb > 512 && high > 0, format!("{mb} MB, limit {limit}"));
    }
    drop(blocks);
    let mut v: Vec<u8> = Vec::new();
    let r = v.try_reserve_exact(64 << 20);
    t.check("memory reusable after free", r.is_ok(), format!("{r:?}"));
}

fn net(t: &mut T) {
    use std::net::{TcpListener, TcpStream, UdpSocket};
    let l = TcpListener::bind("127.0.0.1:0");
    t.check("TcpListener::bind", l.is_ok(), format!("{l:?}"));
    let Ok(l) = l else { return };
    let addr = l.local_addr().unwrap();
    t.check("local_addr port", addr.port() != 0, addr);
    let server = std::thread::spawn(move || -> std::io::Result<usize> {
        let (mut s, _) = l.accept()?;
        let mut buf = vec![0; 65536];
        let mut total = 0;
        loop {
            let n = s.read(&mut buf)?;
            if n == 0 {
                break;
            }
            s.write_all(&buf[..n])?;
            total += n;
        }
        Ok(total)
    });
    let r = (|| -> std::io::Result<bool> {
        let mut c = TcpStream::connect(addr)?;
        c.set_nodelay(true)?;
        let data: Vec<u8> = (0..1 << 20).map(|i: u32| (i % 251) as u8).collect();
        let reader = {
            let mut r = c.try_clone()?;
            std::thread::spawn(move || {
                let mut got = Vec::new();
                r.read_to_end(&mut got).map(|_| got)
            })
        };
        c.write_all(&data)?;
        c.shutdown(std::net::Shutdown::Write)?;
        let got = reader.join().unwrap()?;
        Ok(got == data)
    })();
    t.ok("TCP loopback 1 MB echo", r);
    let s = server.join().unwrap();
    t.check("server saw 1 MB", matches!(s, Ok(n) if n >= 1 << 20), format!("{s:?}"));

    let e = TcpStream::connect("127.0.0.1:1");
    t.check("connection refused", matches!(&e, Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused), format!("{e:?}"));

    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    let c = TcpStream::connect_timeout(&a, Duration::from_secs(2));
    t.check("connect_timeout", c.is_ok(), format!("{c:?}"));
    let (s, _) = l.accept().unwrap();
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let start = Instant::now();
    let r = (&s).read(&mut [0; 4]);
    let el = start.elapsed();
    t.check(
        "read timeout",
        matches!(&r, Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)) && el >= Duration::from_millis(150),
        format!("{r:?} after {el:?}"),
    );
    s.set_nonblocking(true).unwrap();
    let r = (&s).read(&mut [0; 4]);
    t.check("nonblocking read", matches!(&r, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock), format!("{r:?}"));
    l.set_nonblocking(true).unwrap();
    let r = l.accept();
    t.check("nonblocking accept", matches!(&r, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock), format!("{r:?}"));

    let r = (|| -> std::io::Result<bool> {
        let a = UdpSocket::bind("127.0.0.1:0")?;
        let b = UdpSocket::bind("127.0.0.1:0")?;
        a.send_to(b"datagram", b.local_addr()?)?;
        let mut buf = [0; 64];
        let (n, from) = b.recv_from(&mut buf)?;
        Ok(&buf[..n] == b"datagram" && from == a.local_addr()?)
    })();
    t.ok("UDP loopback", r);
    use std::net::ToSocketAddrs;
    let r = "localhost:80".to_socket_addrs().map(|a| a.collect::<Vec<_>>());
    t.check("resolve localhost", matches!(&r, Ok(v) if v.iter().any(|a| a.ip().is_loopback())), format!("{r:?}"));
}

fn host(t: &mut T, target: &str) {
    use std::net::TcpStream;
    let r = (|| -> std::io::Result<String> {
        // The network may still be coming up (DHCP): retry for a while.
        let start = Instant::now();
        let mut s = loop {
            match TcpStream::connect(target) {
                Ok(s) => break s,
                Err(e) if start.elapsed() > Duration::from_secs(60) => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_secs(1)),
            }
        };
        write!(s, "GET / HTTP/1.0\r\nHost: {target}\r\n\r\n")?;
        let mut body = String::new();
        s.read_to_string(&mut body)?;
        Ok(body)
    })();
    t.check("HTTP GET from the host", matches!(&r, Ok(b) if b.starts_with("HTTP/1.")), format!("{:?}", r.map(|b| b.chars().take(80).collect::<String>())));
}

fn process(t: &mut T) {
    let out = std::process::Command::new("CMD.EXE").args(["/C", "ECHO", "hello"]).output();
    t.check(
        "Command output",
        matches!(&out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "hello"),
        format!("{out:?}"),
    );
    let exe = std::env::current_exe().unwrap();
    let st = std::process::Command::new(&exe).args(["child-exit", "3"]).status();
    t.check("exit status", matches!(&st, Ok(s) if s.code() == Some(3)), format!("{st:?}"));
    let out = std::process::Command::new(&exe)
        .args(["child-echo"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin.take().unwrap().write_all(b"through a pipe\n")?;
            c.wait_with_output()
        });
    t.check(
        "stdin and stdout piped",
        matches!(&out, Ok(o) if String::from_utf8_lossy(&o.stdout) == "got: through a pipe\n"),
        format!("{out:?}"),
    );
    let e = std::process::Command::new("NOSUCHPROGRAM").status();
    t.check("missing program", matches!(&e, Err(e) if e.kind() == std::io::ErrorKind::NotFound), format!("{e:?}"));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut t = T { pass: 0, fail: 0 };
    let want: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    // Child processes of the process section.
    match want.first() {
        Some(&"child-exit") => std::process::exit(want.get(1).and_then(|c| c.parse().ok()).unwrap_or(1)),
        Some(&"child-echo") => {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
            print!("got: {line}");
            return;
        }
        _ => {}
    }
    let all = want.is_empty();
    let on = |s: &str| all || want.contains(&s);
    println!("STDTEST: Rust std on rs2");
    if on("env") {
        env(&mut t, &args);
    }
    if on("time") {
        time(&mut t);
    }
    if on("threads") {
        threads(&mut t);
    }
    if on("fs") {
        fs(&mut t, "C:");
    }
    if on("panic") {
        panics(&mut t);
    }
    if on("net") {
        net(&mut t);
    }
    if on("process") {
        process(&mut t);
    }
    if on("mem") {
        mem(&mut t);
    }
    if let Some(i) = want.iter().position(|&w| w == "host") {
        host(&mut t, want.get(i + 1).copied().unwrap_or("10.0.2.2:8000"));
    }
    println!("stdtest: {} passed, {} failed", t.pass, t.fail);
    std::process::exit(if t.fail == 0 { 0 } else { 1 });
}
