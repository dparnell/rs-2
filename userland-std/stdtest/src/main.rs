use std::collections::HashMap;
use std::io::{Read, Write};
extern crate rs2libc;
fn main() {
    println!("hello from std");
    let t = std::thread::spawn(|| 42);
    println!("{}", t.join().unwrap());
    let m = std::sync::Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new()));
    let _ = m.1.wait_timeout(m.0.lock().unwrap(), std::time::Duration::from_millis(1));
    let rw = std::sync::RwLock::new(1);
    let _g = rw.read();
    let mut hm = HashMap::new();
    hm.insert(1, 2);
    println!("{:?} {:?} {:?}", hm, std::time::Instant::now(), std::time::SystemTime::now());
    std::thread::sleep(std::time::Duration::from_millis(10));
    for e in std::fs::read_dir("C:/").unwrap() {
        let e = e.unwrap();
        println!("{:?} {:?}", e.path(), e.metadata().map(|m| m.len()));
    }
    std::fs::write("x", b"abc").unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open("x").unwrap();
    f.write_all(b"d").unwrap();
    let _ = f.sync_all();
    let _ = f.set_len(2);
    let mut s = String::new();
    std::fs::File::open("x").unwrap().read_to_string(&mut s).unwrap();
    std::fs::create_dir_all("a/b").unwrap();
    std::fs::rename("x", "a/y").unwrap();
    std::fs::remove_file("a/y").unwrap();
    std::fs::remove_dir_all("a").unwrap();
    let _ = std::fs::canonicalize(".");
    let _ = std::env::current_dir();
    let _ = std::env::set_current_dir("C:/");
    let _ = std::env::current_exe();
    unsafe { std::env::set_var("A", "B") };
    unsafe { std::env::remove_var("A") };
    let _ = std::env::args().count();
    let _ = std::env::temp_dir();
    let s = std::net::TcpStream::connect("10.0.2.2:80");
    if let Ok(mut s) = s {
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(1)));
        let _ = s.set_nodelay(true);
        let _ = s.write(b"x");
        let mut b = [0; 10];
        let _ = s.read(&mut b);
        let _ = s.peer_addr();
        let _ = s.local_addr();
        let _ = s.shutdown(std::net::Shutdown::Both);
        let _ = s.set_nonblocking(true);
        let _ = s.try_clone();
    }
    let _ = std::net::TcpStream::connect_timeout(&"10.0.2.2:80".parse().unwrap(), std::time::Duration::from_secs(1));
    if let Ok(l) = std::net::TcpListener::bind("0.0.0.0:0") {
        let _ = l.accept();
    }
    if let Ok(u) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let _ = u.send_to(b"x", "10.0.2.2:53");
        let mut b = [0; 10];
        let _ = u.recv_from(&mut b);
    }
    use std::net::ToSocketAddrs;
    let _ = "example.com:443".to_socket_addrs();
    println!("{:?}", std::env::vars().count());
    let r = std::panic::catch_unwind(|| panic!("x"));
    println!("{:?}", r.is_err());
    let _ = std::thread::available_parallelism();
    let _ = std::process::Command::new("CMD.EXE").arg("/c").output();
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(0);
}
