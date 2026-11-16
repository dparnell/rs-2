//! KEYCHAIN: rs2's system keychain (docs/KEYCHAIN.md), through RS2API
//! 630-635 (`kernel/src/api/keychain.rs`).
//!
//! `KEYCHAIN [LIST]`: its state and entries (never the secrets).
//! `KEYCHAIN SET service account [secret] [/READABLE]`: store a secret
//! (asked for, unechoed, when not given). /READABLE lets programs read it
//! back; without it only the system can (WiFi passphrases are so).
//! `KEYCHAIN GET service account`: show a readable secret.
//! `KEYCHAIN DELETE service account`.
//! `KEYCHAIN UNLOCK [password]` / `KEYCHAIN LOCK`: with a master password.
//! `KEYCHAIN PASSWORD`: set, change or remove (empty) the master password.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "RS2API.630"]
    fn Rs2KeyQuery(buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.631"]
    fn Rs2KeySet(service: *const u8, account: *const u8, secret: *const u8, cb: u32, flags: u32) -> u32;
    #[link_name = "RS2API.632"]
    fn Rs2KeyGet(service: *const u8, account: *const u8, buf: *mut u8, cb: u32, pcb: *mut u32) -> u32;
    #[link_name = "RS2API.633"]
    fn Rs2KeyDelete(service: *const u8, account: *const u8) -> u32;
    #[link_name = "RS2API.634"]
    fn Rs2KeyUnlock(password: *const u8) -> u32;
    #[link_name = "RS2API.635"]
    fn Rs2KeySetPassword(old: *const u8, new: *const u8) -> u32;
}

const ERROR_BUFFER_OVERFLOW: u32 = 111;

fn cstr(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).chain(core::iter::once(0)).collect()
}

fn why(rc: u32) -> String {
    match rc {
        2 => "no such entry".into(),
        5 => "that entry is for the system only".into(),
        21 => "the keychain is locked (KEYCHAIN UNLOCK)".into(),
        31 => "the keychain file is damaged".into(),
        86 => "wrong master password".into(),
        87 => "a name or secret is too long".into(),
        e => alloc::format!("error {e}"),
    }
}

fn fail(rc: u32) -> u32 {
    eprint(&alloc::format!("KEYCHAIN: {}\r\n", why(rc)));
    2
}

fn query() -> Result<String, u32> {
    let mut cb = 4096u32;
    loop {
        let mut buf = alloc::vec![0u8; cb as usize];
        let mut need = 0u32;
        match unsafe { Rs2KeyQuery(buf.as_mut_ptr(), cb, &mut need) } {
            0 => {
                let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                return Ok(buf[..n].iter().map(|&b| b as char).collect());
            }
            ERROR_BUFFER_OVERFLOW if need > cb => cb = need,
            e => return Err(e),
        }
    }
}

fn field<'a>(t: &'a str, k: &str) -> Option<&'a str> {
    t.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
}

fn list() -> u32 {
    let t = match query() {
        Ok(t) => t,
        Err(rc) => return fail(rc),
    };
    let state = field(&t, "state").unwrap_or("?");
    let pw = field(&t, "password") == Some("yes");
    outln(&alloc::format!(
        "Keychain {}: {}{}",
        field(&t, "file").unwrap_or(""),
        match state {
            "empty" => "no entries yet",
            "unlocked" => "unlocked",
            "locked" => "locked (KEYCHAIN UNLOCK)",
            "damaged" => "damaged, not used",
            s => s,
        },
        if pw { ", protected by a master password" } else { ", no master password" }
    ));
    let rows: Vec<Vec<&str>> = t.lines().filter_map(|l| l.strip_prefix("entry=")).map(|l| l.split('\t').collect()).filter(|r: &Vec<&str>| r.len() >= 3).collect();
    if !rows.is_empty() {
        outln("");
        outln("Service          Account                          Programs may read");
        for r in &rows {
            let mut s = String::from(r[0]);
            while s.len() < 16 {
                s.push(' ');
            }
            let mut a = String::from(r[1]);
            while a.len() < 32 {
                a.push(' ');
            }
            outln(&alloc::format!("{s} {a} {}", if r[2] == "1" { "yes" } else { "no (system)" }));
        }
    }
    0
}

fn main(s: &os2::Startup) -> u32 {
    let mut a = args(s);
    let readable = a.iter().any(|x| x.eq_ignore_ascii_case("/READABLE") || x.eq_ignore_ascii_case("-READABLE"));
    a.retain(|x| !x.eq_ignore_ascii_case("/READABLE") && !x.eq_ignore_ascii_case("-READABLE"));
    let cmd = a.first().map(|c| c.to_ascii_uppercase()).unwrap_or_default();
    let names = |a: &Vec<String>| -> Option<(Vec<u8>, Vec<u8>)> { Some((cstr(a.get(1)?), cstr(a.get(2)?))) };
    match cmd.as_str() {
        "" | "LIST" => list(),
        "SET" => {
            let Some((sv, ac)) = names(&a) else {
                eprint("Usage: KEYCHAIN SET service account [secret] [/READABLE]\r\n");
                return 1;
            };
            let secret = match a.get(3) {
                Some(k) => k.clone(),
                None => match read_password("Secret: ") {
                    Some(k) if !k.is_empty() => k,
                    _ => return 1,
                },
            };
            let k: Vec<u8> = secret.chars().map(|c| c as u32 as u8).collect();
            match unsafe { Rs2KeySet(sv.as_ptr(), ac.as_ptr(), k.as_ptr(), k.len() as u32, readable as u32) } {
                0 => {
                    outln("Saved.");
                    0
                }
                rc => fail(rc),
            }
        }
        "GET" => {
            let Some((sv, ac)) = names(&a) else {
                eprint("Usage: KEYCHAIN GET service account\r\n");
                return 1;
            };
            let mut buf = alloc::vec![0u8; 65536];
            let mut n = 0u32;
            match unsafe { Rs2KeyGet(sv.as_ptr(), ac.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, &mut n) } {
                0 => {
                    outln(&buf[..n as usize].iter().map(|&b| b as char).collect::<String>());
                    0
                }
                rc => fail(rc),
            }
        }
        "DELETE" | "DEL" => {
            let Some((sv, ac)) = names(&a) else {
                eprint("Usage: KEYCHAIN DELETE service account\r\n");
                return 1;
            };
            match unsafe { Rs2KeyDelete(sv.as_ptr(), ac.as_ptr()) } {
                0 => {
                    outln("Deleted.");
                    0
                }
                rc => fail(rc),
            }
        }
        "UNLOCK" => {
            let pw = match a.get(1) {
                Some(p) => p.clone(),
                None => match read_password("Master password: ") {
                    Some(p) => p,
                    None => return 1,
                },
            };
            match unsafe { Rs2KeyUnlock(cstr(&pw).as_ptr()) } {
                0 => {
                    outln("Unlocked.");
                    0
                }
                rc => fail(rc),
            }
        }
        "LOCK" => match unsafe { Rs2KeyUnlock(core::ptr::null()) } {
            0 => {
                outln("Locked.");
                0
            }
            2 => {
                eprint("KEYCHAIN: only a keychain with a master password can be locked (KEYCHAIN PASSWORD)\r\n");
                2
            }
            rc => fail(rc),
        },
        "PASSWORD" => {
            let has = query().ok().is_some_and(|t| field(&t, "password") == Some("yes"));
            let old = if has {
                match read_password("Current master password: ") {
                    Some(p) => Some(p),
                    None => return 1,
                }
            } else {
                None
            };
            let Some(new) = read_password("New master password (empty for none): ") else { return 1 };
            if !new.is_empty() {
                let Some(again) = read_password("Again: ") else { return 1 };
                if again != new {
                    eprint("KEYCHAIN: the passwords differ; nothing changed\r\n");
                    return 1;
                }
            }
            let o = old.as_deref().map(cstr);
            let n = (!new.is_empty()).then(|| cstr(&new));
            let p = |v: &Option<Vec<u8>>| v.as_ref().map_or(core::ptr::null(), |v| v.as_ptr());
            match unsafe { Rs2KeySetPassword(p(&o), p(&n)) } {
                0 => {
                    outln(if n.is_some() {
                        "Master password set. The keychain is locked at each boot until KEYCHAIN UNLOCK."
                    } else {
                        "No master password now."
                    });
                    0
                }
                rc => fail(rc),
            }
        }
        _ => {
            eprint("Usage: KEYCHAIN [LIST | SET service account [secret] [/READABLE] | GET service account | DELETE service account | UNLOCK [password] | LOCK | PASSWORD]\r\n");
            1
        }
    }
}
