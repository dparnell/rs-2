//! FTCOMP stream decoder: Huffman blocks producing an LZ token stream, which
//! is then expanded against a sliding window.

use alloc::vec::Vec;

use crate::Error;
use crate::huffman::{Bits, Builder, SYMBOLS, Tree};
use crate::tables;

/// Signature at the start of every compressed block.
pub const BLOCK_MAGIC: [u8; 4] = *b"fT19";
/// Escape byte of the LZ token stream.
const ESCAPE: u8 = 0x9e;
/// Bytes of history available before the first output byte of a chunk.
const WINDOW: usize = 0xcfdc;
/// Largest expanded size of one LZ chunk.
const CHUNK_MAX: usize = 0x3008;
/// Largest token stream one block may carry.
const TOKENS_MAX: usize = 0xfe00;
/// Entries of each move-to-front list.
const MRU_LEN: usize = 0x30;

/// Decoder for FTCOMP streams. Holds the two fixed trees, which are costly
/// to build, so reuse one instance for many streams.
pub struct Decompressor {
    builder: Builder,
    /// Tree for the per-block symbol counts.
    counts: Tree,
    /// Tree for the bytes that follow an LZ escape.
    extra: Tree,
}

impl Default for Decompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Decompressor {
    pub fn new() -> Self {
        let mut builder = Builder::new();
        let counts = fixed_tree(&mut builder, &tables::FREQ_A);
        let extra = fixed_tree(&mut builder, &tables::FREQ_B);
        Decompressor {
            builder,
            counts,
            extra,
        }
    }

    /// Decompress one stream (a file's data or its extended attributes):
    /// the 4-byte stream prefix followed by blocks. `size_hint` only
    /// preallocates.
    pub fn stream(&mut self, data: &[u8], size_hint: usize) -> Result<Vec<u8>, Error> {
        let mut window = Window::new(size_hint);
        let mut pos = data.len().min(4);
        while pos < data.len() {
            let rest = &data[pos..];
            if rest.len() < 4 || rest[..4] != BLOCK_MAGIC {
                // Not compressed: the rest of the stream is stored as is.
                window.out.extend_from_slice(rest);
                break;
            }
            let (tokens, used) = self.block(&rest[4..])?;
            pos += 4 + used;
            expand_tokens(&tokens, &mut window)?;
        }
        Ok(window.into_output())
    }

    /// Decode one block (after its signature) into LZ tokens. Returns the
    /// tokens and the number of input bytes consumed.
    fn block(&mut self, data: &[u8]) -> Result<(Vec<u8>, usize), Error> {
        let rd16 = |o: usize| -> Result<u16, Error> {
            data.get(o..o + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .ok_or(Error::Truncated)
        };
        let total = rd16(0)?;
        if total == 0xffff {
            // Entropy coding skipped: tokens stored verbatim.
            let len = rd16(2)? as usize;
            let tokens = data.get(4..4 + len).ok_or(Error::Truncated)?;
            return Ok((tokens.to_vec(), 4 + len));
        }
        let total = total as usize;
        if total > TOKENS_MAX {
            return Err(Error::Corrupt);
        }
        let hdr = data.get(2..6).ok_or(Error::Truncated)?;
        let weights = [hdr[0] as u16, hdr[1] as u16, hdr[2] as u16, hdr[3] as u16];
        let mut bits = Bits::new(data, 6);

        // Symbol counts of the two block trees, sent with the fixed tree.
        let mut counts = [0u16; SYMBOLS];
        let mut i = 0;
        while i < SYMBOLS {
            let s = bits.symbol(&self.counts).ok_or(Error::Corrupt)?;
            if s == 0x100 {
                let run = 16.min(SYMBOLS - i);
                counts[i..i + run].fill(0);
                i += run;
            } else {
                counts[i] = s & 0xff;
                i += 1;
            }
        }
        let (first, second) = self.block_trees(&counts, weights)?;

        let mut out: Vec<u8> = Vec::with_capacity(total + 8);
        // Positions of recent escape type bytes, and recent copied words.
        let mut esc_mru = Mru::new(16);
        let mut word_mru = Mru::new(15);
        // Last values of the five kinds of escape extra bytes.
        let mut last = [0u16; 5];
        let mut use_second = false;
        let mut state = 0u8;
        while out.len() < total {
            if state == 0 {
                let tree = if use_second {
                    second.as_ref()
                } else {
                    first.as_ref()
                };
                let s = bits
                    .symbol(tree.ok_or(Error::Corrupt)?)
                    .ok_or(Error::Corrupt)?;
                use_second = tables::CONTEXT[s as usize] != 0;
                let o = out.len();
                match s {
                    0..=0xff => out.push(s as u8),
                    0x100..=0x180 => {
                        let kind = (s - 0x100) as usize;
                        out.push(ESCAPE);
                        out.push(kind as u8);
                        state = tables::ESCAPE_STATE[kind];
                        if state != 0 {
                            esc_mru.push(o as u16 + 1);
                        }
                    }
                    0x181..=0x190 => {
                        // Repeat the word ending 2..17 bytes back.
                        let from = (o + 0x17f).checked_sub(s as usize).ok_or(Error::Corrupt)?;
                        let w = [out[from], out[from + 1]];
                        out.extend_from_slice(&w);
                        word_mru.push(u16::from_le_bytes(w));
                    }
                    0x191..=0x1a0 => {
                        let w = word_mru.take((s - 0x191) as usize);
                        out.extend_from_slice(&w.to_le_bytes());
                    }
                    _ => {
                        // Repeat a recent escape with its extra bytes.
                        let p = esc_mru.take((s - 0x1a1) as usize) as usize;
                        let at = |k: usize| out.get(p + k).copied().unwrap_or(0);
                        let kind = at(0);
                        let n = if kind == 0x80 {
                            4
                        } else if kind & 0x40 != 0 {
                            3
                        } else {
                            2
                        };
                        let rep: Vec<u8> = (0..n).map(at).collect();
                        out.push(ESCAPE);
                        out.extend_from_slice(&rep);
                        esc_mru.set_front(o as u16 + 1);
                    }
                }
            } else if state >= 3 {
                // Length byte and distance word of a long match.
                let s = bits.symbol(&self.extra).ok_or(Error::Corrupt)?;
                state += 1;
                let v = if state == 6 {
                    state = 0;
                    if s == 0x100 {
                        last[4]
                    } else {
                        last[4] = s;
                        s
                    }
                } else if s == 0x100 {
                    0
                } else {
                    s + 1
                };
                out.push(v as u8);
            } else {
                // Distance after a short match: one byte (state 1) or a word
                // made of a 4/6/7-bit low part and a tree-coded high part.
                let mut low = 0u16;
                let mut class = 0usize;
                if state == 2 {
                    bits.refill();
                    let b = bits.peek();
                    if b & 0x8000 == 0 {
                        low = (b >> 11) & 0xf;
                        bits.skip(5);
                        class = 0;
                    } else if b & 0x4000 == 0 {
                        low = (b >> 8) & 0x3f;
                        bits.skip(8);
                        class = 1;
                    } else {
                        low = (b >> 7) & 0x7f;
                        bits.skip(9);
                        class = 2;
                    }
                }
                let mut s = bits.symbol(&self.extra).ok_or(Error::Corrupt)?;
                if state == 1 {
                    if s == 0x100 {
                        s = last[0];
                    }
                    last[0] = s;
                    out.push(s as u8);
                } else {
                    let slot = 1 + class;
                    if s == 0x100 {
                        s = last[slot];
                    }
                    last[slot] = s;
                    let (bias, shift) = [(0x10u16, 4u32), (0x44, 6), (0xa2, 7)][class];
                    let v = (s.wrapping_add(bias) << shift).wrapping_add(low);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                state = 0;
            }
        }
        if bits.overran() {
            return Err(Error::Truncated);
        }
        Ok((out, bits.consumed()))
    }

    /// Build the two trees of a block from its symbol counts and weights.
    fn block_trees(
        &mut self,
        counts: &[u16; SYMBOLS],
        w: [u16; 4],
    ) -> Result<(Option<Tree>, Option<Tree>), Error> {
        let b = &mut self.builder;
        b.clear();
        weigh(b, counts, w[0], w[1]);
        let first = b.build();
        // The second tree is the first one unless the weights differ; with
        // both second weights zero it is rebuilt from the current state.
        let second = if w[2] == w[1] && w[3] == w[0] {
            first.clone()
        } else {
            if w[2] != 0 || w[3] != 0 {
                b.clear();
                weigh(b, counts, w[3], w[2]);
            }
            b.build()
        };
        Ok((first, second))
    }
}

/// Set leaf frequencies `count * weight` (weight chosen by the symbol's
/// context class), scaled down to fit when large.
fn weigh(b: &mut Builder, counts: &[u16; SYMBOLS], w_plain: u16, w_ctx: u16) {
    let mut max = 0u16;
    for (k, &c) in counts.iter().enumerate() {
        let f = if c == 0 {
            0
        } else {
            let f = c.wrapping_mul(if tables::CONTEXT[k] != 0 {
                w_ctx
            } else {
                w_plain
            });
            max = max.max(f);
            f
        };
        b.set_freq(k, f);
    }
    if max > 0xff {
        let factor = 0xffff / max;
        for k in 0..SYMBOLS {
            let f = b.freq(k);
            let mut g = f.wrapping_mul(factor) >> 8;
            if f != 0 && g == 0 {
                g = 1;
            }
            b.set_freq(k, g);
        }
    }
}

fn fixed_tree(b: &mut Builder, freq: &[u16; SYMBOLS]) -> Tree {
    b.clear();
    for (k, &f) in freq.iter().enumerate() {
        b.set_freq(k, f);
    }
    b.build().expect("fixed tree has symbols")
}

/// A move-to-front list kept as a downward-growing stack, as the format
/// defines it: when the stack runs out, its newest entries are copied to
/// slot 0x20 upwards and it restarts below them (16 entries are kept for
/// the escape list, 15 for the word list).
struct Mru {
    slots: [u16; MRU_LEN],
    top: usize,
    /// Entries kept when the stack wraps.
    keep: usize,
}

impl Mru {
    fn new(keep: usize) -> Self {
        Mru {
            slots: [0; MRU_LEN],
            top: 0x20,
            keep,
        }
    }

    fn push(&mut self, v: u16) {
        if self.top == 0 {
            self.slots.copy_within(0..self.keep, 0x20);
            self.top = 0x1f;
        } else {
            self.top -= 1;
        }
        self.slots[self.top] = v;
    }

    /// Fetch the `k`-th newest entry and move it to the front.
    fn take(&mut self, k: usize) -> u16 {
        let i = (self.top + k).min(MRU_LEN - 1);
        let v = self.slots[i];
        self.slots.copy_within(self.top..i, self.top + 1);
        self.slots[self.top] = v;
        v
    }

    /// Replace the front entry (after `take`, for escape positions: the
    /// repeated escape's own position becomes the newest).
    fn set_front(&mut self, v: u16) {
        self.slots[self.top] = v;
    }
}

/// Output plus the history the LZ references may reach.
struct Window {
    /// Initial window content followed by everything produced so far.
    out: Vec<u8>,
}

impl Window {
    fn new(size_hint: usize) -> Self {
        let mut out = Vec::with_capacity(WINDOW + size_hint);
        out.resize(0xbc62, 0);
        out.resize(0xbc62 + 0x140, b' ');
        out.resize(0xbc62 + 0x280, 0xff);
        out.resize(0xbc62 + 0x3c0, 0);
        out.extend_from_slice(&tables::DICTIONARY);
        debug_assert_eq!(out.len(), WINDOW);
        Window { out }
    }

    fn into_output(mut self) -> Vec<u8> {
        self.out.drain(..WINDOW);
        self.out
    }
}

/// Split a block's tokens into length-prefixed chunks and expand each.
fn expand_tokens(tokens: &[u8], w: &mut Window) -> Result<(), Error> {
    let mut p = 0;
    while p < tokens.len() {
        let len = tokens.get(p..p + 2).ok_or(Error::Corrupt)?;
        let len = u16::from_le_bytes([len[0], len[1]]) as usize;
        p += 2;
        if len > CHUNK_MAX || len == 0 {
            return Err(Error::Corrupt);
        }
        let chunk = tokens.get(p..p + len).ok_or(Error::Corrupt)?;
        p += len;
        if chunk[0] == 0 {
            w.out.extend_from_slice(&chunk[1..]);
        } else {
            expand_lz(&chunk[1..], w)?;
        }
    }
    Ok(())
}

/// Expand one LZ chunk. Literal bytes copy through; `0x9E t ...` is an
/// escape: `t == 0x40` is a literal 0x9E, `t == 0x80` a match of
/// `0x43 + byte` with a word distance, `t & 0x40` a match of `3 + (t & 0x3F)`
/// with a word distance, otherwise `3 + t` with a byte distance. A match
/// copies from `distance + 1` bytes back.
fn expand_lz(src: &[u8], w: &mut Window) -> Result<(), Error> {
    let start = w.out.len();
    let mut i = 0;
    while i < src.len() {
        let b = src[i];
        if b != ESCAPE {
            w.out.push(b);
            i += 1;
            continue;
        }
        let byte = |k: usize| src.get(i + k).copied().ok_or(Error::Corrupt);
        let t = byte(1)?;
        let (len, dist, used) = if t == 0x40 {
            w.out.push(ESCAPE);
            i += 2;
            continue;
        } else if t == 0x80 {
            (
                0x43 + byte(2)? as usize,
                u16::from_le_bytes([byte(3)?, byte(4)?]) as usize,
                5,
            )
        } else if t & 0x40 != 0 {
            (
                3 + (t & 0x3f) as usize,
                u16::from_le_bytes([byte(2)?, byte(3)?]) as usize,
                4,
            )
        } else {
            (3 + t as usize, byte(2)? as usize, 3)
        };
        let here = w.out.len();
        if dist >= WINDOW + (here - start) {
            return Err(Error::Corrupt);
        }
        let from = here - dist - 1;
        for k in 0..len {
            let v = w.out[from + k];
            w.out.push(v);
        }
        i += used;
    }
    if w.out.len() - start > CHUNK_MAX {
        return Err(Error::Corrupt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn mru_wraps_like_a_stack() {
        let mut m = Mru::new(16);
        for v in 1..=40u16 {
            m.push(v);
        }
        assert_eq!(m.take(0), 40);
        assert_eq!(m.take(3), 37);
        assert_eq!(m.take(0), 37);
        assert_eq!(m.take(1), 40);
    }

    #[test]
    fn lz_escapes() {
        let mut w = Window::new(0);
        // "ab", copy 3 from distance 1 ("aba"), literal escape byte.
        expand_lz(&[b'a', b'b', ESCAPE, 0x00, 0x01, ESCAPE, 0x40], &mut w).unwrap();
        assert_eq!(w.into_output(), b"ababa\x9e");
    }

    #[test]
    fn stored_block() {
        // Prefix, block magic, stored tokens: one LZ chunk of literals.
        let mut s = vec![0x80, 0x60, 0, 0];
        s.extend_from_slice(&BLOCK_MAGIC);
        s.extend_from_slice(&[0xff, 0xff, 6, 0, 4, 0, 1, b'x', b'y', b'z']);
        let out = Decompressor::new().stream(&s, 3).unwrap();
        assert_eq!(out, b"xyz");
    }
}
