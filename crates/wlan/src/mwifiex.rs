//! The host interface of Marvell's full-MAC WiFi firmware (88W8897,
//! 88W8997 and relatives; Linux `drivers/net/wireless/marvell/mwifiex`,
//! `fw.h`), without the bus: what goes in a command and comes back in a
//! response or event. The PCIe transport is `kernel/src/drivers/mwifiex.rs`.
//!
//! Everything the firmware and the host exchange starts with a 4-byte
//! interface header: length (including the header) and type ([`TYPE_CMD`],
//! [`TYPE_DATA`], [`TYPE_EVENT`]). A command is an 8-byte header
//! (command, size, sequence number, result) and a body; the response has
//! the same header with [`RET_BIT`] set in the command and the result
//! filled in. Variable parts are TLVs: type, length (both u16 LE), value.

use alloc::string::String;
use alloc::vec::Vec;

use crate::ie::Bss;

/// Interface header: u16 length, u16 type.
pub const INTF_HDR: usize = 4;
pub const TYPE_DATA: u16 = 0;
pub const TYPE_CMD: u16 = 1;
pub const TYPE_EVENT: u16 = 3;

/// Command header (`S_DS_GEN`).
pub const CMD_HDR: usize = 8;
pub const RET_BIT: u16 = 0x8000;

pub const CMD_GET_HW_SPEC: u16 = 0x0003;
pub const CMD_802_11_SCAN: u16 = 0x0006;
pub const CMD_802_11_ASSOCIATE: u16 = 0x0012;
pub const CMD_802_11_SNMP_MIB: u16 = 0x0016;
pub const CMD_802_11_DEAUTHENTICATE: u16 = 0x0024;
pub const CMD_MAC_CONTROL: u16 = 0x0028;
pub const CMD_802_11_MAC_ADDRESS: u16 = 0x004d;
pub const CMD_802_11_KEY_MATERIAL: u16 = 0x005e;
pub const CMD_VERSION_EXT: u16 = 0x0097;
pub const CMD_FUNC_INIT: u16 = 0x00a9;
pub const CMD_FUNC_SHUTDOWN: u16 = 0x00aa;
pub const CMD_RECONFIGURE_TX_BUFF: u16 = 0x00d9;
pub const CMD_802_11_PS_MODE_ENH: u16 = 0x00e4;
pub const CMD_PCIE_DESC_DETAILS: u16 = 0x00fa;
pub const CMD_802_11_SCAN_EXT: u16 = 0x0107;

pub const EVENT_LINK_LOST: u32 = 0x03;
pub const EVENT_LINK_SENSED: u32 = 0x04;
pub const EVENT_INIT_DONE: u32 = 0x07;
pub const EVENT_DEAUTHENTICATED: u32 = 0x08;
pub const EVENT_DISASSOCIATED: u32 = 0x09;
pub const EVENT_PS_AWAKE: u32 = 0x0a;
pub const EVENT_PS_SLEEP: u32 = 0x0b;
pub const EVENT_PORT_RELEASE: u32 = 0x2b;
pub const EVENT_EXT_SCAN_REPORT: u32 = 0x58;

pub const TLV_SUPP_RATES: u16 = 0x0001;
pub const TLV_CHANLIST: u16 = 0x0101;
pub const TLV_NUMPROBES: u16 = 0x0102;
pub const TLV_TSFTIMESTAMP: u16 = 0x0113;
pub const TLV_CHANNELBANDLIST: u16 = 0x012a;
pub const TLV_BSS_SCAN_RSP: u16 = 0x0156;
pub const TLV_BSS_SCAN_INFO: u16 = 0x0157;
pub const TLV_BSS_MODE: u16 = 0x01ce;

/// MAC_CONTROL action bits.
pub const MAC_RX_ON: u32 = 1 << 0;
pub const MAC_TX_ON: u32 = 1 << 1;
pub const MAC_ETHERNETII: u32 = 1 << 4;

/// Scan mode bits of a channel entry.
pub const SCAN_PASSIVE: u8 = 1 << 0;
pub const SCAN_DISABLE_CHAN_FILT: u8 = 1 << 1;
pub const SCAN_HIDDEN_SSID_REPORT: u8 = 1 << 4;
/// BSS mode: infrastructure, ad hoc, any.
pub const BSS_MODE_ANY: u8 = 3;

/// Most BSS descriptions in one scan response (`MWIFIEX_MAX_AP`).
pub const MAX_AP: usize = 128;

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The TLVs of a buffer: (type, value). Stops at a truncated one.
pub fn tlvs(mut b: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    core::iter::from_fn(move || {
        if b.len() < 4 {
            return None;
        }
        let (t, n) = (le16(b, 0), le16(b, 2) as usize);
        if b.len() < 4 + n {
            return None;
        }
        let v = &b[4..4 + n];
        b = &b[4 + n..];
        Some((t, v))
    })
}

fn push_tlv(v: &mut Vec<u8>, t: u16, value: &[u8]) {
    v.extend(t.to_le_bytes());
    v.extend((value.len() as u16).to_le_bytes());
    v.extend(value);
}

/// A command (header and body), without the interface header. The
/// sequence number's high byte is the BSS number and type (0: the first
/// station interface).
pub fn command(cmd: u16, seq: u8, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(CMD_HDR + body.len());
    v.extend(cmd.to_le_bytes());
    v.extend(((CMD_HDR + body.len()) as u16).to_le_bytes());
    v.extend((seq as u16).to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(body);
    v
}

/// A command response.
#[derive(Debug, PartialEq, Eq)]
pub struct Response<'a> {
    /// The command it answers (without [`RET_BIT`]).
    pub command: u16,
    pub seq: u8,
    /// 0: done.
    pub result: u16,
    pub body: &'a [u8],
}

/// Parse a response (the bytes after the interface header).
pub fn response(b: &[u8]) -> Option<Response<'_>> {
    if b.len() < CMD_HDR {
        return None;
    }
    let cmd = le16(b, 0);
    let size = (le16(b, 2) as usize).clamp(CMD_HDR, b.len());
    if cmd & RET_BIT == 0 {
        return None;
    }
    Some(Response { command: cmd & !RET_BIT, seq: le16(b, 4) as u8, result: le16(b, 6), body: &b[CMD_HDR..size] })
}

/// An event (the bytes after the interface header): its cause and body.
pub fn event(b: &[u8]) -> Option<(u32, &[u8])> {
    if b.len() < 4 {
        return None;
    }
    Some((le32(b, 0) & 0xffff, &b[4..]))
}

/// PCIE_DESC_DETAILS: where the host put its rings.
pub fn pcie_desc_details(tx: (u64, u32), rx: (u64, u32), evt: (u64, u32), sleep_cookie: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(44);
    for (a, n) in [tx, rx, evt] {
        v.extend((a as u32).to_le_bytes());
        v.extend(((a >> 32) as u32).to_le_bytes());
        v.extend(n.to_le_bytes());
    }
    v.extend((sleep_cookie as u32).to_le_bytes());
    v.extend(((sleep_cookie >> 32) as u32).to_le_bytes());
    v
}

/// The body of GET_HW_SPEC (`host_cmd_ds_get_hw_spec`, all zero).
pub const HW_SPEC_LEN: usize = 63;

/// The GET_HW_SPEC response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwSpec {
    pub hw_if_version: u16,
    pub version: u16,
    pub mac: [u8; 6],
    pub region: u16,
    pub antennas: u16,
    pub fw_release: u32,
    pub fw_cap: u32,
    pub dot11n_cap: u32,
    pub dot11ac_cap: u32,
}

impl HwSpec {
    pub fn parse(b: &[u8]) -> Option<HwSpec> {
        if b.len() < 38 {
            return None;
        }
        let mut mac = [0; 6];
        mac.copy_from_slice(&b[8..14]);
        Some(HwSpec {
            hw_if_version: le16(b, 0),
            version: le16(b, 2),
            mac,
            region: le16(b, 14),
            antennas: le16(b, 16),
            fw_release: le32(b, 18),
            fw_cap: le32(b, 34),
            dot11n_cap: if b.len() >= 42 { le32(b, 38) } else { 0 },
            dot11ac_cap: if b.len() >= 59 { le32(b, 55) } else { 0 },
        })
    }

    /// "15.68.19.p21", as Linux prints it.
    pub fn fw_version(&self) -> String {
        let c = self.fw_release.to_le_bytes();
        alloc::format!("{}.{}.{}.p{}", c[2], c[1], c[0], c[3])
    }
}

/// A channel to scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanChannel {
    /// 0: 2.4 GHz (B/G), 1: 5 GHz (A).
    pub band: u8,
    pub channel: u8,
    /// Listen only (no probe requests): channels a station may not
    /// transmit on before it knows the regulatory domain (DFS, 12-13).
    pub passive: bool,
    pub time_ms: u16,
}

/// Active and passive dwell times (Linux: 40 and 110 ms).
pub const ACTIVE_MS: u16 = 40;
pub const PASSIVE_MS: u16 = 110;

/// The channels scanned when no regulatory domain is known: 2.4 GHz 1-11
/// active and 12-13 passive; 5 GHz 36-48 active (allowed nearly
/// everywhere), the DFS channels and 149-165 passive.
pub fn default_channels() -> Vec<ScanChannel> {
    let mut v = Vec::new();
    for c in 1..=13u8 {
        let passive = c > 11;
        v.push(ScanChannel { band: 0, channel: c, passive, time_ms: if passive { PASSIVE_MS } else { ACTIVE_MS } });
    }
    let five: &[u8] = &[36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161, 165];
    for &c in five {
        let passive = c > 48;
        v.push(ScanChannel { band: 1, channel: c, passive, time_ms: if passive { PASSIVE_MS } else { ACTIVE_MS } });
    }
    v
}

const RATES_BG: [u8; 12] = [0x82, 0x84, 0x8b, 0x96, 0x0c, 0x12, 0x18, 0x24, 0x30, 0x48, 0x60, 0x6c];
const RATES_A: [u8; 8] = [0x0c, 0x12, 0x18, 0x24, 0x30, 0x48, 0x60, 0x6c];

/// The body of a scan command for `chans` (all of one band): the legacy
/// 802_11_SCAN (results in the response) or 802_11_SCAN_EXT (results in
/// EXT_SCAN_REPORT events).
pub fn scan_body(chans: &[ScanChannel], probes: u16, ext: bool) -> Vec<u8> {
    let mut v = Vec::new();
    if ext {
        v.extend(0u32.to_le_bytes());
        push_tlv(&mut v, TLV_BSS_MODE, &[BSS_MODE_ANY]);
    } else {
        v.push(BSS_MODE_ANY);
        v.extend([0u8; 6]);
    }
    if probes != 0 {
        push_tlv(&mut v, TLV_NUMPROBES, &probes.to_le_bytes());
    }
    let mut list = Vec::new();
    for c in chans {
        let mut mode = SCAN_DISABLE_CHAN_FILT;
        if c.passive {
            mode |= SCAN_PASSIVE | SCAN_HIDDEN_SSID_REPORT;
        }
        list.extend([c.band, c.channel, mode]);
        list.extend(c.time_ms.to_le_bytes());
        list.extend(c.time_ms.to_le_bytes());
    }
    push_tlv(&mut v, TLV_CHANLIST, &list);
    let a = chans.first().is_some_and(|c| c.band == 1);
    push_tlv(&mut v, TLV_SUPP_RATES, if a { &RATES_A } else { &RATES_BG });
    v
}

/// One BSS description: bssid, (RSSI byte), timestamp, beacon interval,
/// capability, elements.
fn bss(d: &[u8], rssi_byte: bool, rssi: i8, chan: u8) -> Option<Bss> {
    let fixed = 6 + rssi_byte as usize + 12;
    if d.len() < fixed {
        return None;
    }
    let mut bssid = [0; 6];
    bssid.copy_from_slice(&d[..6]);
    let mut p = 6;
    let rssi = if rssi_byte {
        p += 1;
        // Positive: minus dBm.
        (-(d[6] as i16)).max(-128) as i8
    } else {
        rssi
    };
    let interval = le16(d, p + 8);
    let cap = le16(d, p + 10);
    Some(Bss::new(bssid, rssi, interval, cap, &d[fixed..], chan))
}

/// The networks in an 802_11_SCAN response body.
pub fn parse_scan_response(body: &[u8]) -> Option<Vec<Bss>> {
    if body.len() < 3 {
        return None;
    }
    let size = le16(body, 0) as usize;
    let sets = body[2] as usize;
    if sets > MAX_AP || 3 + size > body.len() {
        return None;
    }
    let descs = &body[3..3 + size];
    // A channel/band list TLV after the descriptions gives each one's
    // channel (needed for networks without a DS element, on 5 GHz).
    let chans = tlvs(&body[3 + size..]).find(|t| t.0 == TLV_CHANNELBANDLIST).map(|t| t.1).unwrap_or(&[]);
    let mut out = Vec::new();
    let mut p = 0;
    for i in 0..sets {
        if p + 2 > descs.len() {
            break;
        }
        let n = le16(descs, p) as usize;
        p += 2;
        if n == 0 || p + n > descs.len() {
            break;
        }
        let chan = chans.get(i * 2 + 1).copied().unwrap_or(0);
        if let Some(b) = bss(&descs[p..p + n], true, 0, chan) {
            out.push(b);
        }
        p += n;
    }
    Some(out)
}

/// The networks in an EXT_SCAN_REPORT event body (after the cause), and
/// whether more reports follow for this scan.
pub fn parse_ext_scan_report(body: &[u8]) -> Option<(Vec<Bss>, bool)> {
    if body.len() < 7 {
        return None;
    }
    let more = body[0] != 0;
    let size = (le16(body, 4) as usize).min(body.len() - 7);
    let sets = body[6] as usize;
    if sets > MAX_AP {
        return None;
    }
    let mut out = Vec::new();
    let mut cur: Option<&[u8]> = None;
    let mut info: Option<&[u8]> = None;
    let flush = |cur: Option<&[u8]>, info: Option<&[u8]>, out: &mut Vec<Bss>| {
        if let Some(d) = cur {
            let (rssi, chan) = match info {
                Some(i) if i.len() >= 7 => ((le16(i, 0) as i16).clamp(-128, 127) as i8, i[6]),
                _ => (0, 0),
            };
            if let Some(b) = bss(d, false, rssi, chan) {
                out.push(b);
            }
        }
    };
    for (t, v) in tlvs(&body[7..7 + size]) {
        match t {
            TLV_BSS_SCAN_RSP => {
                flush(cur, info, &mut out);
                cur = Some(v);
                info = None;
            }
            TLV_BSS_SCAN_INFO => info = Some(v),
            _ => {}
        }
    }
    flush(cur, info, &mut out);
    out.truncate(sets);
    Some((out, more))
}

pub const TLV_KEY_MATERIAL: u16 = 0x0100;
pub const TLV_AUTH_TYPE: u16 = 0x011f;
pub const TLV_KEY_PARAM_V2: u16 = 0x019c;
pub const TLV_API_REV: u16 = 0x01c7;

pub const ACT_GET: u16 = 0;
pub const ACT_SET: u16 = 1;
pub const ACT_REMOVE: u16 = 4;

/// The key API version (major, minor) from the TLVs after the fixed
/// GET_HW_SPEC body: 2 means KEY_MATERIAL in the "v2" layout. (0, 0) when
/// the firmware does not say (the old layout).
pub fn key_api_version(hw_spec: &[u8]) -> (u8, u8) {
    if hw_spec.len() <= HW_SPEC_LEN {
        return (0, 0);
    }
    for (t, v) in tlvs(&hw_spec[HW_SPEC_LEN..]) {
        // api_id 1: the key API.
        if t == TLV_API_REV && v.len() >= 4 && le16(v, 0) == 1 {
            return (v[2], v[3]);
        }
    }
    (0, 0)
}

/// Capability bits the host must not ask for (`CAPINFO_MASK` in Linux).
const CAP_NOT_ASKED: u16 = 1 << 15 | 1 << 14 | 1 << 12 | 1 << 11 | 1 << 9;
pub const LISTEN_INTERVAL: u16 = 10;

/// The rates of a network's (extended) supported rates elements that the
/// chip has too, with the network's basic-rate bits.
fn common_rates(bss: &Bss) -> Vec<u8> {
    let ours: &[u8] = if bss.channel > 14 { &RATES_A } else { &RATES_BG };
    let mut v = Vec::new();
    for (id, body) in crate::ie::elements(&bss.ies) {
        if id == crate::ie::EID_SUPP_RATES || id == crate::ie::EID_EXT_RATES {
            for &r in body {
                if ours.iter().any(|&o| o & 0x7f == r & 0x7f) && !v.iter().any(|&x: &u8| x & 0x7f == r & 0x7f) {
                    v.push(r);
                }
            }
        }
    }
    if v.is_empty() {
        v.extend_from_slice(ours);
    }
    v
}

/// The body of 802_11_ASSOCIATE for `bss` (from a scan just before: the
/// firmware joins what it found). `rsn_ie` is the RSN element (id, length,
/// body) the station offers, for WPA2 networks. Open System
/// authentication; no 802.11n/ac elements yet (the link runs at legacy
/// rates).
pub fn associate_body(bss: &Bss, rsn_ie: Option<&[u8]>) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend(bss.bssid);
    v.extend((bss.capability & !CAP_NOT_ASKED).to_le_bytes());
    v.extend(LISTEN_INTERVAL.to_le_bytes());
    v.extend(bss.beacon_interval.to_le_bytes());
    v.push(0); // DTIM period: the firmware's
    push_tlv(&mut v, crate::ie::EID_SSID as u16, &bss.ssid);
    push_tlv(&mut v, crate::ie::EID_DS_PARAMS as u16, &[bss.channel]);
    // CF parameter set (none).
    push_tlv(&mut v, 4, &[0; 6]);
    push_tlv(&mut v, TLV_SUPP_RATES, &common_rates(bss));
    push_tlv(&mut v, TLV_AUTH_TYPE, &0u16.to_le_bytes());
    // The channel and band the network is on.
    let band = if bss.channel > 14 { 1 } else { 0 };
    push_tlv(&mut v, TLV_CHANLIST, &[band, bss.channel, 0, 0, 0, 0, 0]);
    if let Some(ie) = rsn_ie.filter(|ie| ie.len() >= 2) {
        push_tlv(&mut v, ie[0] as u16, &ie[2..]);
    }
    v
}

/// The 802_11_ASSOCIATE response: (capability, status code, association
/// ID). Status 0 is success; the firmware puts its own reason in the
/// capability field when the status is not 0 (1: authentication failed,
/// 2: the AP did not answer the authentication, 3: association timeout,
/// 4: association refused, 5: station failure).
pub fn associate_result(body: &[u8]) -> Option<(u16, u16, u16)> {
    (body.len() >= 6).then(|| (le16(body, 0), le16(body, 2), le16(body, 4) & 0x3fff))
}

/// 802_11_DEAUTHENTICATE: leave `bssid` with `reason` (3: leaving).
pub fn deauth_body(bssid: [u8; 6], reason: u16) -> Vec<u8> {
    let mut v = bssid.to_vec();
    v.extend(reason.to_le_bytes());
    v
}

/// A key to install in the firmware.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// Pairwise (the AP's address) or group (broadcast).
    pub pairwise: bool,
    pub peer: [u8; 6],
    pub index: u8,
    /// 16 bytes CCMP, 32 bytes TKIP.
    pub key: Vec<u8>,
    /// The receive sequence counter to start from (group keys).
    pub rsc: u64,
    /// The first group key since association (the firmware marks keys
    /// "default" until it has one).
    pub first: bool,
}

const KEY_MCAST: u16 = 1 << 0;
const KEY_UNICAST: u16 = 1 << 1;
const KEY_ENABLED: u16 = 1 << 2;
const KEY_DEFAULT: u16 = 1 << 3;
const KEY_TX: u16 = 1 << 4;
const KEY_RX: u16 = 1 << 5;
const KEY_TYPE_TKIP: u8 = 1;
const KEY_TYPE_AES: u8 = 2;

/// KEY_MATERIAL (set) for `k`, in the layout of key API `major` (2: the
/// v2 TLV with address, index and receive counter; else the old one).
pub fn key_material_body(k: &Key, major: u8) -> Vec<u8> {
    let tkip = k.key.len() == 32;
    let mut v = ACT_SET.to_le_bytes().to_vec();
    if major == 2 {
        let mut info = KEY_ENABLED;
        info |= if k.pairwise { KEY_UNICAST | KEY_TX | KEY_RX } else { KEY_MCAST | KEY_RX };
        if k.first {
            info |= KEY_DEFAULT;
        }
        let mut t = k.peer.to_vec();
        t.push(k.index & 0xf);
        t.push(if tkip { KEY_TYPE_TKIP } else { KEY_TYPE_AES });
        t.extend(info.to_le_bytes());
        t.extend(k.rsc.to_le_bytes());
        t.extend((k.key.len() as u16).to_le_bytes());
        let mut key = [0u8; 32];
        key[..k.key.len().min(32)].copy_from_slice(&k.key[..k.key.len().min(32)]);
        t.extend(key);
        push_tlv(&mut v, TLV_KEY_PARAM_V2, &t);
    } else {
        let info = KEY_ENABLED | if k.pairwise { KEY_UNICAST } else { KEY_MCAST };
        let mut t = (if tkip { KEY_TYPE_TKIP } else { KEY_TYPE_AES } as u16).to_le_bytes().to_vec();
        t.extend(info.to_le_bytes());
        t.extend((k.key.len() as u16).to_le_bytes());
        t.extend(&k.key);
        push_tlv(&mut v, TLV_KEY_MATERIAL, &t);
    }
    v
}

/// The TxPD the firmware wants before each transmitted 802.3 frame (BSS
/// 0, station): length and offset of the frame, default priority and
/// rate control.
pub const TXPD_LEN: usize = 20;

pub fn txpd(frame_len: usize) -> [u8; TXPD_LEN] {
    let mut t = [0u8; TXPD_LEN];
    t[2..4].copy_from_slice(&(frame_len as u16).to_le_bytes());
    t[4..6].copy_from_slice(&(TXPD_LEN as u16).to_le_bytes());
    t
}

/// RxPD packet types. Data comes as several of them (the 88W8897 gave
/// the WPA2 handshake's EAPOL frames type 2); as in Linux, every type but
/// these is an ordinary frame.
pub const PKT_TYPE_DATA: u16 = 0;
pub const PKT_TYPE_AMSDU: u16 = 3;
pub const PKT_TYPE_BAR: u16 = 0x0a;
pub const PKT_TYPE_MGMT: u16 = 0xe5;

/// A received data packet (after the interface header: RxPD, then an
/// 802.3 frame with an LLC/SNAP header) as an Ethernet II frame.
/// None: not a data frame, or malformed.
pub fn rx_frame(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 12 {
        return None;
    }
    let len = le16(b, 2) as usize;
    let off = le16(b, 4) as usize;
    let kind = le16(b, 6);
    // A-MSDUs (several frames in one) are not taken apart yet: only
    // offered with 802.11n, which rs2 does not ask for.
    if matches!(kind, PKT_TYPE_AMSDU | PKT_TYPE_BAR | PKT_TYPE_MGMT) || off < 12 || off + 14 > b.len() {
        return None;
    }
    let p = &b[off..(off + len).min(b.len())];
    if p.len() < 14 {
        return None;
    }
    // RFC 1042 (AA AA 03 00 00 00) or bridge tunnel (AA AA 03 00 00 F8)
    // SNAP: Ethernet II with the SNAP's type.
    let snap = p.len() >= 22 && p[14..17] == [0xaa, 0xaa, 0x03] && (p[17..20] == [0, 0, 0] || p[17..20] == [0, 0, 0xf8]);
    let mut f = Vec::with_capacity(p.len());
    f.extend(&p[..12]);
    if snap {
        f.extend(&p[20..]);
    } else {
        f.extend(&p[12..]);
    }
    Some(f)
}

pub fn command_name(cmd: u16) -> &'static str {
    match cmd {
        CMD_GET_HW_SPEC => "GET_HW_SPEC",
        CMD_802_11_SCAN => "SCAN",
        CMD_802_11_ASSOCIATE => "ASSOCIATE",
        CMD_802_11_SNMP_MIB => "SNMP_MIB",
        CMD_802_11_DEAUTHENTICATE => "DEAUTHENTICATE",
        CMD_MAC_CONTROL => "MAC_CONTROL",
        CMD_802_11_MAC_ADDRESS => "MAC_ADDRESS",
        CMD_802_11_KEY_MATERIAL => "KEY_MATERIAL",
        CMD_VERSION_EXT => "VERSION_EXT",
        CMD_FUNC_INIT => "FUNC_INIT",
        CMD_FUNC_SHUTDOWN => "FUNC_SHUTDOWN",
        CMD_RECONFIGURE_TX_BUFF => "RECONFIGURE_TX_BUFF",
        CMD_802_11_PS_MODE_ENH => "PS_MODE_ENH",
        CMD_PCIE_DESC_DETAILS => "PCIE_DESC_DETAILS",
        CMD_802_11_SCAN_EXT => "SCAN_EXT",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn command_header() {
        let c = command(CMD_FUNC_INIT, 5, &[]);
        assert_eq!(c, vec![0xa9, 0, 8, 0, 5, 0, 0, 0]);
        let mut r = c.clone();
        r[1] |= 0x80;
        r[6] = 2;
        let p = response(&r).unwrap();
        assert_eq!((p.command, p.seq, p.result, p.body.len()), (CMD_FUNC_INIT, 5, 2, 0));
        assert_eq!(response(&c), None);
    }

    #[test]
    fn desc_details_layout() {
        let v = pcie_desc_details((0x1_2345_6000, 32), (0x7000, 32), (0x8000, 8), 0);
        assert_eq!(v.len(), 44);
        assert_eq!(&v[..12], &[0x00, 0x60, 0x45, 0x23, 1, 0, 0, 0, 32, 0, 0, 0]);
        assert_eq!(&v[24..28], &[0x00, 0x80, 0, 0]);
        assert_eq!(&v[32..36], &[8, 0, 0, 0]);
    }

    #[test]
    fn hw_spec() {
        let mut b = vec![0u8; HW_SPEC_LEN];
        b[8..14].copy_from_slice(&[0x98, 0x5f, 0xd3, 1, 2, 3]);
        b[18..22].copy_from_slice(&[19, 68, 15, 21]);
        b[34] = 0x55;
        let h = HwSpec::parse(&b).unwrap();
        assert_eq!(h.mac, [0x98, 0x5f, 0xd3, 1, 2, 3]);
        assert_eq!(h.fw_version(), "15.68.19.p21");
        assert_eq!(h.fw_cap, 0x55);
    }

    #[test]
    fn scan_command() {
        let chans = [ScanChannel { band: 0, channel: 1, passive: false, time_ms: 40 }, ScanChannel { band: 0, channel: 13, passive: true, time_ms: 110 }];
        let b = scan_body(&chans, 1, false);
        assert_eq!(&b[..7], &[3, 0, 0, 0, 0, 0, 0]);
        let t: Vec<_> = tlvs(&b[7..]).collect();
        assert_eq!(t[0], (TLV_NUMPROBES, &[1, 0][..]));
        assert_eq!(t[1], (TLV_CHANLIST, &[0, 1, 2, 40, 0, 40, 0, 0, 13, 0x13, 110, 0, 110, 0][..]));
        assert_eq!(t[2].0, TLV_SUPP_RATES);
        assert_eq!(t[2].1.len(), 12);
        let e = scan_body(&chans[..1], 0, true);
        assert_eq!(&e[..9], &[0, 0, 0, 0, 0xce, 1, 1, 0, 3]);
        assert_eq!(default_channels().iter().filter(|c| !c.passive).count(), 15);
    }

    fn beacon(ssid: &[u8], ds: Option<u8>) -> Vec<u8> {
        // timestamp, beacon interval 100, capability ESS|privacy.
        let mut v = vec![0u8; 8];
        v.extend([100, 0, 0x11, 0]);
        v.extend([0, ssid.len() as u8]);
        v.extend(ssid);
        if let Some(c) = ds {
            v.extend([3, 1, c]);
        }
        v
    }

    #[test]
    fn legacy_scan_response() {
        let mut descs = Vec::new();
        for (mac, rssi, ssid, ds) in [([1u8; 6], 45u8, &b"Home"[..], Some(6)), ([2u8; 6], 80, &b"Five"[..], None)] {
            let mut d = mac.to_vec();
            d.push(rssi);
            d.extend(beacon(ssid, ds));
            descs.extend((d.len() as u16).to_le_bytes());
            descs.extend(d);
        }
        let mut body = (descs.len() as u16).to_le_bytes().to_vec();
        body.push(2);
        body.extend(&descs);
        push_tlv(&mut body, TLV_CHANNELBANDLIST, &[0, 6, 1, 36]);
        let v = parse_scan_response(&body).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].ssid_text().as_str(), v[0].rssi, v[0].channel, v[0].beacon_interval), ("Home", -45, 6, 100));
        assert_eq!((v[1].ssid_text().as_str(), v[1].rssi, v[1].channel), ("Five", -80, 36));
        assert_eq!(v[1].security(), crate::ie::Security::Wep);
        // A description size past the end of the response.
        body[0] += 10;
        assert_eq!(parse_scan_response(&body), None);
    }

    #[test]
    fn ext_scan_report() {
        let mut tl = Vec::new();
        for (mac, rssi, chan) in [([1u8; 6], -40i16, 1u8), ([2u8; 6], -71, 149)] {
            let mut d = mac.to_vec();
            d.extend(beacon(b"Net", None));
            push_tlv(&mut tl, TLV_BSS_SCAN_RSP, &d);
            let mut info = rssi.to_le_bytes().to_vec();
            info.extend([0, 0, 0, 0, chan, 0]);
            info.extend([0; 8]);
            push_tlv(&mut tl, TLV_BSS_SCAN_INFO, &info);
        }
        let mut body = vec![1, 0, 0, 0];
        body.extend((tl.len() as u16).to_le_bytes());
        body.push(2);
        body.extend(&tl);
        let (v, more) = parse_ext_scan_report(&body).unwrap();
        assert!(more);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].rssi, v[0].channel), (-40, 1));
        assert_eq!((v[1].rssi, v[1].channel, v[1].band()), (-71, 149, "5GHz"));
    }

    #[test]
    fn associate_command() {
        let mut ies = vec![0, 4, b'H', b'o', b'm', b'e', 1, 4, 0x82, 0x84, 0x8b, 0x96, 50, 2, 0x0c, 0x6c, 3, 1, 6];
        ies.extend([48, 2, 1, 0]);
        let b = Bss::new([2, 0, 0, 0, 0, 9], -50, 100, 0x0411 | 1 << 15, &ies, 6);
        let rsn = [48u8, 3, 1, 0, 7];
        let v = associate_body(&b, Some(&rsn));
        assert_eq!(&v[..6], &[2, 0, 0, 0, 0, 9]);
        assert_eq!(&v[6..13], &[0x11, 0x04, 10, 0, 100, 0, 0]);
        let t: Vec<_> = tlvs(&v[13..]).collect();
        assert_eq!(t[0], (0, &b"Home"[..]));
        assert_eq!(t[1], (3, &[6][..]));
        assert_eq!(t[2], (4, &[0u8; 6][..]));
        assert_eq!(t[3], (TLV_SUPP_RATES, &[0x82, 0x84, 0x8b, 0x96, 0x0c, 0x6c][..]));
        assert_eq!(t[4], (TLV_AUTH_TYPE, &[0, 0][..]));
        assert_eq!(t[5], (TLV_CHANLIST, &[0, 6, 0, 0, 0, 0, 0][..]));
        assert_eq!(t[6], (48, &[1, 0, 7][..]));
        assert_eq!(associate_result(&[0x11, 0, 0, 0, 1, 0xc0]), Some((0x11, 0, 1)));
    }

    #[test]
    fn key_material() {
        let k = Key { pairwise: true, peer: [1, 2, 3, 4, 5, 6], index: 0, key: vec![0xab; 16], rsc: 0, first: true };
        let v = key_material_body(&k, 2);
        assert_eq!(&v[..2], &[1, 0]);
        let (t, b) = tlvs(&v[2..]).next().unwrap();
        assert_eq!((t, b.len()), (TLV_KEY_PARAM_V2, 52));
        assert_eq!(&b[..10], &[1, 2, 3, 4, 5, 6, 0, KEY_TYPE_AES, 0x3e, 0]);
        assert_eq!(&b[18..20], &[16, 0]);
        assert_eq!(b[20], 0xab);
        let g = Key { pairwise: false, peer: [0xff; 6], index: 1, key: vec![7; 16], rsc: 5, first: false };
        let b = key_material_body(&g, 2);
        assert_eq!(&b[4 + 2 + 6..4 + 2 + 12], &[1, KEY_TYPE_AES, 0x25, 0, 5, 0]);
        let v1 = key_material_body(&g, 1);
        let (t, b) = tlvs(&v1[2..]).next().unwrap();
        assert_eq!(t, TLV_KEY_MATERIAL);
        assert_eq!(&b[..6], &[KEY_TYPE_AES, 0, 0x05, 0, 16, 0]);
        assert_eq!(b.len(), 22);
    }

    #[test]
    fn hw_spec_key_api() {
        let mut b = vec![0u8; HW_SPEC_LEN];
        push_tlv(&mut b, TLV_API_REV, &[2, 0, 9, 9]);
        push_tlv(&mut b, TLV_API_REV, &[1, 0, 2, 1]);
        assert_eq!(key_api_version(&b), (2, 1));
        assert_eq!(key_api_version(&b[..HW_SPEC_LEN]), (0, 0));
    }

    #[test]
    fn data_frames() {
        assert_eq!(txpd(60), [0, 0, 60, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // RxPD (offset 20), 802.3 header, SNAP, ARP payload.
        let mut b = vec![0u8; 20];
        let mut p = vec![0xff; 6];
        p.extend([2, 0, 0, 0, 0, 1]);
        p.extend([0, 10]);
        p.extend([0xaa, 0xaa, 3, 0, 0, 0, 0x08, 0x06]);
        p.extend([1, 2]);
        b[2..4].copy_from_slice(&(p.len() as u16).to_le_bytes());
        b[4] = 20;
        b.extend(&p);
        let f = rx_frame(&b).unwrap();
        assert_eq!(f.len(), 16);
        assert_eq!(&f[12..], &[0x08, 0x06, 1, 2]);
        // Other packet types are data too (the 88W8897's EAPOL frames
        // came as type 2), A-MSDU, BAR and management frames are not.
        b[6] = 2;
        assert_eq!(rx_frame(&b).unwrap(), f);
        for t in [PKT_TYPE_AMSDU, PKT_TYPE_BAR, PKT_TYPE_MGMT] {
            b[6..8].copy_from_slice(&t.to_le_bytes());
            assert_eq!(rx_frame(&b), None);
        }
        // An Ethernet II frame as the 88W8897 delivered it (MAC_CONTROL
        // asks for Ethernet II): no SNAP, at offset 90, EAPOL-Key.
        let mut b = vec![0u8; 90];
        let mut p = vec![0x98, 0x5f, 0xd3, 0xc8, 0xed, 0x12, 0xe0, 0x63, 0xda, 0xb1, 0x61, 0x6a, 0x88, 0x8e];
        p.extend([2, 3, 0, 95]);
        p.extend([0u8; 95]);
        b[2..4].copy_from_slice(&(p.len() as u16).to_le_bytes());
        b[4] = 90;
        b[6] = 2;
        b.extend(&p);
        assert_eq!(rx_frame(&b).unwrap(), p);
    }

    #[test]
    fn events() {
        assert_eq!(event(&[0x58, 0, 0, 0, 9]), Some((EVENT_EXT_SCAN_REPORT, &[9u8][..])));
    }
}
