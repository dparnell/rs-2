//! An SMB client for rs2's LAN requester, independent of the kernel:
//! messages go out and come in through a [`Transport`] (a connected
//! byte stream), time and random numbers come from an [`Env`]. The
//! kernel's redirector (`kernel/src/fs/redir`) runs it over its TCP
//! sockets; the host tests run it over std's TcpStream against
//! `tools/scripts/smbhost.py`.
//!
//! * [`crypto`]: MD4, MD5, SHA-256, HMAC, RC4.
//! * [`ntlm`]: NTLMSSP with NTLMv2 and SPNEGO.
//! * [`netbios`]: names, the session service, NBNS, LMHOSTS.
//! * [`smb2`]: SMB 2.0.2 and 2.1 (MS-SMB2), signing.
//! * [`smb1`]: SMB1 with the NT LM 0.12 dialect (MS-CIFS/MS-SMB) for
//!   servers that speak nothing newer.
//! * [`rpc`]: share lists: DCE/RPC srvsvc NetrShareEnum (SMB2) and RAP
//!   NetShareEnum/NetServerEnum2 (SMB1).
//! * [`connect`]: dialect negotiation and the [`Session`] trait both
//!   dialects implement.

#![no_std]

extern crate alloc;

pub mod crypto;
pub mod netbios;
pub mod ntlm;
pub mod rpc;
pub mod smb1;
pub mod smb2;
pub mod status;
pub mod wire;

mod connect;

pub use connect::{connect, Options};
pub use ntlm::Credentials;

use alloc::string::String;
use alloc::vec::Vec;

/// Errors of SMB operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The transport failed (an errno of the socket layer).
    Io(u32),
    /// The connection was closed by the server.
    Closed,
    /// The server answered with an NTSTATUS failure.
    Status(u32),
    /// An SMB1 DOS error class and code.
    Dos(u8, u16),
    /// Something unexpected on the wire.
    Protocol(&'static str),
    /// The server speaks no dialect we do.
    NoDialect,
    /// The session service refused the called name (RFC 1002 code).
    NetbiosRefused(u8),
}

impl Error {
    /// The OS/2 error code (ERROR_*) for this error.
    pub fn os2(&self) -> u32 {
        match *self {
            Error::Io(_) | Error::Closed => status::ERROR_UNEXP_NET_ERR,
            Error::Status(s) => status::nt_to_os2(s),
            Error::Dos(class, code) => status::dos_to_os2(class, code),
            Error::Protocol(_) => status::ERROR_UNEXP_NET_ERR,
            Error::NoDialect => status::ERROR_NOT_SUPPORTED,
            Error::NetbiosRefused(_) => status::ERROR_BAD_NETPATH,
        }
    }

    /// True if the connection is unusable after this error.
    pub fn fatal(&self) -> bool {
        matches!(self, Error::Io(_) | Error::Closed | Error::Protocol(_))
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// A connected, reliable byte stream (TCP).
pub trait Transport {
    fn send(&mut self, data: &[u8]) -> Result<()>;
    /// Read into `buf`, returning at least one byte (0 = closed).
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize>;
}

/// Time and randomness.
pub trait Env {
    /// The time now as a FILETIME (100 ns since 1601).
    fn now(&self) -> u64;
    fn random(&self, buf: &mut [u8]);
}

/// Read exactly `buf.len()` bytes.
pub fn recv_exact(t: &mut dyn Transport, buf: &mut [u8]) -> Result<()> {
    let mut at = 0;
    while at < buf.len() {
        let n = t.recv(&mut buf[at..])?;
        if n == 0 {
            return Err(Error::Closed);
        }
        at += n;
    }
    Ok(())
}

/// Send one message with the 4-byte session service header (type 0,
/// 24-bit length: direct TCP and NetBIOS alike).
pub fn send_frame(t: &mut dyn Transport, msg: &[u8]) -> Result<()> {
    let n = msg.len();
    let mut v = Vec::with_capacity(n + 4);
    v.extend_from_slice(&[0, (n >> 16) as u8, (n >> 8) as u8, n as u8]);
    v.extend_from_slice(msg);
    t.send(&v)
}

/// Receive one message, skipping session keepalives.
pub fn recv_frame(t: &mut dyn Transport) -> Result<Vec<u8>> {
    loop {
        let mut h = [0u8; 4];
        recv_exact(t, &mut h)?;
        // Direct TCP has a 24-bit length; other NetBIOS packets 17 bits.
        let high = if h[0] == 0 { h[1] as usize } else { h[1] as usize & 1 };
        let n = high << 16 | (h[2] as usize) << 8 | h[3] as usize;
        let mut v = alloc::vec![0u8; n];
        recv_exact(t, &mut v)?;
        if h[0] == 0x85 {
            continue;
        }
        if h[0] != 0 {
            return Err(Error::Protocol("unexpected session packet"));
        }
        return Ok(v);
    }
}

/// Do the NetBIOS session request on a fresh connection to port 139.
pub fn netbios_session(t: &mut dyn Transport, called: &str, calling: &str) -> Result<()> {
    t.send(&netbios::session_request(called, calling))?;
    let mut h = [0u8; 4];
    recv_exact(t, &mut h)?;
    let n = (h[2] as usize) << 8 | h[3] as usize;
    let mut body = alloc::vec![0u8; n];
    recv_exact(t, &mut body)?;
    match netbios::parse_session_reply(&h, &body) {
        netbios::SessionReply::Positive => Ok(()),
        netbios::SessionReply::Negative(e) => Err(Error::NetbiosRefused(e)),
        _ => Err(Error::Protocol("session request not accepted")),
    }
}

/// FILETIME of the Unix epoch.
pub const FILETIME_1970: u64 = 116_444_736_000_000_000;

/// File attributes as both dialects carry them (FILE_ATTRIBUTE_*).
pub mod attr {
    pub const READONLY: u32 = 0x01;
    pub const HIDDEN: u32 = 0x02;
    pub const SYSTEM: u32 = 0x04;
    pub const DIRECTORY: u32 = 0x10;
    pub const ARCHIVE: u32 = 0x20;
    pub const NORMAL: u32 = 0x80;
}

/// What the server says about a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileAttrs {
    pub attr: u32,
    pub size: u64,
    pub alloc: u64,
    /// FILETIMEs.
    pub created: u64,
    pub accessed: u64,
    pub written: u64,
    pub changed: u64,
    /// Size of the file's extended attributes (0: none).
    pub ea_size: u32,
}

impl FileAttrs {
    pub fn is_dir(&self) -> bool {
        self.attr & attr::DIRECTORY != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub attrs: FileAttrs,
}

/// Times and attributes to change (None or 0 leaves them alone).
#[derive(Clone, Debug, Default)]
pub struct SetBasic {
    pub created: u64,
    pub accessed: u64,
    pub written: u64,
    pub attr: Option<u32>,
}

/// An extended attribute: flags (0x80 = needed), name, value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ea {
    pub flags: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// What to do when the file exists or not (CreateDisposition).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Open; fail if missing.
    Open = 1,
    /// Create; fail if it exists.
    Create = 2,
    /// Open or create.
    OpenIf = 3,
    /// Truncate an existing file; fail if missing.
    Overwrite = 4,
    /// Truncate or create.
    OverwriteIf = 5,
}

/// Arguments of an open.
#[derive(Clone, Debug)]
pub struct OpenArgs {
    pub read: bool,
    pub write: bool,
    /// Also ask for DELETE access (renames, deletes).
    pub delete: bool,
    pub disposition: Disposition,
    /// Some(true): must be a directory; Some(false): must not be.
    pub directory: Option<bool>,
    /// Attributes for a new file.
    pub attr: u32,
    /// Sharing: allow others to read, write, delete.
    pub share_read: bool,
    pub share_write: bool,
}

impl OpenArgs {
    pub fn read() -> OpenArgs {
        OpenArgs {
            read: true,
            write: false,
            delete: false,
            disposition: Disposition::Open,
            directory: None,
            attr: 0,
            share_read: true,
            share_write: true,
        }
    }
    /// Just the attributes (stat, delete, rename).
    pub fn attributes() -> OpenArgs {
        OpenArgs { read: false, ..OpenArgs::read() }
    }
}

/// What a create did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Opened,
    Created,
    Overwritten,
}

/// A file handle on the server (SMB2: the 16-byte FileId; SMB1: the FID
/// in the first two bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileId(pub [u8; 16]);

/// A connected share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tree {
    pub id: u32,
    pub kind: ShareKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareKind {
    Disk,
    Pipe,
    Print,
}

/// A share as the share list gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareInfo {
    pub name: String,
    /// STYPE_*: 0 disk, 1 print queue, 2 device, 3 IPC; 0x80000000 special.
    pub typ: u32,
    pub remark: String,
}

/// A server as a browse list gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerInfo {
    pub name: String,
    pub version: (u8, u8),
    pub typ: u32,
    pub comment: String,
}

/// Size of a share's volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FsSize {
    pub total_units: u64,
    pub free_units: u64,
    pub sectors_per_unit: u32,
    pub bytes_per_sector: u32,
}

/// The operations of a logged-on SMB session (either dialect). Paths
/// are relative to the share, `\`-separated, without a leading `\`
/// (empty = the share's root).
pub trait Session: Send {
    /// "SMB 2.1", "SMB 2.0.2", "NT LM 0.12".
    fn dialect(&self) -> &'static str;
    /// The server's NetBIOS domain/workgroup from the logon.
    fn server_domain(&self) -> String;
    /// The largest read or write the session does in one request.
    fn max_io(&self) -> u32;
    fn tree_connect(&mut self, share: &str) -> Result<Tree>;
    fn tree_disconnect(&mut self, tid: u32) -> Result<()>;
    fn open(&mut self, tid: u32, path: &str, a: &OpenArgs) -> Result<(FileId, FileAttrs, Action)>;
    fn close(&mut self, tid: u32, f: FileId) -> Result<()>;
    /// Read up to `len` bytes (at most max_io); empty at the end.
    fn read(&mut self, tid: u32, f: FileId, off: u64, len: u32) -> Result<Vec<u8>>;
    /// Write (at most max_io bytes); the count written.
    fn write(&mut self, tid: u32, f: FileId, off: u64, data: &[u8]) -> Result<u32>;
    fn flush(&mut self, tid: u32, f: FileId) -> Result<()>;
    fn stat(&mut self, tid: u32, f: FileId) -> Result<FileAttrs>;
    fn stat_path(&mut self, tid: u32, path: &str) -> Result<FileAttrs>;
    /// The entries of directory `dir` matching `pattern` (`*` for all),
    /// without "." and "..".
    fn list(&mut self, tid: u32, dir: &str, pattern: &str) -> Result<Vec<DirEntry>>;
    fn set_basic(&mut self, tid: u32, f: FileId, b: &SetBasic) -> Result<()>;
    fn set_basic_path(&mut self, tid: u32, path: &str, b: &SetBasic) -> Result<()>;
    fn set_size(&mut self, tid: u32, f: FileId, size: u64) -> Result<()>;
    fn mkdir(&mut self, tid: u32, path: &str) -> Result<()>;
    fn rmdir(&mut self, tid: u32, path: &str) -> Result<()>;
    fn delete(&mut self, tid: u32, path: &str) -> Result<()>;
    fn rename(&mut self, tid: u32, from: &str, to: &str) -> Result<()>;
    fn fs_size(&mut self, tid: u32) -> Result<FsSize>;
    fn eas(&mut self, tid: u32, path: &str) -> Result<Vec<Ea>>;
    /// Set (or, with empty values, remove) the given EAs.
    fn set_eas(&mut self, tid: u32, path: &str, eas: &[Ea]) -> Result<()>;
    /// The server's shares (through IPC$).
    fn shares(&mut self) -> Result<Vec<ShareInfo>>;
    /// The servers the server knows of (SMB1 RAP NetServerEnum2; SMB2
    /// servers have no browse list: empty).
    fn servers(&mut self, domain: Option<&str>, typ: u32) -> Result<Vec<ServerInfo>>;
    fn echo(&mut self) -> Result<()>;
    fn logoff(&mut self) -> Result<()>;
}
