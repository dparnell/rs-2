//! REXEC: run a command on a host's rexec server (port 512).
//!
//! `REXEC host [-l user] [-p password] [-n] command`: the user and
//! password come from the options, from the host's entry in %ETC%\NETRC,
//! or are asked for. -n sends no input (otherwise redirected standard
//! input goes to the command).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut host = None;
    let mut user = None;
    let mut pass = None;
    let mut no_input = false;
    let mut cmd: Vec<String> = Vec::new();
    let mut i = 0;
    while i < a.len() {
        if !cmd.is_empty() {
            cmd.push(a[i].clone());
        } else {
            match a[i].as_str() {
                "-l" => {
                    i += 1;
                    user = a.get(i).cloned();
                }
                "-p" => {
                    i += 1;
                    pass = a.get(i).cloned();
                }
                "-n" => no_input = true,
                "-d" => {}
                x if host.is_none() => host = Some(String::from(x)),
                x => cmd.push(String::from(x)),
            }
        }
        i += 1;
    }
    let (Some(host), false) = (host, cmd.is_empty()) else {
        eprint("Usage: REXEC host [-l user] [-p password] [-n] command\r\n");
        return 1;
    };
    let (nu, np) = netrc(&host);
    let user = user.or(nu).or_else(|| read_line(&alloc::format!("Name ({host}): "))).unwrap_or_default();
    let pass = pass.or(np).or_else(|| read_password("Password: ")).unwrap_or_default();
    let port = service_port("exec", "tcp", 512);
    let (sock, _) = match Sock::connect(&host, port) {
        Ok(v) => v,
        Err(e) => {
            eprint(&alloc::format!("REXEC: {e}\r\n"));
            return 2;
        }
    };
    let mut req = Vec::new();
    for f in ["0", &user, &pass, &cmd.join(" ")] {
        req.extend_from_slice(f.as_bytes());
        req.push(0);
    }
    if sock.send(&req).is_err() {
        return 3;
    }
    run_remote(&sock, no_input, "REXEC")
}

/// The %ETC%\NETRC user and password for a host.
fn netrc(host: &str) -> (Option<String>, Option<String>) {
    let Some(d) = read_file(&etc_path("NETRC")) else { return (None, None) };
    let text: String = d.iter().map(|&b| b as char).collect();
    let t: Vec<&str> = text.split_whitespace().collect();
    let mut mine = false;
    let (mut u, mut p) = (None, None);
    let mut i = 0;
    while i + 1 < t.len() {
        match t[i] {
            "machine" => mine = t[i + 1].eq_ignore_ascii_case(host),
            "login" if mine => u = Some(String::from(t[i + 1])),
            "password" if mine => p = Some(String::from(t[i + 1])),
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    (u, p)
}
