//! SMB 2.0.2 and 2.1 (MS-SMB2): one request at a time, I/O of at most
//! 64 KB per request (one credit), HMAC-SHA256 signing when the server
//! requires it.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ntlm::{self, Credentials};
use crate::status as st;
use crate::wire::{from_utf16, slice, u16_at, u32_at, u64_at, utf16, Buf};
use crate::{
    Action, DirEntry, Ea, Env, Error, FileAttrs, FileId, FsSize, OpenArgs, Result, ServerInfo, SetBasic, ShareInfo, ShareKind, Transport, Tree,
};

pub const NEGOTIATE: u16 = 0;
pub const SESSION_SETUP: u16 = 1;
pub const LOGOFF: u16 = 2;
pub const TREE_CONNECT: u16 = 3;
pub const TREE_DISCONNECT: u16 = 4;
pub const CREATE: u16 = 5;
pub const CLOSE: u16 = 6;
pub const FLUSH: u16 = 7;
pub const READ: u16 = 8;
pub const WRITE: u16 = 9;
pub const IOCTL: u16 = 11;
pub const ECHO: u16 = 13;
pub const QUERY_DIRECTORY: u16 = 14;
pub const QUERY_INFO: u16 = 16;
pub const SET_INFO: u16 = 17;

const FLAGS_SERVER_TO_REDIR: u32 = 1;
const FLAGS_ASYNC: u32 = 2;
const FLAGS_SIGNED: u32 = 8;

pub const DIALECT_202: u16 = 0x0202;
pub const DIALECT_210: u16 = 0x0210;
pub const DIALECT_WILDCARD: u16 = 0x02FF;

const SIGNING_ENABLED: u16 = 1;
const SIGNING_REQUIRED: u16 = 2;

// Access mask bits.
const FILE_READ_DATA: u32 = 0x1;
const FILE_WRITE_DATA: u32 = 0x2;
const FILE_APPEND_DATA: u32 = 0x4;
const FILE_READ_EA: u32 = 0x8;
const FILE_WRITE_EA: u32 = 0x10;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
const DELETE: u32 = 0x10000;
const READ_CONTROL: u32 = 0x20000;
const SYNCHRONIZE: u32 = 0x100000;

// Create options.
const FILE_DIRECTORY_FILE: u32 = 0x1;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_DELETE_ON_CLOSE: u32 = 0x1000;

// Information classes.
const INFO_FILE: u8 = 1;
const INFO_FILESYSTEM: u8 = 2;
const FILE_FULL_DIRECTORY_INFORMATION: u8 = 2;
const FILE_BASIC_INFORMATION: u8 = 4;
const FILE_RENAME_INFORMATION: u8 = 10;
const FILE_DISPOSITION_INFORMATION: u8 = 13;
const FILE_FULL_EA_INFORMATION: u8 = 15;
const FILE_END_OF_FILE_INFORMATION: u8 = 20;
const FILE_NETWORK_OPEN_INFORMATION: u8 = 34;
const FILE_EA_INFORMATION: u8 = 7;
const FS_SIZE_INFORMATION: u8 = 3;

const FSCTL_PIPE_TRANSCEIVE: u32 = 0x0011_C017;

/// Largest single read/write/transfer we do.
const MAX_IO: u32 = 65536;

/// A response: status and the whole message (header included, so body
/// offsets index it directly).
struct Resp {
    status: u32,
    m: Vec<u8>,
}

impl Resp {
    fn body(&self, at: usize) -> usize {
        64 + at
    }
    fn u16(&self, at: usize) -> u16 {
        u16_at(&self.m, 64 + at)
    }
    fn u32(&self, at: usize) -> u32 {
        u32_at(&self.m, 64 + at)
    }
    fn u64(&self, at: usize) -> u64 {
        u64_at(&self.m, 64 + at)
    }
}

pub struct Smb2 {
    t: Box<dyn Transport + Send>,
    env: Box<dyn Env + Send>,
    server: String,
    msg_id: u64,
    session_id: u64,
    pub dialect: u16,
    server_mode: u16,
    signing: bool,
    key: [u8; 16],
    max_read: u32,
    max_write: u32,
    max_trans: u32,
    domain: String,
    ipc: Option<u32>,
}

impl Smb2 {
    /// A client on a connected transport. `next_msg_id` is 1 when an
    /// SMB1 negotiate already took message id 0.
    pub fn new(t: Box<dyn Transport + Send>, env: Box<dyn Env + Send>, server: &str, next_msg_id: u64) -> Smb2 {
        Smb2 {
            t,
            env,
            server: crate::wire::upper(server),
            msg_id: next_msg_id,
            session_id: 0,
            dialect: 0,
            server_mode: 0,
            signing: false,
            key: [0; 16],
            max_read: MAX_IO,
            max_write: MAX_IO,
            max_trans: MAX_IO,
            domain: String::new(),
            ipc: None,
        }
    }

    fn call(&mut self, cmd: u16, tid: u32, body: &[u8]) -> Result<Resp> {
        let id = self.msg_id;
        self.msg_id += 1;
        let mut h = Buf::new();
        h.bytes(b"\xfeSMB").u16(64).u16(if self.dialect >= DIALECT_210 { 1 } else { 0 }).u32(0).u16(cmd).u16(64);
        let sign = self.signing && cmd != NEGOTIATE && cmd != SESSION_SETUP;
        h.u32(if sign { FLAGS_SIGNED } else { 0 }).u32(0).u64(id).u32(0xFEFF).u32(tid).u64(self.session_id).zeros(16);
        h.bytes(body);
        if sign {
            let sig = crate::crypto::hmac_sha256(&self.key, &[&h.0]);
            h.0[48..64].copy_from_slice(&sig[..16]);
        }
        crate::send_frame(&mut *self.t, &h.0)?;
        loop {
            let m = crate::recv_frame(&mut *self.t)?;
            if m.len() < 64 || &m[..4] != b"\xfeSMB" {
                return Err(Error::Protocol("not an SMB2 response"));
            }
            let flags = u32_at(&m, 16);
            if flags & FLAGS_SERVER_TO_REDIR == 0 || u64_at(&m, 24) != id {
                // An oplock break or a stale answer.
                continue;
            }
            let status = u32_at(&m, 8);
            if status == st::PENDING && flags & FLAGS_ASYNC != 0 {
                continue;
            }
            return Ok(Resp { status, m });
        }
    }

    /// Call and fail on any status but success (and the ones in `ok`).
    fn ok(&mut self, cmd: u16, tid: u32, body: &[u8], ok: &[u32]) -> Result<Resp> {
        let r = self.call(cmd, tid, body)?;
        if r.status != st::SUCCESS && !ok.contains(&r.status) {
            return Err(Error::Status(r.status));
        }
        // An error response has a 9-byte body; anything else needs its own.
        if r.status == st::SUCCESS && r.m.len() < 66 {
            return Err(Error::Protocol("short response"));
        }
        Ok(r)
    }

    /// Send the SMB2 NEGOTIATE (2.0.2 and 2.1) and read the answer.
    pub fn negotiate(&mut self, client_guid: [u8; 16]) -> Result<()> {
        let mut b = Buf::new();
        b.u16(36).u16(2).u16(SIGNING_ENABLED).u16(0).u32(0).bytes(&client_guid).u64(0).u16(DIALECT_202).u16(DIALECT_210);
        let r = self.ok(NEGOTIATE, 0, &b.0, &[])?;
        self.negotiated(&r.m)
    }

    /// Take the dialect and limits from a NEGOTIATE response (also the
    /// one an SMB1 negotiate gets from an SMB2 server).
    pub fn negotiated(&mut self, m: &[u8]) -> Result<()> {
        let body = &m[64..];
        if u16_at(body, 0) != 65 {
            return Err(Error::Protocol("bad negotiate response"));
        }
        self.server_mode = u16_at(body, 2);
        self.dialect = u16_at(body, 4);
        self.max_trans = u32_at(body, 28).clamp(4096, MAX_IO);
        self.max_read = u32_at(body, 32).clamp(4096, MAX_IO);
        self.max_write = u32_at(body, 36).clamp(4096, MAX_IO);
        match self.dialect {
            DIALECT_202 | DIALECT_210 | DIALECT_WILDCARD => Ok(()),
            _ => Err(Error::NoDialect),
        }
    }

    /// NTLMSSP logon in two SESSION_SETUP rounds.
    pub fn session_setup(&mut self, cred: &Credentials) -> Result<()> {
        let mode = if self.server_mode & SIGNING_REQUIRED != 0 { SIGNING_REQUIRED } else { SIGNING_ENABLED };
        let setup = |blob: &[u8]| {
            let mut b = Buf::new();
            b.u16(25).u8(0).u8(mode as u8).u32(0).u32(0).u16(64 + 24).u16(blob.len() as u16).u64(0).bytes(blob);
            b.0
        };
        let r = self.call(SESSION_SETUP, 0, &setup(&ntlm::spnego_init(&ntlm::negotiate())))?;
        if r.status != st::MORE_PROCESSING_REQUIRED {
            return Err(Error::Status(if r.status == 0 { st::LOGON_FAILURE } else { r.status }));
        }
        self.session_id = u64_at(&r.m, 40);
        let blob = slice(&r.m, r.u16(4) as usize, r.u16(6) as usize);
        let ch = ntlm::parse_challenge(blob).ok_or(Error::Protocol("no NTLM challenge"))?;
        self.domain = ch.nb_domain.clone();
        let mut rnd = [0u8; 24];
        self.env.random(&mut rnd);
        let auth = ntlm::authenticate(cred, &ch, self.env.now(), &rnd);
        let r = self.call(SESSION_SETUP, 0, &setup(&ntlm::spnego_response(&auth.message)))?;
        if r.status != st::SUCCESS {
            self.session_id = 0;
            return Err(Error::Status(r.status));
        }
        let flags = r.u16(2);
        // Guest and anonymous sessions cannot sign.
        let guest = flags & 3 != 0 || cred.anonymous();
        self.key = auth.session_key;
        self.signing = self.server_mode & SIGNING_REQUIRED != 0 && !guest;
        Ok(())
    }

    fn tree_path(&self, share: &str) -> Vec<u8> {
        utf16(&alloc::format!("\\\\{}\\{}", self.server, share))
    }

    fn create(&mut self, tid: u32, path: &str, access: u32, attr: u32, share: u32, disp: u32, options: u32) -> Result<(FileId, FileAttrs, Action)> {
        let name = utf16(path);
        let mut b = Buf::new();
        b.u16(57).u8(0).u8(0).u32(2).u64(0).u64(0).u32(access).u32(attr).u32(share).u32(disp).u32(options);
        b.u16(64 + 56).u16(name.len() as u16).u32(0).u32(0);
        if name.is_empty() {
            b.u8(0);
        } else {
            b.bytes(&name);
        }
        let r = self.ok(CREATE, tid, &b.0, &[])?;
        let action = match r.u32(4) {
            2 => Action::Created,
            0 | 3 => Action::Overwritten,
            _ => Action::Opened,
        };
        let a = FileAttrs {
            created: r.u64(8),
            accessed: r.u64(16),
            written: r.u64(24),
            changed: r.u64(32),
            alloc: r.u64(40),
            size: r.u64(48),
            attr: r.u32(56),
            ea_size: 0,
        };
        let mut f = FileId::default();
        f.0.copy_from_slice(slice(&r.m, r.body(64), 16));
        Ok((f, a, action))
    }

    fn fid_body(size: u16, f: FileId) -> Vec<u8> {
        let mut b = Buf::new();
        b.u16(size).u16(0).u32(0).bytes(&f.0);
        b.0
    }

    fn query_info(&mut self, tid: u32, f: FileId, typ: u8, class: u8, out: u32) -> Result<Vec<u8>> {
        let mut b = Buf::new();
        b.u16(41).u8(typ).u8(class).u32(out).u16(0).u16(0).u32(0).u32(0).u32(1).bytes(&f.0).u8(0);
        let r = self.ok(QUERY_INFO, tid, &b.0, &[st::BUFFER_OVERFLOW])?;
        Ok(slice(&r.m, r.u16(2) as usize, r.u32(4) as usize).to_vec())
    }

    fn set_info(&mut self, tid: u32, f: FileId, typ: u8, class: u8, data: &[u8]) -> Result<()> {
        let mut b = Buf::new();
        b.u16(33).u8(typ).u8(class).u32(data.len() as u32).u16(64 + 32).u16(0).u32(0).bytes(&f.0).bytes(data);
        self.ok(SET_INFO, tid, &b.0, &[])?;
        Ok(())
    }

    /// Open `path`, run `f` on the handle, close it whatever happened.
    fn with_open<R>(
        &mut self,
        tid: u32,
        path: &str,
        access: u32,
        options: u32,
        f: impl FnOnce(&mut Self, FileId, &FileAttrs) -> Result<R>,
    ) -> Result<R> {
        let (fid, a, _) = self.create(tid, path, access | SYNCHRONIZE, 0, 7, 1, options)?;
        let r = f(self, fid, &a);
        let c = crate::Session::close(self, tid, fid);
        let v = r?;
        c?;
        Ok(v)
    }

    fn ipc(&mut self) -> Result<u32> {
        if let Some(t) = self.ipc {
            return Ok(t);
        }
        let t = crate::Session::tree_connect(self, "IPC$")?.id;
        self.ipc = Some(t);
        Ok(t)
    }

    /// One DCE/RPC call on a pipe of IPC$: write the request, read the
    /// whole response (FSCTL_PIPE_TRANSCEIVE, then READs while the server
    /// says there is more).
    fn transceive(&mut self, tid: u32, f: FileId, req: &[u8]) -> Result<Vec<u8>> {
        let mut b = Buf::new();
        b.u16(57).u16(0).u32(FSCTL_PIPE_TRANSCEIVE).bytes(&f.0).u32(64 + 56).u32(req.len() as u32).u32(0).u32(0).u32(0);
        b.u32(self.max_trans).u32(1).u32(0).bytes(req);
        let r = self.ok(IOCTL, tid, &b.0, &[st::BUFFER_OVERFLOW])?;
        let mut out = slice(&r.m, r.u32(32) as usize, r.u32(36) as usize).to_vec();
        let mut more = r.status == st::BUFFER_OVERFLOW;
        while more {
            let mut b = Buf::new();
            b.u16(49).u8(0x50).u8(0).u32(self.max_read).u64(0).bytes(&f.0).u32(0).u32(0).u32(0).u16(0).u16(0).u8(0);
            let r = self.ok(READ, tid, &b.0, &[st::BUFFER_OVERFLOW])?;
            out.extend_from_slice(slice(&r.m, r.m[64 + 2] as usize, r.u32(4) as usize));
            more = r.status == st::BUFFER_OVERFLOW;
        }
        Ok(out)
    }
}

fn access_of(a: &OpenArgs) -> u32 {
    let mut m = FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    if a.read {
        m |= FILE_READ_DATA | FILE_READ_EA | READ_CONTROL;
    }
    if a.write {
        m |= FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES;
    }
    if a.delete {
        m |= DELETE;
    }
    m
}

/// FileFullEaInformation list to EAs.
pub fn parse_full_ea(b: &[u8]) -> Vec<Ea> {
    let mut v = Vec::new();
    let mut at = 0;
    while at + 8 <= b.len() {
        let next = u32_at(b, at) as usize;
        let (flags, nlen, vlen) = (b[at + 4], b[at + 5] as usize, u16_at(b, at + 6) as usize);
        let name = slice(b, at + 8, nlen).to_vec();
        let value = slice(b, at + 9 + nlen, vlen).to_vec();
        v.push(Ea { flags, name, value });
        if next == 0 {
            break;
        }
        at += next;
    }
    v
}

/// EAs as a FileFullEaInformation list.
pub fn full_ea(eas: &[Ea]) -> Vec<u8> {
    let mut b = Buf::new();
    for (i, e) in eas.iter().enumerate() {
        let start = b.len();
        b.u32(0).u8(e.flags).u8(e.name.len() as u8).u16(e.value.len() as u16).bytes(&e.name).u8(0).bytes(&e.value);
        if i + 1 < eas.len() {
            b.align(4);
            let next = (b.len() - start) as u32;
            b.put_u32(start, next);
        }
    }
    b.0
}

impl crate::Session for Smb2 {
    fn dialect(&self) -> &'static str {
        if self.dialect == DIALECT_210 { "SMB 2.1" } else { "SMB 2.0.2" }
    }

    fn server_domain(&self) -> String {
        self.domain.clone()
    }

    fn max_io(&self) -> u32 {
        self.max_read.min(self.max_write)
    }

    fn tree_connect(&mut self, share: &str) -> Result<Tree> {
        let p = self.tree_path(share);
        let mut b = Buf::new();
        b.u16(9).u16(0).u16(64 + 8).u16(p.len() as u16).bytes(&p);
        let r = self.ok(TREE_CONNECT, 0, &b.0, &[])?;
        let kind = match r.m[64 + 2] {
            2 => ShareKind::Pipe,
            3 => ShareKind::Print,
            _ => ShareKind::Disk,
        };
        Ok(Tree { id: u32_at(&r.m, 36), kind })
    }

    fn tree_disconnect(&mut self, tid: u32) -> Result<()> {
        self.ok(TREE_DISCONNECT, tid, &[4, 0, 0, 0], &[])?;
        Ok(())
    }

    fn open(&mut self, tid: u32, path: &str, a: &OpenArgs) -> Result<(FileId, FileAttrs, Action)> {
        let options = match a.directory {
            Some(true) => FILE_DIRECTORY_FILE,
            Some(false) => FILE_NON_DIRECTORY_FILE,
            None => 0,
        };
        let share = a.share_read as u32 | (a.share_write as u32) << 1 | 4;
        self.create(tid, path, access_of(a), a.attr, share, a.disposition as u32, options)
    }

    fn close(&mut self, tid: u32, f: FileId) -> Result<()> {
        self.ok(CLOSE, tid, &Self::fid_body(24, f), &[])?;
        Ok(())
    }

    fn read(&mut self, tid: u32, f: FileId, off: u64, len: u32) -> Result<Vec<u8>> {
        let len = len.min(self.max_read);
        let mut b = Buf::new();
        b.u16(49).u8(0x50).u8(0).u32(len).u64(off).bytes(&f.0).u32(0).u32(0).u32(0).u16(0).u16(0).u8(0);
        let r = self.ok(READ, tid, &b.0, &[st::END_OF_FILE])?;
        if r.status == st::END_OF_FILE {
            return Ok(Vec::new());
        }
        Ok(slice(&r.m, r.m[64 + 2] as usize, r.u32(4) as usize).to_vec())
    }

    fn write(&mut self, tid: u32, f: FileId, off: u64, data: &[u8]) -> Result<u32> {
        let data = &data[..data.len().min(self.max_write as usize)];
        let mut b = Buf::new();
        b.u16(49).u16(64 + 48).u32(data.len() as u32).u64(off).bytes(&f.0).u32(0).u32(0).u16(0).u16(0).u32(0).bytes(data);
        let r = self.ok(WRITE, tid, &b.0, &[])?;
        Ok(r.u32(4))
    }

    fn flush(&mut self, tid: u32, f: FileId) -> Result<()> {
        self.ok(FLUSH, tid, &Self::fid_body(24, f), &[])?;
        Ok(())
    }

    fn stat(&mut self, tid: u32, f: FileId) -> Result<FileAttrs> {
        let b = self.query_info(tid, f, INFO_FILE, FILE_NETWORK_OPEN_INFORMATION, 56)?;
        let mut a = FileAttrs {
            created: u64_at(&b, 0),
            accessed: u64_at(&b, 8),
            written: u64_at(&b, 16),
            changed: u64_at(&b, 24),
            alloc: u64_at(&b, 32),
            size: u64_at(&b, 40),
            attr: u32_at(&b, 48),
            ea_size: 0,
        };
        if let Ok(e) = self.query_info(tid, f, INFO_FILE, FILE_EA_INFORMATION, 4) {
            a.ea_size = u32_at(&e, 0);
        }
        Ok(a)
    }

    fn stat_path(&mut self, tid: u32, path: &str) -> Result<FileAttrs> {
        self.with_open(tid, path, FILE_READ_ATTRIBUTES | FILE_READ_EA, 0, |s, f, a| {
            let mut a = a.clone();
            if let Ok(e) = s.query_info(tid, f, INFO_FILE, FILE_EA_INFORMATION, 4) {
                a.ea_size = u32_at(&e, 0);
            }
            Ok(a)
        })
    }

    fn list(&mut self, tid: u32, dir: &str, pattern: &str) -> Result<Vec<DirEntry>> {
        let pat = utf16(pattern);
        let out_len = self.max_trans;
        self.with_open(tid, dir, FILE_READ_DATA | FILE_READ_ATTRIBUTES, FILE_DIRECTORY_FILE, |s, f, _| {
            let mut v = Vec::new();
            let mut flags = 1u8; // RESTART_SCANS
            loop {
                let mut b = Buf::new();
                b.u16(33).u8(FILE_FULL_DIRECTORY_INFORMATION).u8(flags).u32(0).bytes(&f.0).u16(64 + 32).u16(pat.len() as u16).u32(out_len).bytes(&pat);
                let r = s.call(QUERY_DIRECTORY, tid, &b.0)?;
                match r.status {
                    st::SUCCESS => {}
                    st::NO_MORE_FILES | st::NO_SUCH_FILE => break,
                    e => return Err(Error::Status(e)),
                }
                flags = 0;
                let buf = slice(&r.m, r.u16(2) as usize, r.u32(4) as usize);
                let mut at = 0;
                while at + 68 <= buf.len() {
                    let next = u32_at(buf, at) as usize;
                    let name = from_utf16(slice(buf, at + 68, u32_at(buf, at + 60) as usize));
                    if name != "." && name != ".." {
                        v.push(DirEntry {
                            name,
                            attrs: FileAttrs {
                                created: u64_at(buf, at + 8),
                                accessed: u64_at(buf, at + 16),
                                written: u64_at(buf, at + 24),
                                changed: u64_at(buf, at + 32),
                                size: u64_at(buf, at + 40),
                                alloc: u64_at(buf, at + 48),
                                attr: u32_at(buf, at + 56),
                                ea_size: u32_at(buf, at + 64),
                            },
                        });
                    }
                    if next == 0 {
                        break;
                    }
                    at += next;
                }
            }
            Ok(v)
        })
    }

    fn set_basic(&mut self, tid: u32, f: FileId, sb: &SetBasic) -> Result<()> {
        let attr = sb.attr.map_or(0, |a| if a == 0 { crate::attr::NORMAL } else { a });
        let mut b = Buf::new();
        b.u64(sb.created).u64(sb.accessed).u64(sb.written).u64(0).u32(attr).u32(0);
        self.set_info(tid, f, INFO_FILE, FILE_BASIC_INFORMATION, &b.0)
    }

    fn set_basic_path(&mut self, tid: u32, path: &str, sb: &SetBasic) -> Result<()> {
        self.with_open(tid, path, FILE_WRITE_ATTRIBUTES | FILE_READ_ATTRIBUTES, 0, |s, f, _| s.set_basic(tid, f, sb))
    }

    fn set_size(&mut self, tid: u32, f: FileId, size: u64) -> Result<()> {
        self.set_info(tid, f, INFO_FILE, FILE_END_OF_FILE_INFORMATION, &size.to_le_bytes())
    }

    fn mkdir(&mut self, tid: u32, path: &str) -> Result<()> {
        let (f, _, _) = self.create(tid, path, FILE_READ_ATTRIBUTES | SYNCHRONIZE, 0, 7, 2, FILE_DIRECTORY_FILE)?;
        self.close(tid, f)
    }

    fn rmdir(&mut self, tid: u32, path: &str) -> Result<()> {
        let (f, _, _) = self.create(tid, path, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, 0, 7, 1, FILE_DIRECTORY_FILE)?;
        let r = self.set_info(tid, f, INFO_FILE, FILE_DISPOSITION_INFORMATION, &[1]);
        let c = self.close(tid, f);
        r?;
        c
    }

    fn delete(&mut self, tid: u32, path: &str) -> Result<()> {
        let (f, _, _) = self.create(tid, path, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, 0, 7, 1, FILE_NON_DIRECTORY_FILE | FILE_DELETE_ON_CLOSE)?;
        self.close(tid, f)
    }

    fn rename(&mut self, tid: u32, from: &str, to: &str) -> Result<()> {
        let name = utf16(to);
        let mut b = Buf::new();
        b.u8(0).zeros(7).u64(0).u32(name.len() as u32).bytes(&name);
        self.with_open(tid, from, DELETE | FILE_READ_ATTRIBUTES, 0, |s, f, _| s.set_info(tid, f, INFO_FILE, FILE_RENAME_INFORMATION, &b.0))
    }

    fn fs_size(&mut self, tid: u32) -> Result<FsSize> {
        self.with_open(tid, "", FILE_READ_ATTRIBUTES, 0, |s, f, _| {
            let b = s.query_info(tid, f, INFO_FILESYSTEM, FS_SIZE_INFORMATION, 24)?;
            Ok(FsSize { total_units: u64_at(&b, 0), free_units: u64_at(&b, 8), sectors_per_unit: u32_at(&b, 16), bytes_per_sector: u32_at(&b, 20) })
        })
    }

    fn eas(&mut self, tid: u32, path: &str) -> Result<Vec<Ea>> {
        let out = self.max_trans;
        self.with_open(tid, path, FILE_READ_EA | FILE_READ_ATTRIBUTES, 0, |s, f, _| {
            match s.query_info(tid, f, INFO_FILE, FILE_FULL_EA_INFORMATION, out) {
                Ok(b) => Ok(parse_full_ea(&b)),
                Err(Error::Status(st::NO_EAS_ON_FILE)) => Ok(Vec::new()),
                Err(e) => Err(e),
            }
        })
    }

    fn set_eas(&mut self, tid: u32, path: &str, eas: &[Ea]) -> Result<()> {
        if eas.is_empty() {
            return Ok(());
        }
        let data = full_ea(eas);
        self.with_open(tid, path, FILE_WRITE_EA | FILE_READ_ATTRIBUTES, 0, |s, f, _| s.set_info(tid, f, INFO_FILE, FILE_FULL_EA_INFORMATION, &data))
    }

    fn shares(&mut self) -> Result<Vec<ShareInfo>> {
        let tid = self.ipc()?;
        let (f, _, _) = self.create(tid, "srvsvc", FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE, 0, 3, 1, 0)?;
        let server = self.server.clone();
        let r = (|| {
            let ack = self.transceive(tid, f, &crate::rpc::bind_srvsvc(1))?;
            crate::rpc::check_bind_ack(&ack)?;
            let resp = self.transceive(tid, f, &crate::rpc::net_share_enum(2, &server))?;
            crate::rpc::parse_share_enum(&resp)
        })();
        let _ = self.close(tid, f);
        r
    }

    fn servers(&mut self, _domain: Option<&str>, _typ: u32) -> Result<Vec<ServerInfo>> {
        Ok(Vec::new())
    }

    fn echo(&mut self) -> Result<()> {
        self.ok(ECHO, 0, &[4, 0, 0, 0], &[])?;
        Ok(())
    }

    fn logoff(&mut self) -> Result<()> {
        self.ok(LOGOFF, 0, &[4, 0, 0, 0], &[])?;
        self.session_id = 0;
        Ok(())
    }
}
