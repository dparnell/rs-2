//! The game: which tile lies where, the rules for free tiles and
//! matches, dealing (at random, or so that the game can be solved),
//! the moves made and taking them back.

use alloc::vec::Vec;

use crate::layout::{Layout, Pos, TILES};

/// Tile faces: 0-8 circles 1-9, 9-17 bamboo 1-9, 18-26 characters 1-9,
/// 27-30 the winds (east, south, west, north), 31-33 the dragons (red,
/// green, white), four each; 34-37 the flowers and 38-41 the seasons,
/// one each.
pub const FLOWER: u8 = 34;
pub const SEASON: u8 = 38;

/// The 144 faces of a set.
pub fn tile_set() -> Vec<u8> {
    let mut v = Vec::with_capacity(TILES);
    for f in 0..34u8 {
        for _ in 0..4 {
            v.push(f);
        }
    }
    for f in 34..42u8 {
        v.push(f);
    }
    v
}

/// Two faces match: the same, or two flowers, or two seasons.
pub fn matches(a: u8, b: u8) -> bool {
    a == b || (a >= FLOWER && a < SEASON && b >= FLOWER && b < SEASON) || (a >= SEASON && b >= SEASON)
}

/// The kind a face belongs to for matching (flowers and seasons each
/// count as one).
pub fn kind(f: u8) -> u8 {
    if f >= SEASON {
        SEASON
    } else if f >= FLOWER {
        FLOWER
    } else {
        f
    }
}

#[derive(Clone, Copy)]
pub struct Move {
    pub a: usize,
    pub b: usize,
    pub cheat: bool,
}

pub struct Game {
    pub pos: Vec<Pos>,
    pub face: Vec<u8>,
    pub present: Vec<bool>,
    pub moves: Vec<Move>,
    /// The game number the deal was made from.
    pub number: u32,
    /// For a solvable deal: an order in which the pairs can be taken.
    pub solution: Vec<(usize, usize)>,
}

/// xorshift, seeded by the game number, so a number always gives the
/// same deal.
struct Rand(u32);

impl Rand {
    fn new(seed: u32) -> Rand {
        let mut r = Rand(seed.wrapping_mul(2654435761) ^ 0x5bd1e995 | 1);
        for _ in 0..8 {
            r.next();
        }
        r
    }
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u32) as usize }
    }
}

/// A tile among `pos` (those `present`) is free: nothing lies on it and
/// its left or its right side is open.
pub fn free_in(pos: &[Pos], present: &[bool], i: usize) -> bool {
    if !present[i] {
        return false;
    }
    let p = pos[i];
    let (mut left, mut right) = (false, false);
    for (j, &q) in pos.iter().enumerate() {
        if j == i || !present[j] {
            continue;
        }
        if q.z == p.z + 1 && (q.x - p.x).abs() < 2 && (q.y - p.y).abs() < 2 {
            return false;
        }
        if q.z == p.z && (q.y - p.y).abs() < 2 {
            if q.x - p.x == -2 {
                left = true;
            } else if q.x - p.x == 2 {
                right = true;
            }
        }
    }
    !(left && right)
}

impl Game {
    /// Deal game `number` on the layout; `solvable` deals so that the
    /// game can be played to the end.
    pub fn deal(l: &Layout, number: u32, solvable: bool) -> Game {
        let n = l.pos.len();
        let mut g = Game { pos: l.pos.clone(), face: alloc::vec![0; n], present: alloc::vec![true; n], moves: Vec::new(), number, solution: Vec::new() };
        let mut set = tile_set();
        set.truncate(n);
        let mut r = Rand::new(number);
        if solvable && n % 2 == 0 && g.deal_solvable(&mut r, &set) {
            return g;
        }
        let mut r = Rand::new(number);
        for i in (1..set.len()).rev() {
            let j = r.below(i + 1);
            set.swap(i, j);
        }
        g.face = set;
        g
    }

    /// Take the tiles off the full board two free ones at a time and give
    /// each pair matching faces: played in that order the game comes
    /// out, so it can be solved. False if no such order was found.
    fn deal_solvable(&mut self, r: &mut Rand, set: &[u8]) -> bool {
        let n = self.pos.len();
        // The faces in pairs, shuffled.
        let mut pairs: Vec<(u8, u8)> = Vec::new();
        let mut kinds: Vec<u8> = set.to_vec();
        kinds.sort();
        let mut i = 0;
        while i + 1 < kinds.len() {
            pairs.push((kinds[i], kinds[i + 1]));
            i += 2;
        }
        for i in (1..pairs.len()).rev() {
            let j = r.below(i + 1);
            pairs.swap(i, j);
        }
        for _attempt in 0..200 {
            let mut present = alloc::vec![true; n];
            let mut face = alloc::vec![0u8; n];
            let mut ok = true;
            let mut order = Vec::new();
            for &(fa, fb) in &pairs {
                let free: Vec<usize> = (0..n).filter(|&i| free_in(&self.pos, &present, i)).collect();
                if free.len() < 2 {
                    ok = false;
                    break;
                }
                // Both must be free while the other is still there, as
                // when they are played.
                let ia = r.below(free.len());
                let mut ib = r.below(free.len() - 1);
                if ib >= ia {
                    ib += 1;
                }
                let (a, b) = (free[ia], free[ib]);
                present[a] = false;
                present[b] = false;
                face[a] = fa;
                face[b] = fb;
                order.push((a, b));
            }
            if ok {
                self.face = face;
                self.solution = order;
                return true;
            }
        }
        false
    }

    pub fn free(&self, i: usize) -> bool {
        free_in(&self.pos, &self.present, i)
    }

    pub fn left(&self) -> usize {
        self.present.iter().filter(|&&p| p).count()
    }

    /// The pairs of free tiles that match.
    pub fn free_pairs(&self) -> Vec<(usize, usize)> {
        let free: Vec<usize> = (0..self.pos.len()).filter(|&i| self.free(i)).collect();
        let mut out = Vec::new();
        for (k, &a) in free.iter().enumerate() {
            for &b in &free[k + 1..] {
                if matches(self.face[a], self.face[b]) {
                    out.push((a, b));
                }
            }
        }
        out
    }

    /// How many pairs can be taken now (OS/2's "matches").
    pub fn match_count(&self) -> usize {
        self.free_pairs().len()
    }

    /// Take two tiles off.
    pub fn remove(&mut self, a: usize, b: usize, cheat: bool) {
        self.present[a] = false;
        self.present[b] = false;
        self.moves.push(Move { a, b, cheat });
    }

    /// Take the last move back.
    pub fn take_back(&mut self) -> Option<Move> {
        let m = self.moves.pop()?;
        self.present[m.a] = true;
        self.present[m.b] = true;
        Some(m)
    }

    /// Start the same deal again.
    pub fn restart(&mut self) {
        for p in self.present.iter_mut() {
            *p = true;
        }
        self.moves.clear();
    }

    pub fn cheats(&self) -> usize {
        self.moves.iter().filter(|m| m.cheat).count()
    }

    pub fn won(&self) -> bool {
        self.left() == 0
    }

    /// Whether the game can still be played to the end from here (a
    /// bounded search, for the self-checks).
    pub fn solvable_from_here(&self, budget: &mut u32) -> bool {
        if self.won() {
            return true;
        }
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let mut g = Game { pos: self.pos.clone(), face: self.face.clone(), present: self.present.clone(), moves: Vec::new(), number: 0, solution: Vec::new() };
        for (a, b) in self.free_pairs() {
            g.present[a] = false;
            g.present[b] = false;
            if g.solvable_from_here(budget) {
                return true;
            }
            g.present[a] = true;
            g.present[b] = true;
        }
        false
    }
}
