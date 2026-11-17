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

use crate::ie::{Bss, EID_BSS_COEX_2040, EID_EXT_CAPABILITY, EID_HT_CAPABILITY, EID_HT_OPERATION, EID_OPMODE_NOTIF, EID_VENDOR, EID_VHT_CAPABILITY, EID_VHT_OPERATION, HT_CAP_LEN, VHT_CAP_LEN, VHT_OPER_LEN};

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
pub const CMD_11N_CFG: u16 = 0x00cd;
pub const CMD_11N_ADDBA_REQ: u16 = 0x00ce;
pub const CMD_11N_ADDBA_RSP: u16 = 0x00cf;
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
/// The AP asks for a block-ack session (body: an ADDBA request).
pub const EVENT_ADDBA: u32 = 0x33;
/// A block-ack session ends (body: a DELBA).
pub const EVENT_DELBA: u32 = 0x34;
pub const EVENT_BA_STREAM_TIMEOUT: u32 = 0x37;
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
    /// Spatial streams: 0x11 one, 0x22 two (low nibble: receive).
    pub dev_mcs: u8,
    pub dot11ac_cap: u32,
    /// VHT MCS maps: receive in the low half, transmit in the high.
    pub dot11ac_mcs: u32,
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
            dev_mcs: if b.len() >= 43 { b[42] } else { 0 },
            dot11ac_cap: if b.len() >= 59 { le32(b, 55) } else { 0 },
            dot11ac_mcs: if b.len() >= 63 { le32(b, 59) } else { 0 },
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
/// authentication. With `caps`, 802.11n and 802.11ac are offered to a
/// network that has them (as Linux `mwifiex_cmd_802_11_associate`).
pub fn associate_body(bss: &Bss, rsn_ie: Option<&[u8]>, caps: Option<&Caps>) -> Vec<u8> {
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
    let band = if bss.channel > 14 { 1 } else { 0 };
    let ht = caps.filter(|c| c.ht()).and_then(|c| bss.element(EID_HT_CAPABILITY).filter(|b| b.len() >= HT_CAP_LEN).map(|ap| (c, ap)));
    if ht.is_none() {
        // The channel and band the network is on (with 802.11n it comes
        // below, with the secondary channel).
        push_tlv(&mut v, TLV_CHANLIST, &[band, bss.channel, 0, 0, 0, 0, 0]);
    }
    if let Some(ie) = rsn_ie.filter(|ie| ie.len() >= 2) {
        push_tlv(&mut v, ie[0] as u16, &ie[2..]);
    }
    if let Some((c, ap)) = ht {
        push_tlv(&mut v, EID_HT_CAPABILITY as u16, &c.ht_cap(ap));
        if let Some(op) = bss.element(EID_HT_OPERATION).filter(|b| b.len() >= 2) {
            // Primary channel, and the secondary one when both sides
            // can use 40 MHz.
            let mut radio = band;
            if c.ht40() && op[1] & 1 << 2 != 0 {
                radio |= (op[1] & 3) << 4;
            }
            push_tlv(&mut v, TLV_CHANLIST, &[radio, op[0], 0, 0, 0, 0, 0]);
        }
        if let Some(b) = bss.element(EID_BSS_COEX_2040).filter(|b| !b.is_empty()) {
            push_tlv(&mut v, EID_BSS_COEX_2040 as u16, &b[..1]);
        }
        if let Some(b) = bss.element(EID_EXT_CAPABILITY) {
            push_tlv(&mut v, EID_EXT_CAPABILITY as u16, b);
        }
        if c.vht() {
            if let Some(ap) = bss.element(EID_VHT_CAPABILITY).filter(|b| b.len() >= VHT_CAP_LEN) {
                push_tlv(&mut v, EID_VHT_CAPABILITY as u16, &c.vht_cap(ap));
            }
            if let Some(op) = bss.element(EID_VHT_OPERATION).filter(|b| b.len() >= VHT_OPER_LEN) {
                let mut o = [0u8; VHT_OPER_LEN];
                o.copy_from_slice(&op[..VHT_OPER_LEN]);
                // Channel width: at most what the chip can (80 MHz unless
                // it says 160 or 80+80).
                o[0] = o[0].min(c.vht_max_width());
                push_tlv(&mut v, EID_VHT_OPERATION as u16, &o);
            }
            if let Some(b) = bss.element(EID_OPMODE_NOTIF).filter(|b| !b.is_empty()) {
                push_tlv(&mut v, EID_OPMODE_NOTIF as u16, &b[..1]);
            }
        }
    }
    if ht.is_some() && bss.wmm() {
        // WMM information element (QoS info 0): 802.11n rates need QoS.
        push_tlv(&mut v, EID_VENDOR as u16, &WMM_INFO);
    }
    v
}

/// The WMM information element's contents (Linux `wmm_info_ie`).
const WMM_INFO: [u8; 7] = [0x00, 0x50, 0xf2, 0x02, 0x00, 0x01, 0x00];

/// What the chip can do in 802.11n/ac, from GET_HW_SPEC (Linux
/// `mwifiex_setup_ht_caps`, `mwifiex_fill_cap_info`, the 11ac parts of
/// `mwifiex_ret_get_hw_spec`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub fw_cap: u32,
    pub dot11n_cap: u32,
    pub dev_mcs: u8,
    pub dot11ac_cap: u32,
    pub dot11ac_mcs: u32,
}

/// VHT capability bits not offered (beamformer roles, sounding
/// dimensions: `MWIFIEX_DEF_11AC_CAP_BF_RESET_MASK`).
const VHT_CAP_BF_RESET: u32 = 1 << 11 | 1 << 19 | 1 << 20 | 7 << 16;
/// HT transmit beamforming capabilities (`MWIFIEX_DEF_11N_TX_BF_CAP`).
const HT_TX_BF_CAP: u32 = 0x09e1_e008;

impl Caps {
    pub fn from_spec(s: &HwSpec) -> Caps {
        Caps { fw_cap: s.fw_cap, dot11n_cap: s.dot11n_cap, dev_mcs: s.dev_mcs, dot11ac_cap: s.dot11ac_cap, dot11ac_mcs: s.dot11ac_mcs }
    }

    pub fn ht(&self) -> bool {
        self.fw_cap & 1 << 11 != 0
    }

    pub fn vht(&self) -> bool {
        self.ht() && self.fw_cap & 1 << 13 != 0
    }

    pub fn ht40(&self) -> bool {
        self.dot11n_cap & 1 << 17 != 0
    }

    /// Receive spatial streams (1 or 2).
    pub fn streams(&self) -> usize {
        ((self.dev_mcs & 0x0f) as usize).clamp(1, 4)
    }

    /// HT capability information field.
    fn ht_cap_info(&self) -> u16 {
        let d = self.dot11n_cap;
        let mut c = 0u16;
        let bit = |b: u32| d & 1 << b != 0;
        if bit(22) {
            c |= 1 << 0; // LDPC
        }
        if bit(17) {
            c |= 1 << 1; // 20/40 MHz
        }
        c |= 3 << 2; // SM power save disabled
        if bit(29) {
            c |= 1 << 4; // greenfield
        }
        if bit(23) {
            c |= 1 << 5; // short GI 20 MHz
        }
        if bit(24) {
            c |= 1 << 6; // short GI 40 MHz
        }
        if bit(25) {
            c |= 1 << 7; // TX STBC
        }
        c |= if self.dev_mcs == 0x22 { 2 } else { 1 } << 8; // RX STBC streams
        if bit(8) {
            c |= 1 << 14; // 40 MHz intolerant
        }
        c
    }

    /// The HT Capabilities element contents offered to a network whose
    /// own is `ap` (its extended capabilities and antenna selection are
    /// kept, as Linux does).
    pub fn ht_cap(&self, ap: &[u8]) -> [u8; HT_CAP_LEN] {
        let mut e = [0u8; HT_CAP_LEN];
        e.copy_from_slice(&ap[..HT_CAP_LEN]);
        e[0..2].copy_from_slice(&self.ht_cap_info().to_le_bytes());
        // A-MPDU: 64 KB, no minimum spacing.
        e[2] = 3;
        // Supported MCS set: 8 MCS per receive stream, MCS 32, no highest
        // rate given, TX set defined and equal.
        let mcs = &mut e[3..19];
        mcs.fill(0);
        for m in mcs.iter_mut().take(self.streams()) {
            *m = 0xff;
        }
        mcs[4] |= 1;
        mcs[12] = 1;
        // Extended HT capabilities: no reverse direction responder.
        let ext = u16::from_le_bytes([e[19], e[20]]) & !(1 << 11);
        e[19..21].copy_from_slice(&ext.to_le_bytes());
        if self.dot11n_cap & 1 << 30 != 0 {
            e[21..25].copy_from_slice(&HT_TX_BF_CAP.to_le_bytes());
        }
        e
    }

    /// The widest VHT channel width (VHT Operation's field): 1 (80 MHz),
    /// 2 (160), 3 (80+80).
    pub fn vht_max_width(&self) -> u8 {
        match (self.dot11ac_cap >> 2) & 3 {
            1 => 2,
            2 => 3,
            _ => 1,
        }
    }

    /// The VHT Capabilities element contents offered to a network whose
    /// own is `ap`: the chip's capabilities without beamformer roles, and
    /// per stream the lower of the chip's and the network's MCS limits
    /// (Linux `mwifiex_fill_vht_cap_tlv`).
    pub fn vht_cap(&self, ap: &[u8]) -> [u8; VHT_CAP_LEN] {
        let mut e = [0u8; VHT_CAP_LEN];
        e.copy_from_slice(&ap[..VHT_CAP_LEN]);
        e[0..4].copy_from_slice(&(self.dot11ac_cap & !VHT_CAP_BF_RESET).to_le_bytes());
        let ours = [(self.dot11ac_mcs & 0xffff) as u16, (self.dot11ac_mcs >> 16) as u16];
        for (k, at) in [4usize, 8].into_iter().enumerate() {
            let theirs = u16::from_le_bytes([e[at], e[at + 1]]);
            let mut map = 0u16;
            for nss in 0..8 {
                let (a, b) = (ours[k] >> (2 * nss) & 3, theirs >> (2 * nss) & 3);
                let m = if a == 3 || b == 3 { 3 } else { a.min(b) };
                map |= m << (2 * nss);
            }
            e[at..at + 2].copy_from_slice(&map.to_le_bytes());
            // Highest long-GI rate: not given.
            e[at + 2..at + 4].copy_from_slice(&[0, 0]);
        }
        e
    }
}

/// 11N_CFG (set): what the firmware may use when it sends with 802.11n
/// (`MWIFIEX_FW_DEF_HTTXCFG`: 20/40 MHz, greenfield, short GI 20 and 40).
pub fn ht_cfg_body() -> Vec<u8> {
    let mut v = Vec::new();
    for x in [ACT_SET, 1 << 1 | 1 << 4 | 1 << 5 | 1 << 6, 0, 0] {
        v.extend(x.to_le_bytes());
    }
    v
}

/// The block-ack window rs2 accepts (frames held for reordering per
/// traffic class).
pub const BA_WINDOW: u16 = 32;

/// A block-ack session the AP asked for (EVENT_ADDBA) and the
/// 11N_ADDBA_RSP accepting it: (command body, TID, starting sequence
/// number, window). Linux `mwifiex_cmd_11n_addba_rsp_gen`.
pub fn addba_accept(req: &[u8], window: u16) -> Option<(Vec<u8>, u8, u16, u16)> {
    if req.len() < 14 {
        return None;
    }
    let param = le16(req, 8);
    let tid = (param >> 2 & 0xf) as u8;
    let ssn = le16(req, 12) & 0xfff;
    // Our window size in bits 6-15; A-MSDUs inside A-MPDUs are fine.
    let param = param & 0x3f | window << 6;
    let mut v = Vec::with_capacity(16);
    v.push(0);
    v.extend(&req[1..8]); // peer address, dialog token
    v.extend(0u16.to_le_bytes()); // status: accepted
    v.extend(param.to_le_bytes());
    v.extend(&req[10..14]); // timeout, starting sequence number
    Some((v, tid, ssn, window))
}

/// The transmit block-ack window rs2 asks for
/// (`MWIFIEX_STA_AMPDU_DEF_TXWINSIZE`).
pub const BA_TX_WINDOW: u16 = 64;

/// 11N_ADDBA_REQ: ask `peer` for a block-ack session for frames rs2
/// sends on `tid` (the firmware then aggregates them into A-MPDUs).
/// Immediate block ack, no A-MSDUs inside (rs2 does not build them),
/// no timeout (Linux `mwifiex_send_addba`).
pub fn addba_req_body(peer: [u8; 6], token: u8, tid: u8, window: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(14);
    v.push(0);
    v.extend(peer);
    v.push(token);
    v.extend(((tid as u16 & 0xf) << 2 | window << 6 | 2).to_le_bytes());
    v.extend(0xffffu16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v
}

/// 11N_ADDBA_REQ's response: (TID, window) when the peer accepted, or
/// the 802.11 status code it gave.
pub fn addba_req_result(rsp: &[u8]) -> Result<(u8, u16), u16> {
    if rsp.len() < 12 {
        return Err(u16::MAX);
    }
    let status = le16(rsp, 8);
    let param = le16(rsp, 10);
    if rsp[0] != 0 || status != 0 {
        return Err(if status != 0 { status } else { rsp[0] as u16 });
    }
    Ok(((param >> 2 & 0xf) as u8, param >> 6))
}

/// PS_MODE_ENH actions and bitmap.
const PS_SLEEP_CONFIRM: u16 = 5;
const PS_EN_AUTO: u16 = 0xff;
const PS_DIS_AUTO: u16 = 0xfe;
const PS_BITMAP_STA: u16 = 0x10;
pub const TLV_PS_PARAM: u16 = 0x0172;

/// PS_MODE_ENH: let the firmware power-save on its own between beacons
/// (station power save with Linux's defaults: wake for every DTIM,
/// beacon-miss timeout 5, go to sleep 1000 ms after the last activity,
/// automatic mode), or stop it.
pub fn ps_mode_body(on: bool) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend((if on { PS_EN_AUTO } else { PS_DIS_AUTO }).to_le_bytes());
    v.extend(PS_BITMAP_STA.to_le_bytes());
    if on {
        let mut p = Vec::new();
        // null packet interval, multiple DTIMs, beacon miss timeout, local
        // listen interval, ad hoc wake period, mode (auto), delay to PS.
        for x in [0u16, 1, 5, 0, 0, 1, 1000] {
            p.extend(x.to_le_bytes());
        }
        push_tlv(&mut v, TLV_PS_PARAM, &p);
    }
    v
}

/// The sleep confirmation (a PS_MODE_ENH command with action SLEEP_CONFIRM
/// and a response asked for): the answer to EVENT_PS_SLEEP when the host
/// has nothing left for the chip.
pub fn sleep_confirm_body() -> Vec<u8> {
    let mut v = PS_SLEEP_CONFIRM.to_le_bytes().to_vec();
    v.extend(1u16.to_le_bytes());
    v
}

/// The TID of a block-ack session that ended (EVENT_DELBA body: result,
/// peer address, DELBA parameters).
pub fn delba_tid(ev: &[u8]) -> Option<u8> {
    (ev.len() >= 9).then(|| (le16(ev, 7) >> 12) as u8)
}

/// The 802_11_ASSOCIATE response: (capability, status code, association
/// ID). Status 0 is success; see [`assoc_failure`] for the rest.
pub fn associate_result(body: &[u8]) -> Option<(u16, u16, u16)> {
    (body.len() >= 6).then(|| (le16(body, 0), le16(body, 2), le16(body, 4) & 0x3fff))
}

/// Why an association failed, from the response's capability and status
/// fields (Linux `mwifiex_ret_802_11_associate`): the reason, and whether
/// the access point itself refused (rather than the firmware giving up
/// waiting for it). The firmware puts its own error in the capability
/// field: 0xfffc no answer (status 1: to the association, 2: to the
/// authentication, 3: joining), 0xfffd authentication refused, 0xfffe
/// an authentication frame it could not handle, 0xfffb or 0xffff a
/// failure of its own. Otherwise the status is the AP's 802.11 status
/// code.
pub fn assoc_failure(cap: u16, status: u16) -> (String, bool) {
    let why = match (cap, status) {
        (0xfffc, 1) => "no answer to the association request",
        (0xfffc, 2) => "no answer to authentication",
        (0xfffc, _) => "no answer from the access point",
        (0xfffd, _) => return (alloc::format!("authentication refused (status {status})"), true),
        (0xfffe, _) => "an authentication frame the firmware could not handle",
        (0xfffb | 0xffff, _) => "station failure in the firmware",
        _ => return (alloc::format!("refused by the access point (status {status})"), true),
    };
    (String::from(why), false)
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
pub const PKT_TYPE_MGMT: u16 = 0xe5;
pub const PKT_TYPE_AMSDU: u16 = 0xe6;
/// A Block Ack Request: no frame, its sequence number moves the reorder
/// window.
pub const PKT_TYPE_BAR: u16 = 0xe7;

/// The RxPD fields rs2 uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RxPd {
    pub kind: u16,
    pub len: usize,
    pub off: usize,
    /// 802.11 sequence number and TID, for block-ack reordering.
    pub seq: u16,
    pub tid: u8,
}

pub fn rxpd(b: &[u8]) -> Option<RxPd> {
    (b.len() >= 12).then(|| RxPd { kind: le16(b, 6), len: le16(b, 2) as usize, off: le16(b, 4) as usize, seq: le16(b, 8) & 0xfff, tid: b[10] & 7 })
}

/// An 802.3 frame (or an A-MSDU subframe) as an Ethernet II frame:
/// RFC 1042 and bridge-tunnel SNAP headers become the type field.
fn ethernet(p: &[u8]) -> Vec<u8> {
    let snap = p.len() >= 22 && p[14..17] == [0xaa, 0xaa, 0x03] && (p[17..20] == [0, 0, 0] || p[17..20] == [0, 0, 0xf8]);
    let mut f = Vec::with_capacity(p.len());
    f.extend(&p[..12]);
    f.extend(if snap { &p[20..] } else { &p[12..] });
    f
}

/// The subframes of an A-MSDU (destination, source, length, LLC/SNAP and
/// payload, each padded to 4 bytes) as Ethernet II frames.
pub fn amsdu_frames(mut b: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while b.len() >= 14 {
        let n = u16::from_be_bytes([b[12], b[13]]) as usize;
        if b.len() < 14 + n || n < 8 {
            break;
        }
        let mut f = Vec::with_capacity(14 + n);
        f.extend(&b[..12]);
        f.extend(&b[12..14]);
        f.extend(&b[14..14 + n]);
        out.push(ethernet(&f));
        let step = (14 + n + 3) & !3;
        if step >= b.len() {
            break;
        }
        b = &b[step..];
    }
    out
}

/// A received packet (after the interface header): its RxPD and the
/// Ethernet II frames it carries (none for a Block Ack Request, several
/// for an A-MSDU). None: a management frame or malformed.
pub fn rx_packet(b: &[u8]) -> Option<(RxPd, Vec<Vec<u8>>)> {
    let pd = rxpd(b)?;
    if pd.kind == PKT_TYPE_MGMT {
        return None;
    }
    if pd.kind == PKT_TYPE_BAR {
        return Some((pd, Vec::new()));
    }
    if pd.off < 12 || pd.off + 14 > b.len() {
        return None;
    }
    let p = &b[pd.off..(pd.off + pd.len).min(b.len())];
    if p.len() < 14 {
        return None;
    }
    let frames = if pd.kind == PKT_TYPE_AMSDU { amsdu_frames(p) } else { alloc::vec![ethernet(p)] };
    Some((pd, frames))
}

/// A received data packet (after the interface header: RxPD, then an
/// 802.3 frame with an LLC/SNAP header) as an Ethernet II frame.
/// None: not a data frame, or malformed.
pub fn rx_frame(b: &[u8]) -> Option<Vec<u8>> {
    let (pd, mut f) = rx_packet(b)?;
    (pd.kind != PKT_TYPE_AMSDU && f.len() == 1).then(|| f.remove(0))
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
        CMD_11N_CFG => "11N_CFG",
        CMD_11N_ADDBA_REQ => "11N_ADDBA_REQ",
        CMD_11N_ADDBA_RSP => "11N_ADDBA_RSP",
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
        let v = associate_body(&b, Some(&rsn), None);
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

    /// The 88W8897 on the Surface Pro 4: GET_HW_SPEC's capability words.
    fn surface_caps() -> Caps {
        Caps { fw_cap: 0x1878_ffa3, dot11n_cap: 0x6ff1_00c3 | 1 << 17 | 1 << 22 | 1 << 23 | 1 << 24 | 1 << 25, dev_mcs: 0x22, dot11ac_cap: 0x0390_51b2, dot11ac_mcs: 0xfffa_fffa }
    }

    fn ht_vht_network() -> Bss {
        let mut ies = Vec::new();
        let el = |v: &mut Vec<u8>, id: u8, b: &[u8]| {
            v.push(id);
            v.push(b.len() as u8);
            v.extend(b);
        };
        el(&mut ies, 0, b"house");
        el(&mut ies, 1, &[0x8c, 0x12, 0x98, 0x24, 0xb0, 0x48, 0x60, 0x6c]);
        el(&mut ies, 3, &[36]);
        let mut htcap = [0u8; 26];
        htcap[0] = 0xef;
        htcap[3] = 0xff;
        htcap[4] = 0xff;
        htcap[19] = 0x00;
        htcap[20] = 0x08; // reverse direction responder (cleared in ours)
        el(&mut ies, EID_HT_CAPABILITY, &htcap);
        let mut htop = [0u8; 22];
        htop[0] = 36;
        htop[1] = 0x05; // secondary above, any width
        el(&mut ies, EID_HT_OPERATION, &htop);
        el(&mut ies, EID_EXT_CAPABILITY, &[0, 0, 0, 0, 0, 0, 0, 0x40]);
        let mut vhtcap = [0u8; 12];
        vhtcap[4..6].copy_from_slice(&0xfffau16.to_le_bytes()); // MCS 0-9 for 1 stream, 0-8 for 2? (2 bits each)
        vhtcap[8..10].copy_from_slice(&0xfffeu16.to_le_bytes());
        el(&mut ies, EID_VHT_CAPABILITY, &vhtcap);
        el(&mut ies, EID_VHT_OPERATION, &[2, 42, 0, 0, 0]); // 160 MHz offered
        el(&mut ies, EID_VENDOR, &[0x00, 0x50, 0xf2, 0x02, 0x01, 0x01, 0x80]); // WMM parameter
        Bss::new([2, 0, 0, 0, 0, 1], -60, 100, 0x0411, &ies, 36)
    }

    #[test]
    fn caps_from_the_88w8897() {
        let c = surface_caps();
        assert!(c.ht() && c.vht() && c.ht40());
        assert_eq!(c.streams(), 2);
        let ht = c.ht_cap(ht_vht_network().element(EID_HT_CAPABILITY).unwrap());
        let info = u16::from_le_bytes([ht[0], ht[1]]);
        // LDPC, 20/40, SM PS off, greenfield, SGI 20/40, TX STBC, RX
        // STBC 2.
        assert_eq!(info & 0x03ff, 1 | 2 | 0xc | 0x10 | 0x20 | 0x40 | 0x80 | 0x200);
        // Beamforming (bit 30): Marvell's TX beamforming capabilities.
        assert_eq!(&ht[21..25], &HT_TX_BF_CAP.to_le_bytes());
        assert_eq!(ht[2], 3);
        assert_eq!(&ht[3..8], &[0xff, 0xff, 0, 0, 1]);
        assert_eq!(ht[15], 1);
        assert_eq!(u16::from_le_bytes([ht[19], ht[20]]) & 1 << 11, 0);
    }

    #[test]
    fn vht_mcs_maps_take_the_lower() {
        let c = surface_caps();
        let v = c.vht_cap(ht_vht_network().element(EID_VHT_CAPABILITY).unwrap());
        assert_eq!(u32::from_le_bytes([v[0], v[1], v[2], v[3]]) & VHT_CAP_BF_RESET, 0);
        // ours 0xfffa (2 streams MCS 0-9), theirs rx 0xfffa: 0xfffa;
        // tx: ours 0xfffa, theirs 0xfffe (1 stream): 0xfffe.
        assert_eq!(u16::from_le_bytes([v[4], v[5]]), 0xfffa);
        assert_eq!(u16::from_le_bytes([v[8], v[9]]), 0xfffe);
        assert_eq!(c.vht_max_width(), 1);
    }

    #[test]
    fn association_offers_ht_and_vht() {
        let b = ht_vht_network();
        let c = surface_caps();
        let legacy = associate_body(&b, None, None);
        let types: Vec<u16> = tlvs(&legacy[13..]).map(|t| t.0).collect();
        assert!(types.contains(&TLV_CHANLIST) && !types.contains(&(EID_HT_CAPABILITY as u16)));
        let v = associate_body(&b, None, Some(&c));
        let t: Vec<(u16, Vec<u8>)> = tlvs(&v[13..]).map(|(t, b)| (t, b.to_vec())).collect();
        let find = |id: u16| t.iter().find(|e| e.0 == id).map(|e| e.1.clone());
        assert_eq!(t.iter().filter(|e| e.0 == TLV_CHANLIST).count(), 1);
        // Channel 36, 5 GHz, secondary channel above.
        assert_eq!(find(TLV_CHANLIST).unwrap()[..2], [1 | 1 << 4, 36]);
        assert_eq!(find(EID_HT_CAPABILITY as u16).unwrap().len(), 26);
        assert_eq!(find(EID_EXT_CAPABILITY as u16).unwrap().len(), 8);
        assert_eq!(find(EID_VHT_CAPABILITY as u16).unwrap().len(), 12);
        // 160 MHz offered, 80 taken.
        assert_eq!(find(EID_VHT_OPERATION as u16).unwrap()[0], 1);
        assert_eq!(find(EID_VENDOR as u16).unwrap(), WMM_INFO.to_vec());
        // Without HT in the firmware: legacy only.
        let mut old = c;
        old.fw_cap &= !(1 << 11);
        assert_eq!(associate_body(&b, None, Some(&old)), legacy);
    }

    #[test]
    fn block_ack_sessions() {
        // ADDBA request: result, peer, token 5, TID 6 with window 64 and
        // A-MSDU allowed, timeout 0, SSN 0x123 (in the low 12 bits).
        let mut req = vec![0u8, 1, 2, 3, 4, 5, 6, 5];
        req.extend((1u16 | 2 << 0 | 6 << 2 | 64 << 6).to_le_bytes());
        req.extend(0u16.to_le_bytes());
        req.extend(0x123u16.to_le_bytes());
        let (rsp, tid, ssn, win) = addba_accept(&req, BA_WINDOW).unwrap();
        assert_eq!((tid, ssn, win), (6, 0x123, 32));
        assert_eq!(rsp.len(), 16);
        assert_eq!(&rsp[1..8], &req[1..8]);
        assert_eq!(&rsp[8..10], &[0, 0]);
        assert_eq!(le16(&rsp, 10) >> 6, 32);
        assert_eq!(le16(&rsp, 10) & 0x3f, le16(&req, 8) & 0x3f);
        assert_eq!(&rsp[12..16], &req[10..14]);
        let mut del = vec![0u8, 1, 2, 3, 4, 5, 6];
        del.extend((6u16 << 12 | 1 << 11).to_le_bytes());
        assert_eq!(delba_tid(&del), Some(6));
        assert_eq!(ht_cfg_body(), [1, 0, 0x72, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn amsdu_and_bar() {
        // Two subframes: an ARP payload with SNAP, then IPv4, padded.
        let sub = |da: u8, ty: [u8; 2], body: &[u8]| {
            let mut f = vec![da; 6];
            f.extend([9u8; 6]);
            let mut p = vec![0xaa, 0xaa, 3, 0, 0, 0];
            p.extend(ty);
            p.extend(body);
            f.extend((p.len() as u16).to_be_bytes());
            f.extend(p);
            f
        };
        let mut a = sub(1, [0x08, 0x06], &[1, 2, 3]);
        while a.len() % 4 != 0 {
            a.push(0);
        }
        a.extend(sub(2, [0x08, 0x00], &[4, 5]));
        let frames = amsdu_frames(&a);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0][..6], [1; 6]);
        assert_eq!(&frames[0][12..], &[0x08, 0x06, 1, 2, 3]);
        assert_eq!(&frames[1][12..], &[0x08, 0x00, 4, 5]);
        let mut b = vec![0u8; 20];
        b[2..4].copy_from_slice(&(a.len() as u16).to_le_bytes());
        b[4] = 20;
        b[6..8].copy_from_slice(&PKT_TYPE_AMSDU.to_le_bytes());
        b[8..10].copy_from_slice(&0x0457u16.to_le_bytes());
        b[10] = 5;
        b.extend(&a);
        let (pd, f) = rx_packet(&b).unwrap();
        assert_eq!((pd.seq, pd.tid, f.len()), (0x457, 5, 2));
        b[6..8].copy_from_slice(&PKT_TYPE_BAR.to_le_bytes());
        let (pd, f) = rx_packet(&b).unwrap();
        assert_eq!((pd.kind, f.len()), (PKT_TYPE_BAR, 0));
    }

    #[test]
    fn transmit_block_ack_and_power_save() {
        let b = addba_req_body([1, 2, 3, 4, 5, 6], 7, 0, BA_TX_WINDOW);
        assert_eq!(b.len(), 14);
        assert_eq!(&b[1..8], &[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(le16(&b, 8), 64 << 6 | 2);
        assert_eq!(le16(&b, 10), 0xffff);
        // Response: result, peer, token, status, parameters.
        let mut r = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 0, 0];
        r.extend((32u16 << 6 | 2).to_le_bytes());
        r.extend([0xff, 0xff, 0, 0]);
        assert_eq!(addba_req_result(&r), Ok((0, 32)));
        r[8] = 37; // request declined
        assert_eq!(addba_req_result(&r), Err(37));
        let on = ps_mode_body(true);
        assert_eq!(&on[..4], &[0xff, 0, 0x10, 0]);
        let t: Vec<(u16, Vec<u8>)> = tlvs(&on[4..]).map(|(t, v)| (t, v.to_vec())).collect();
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].0, t[0].1.len()), (TLV_PS_PARAM, 14));
        assert_eq!(le16(&t[0].1, 12), 1000);
        assert_eq!(ps_mode_body(false), [0xfe, 0, 0x10, 0]);
        assert_eq!(sleep_confirm_body(), [5, 0, 1, 0]);
    }

    #[test]
    fn association_failures() {
        assert_eq!(assoc_failure(0xfffc, 1), (String::from("no answer to the association request"), false));
        assert_eq!(assoc_failure(0xfffc, 2), (String::from("no answer to authentication"), false));
        assert_eq!(assoc_failure(0x0411, 17), (String::from("refused by the access point (status 17)"), true));
        assert!(assoc_failure(0xfffd, 13).1);
    }

    #[test]
    fn events() {
        assert_eq!(event(&[0x58, 0, 0, 0, 9]), Some((EVENT_EXT_SCAN_REPORT, &[9u8][..])));
    }
}
