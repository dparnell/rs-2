//! /EXEPACK: iterated data (EXEPACK:1, NE segments and LX pages) and the
//! LZ-style compression of OS/2 3.0 and later (EXEPACK:2, LX pages). The
//! formats are the ones the loader expands (crates/os2-exe `expand_iterated`
//! and `expand_exepack2`).

use alloc::vec::Vec;

/// Iterated records: `u16 count, u16 length, pattern`. Runs of one byte
/// become one record; the rest is copied in records of count 1.
pub fn iterate(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut lit_start = 0;
    let mut i = 0;
    let flush = |out: &mut Vec<u8>, from: usize, to: usize| {
        let mut p = from;
        while p < to {
            let n = (to - p).min(0xffff);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.extend_from_slice(&data[p..p + n]);
            p += n;
        }
    };
    while i < data.len() {
        let b = data[i];
        let mut r = 1;
        while i + r < data.len() && data[i + r] == b && r < 0xffff {
            r += 1;
        }
        // A run pays off when it saves more than a record header.
        if r >= 8 {
            flush(&mut out, lit_start, i);
            out.extend_from_slice(&(r as u16).to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(b);
            i += r;
            lit_start = i;
        } else {
            i += r;
        }
    }
    flush(&mut out, lit_start, data.len());
    out
}

/// EXEPACK:2 compression of one page.
pub fn exepack2(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut out = Vec::new();
    // Hash chains over 3-byte prefixes.
    const HBITS: usize = 12;
    let mut head = alloc::vec![usize::MAX; 1 << HBITS];
    let mut prev = alloc::vec![usize::MAX; n];
    let hash = |i: usize| -> usize {
        let v = (data[i] as usize) | (data[i + 1] as usize) << 8 | (data[i + 2] as usize) << 16;
        (v.wrapping_mul(2654435761) >> 8) & ((1 << HBITS) - 1)
    };
    let insert = |i: usize, head: &mut Vec<usize>, prev: &mut Vec<usize>| {
        if i + 3 <= n {
            let h = hash(i);
            prev[i] = head[h];
            head[h] = i;
        }
    };
    let mut lits: Vec<u8> = Vec::new();
    let flush_lits = |out: &mut Vec<u8>, lits: &mut Vec<u8>, keep: usize| {
        // Write all but `keep` literals as plain literal ops.
        while lits.len() > keep {
            let k = (lits.len() - keep).min(63);
            out.push((k as u8) << 2);
            out.extend_from_slice(&lits[..k]);
            lits.drain(..k);
        }
    };
    let mut i = 0;
    while i < n {
        // A run of one byte.
        let mut run = 1;
        while i + run < n && data[i + run] == data[i] && run < 255 {
            run += 1;
        }
        // The longest earlier match.
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 3 <= n {
            let mut c = head[hash(i)];
            let mut tries = 0;
            while c != usize::MAX && tries < 64 {
                let dist = i - c;
                if dist >= 4096 {
                    break;
                }
                let mut l = 0;
                while i + l < n && l < 63 && data[c + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = dist;
                }
                c = prev[c];
                tries += 1;
            }
        }
        if run >= 4 && run >= best_len {
            flush_lits(&mut out, &mut lits, 0);
            out.push(0);
            out.push(run as u8);
            out.push(data[i]);
            for k in i..i + run {
                insert(k, &mut head, &mut prev);
            }
            i += run;
            continue;
        }
        if best_len >= 3 {
            let (len, dist) = (best_len, best_dist);
            if lits.is_empty() && len <= 6 {
                let w = 2 | ((len - 3) << 2) | (dist << 4);
                out.extend_from_slice(&(w as u16).to_le_bytes());
            } else if lits.len() <= 3 && len <= 10 && dist < 512 {
                let w = 1 | (lits.len() << 2) | ((len - 3) << 4) | (dist << 7);
                out.extend_from_slice(&(w as u16).to_le_bytes());
                out.extend_from_slice(&lits);
                lits.clear();
            } else {
                flush_lits(&mut out, &mut lits, 15);
                let v = 3 | (lits.len() << 2) | (len << 6) | (dist << 12);
                out.extend_from_slice(&(v as u32).to_le_bytes()[..3]);
                out.extend_from_slice(&lits);
                lits.clear();
            }
            for k in i..i + len {
                insert(k, &mut head, &mut prev);
            }
            i += len;
            continue;
        }
        lits.push(data[i]);
        insert(i, &mut head, &mut prev);
        i += 1;
    }
    flush_lits(&mut out, &mut lits, 0);
    out
}
