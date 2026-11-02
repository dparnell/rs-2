//! The game: piles, the rules, moves with Take back and Replay, the
//! score, Auto finish and Auto play. No PM here (the self-test drives it).
//!
//! Cards are 0..52: suit `c / 13` (clubs, diamonds, hearts, spades),
//! rank `c % 13` (0 is the ace). Piles: 0 the talon (face down), 1 the
//! waste pile, 2..6 the foundations, 6..13 the tableau.
//!
//! The score is OS/2's (studied in its binary): a deal costs 52, each
//! card put on a foundation earns 5 (taken off again, it costs 5), each
//! turn of the talon after the first pass costs 25; Score > Reset starts
//! again at 500.

use alloc::vec::Vec;

pub const TALON: usize = 0;
pub const WASTE: usize = 1;
pub const FOUNDATION: usize = 2;
pub const TABLEAU: usize = 6;
pub const PILES: usize = 13;

pub const DEAL_COST: i32 = 52;
pub const FOUNDATION_CARD: i32 = 5;
pub const TURN_COST: i32 = 25;
pub const START_SCORE: i32 = 500;

/// Unlimited turns of the talon.
pub const MANY: u32 = 32000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub id: u8,
    pub up: bool,
}

pub fn suit(c: u8) -> u8 {
    c / 13
}

pub fn rank(c: u8) -> u8 {
    c % 13
}

pub fn red(c: u8) -> bool {
    matches!(suit(c), 1 | 2)
}

pub fn is_foundation(p: usize) -> bool {
    (FOUNDATION..TABLEAU).contains(&p)
}

pub fn is_tableau(p: usize) -> bool {
    (TABLEAU..PILES).contains(&p)
}

/// One step of the game, kept so that it can be taken back and replayed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    /// n cards from the talon to the waste pile (turned up).
    Draw { n: u8 },
    /// The waste pile turned over onto the talon (n cards).
    Turn { n: u8, paid: bool },
    /// n cards from one pile to another; `flipped`: the card left on top
    /// of `from` was turned up afterwards (auto-flip).
    Transfer { from: u8, to: u8, n: u8, flipped: bool, cheat: bool },
    /// The face-down top card of a tableau pile turned up by hand.
    Flip { pile: u8 },
}

#[derive(Clone)]
pub struct Game {
    pub piles: [Vec<Card>; PILES],
    /// Cards turned from the talon at a time (1 or 3).
    pub draw: u32,
    /// Turns of the talon allowed after the first pass (0, 3 or MANY).
    pub turns_allowed: u32,
    /// Turns made so far.
    pub turns: u32,
    pub auto_flip: bool,
    pub score: i32,
    pub cheats: u32,
    /// The moves made; `done` of them are in effect (the rest can be
    /// replayed).
    pub history: Vec<Move>,
    pub done: usize,
}

impl Game {
    pub fn new() -> Game {
        Game {
            piles: Default::default(),
            draw: 3,
            turns_allowed: MANY,
            turns: 0,
            auto_flip: true,
            score: START_SCORE,
            cheats: 0,
            history: Vec::new(),
            done: 0,
        }
    }

    /// Deal a shuffled deck (`order`: the 52 cards from the top of the
    /// deck): one card to the first tableau pile, two to the second...,
    /// the top card of each turned up, the rest on the talon.
    pub fn deal(&mut self, order: &[u8]) {
        for p in self.piles.iter_mut() {
            p.clear();
        }
        let mut it = order.iter().copied();
        for row in 0..7 {
            for col in row..7 {
                let id = it.next().unwrap_or(0);
                self.piles[TABLEAU + col].push(Card { id, up: col == row });
            }
        }
        for id in it {
            self.piles[TALON].push(Card { id, up: false });
        }
        self.turns = 0;
        self.cheats = 0;
        self.history.clear();
        self.done = 0;
        self.score -= DEAL_COST;
    }

    pub fn top(&self, p: usize) -> Option<Card> {
        self.piles[p].last().copied()
    }

    /// Cards on the foundations.
    pub fn on_foundations(&self) -> usize {
        (FOUNDATION..TABLEAU).map(|p| self.piles[p].len()).sum()
    }

    pub fn won(&self) -> bool {
        self.on_foundations() == 52
    }

    /// The talon can be turned over (the waste pile back onto it).
    pub fn can_turn(&self) -> bool {
        self.piles[TALON].is_empty() && !self.piles[WASTE].is_empty() && (self.turns_allowed >= MANY || self.turns < self.turns_allowed)
    }

    /// How many cards from the top of pile `from` may be picked up (the
    /// face-up run on the tableau, one card elsewhere); `n` must not be
    /// larger.
    pub fn movable(&self, from: usize) -> usize {
        let p = &self.piles[from];
        if from == TALON || p.is_empty() {
            return 0;
        }
        if is_tableau(from) {
            p.iter().rev().take_while(|c| c.up).count()
        } else {
            1
        }
    }

    /// May the `n` top cards of `from` go onto `to` by the rules?
    pub fn legal(&self, from: usize, to: usize, n: usize) -> bool {
        if from == to || n == 0 || n > self.movable(from) || to == TALON || to == WASTE {
            return false;
        }
        let card = self.piles[from][self.piles[from].len() - n].id;
        let dst = self.top(to);
        if is_foundation(to) {
            return n == 1
                && match dst {
                    None => rank(card) == 0,
                    Some(d) => suit(d.id) == suit(card) && rank(d.id) + 1 == rank(card),
                };
        }
        match dst {
            None => rank(card) == 12,
            Some(d) => d.up && red(d.id) != red(card) && rank(d.id) == rank(card) + 1,
        }
    }

    /// Make a move (it is recorded; moves that were taken back are
    /// dropped). Returns false when it cannot be made.
    pub fn play(&mut self, m: Move) -> bool {
        let m = match m {
            Move::Draw { .. } => {
                if self.piles[TALON].is_empty() {
                    return false;
                }
                Move::Draw { n: self.draw.min(self.piles[TALON].len() as u32) as u8 }
            }
            Move::Turn { .. } => {
                if !self.can_turn() {
                    return false;
                }
                Move::Turn { n: self.piles[WASTE].len() as u8, paid: true }
            }
            Move::Transfer { from, to, n, cheat, .. } => {
                let (f, t, k) = (from as usize, to as usize, n as usize);
                if !(cheat && k > 0 && k <= self.movable(f) && f != t && t != TALON && t != WASTE) && !self.legal(f, t, k) {
                    return false;
                }
                let rest = self.piles[f].len() - k;
                let flipped = self.auto_flip && is_tableau(f) && rest > 0 && !self.piles[f][rest - 1].up;
                Move::Transfer { from, to, n, flipped, cheat }
            }
            Move::Flip { pile } => {
                let p = pile as usize;
                if !is_tableau(p) || self.top(p).is_none_or(|c| c.up) {
                    return false;
                }
                m
            }
        };
        self.history.truncate(self.done);
        self.history.push(m);
        self.apply(m, true);
        self.done += 1;
        true
    }

    /// Do (`fwd`) or undo a move.
    fn apply(&mut self, m: Move, fwd: bool) {
        match m {
            Move::Draw { n } => {
                for _ in 0..n {
                    let (a, b) = if fwd { (TALON, WASTE) } else { (WASTE, TALON) };
                    if let Some(mut c) = self.piles[a].pop() {
                        c.up = fwd;
                        self.piles[b].push(c);
                    }
                }
            }
            Move::Turn { n, .. } => {
                for _ in 0..n {
                    let (a, b) = if fwd { (WASTE, TALON) } else { (TALON, WASTE) };
                    if let Some(mut c) = self.piles[a].pop() {
                        c.up = !fwd;
                        self.piles[b].push(c);
                    }
                }
                if fwd {
                    self.turns += 1;
                    self.score -= TURN_COST;
                } else {
                    self.turns -= 1;
                    self.score += TURN_COST;
                }
            }
            Move::Transfer { from, to, n, flipped, cheat } => {
                let (f, t) = if fwd { (from as usize, to as usize) } else { (to as usize, from as usize) };
                if !fwd && flipped {
                    if let Some(c) = self.piles[from as usize].last_mut() {
                        c.up = false;
                    }
                }
                let at = self.piles[f].len() - n as usize;
                let moved: Vec<Card> = self.piles[f].drain(at..).collect();
                self.piles[t].extend(moved);
                if fwd && flipped {
                    if let Some(c) = self.piles[from as usize].last_mut() {
                        c.up = true;
                    }
                }
                let delta = n as i32 * FOUNDATION_CARD;
                let sign = if fwd { 1 } else { -1 };
                if is_foundation(to as usize) {
                    self.score += sign * delta;
                }
                if is_foundation(from as usize) {
                    self.score -= sign * delta;
                }
                if cheat {
                    if fwd {
                        self.cheats += 1;
                    }
                }
            }
            Move::Flip { pile } => {
                if let Some(c) = self.piles[pile as usize].last_mut() {
                    c.up = fwd;
                }
            }
        }
    }

    pub fn can_take_back(&self) -> bool {
        self.done > 0
    }

    pub fn can_replay(&self) -> bool {
        self.done < self.history.len()
    }

    pub fn take_back(&mut self) -> Option<Move> {
        if self.done == 0 {
            return None;
        }
        self.done -= 1;
        let m = self.history[self.done];
        self.apply(m, false);
        Some(m)
    }

    pub fn replay(&mut self) -> Option<Move> {
        if self.done >= self.history.len() {
            return None;
        }
        let m = self.history[self.done];
        self.apply(m, true);
        self.done += 1;
        Some(m)
    }

    /// A foundation that takes this pile's top card.
    pub fn foundation_for(&self, from: usize) -> Option<usize> {
        if self.movable(from) == 0 || is_foundation(from) {
            return None;
        }
        (FOUNDATION..TABLEAU).find(|&f| self.legal(from, f, 1))
    }

    /// The next move of Auto finish: a card onto a foundation (the waste
    /// pile's top, then the tableau's), else None.
    pub fn finish_step(&self) -> Option<Move> {
        for from in core::iter::once(WASTE).chain(TABLEAU..PILES) {
            if let Some(to) = self.foundation_for(from) {
                return Some(Move::Transfer { from: from as u8, to: to as u8, n: 1, flipped: false, cheat: false });
            }
        }
        // Without auto-flip, face-down tops are turned up too.
        (TABLEAU..PILES).find(|&p| self.top(p).is_some_and(|c| !c.up)).map(|p| Move::Flip { pile: p as u8 })
    }

    /// The next move of Auto play, or None when it is stuck. `idle`
    /// counts the talon cards drawn since the last useful move; once a
    /// whole pass brought nothing the game is stuck.
    pub fn auto_step(&self, idle: usize) -> Option<Move> {
        if let Some(m) = self.finish_step() {
            return Some(m);
        }
        // A run whose base uncovers a face-down card or empties a pile
        // (not a king already at the bottom), onto another tableau pile.
        for from in TABLEAU..PILES {
            let n = self.movable(from);
            if n == 0 {
                continue;
            }
            let len = self.piles[from].len();
            let uncovers = len > n;
            let king_at_bottom = len == n && rank(self.piles[from][0].id) == 12;
            if king_at_bottom {
                continue;
            }
            for to in TABLEAU..PILES {
                if self.legal(from, to, n) && (uncovers || !self.piles[to].is_empty()) {
                    return Some(Move::Transfer { from: from as u8, to: to as u8, n: n as u8, flipped: false, cheat: false });
                }
            }
        }
        // The waste pile's top onto the tableau.
        for to in TABLEAU..PILES {
            if self.legal(WASTE, to, 1) {
                return Some(Move::Transfer { from: WASTE as u8, to: to as u8, n: 1, flipped: false, cheat: false });
            }
        }
        let cycle = self.piles[TALON].len() + self.piles[WASTE].len();
        if idle > cycle + self.draw as usize {
            return None;
        }
        if !self.piles[TALON].is_empty() {
            return Some(Move::Draw { n: 0 });
        }
        if self.can_turn() {
            return Some(Move::Turn { n: 0, paid: true });
        }
        None
    }

    /// Whether a move is a "useful" one for Auto play's stuck test.
    pub fn progress(m: &Move) -> bool {
        !matches!(m, Move::Draw { .. } | Move::Turn { .. })
    }
}

/// A shuffled deck.
pub fn shuffled(rng: &mut pmkit::Rng) -> Vec<u8> {
    let mut d: Vec<u8> = (0..52).collect();
    rng.shuffle(&mut d);
    d
}
