//! DNS messages (RFC 1035): A queries and the parsing of their answers,
//! with name compression and CNAME chains. The stack's resolver
//! (`stack::Stack::resolve_start`) sends the queries, retries them and
//! caches the results.

use alloc::string::String;
use alloc::vec::Vec;

use crate::wire::{be16, be32};
use crate::Ipv4;

pub const PORT: u16 = 53;
pub const TYPE_A: u16 = 1;
pub const TYPE_CNAME: u16 = 5;
pub const CLASS_IN: u16 = 1;

// Response codes.
pub const NOERROR: u8 = 0;
pub const FORMERR: u8 = 1;
pub const SERVFAIL: u8 = 2;
pub const NXDOMAIN: u8 = 3;

/// Encode `name` as labels; None if it is not a valid host name.
pub fn encode_name(name: &str, out: &mut Vec<u8>) -> Option<()> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty() || name.len() > 253 {
        return None;
    }
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Some(())
}

/// A recursive query for the A records of `name`.
pub fn query(id: u16, name: &str) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&0x0100u16.to_be_bytes()); // RD
    q.extend_from_slice(&1u16.to_be_bytes());
    q.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    encode_name(name, &mut q)?;
    q.extend_from_slice(&TYPE_A.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    Some(q)
}

/// Read a (possibly compressed) name at `pos`; returns it and the
/// position after it in the message.
pub fn read_name(m: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *m.get(pos)? as usize;
        match len & 0xc0 {
            0xc0 => {
                let target = (be16(m, pos) & 0x3fff) as usize;
                if m.len() < pos + 2 {
                    return None;
                }
                end.get_or_insert(pos + 2);
                jumps += 1;
                if jumps > 32 || target >= m.len() {
                    return None;
                }
                pos = target;
            }
            0 => {
                if len == 0 {
                    return Some((name, end.unwrap_or(pos + 1)));
                }
                let label = m.get(pos + 1..pos + 1 + len)?;
                if !name.is_empty() {
                    name.push('.');
                }
                name.extend(label.iter().map(|&c| c as char));
                if name.len() > 255 {
                    return None;
                }
                pos += 1 + len;
            }
            _ => return None,
        }
    }
}

/// The answer to a query.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Answer {
    pub rcode: u8,
    /// A records of the name (after following CNAMEs) and their TTLs.
    pub addrs: Vec<(Ipv4, u32)>,
    /// The canonical name, if a CNAME was followed.
    pub cname: Option<String>,
}

/// Parse a response to query `id` for `name`. None if it is not one.
pub fn parse_response(m: &[u8], id: u16, name: &str) -> Option<Answer> {
    if m.len() < 12 || be16(m, 0) != id {
        return None;
    }
    let flags = be16(m, 2);
    if flags & 0x8000 == 0 {
        return None;
    }
    let qd = be16(m, 4) as usize;
    let an = be16(m, 6) as usize;
    let mut pos = 12;
    let mut asked = None;
    for _ in 0..qd {
        let (n, p) = read_name(m, pos)?;
        asked.get_or_insert(n);
        pos = p + 4;
    }
    let want = name.strip_suffix('.').unwrap_or(name);
    if asked.as_deref().is_some_and(|a| !a.eq_ignore_ascii_case(want)) {
        return None;
    }
    let mut ans = Answer { rcode: (flags & 0xf) as u8, ..Default::default() };
    let mut target = String::from(want);
    let mut records = Vec::new();
    for _ in 0..an {
        let (n, p) = read_name(m, pos)?;
        if m.len() < p + 10 {
            return None;
        }
        let (typ, class, ttl, len) = (be16(m, p), be16(m, p + 2), be32(m, p + 4), be16(m, p + 8) as usize);
        let data = p + 10;
        if m.len() < data + len {
            return None;
        }
        records.push((n, typ, class, ttl, data, len));
        pos = data + len;
    }
    // Follow CNAMEs (in any order), then collect the A records.
    for _ in 0..8 {
        let next = records.iter().find(|r| r.1 == TYPE_CNAME && r.2 == CLASS_IN && r.0.eq_ignore_ascii_case(&target));
        match next {
            Some(r) => {
                target = read_name(m, r.4)?.0;
                ans.cname = Some(target.clone());
            }
            None => break,
        }
    }
    for r in &records {
        if r.1 == TYPE_A && r.2 == CLASS_IN && r.5 == 4 && r.0.eq_ignore_ascii_case(&target) {
            ans.addrs.push((Ipv4([m[r.4], m[r.4 + 1], m[r.4 + 2], m[r.4 + 3]]), r.3));
        }
    }
    Some(ans)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A response to `q` with a CNAME (compressed) and two A records.
    fn response(q: &[u8]) -> Vec<u8> {
        let mut r = q.to_vec();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 3; // ANCOUNT
        // www.example.com CNAME web.example.com (name pointer to offset 12).
        r.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60]);
        let cname = [3, b'w', b'e', b'b', 0xc0, 16];
        r.extend_from_slice(&(cname.len() as u16).to_be_bytes());
        let at = r.len();
        r.extend_from_slice(&cname);
        for ip in [[93, 184, 216, 34], [93, 184, 216, 35]] {
            r.extend_from_slice(&[0xc0, at as u8, 0, 1, 0, 1, 0, 0, 1, 0, 0, 4]);
            r.extend_from_slice(&ip);
        }
        r
    }

    #[test]
    fn query_and_answer() {
        let q = query(0x4242, "www.example.com").unwrap();
        assert_eq!(&q[12..17], &[3, b'w', b'w', b'w', 7]);
        let a = parse_response(&response(&q), 0x4242, "www.example.com").unwrap();
        assert_eq!(a.rcode, NOERROR);
        assert_eq!(a.cname.as_deref(), Some("web.example.com"));
        assert_eq!(a.addrs, alloc::vec![(Ipv4::new(93, 184, 216, 34), 256), (Ipv4::new(93, 184, 216, 35), 256)]);
        assert!(parse_response(&response(&q), 0x4243, "www.example.com").is_none());
    }

    #[test]
    fn bad_names_and_loops() {
        assert!(query(1, "").is_none());
        assert!(query(1, "a..b").is_none());
        // A pointer to itself.
        let m = [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 12];
        assert!(read_name(&m, 12).is_none());
    }
}
