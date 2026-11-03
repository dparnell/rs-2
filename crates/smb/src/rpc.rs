//! Share and server lists: DCE/RPC (C706, connection-oriented, NDR) for
//! the srvsvc pipe of SMB2 servers, and the Remote Administration
//! Protocol (RAP, MS-RAP) of SMB1 servers' \PIPE\LANMAN.

use alloc::string::String;
use alloc::vec::Vec;

use crate::wire::{from_oem, from_utf16, slice, u16_at, u32_at, utf16_plain, Buf};
use crate::{Error, Result, ServerInfo, ShareInfo};

const SRVSVC: [u8; 16] = uuid(0x4b324fc8, 0x1670, 0x01d3, [0x12, 0x78, 0x5a, 0x47, 0xbf, 0x6e, 0xe1, 0x88]);
const NDR: [u8; 16] = uuid(0x8a885d04, 0x1ceb, 0x11c9, [0x9f, 0xe8, 0x08, 0x00, 0x2b, 0x10, 0x48, 0x60]);

const fn uuid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
}

fn header(ptype: u8, call_id: u32) -> Buf {
    let mut b = Buf::new();
    b.u8(5).u8(0).u8(ptype).u8(3).bytes(&[0x10, 0, 0, 0]).u16(0).u16(0).u32(call_id);
    b
}

fn finish(mut b: Buf) -> Vec<u8> {
    let n = b.len() as u16;
    b.put_u16(8, n);
    b.0
}

/// BIND to srvsvc 3.0 with NDR transfer syntax.
pub fn bind_srvsvc(call_id: u32) -> Vec<u8> {
    let mut b = header(11, call_id);
    b.u16(4280).u16(4280).u32(0).u8(1).zeros(3);
    b.u16(0).u8(1).u8(0).bytes(&SRVSVC).u16(3).u16(0).bytes(&NDR).u32(2);
    finish(b)
}

pub fn check_bind_ack(p: &[u8]) -> Result<()> {
    if p.len() < 24 || p[2] != 12 {
        return Err(Error::Protocol("RPC bind refused"));
    }
    let sec = u16_at(p, 24) as usize;
    let mut at = 26 + sec;
    at = (at + 3) & !3;
    if p.len() < at + 6 || u16_at(p, at + 4) != 0 {
        return Err(Error::Protocol("RPC bind refused"));
    }
    Ok(())
}

fn ndr_string(b: &mut Buf, s: &str) {
    let mut w = utf16_plain(s);
    w.extend_from_slice(&[0, 0]);
    let n = (w.len() / 2) as u32;
    b.u32(n).u32(0).u32(n).bytes(&w).align(4);
}

fn request(call_id: u32, opnum: u16, stub: &[u8]) -> Vec<u8> {
    let mut b = header(0, call_id);
    b.u32(stub.len() as u32).u16(0).u16(opnum).bytes(stub);
    finish(b)
}

/// NetrShareEnum (opnum 15), level 1, on `\\server`.
pub fn net_share_enum(call_id: u32, server: &str) -> Vec<u8> {
    let mut s = Buf::new();
    s.u32(0x20000);
    ndr_string(&mut s, &alloc::format!("\\\\{server}"));
    s.u32(1).u32(1).u32(0x20004).u32(0).u32(0).u32(0xFFFF_FFFF).u32(0x20008).u32(0);
    request(call_id, 15, &s.0)
}

/// The stub data of a (possibly fragmented) response.
fn response_stub(p: &[u8]) -> Result<Vec<u8>> {
    let mut stub = Vec::new();
    let mut at = 0;
    while at + 24 <= p.len() {
        let frag = u16_at(p, at + 8) as usize;
        match p[at + 2] {
            2 => {}
            3 => return Err(Error::Protocol("RPC fault")),
            _ => return Err(Error::Protocol("bad RPC response")),
        }
        let auth = u16_at(p, at + 10) as usize;
        let end = (at + frag).min(p.len());
        let data_end = end.saturating_sub(if auth > 0 { auth + 8 } else { 0 });
        stub.extend_from_slice(p.get(at + 24..data_end).unwrap_or(&[]));
        if p[at + 3] & 2 != 0 || frag == 0 {
            break;
        }
        at += frag;
    }
    Ok(stub)
}

struct Ndr<'a> {
    b: &'a [u8],
    at: usize,
}

impl Ndr<'_> {
    fn u32(&mut self) -> u32 {
        self.at = (self.at + 3) & !3;
        let v = u32_at(self.b, self.at);
        self.at += 4;
        v
    }
    fn string(&mut self) -> String {
        let _max = self.u32();
        let _off = self.u32();
        let n = self.u32() as usize;
        let s = from_utf16(slice(self.b, self.at, n * 2));
        self.at += n * 2;
        s
    }
}

/// The SHARE_INFO_1 entries of a NetrShareEnum response.
pub fn parse_share_enum(p: &[u8]) -> Result<Vec<ShareInfo>> {
    let stub = response_stub(p)?;
    let mut r = Ndr { b: &stub, at: 0 };
    let _level = r.u32();
    let _tag = r.u32();
    if r.u32() == 0 {
        return Ok(Vec::new());
    }
    let count = r.u32() as usize;
    if r.u32() == 0 {
        return Ok(Vec::new());
    }
    let max = r.u32() as usize;
    let mut ptrs = Vec::new();
    for _ in 0..max.min(count).min(4096) {
        let n = r.u32();
        let t = r.u32();
        let m = r.u32();
        ptrs.push((n, t, m));
    }
    let mut v = Vec::new();
    for (n, typ, m) in ptrs {
        let name = if n != 0 { r.string() } else { String::new() };
        let remark = if m != 0 { r.string() } else { String::new() };
        v.push(ShareInfo { name, typ, remark });
    }
    // The return code is the last ULONG.
    let werr = u32_at(&stub, stub.len().saturating_sub(4));
    if werr != 0 {
        return Err(Error::Status(crate::status::ACCESS_DENIED));
    }
    Ok(v)
}

// ---- RAP ----

/// Parameters of RAP NetShareEnum (0), level 1.
pub fn rap_share_enum(buflen: u16) -> Vec<u8> {
    let mut b = Buf::new();
    b.u16(0).bytes(b"WrLeh\0").bytes(b"B13BWz\0").u16(1).u16(buflen);
    b.0
}

/// Parameters of RAP NetServerEnum2 (104), level 1.
pub fn rap_server_enum2(buflen: u16, typ: u32, domain: Option<&str>) -> Vec<u8> {
    let mut b = Buf::new();
    b.u16(104).bytes(b"WrLehDz\0").bytes(b"B16BBDz\0").u16(1).u16(buflen).u32(typ);
    if let Some(d) = domain {
        b.bytes(&crate::wire::oem(d));
    }
    b.u8(0);
    b.0
}

fn rap_string(data: &[u8], ptr: u32, conv: u16) -> String {
    let off = (ptr & 0xFFFF).wrapping_sub(conv as u32) as usize;
    data.get(off..).map(from_oem).unwrap_or_default()
}

/// Status, entry count and converter of a RAP response's parameters.
fn rap_head(params: &[u8]) -> Result<(usize, u16)> {
    let status = u16_at(params, 0);
    // 234: more data (ERROR_MORE_DATA); keep what came.
    if status != 0 && status != 234 {
        return Err(Error::Dos(1, status));
    }
    Ok((u16_at(params, 4) as usize, u16_at(params, 2)))
}

pub fn parse_rap_shares(params: &[u8], data: &[u8]) -> Result<Vec<ShareInfo>> {
    let (n, conv) = rap_head(params)?;
    Ok((0..n)
        .filter_map(|i| {
            let e = data.get(i * 20..i * 20 + 20)?;
            Some(ShareInfo { name: from_oem(&e[..13]), typ: u16_at(e, 14) as u32, remark: rap_string(data, u32_at(e, 16), conv) })
        })
        .collect())
}

pub fn parse_rap_servers(params: &[u8], data: &[u8]) -> Result<Vec<ServerInfo>> {
    let (n, conv) = rap_head(params)?;
    Ok((0..n)
        .filter_map(|i| {
            let e = data.get(i * 26..i * 26 + 26)?;
            Some(ServerInfo { name: from_oem(&e[..16]), version: (e[16], e[17]), typ: u32_at(e, 18), comment: rap_string(data, u32_at(e, 22), conv) })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_enum_response() {
        // Build a response the way a server marshals it.
        let mut s = Buf::new();
        s.u32(1).u32(1).u32(0x20000).u32(2).u32(0x20004).u32(2);
        s.u32(0x20008).u32(0).u32(0x2000c);
        s.u32(0x20010).u32(0x8000_0003).u32(0x20014);
        ndr_string(&mut s, "DATA");
        ndr_string(&mut s, "files");
        ndr_string(&mut s, "IPC$");
        ndr_string(&mut s, "");
        s.u32(2).u32(0).u32(0);
        let mut p = header(2, 2);
        p.u32(s.len() as u32).u16(0).u8(0).u8(0).bytes(&s.0);
        let p = finish(p);
        let v = parse_share_enum(&p).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], ShareInfo { name: "DATA".into(), typ: 0, remark: "files".into() });
        assert_eq!(v[1].typ, 0x8000_0003);
        assert_eq!(bind_srvsvc(1).len(), 72);
    }

    #[test]
    fn rap_parse() {
        let mut data = alloc::vec![0u8; 20];
        data[..4].copy_from_slice(b"DATA");
        data[14] = 0;
        data[16..20].copy_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(b"files\0");
        let params = [0, 0, 0, 0, 1, 0, 1, 0];
        let v = parse_rap_shares(&params, &data).unwrap();
        assert_eq!(v[0].remark, "files");
    }
}
