//! NTLMSSP (MS-NLMP) with NTLMv2 responses, and the bits of SPNEGO
//! (RFC 4178) that carry it in SMB session setups.

use alloc::string::String;
use alloc::vec::Vec;

use crate::crypto::{hmac_md5, md4, rc4};
use crate::wire::{cp_to_unicode, u16_at, u32_at, Buf};

pub const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
pub const NEGOTIATE_OEM: u32 = 0x0000_0002;
pub const REQUEST_TARGET: u32 = 0x0000_0004;
pub const NEGOTIATE_SIGN: u32 = 0x0000_0010;
pub const NEGOTIATE_NTLM: u32 = 0x0000_0200;
pub const NEGOTIATE_ANONYMOUS: u32 = 0x0000_0800;
pub const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
pub const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
pub const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
pub const NEGOTIATE_128: u32 = 0x2000_0000;
pub const NEGOTIATE_KEY_EXCH: u32 = 0x4000_0000;
pub const NEGOTIATE_56: u32 = 0x8000_0000;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

/// Who logs on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credentials {
    pub user: String,
    pub password: String,
    pub domain: String,
    /// This machine's name (the workstation field).
    pub workstation: String,
}

impl Credentials {
    pub fn anonymous(&self) -> bool {
        self.user.is_empty()
    }
}

/// UTF-16LE of an rs2 string, optionally upper-cased (in Unicode).
fn unicode(s: &str, upper: bool) -> Vec<u8> {
    let mut v = Vec::new();
    for c in s.chars() {
        let u = if (c as u32) < 0x100 { cp_to_unicode(c as u32 as u8) } else { c as u32 as u16 };
        let ch = char::from_u32(u as u32).unwrap_or('?');
        if upper {
            for x in ch.to_uppercase() {
                let mut b = [0u16; 2];
                for w in x.encode_utf16(&mut b) {
                    v.extend_from_slice(&w.to_le_bytes());
                }
            }
        } else {
            v.extend_from_slice(&u.to_le_bytes());
        }
    }
    v
}

/// NTOWFv2: HMAC-MD5 of the user and domain, keyed with the NT hash.
pub fn ntowf_v2(password: &str, user: &str, domain: &str) -> [u8; 16] {
    let nt = md4(&unicode(password, false));
    hmac_md5(&nt, &[&unicode(user, true), &unicode(domain, false)])
}

/// LMv2 and NTLMv2 responses and the session base key for a server
/// challenge (also used by SMB1 logons without NTLMSSP).
pub fn responses(cred: &Credentials, challenge: &[u8; 8], target_info: &[u8], time: u64, client_challenge: [u8; 8]) -> (Vec<u8>, Vec<u8>, [u8; 16]) {
    let key = ntowf_v2(&cred.password, &cred.user, &cred.domain);
    let mut temp = Buf::new();
    temp.u8(1).u8(1).u16(0).u32(0).u64(time).bytes(&client_challenge).u32(0).bytes(target_info).u32(0);
    let proof = hmac_md5(&key, &[challenge, &temp.0]);
    let mut nt = proof.to_vec();
    nt.extend_from_slice(&temp.0);
    let mut lm = hmac_md5(&key, &[challenge, &client_challenge]).to_vec();
    lm.extend_from_slice(&client_challenge);
    (lm, nt, hmac_md5(&key, &[&proof]))
}

/// The NEGOTIATE_MESSAGE.
pub fn negotiate() -> Vec<u8> {
    let flags = NEGOTIATE_UNICODE
        | NEGOTIATE_OEM
        | REQUEST_TARGET
        | NEGOTIATE_SIGN
        | NEGOTIATE_NTLM
        | NEGOTIATE_ALWAYS_SIGN
        | NEGOTIATE_EXTENDED_SESSIONSECURITY
        | NEGOTIATE_TARGET_INFO
        | NEGOTIATE_128
        | NEGOTIATE_KEY_EXCH
        | NEGOTIATE_56;
    let mut b = Buf::new();
    b.bytes(SIGNATURE).u32(1).u32(flags);
    // Domain and workstation: empty, offset past the header.
    b.u16(0).u16(0).u32(32).u16(0).u16(0).u32(32);
    b.0
}

/// What the server's CHALLENGE_MESSAGE says.
#[derive(Clone, Debug, Default)]
pub struct Challenge {
    pub flags: u32,
    pub challenge: [u8; 8],
    pub target_info: Vec<u8>,
    /// The server's NetBIOS domain and computer names from the target info.
    pub nb_domain: String,
    pub nb_computer: String,
    pub timestamp: Option<u64>,
}

/// Parse a CHALLENGE_MESSAGE (found anywhere in `blob`, so SPNEGO wrapping
/// need not be decoded exactly).
pub fn parse_challenge(blob: &[u8]) -> Option<Challenge> {
    let at = blob.windows(8).position(|w| w == SIGNATURE)?;
    let m = &blob[at..];
    if u32_at(m, 8) != 2 || m.len() < 32 {
        return None;
    }
    let mut c = Challenge { flags: u32_at(m, 20), ..Default::default() };
    c.challenge.copy_from_slice(&m[24..32]);
    if m.len() >= 48 {
        let (len, off) = (u16_at(m, 40) as usize, u32_at(m, 44) as usize);
        c.target_info = m.get(off..off + len).unwrap_or(&[]).to_vec();
    }
    let ti = &c.target_info;
    let mut i = 0;
    while i + 4 <= ti.len() {
        let (id, len) = (u16_at(ti, i), u16_at(ti, i + 2) as usize);
        let v = ti.get(i + 4..i + 4 + len).unwrap_or(&[]);
        match id {
            0 => break,
            1 => c.nb_computer = crate::wire::from_utf16(v),
            2 => c.nb_domain = crate::wire::from_utf16(v),
            7 if v.len() == 8 => c.timestamp = Some(crate::wire::u64_at(v, 0)),
            _ => {}
        }
        i += 4 + len;
    }
    Some(c)
}

/// The AUTHENTICATE_MESSAGE and the key the session signs with.
pub struct Authenticate {
    pub message: Vec<u8>,
    pub session_key: [u8; 16],
}

/// Build the AUTHENTICATE_MESSAGE for `cred` answering `ch`. `now` is the
/// time as a FILETIME (used when the server sent none), `random` 24 random
/// bytes (8 for the client challenge, 16 for the exported session key).
pub fn authenticate(cred: &Credentials, ch: &Challenge, now: u64, random: &[u8; 24]) -> Authenticate {
    let client_challenge: [u8; 8] = random[..8].try_into().unwrap();
    let mut flags = ch.flags
        & (NEGOTIATE_UNICODE
            | NEGOTIATE_SIGN
            | NEGOTIATE_NTLM
            | NEGOTIATE_ALWAYS_SIGN
            | NEGOTIATE_EXTENDED_SESSIONSECURITY
            | NEGOTIATE_TARGET_INFO
            | NEGOTIATE_128
            | NEGOTIATE_KEY_EXCH
            | NEGOTIATE_56);
    if flags & NEGOTIATE_UNICODE == 0 {
        flags |= NEGOTIATE_OEM;
    }
    let (lm, nt, session_key) = if cred.anonymous() {
        flags |= NEGOTIATE_ANONYMOUS;
        flags &= !NEGOTIATE_KEY_EXCH;
        (alloc::vec![0u8], Vec::new(), [0u8; 16])
    } else {
        let (mut lm, nt, key) = responses(cred, &ch.challenge, &ch.target_info, ch.timestamp.unwrap_or(now), client_challenge);
        if ch.timestamp.is_some() {
            lm = alloc::vec![0u8; 24];
        }
        (lm, nt, key)
    };
    let (exported, encrypted) = if flags & NEGOTIATE_KEY_EXCH != 0 {
        let r: [u8; 16] = random[8..24].try_into().unwrap();
        let mut e = r;
        rc4(&session_key, &mut e);
        (r, e.to_vec())
    } else {
        (session_key, Vec::new())
    };
    let unicode_names = flags & NEGOTIATE_UNICODE != 0;
    let name = |s: &str| if unicode_names { unicode(s, false) } else { crate::wire::oem(&crate::wire::upper(s)) };
    let fields: [Vec<u8>; 6] = [lm, nt, name(&cred.domain), name(&cred.user), name(&cred.workstation), encrypted];
    let mut b = Buf::new();
    b.bytes(SIGNATURE).u32(3);
    // Payload order: LM, NT, domain, user, workstation, session key.
    let header = 64;
    let mut off = header;
    let mut descr = Vec::new();
    for f in &fields {
        descr.push((f.len() as u16, off as u32));
        off += f.len();
    }
    for (i, &(len, at)) in descr.iter().enumerate() {
        // On the wire: LM, NT, domain, user, workstation, key; then flags.
        let _ = i;
        b.u16(len).u16(len).u32(at);
    }
    b.u32(flags);
    for f in &fields {
        b.bytes(f);
    }
    Authenticate { message: b.0, session_key: exported }
}

// ---- SPNEGO ----

const OID_SPNEGO: &[u8] = &[0x06, 0x06, 0x2b, 0x06, 0x01, 0x05, 0x05, 0x02];
const OID_NTLMSSP: &[u8] = &[0x06, 0x0a, 0x2b, 0x06, 0x01, 0x04, 0x01, 0x82, 0x37, 0x02, 0x02, 0x0a];

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = alloc::vec![tag];
    let n = content.len();
    if n < 0x80 {
        v.push(n as u8);
    } else if n < 0x100 {
        v.extend_from_slice(&[0x81, n as u8]);
    } else {
        v.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
    v.extend_from_slice(content);
    v
}

/// NegTokenInit offering NTLMSSP, with the NTLM NEGOTIATE_MESSAGE.
pub fn spnego_init(ntlm: &[u8]) -> Vec<u8> {
    let mech_list = der(0xa0, &der(0x30, OID_NTLMSSP));
    let token = der(0xa2, &der(0x04, ntlm));
    let mut seq = mech_list;
    seq.extend_from_slice(&token);
    let init = der(0xa0, &der(0x30, &seq));
    let mut inner = OID_SPNEGO.to_vec();
    inner.extend_from_slice(&init);
    der(0x60, &inner)
}

/// NegTokenResp carrying the NTLM AUTHENTICATE_MESSAGE.
pub fn spnego_response(ntlm: &[u8]) -> Vec<u8> {
    der(0xa1, &der(0x30, &der(0xa2, &der(0x04, ntlm))))
}

/// True if a security blob from the server advertises NTLMSSP (or is
/// empty: servers may send nothing and still take NTLMSSP).
pub fn offers_ntlmssp(blob: &[u8]) -> bool {
    blob.is_empty() || blob.windows(OID_NTLMSSP.len() - 2).any(|w| w == &OID_NTLMSSP[2..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| alloc::format!("{x:02x}")).collect()
    }

    /// MS-NLMP 4.2.4 (NTLMv2 authentication) test vectors.
    #[test]
    fn ntlmv2_vectors() {
        let key = ntowf_v2("Password", "User", "Domain");
        assert_eq!(hex(&key), "0c868a403bfd7a93a3001ef22ef02e3f");
        let server_challenge = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let client_challenge = [0xaa; 8];
        let lm = hmac_md5(&key, &[&server_challenge, &client_challenge]);
        assert_eq!(hex(&lm), "86c35097ac9cec102554764a57cccc19");
        // Target info: domain "Domain", server "Server", EOL.
        let mut ti = Buf::new();
        ti.u16(2).u16(12).bytes(&unicode("Domain", false)).u16(1).u16(12).bytes(&unicode("Server", false)).u16(0).u16(0);
        let mut temp = Buf::new();
        temp.u8(1).u8(1).u16(0).u32(0).u64(0).bytes(&client_challenge).u32(0).bytes(&ti.0).u32(0);
        let proof = hmac_md5(&key, &[&server_challenge, &temp.0]);
        assert_eq!(hex(&proof), "68cd0ab851e51c96aabc927bebef6a1c");
        let session = hmac_md5(&key, &[&proof]);
        assert_eq!(hex(&session), "8de40ccadbc14a82f15cb0ad0de95ca3");
    }

    #[test]
    fn challenge_round_trip() {
        let mut ti = Buf::new();
        ti.u16(2).u16(4).bytes(&unicode("WG", false)).u16(1).u16(6).bytes(&unicode("SRV", false)).u16(7).u16(8).u64(42).u16(0).u16(0);
        let mut m = Buf::new();
        m.bytes(SIGNATURE).u32(2).u16(0).u16(0).u32(48).u32(NEGOTIATE_UNICODE | NEGOTIATE_KEY_EXCH).bytes(&[9; 8]).u64(0);
        m.u16(ti.len() as u16).u16(ti.len() as u16).u32(48).bytes(&ti.0);
        let wrapped = spnego_response(&m.0);
        let c = parse_challenge(&wrapped).unwrap();
        assert_eq!(c.challenge, [9; 8]);
        assert_eq!(c.nb_domain, "WG");
        assert_eq!(c.nb_computer, "SRV");
        assert_eq!(c.timestamp, Some(42));
        let cred = Credentials { user: "u".into(), password: "p".into(), domain: "WG".into(), workstation: "RS2".into() };
        let a = authenticate(&cred, &c, 0, &[7; 24]);
        assert_eq!(&a.message[..8], SIGNATURE);
        assert_eq!(a.session_key, [7; 16]);
        assert!(offers_ntlmssp(&spnego_init(&negotiate())));
    }
}
