//! rs2's small cryptography library, no_std. What WPA2-Personal needs
//! (IEEE 802.11-2020 12.7): SHA-1, HMAC-SHA1, PBKDF2 (passphrase to PSK),
//! the 802.11 PRF (PMK to PTK), AES-128 and the RFC 3394 key wrap (key
//! data in EAPOL-Key frames); and for the system keychain (`keychain`
//! crate) AES-128-CTR and a constant-time comparison. CCMP itself runs in
//! the WiFi chip's firmware. Simple byte-at-a-time code, not fast and
//! (apart from [`ct_eq`]) not constant-time: fine for a few handshakes
//! and keychain saves.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// AES-128 in counter mode (NIST SP 800-38A): `data` is XORed with
/// the encrypted counter blocks, the 128-bit big-endian counter starting at
/// `iv`. The same call encrypts and decrypts.
pub fn aes128_ctr(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) {
    let aes = Aes128::new(key);
    let mut ctr = u128::from_be_bytes(*iv);
    for chunk in data.chunks_mut(16) {
        let mut ks = ctr.to_be_bytes();
        aes.encrypt_block(&mut ks);
        for (d, k) in chunk.iter_mut().zip(ks) {
            *d ^= k;
        }
        ctr = ctr.wrapping_add(1);
    }
}

/// Equal contents, compared in a time that does not depend on where they
/// differ (for MACs).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Streaming SHA-1 (FIPS 180-4).
#[derive(Clone)]
pub struct Sha1 {
    h: [u32; 5],
    buf: [u8; 64],
    n: usize,
    len: u64,
}

impl Sha1 {
    pub fn new() -> Sha1 {
        Sha1 { h: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0], buf: [0; 64], n: 0, len: 0 }
    }

    pub fn update(&mut self, mut d: &[u8]) {
        self.len += d.len() as u64;
        while !d.is_empty() {
            let k = (64 - self.n).min(d.len());
            self.buf[self.n..self.n + k].copy_from_slice(&d[..k]);
            self.n += k;
            d = &d[k..];
            if self.n == 64 {
                let b = self.buf;
                self.block(&b);
                self.n = 0;
            }
        }
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.len * 8;
        self.update(&[0x80]);
        while self.n != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = [0; 20];
        for (o, h) in out.chunks_mut(4).zip(self.h) {
            o.copy_from_slice(&h.to_be_bytes());
        }
        out
    }

    fn block(&mut self, b: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = self.h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5a827999),
                1 => (b ^ c ^ d, 0x6ed9eba1),
                2 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (h, v) in self.h.iter_mut().zip([a, b, c, d, e]) {
            *h = h.wrapping_add(v);
        }
    }
}

pub fn sha1(d: &[u8]) -> [u8; 20] {
    let mut s = Sha1::new();
    s.update(d);
    s.finish()
}

/// HMAC-SHA1 (RFC 2104) of the concatenation of `parts`.
pub fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha1::new();
    inner.update(&k.map(|b| b ^ 0x36));
    for p in parts {
        inner.update(p);
    }
    let mut outer = Sha1::new();
    outer.update(&k.map(|b| b ^ 0x5c));
    outer.update(&inner.finish());
    outer.finish()
}

/// PBKDF2 with HMAC-SHA1 (RFC 8018), filling `out`.
pub fn pbkdf2_sha1(pass: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(20).enumerate() {
        let mut u = hmac_sha1(pass, &[salt, &(i as u32 + 1).to_be_bytes()]);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha1(pass, &[&u]);
            for (a, b) in t.iter_mut().zip(u) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

/// The WPA2-Personal PSK (and so the PMK) for a passphrase of 8 to 63
/// characters (IEEE 802.11-2020 J.4.1).
pub fn psk(passphrase: &[u8], ssid: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    pbkdf2_sha1(passphrase, ssid, 4096, &mut out);
    out
}

/// The 802.11 PRF (12.7.1.2): HMAC-SHA1(K, label || 0 || data || i) for
/// i = 0, 1, ... concatenated and cut to `out.len()` bytes.
pub fn prf(key: &[u8], label: &[u8], data: &[u8], out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(20).enumerate() {
        let h = hmac_sha1(key, &[label, &[0], data, &[i as u8]]);
        chunk.copy_from_slice(&h[..chunk.len()]);
    }
}

const fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0;
    while b != 0 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = (a << 1) ^ if a & 0x80 != 0 { 0x1b } else { 0 };
        b >>= 1;
    }
    r
}

/// The AES S-box and its inverse, computed at compile time from the
/// definition (multiplicative inverse in GF(2^8), then the affine map).
const SBOXES: ([u8; 256], [u8; 256]) = {
    let mut s = [0u8; 256];
    let mut inv = [0u8; 256];
    let mut x = 0;
    while x < 256 {
        let mut y = 0u8;
        if x != 0 {
            let mut c = 1;
            while c < 256 {
                if gmul(x as u8, c as u8) == 1 {
                    y = c as u8;
                    break;
                }
                c += 1;
            }
        }
        let v = y ^ y.rotate_left(1) ^ y.rotate_left(2) ^ y.rotate_left(3) ^ y.rotate_left(4) ^ 0x63;
        s[x] = v;
        inv[v as usize] = x as u8;
        x += 1;
    }
    (s, inv)
};
const SBOX: [u8; 256] = SBOXES.0;
const INV_SBOX: [u8; 256] = SBOXES.1;

/// AES-128 (FIPS 197). Blocks are in byte order; the state is column
/// major, so byte `4 * c + r` is row r of column c.
pub struct Aes128 {
    rk: [[u8; 16]; 11],
}

impl Aes128 {
    pub fn new(key: &[u8; 16]) -> Aes128 {
        let mut rk = [[0u8; 16]; 11];
        rk[0] = *key;
        let mut rcon = 1u8;
        for r in 1..11 {
            let p = rk[r - 1];
            let mut t = [SBOX[p[13] as usize], SBOX[p[14] as usize], SBOX[p[15] as usize], SBOX[p[12] as usize]];
            t[0] ^= rcon;
            rcon = gmul(rcon, 2);
            for c in 0..4 {
                for i in 0..4 {
                    let prev = if c == 0 { t[i] } else { rk[r][4 * (c - 1) + i] };
                    rk[r][4 * c + i] = p[4 * c + i] ^ prev;
                }
            }
        }
        Aes128 { rk }
    }

    pub fn encrypt_block(&self, b: &mut [u8; 16]) {
        xor16(b, &self.rk[0]);
        for r in 1..11 {
            for x in b.iter_mut() {
                *x = SBOX[*x as usize];
            }
            // ShiftRows: row i moves left by i columns.
            let s = *b;
            for c in 0..4 {
                for i in 0..4 {
                    b[4 * c + i] = s[4 * ((c + i) % 4) + i];
                }
            }
            if r != 10 {
                for c in b.chunks_mut(4) {
                    let [a0, a1, a2, a3] = [c[0], c[1], c[2], c[3]];
                    c[0] = gmul(a0, 2) ^ gmul(a1, 3) ^ a2 ^ a3;
                    c[1] = a0 ^ gmul(a1, 2) ^ gmul(a2, 3) ^ a3;
                    c[2] = a0 ^ a1 ^ gmul(a2, 2) ^ gmul(a3, 3);
                    c[3] = gmul(a0, 3) ^ a1 ^ a2 ^ gmul(a3, 2);
                }
            }
            xor16(b, &self.rk[r]);
        }
    }

    pub fn decrypt_block(&self, b: &mut [u8; 16]) {
        xor16(b, &self.rk[10]);
        for r in (0..10).rev() {
            let s = *b;
            for c in 0..4 {
                for i in 0..4 {
                    b[4 * ((c + i) % 4) + i] = s[4 * c + i];
                }
            }
            for x in b.iter_mut() {
                *x = INV_SBOX[*x as usize];
            }
            xor16(b, &self.rk[r]);
            if r != 0 {
                for c in b.chunks_mut(4) {
                    let [a0, a1, a2, a3] = [c[0], c[1], c[2], c[3]];
                    c[0] = gmul(a0, 14) ^ gmul(a1, 11) ^ gmul(a2, 13) ^ gmul(a3, 9);
                    c[1] = gmul(a0, 9) ^ gmul(a1, 14) ^ gmul(a2, 11) ^ gmul(a3, 13);
                    c[2] = gmul(a0, 13) ^ gmul(a1, 9) ^ gmul(a2, 14) ^ gmul(a3, 11);
                    c[3] = gmul(a0, 11) ^ gmul(a1, 13) ^ gmul(a2, 9) ^ gmul(a3, 14);
                }
            }
        }
    }
}

fn xor16(b: &mut [u8; 16], k: &[u8; 16]) {
    for (x, y) in b.iter_mut().zip(k) {
        *x ^= y;
    }
}

const WRAP_IV: [u8; 8] = [0xa6; 8];

/// RFC 3394 AES key wrap: `p` is a multiple of 8 bytes, at least 16.
/// The result is 8 bytes longer.
pub fn aes_key_wrap(kek: &[u8; 16], p: &[u8]) -> Vec<u8> {
    assert!(p.len() % 8 == 0 && p.len() >= 16);
    let aes = Aes128::new(kek);
    let n = p.len() / 8;
    let mut a = WRAP_IV;
    let mut r: Vec<u8> = p.to_vec();
    for j in 0..6 {
        for i in 0..n {
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&a);
            b[8..].copy_from_slice(&r[8 * i..8 * i + 8]);
            aes.encrypt_block(&mut b);
            let t = (n * j + i + 1) as u64;
            for (x, (y, z)) in a.iter_mut().zip(b[..8].iter().zip(t.to_be_bytes())) {
                *x = y ^ z;
            }
            r[8 * i..8 * i + 8].copy_from_slice(&b[8..]);
        }
    }
    let mut out = Vec::with_capacity(p.len() + 8);
    out.extend_from_slice(&a);
    out.extend_from_slice(&r);
    out
}

/// RFC 3394 AES key unwrap; `None` if `c` is malformed or its integrity
/// check value is wrong (wrong KEK or tampered data).
pub fn aes_key_unwrap(kek: &[u8; 16], c: &[u8]) -> Option<Vec<u8>> {
    if c.len() % 8 != 0 || c.len() < 24 {
        return None;
    }
    let aes = Aes128::new(kek);
    let n = c.len() / 8 - 1;
    let mut a = [0u8; 8];
    a.copy_from_slice(&c[..8]);
    let mut r: Vec<u8> = c[8..].to_vec();
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut b = [0u8; 16];
            for (x, (y, z)) in b[..8].iter_mut().zip(a.iter().zip(t.to_be_bytes())) {
                *x = y ^ z;
            }
            b[8..].copy_from_slice(&r[8 * i..8 * i + 8]);
            aes.decrypt_block(&mut b);
            a.copy_from_slice(&b[..8]);
            r[8 * i..8 * i + 8].copy_from_slice(&b[8..]);
        }
    }
    if a == WRAP_IV { Some(r) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: Vec<u8> = s.bytes().filter(|b| b.is_ascii_hexdigit()).collect();
        s.chunks(2).map(|p| u8::from_str_radix(core::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
    }

    #[test]
    fn sha1_fips180() {
        assert_eq!(sha1(b"abc").to_vec(), hex("a9993e364706816aba3e25717850c26c9cd0d89d"));
        assert_eq!(sha1(b"").to_vec(), hex("da39a3ee5e6b4b0d3255bfef95601890afd80709"));
        assert_eq!(
            sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").to_vec(),
            hex("84983e441c3bd26ebaae4aa1f95129e5e54670f1")
        );
        // A million 'a's, fed in uneven pieces through the streaming API.
        let mut s = Sha1::new();
        let a = [b'a'; 1000];
        for i in 0..1000 {
            let k = i % 7 * 100;
            s.update(&a[..k]);
            s.update(&a[k..]);
        }
        assert_eq!(s.finish().to_vec(), hex("34aa973cd4c4daa4f61eeb2bdbad27316534016f"));
    }

    #[test]
    fn hmac_rfc2202() {
        assert_eq!(hmac_sha1(&[0x0b; 20], &[b"Hi There"]).to_vec(), hex("b617318655057264e28bc0b6fb378c8ef146be00"));
        assert_eq!(
            hmac_sha1(b"Jefe", &[b"what do ya want ", b"for nothing?"]).to_vec(),
            hex("effcdf6ae5eb2fa2d27416d5f184df9c259a7c79")
        );
        assert_eq!(hmac_sha1(&[0xaa; 20], &[&[0xdd; 50]]).to_vec(), hex("125d7342b9ac11cd91a39af48aa17b4f63f175d3"));
        assert_eq!(
            hmac_sha1(&[0xaa; 80], &[b"Test Using Larger Than Block-Size Key - Hash Key First"]).to_vec(),
            hex("aa4ae5e15272d00e95705637ce8a3b55ed402112")
        );
    }

    #[test]
    fn pbkdf2_rfc6070() {
        let mut out = [0; 20];
        pbkdf2_sha1(b"password", b"salt", 1, &mut out);
        assert_eq!(out.to_vec(), hex("0c60c80f961f0e71f3a9b524af6012062fe037a6"));
        pbkdf2_sha1(b"password", b"salt", 2, &mut out);
        assert_eq!(out.to_vec(), hex("ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957"));
        pbkdf2_sha1(b"password", b"salt", 4096, &mut out);
        assert_eq!(out.to_vec(), hex("4b007901b765489abead49d926f721d065a429c1"));
        let mut out = [0; 25];
        pbkdf2_sha1(b"passwordPASSWORDpassword", b"saltSALTsaltSALTsaltSALTsaltSALTsalt", 4096, &mut out);
        assert_eq!(out.to_vec(), hex("3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038"));
    }

    #[test]
    fn psk_annex_j() {
        assert_eq!(
            psk(b"password", b"IEEE").to_vec(),
            hex("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e")
        );
        assert_eq!(
            psk(b"ThisIsAPassword", b"ThisIsASSID").to_vec(),
            hex("0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af")
        );
    }

    #[test]
    fn prf_annex_j() {
        let mut out = [0; 64];
        prf(&[0x0b; 20], b"prefix", b"Hi There", &mut out);
        assert_eq!(
            out.to_vec(),
            hex("bcd4c650b30b9684951829e0d75f9d54b862175ed9f00606e17d8da35402ffee
                 75df78c3d31e0f889f012120c0862beb67753e7439ae242edb8373698356cf5a")
        );
        let mut out = [0; 48];
        prf(b"Jefe", b"prefix-2", b"what do ya want for nothing?", &mut out);
        assert_eq!(
            out.to_vec(),
            hex("47c4908e30c947521ad20be9053450ecbea23d3aa604b77326d8b3825ff7475c06f51fb9c5313d1e9f90d897d134b72e")
        );
    }

    #[test]
    fn aes_fips197() {
        let key: [u8; 16] = core::array::from_fn(|i| i as u8);
        let aes = Aes128::new(&key);
        let mut b: [u8; 16] = core::array::from_fn(|i| (i * 0x11) as u8);
        aes.encrypt_block(&mut b);
        assert_eq!(b.to_vec(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
        aes.decrypt_block(&mut b);
        assert_eq!(b, core::array::from_fn(|i| (i * 0x11) as u8));
        // FIPS 197 appendix B.
        let aes = Aes128::new(&hex("2b7e151628aed2a6abf7158809cf4f3c").try_into().unwrap());
        let mut b: [u8; 16] = hex("3243f6a8885a308d313198a2e0370734").try_into().unwrap();
        aes.encrypt_block(&mut b);
        assert_eq!(b.to_vec(), hex("3925841d02dc09fbdc118597196a0b32"));
    }

    #[test]
    fn ctr_sp800_38a() {
        // NIST SP 800-38A F.5.1, CTR-AES128.Encrypt, first two blocks.
        let key: [u8; 16] = hex("2b7e151628aed2a6abf7158809cf4f3c").try_into().unwrap();
        let iv: [u8; 16] = hex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff").try_into().unwrap();
        let mut d = hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");
        aes128_ctr(&key, &iv, &mut d);
        assert_eq!(d, hex("874d6191b620e3261bef6864990db6ce9806f66b7970fdff8617187bb9fffdff"));
        aes128_ctr(&key, &iv, &mut d[..20]);
        assert_eq!(&d[..16], &hex("6bc1bee22e409f96e93d7e117393172a")[..]);
        assert!(ct_eq(b"abc", b"abc") && !ct_eq(b"abc", b"abd") && !ct_eq(b"ab", b"abc"));
    }

    #[test]
    fn key_wrap_rfc3394() {
        let kek: [u8; 16] = core::array::from_fn(|i| i as u8);
        let p = hex("00112233445566778899aabbccddeeff");
        let c = aes_key_wrap(&kek, &p);
        assert_eq!(c, hex("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5"));
        assert_eq!(aes_key_unwrap(&kek, &c), Some(p));
        let mut bad = c.clone();
        bad[10] ^= 1;
        assert_eq!(aes_key_unwrap(&kek, &bad), None);
        assert_eq!(aes_key_unwrap(&kek, &c[..16]), None);
        let long: Vec<u8> = (0..48).collect();
        assert_eq!(aes_key_unwrap(&kek, &aes_key_wrap(&kek, &long)), Some(long));
    }
}
