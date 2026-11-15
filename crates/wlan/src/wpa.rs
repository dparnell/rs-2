//! The WPA2-Personal supplicant: the 4-way handshake and the group key
//! handshake (IEEE 802.11-2020 12.7.6, 12.7.7) for RSN networks with
//! pairwise CCMP, AKM PSK, and group CCMP or TKIP. Only EAPOL-Key
//! descriptor type 2 (RSN) with key descriptor version 2 (HMAC-SHA1-128
//! MIC, AES key wrap) is handled: no WPA1, no SHA256 AKMs, no SAE.
//!
//! The caller owns everything around it: it associates with our RSN
//! element, feeds every EAPOL frame from the AP to [`Supplicant::rx_eapol`]
//! and carries out the returned [`Action`]s in order (send frames, install
//! keys in the chip, report the link up or deauthenticate).

use alloc::vec;
use alloc::vec::Vec;

use crate::crypto::{aes_key_unwrap, hmac_sha1, prf};
use crate::ie::{EID_RSN, EID_VENDOR};

/// EAPOL packet type of an EAPOL-Key frame.
pub const EAPOL_KEY: u8 = 3;
/// Key descriptor type RSN.
pub const DESC_RSN: u8 = 2;

/// Key Information bits (12.7.2).
pub const KI_VERSION_MASK: u16 = 0b111;
/// Key descriptor version 2: HMAC-SHA1-128 MIC, AES key wrap.
pub const KI_VERSION_AES: u16 = 2;
/// Pairwise key (clear: group key).
pub const KI_PAIRWISE: u16 = 1 << 3;
pub const KI_INSTALL: u16 = 1 << 6;
pub const KI_ACK: u16 = 1 << 7;
pub const KI_MIC: u16 = 1 << 8;
pub const KI_SECURE: u16 = 1 << 9;
pub const KI_ERROR: u16 = 1 << 10;
pub const KI_REQUEST: u16 = 1 << 11;
pub const KI_ENC_KEY_DATA: u16 = 1 << 12;

/// Offsets in an EAPOL-Key packet, counted from the EAPOL version byte:
/// 4-byte EAPOL header, then the key descriptor.
const O_DESC: usize = 4;
const O_INFO: usize = 5;
const O_KEYLEN: usize = 7;
const O_REPLAY: usize = 9;
const O_NONCE: usize = 17;
const O_RSC: usize = 65;
const O_MIC: usize = 81;
const O_DATALEN: usize = 97;
/// Where the key data starts; also the shortest valid packet.
const O_DATA: usize = 99;

/// The KDE OUI (00-0F-AC) and the GTK KDE's data type.
const OUI_RSN: [u8; 3] = [0x00, 0x0f, 0xac];
const KDE_GTK: u8 = 1;

/// The pairwise transient key for CCMP (PRF-384).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ptk {
    /// Key confirmation key: the EAPOL-Key MIC.
    pub kck: [u8; 16],
    /// Key encryption key: wraps the key data.
    pub kek: [u8; 16],
    /// Temporal key: CCMP for unicast data.
    pub tk: [u8; 16],
}

impl Ptk {
    /// PRF-384(PMK, "Pairwise key expansion", min(AA, SPA) || max(AA, SPA)
    /// || min(ANonce, SNonce) || max(ANonce, SNonce)) (12.7.1.3).
    pub fn derive(pmk: &[u8; 32], aa: &[u8; 6], spa: &[u8; 6], anonce: &[u8; 32], snonce: &[u8; 32]) -> Ptk {
        let mut data = [0u8; 76];
        data[..6].copy_from_slice(aa.min(spa));
        data[6..12].copy_from_slice(aa.max(spa));
        data[12..44].copy_from_slice(anonce.min(snonce));
        data[44..].copy_from_slice(anonce.max(snonce));
        let mut k = [0u8; 48];
        prf(pmk, b"Pairwise key expansion", &data, &mut k);
        let part = |i: usize| -> [u8; 16] { k[i..i + 16].try_into().unwrap() };
        Ptk { kck: part(0), kek: part(16), tk: part(32) }
    }
}

/// The key MIC of an EAPOL-Key packet: HMAC-SHA1 with the KCK over the
/// whole packet with its MIC field zeroed, cut to 16 bytes.
pub fn key_mic(kck: &[u8; 16], pkt: &[u8]) -> [u8; 16] {
    let zero = [0u8; 16];
    let h = hmac_sha1(kck, &[&pkt[..O_MIC], &zero, &pkt[O_MIC + 16..]]);
    h[..16].try_into().unwrap()
}

/// Builds an EAPOL-Key packet (descriptor type RSN), with its MIC
/// computed if `kck` is given (`info` should then have [`KI_MIC`]).
pub fn build_key_frame(
    version: u8,
    info: u16,
    key_len: u16,
    replay: u64,
    nonce: &[u8; 32],
    rsc: u64,
    data: &[u8],
    kck: Option<&[u8; 16]>,
) -> Vec<u8> {
    let mut p = vec![0u8; O_DATA + data.len()];
    p[0] = version;
    p[1] = EAPOL_KEY;
    p[2..4].copy_from_slice(&((O_DATA - 4 + data.len()) as u16).to_be_bytes());
    p[O_DESC] = DESC_RSN;
    p[O_INFO..O_INFO + 2].copy_from_slice(&info.to_be_bytes());
    p[O_KEYLEN..O_KEYLEN + 2].copy_from_slice(&key_len.to_be_bytes());
    p[O_REPLAY..O_REPLAY + 8].copy_from_slice(&replay.to_be_bytes());
    p[O_NONCE..O_NONCE + 32].copy_from_slice(nonce);
    p[O_RSC..O_RSC + 8].copy_from_slice(&rsc.to_le_bytes());
    p[O_DATALEN..O_DATALEN + 2].copy_from_slice(&(data.len() as u16).to_be_bytes());
    p[O_DATA..].copy_from_slice(data);
    if let Some(kck) = kck {
        let mic = key_mic(kck, &p);
        p[O_MIC..O_MIC + 16].copy_from_slice(&mic);
    }
    p
}

/// The fields of a received EAPOL-Key packet the supplicant looks at.
pub struct KeyFrame<'a> {
    /// EAPOL protocol version.
    pub version: u8,
    pub info: u16,
    pub replay: u64,
    pub nonce: [u8; 32],
    /// Key RSC, little endian (the first byte is the lowest PN byte).
    pub rsc: u64,
    pub mic: [u8; 16],
    pub data: &'a [u8],
    /// The packet itself, cut to the length its EAPOL header gives
    /// (without any Ethernet padding): what the MIC covers.
    pub raw: &'a [u8],
}

impl KeyFrame<'_> {
    /// An EAPOL-Key packet of descriptor type RSN, or `None`.
    pub fn parse(pkt: &[u8]) -> Option<KeyFrame<'_>> {
        if pkt.len() < O_DATA || pkt[1] != EAPOL_KEY || pkt[O_DESC] != DESC_RSN {
            return None;
        }
        let len = 4 + u16::from_be_bytes([pkt[2], pkt[3]]) as usize;
        if len < O_DATA || len > pkt.len() {
            return None;
        }
        let raw = &pkt[..len];
        let dlen = u16::from_be_bytes([raw[O_DATALEN], raw[O_DATALEN + 1]]) as usize;
        if O_DATA + dlen > len {
            return None;
        }
        Some(KeyFrame {
            version: raw[0],
            info: u16::from_be_bytes([raw[O_INFO], raw[O_INFO + 1]]),
            replay: u64::from_be_bytes(raw[O_REPLAY..O_REPLAY + 8].try_into().unwrap()),
            nonce: raw[O_NONCE..O_NONCE + 32].try_into().unwrap(),
            rsc: u64::from_le_bytes(raw[O_RSC..O_RSC + 8].try_into().unwrap()),
            mic: raw[O_MIC..O_MIC + 16].try_into().unwrap(),
            data: &raw[O_DATA..O_DATA + dlen],
            raw,
        })
    }
}

/// Where the supplicant is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Nothing received yet.
    Idle,
    /// Message 1 seen and message 2 sent; waiting for message 3.
    PtkNegotiating,
    /// The PTK and GTK are installed; group rekeys and new 4-way
    /// handshakes (PTK rekey) are still answered.
    Done,
    /// A real failure (an [`Action::Failed`] was returned): everything
    /// else is ignored until a new `Supplicant` is made for a new
    /// association.
    Failed,
}

/// What the caller must do, in the order returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// An EAPOL packet for the AP (destination: the BSSID, ethertype
    /// 0x888E), sent unencrypted if no PTK is installed yet.
    Send(Vec<u8>),
    /// Install the pairwise CCMP key (key index 0) for the AP.
    InstallPtk { tk: [u8; 16] },
    /// Install a group key: 16 bytes for CCMP, 32 for TKIP (the cipher
    /// is the RSN element's group cipher). `rsc` is the receive sequence
    /// counter to start from (the first 6 bytes, little endian, are the
    /// CCMP PN or TKIP TSC).
    InstallGtk { key: Vec<u8>, index: u8, rsc: u64 },
    /// The 4-way handshake is finished: the port is open.
    Complete,
    /// The handshake cannot succeed (the caller should deauthenticate).
    Failed(&'static str),
}

pub struct Supplicant {
    pmk: [u8; 32],
    aa: [u8; 6],
    spa: [u8; 6],
    rsn_ie: Vec<u8>,
    ap_rsn_ie: Option<Vec<u8>>,
    snonce: [u8; 32],
    /// ANonce and PTK of the 4-way handshake in progress.
    anonce: Option<[u8; 32]>,
    tptk: Option<Ptk>,
    /// The PTK in use (after message 3).
    ptk: Option<Ptk>,
    /// The highest replay counter of a frame whose MIC checked out;
    /// later frames must exceed it.
    replay: Option<u64>,
    /// The installed GTK, so a repeated one is not installed again
    /// (which would reset its replay counter).
    gtk: Option<(u8, Vec<u8>)>,
    state: State,
}

impl Supplicant {
    /// `pmk`: from [`crate::crypto::psk`]; `aa`: the AP's MAC address
    /// (BSSID); `spa`: ours; `rsn_ie`: the whole RSN element (id 48,
    /// length, body) we sent in the (re)association request; `snonce`: 32
    /// random bytes.
    pub fn new(pmk: [u8; 32], aa: [u8; 6], spa: [u8; 6], rsn_ie: Vec<u8>, snonce: [u8; 32]) -> Supplicant {
        Supplicant {
            pmk,
            aa,
            spa,
            rsn_ie,
            ap_rsn_ie: None,
            snonce,
            anonce: None,
            tptk: None,
            ptk: None,
            replay: None,
            gtk: None,
            state: State::Idle,
        }
    }

    /// The AP's RSN element (id, length, body) from its beacon or probe
    /// response. Message 3 must repeat it exactly (a downgrade check);
    /// if it is never given, message 3's element is accepted as is.
    pub fn set_ap_rsn_ie(&mut self, ie: Vec<u8>) {
        self.ap_rsn_ie = Some(ie);
    }

    /// A fresh SNonce for the next message 1 (a PTK rekey); otherwise
    /// the first one is used again.
    pub fn set_snonce(&mut self, snonce: [u8; 32]) {
        self.snonce = snonce;
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The PTK in use, once message 3 has been accepted.
    pub fn ptk(&self) -> Option<&Ptk> {
        self.ptk.as_ref()
    }

    /// An EAPOL packet from the AP: the payload of an 802.3 frame with
    /// ethertype 0x888E, starting at the EAPOL version byte. Frames that
    /// are not EAPOL-Key, are malformed, replayed, unexpected or fail
    /// the MIC are dropped (an empty list): anyone can send them.
    pub fn rx_eapol(&mut self, pkt: &[u8]) -> Vec<Action> {
        if self.state == State::Failed {
            return Vec::new();
        }
        let Some(k) = KeyFrame::parse(pkt) else { return Vec::new() };
        if k.info & KI_VERSION_MASK != KI_VERSION_AES || k.info & (KI_REQUEST | KI_ERROR) != 0 || k.info & KI_ACK == 0 {
            return Vec::new();
        }
        if self.replay.is_some_and(|r| k.replay <= r) {
            return Vec::new();
        }
        if k.info & KI_PAIRWISE != 0 {
            if k.info & KI_MIC == 0 { self.msg1(&k) } else { self.msg3(&k) }
        } else {
            self.group1(&k)
        }
    }

    /// Message 1: derive a PTK from the ANonce and answer with message 2.
    /// Not protected, so it changes nothing that a real message 3 needs
    /// until message 3 verifies with the PTK it implies.
    fn msg1(&mut self, k: &KeyFrame) -> Vec<Action> {
        let ptk = Ptk::derive(&self.pmk, &self.aa, &self.spa, &k.nonce, &self.snonce);
        self.anonce = Some(k.nonce);
        self.tptk = Some(ptk);
        if self.state == State::Idle {
            self.state = State::PtkNegotiating;
        }
        let info = KI_VERSION_AES | KI_PAIRWISE | KI_MIC;
        let m2 = build_key_frame(eapol_version(k.version), info, 0, k.replay, &self.snonce, 0, &self.rsn_ie, Some(&ptk.kck));
        vec![Action::Send(m2)]
    }

    /// Message 3: install the PTK and the GTK it carries.
    fn msg3(&mut self, k: &KeyFrame) -> Vec<Action> {
        let need = KI_INSTALL | KI_SECURE | KI_ENC_KEY_DATA;
        let (Some(anonce), Some(ptk)) = (self.anonce, self.tptk) else { return Vec::new() };
        if k.info & need != need || k.nonce != anonce || key_mic(&ptk.kck, k.raw) != k.mic {
            return Vec::new();
        }
        self.replay = Some(k.replay);
        let Some(data) = aes_key_unwrap(&ptk.kek, k.data) else { return self.fail("message 3 key data does not unwrap") };
        let Some(ie) = find_rsn_ie(&data) else { return self.fail("message 3 has no RSN element") };
        if self.ap_rsn_ie.as_deref().is_some_and(|a| a != ie) {
            return self.fail("message 3 RSN element differs from the AP's beacon");
        }
        let Some((index, gtk)) = find_gtk(&data) else { return self.fail("message 3 has no GTK") };
        let info = KI_VERSION_AES | KI_PAIRWISE | KI_MIC | KI_SECURE;
        let m4 = build_key_frame(eapol_version(k.version), info, 0, k.replay, &[0; 32], 0, &[], Some(&ptk.kck));
        let mut out = vec![Action::Send(m4)];
        // A retransmitted message 3 (our message 4 was lost) is answered
        // again, but the keys already in use are not reinstalled: that
        // would reset their nonces (the KRACK attack).
        if self.ptk != Some(ptk) {
            out.push(Action::InstallPtk { tk: ptk.tk });
        }
        self.ptk = Some(ptk);
        self.install_gtk(index, gtk, k.rsc, &mut out);
        if self.state != State::Done {
            self.state = State::Done;
            out.push(Action::Complete);
        }
        out
    }

    /// Group key message 1: a new GTK, answered with group message 2.
    fn group1(&mut self, k: &KeyFrame) -> Vec<Action> {
        let need = KI_MIC | KI_SECURE | KI_ENC_KEY_DATA;
        let Some(ptk) = self.ptk else { return Vec::new() };
        if k.info & need != need || key_mic(&ptk.kck, k.raw) != k.mic {
            return Vec::new();
        }
        self.replay = Some(k.replay);
        let Some(data) = aes_key_unwrap(&ptk.kek, k.data) else { return self.fail("group message 1 key data does not unwrap") };
        let Some((index, gtk)) = find_gtk(&data) else { return self.fail("group message 1 has no GTK") };
        let info = KI_VERSION_AES | KI_MIC | KI_SECURE;
        let m2 = build_key_frame(eapol_version(k.version), info, 0, k.replay, &[0; 32], 0, &[], Some(&ptk.kck));
        let mut out = vec![Action::Send(m2)];
        self.install_gtk(index, gtk, k.rsc, &mut out);
        out
    }

    fn install_gtk(&mut self, index: u8, key: &[u8], rsc: u64, out: &mut Vec<Action>) {
        if self.gtk.as_ref().is_some_and(|(i, g)| *i == index && g == key) {
            return;
        }
        self.gtk = Some((index, key.to_vec()));
        out.push(Action::InstallGtk { key: key.to_vec(), index, rsc });
    }

    fn fail(&mut self, why: &'static str) -> Vec<Action> {
        self.state = State::Failed;
        vec![Action::Failed(why)]
    }
}

/// Replies use the AP's EAPOL version (1 or 2; anything else becomes 1).
fn eapol_version(v: u8) -> u8 {
    if v == 2 { 2 } else { 1 }
}

/// Elements and KDEs in decrypted key data. The padding (0xdd then
/// zeros) ends the walk.
fn key_data_items(data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    crate::ie::elements(data).take_while(|&(id, body)| !(id == EID_VENDOR && body.is_empty()))
}

/// The first RSN element in key data, with its id and length.
fn find_rsn_ie(data: &[u8]) -> Option<&[u8]> {
    let mut off = 0;
    for (id, body) in key_data_items(data) {
        if id == EID_RSN {
            return Some(&data[off..off + 2 + body.len()]);
        }
        off += 2 + body.len();
    }
    None
}

/// The GTK KDE: (key index, key).
fn find_gtk(data: &[u8]) -> Option<(u8, &[u8])> {
    key_data_items(data).find_map(|(id, b)| {
        (id == EID_VENDOR && b.len() > 6 && b[..3] == OUI_RSN && b[3] == KDE_GTK).then(|| (b[4] & 3, &b[6..]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{aes_key_wrap, psk};

    const AA: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    const SPA: [u8; 6] = [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
    /// RSN element: version 1, group CCMP, pairwise CCMP, AKM PSK.
    const RSN_IE: [u8; 22] = [
        48, 20, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 2, 0, 0,
    ];

    /// Just enough of an authenticator (the AP side) to drive the
    /// supplicant, built from the same primitives.
    struct Auth {
        pmk: [u8; 32],
        anonce: [u8; 32],
        replay: u64,
        ptk: Option<Ptk>,
        gtk: Vec<u8>,
        gtk_index: u8,
        rsn_ie: Vec<u8>,
    }

    impl Auth {
        fn new(pmk: [u8; 32]) -> Auth {
            Auth {
                pmk,
                anonce: core::array::from_fn(|i| 0xa0 ^ i as u8),
                replay: 1,
                ptk: None,
                gtk: (0x40..0x50).collect(),
                gtk_index: 1,
                rsn_ie: RSN_IE.to_vec(),
            }
        }

        fn msg1(&mut self) -> Vec<u8> {
            self.replay += 1;
            build_key_frame(2, KI_VERSION_AES | KI_PAIRWISE | KI_ACK, 16, self.replay, &self.anonce, 0, &[], None)
        }

        /// Checks message 2 and derives the PTK from its SNonce.
        fn rx_msg2(&mut self, p: &[u8]) {
            let k = KeyFrame::parse(p).unwrap();
            assert_eq!(k.info, KI_VERSION_AES | KI_PAIRWISE | KI_MIC);
            assert_eq!(k.replay, self.replay);
            assert_eq!(k.version, 2);
            assert_eq!(k.data, &RSN_IE);
            let ptk = Ptk::derive(&self.pmk, &AA, &SPA, &self.anonce, &k.nonce);
            assert_eq!(key_mic(&ptk.kck, k.raw), k.mic);
            self.ptk = Some(ptk);
        }

        fn wrapped(&self, ies: &[u8]) -> Vec<u8> {
            let mut d = ies.to_vec();
            d.extend_from_slice(&[0xdd, 6 + self.gtk.len() as u8, 0x00, 0x0f, 0xac, 1, self.gtk_index, 0]);
            d.extend_from_slice(&self.gtk);
            // Padding (12.7.2): 0xdd then zeros up to a multiple of 8,
            // at least 16 bytes.
            if d.len() % 8 != 0 || d.len() < 16 {
                d.push(0xdd);
                while d.len() % 8 != 0 || d.len() < 16 {
                    d.push(0);
                }
            }
            aes_key_wrap(&self.ptk.unwrap().kek, &d)
        }

        fn msg3(&mut self) -> Vec<u8> {
            self.replay += 1;
            let data = self.wrapped(&self.rsn_ie);
            let info = KI_VERSION_AES | KI_PAIRWISE | KI_INSTALL | KI_ACK | KI_MIC | KI_SECURE | KI_ENC_KEY_DATA;
            build_key_frame(2, info, 16, self.replay, &self.anonce, 0x1234, &data, Some(&self.ptk.unwrap().kck))
        }

        fn rx_msg4(&self, p: &[u8], info: u16) {
            let k = KeyFrame::parse(p).unwrap();
            assert_eq!(k.info, info);
            assert_eq!(k.replay, self.replay);
            assert!(k.data.is_empty());
            assert_eq!(key_mic(&self.ptk.unwrap().kck, k.raw), k.mic);
        }

        fn group1(&mut self) -> Vec<u8> {
            self.replay += 1;
            let data = self.wrapped(&[]);
            let info = KI_VERSION_AES | KI_ACK | KI_MIC | KI_SECURE | KI_ENC_KEY_DATA;
            build_key_frame(2, info, 0, self.replay, &[0; 32], 0x99, &data, Some(&self.ptk.unwrap().kck))
        }
    }

    fn setup() -> (Auth, Supplicant) {
        let pmk = psk(b"correct horse", b"rs2-test");
        let s = Supplicant::new(pmk, AA, SPA, RSN_IE.to_vec(), [0x5a; 32]);
        (Auth::new(pmk), s)
    }

    fn sent(a: &Action) -> &[u8] {
        match a {
            Action::Send(p) => p,
            _ => panic!("expected Send, got {:?}", a),
        }
    }

    /// Runs messages 1 and 2; returns message 3.
    fn to_msg3(a: &mut Auth, s: &mut Supplicant) -> Vec<u8> {
        let r = s.rx_eapol(&a.msg1());
        assert_eq!(r.len(), 1);
        a.rx_msg2(sent(&r[0]));
        assert_eq!(s.state(), State::PtkNegotiating);
        a.msg3()
    }

    #[test]
    fn four_way_and_group_rekey() {
        let (mut a, mut s) = setup();
        s.set_ap_rsn_ie(RSN_IE.to_vec());
        assert_eq!(s.state(), State::Idle);
        let m3 = to_msg3(&mut a, &mut s);
        let r = s.rx_eapol(&m3);
        assert_eq!(r.len(), 4);
        a.rx_msg4(sent(&r[0]), KI_VERSION_AES | KI_PAIRWISE | KI_MIC | KI_SECURE);
        assert_eq!(r[1], Action::InstallPtk { tk: a.ptk.unwrap().tk });
        assert_eq!(r[2], Action::InstallGtk { key: a.gtk.clone(), index: 1, rsc: 0x1234 });
        assert_eq!(r[3], Action::Complete);
        assert_eq!(s.state(), State::Done);
        assert_eq!(s.ptk(), a.ptk.as_ref());

        // Group rekey: a 32-byte TKIP group key with index 2.
        a.gtk = (0x60..0x80).collect();
        a.gtk_index = 2;
        let r = s.rx_eapol(&a.group1());
        assert_eq!(r.len(), 2);
        a.rx_msg4(sent(&r[0]), KI_VERSION_AES | KI_MIC | KI_SECURE);
        assert_eq!(r[1], Action::InstallGtk { key: a.gtk.clone(), index: 2, rsc: 0x99 });

        // The same group message again is a replay; a new one with the
        // same key is answered but not installed twice.
        let g = a.group1();
        assert_eq!(s.rx_eapol(&g).len(), 1);
        assert!(s.rx_eapol(&g).is_empty());
    }

    #[test]
    fn retransmitted_msg1_and_msg3() {
        let (mut a, mut s) = setup();
        // The AP repeats message 1 (message 2 lost): both are answered.
        let r = s.rx_eapol(&a.msg1());
        a.rx_msg2(sent(&r[0]));
        let m3 = to_msg3(&mut a, &mut s);
        assert_eq!(s.rx_eapol(&m3).len(), 4);
        // Message 4 lost: the AP sends message 3 again with a higher
        // counter; message 4 is repeated, no keys are reinstalled.
        let r = s.rx_eapol(&a.msg3());
        assert_eq!(r.len(), 1);
        a.rx_msg4(sent(&r[0]), KI_VERSION_AES | KI_PAIRWISE | KI_MIC | KI_SECURE);
    }

    #[test]
    fn bad_mic_in_msg3() {
        let (mut a, mut s) = setup();
        let mut m3 = to_msg3(&mut a, &mut s);
        m3[O_MIC] ^= 1;
        assert!(s.rx_eapol(&m3).is_empty());
        // A tampered key data byte also breaks the MIC.
        let mut m3 = a.msg3();
        m3[O_DATA + 3] ^= 1;
        assert!(s.rx_eapol(&m3).is_empty());
        assert_eq!(s.state(), State::PtkNegotiating);
        // The real one still works.
        assert_eq!(s.rx_eapol(&a.msg3()).len(), 4);
    }

    #[test]
    fn replayed_msg3() {
        let (mut a, mut s) = setup();
        let m3 = to_msg3(&mut a, &mut s);
        assert_eq!(s.rx_eapol(&m3).len(), 4);
        assert!(s.rx_eapol(&m3).is_empty());
        assert_eq!(s.state(), State::Done);
    }

    #[test]
    fn anonce_mismatch() {
        let (mut a, mut s) = setup();
        to_msg3(&mut a, &mut s);
        a.anonce[0] ^= 1;
        assert!(s.rx_eapol(&a.msg3()).is_empty());
        assert_eq!(s.state(), State::PtkNegotiating);
    }

    #[test]
    fn rsn_ie_mismatch_fails() {
        let (mut a, mut s) = setup();
        let mut beacon = RSN_IE.to_vec();
        beacon[7] = 2; // the beacon offered a TKIP group cipher
        s.set_ap_rsn_ie(beacon);
        let m3 = to_msg3(&mut a, &mut s);
        assert!(matches!(s.rx_eapol(&m3)[..], [Action::Failed(_)]));
        assert_eq!(s.state(), State::Failed);
    }

    #[test]
    fn wrong_passphrase() {
        let (mut a, _) = setup();
        let mut s = Supplicant::new(psk(b"wrong horse", b"rs2-test"), AA, SPA, RSN_IE.to_vec(), [0x5a; 32]);
        let r = s.rx_eapol(&a.msg1());
        // The AP sees a bad MIC on message 2 and never sends message 3.
        let k = KeyFrame::parse(sent(&r[0])).unwrap();
        let ptk = Ptk::derive(&a.pmk, &AA, &SPA, &a.anonce, &k.nonce);
        assert_ne!(key_mic(&ptk.kck, k.raw), k.mic);
    }

    #[test]
    fn ptk_symmetric_in_addresses_and_nonces() {
        let pmk = [7; 32];
        let (n1, n2) = ([1; 32], [2; 32]);
        assert_eq!(Ptk::derive(&pmk, &AA, &SPA, &n1, &n2), Ptk::derive(&pmk, &SPA, &AA, &n2, &n1));
        assert_ne!(Ptk::derive(&pmk, &AA, &SPA, &n1, &n2).tk, Ptk::derive(&pmk, &AA, &SPA, &n1, &[3; 32]).tk);
    }

    #[test]
    fn ethernet_padding_and_junk() {
        let (mut a, mut s) = setup();
        let mut m1 = a.msg1();
        m1.extend_from_slice(&[0; 20]);
        assert_eq!(s.rx_eapol(&m1).len(), 1);
        assert!(s.rx_eapol(&[2, 0, 0, 0]).is_empty());
        assert!(s.rx_eapol(&m1[..50]).is_empty());
    }
}
