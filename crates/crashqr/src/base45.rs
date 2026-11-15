//! Base45 (RFC 9285): two bytes become three characters of the QR
//! alphanumeric set, least significant first.

pub const CHARSET: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

/// Characters needed for `n` bytes.
pub fn encoded_len(n: usize) -> usize {
    n / 2 * 3 + n % 2 * 2
}

pub fn encode(data: &[u8], out: &mut [u8]) -> Option<usize> {
    let need = encoded_len(data.len());
    if out.len() < need {
        return None;
    }
    let mut o = 0;
    for pair in data.chunks(2) {
        if pair.len() == 2 {
            let n = pair[0] as usize * 256 + pair[1] as usize;
            out[o] = CHARSET[n % 45];
            out[o + 1] = CHARSET[n / 45 % 45];
            out[o + 2] = CHARSET[n / 2025];
            o += 3;
        } else {
            let n = pair[0] as usize;
            out[o] = CHARSET[n % 45];
            out[o + 1] = CHARSET[n / 45];
            o += 2;
        }
    }
    Some(o)
}

/// Decode into `out`; None on a bad character, length or value.
pub fn decode(text: &[u8], out: &mut [u8]) -> Option<usize> {
    let val = |c: u8| CHARSET.iter().position(|&x| x == c);
    if text.len() % 3 == 1 {
        return None;
    }
    let mut o = 0;
    for chunk in text.chunks(3) {
        let mut n = 0usize;
        for (i, &c) in chunk.iter().enumerate() {
            n += val(c)? * [1, 45, 2025][i];
        }
        if chunk.len() == 3 {
            if n > 0xffff || o + 2 > out.len() {
                return None;
            }
            out[o] = (n >> 8) as u8;
            out[o + 1] = n as u8;
            o += 2;
        } else {
            if n > 0xff || o >= out.len() {
                return None;
            }
            out[o] = n as u8;
            o += 1;
        }
    }
    Some(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc9285_examples() {
        let mut b = [0u8; 64];
        let n = encode(b"AB", &mut b).unwrap();
        assert_eq!(&b[..n], b"BB8");
        let n = encode(b"Hello!!", &mut b).unwrap();
        assert_eq!(&b[..n], b"%69 VD92EX0");
        let n = encode(b"base-45", &mut b).unwrap();
        assert_eq!(&b[..n], b"UJCLQE7W581");
        let n = decode(b"QED8WEX0", &mut b).unwrap();
        assert_eq!(&b[..n], b"ietf!");
        assert!(decode(b"GGW", &mut b).is_none()); // 65535 + 1
    }
}
