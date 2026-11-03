//! SMB1 with the NT LM 0.12 dialect (MS-CIFS, MS-SMB): for servers
//! that speak nothing newer. Unicode names, NT status codes when the
//! server gives them (DOS error classes otherwise), extended security
//! (NTLMSSP) or NTLMv2 responses in the password fields, MD5 signing.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ntlm::{self, Credentials};
use crate::status as st;
use crate::wire::{from_utf16, slice, u16_at, u32_at, u64_at, utf16, Buf};
use crate::{
    Action, DirEntry, Ea, Env, Error, FileAttrs, FileId, FsSize, OpenArgs, Result, ServerInfo, SetBasic, ShareInfo, ShareKind, Transport, Tree,
};

pub const CREATE_DIRECTORY: u8 = 0x00;
pub const DELETE_DIRECTORY: u8 = 0x01;
pub const CLOSE: u8 = 0x04;
pub const FLUSH: u8 = 0x05;
pub const DELETE: u8 = 0x06;
pub const RENAME: u8 = 0x07;
pub const TRANSACTION: u8 = 0x25;
pub const ECHO: u8 = 0x2B;
pub const READ_ANDX: u8 = 0x2E;
pub const WRITE_ANDX: u8 = 0x2F;
pub const TRANSACTION2: u8 = 0x32;
pub const TREE_DISCONNECT: u8 = 0x71;
pub const NEGOTIATE: u8 = 0x72;
pub const SESSION_SETUP_ANDX: u8 = 0x73;
pub const LOGOFF_ANDX: u8 = 0x74;
pub const TREE_CONNECT_ANDX: u8 = 0x75;
pub const NT_CREATE_ANDX: u8 = 0xA2;

const FLAGS2_LONG_NAMES: u16 = 0x0001;
const FLAGS2_EAS: u16 = 0x0002;
const FLAGS2_SIGNATURES: u16 = 0x0004;
const FLAGS2_EXTENDED_SECURITY: u16 = 0x0800;
const FLAGS2_NT_STATUS: u16 = 0x4000;
const FLAGS2_UNICODE: u16 = 0x8000;

const CAP_UNICODE: u32 = 0x4;
const CAP_LARGE_FILES: u32 = 0x8;
const CAP_NT_SMBS: u32 = 0x10;
const CAP_STATUS32: u32 = 0x40;
const CAP_LARGE_READX: u32 = 0x4000;
const CAP_LARGE_WRITEX: u32 = 0x8000;
const CAP_EXTENDED_SECURITY: u32 = 0x8000_0000;

// TRANS2 subcommands and information levels.
const TRANS2_FIND_FIRST2: u16 = 1;
const TRANS2_FIND_NEXT2: u16 = 2;
const TRANS2_QUERY_FS_INFORMATION: u16 = 3;
const TRANS2_QUERY_PATH_INFORMATION: u16 = 5;
const TRANS2_SET_PATH_INFORMATION: u16 = 6;
const TRANS2_QUERY_FILE_INFORMATION: u16 = 7;
const TRANS2_SET_FILE_INFORMATION: u16 = 8;
const INFO_SET_EAS: u16 = 2;
const INFO_QUERY_ALL_EAS: u16 = 4;
const FIND_FILE_BOTH_DIRECTORY_INFO: u16 = 0x104;
const QUERY_FS_SIZE_INFO: u16 = 0x103;
const SET_FILE_BASIC_INFO: u16 = 0x101;
const SET_FILE_END_OF_FILE_INFO: u16 = 0x104;
const QUERY_FILE_ALL_INFO: u16 = 0x107;

/// The dialects offered in the negotiate (index matters).
pub const DIALECTS: &[&str] = &["PC NETWORK PROGRAM 1.0", "LANMAN1.0", "LM1.2X002", "LANMAN2.1", "NT LM 0.12", "SMB 2.002", "SMB 2.???"];
pub const NT_LM_012: u16 = 4;

/// The SMB1 NEGOTIATE that also offers SMB2 (MS-SMB2 3.2.4.2.2.1).
pub fn multi_negotiate() -> Vec<u8> {
    let mut bytes = Buf::new();
    for d in DIALECTS {
        bytes.u8(2).bytes(d.as_bytes()).u8(0);
    }
    let mut m = header(NEGOTIATE, FLAGS2_UNICODE | FLAGS2_NT_STATUS | FLAGS2_EXTENDED_SECURITY | FLAGS2_LONG_NAMES | FLAGS2_EAS, 0, 0, 0);
    m.u8(0).u16(bytes.len() as u16).bytes(&bytes.0);
    m.0
}

fn header(cmd: u8, flags2: u16, tid: u16, uid: u16, mid: u16) -> Buf {
    let mut b = Buf::new();
    b.bytes(b"\xffSMB").u8(cmd).u32(0).u8(0x18).u16(flags2).u16(0).zeros(8).u16(0).u16(tid).u16(0xFEFF).u16(uid).u16(mid);
    b
}

/// A parsed response.
struct R {
    m: Vec<u8>,
}

impl R {
    fn wc(&self) -> usize {
        self.m.get(32).copied().unwrap_or(0) as usize
    }
    fn w(&self, at: usize) -> u16 {
        u16_at(&self.m, 33 + at)
    }
    fn w32(&self, at: usize) -> u32 {
        u32_at(&self.m, 33 + at)
    }
    fn w64(&self, at: usize) -> u64 {
        u64_at(&self.m, 33 + at)
    }
    fn bytes(&self) -> &[u8] {
        let at = 33 + self.wc() * 2;
        slice(&self.m, at + 2, u16_at(&self.m, at) as usize)
    }
}

/// What the negotiate response told us.
#[derive(Clone, Debug, Default)]
pub struct Negotiated {
    pub security_mode: u8,
    pub max_buffer: u32,
    pub session_key: u32,
    pub capabilities: u32,
    pub challenge: Option<[u8; 8]>,
    pub domain: String,
}

/// Parse an SMB1 NEGOTIATE response (the NT LM 0.12 form).
pub fn parse_negotiate(m: &[u8]) -> Result<Negotiated> {
    let r = R { m: m.to_vec() };
    if r.wc() != 17 || r.w(0) != NT_LM_012 {
        return Err(Error::NoDialect);
    }
    let mut n = Negotiated {
        security_mode: m[33 + 2],
        max_buffer: r.w32(7),
        session_key: r.w32(15),
        capabilities: r.w32(19),
        ..Default::default()
    };
    let clen = m[33 + 33] as usize;
    let b = r.bytes();
    if n.capabilities & CAP_EXTENDED_SECURITY == 0 && clen == 8 && b.len() >= 8 {
        n.challenge = Some(b[..8].try_into().unwrap());
        n.domain = from_utf16(&b[8..]);
    }
    Ok(n)
}

pub struct Smb1 {
    t: Box<dyn Transport + Send>,
    env: Box<dyn Env + Send>,
    server: String,
    neg: Negotiated,
    uid: u16,
    mid: u16,
    sign: bool,
    key: Vec<u8>,
    seq: u32,
    domain: String,
    ipc: Option<u16>,
}

impl Smb1 {
    pub fn new(t: Box<dyn Transport + Send>, env: Box<dyn Env + Send>, server: &str, neg: Negotiated) -> Smb1 {
        Smb1 { t, env, server: crate::wire::upper(server), neg, uid: 0, mid: 1, sign: false, key: Vec::new(), seq: 0, domain: String::new(), ipc: None }
    }

    fn flags2(&self) -> u16 {
        let mut f = FLAGS2_UNICODE | FLAGS2_NT_STATUS | FLAGS2_LONG_NAMES | FLAGS2_EAS;
        if self.neg.capabilities & CAP_EXTENDED_SECURITY != 0 {
            f |= FLAGS2_EXTENDED_SECURITY;
        }
        if self.sign {
            f |= FLAGS2_SIGNATURES;
        }
        f
    }

    /// Send a request: `words` and `bytes` already built (bytes built with
    /// [`Smb1::bytes_base`] in mind for alignment).
    fn send(&mut self, cmd: u8, tid: u16, words: &[u8], bytes: &[u8]) -> Result<R> {
        let mid = self.mid;
        self.mid = self.mid.wrapping_add(1).max(1);
        let mut m = header(cmd, self.flags2(), tid, self.uid, mid);
        m.u8((words.len() / 2) as u8).bytes(words).u16(bytes.len() as u16).bytes(bytes);
        if self.sign {
            m.0[14..18].copy_from_slice(&self.seq.to_le_bytes());
            let mac = crate::crypto::md5(&[&self.key[..], &m.0[..]].concat());
            m.0[14..22].copy_from_slice(&mac[..8]);
            self.seq += 2;
        }
        crate::send_frame(&mut *self.t, &m.0)?;
        loop {
            let r = crate::recv_frame(&mut *self.t)?;
            if r.len() < 35 || &r[..4] != b"\xffSMB" {
                return Err(Error::Protocol("not an SMB1 response"));
            }
            if u16_at(&r, 30) != mid {
                continue;
            }
            return Ok(R { m: r });
        }
    }

    fn status(r: &R) -> Result<()> {
        let flags2 = u16_at(&r.m, 10);
        if flags2 & FLAGS2_NT_STATUS != 0 {
            match u32_at(&r.m, 5) {
                0 => Ok(()),
                s => Err(Error::Status(s)),
            }
        } else if r.m[5] != 0 {
            Err(Error::Dos(r.m[5], u16_at(&r.m, 7)))
        } else {
            Ok(())
        }
    }

    fn call(&mut self, cmd: u8, tid: u16, words: &[u8], bytes: &[u8]) -> Result<R> {
        let r = self.send(cmd, tid, words, bytes)?;
        Self::status(&r)?;
        Ok(r)
    }

    /// Offset of the bytes area for a request with `nwords` words.
    fn bytes_base(nwords: usize) -> usize {
        32 + 1 + nwords * 2 + 2
    }

    /// Append a Unicode string (NUL-terminated), aligned to 2 from the
    /// start of the message.
    fn ustr(b: &mut Buf, base: usize, s: &str) {
        if (base + b.len()) % 2 != 0 {
            b.u8(0);
        }
        b.bytes(&utf16(s)).u16(0);
    }

    fn path(p: &str) -> String {
        alloc::format!("\\{}", p)
    }

    /// Log on: NTLMSSP in SPNEGO with extended security, else NTLMv2
    /// responses in the password fields.
    pub fn session_setup(&mut self, cred: &Credentials) -> Result<()> {
        let caps = CAP_UNICODE | CAP_LARGE_FILES | CAP_NT_SMBS | CAP_STATUS32 | CAP_LARGE_READX | CAP_LARGE_WRITEX;
        let mut rnd = [0u8; 24];
        self.env.random(&mut rnd);
        let key;
        if self.neg.capabilities & CAP_EXTENDED_SECURITY != 0 {
            let caps = caps | CAP_EXTENDED_SECURITY;
            let words = |blob_len: u16, sk: u32| {
                let mut w = Buf::new();
                w.u8(0xFF).u8(0).u16(0).u16(16644).u16(2).u16(0).u32(sk).u16(blob_len).u32(0).u32(caps);
                w.0
            };
            let tail = |b: &mut Buf| {
                Self::ustr(b, Self::bytes_base(12), "OS/2");
                Self::ustr(b, Self::bytes_base(12), "rs2");
            };
            let blob = ntlm::spnego_init(&ntlm::negotiate());
            let mut b = Buf::new();
            b.bytes(&blob);
            tail(&mut b);
            let r = self.send(SESSION_SETUP_ANDX, 0, &words(blob.len() as u16, self.neg.session_key), &b.0)?;
            if u32_at(&r.m, 5) != st::MORE_PROCESSING_REQUIRED {
                Self::status(&r)?;
                return Err(Error::Status(st::LOGON_FAILURE));
            }
            self.uid = u16_at(&r.m, 28);
            let blob = slice(r.bytes(), 0, r.w(6) as usize);
            let ch = ntlm::parse_challenge(blob).ok_or(Error::Protocol("no NTLM challenge"))?;
            self.domain = ch.nb_domain.clone();
            let auth = ntlm::authenticate(cred, &ch, self.env.now(), &rnd);
            let blob = ntlm::spnego_response(&auth.message);
            let mut b = Buf::new();
            b.bytes(&blob);
            tail(&mut b);
            let r = self.send(SESSION_SETUP_ANDX, 0, &words(blob.len() as u16, self.neg.session_key), &b.0)?;
            Self::status(&r)?;
            key = auth.session_key.to_vec();
            if r.w(4) & 1 != 0 {
                // Logged on as a guest: no signing.
                self.neg.security_mode &= !0x08;
            }
        } else {
            let challenge = self.neg.challenge.ok_or(Error::Protocol("no challenge"))?;
            let (lm, nt, k) = if cred.anonymous() {
                (Vec::new(), Vec::new(), [0u8; 16])
            } else {
                ntlm::responses(cred, &challenge, &[], self.env.now(), rnd[..8].try_into().unwrap())
            };
            let mut w = Buf::new();
            w.u8(0xFF).u8(0).u16(0).u16(16644).u16(2).u16(0).u32(self.neg.session_key).u16(lm.len() as u16).u16(nt.len() as u16).u32(0).u32(caps);
            let mut b = Buf::new();
            b.bytes(&lm).bytes(&nt);
            let base = Self::bytes_base(13);
            Self::ustr(&mut b, base, &cred.user);
            Self::ustr(&mut b, base, &cred.domain);
            Self::ustr(&mut b, base, "OS/2");
            Self::ustr(&mut b, base, "rs2");
            let r = self.call(SESSION_SETUP_ANDX, 0, &w.0, &b.0)?;
            self.uid = u16_at(&r.m, 28);
            self.domain = self.neg.domain.clone();
            key = k.to_vec();
        }
        if self.neg.security_mode & 0x08 != 0 && !cred.anonymous() {
            self.sign = true;
            self.key = key;
            self.seq = 2;
        }
        Ok(())
    }

    /// A transaction (TRANS2 with a setup word, or TRANSACTION on a
    /// named pipe): the response's parameters and data, reassembled.
    fn trans(&mut self, cmd: u8, tid: u16, setup: &[u16], name: Option<&str>, params: &[u8], data: &[u8], max_data: u16) -> Result<(Vec<u8>, Vec<u8>)> {
        let nwords = 14 + setup.len();
        let base = Self::bytes_base(nwords);
        let mut b = Buf::new();
        match name {
            Some(n) => Self::ustr(&mut b, base, n),
            None => {
                b.u8(0);
            }
        }
        while (base + b.len()) % 4 != 0 {
            b.u8(0);
        }
        let poff = base + b.len();
        b.bytes(params);
        while (base + b.len()) % 4 != 0 {
            b.u8(0);
        }
        let doff = base + b.len();
        b.bytes(data);
        let mut w = Buf::new();
        w.u16(params.len() as u16).u16(data.len() as u16).u16(1024).u16(max_data).u8(0).u8(0).u16(0).u32(0).u16(0);
        w.u16(params.len() as u16).u16(poff as u16).u16(data.len() as u16).u16(doff as u16).u8(setup.len() as u8).u8(0);
        for s in setup {
            w.u16(*s);
        }
        let r = self.call(cmd, tid, &w.0, &b.0)?;
        let (tp, td) = (r.w(0) as usize, r.w(2) as usize);
        let mut p = alloc::vec![0u8; tp];
        let mut d = alloc::vec![0u8; td];
        let (mut gp, mut gd) = (0, 0);
        let mut r = r;
        loop {
            let (pc, po, pd) = (r.w(6) as usize, r.w(8) as usize, r.w(10) as usize);
            let (dc, dof, dd) = (r.w(12) as usize, r.w(14) as usize, r.w(16) as usize);
            if pd + pc <= p.len() {
                p[pd..pd + pc].copy_from_slice(slice(&r.m, po, pc));
            }
            if dd + dc <= d.len() {
                d[dd..dd + dc].copy_from_slice(slice(&r.m, dof, dc));
            }
            gp += pc;
            gd += dc;
            if gp >= tp && gd >= td {
                break;
            }
            // Secondary responses come with the same MID.
            let m = crate::recv_frame(&mut *self.t)?;
            r = R { m };
            Self::status(&r)?;
        }
        Ok((p, d))
    }

    fn trans2(&mut self, tid: u16, sub: u16, params: &[u8], data: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let max = (self.neg.max_buffer.min(65535) as u16).saturating_sub(256);
        self.trans(TRANSACTION2, tid, &[sub], None, params, data, max)
    }

    fn name_params(prefix: &[u8], path: &str) -> Vec<u8> {
        let mut b = Buf::new();
        b.bytes(prefix).bytes(&utf16(path)).u16(0);
        b.0
    }

    fn all_info(b: &[u8]) -> FileAttrs {
        FileAttrs {
            created: u64_at(b, 0),
            accessed: u64_at(b, 8),
            written: u64_at(b, 16),
            changed: u64_at(b, 24),
            attr: u32_at(b, 32),
            alloc: u64_at(b, 40),
            size: u64_at(b, 48),
            ea_size: u32_at(b, 64),
        }
    }

    fn fid(f: FileId) -> u16 {
        u16::from_le_bytes([f.0[0], f.0[1]])
    }

    fn ipc(&mut self) -> Result<u16> {
        if let Some(t) = self.ipc {
            return Ok(t);
        }
        let t = crate::Session::tree_connect(self, "IPC$")?.id as u16;
        self.ipc = Some(t);
        Ok(t)
    }

    fn simple_path(&mut self, cmd: u8, tid: u32, words: &[u8], path: &str) -> Result<()> {
        let mut b = Buf::new();
        b.u8(4);
        Self::ustr(&mut b, Self::bytes_base(words.len() / 2), &Self::path(path));
        self.call(cmd, tid as u16, words, &b.0)?;
        Ok(())
    }

    fn io_size(&self, large: u32) -> u32 {
        if self.neg.capabilities & large != 0 { 60 * 1024 } else { self.neg.max_buffer.saturating_sub(128).clamp(512, 60 * 1024) }
    }
}

/// An FEALIST (OS/2's EA list) as SMB1 carries it.
pub fn fealist(eas: &[Ea]) -> Vec<u8> {
    let mut b = Buf::new();
    b.u32(0);
    for e in eas {
        b.u8(e.flags).u8(e.name.len() as u8).u16(e.value.len() as u16).bytes(&e.name).u8(0).bytes(&e.value);
    }
    let n = b.len() as u32;
    b.put_u32(0, n);
    b.0
}

pub fn parse_fealist(b: &[u8]) -> Vec<Ea> {
    let end = (u32_at(b, 0) as usize).min(b.len());
    let mut v = Vec::new();
    let mut at = 4;
    while at + 4 <= end {
        let (flags, nlen, vlen) = (b[at], b[at + 1] as usize, u16_at(b, at + 2) as usize);
        let name = slice(b, at + 4, nlen).to_vec();
        let value = slice(b, at + 5 + nlen, vlen).to_vec();
        at += 5 + nlen + vlen;
        v.push(Ea { flags, name, value });
    }
    v
}

impl crate::Session for Smb1 {
    fn dialect(&self) -> &'static str {
        "NT LM 0.12"
    }

    fn server_domain(&self) -> String {
        self.domain.clone()
    }

    fn max_io(&self) -> u32 {
        self.io_size(CAP_LARGE_READX).min(self.io_size(CAP_LARGE_WRITEX))
    }

    fn tree_connect(&mut self, share: &str) -> Result<Tree> {
        let mut b = Buf::new();
        b.u8(0);
        Self::ustr(&mut b, Self::bytes_base(4), &alloc::format!("\\\\{}\\{}", self.server, share));
        b.bytes(b"?????\0");
        let mut w = Buf::new();
        w.u8(0xFF).u8(0).u16(0).u16(0).u16(1);
        let r = self.call(TREE_CONNECT_ANDX, 0, &w.0, &b.0)?;
        let service = r.bytes();
        let kind = if service.starts_with(b"IPC") {
            ShareKind::Pipe
        } else if service.starts_with(b"LPT") {
            ShareKind::Print
        } else {
            ShareKind::Disk
        };
        Ok(Tree { id: u16_at(&r.m, 24) as u32, kind })
    }

    fn tree_disconnect(&mut self, tid: u32) -> Result<()> {
        self.call(TREE_DISCONNECT, tid as u16, &[], &[])?;
        Ok(())
    }

    fn open(&mut self, tid: u32, path: &str, a: &OpenArgs) -> Result<(FileId, FileAttrs, Action)> {
        let mut access = 0x80 | 0x100000; // READ_ATTRIBUTES | SYNCHRONIZE
        if a.read {
            access |= 0x1 | 0x8 | 0x20000;
        }
        if a.write {
            access |= 0x2 | 0x4 | 0x10 | 0x100;
        }
        if a.delete {
            access |= 0x10000;
        }
        let options = match a.directory {
            Some(true) => 1,
            Some(false) => 0x40,
            None => 0,
        };
        let share = a.share_read as u32 | (a.share_write as u32) << 1 | 4;
        let name = utf16(&Self::path(path));
        let mut w = Buf::new();
        w.u8(0xFF).u8(0).u16(0).u8(0).u16(name.len() as u16).u32(0).u32(0).u32(access).u64(0).u32(a.attr).u32(share);
        w.u32(a.disposition as u32).u32(options).u32(2).u8(0);
        let mut b = Buf::new();
        Self::ustr(&mut b, Self::bytes_base(24), &Self::path(path));
        let r = self.call(NT_CREATE_ANDX, tid as u16, &w.0, &b.0)?;
        let mut f = FileId::default();
        f.0[..2].copy_from_slice(&r.w(5).to_le_bytes());
        let action = match r.w32(7) {
            2 => Action::Created,
            0 | 3 => Action::Overwritten,
            _ => Action::Opened,
        };
        let at = FileAttrs {
            created: r.w64(11),
            accessed: r.w64(19),
            written: r.w64(27),
            changed: r.w64(35),
            attr: r.w32(43),
            alloc: r.w64(47),
            size: r.w64(55),
            ea_size: 0,
        };
        Ok((f, at, action))
    }

    fn close(&mut self, tid: u32, f: FileId) -> Result<()> {
        let mut w = Buf::new();
        w.u16(Self::fid(f)).u32(0xFFFF_FFFF);
        self.call(CLOSE, tid as u16, &w.0, &[])?;
        Ok(())
    }

    fn read(&mut self, tid: u32, f: FileId, off: u64, len: u32) -> Result<Vec<u8>> {
        let len = len.min(self.io_size(CAP_LARGE_READX));
        let mut w = Buf::new();
        w.u8(0xFF).u8(0).u16(0).u16(Self::fid(f)).u32(off as u32).u16(len as u16).u16(0).u32(len >> 16).u16(0).u32((off >> 32) as u32);
        let r = match self.call(READ_ANDX, tid as u16, &w.0, &[]) {
            Err(Error::Status(st::END_OF_FILE)) | Err(Error::Dos(1, 38)) => return Ok(Vec::new()),
            r => r?,
        };
        let n = r.w(10) as usize | (r.w(14) as usize) << 16;
        Ok(slice(&r.m, r.w(12) as usize, n).to_vec())
    }

    fn write(&mut self, tid: u32, f: FileId, off: u64, data: &[u8]) -> Result<u32> {
        let data = &data[..data.len().min(self.io_size(CAP_LARGE_WRITEX) as usize)];
        let base = Self::bytes_base(14);
        let doff = base + 1;
        let mut w = Buf::new();
        w.u8(0xFF).u8(0).u16(0).u16(Self::fid(f)).u32(off as u32).u32(0).u16(0).u16(0);
        w.u16((data.len() >> 16) as u16).u16(data.len() as u16).u16(doff as u16).u32((off >> 32) as u32);
        let mut b = Buf::new();
        b.u8(0).bytes(data);
        let r = self.call(WRITE_ANDX, tid as u16, &w.0, &b.0)?;
        Ok(r.w(4) as u32 | (r.w(8) as u32) << 16)
    }

    fn flush(&mut self, tid: u32, f: FileId) -> Result<()> {
        self.call(FLUSH, tid as u16, &Self::fid(f).to_le_bytes(), &[])?;
        Ok(())
    }

    fn stat(&mut self, tid: u32, f: FileId) -> Result<FileAttrs> {
        let mut p = Buf::new();
        p.u16(Self::fid(f)).u16(QUERY_FILE_ALL_INFO);
        let (_, d) = self.trans2(tid as u16, TRANS2_QUERY_FILE_INFORMATION, &p.0, &[])?;
        Ok(Self::all_info(&d))
    }

    fn stat_path(&mut self, tid: u32, path: &str) -> Result<FileAttrs> {
        let mut pre = Buf::new();
        pre.u16(QUERY_FILE_ALL_INFO).u32(0);
        let (_, d) = self.trans2(tid as u16, TRANS2_QUERY_PATH_INFORMATION, &Self::name_params(&pre.0, &Self::path(path)), &[])?;
        Ok(Self::all_info(&d))
    }

    fn list(&mut self, tid: u32, dir: &str, pattern: &str) -> Result<Vec<DirEntry>> {
        let spec = if dir.is_empty() { alloc::format!("\\{pattern}") } else { alloc::format!("\\{dir}\\{pattern}") };
        let mut pre = Buf::new();
        pre.u16(0x16).u16(512).u16(0x0002 | 0x0004).u16(FIND_FILE_BOTH_DIRECTORY_INFO).u32(0);
        let (p, d) = match self.trans2(tid as u16, TRANS2_FIND_FIRST2, &Self::name_params(&pre.0, &spec), &[]) {
            Err(Error::Status(st::NO_SUCH_FILE)) | Err(Error::Status(st::NO_MORE_FILES)) | Err(Error::Dos(1, 2)) | Err(Error::Dos(1, 18)) => {
                return Ok(Vec::new());
            }
            r => r?,
        };
        let sid = u16_at(&p, 0);
        let mut count = u16_at(&p, 2) as usize;
        let mut end = u16_at(&p, 4) != 0;
        let mut data = d;
        let mut v = Vec::new();
        loop {
            let mut at = 0;
            let mut last = String::new();
            for _ in 0..count {
                if at + 94 > data.len() {
                    break;
                }
                let next = u32_at(&data, at) as usize;
                let name = from_utf16(slice(&data, at + 94, u32_at(&data, at + 60) as usize));
                last = name.clone();
                if name != "." && name != ".." {
                    v.push(DirEntry {
                        name,
                        attrs: FileAttrs {
                            created: u64_at(&data, at + 8),
                            accessed: u64_at(&data, at + 16),
                            written: u64_at(&data, at + 24),
                            changed: u64_at(&data, at + 32),
                            size: u64_at(&data, at + 40),
                            alloc: u64_at(&data, at + 48),
                            attr: u32_at(&data, at + 56),
                            ea_size: u32_at(&data, at + 64),
                        },
                    });
                }
                if next == 0 {
                    break;
                }
                at += next;
            }
            if end || count == 0 {
                break;
            }
            let mut pre = Buf::new();
            pre.u16(sid).u16(512).u16(FIND_FILE_BOTH_DIRECTORY_INFO).u32(0).u16(0x0002 | 0x0004 | 0x0008);
            let (p, d) = self.trans2(tid as u16, TRANS2_FIND_NEXT2, &Self::name_params(&pre.0, &last), &[])?;
            count = u16_at(&p, 0) as usize;
            end = u16_at(&p, 2) != 0;
            data = d;
        }
        Ok(v)
    }

    fn set_basic(&mut self, tid: u32, f: FileId, sb: &SetBasic) -> Result<()> {
        let attr = sb.attr.map_or(0, |a| if a == 0 { crate::attr::NORMAL } else { a });
        let mut d = Buf::new();
        d.u64(sb.created).u64(sb.accessed).u64(sb.written).u64(0).u32(attr).u32(0);
        let mut p = Buf::new();
        p.u16(Self::fid(f)).u16(SET_FILE_BASIC_INFO).u16(0);
        self.trans2(tid as u16, TRANS2_SET_FILE_INFORMATION, &p.0, &d.0)?;
        Ok(())
    }

    fn set_basic_path(&mut self, tid: u32, path: &str, sb: &SetBasic) -> Result<()> {
        let attr = sb.attr.map_or(0, |a| if a == 0 { crate::attr::NORMAL } else { a });
        let mut d = Buf::new();
        d.u64(sb.created).u64(sb.accessed).u64(sb.written).u64(0).u32(attr).u32(0);
        let mut pre = Buf::new();
        pre.u16(SET_FILE_BASIC_INFO).u32(0);
        self.trans2(tid as u16, TRANS2_SET_PATH_INFORMATION, &Self::name_params(&pre.0, &Self::path(path)), &d.0)?;
        Ok(())
    }

    fn set_size(&mut self, tid: u32, f: FileId, size: u64) -> Result<()> {
        let mut p = Buf::new();
        p.u16(Self::fid(f)).u16(SET_FILE_END_OF_FILE_INFO).u16(0);
        self.trans2(tid as u16, TRANS2_SET_FILE_INFORMATION, &p.0, &size.to_le_bytes())?;
        Ok(())
    }

    fn mkdir(&mut self, tid: u32, path: &str) -> Result<()> {
        self.simple_path(CREATE_DIRECTORY, tid, &[], path)
    }

    fn rmdir(&mut self, tid: u32, path: &str) -> Result<()> {
        self.simple_path(DELETE_DIRECTORY, tid, &[], path)
    }

    fn delete(&mut self, tid: u32, path: &str) -> Result<()> {
        self.simple_path(DELETE, tid, &0x06u16.to_le_bytes(), path)
    }

    fn rename(&mut self, tid: u32, from: &str, to: &str) -> Result<()> {
        let base = Self::bytes_base(1);
        let mut b = Buf::new();
        b.u8(4);
        Self::ustr(&mut b, base, &Self::path(from));
        b.u8(4);
        Self::ustr(&mut b, base, &Self::path(to));
        self.call(RENAME, tid as u16, &0x16u16.to_le_bytes(), &b.0)?;
        Ok(())
    }

    fn fs_size(&mut self, tid: u32) -> Result<FsSize> {
        let (_, d) = self.trans2(tid as u16, TRANS2_QUERY_FS_INFORMATION, &QUERY_FS_SIZE_INFO.to_le_bytes(), &[])?;
        Ok(FsSize { total_units: u64_at(&d, 0), free_units: u64_at(&d, 8), sectors_per_unit: u32_at(&d, 16), bytes_per_sector: u32_at(&d, 20) })
    }

    fn eas(&mut self, tid: u32, path: &str) -> Result<Vec<Ea>> {
        let mut pre = Buf::new();
        pre.u16(INFO_QUERY_ALL_EAS).u32(0);
        match self.trans2(tid as u16, TRANS2_QUERY_PATH_INFORMATION, &Self::name_params(&pre.0, &Self::path(path)), &[]) {
            Ok((_, d)) => Ok(parse_fealist(&d)),
            Err(Error::Status(st::NO_EAS_ON_FILE)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn set_eas(&mut self, tid: u32, path: &str, eas: &[Ea]) -> Result<()> {
        if eas.is_empty() {
            return Ok(());
        }
        let mut pre = Buf::new();
        pre.u16(INFO_SET_EAS).u32(0);
        self.trans2(tid as u16, TRANS2_SET_PATH_INFORMATION, &Self::name_params(&pre.0, &Self::path(path)), &fealist(eas))?;
        Ok(())
    }

    fn shares(&mut self) -> Result<Vec<ShareInfo>> {
        let tid = self.ipc()?;
        let (p, d) = self.trans(TRANSACTION, tid, &[], Some("\\PIPE\\LANMAN"), &crate::rpc::rap_share_enum(16384), &[], 16384)?;
        crate::rpc::parse_rap_shares(&p, &d)
    }

    fn servers(&mut self, domain: Option<&str>, typ: u32) -> Result<Vec<ServerInfo>> {
        let tid = self.ipc()?;
        let (p, d) = self.trans(TRANSACTION, tid, &[], Some("\\PIPE\\LANMAN"), &crate::rpc::rap_server_enum2(16384, typ, domain), &[], 16384)?;
        crate::rpc::parse_rap_servers(&p, &d)
    }

    fn echo(&mut self) -> Result<()> {
        self.call(ECHO, 0xFFFF, &1u16.to_le_bytes(), b"rs2")?;
        Ok(())
    }

    fn logoff(&mut self) -> Result<()> {
        self.call(LOGOFF_ANDX, 0, &[0xFF, 0, 0, 0], &[])?;
        self.uid = 0;
        Ok(())
    }
}
