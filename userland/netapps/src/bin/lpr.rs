//! LPR: send files to a printer of a remote LPD (RFC 1179).
//!
//! `LPR [-p printer] [-s server] [-b | -f] [-C class] [-J job] [-T title]
//! [-o] [-# copies] [-h] [file...]`: -b sends the files as they are (the
//! default, control file `l`), -f as text to be formatted (`f`), -o as
//! PostScript (`o`), -h without a banner page. Without files it sends
//! standard input.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
os2::entry!(main);

fn usage(msg: &str) -> u32 {
    if !msg.is_empty() {
        eprint(&alloc::format!("LPR: {msg}\r\n"));
    }
    eprint("Usage: LPR [-p printer] [-s server] [-b|-f|-o] [-C class] [-J job] [-T title] [-# copies] [-h] [file ...]\r\n");
    1
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let (t, opts, files) = match lpd::parse(&a, &["c", "j", "t", "#", "m"]) {
        Ok(v) => v,
        Err(e) => return usage(&e),
    };
    let mut kind = 'l';
    let mut class = String::new();
    let mut job = String::new();
    let mut title = String::new();
    let mut copies = 1u32;
    let mut banner = true;
    let mut mail = String::new();
    for (f, v) in &opts {
        match f.as_str() {
            "b" | "l" => kind = 'l',
            "f" => kind = 'f',
            "o" => kind = 'o',
            "c" => class = v.clone(),
            "j" => job = v.clone(),
            "t" => title = v.clone(),
            "#" => copies = v.parse().unwrap_or(1).clamp(1, 100),
            "h" => banner = false,
            "m" => mail = v.clone(),
            o => return usage(&alloc::format!("unknown option -{o}")),
        }
    }
    let mut docs: Vec<(String, Vec<u8>)> = Vec::new();
    if files.is_empty() {
        docs.push((String::from("(stdin)"), utils::read_all(0)));
    }
    for f in &files {
        match read_file(f) {
            Some(d) => docs.push((f.clone(), d)),
            None => {
                eprint(&alloc::format!("LPR: cannot open {f}\r\n"));
                return 2;
            }
        }
    }
    let host = os2::net::hostname();
    let user = lpd::user();
    let jobnum = (os2::net::ms() / 7 % 1000) as u16;
    let sock = match lpd::connect(&t) {
        Ok(s) => s,
        Err(e) => {
            eprint(&alloc::format!("LPR: {e}\r\n"));
            return 3;
        }
    };
    let res = (|| -> Result<(), String> {
        let e = |e: u32| String::from(os2::net::strerror(e));
        sock.send_str(&alloc::format!("\x02{}\n", t.printer)).map_err(e)?;
        lpd::ack(&sock)?;
        let jname = if !job.is_empty() { job.clone() } else { docs[0].0.clone() };
        let mut cf = alloc::format!("H{host}\nP{user}\n");
        if banner {
            if !class.is_empty() {
                cf.push_str(&alloc::format!("C{class}\n"));
            }
            cf.push_str(&alloc::format!("J{jname}\nL{user}\n"));
        }
        if !mail.is_empty() {
            cf.push_str(&alloc::format!("M{mail}\n"));
        }
        for (i, (name, data)) in docs.iter().enumerate() {
            let df = alloc::format!("df{}{jobnum:03}{host}", (b'A' + i as u8) as char);
            sock.send_str(&alloc::format!("\x03{} {df}\n", data.len())).map_err(e)?;
            lpd::ack(&sock)?;
            sock.send(data).map_err(e)?;
            sock.send(&[0]).map_err(e)?;
            lpd::ack(&sock)?;
            if !title.is_empty() && kind == 'f' {
                cf.push_str(&alloc::format!("T{title}\n"));
            }
            for _ in 0..copies {
                cf.push_str(&alloc::format!("{kind}{df}\n"));
            }
            cf.push_str(&alloc::format!("U{df}\nN{name}\n"));
        }
        let cfn = alloc::format!("cfA{jobnum:03}{host}");
        sock.send_str(&alloc::format!("\x02{} {cfn}\n", cf.len())).map_err(e)?;
        lpd::ack(&sock)?;
        sock.send_str(&cf).map_err(e)?;
        sock.send(&[0]).map_err(e)?;
        lpd::ack(&sock)?;
        Ok(())
    })();
    match res {
        Ok(()) => {
            outln(&alloc::format!("LPR: job {jobnum:03} sent to {} on {}", t.printer, t.server));
            0
        }
        Err(e) => {
            eprint(&alloc::format!("LPR: {e}\r\n"));
            4
        }
    }
}
