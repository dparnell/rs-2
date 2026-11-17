//! 802.11 information elements (IEEE 802.11-2020 9.4.2) and what rs2
//! keeps of a scanned network.

use alloc::string::String;
use alloc::vec::Vec;

pub const EID_SSID: u8 = 0;
pub const EID_SUPP_RATES: u8 = 1;
pub const EID_DS_PARAMS: u8 = 3;
pub const EID_RSN: u8 = 48;
pub const EID_EXT_RATES: u8 = 50;
pub const EID_HT_CAPABILITY: u8 = 45;
pub const EID_HT_OPERATION: u8 = 61;
pub const EID_BSS_COEX_2040: u8 = 72;
pub const EID_EXT_CAPABILITY: u8 = 127;
pub const EID_VHT_CAPABILITY: u8 = 191;
pub const EID_VHT_OPERATION: u8 = 192;
pub const EID_OPMODE_NOTIF: u8 = 199;
pub const EID_VENDOR: u8 = 221;

/// HT Capabilities and VHT Capabilities element sizes.
pub const HT_CAP_LEN: usize = 26;
pub const VHT_CAP_LEN: usize = 12;
pub const VHT_OPER_LEN: usize = 5;

/// Capability information bits.
pub const CAP_ESS: u16 = 1 << 0;
pub const CAP_IBSS: u16 = 1 << 1;
pub const CAP_PRIVACY: u16 = 1 << 4;

/// The elements of a beacon or probe response body: (id, contents).
/// Iteration stops at the first truncated element.
pub fn elements(mut b: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    core::iter::from_fn(move || {
        if b.len() < 2 || b.len() < 2 + b[1] as usize {
            return None;
        }
        let (id, n) = (b[0], b[1] as usize);
        let body = &b[2..2 + n];
        b = &b[2 + n..];
        Some((id, body))
    })
}

/// Cipher suite types (00-0F-AC:n in RSN, 00-50-F2:n in WPA).
pub const CIPHER_WEP40: u8 = 1;
pub const CIPHER_TKIP: u8 = 2;
pub const CIPHER_CCMP: u8 = 4;
pub const CIPHER_WEP104: u8 = 5;
pub const CIPHER_GCMP: u8 = 8;
/// AKM suite types.
pub const AKM_8021X: u8 = 1;
pub const AKM_PSK: u8 = 2;
pub const AKM_8021X_SHA256: u8 = 5;
pub const AKM_PSK_SHA256: u8 = 6;
pub const AKM_SAE: u8 = 8;

const OUI_RSN: [u8; 3] = [0x00, 0x0f, 0xac];
const OUI_MS: [u8; 3] = [0x00, 0x50, 0xf2];

/// A parsed RSN element (or the older WPA vendor element, which has the
/// same layout after its OUI and type).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Rsn {
    pub group: u8,
    /// Bit n: pairwise cipher type n is offered.
    pub pairwise: u32,
    /// Bit n: AKM type n is offered.
    pub akm: u32,
    pub caps: u16,
}

impl Rsn {
    /// The RSN element body (after id and length).
    pub fn parse(b: &[u8]) -> Option<Rsn> {
        Self::parse_suites(b, OUI_RSN)
    }

    /// The WPA vendor element body (after id and length), starting with
    /// 00-50-F2 01.
    pub fn parse_wpa(b: &[u8]) -> Option<Rsn> {
        if b.len() < 4 || b[..3] != OUI_MS || b[3] != 1 {
            return None;
        }
        Self::parse_suites(&b[4..], OUI_MS)
    }

    fn parse_suites(b: &[u8], oui: [u8; 3]) -> Option<Rsn> {
        // Version 1; every later field is optional (defaults: CCMP for
        // RSN, TKIP for WPA, 802.1X authentication).
        if b.len() < 2 || u16::from_le_bytes([b[0], b[1]]) != 1 {
            return None;
        }
        let dflt = if oui == OUI_RSN { CIPHER_CCMP } else { CIPHER_TKIP };
        let mut r = Rsn { group: dflt, pairwise: 1 << dflt, akm: 1 << AKM_8021X, caps: 0 };
        let mut p = &b[2..];
        let suite = |s: &[u8]| if s[..3] == oui { Some(s[3]) } else { None };
        if p.len() < 4 {
            return Some(r);
        }
        r.group = suite(&p[..4]).unwrap_or(0);
        p = &p[4..];
        for field in 0..2 {
            if p.len() < 2 {
                return Some(r);
            }
            let n = u16::from_le_bytes([p[0], p[1]]) as usize;
            p = &p[2..];
            if p.len() < n * 4 {
                return None;
            }
            let mut bits = 0u32;
            for s in p[..n * 4].chunks(4) {
                if let Some(t) = suite(s).filter(|&t| t < 32) {
                    bits |= 1 << t;
                }
            }
            if field == 0 { r.pairwise = bits } else { r.akm = bits }
            p = &p[n * 4..];
        }
        if p.len() >= 2 {
            r.caps = u16::from_le_bytes([p[0], p[1]]);
        }
        Some(r)
    }

    pub fn has_akm(&self, t: u8) -> bool {
        self.akm & 1 << t != 0
    }

    pub fn has_pairwise(&self, t: u8) -> bool {
        self.pairwise & 1 << t != 0
    }
}

/// What a network asks of a station that joins it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    Wep,
    /// WPA (version 1) with a passphrase.
    WpaPsk,
    /// WPA2 with a passphrase (CCMP).
    Wpa2Psk,
    /// WPA2 and WPA3 both offered (transition mode): WPA2 works.
    Wpa2Wpa3,
    /// WPA3 only (SAE): not supported by the 88W8897 firmware.
    Wpa3,
    /// WPA/WPA2 Enterprise (802.1X).
    Enterprise,
    /// Something else (an unknown AKM).
    Other,
}

impl Security {
    pub fn name(self) -> &'static str {
        match self {
            Security::Open => "open",
            Security::Wep => "WEP",
            Security::WpaPsk => "WPA",
            Security::Wpa2Psk => "WPA2",
            Security::Wpa2Wpa3 => "WPA2/WPA3",
            Security::Wpa3 => "WPA3",
            Security::Enterprise => "802.1X",
            Security::Other => "unknown",
        }
    }

    /// A passphrase is needed to join.
    pub fn needs_passphrase(self) -> bool {
        matches!(self, Security::WpaPsk | Security::Wpa2Psk | Security::Wpa2Wpa3 | Security::Wpa3)
    }
}

/// A network found by a scan: one access point (BSS).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bss {
    pub bssid: [u8; 6],
    pub ssid: Vec<u8>,
    pub channel: u8,
    /// Signal strength in dBm.
    pub rssi: i8,
    pub capability: u16,
    pub beacon_interval: u16,
    pub rsn: Option<Rsn>,
    pub wpa: Option<Rsn>,
    /// The elements as received (an association request repeats some).
    pub ies: Vec<u8>,
}

impl Bss {
    /// From the fixed fields and elements of a beacon or probe response.
    /// `channel` is used when the elements do not name one.
    pub fn new(bssid: [u8; 6], rssi: i8, beacon_interval: u16, capability: u16, ies: &[u8], channel: u8) -> Bss {
        let mut b = Bss {
            bssid,
            ssid: Vec::new(),
            channel,
            rssi,
            capability,
            beacon_interval,
            rsn: None,
            wpa: None,
            ies: ies.to_vec(),
        };
        let mut ds = None;
        let mut ht = None;
        for (id, body) in elements(ies) {
            match id {
                EID_SSID if body.len() <= 32 => b.ssid = body.to_vec(),
                EID_DS_PARAMS if !body.is_empty() => ds = Some(body[0]),
                EID_HT_OPERATION if !body.is_empty() => ht = Some(body[0]),
                EID_RSN => b.rsn = Rsn::parse(body),
                EID_VENDOR if b.wpa.is_none() => b.wpa = Rsn::parse_wpa(body),
                _ => {}
            }
        }
        if let Some(c) = ds.or(ht).filter(|&c| c != 0) {
            b.channel = c;
        }
        b
    }

    /// No SSID (or all zero bytes): a hidden network.
    pub fn hidden(&self) -> bool {
        self.ssid.iter().all(|&c| c == 0)
    }

    /// The SSID for display: UTF-8 where it is, other bytes and control
    /// characters as '?'.
    /// The contents of the first element `id`.
    pub fn element(&self, id: u8) -> Option<&[u8]> {
        elements(&self.ies).find(|e| e.0 == id).map(|e| e.1)
    }

    /// The WMM (QoS) parameter or information element: 802.11n needs QoS.
    pub fn wmm(&self) -> bool {
        elements(&self.ies).any(|(id, b)| id == EID_VENDOR && b.len() >= 5 && b[..4] == [0x00, 0x50, 0xf2, 0x02] && b[4] <= 1)
    }

    /// The fastest standard the network offers: "ac", "n" or "" (legacy).
    pub fn standard(&self) -> &'static str {
        if self.element(EID_VHT_CAPABILITY).is_some_and(|b| b.len() >= VHT_CAP_LEN) {
            "ac"
        } else if self.element(EID_HT_CAPABILITY).is_some_and(|b| b.len() >= HT_CAP_LEN) {
            "n"
        } else {
            ""
        }
    }

    pub fn ssid_text(&self) -> String {
        ssid_text(&self.ssid)
    }

    pub fn security(&self) -> Security {
        if let Some(r) = &self.rsn {
            let psk = r.has_akm(AKM_PSK) || r.has_akm(AKM_PSK_SHA256);
            let sae = r.has_akm(AKM_SAE);
            return match (psk, sae) {
                (true, true) => Security::Wpa2Wpa3,
                (true, false) => Security::Wpa2Psk,
                (false, true) => Security::Wpa3,
                _ if r.has_akm(AKM_8021X) || r.has_akm(AKM_8021X_SHA256) => Security::Enterprise,
                _ => Security::Other,
            };
        }
        if let Some(w) = &self.wpa {
            return if w.has_akm(AKM_PSK) { Security::WpaPsk } else { Security::Enterprise };
        }
        if self.capability & CAP_PRIVACY != 0 { Security::Wep } else { Security::Open }
    }

    /// 2.4 or 5 GHz.
    pub fn band(&self) -> &'static str {
        if self.channel > 14 { "5GHz" } else { "2.4GHz" }
    }
}

pub fn ssid_text(ssid: &[u8]) -> String {
    match core::str::from_utf8(ssid) {
        Ok(s) => s.chars().map(|c| if c.is_control() { '?' } else { c }).collect(),
        Err(_) => ssid.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '?' }).collect(),
    }
}

/// Merge scan results into `list`: one entry per BSSID (the newer one
/// wins), strongest signal first.
pub fn merge(list: &mut Vec<Bss>, new: Vec<Bss>) {
    for b in new {
        match list.iter_mut().find(|x| x.bssid == b.bssid) {
            Some(x) => *x = b,
            None => list.push(b),
        }
    }
    list.sort_by(|a, b| b.rssi.cmp(&a.rssi));
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn rsn_psk_ccmp() -> Vec<u8> {
        vec![
            48, 20, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 2, 0x0c, 0,
        ]
    }

    #[test]
    fn walks_elements() {
        let ies = [0, 3, b'a', b'b', b'c', 3, 1, 6, 99, 5, 1];
        let v: Vec<_> = elements(&ies).collect();
        assert_eq!(v, vec![(0, &b"abc"[..]), (3, &[6][..])]);
    }

    #[test]
    fn wpa2_network() {
        let mut ies = vec![0, 4, b'H', b'o', b'm', b'e', 1, 4, 0x82, 0x84, 0x8b, 0x96, 3, 1, 11];
        ies.extend(rsn_psk_ccmp());
        let b = Bss::new([2, 0, 0, 0, 0, 1], -52, 100, CAP_ESS | CAP_PRIVACY, &ies, 0);
        assert_eq!(b.ssid_text(), "Home");
        assert_eq!(b.channel, 11);
        assert_eq!(b.security(), Security::Wpa2Psk);
        let r = b.rsn.unwrap();
        assert_eq!(r.group, CIPHER_CCMP);
        assert!(r.has_pairwise(CIPHER_CCMP));
        assert_eq!(r.caps, 0x0c);
    }

    #[test]
    fn transition_mode_and_wpa3() {
        let rsn = |akms: &[u8]| {
            let mut v = vec![1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4, akms.len() as u8, 0];
            for &a in akms {
                v.extend([0x00, 0x0f, 0xac, a]);
            }
            v
        };
        let mk = |akms: &[u8]| {
            let body = rsn(akms);
            let mut ies = vec![48, body.len() as u8];
            ies.extend(body);
            Bss::new([0; 6], -60, 100, CAP_PRIVACY, &ies, 36)
        };
        assert_eq!(mk(&[AKM_PSK, AKM_SAE]).security(), Security::Wpa2Wpa3);
        assert_eq!(mk(&[AKM_SAE]).security(), Security::Wpa3);
        assert_eq!(mk(&[AKM_8021X]).security(), Security::Enterprise);
        assert_eq!(mk(&[AKM_SAE]).band(), "5GHz");
    }

    #[test]
    fn wpa1_wep_open_hidden() {
        let wpa = [221, 22, 0x00, 0x50, 0xf2, 1, 1, 0, 0x00, 0x50, 0xf2, 2, 1, 0, 0x00, 0x50, 0xf2, 2, 1, 0, 0x00, 0x50, 0xf2, 2];
        let b = Bss::new([0; 6], -70, 100, CAP_PRIVACY, &wpa, 6);
        assert_eq!(b.security(), Security::WpaPsk);
        assert_eq!(b.channel, 6);
        assert_eq!(Bss::new([0; 6], -70, 100, CAP_PRIVACY, &[], 1).security(), Security::Wep);
        let open = Bss::new([0; 6], -70, 100, CAP_ESS, &[0, 0], 1);
        assert_eq!(open.security(), Security::Open);
        assert!(open.hidden());
    }

    #[test]
    fn short_rsn_defaults() {
        assert_eq!(Rsn::parse(&[1, 0]).unwrap().group, CIPHER_CCMP);
        assert_eq!(Rsn::parse(&[2, 0]), None);
        assert_eq!(Rsn::parse(&[1, 0, 0, 0x0f, 0xac, 4, 5, 0]), None);
    }

    #[test]
    fn merge_keeps_one_per_bssid() {
        let a = Bss::new([1; 6], -80, 100, 0, &[0, 1, b'a'], 1);
        let b = Bss::new([2; 6], -40, 100, 0, &[0, 1, b'b'], 1);
        let a2 = Bss::new([1; 6], -30, 100, 0, &[0, 1, b'a'], 1);
        let mut l = Vec::new();
        merge(&mut l, vec![a, b]);
        merge(&mut l, vec![a2]);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].rssi, -30);
    }

    #[test]
    fn ssid_display() {
        assert_eq!(ssid_text("Café".as_bytes()), "Café");
        assert_eq!(ssid_text(&[b'a', 0xff, 1]), "a??");
    }
}
