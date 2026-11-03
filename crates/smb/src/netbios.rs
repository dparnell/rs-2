//! NetBIOS over TCP/IP (RFC 1001/1002): names, the session service's
//! SESSION REQUEST, NBNS name queries and node status, and LMHOSTS.

use alloc::string::String;
use alloc::vec::Vec;

use crate::wire::upper;

/// Name types (the 16th byte).
pub const WORKSTATION: u8 = 0x00;
pub const SERVER: u8 = 0x20;
pub const MASTER_BROWSER: u8 = 0x1D;
pub const BROWSER_ELECTION: u8 = 0x1E;

/// First-level encoding of a NetBIOS name: padded to 15 with blanks,
/// the type byte, each nibble as 'A' + n; with the length byte and the
/// empty scope.
pub fn encode_name(name: &str, typ: u8) -> Vec<u8> {
    let mut raw = [b' '; 16];
    if name == "*" {
        raw = [0; 16];
        raw[0] = b'*';
    } else {
        for (i, c) in crate::wire::oem(&upper(name)).iter().take(15).enumerate() {
            raw[i] = *c;
        }
    }
    raw[15] = typ;
    let mut v = Vec::with_capacity(34);
    v.push(32);
    for b in raw {
        v.push(b'A' + (b >> 4));
        v.push(b'A' + (b & 15));
    }
    v.push(0);
    v
}

/// SESSION REQUEST (type 0x81) for `called` (type 0x20) from `calling`.
pub fn session_request(called: &str, calling: &str) -> Vec<u8> {
    let mut body = encode_name(called, SERVER);
    body.extend_from_slice(&encode_name(calling, WORKSTATION));
    let mut v = alloc::vec![0x81, 0, 0, body.len() as u8];
    v.extend_from_slice(&body);
    v
}

/// The answer to a session request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionReply {
    Positive,
    /// Negative, with the error code (0x80 not listening on called name,
    /// 0x82 called name not present, 0x83 insufficient resources).
    Negative(u8),
    /// Retarget to another address and port.
    Retarget([u8; 4], u16),
    Other(u8),
}

pub fn parse_session_reply(hdr: &[u8; 4], body: &[u8]) -> SessionReply {
    match hdr[0] {
        0x82 => SessionReply::Positive,
        0x83 => SessionReply::Negative(body.first().copied().unwrap_or(0x8F)),
        0x84 if body.len() >= 6 => SessionReply::Retarget([body[0], body[1], body[2], body[3]], u16::from_be_bytes([body[4], body[5]])),
        t => SessionReply::Other(t),
    }
}

// ---- Name service (UDP 137) ----

/// A NAME QUERY REQUEST. `broadcast` sets the B flag; recursion is
/// desired for a unicast query to a WINS server.
pub fn name_query(id: u16, name: &str, typ: u8, broadcast: bool) -> Vec<u8> {
    let flags: u16 = if broadcast { 0x0110 } else { 0x0100 };
    let mut v = Vec::new();
    v.extend_from_slice(&id.to_be_bytes());
    v.extend_from_slice(&flags.to_be_bytes());
    v.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
    v.extend_from_slice(&encode_name(name, typ));
    v.extend_from_slice(&[0, 0x20, 0, 1]); // NB, IN
    v
}

/// A NODE STATUS REQUEST for `*` (any name) at a node.
pub fn node_status_query(id: u16) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&id.to_be_bytes());
    v.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    v.extend_from_slice(&encode_name("*", 0));
    v.extend_from_slice(&[0, 0x21, 0, 1]); // NBSTAT, IN
    v
}

fn skip_name(p: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let l = *p.get(at)? as usize;
        if l == 0 {
            return Some(at + 1);
        }
        if l & 0xC0 == 0xC0 {
            return Some(at + 2);
        }
        at += 1 + l;
    }
}

/// The addresses in a positive NAME QUERY RESPONSE with transaction `id`.
pub fn parse_query_response(p: &[u8], id: u16) -> Option<Vec<[u8; 4]>> {
    if p.len() < 12 || u16::from_be_bytes([p[0], p[1]]) != id || p[2] & 0x80 == 0 {
        return None;
    }
    if p[3] & 0x0F != 0 || u16::from_be_bytes([p[6], p[7]]) == 0 {
        return None;
    }
    let at = skip_name(p, 12)?;
    let rdlen = u16::from_be_bytes([*p.get(at + 8)?, *p.get(at + 9)?]) as usize;
    let rd = p.get(at + 10..at + 10 + rdlen)?;
    Some(rd.chunks_exact(6).map(|e| [e[2], e[3], e[4], e[5]]).collect())
}

/// A name registered at a node: name, type, group flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeName {
    pub name: String,
    pub typ: u8,
    pub group: bool,
}

/// The names of a NODE STATUS RESPONSE.
pub fn parse_node_status(p: &[u8], id: u16) -> Option<Vec<NodeName>> {
    if p.len() < 12 || u16::from_be_bytes([p[0], p[1]]) != id || p[2] & 0x80 == 0 {
        return None;
    }
    let at = skip_name(p, 12)?;
    let data = p.get(at + 10..)?;
    let n = *data.first()? as usize;
    let mut v = Vec::new();
    for i in 0..n {
        let e = data.get(1 + i * 18..1 + i * 18 + 18)?;
        let name = crate::wire::from_oem(&e[..15]).trim_end().into();
        v.push(NodeName { name, typ: e[15], group: e[16] & 0x80 != 0 });
    }
    Some(v)
}

// ---- LMHOSTS ----

/// One LMHOSTS entry: address, name, and the `#DOM:domain` tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LmHost {
    pub addr: [u8; 4],
    pub name: String,
    pub domain: Option<String>,
}

pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut a = [0u8; 4];
    let mut it = s.split('.');
    for x in a.iter_mut() {
        *x = it.next()?.parse().ok()?;
    }
    it.next().is_none().then_some(a)
}

/// Parse an LMHOSTS file: `address name [#PRE] [#DOM:domain]`, `#`
/// comments; names may be quoted.
pub fn parse_lmhosts(text: &str) -> Vec<LmHost> {
    let mut v = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(addr) = words.next().and_then(parse_ipv4) else { continue };
        let Some(name) = words.next() else { continue };
        let name = upper(name.trim_matches('"'));
        let mut domain = None;
        for w in words {
            if let Some(d) = w.strip_prefix("#DOM:").or_else(|| w.strip_prefix("#dom:")) {
                domain = Some(upper(d));
            }
        }
        v.push(LmHost { addr, name, domain });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_encoding() {
        // RFC 1001 14.1: "FRED" padded with blanks.
        let e = encode_name("Fred", 0x20);
        assert_eq!(e[0], 32);
        assert_eq!(&e[1..9], b"EGFCEFEE");
        assert_eq!(&e[31..33], b"CA");
        assert_eq!(*e.last().unwrap(), 0);
        let s = session_request("SMBHOST", "RS2");
        assert_eq!(s[0], 0x81);
        assert_eq!(s.len(), 4 + 68);
    }

    #[test]
    fn query_and_status() {
        let q = name_query(7, "SMBHOST", 0x20, true);
        assert_eq!(q[2..4], [0x01, 0x10]);
        // A positive response: header, name, type/class, TTL, rdlen 6, flags + address.
        let mut r = alloc::vec![0, 7, 0x85, 0x00, 0, 0, 0, 1, 0, 0, 0, 0];
        r.extend_from_slice(&encode_name("SMBHOST", 0x20));
        r.extend_from_slice(&[0, 0x20, 0, 1, 0, 0, 0, 0, 0, 6, 0, 0, 10, 0, 2, 2]);
        assert_eq!(parse_query_response(&r, 7), Some(alloc::vec![[10, 0, 2, 2]]));
        let mut s = alloc::vec![0, 9, 0x84, 0x00, 0, 0, 0, 1, 0, 0, 0, 0];
        s.extend_from_slice(&encode_name("*", 0));
        s.extend_from_slice(&[0, 0x21, 0, 1, 0, 0, 0, 0, 0, 37, 2]);
        let mut e = *b"SMBHOST        \x20\x04\x00";
        s.extend_from_slice(&e);
        e = *b"WORKGROUP      \x00\x84\x00";
        s.extend_from_slice(&e);
        let n = parse_node_status(&s, 9).unwrap();
        assert_eq!(n[0], NodeName { name: "SMBHOST".into(), typ: 0x20, group: false });
        assert_eq!(n[1], NodeName { name: "WORKGROUP".into(), typ: 0, group: true });
    }

    #[test]
    fn lmhosts() {
        let v = parse_lmhosts("# comment\n10.0.2.100  smbhost  #PRE #DOM:Workgroup\n192.168.1.5 \"other\"\nbad line\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "SMBHOST");
        assert_eq!(v[0].domain.as_deref(), Some("WORKGROUP"));
        assert_eq!(v[1].addr, [192, 168, 1, 5]);
    }
}
