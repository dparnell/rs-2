//! Setting up a session: NetBIOS session request (port 139), dialect
//! negotiation, logon.

use alloc::boxed::Box;
use alloc::string::String;

use crate::ntlm::Credentials;
use crate::smb1::{self, Smb1};
use crate::smb2::{self, Smb2};
use crate::{Env, Error, Result, Session, Transport};

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The server's name (NetBIOS name for port 139, and the name in
    /// tree paths).
    pub server: String,
    /// Do the RFC 1002 session request first (port 139).
    pub netbios: bool,
    /// Our NetBIOS name (the calling name).
    pub calling: String,
    pub cred: Credentials,
    /// Skip the SMB1 multi-protocol negotiate and send the SMB2 one
    /// (servers that drop SMB1 packets).
    pub smb2_only: bool,
    /// Do not offer SMB2 (testing the SMB1 path).
    pub smb1_only: bool,
    pub client_guid: [u8; 16],
}

/// Negotiate and log on over a connected transport.
pub fn connect(mut t: Box<dyn Transport + Send>, env: Box<dyn Env + Send>, o: &Options) -> Result<Box<dyn Session>> {
    if o.netbios {
        crate::netbios_session(&mut *t, &o.server, &o.calling)?;
    }
    if o.smb2_only {
        let mut s = Smb2::new(t, env, &o.server, 0);
        s.negotiate(o.client_guid)?;
        s.session_setup(&o.cred)?;
        return Ok(Box::new(s));
    }
    let mut neg = smb1::multi_negotiate();
    if o.smb1_only {
        // Offer only the SMB1 dialects (cut "SMB 2.002", "SMB 2.???").
        let cut = b"\x02SMB 2.002\0\x02SMB 2.???\0".len();
        neg.truncate(neg.len() - cut);
        let n = (neg.len() - 35) as u16;
        neg[33..35].copy_from_slice(&n.to_le_bytes());
    }
    crate::send_frame(&mut *t, &neg)?;
    let m = crate::recv_frame(&mut *t)?;
    if m.len() >= 64 && &m[..4] == b"\xfeSMB" {
        let mut s = Smb2::new(t, env, &o.server, 1);
        s.negotiated(&m)?;
        if s.dialect == smb2::DIALECT_WILDCARD {
            s.negotiate(o.client_guid)?;
        }
        s.session_setup(&o.cred)?;
        return Ok(Box::new(s));
    }
    if m.len() >= 35 && &m[..4] == b"\xffSMB" {
        if m[5] != 0 || m[32] == 1 {
            return Err(Error::NoDialect);
        }
        let n = smb1::parse_negotiate(&m)?;
        let mut s = Smb1::new(t, env, &o.server, n);
        s.session_setup(&o.cred)?;
        return Ok(Box::new(s));
    }
    Err(Error::Protocol("no SMB answer"))
}
