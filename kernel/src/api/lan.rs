//! RS2API 600-608: the LAN requester (`fs::redir`) for rs2's NETAPI
//! DLLs and NET.EXE. Calls return 0, an OS/2 error or a NERR_ code.
//! Lists come back as text, one entry per line, fields separated by tabs.
//!
//! | Ord | Entry |
//! |-----|-------|
//! | 600 | Rs2LanStart(PSZ computer, PSZ workgroup, PSZ options) |
//! | 601 | Rs2LanStop(ULONG force) |
//! | 602 | Rs2LanQuery(PCHAR buf, ULONG cb, PULONG pcb): `key=value` lines |
//! | 603 | Rs2LanLogon(PSZ user, PSZ password, PSZ domain); user "" logs off |
//! | 604 | Rs2LanUseAdd(PSZ local, PSZ remote, PSZ password, PSZ user) (NULL: none) |
//! | 605 | Rs2LanUseDel(PSZ local or UNC name, ULONG force) |
//! | 606 | Rs2LanUseEnum(PCHAR buf, ULONG cb, PULONG pcb): local, remote, type, status, opens |
//! | 607 | Rs2LanShareEnum(PSZ server, PCHAR buf, ULONG cb, PULONG pcb): name, type, remark |
//! | 608 | Rs2LanServerEnum(PSZ domain, PCHAR buf, ULONG cb, PULONG pcb): name, workgroup, type, comment |
//!
//! A list larger than `cb` gives ERROR_BUFFER_OVERFLOW with the size
//! needed in `*pcb`.

use alloc::string::String;

use super::{err, Ctx};
use crate::fs::redir;

fn opt_str(c: &Ctx, at: u32) -> Result<Option<String>, u32> {
    if at == 0 {
        return Ok(None);
    }
    c.read_str(at, 512).map(Some).ok_or(err::INVALID_ADDRESS)
}

fn str_arg(c: &Ctx, i: usize) -> Result<String, u32> {
    Ok(opt_str(c, c.arg(i))?.unwrap_or_default())
}

fn done(r: Result<(), u32>) -> u32 {
    r.err().unwrap_or(0)
}

/// Copy a text result to (buf, cb, pcb).
fn out(c: &Ctx, text: &str, i: usize) -> u32 {
    let (buf, cb, pcb) = (c.arg(i), c.arg(i + 1), c.arg(i + 2));
    let b: alloc::vec::Vec<u8> = text.chars().map(|ch| ch as u32 as u8).chain(core::iter::once(0)).collect();
    if pcb != 0 {
        c.write_u32(pcb, b.len() as u32);
    }
    if b.len() > cb as usize {
        return err::BUFFER_OVERFLOW;
    }
    if c.write(buf, &b) { 0 } else { err::INVALID_ADDRESS }
}

pub fn start(c: &mut Ctx) -> u32 {
    let r = (|| redir::start(&str_arg(c, 0)?, &str_arg(c, 1)?, &str_arg(c, 2)?))();
    done(r)
}

pub fn stop(_c: &mut Ctx) -> u32 {
    redir::stop();
    0
}

pub fn query(c: &mut Ctx) -> u32 {
    let mut s = String::new();
    match redir::config() {
        Some(k) => {
            s.push_str("started=1\n");
            for (key, v) in [("computer", &k.computer), ("workgroup", &k.workgroup), ("user", &k.user), ("domain", &k.domain)] {
                s.push_str(&alloc::format!("{key}={v}\n"));
            }
            if !k.other_domains.is_empty() {
                s.push_str(&alloc::format!("othdomains={}\n", k.other_domains.join(",")));
            }
        }
        None => s.push_str("started=0\n"),
    }
    out(c, &s, 0)
}

pub fn logon(c: &mut Ctx) -> u32 {
    let r = (|| redir::logon(&str_arg(c, 0)?, &str_arg(c, 1)?, &str_arg(c, 2)?))();
    done(r)
}

pub fn use_add(c: &mut Ctx) -> u32 {
    let r = (|| {
        let (local, remote) = (str_arg(c, 0)?, str_arg(c, 1)?);
        let (pw, user) = (opt_str(c, c.arg(2))?, opt_str(c, c.arg(3))?);
        redir::use_add(&local, &remote, pw.as_deref(), user.as_deref())
    })();
    done(r)
}

pub fn use_del(c: &mut Ctx) -> u32 {
    let r = (|| redir::use_del(&str_arg(c, 0)?, c.arg(1) != 0))();
    done(r)
}

pub fn use_enum(c: &mut Ctx) -> u32 {
    if redir::config().is_none() {
        return redir::NERR_WKSTA_NOT_STARTED;
    }
    let mut s = String::new();
    for u in redir::uses() {
        let typ = match u.kind {
            smb::ShareKind::Disk => 0,
            smb::ShareKind::Print => 1,
            smb::ShareKind::Pipe => 3,
        };
        let opens = match u.local {
            redir::Local::Drive(d) => crate::fs::open_count(d),
            _ => 0,
        };
        s.push_str(&alloc::format!("{}\t{}\t{}\t0\t{}\n", u.local.name(), u.remote, typ, opens));
    }
    out(c, &s, 0)
}

pub fn share_enum(c: &mut Ctx) -> u32 {
    let server = match str_arg(c, 0) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let server = server.trim_start_matches(['\\', '/']).to_ascii_uppercase();
    match redir::shares(&server) {
        Ok(v) => {
            let mut s = String::new();
            for x in v {
                s.push_str(&alloc::format!("{}\t{}\t{}\n", x.name, x.typ, x.remark));
            }
            out(c, &s, 1)
        }
        Err(e) => e,
    }
}

pub fn server_enum(c: &mut Ctx) -> u32 {
    let dom = match opt_str(c, c.arg(0)) {
        Ok(d) => d.filter(|d| !d.is_empty()),
        Err(e) => return e,
    };
    match redir::servers(dom.as_deref()) {
        Ok(v) => {
            let mut s = String::new();
            for x in v {
                s.push_str(&alloc::format!("{}\t{}\t{}\t{}\n", x.name, x.workgroup, x.typ, x.comment));
            }
            out(c, &s, 1)
        }
        Err(e) => e,
    }
}
