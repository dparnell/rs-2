//! Huffman trees of the FTCOMP entropy stage.
//!
//! The packer never transmits codes or code lengths: both sides build a tree
//! from symbol frequencies with the same deterministic procedure, so the
//! exact tie-breaking of that procedure is part of the format. The steps
//! below (seeding order, the quicksort/insertion sort hybrid, the binary
//! search used to re-insert merged nodes) reproduce it precisely.

use alloc::vec;
use alloc::vec::Vec;

/// Number of symbols (leaves) of every tree.
pub(crate) const SYMBOLS: usize = 0x1b1;
/// Leaves plus the at most `SYMBOLS - 1` internal nodes, rounded up the same
/// way the original sizes its node table.
const NODES: usize = 0x366;
/// Bits resolved by one table lookup.
const LOOKUP_BITS: u32 = 9;

#[derive(Clone, Copy, Default)]
struct Node {
    freq: u16,
    parent: u16,
    /// Child taken for a 1 bit, then for a 0 bit.
    child: [u16; 2],
}

/// Tree builder state: the node table and the work list. Building a tree
/// from the state left over by the previous build is meaningful (the format
/// relies on it in one case), so the state outlives a single build.
pub(crate) struct Builder {
    nodes: Vec<Node>,
    list: Vec<u16>,
}

/// A finished decoding tree.
#[derive(Clone)]
pub(crate) struct Tree {
    child: Vec<[u16; 2]>,
    /// For each 9-bit prefix: the node reached and the bits consumed.
    lookup: Vec<(u16, u8)>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Builder {
            nodes: vec![Node::default(); NODES],
            list: vec![0; 0x400],
        }
    }

    /// Clear all nodes (frequencies included).
    pub(crate) fn clear(&mut self) {
        self.nodes.fill(Node::default());
    }

    pub(crate) fn set_freq(&mut self, sym: usize, freq: u16) {
        self.nodes[sym].freq = freq;
    }

    pub(crate) fn freq(&self, sym: usize) -> u16 {
        self.nodes[sym].freq
    }

    fn f(&self, list_index: usize) -> u16 {
        self.nodes[self.list[list_index] as usize].freq
    }

    /// Order `list[lo..=hi]` by ascending frequency.
    ///
    /// Explicit-stack quicksort (middle element pivot; the larger part is
    /// pushed, the smaller one handled next) that switches to insertion sort
    /// for ranges of at most 17 entries. The stack lives in the list just
    /// above `hi`. Not stable: the resulting order of equal frequencies is
    /// part of the format.
    fn sort(&mut self, lo: usize, hi: usize) {
        let base = hi + 1;
        self.list[hi + 1] = lo as u16;
        self.list[hi + 2] = hi as u16;
        let mut sp = hi + 3;
        loop {
            sp -= 1;
            let mut hi = self.list[sp] as isize;
            sp -= 1;
            let mut lo = self.list[sp] as isize;
            while hi > lo {
                if hi - lo > 0x10 {
                    let pivot = self.f(((hi + lo) >> 1) as usize);
                    let mut i = lo;
                    let mut j = hi;
                    loop {
                        while self.f(i as usize) < pivot {
                            i += 1;
                        }
                        while self.f(j as usize) > pivot {
                            j -= 1;
                        }
                        if j >= i {
                            self.list.swap(i as usize, j as usize);
                            i += 1;
                            j -= 1;
                        }
                        if j < i {
                            break;
                        }
                    }
                    if hi - i > j - lo {
                        if hi > i {
                            self.list[sp] = i as u16;
                            self.list[sp + 1] = hi as u16;
                            sp += 2;
                        }
                        hi = j;
                    } else {
                        if lo < j {
                            self.list[sp] = lo as u16;
                            self.list[sp + 1] = j as u16;
                            sp += 2;
                        }
                        lo = i;
                    }
                } else {
                    let (lo_u, hi_u) = (lo as usize, hi as usize);
                    for k in lo_u + 1..=hi_u {
                        let v = self.list[k];
                        let fv = self.nodes[v as usize].freq;
                        let mut pos = lo_u;
                        if self.f(lo_u) < fv {
                            while pos < k {
                                pos += 1;
                                if self.f(pos) >= fv {
                                    break;
                                }
                            }
                        }
                        self.list.copy_within(pos..k, pos + 1);
                        self.list[pos] = v;
                    }
                    break;
                }
            }
            if sp == base {
                return;
            }
        }
    }

    /// Build a tree from the leaf frequencies currently in the node table.
    /// Returns `None` when no symbol has a nonzero frequency.
    pub(crate) fn build(&mut self) -> Option<Tree> {
        // Seed the list: frequency-1 leaves are gathered at the front (the
        // entry they displace moves to the end), the rest in symbol order.
        let mut n = 0usize;
        let mut ones = 0usize;
        let mut last_zero = 0u16;
        for k in 0..SYMBOLS {
            self.nodes[k].parent = 0;
            match self.nodes[k].freq {
                0 => last_zero = k as u16,
                1 => {
                    self.list[n] = self.list[ones];
                    n += 1;
                    self.list[ones] = k as u16;
                    ones += 1;
                }
                _ => {
                    self.list[n] = k as u16;
                    n += 1;
                }
            }
        }
        if n == 0 {
            return None;
        }
        if n == 1 {
            // A tree needs two leaves: borrow an unused symbol.
            self.list[n] = self.list[ones];
            n += 1;
            self.list[ones] = last_zero;
            ones += 1;
            self.nodes[last_zero as usize].freq = 1;
        }
        self.sort(ones, n - 1);

        // Repeatedly merge the two lowest entries; the merged node is put
        // back before the first entry whose frequency is not lower.
        let mut next = SYMBOLS;
        let mut i = 0usize;
        if n != 2 {
            let mut count = n;
            loop {
                count -= 1;
                let a = self.list[i];
                let b = self.list[i + 1];
                let lo = i + 2;
                i += 1;
                let sum = self.nodes[b as usize]
                    .freq
                    .wrapping_add(self.nodes[a as usize].freq);
                let mut mid = (n + lo) >> 1;
                if n > lo {
                    let (mut l, mut h) = (lo, n);
                    let mut m = mid;
                    loop {
                        if self.f(m) < sum {
                            l = m + 1;
                        } else {
                            h = m;
                        }
                        m = (h + l) >> 1;
                        if h <= l {
                            break;
                        }
                    }
                    mid = m;
                }
                self.list.copy_within(i + 1..mid, i);
                self.list[mid - 1] = next as u16;
                self.join(next, a, b, sum);
                next += 1;
                if count == 2 {
                    break;
                }
            }
        }
        let a = self.list[i];
        let b = self.list[i + 1];
        let sum = self.nodes[b as usize]
            .freq
            .wrapping_add(self.nodes[a as usize].freq);
        self.join(next, a, b, sum);
        let root = next as u16;

        let child: Vec<[u16; 2]> = self.nodes.iter().map(|n| n.child).collect();
        let mut lookup = Vec::with_capacity(1 << LOOKUP_BITS);
        for p in 0..1u32 << LOOKUP_BITS {
            let mut bits = (p << (16 - LOOKUP_BITS)) as u16;
            let mut node = root;
            let mut used = 0u8;
            loop {
                node = child[node as usize][(bits & 0x8000 == 0) as usize];
                bits <<= 1;
                used += 1;
                if (node as usize) < SYMBOLS || used as u32 >= LOOKUP_BITS {
                    break;
                }
            }
            lookup.push((node, used));
        }
        Some(Tree { child, lookup })
    }

    fn join(&mut self, at: usize, a: u16, b: u16, sum: u16) {
        self.nodes[at] = Node {
            freq: sum,
            parent: 0,
            child: [a, b],
        };
        self.nodes[a as usize].parent = at as u16;
        self.nodes[b as usize].parent = at as u16;
    }
}

/// MSB-first bit reader over one block.
pub(crate) struct Bits<'a> {
    data: &'a [u8],
    pub(crate) pos: usize,
    buf: u16,
    count: u32,
    /// Bytes requested beyond the end of `data`.
    overrun: usize,
}

impl<'a> Bits<'a> {
    pub(crate) fn new(data: &'a [u8], pos: usize) -> Self {
        Bits {
            data,
            pos,
            buf: 0,
            count: 0,
            overrun: 0,
        }
    }

    fn byte(&mut self) -> u8 {
        let b = match self.data.get(self.pos) {
            Some(&b) => b,
            None => {
                self.overrun += 1;
                0
            }
        };
        self.pos += 1;
        b
    }

    /// Top up the buffer so it holds at least 9 bits.
    pub(crate) fn refill(&mut self) {
        while self.count <= 8 {
            let b = self.byte() as u16;
            self.buf |= b << (8 - self.count);
            self.count += 8;
        }
    }

    /// The buffered bits, most significant first (call `refill` first).
    pub(crate) fn peek(&self) -> u16 {
        self.buf
    }

    pub(crate) fn skip(&mut self, n: u32) {
        self.buf <<= n;
        self.count -= n;
    }

    /// Decode one symbol.
    pub(crate) fn symbol(&mut self, tree: &Tree) -> Option<u16> {
        self.refill();
        let (mut node, used) = tree.lookup[(self.buf >> (16 - LOOKUP_BITS)) as usize];
        self.skip(used as u32);
        while node as usize >= SYMBOLS {
            if self.count == 0 {
                self.buf = (self.byte() as u16) << 8;
                self.count = 7;
            } else {
                self.count -= 1;
            }
            let bit = self.buf & 0x8000;
            node = *tree.child.get(node as usize)?.get((bit == 0) as usize)?;
            self.buf <<= 1;
        }
        Some(node)
    }

    /// Bytes consumed, not counting whole bytes still buffered.
    pub(crate) fn consumed(&self) -> usize {
        self.pos - (self.count as usize >> 3)
    }

    pub(crate) fn overran(&self) -> bool {
        self.overrun > 0 && self.consumed() > self.data.len()
    }
}
