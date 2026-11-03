//! The hashes and ciphers NTLM and SMB signing need: MD4 (RFC 1320),
//! MD5 (RFC 1321), SHA-256 (FIPS 180-4), HMAC (RFC 2104) and RC4.
//! Small and unoptimised: they run over a few hundred bytes per logon
//! and over each signed message.

use alloc::vec::Vec;

fn md_pad(data: &[u8], big_endian: bool) -> Vec<u8> {
    let mut m = Vec::with_capacity(data.len() + 72);
    m.extend_from_slice(data);
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    let bits = (data.len() as u64).wrapping_mul(8);
    if big_endian {
        m.extend_from_slice(&bits.to_be_bytes());
    } else {
        m.extend_from_slice(&bits.to_le_bytes());
    }
    m
}

fn words_le(block: &[u8]) -> [u32; 16] {
    let mut x = [0u32; 16];
    for (i, w) in x.iter_mut().enumerate() {
        *w = u32::from_le_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
    }
    x
}

pub fn md4(data: &[u8]) -> [u8; 16] {
    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in md_pad(data, false).chunks(64) {
        let x = words_le(block);
        let [mut a, mut b, mut c, mut d] = h;
        let f = |x: u32, y: u32, z: u32| (x & y) | (!x & z);
        let g = |x: u32, y: u32, z: u32| (x & y) | (x & z) | (y & z);
        let hh = |x: u32, y: u32, z: u32| x ^ y ^ z;
        for &i in &[0usize, 4, 8, 12] {
            a = a.wrapping_add(f(b, c, d)).wrapping_add(x[i]).rotate_left(3);
            d = d.wrapping_add(f(a, b, c)).wrapping_add(x[i + 1]).rotate_left(7);
            c = c.wrapping_add(f(d, a, b)).wrapping_add(x[i + 2]).rotate_left(11);
            b = b.wrapping_add(f(c, d, a)).wrapping_add(x[i + 3]).rotate_left(19);
        }
        for &i in &[0usize, 1, 2, 3] {
            let k = 0x5a827999u32;
            a = a.wrapping_add(g(b, c, d)).wrapping_add(x[i]).wrapping_add(k).rotate_left(3);
            d = d.wrapping_add(g(a, b, c)).wrapping_add(x[i + 4]).wrapping_add(k).rotate_left(5);
            c = c.wrapping_add(g(d, a, b)).wrapping_add(x[i + 8]).wrapping_add(k).rotate_left(9);
            b = b.wrapping_add(g(c, d, a)).wrapping_add(x[i + 12]).wrapping_add(k).rotate_left(13);
        }
        for &i in &[0usize, 2, 1, 3] {
            let k = 0x6ed9eba1u32;
            a = a.wrapping_add(hh(b, c, d)).wrapping_add(x[i]).wrapping_add(k).rotate_left(3);
            d = d.wrapping_add(hh(a, b, c)).wrapping_add(x[i + 8]).wrapping_add(k).rotate_left(9);
            c = c.wrapping_add(hh(d, a, b)).wrapping_add(x[i + 4]).wrapping_add(k).rotate_left(11);
            b = b.wrapping_add(hh(c, d, a)).wrapping_add(x[i + 12]).wrapping_add(k).rotate_left(15);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for i in 0..4 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_le_bytes());
    }
    out
}

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23,
    4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

fn md5_k(i: usize) -> u32 {
    // floor(abs(sin(i + 1)) * 2^32), precomputed.
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1,
        0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453,
        0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942,
        0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d,
        0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];
    K[i]
}

pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in md_pad(data, false).chunks(64) {
        let m = words_le(block);
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = b;
            b = b.wrapping_add(a.wrapping_add(f).wrapping_add(md5_k(i)).wrapping_add(m[g]).rotate_left(MD5_S[i]));
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for i in 0..4 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_le_bytes());
    }
    out
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    for block in md_pad(data, true).chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

fn hmac<const N: usize>(hash: impl Fn(&[u8]) -> [u8; N], key: &[u8], parts: &[&[u8]]) -> [u8; N] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..N].copy_from_slice(&hash(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::new();
    inner.extend(k.iter().map(|b| b ^ 0x36));
    for p in parts {
        inner.extend_from_slice(p);
    }
    let ih = hash(&inner);
    let mut outer = Vec::with_capacity(64 + N);
    outer.extend(k.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&ih);
    hash(&outer)
}

/// HMAC-MD5 over the concatenation of `parts`.
pub fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    hmac(md5, key, parts)
}

/// HMAC-SHA256 over the concatenation of `parts`.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    hmac(sha256, key, parts)
}

/// RC4 encryption (or decryption) of `data` in place.
pub fn rc4(key: &[u8], data: &mut [u8]) {
    let mut s = [0u8; 256];
    for (i, x) in s.iter_mut().enumerate() {
        *x = i as u8;
    }
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    for b in data.iter_mut() {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        *b ^= s[s[i as usize].wrapping_add(s[j as usize]) as usize];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> alloc::string::String {
        b.iter().map(|x| alloc::format!("{x:02x}")).collect()
    }

    #[test]
    fn md4_vectors() {
        assert_eq!(hex(&md4(b"")), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(hex(&md4(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(hex(&md4(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890")), "e33b4ddc9c38f2199c3e7b164fcc0536");
    }

    #[test]
    fn md5_vectors() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
    }

    #[test]
    fn sha256_vectors() {
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = alloc::vec![b'a'; 1000];
        assert_eq!(hex(&sha256(&long)), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn hmac_vectors() {
        // RFC 2104 / RFC 4231 test case 2.
        assert_eq!(hex(&hmac_md5(b"Jefe", &[b"what do ya want ", b"for nothing?"])), "750c783e6ab0b503eaa86e310a5db738");
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", &[b"what do ya want for nothing?"])),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn rc4_vector() {
        let mut d = *b"Plaintext";
        rc4(b"Key", &mut d);
        assert_eq!(hex(&d), "bbf316e8d940af0ad3");
    }
}
