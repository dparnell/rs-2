//! The chess rules and the computer player: positions, legal moves
//! (castling, en passant, promotion), check, mate, stalemate and the
//! draw rules (50 moves, threefold repetition, too few pieces), move
//! notation, and an alpha-beta search with iterative deepening and a
//! time limit. No PM in here: `/SELFTEST` checks it (perft counts).

use alloc::vec::Vec;

pub const PAWN: i8 = 1;
pub const KNIGHT: i8 = 2;
pub const BISHOP: i8 = 3;
pub const ROOK: i8 = 4;
pub const QUEEN: i8 = 5;
pub const KING: i8 = 6;

pub const WHITE: i8 = 1;
pub const BLACK: i8 = -1;

/// Castling rights.
const WK: u8 = 1;
const WQ: u8 = 2;
const BK: u8 = 4;
const BQ: u8 = 8;

/// A move: from, to, the piece promoted to (0: none) and flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub promo: i8,
    pub flags: u8,
}

pub const F_CAPTURE: u8 = 1;
pub const F_EP: u8 = 2;
pub const F_CASTLE: u8 = 4;
pub const F_DOUBLE: u8 = 8;

impl Move {
    pub const NONE: Move = Move { from: 0, to: 0, promo: 0, flags: 0 };
    pub fn is_none(&self) -> bool {
        self.from == self.to
    }
}

/// What a move changed, to take it back.
#[derive(Clone, Copy)]
struct Undo {
    mv: Move,
    captured: i8,
    castling: u8,
    ep: u8,
    halfmove: u16,
    hash: u64,
}

#[derive(Clone)]
pub struct Position {
    /// a1 = 0, b1 = 1 ... h8 = 63; white pieces > 0, black < 0.
    pub sq: [i8; 64],
    pub side: i8,
    pub castling: u8,
    /// The en passant target square, or 64.
    pub ep: u8,
    pub halfmove: u16,
    pub fullmove: u16,
    pub hash: u64,
    undo: Vec<Undo>,
    /// Hashes of the earlier positions (repetitions).
    history: Vec<u64>,
}

pub fn file(s: u8) -> i32 {
    (s & 7) as i32
}

pub fn rank(s: u8) -> i32 {
    (s >> 3) as i32
}

fn at(f: i32, r: i32) -> Option<u8> {
    ((0..8).contains(&f) && (0..8).contains(&r)).then_some((r * 8 + f) as u8)
}

// ---- Zobrist keys ----

struct Keys {
    piece: [[u64; 64]; 13],
    side: u64,
    castle: [u64; 16],
    ep: [u64; 8],
}

static mut KEYS: Option<Keys> = None;

fn keys() -> &'static Keys {
    unsafe {
        let k = &mut *core::ptr::addr_of_mut!(KEYS);
        if k.is_none() {
            let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
            let mut next = || {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s
            };
            let mut piece = [[0u64; 64]; 13];
            for p in piece.iter_mut() {
                for v in p.iter_mut() {
                    *v = next();
                }
            }
            let side = next();
            let mut castle = [0u64; 16];
            for v in castle.iter_mut() {
                *v = next();
            }
            let mut ep = [0u64; 8];
            for v in ep.iter_mut() {
                *v = next();
            }
            *k = Some(Keys { piece, side, castle, ep });
        }
        k.as_ref().unwrap()
    }
}

fn pkey(p: i8, s: u8) -> u64 {
    keys().piece[(p + 6) as usize][s as usize]
}

const KNIGHT_D: [(i32, i32); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_D: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const DIAG: [(i32, i32); 4] = [(1, 1), (-1, 1), (-1, -1), (1, -1)];
const ORTH: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

pub const START_FEN: &[u8] = b"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

impl Position {
    pub fn start() -> Position {
        Position::from_fen(START_FEN).unwrap()
    }

    pub fn empty() -> Position {
        Position { sq: [0; 64], side: WHITE, castling: 0, ep: 64, halfmove: 0, fullmove: 1, hash: 0, undo: Vec::new(), history: Vec::new() }
    }

    /// A position in Forsyth-Edwards notation.
    pub fn from_fen(f: &[u8]) -> Option<Position> {
        let mut p = Position::empty();
        let mut parts = f.split(|&c| c == b' ').filter(|s| !s.is_empty());
        let board = parts.next()?;
        let (mut fl, mut r) = (0i32, 7i32);
        for &c in board {
            match c {
                b'/' => {
                    r -= 1;
                    fl = 0;
                }
                b'1'..=b'8' => fl += (c - b'0') as i32,
                _ => {
                    let kind = match c.to_ascii_lowercase() {
                        b'p' => PAWN,
                        b'n' => KNIGHT,
                        b'b' => BISHOP,
                        b'r' => ROOK,
                        b'q' => QUEEN,
                        b'k' => KING,
                        _ => return None,
                    };
                    let s = at(fl, r)?;
                    p.sq[s as usize] = if c.is_ascii_uppercase() { kind } else { -kind };
                    fl += 1;
                }
            }
        }
        p.side = if parts.next() == Some(b"b") { BLACK } else { WHITE };
        if let Some(c) = parts.next() {
            for &ch in c {
                p.castling |= match ch {
                    b'K' => WK,
                    b'Q' => WQ,
                    b'k' => BK,
                    b'q' => BQ,
                    _ => 0,
                };
            }
        }
        if let Some(e) = parts.next()
            && e.len() == 2
        {
            p.ep = at((e[0] as i32) - b'a' as i32, (e[1] as i32) - b'1' as i32).unwrap_or(64);
        }
        if let Some(h) = parts.next() {
            p.halfmove = h.iter().fold(0u16, |a, &c| a * 10 + (c.wrapping_sub(b'0') as u16 % 10));
        }
        if let Some(n) = parts.next() {
            p.fullmove = n.iter().fold(0u16, |a, &c| a * 10 + (c.wrapping_sub(b'0') as u16 % 10)).max(1);
        }
        p.hash = p.compute_hash();
        Some(p)
    }

    /// The position in Forsyth-Edwards notation.
    pub fn fen(&self) -> Vec<u8> {
        let mut s = Vec::new();
        for r in (0..8).rev() {
            let mut empty = 0;
            for f in 0..8 {
                let pc = self.sq[r * 8 + f];
                if pc == 0 {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    s.push(b'0' + empty);
                    empty = 0;
                }
                let c = b" pnbrqk"[pc.unsigned_abs() as usize];
                s.push(if pc > 0 { c.to_ascii_uppercase() } else { c });
            }
            if empty > 0 {
                s.push(b'0' + empty);
            }
            if r > 0 {
                s.push(b'/');
            }
        }
        s.extend_from_slice(if self.side == WHITE { b" w " } else { b" b " });
        if self.castling == 0 {
            s.push(b'-');
        }
        for (bit, c) in [(WK, b'K'), (WQ, b'Q'), (BK, b'k'), (BQ, b'q')] {
            if self.castling & bit != 0 {
                s.push(c);
            }
        }
        s.push(b' ');
        if self.ep < 64 {
            s.extend_from_slice(&square_name(self.ep));
        } else {
            s.push(b'-');
        }
        s.extend_from_slice(alloc::format!(" {} {}", self.halfmove, self.fullmove).as_bytes());
        s
    }

    /// The castling rights the pieces allow (kings and rooks at home).
    pub fn infer_castling(&mut self) {
        let mut c = 0;
        if self.sq[4] == KING {
            if self.sq[7] == ROOK {
                c |= WK;
            }
            if self.sq[0] == ROOK {
                c |= WQ;
            }
        }
        if self.sq[60] == -KING {
            if self.sq[63] == -ROOK {
                c |= BK;
            }
            if self.sq[56] == -ROOK {
                c |= BQ;
            }
        }
        self.castling = c;
    }

    fn compute_hash(&self) -> u64 {
        let mut h = 0;
        for s in 0..64u8 {
            let pc = self.sq[s as usize];
            if pc != 0 {
                h ^= pkey(pc, s);
            }
        }
        if self.side == BLACK {
            h ^= keys().side;
        }
        h ^= keys().castle[self.castling as usize];
        if self.ep < 64 {
            h ^= keys().ep[file(self.ep) as usize];
        }
        h
    }

    pub fn king(&self, side: i8) -> u8 {
        (0..64u8).find(|&s| self.sq[s as usize] == KING * side).unwrap_or(64)
    }

    /// Is `s` attacked by a piece of `by`?
    pub fn attacked(&self, s: u8, by: i8) -> bool {
        let (f, r) = (file(s), rank(s));
        // Pawns attack from behind (from the attacker's point of view).
        let pr = r - by as i32;
        for df in [-1, 1] {
            if let Some(t) = at(f + df, pr)
                && self.sq[t as usize] == PAWN * by
            {
                return true;
            }
        }
        for (df, dr) in KNIGHT_D {
            if let Some(t) = at(f + df, r + dr)
                && self.sq[t as usize] == KNIGHT * by
            {
                return true;
            }
        }
        for (df, dr) in KING_D {
            if let Some(t) = at(f + df, r + dr)
                && self.sq[t as usize] == KING * by
            {
                return true;
            }
        }
        for (dirs, a, b) in [(&DIAG, BISHOP, QUEEN), (&ORTH, ROOK, QUEEN)] {
            for &(df, dr) in dirs.iter() {
                let (mut x, mut y) = (f + df, r + dr);
                while let Some(t) = at(x, y) {
                    let pc = self.sq[t as usize];
                    if pc != 0 {
                        if pc == a * by || pc == b * by {
                            return true;
                        }
                        break;
                    }
                    x += df;
                    y += dr;
                }
            }
        }
        false
    }

    pub fn in_check(&self, side: i8) -> bool {
        let k = self.king(side);
        k < 64 && self.attacked(k, -side)
    }

    fn add_pawn_moves(&self, from: u8, to: u8, flags: u8, out: &mut Vec<Move>) {
        if rank(to) == 0 || rank(to) == 7 {
            for p in [QUEEN, ROOK, BISHOP, KNIGHT] {
                out.push(Move { from, to, promo: p * self.side, flags });
            }
        } else {
            out.push(Move { from, to, promo: 0, flags });
        }
    }

    /// The moves of the side to move, without checking that its king is
    /// left safe; `captures`: only captures and promotions.
    pub fn pseudo_moves(&self, captures: bool, out: &mut Vec<Move>) {
        let me = self.side;
        for s in 0..64u8 {
            let pc = self.sq[s as usize];
            if pc == 0 || (pc > 0) != (me > 0) {
                continue;
            }
            let (f, r) = (file(s), rank(s));
            match pc.abs() {
                PAWN => {
                    let dir = me as i32;
                    if let Some(t) = at(f, r + dir)
                        && self.sq[t as usize] == 0
                    {
                        if !captures || rank(t) == 0 || rank(t) == 7 {
                            self.add_pawn_moves(s, t, 0, out);
                        }
                        let home = if me > 0 { 1 } else { 6 };
                        if !captures
                            && r == home
                            && let Some(t2) = at(f, r + 2 * dir)
                            && self.sq[t2 as usize] == 0
                        {
                            out.push(Move { from: s, to: t2, promo: 0, flags: F_DOUBLE });
                        }
                    }
                    for df in [-1, 1] {
                        if let Some(t) = at(f + df, r + dir) {
                            let v = self.sq[t as usize];
                            if v != 0 && (v > 0) != (me > 0) {
                                self.add_pawn_moves(s, t, F_CAPTURE, out);
                            } else if t == self.ep {
                                out.push(Move { from: s, to: t, promo: 0, flags: F_CAPTURE | F_EP });
                            }
                        }
                    }
                }
                KNIGHT | KING => {
                    let d = if pc.abs() == KNIGHT { &KNIGHT_D } else { &KING_D };
                    for &(df, dr) in d.iter() {
                        if let Some(t) = at(f + df, r + dr) {
                            let v = self.sq[t as usize];
                            if v == 0 {
                                if !captures {
                                    out.push(Move { from: s, to: t, promo: 0, flags: 0 });
                                }
                            } else if (v > 0) != (me > 0) {
                                out.push(Move { from: s, to: t, promo: 0, flags: F_CAPTURE });
                            }
                        }
                    }
                    if pc.abs() == KING && !captures {
                        self.castle_moves(s, out);
                    }
                }
                kind => {
                    let mut dirs: Vec<(i32, i32)> = Vec::new();
                    if kind == BISHOP || kind == QUEEN {
                        dirs.extend_from_slice(&DIAG);
                    }
                    if kind == ROOK || kind == QUEEN {
                        dirs.extend_from_slice(&ORTH);
                    }
                    for (df, dr) in dirs {
                        let (mut x, mut y) = (f + df, r + dr);
                        while let Some(t) = at(x, y) {
                            let v = self.sq[t as usize];
                            if v == 0 {
                                if !captures {
                                    out.push(Move { from: s, to: t, promo: 0, flags: 0 });
                                }
                            } else {
                                if (v > 0) != (me > 0) {
                                    out.push(Move { from: s, to: t, promo: 0, flags: F_CAPTURE });
                                }
                                break;
                            }
                            x += df;
                            y += dr;
                        }
                    }
                }
            }
        }
    }

    fn castle_moves(&self, k: u8, out: &mut Vec<Move>) {
        let me = self.side;
        let (home, kside, qside) = if me > 0 { (4u8, WK, WQ) } else { (60u8, BK, BQ) };
        if k != home || self.attacked(k, -me) {
            return;
        }
        if self.castling & kside != 0
            && self.sq[(home + 1) as usize] == 0
            && self.sq[(home + 2) as usize] == 0
            && self.sq[(home + 3) as usize] == ROOK * me
            && !self.attacked(home + 1, -me)
            && !self.attacked(home + 2, -me)
        {
            out.push(Move { from: k, to: home + 2, promo: 0, flags: F_CASTLE });
        }
        if self.castling & qside != 0
            && self.sq[(home - 1) as usize] == 0
            && self.sq[(home - 2) as usize] == 0
            && self.sq[(home - 3) as usize] == 0
            && self.sq[(home - 4) as usize] == ROOK * me
            && !self.attacked(home - 1, -me)
            && !self.attacked(home - 2, -me)
        {
            out.push(Move { from: k, to: home - 2, promo: 0, flags: F_CASTLE });
        }
    }

    /// Make a move (pseudo-legal); returns false (and takes it back)
    /// when it leaves the mover's king in check.
    pub fn make(&mut self, m: Move) -> bool {
        let me = self.side;
        let k = keys();
        let pc = self.sq[m.from as usize];
        let mut captured = self.sq[m.to as usize];
        self.undo.push(Undo { mv: m, captured, castling: self.castling, ep: self.ep, halfmove: self.halfmove, hash: self.hash });
        self.history.push(self.hash);
        let mut h = self.hash ^ k.castle[self.castling as usize];
        if self.ep < 64 {
            h ^= k.ep[file(self.ep) as usize];
        }
        if m.flags & F_EP != 0 {
            let cap = (m.to as i32 - 8 * me as i32) as u8;
            captured = self.sq[cap as usize];
            self.sq[cap as usize] = 0;
            h ^= pkey(captured, cap);
            if let Some(u) = self.undo.last_mut() {
                u.captured = captured;
            }
        } else if captured != 0 {
            h ^= pkey(captured, m.to);
        }
        h ^= pkey(pc, m.from);
        self.sq[m.from as usize] = 0;
        let placed = if m.promo != 0 { m.promo } else { pc };
        self.sq[m.to as usize] = placed;
        h ^= pkey(placed, m.to);
        if m.flags & F_CASTLE != 0 {
            let (rf, rt) = if m.to > m.from { (m.from + 3, m.from + 1) } else { (m.from - 4, m.from - 1) };
            let rook = self.sq[rf as usize];
            self.sq[rf as usize] = 0;
            self.sq[rt as usize] = rook;
            h ^= pkey(rook, rf) ^ pkey(rook, rt);
        }
        // Castling rights go with the king's or a rook's first move, or
        // a rook taken at home.
        for s in [m.from, m.to] {
            self.castling &= match s {
                4 => !(WK | WQ),
                0 => !WQ,
                7 => !WK,
                60 => !(BK | BQ),
                56 => !BQ,
                63 => !BK,
                _ => 0xff,
            };
        }
        self.ep = if m.flags & F_DOUBLE != 0 { ((m.from as i32 + m.to as i32) / 2) as u8 } else { 64 };
        if self.ep < 64 {
            h ^= k.ep[file(self.ep) as usize];
        }
        h ^= k.castle[self.castling as usize];
        self.halfmove = if pc.abs() == PAWN || captured != 0 { 0 } else { self.halfmove + 1 };
        if me == BLACK {
            self.fullmove += 1;
        }
        self.side = -me;
        h ^= k.side;
        self.hash = h;
        if self.in_check(me) {
            self.unmake();
            return false;
        }
        true
    }

    pub fn unmake(&mut self) {
        let Some(u) = self.undo.pop() else { return };
        self.history.pop();
        let m = u.mv;
        self.side = -self.side;
        let me = self.side;
        if me == BLACK {
            self.fullmove -= 1;
        }
        let placed = self.sq[m.to as usize];
        let pc = if m.promo != 0 { PAWN * me } else { placed };
        self.sq[m.from as usize] = pc;
        if m.flags & F_EP != 0 {
            self.sq[m.to as usize] = 0;
            self.sq[(m.to as i32 - 8 * me as i32) as usize] = u.captured;
        } else {
            self.sq[m.to as usize] = u.captured;
        }
        if m.flags & F_CASTLE != 0 {
            let (rf, rt) = if m.to > m.from { (m.from + 3, m.from + 1) } else { (m.from - 4, m.from - 1) };
            self.sq[rf as usize] = self.sq[rt as usize];
            self.sq[rt as usize] = 0;
        }
        self.castling = u.castling;
        self.ep = u.ep;
        self.halfmove = u.halfmove;
        self.hash = u.hash;
    }

    /// The legal moves.
    pub fn legal_moves(&mut self) -> Vec<Move> {
        let mut v = Vec::with_capacity(48);
        self.pseudo_moves(false, &mut v);
        v.retain(|&m| {
            let ok = self.make(m);
            if ok {
                self.unmake();
            }
            ok
        });
        v
    }

    /// The legal move from `from` to `to` (promoting to `promo` kind if
    /// it is a promotion).
    pub fn find_move(&mut self, from: u8, to: u8, promo: i8) -> Option<Move> {
        let want = if promo == 0 { QUEEN } else { promo.abs() };
        self.legal_moves().into_iter().find(|m| m.from == from && m.to == to && (m.promo == 0 || m.promo.abs() == want))
    }

    /// How often the current position occurred before.
    pub fn repetitions(&self) -> usize {
        let n = self.halfmove as usize;
        self.history.iter().rev().take(n).filter(|&&h| h == self.hash).count()
    }

    /// Not enough material for anyone to mate (kings, one minor piece,
    /// or bishops all on one colour).
    pub fn insufficient(&self) -> bool {
        let mut minors = 0;
        let mut bishop_colors = [false; 2];
        let mut knights = 0;
        for s in 0..64u8 {
            match self.sq[s as usize].abs() {
                0 | KING => {}
                BISHOP => {
                    minors += 1;
                    bishop_colors[((file(s) + rank(s)) & 1) as usize] = true;
                }
                KNIGHT => {
                    minors += 1;
                    knights += 1;
                }
                _ => return false,
            }
        }
        minors <= 1 || (knights == 0 && !(bishop_colors[0] && bishop_colors[1]))
    }

    pub fn status(&mut self) -> Status {
        let moves = self.legal_moves();
        if moves.is_empty() {
            return if self.in_check(self.side) { Status::Checkmate } else { Status::Stalemate };
        }
        if self.halfmove >= 100 {
            return Status::Fifty;
        }
        if self.repetitions() >= 2 {
            return Status::Repetition;
        }
        if self.insufficient() {
            return Status::Material;
        }
        if self.in_check(self.side) { Status::Check } else { Status::Normal }
    }

    pub fn perft(&mut self, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let mut v = Vec::with_capacity(48);
        self.pseudo_moves(false, &mut v);
        let mut n = 0;
        for m in v {
            if self.make(m) {
                n += if depth == 1 { 1 } else { self.perft(depth - 1) };
                self.unmake();
            }
        }
        n
    }

    /// The move in long algebraic notation: "e2-e4", "Ng1-f3", "e4xd5",
    /// "O-O", "e7-e8=Q", with "+" or "#" after a check or mate.
    pub fn notation(&mut self, m: Move) -> Vec<u8> {
        let mut s = Vec::new();
        let pc = self.sq[m.from as usize].abs();
        if m.flags & F_CASTLE != 0 {
            s.extend_from_slice(if m.to > m.from { b"O-O" } else { b"O-O-O" });
        } else {
            if pc != PAWN {
                s.push(PIECE_LETTERS[pc as usize]);
            }
            s.extend_from_slice(&square_name(m.from));
            s.push(if m.flags & F_CAPTURE != 0 { b'x' } else { b'-' });
            s.extend_from_slice(&square_name(m.to));
            if m.promo != 0 {
                s.push(b'=');
                s.push(PIECE_LETTERS[m.promo.unsigned_abs() as usize]);
            }
        }
        if self.make(m) {
            let st = self.status();
            if st == Status::Checkmate {
                s.push(b'#');
            } else if self.in_check(self.side) {
                s.push(b'+');
            }
            self.unmake();
        }
        s
    }
}

pub const PIECE_LETTERS: [u8; 7] = [b' ', b'P', b'N', b'B', b'R', b'Q', b'K'];

pub fn square_name(s: u8) -> [u8; 2] {
    [b'a' + file(s) as u8, b'1' + rank(s) as u8]
}

/// "e2" as a square.
pub fn parse_square(t: &[u8]) -> Option<u8> {
    if t.len() < 2 {
        return None;
    }
    at(t[0].to_ascii_lowercase() as i32 - b'a' as i32, t[1] as i32 - b'1' as i32)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Normal,
    Check,
    Checkmate,
    Stalemate,
    Fifty,
    Repetition,
    Material,
}

// ---- Evaluation ----

const VALUE: [i32; 7] = [0, 100, 320, 330, 500, 900, 0];

/// Bonuses by square for white (a1 first); black mirrors them.
const PAWN_T: [i8; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, //
    4, 6, 6, -14, -14, 6, 6, 4, //
    4, -2, -6, 2, 2, -6, -2, 4, //
    0, 0, 2, 18, 18, 2, 0, 0, //
    6, 6, 12, 22, 22, 12, 6, 6, //
    12, 12, 20, 28, 28, 20, 12, 12, //
    40, 40, 40, 40, 40, 40, 40, 40, //
    0, 0, 0, 0, 0, 0, 0, 0,
];
const KNIGHT_T: [i8; 64] = [
    -40, -30, -20, -20, -20, -20, -30, -40, //
    -30, -10, 0, 4, 4, 0, -10, -30, //
    -20, 4, 10, 14, 14, 10, 4, -20, //
    -20, 0, 14, 20, 20, 14, 0, -20, //
    -20, 4, 14, 20, 20, 14, 4, -20, //
    -20, 0, 10, 14, 14, 10, 0, -20, //
    -30, -10, 0, 0, 0, 0, -10, -30, //
    -40, -30, -20, -20, -20, -20, -30, -40,
];
const BISHOP_T: [i8; 64] = [
    -16, -8, -10, -8, -8, -10, -8, -16, //
    -8, 6, 0, 2, 2, 0, 6, -8, //
    -8, 8, 8, 8, 8, 8, 8, -8, //
    -8, 0, 10, 10, 10, 10, 0, -8, //
    -8, 4, 6, 10, 10, 6, 4, -8, //
    -8, 0, 6, 8, 8, 6, 0, -8, //
    -8, 0, 0, 0, 0, 0, 0, -8, //
    -16, -8, -8, -8, -8, -8, -8, -16,
];
const ROOK_T: [i8; 64] = [
    0, 0, 2, 6, 6, 2, 0, 0, //
    -4, 0, 0, 0, 0, 0, 0, -4, //
    -4, 0, 0, 0, 0, 0, 0, -4, //
    -4, 0, 0, 0, 0, 0, 0, -4, //
    -4, 0, 0, 0, 0, 0, 0, -4, //
    -4, 0, 0, 0, 0, 0, 0, -4, //
    6, 10, 10, 10, 10, 10, 10, 6, //
    0, 0, 0, 0, 0, 0, 0, 0,
];
const KING_T: [i8; 64] = [
    16, 24, 8, -4, 0, 8, 26, 16, //
    10, 10, -4, -8, -8, -4, 10, 10, //
    -10, -16, -18, -20, -20, -18, -16, -10, //
    -20, -26, -28, -34, -34, -28, -26, -20, //
    -28, -34, -36, -42, -42, -36, -34, -28, //
    -30, -36, -38, -44, -44, -38, -36, -30, //
    -30, -36, -38, -44, -44, -38, -36, -30, //
    -30, -36, -38, -44, -44, -38, -36, -30,
];
const KING_END_T: [i8; 64] = [
    -40, -24, -16, -10, -10, -16, -24, -40, //
    -24, -10, 0, 4, 4, 0, -10, -24, //
    -16, 0, 12, 18, 18, 12, 0, -16, //
    -10, 4, 18, 26, 26, 18, 4, -10, //
    -10, 4, 18, 26, 26, 18, 4, -10, //
    -16, 0, 12, 18, 18, 12, 0, -16, //
    -24, -10, 0, 4, 4, 0, -10, -24, //
    -40, -24, -16, -10, -10, -16, -24, -40,
];

/// The score for the side to move, in centipawns.
pub fn evaluate(p: &Position) -> i32 {
    let mut material = 0;
    for &pc in p.sq.iter() {
        let k = pc.abs();
        if k != PAWN && k != KING {
            material += VALUE[k as usize];
        }
    }
    let endgame = material <= 2600;
    let mut score = 0;
    for s in 0..64usize {
        let pc = p.sq[s];
        if pc == 0 {
            continue;
        }
        let i = if pc > 0 { s } else { s ^ 56 };
        let t = match pc.abs() {
            PAWN => PAWN_T[i],
            KNIGHT => KNIGHT_T[i],
            BISHOP => BISHOP_T[i],
            ROOK => ROOK_T[i],
            QUEEN => (KNIGHT_T[i] / 3) as i8,
            _ => {
                if endgame {
                    KING_END_T[i]
                } else {
                    KING_T[i]
                }
            }
        } as i32;
        let v = VALUE[pc.unsigned_abs() as usize] + t;
        score += if pc > 0 { v } else { -v };
    }
    score * p.side as i32
}

// ---- Search ----

pub const MATE: i32 = 30000;

#[derive(Clone, Copy, Default)]
struct TtEntry {
    key: u32,
    depth: i8,
    flag: u8,
    score: i16,
    mv: Move,
}

const TT_SIZE: usize = 1 << 15;

pub struct Search {
    pub nodes: u64,
    pub deadline: u32,
    pub stop: bool,
    /// Something outside asks the search to end (the user's Take back or
    /// New game while the computer thinks).
    pub abort: *const bool,
    tt: Vec<TtEntry>,
    killers: [[Move; 2]; 64],
    pub best: Move,
    pub depth_done: u32,
    pub score: i32,
    pub clock: fn() -> u32,
}

impl Search {
    pub fn new(clock: fn() -> u32) -> Search {
        Search { nodes: 0, deadline: 0, stop: false, abort: core::ptr::null(), tt: alloc::vec![TtEntry::default(); TT_SIZE], killers: [[Move::NONE; 2]; 64], best: Move::NONE, depth_done: 0, score: 0, clock }
    }

    fn time_up(&mut self) -> bool {
        if self.nodes & 1023 == 0 {
            let aborted = !self.abort.is_null() && unsafe { core::ptr::read_volatile(self.abort) };
            if aborted || (self.clock)().wrapping_sub(self.deadline) as i32 >= 0 {
                self.stop = true;
            }
        }
        self.stop
    }

    fn order(&self, p: &Position, moves: &mut [Move], hash_move: Move, ply: usize) {
        let score = |m: &Move| -> i32 {
            if *m == hash_move {
                return 1_000_000;
            }
            if m.flags & F_CAPTURE != 0 {
                let victim = if m.flags & F_EP != 0 { PAWN } else { p.sq[m.to as usize].abs() };
                return 100_000 + VALUE[victim as usize] * 10 - VALUE[p.sq[m.from as usize].unsigned_abs() as usize] / 10;
            }
            if m.promo != 0 {
                return 90_000;
            }
            if ply < 64 && (self.killers[ply][0] == *m || self.killers[ply][1] == *m) {
                return 50_000;
            }
            0
        };
        moves.sort_unstable_by_key(|m| -score(m));
    }

    fn quiesce(&mut self, p: &mut Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        if self.time_up() {
            return 0;
        }
        let stand = evaluate(p);
        if stand >= beta {
            return stand;
        }
        alpha = alpha.max(stand);
        if ply > 40 {
            return stand;
        }
        let mut v = Vec::with_capacity(16);
        p.pseudo_moves(true, &mut v);
        self.order(p, &mut v, Move::NONE, 63);
        for m in v {
            if !p.make(m) {
                continue;
            }
            let s = -self.quiesce(p, -beta, -alpha, ply + 1);
            p.unmake();
            if self.stop {
                return 0;
            }
            if s >= beta {
                return s;
            }
            alpha = alpha.max(s);
        }
        alpha
    }

    fn negamax(&mut self, p: &mut Position, depth: i32, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        if ply > 0 && (p.halfmove >= 100 || p.repetitions() >= 1 || p.insufficient()) {
            return 0;
        }
        if depth <= 0 {
            return self.quiesce(p, alpha, beta, ply);
        }
        self.nodes += 1;
        if self.time_up() {
            return 0;
        }
        let slot = (p.hash as usize) & (TT_SIZE - 1);
        let key = (p.hash >> 32) as u32;
        let e = self.tt[slot];
        let mut hash_move = Move::NONE;
        if e.key == key {
            hash_move = e.mv;
            if ply > 0 && e.depth as i32 >= depth {
                let s = e.score as i32;
                match e.flag {
                    0 => return s,
                    1 if s >= beta => return s,
                    2 if s <= alpha => return s,
                    _ => {}
                }
            }
        }
        let in_check = p.in_check(p.side);
        let depth = if in_check { depth + 1 } else { depth };
        let mut v = Vec::with_capacity(48);
        p.pseudo_moves(false, &mut v);
        self.order(p, &mut v, hash_move, ply);
        let alpha0 = alpha;
        let mut best = -MATE - 1;
        let mut best_move = Move::NONE;
        let mut legal = 0;
        for m in v {
            if !p.make(m) {
                continue;
            }
            legal += 1;
            let s = -self.negamax(p, depth - 1, -beta, -alpha, ply + 1);
            p.unmake();
            if self.stop {
                return 0;
            }
            if s > best {
                best = s;
                best_move = m;
                if ply == 0 {
                    self.best = m;
                    self.score = s;
                }
            }
            if s > alpha {
                alpha = s;
            }
            if alpha >= beta {
                if m.flags & F_CAPTURE == 0 && ply < 64 {
                    self.killers[ply][1] = self.killers[ply][0];
                    self.killers[ply][0] = m;
                }
                break;
            }
        }
        if legal == 0 {
            return if in_check { -MATE + ply as i32 } else { 0 };
        }
        let flag = if best >= beta {
            1
        } else if best <= alpha0 {
            2
        } else {
            0
        };
        self.tt[slot] = TtEntry { key, depth: depth.min(100) as i8, flag, score: best.clamp(-32000, 32000) as i16, mv: best_move };
        best
    }

    /// Search `p` up to `max_depth` plies or `ms` milliseconds; the best
    /// move found (none when there is no legal move).
    pub fn think(&mut self, p: &mut Position, max_depth: u32, ms: u32) -> Move {
        self.stop = false;
        self.nodes = 0;
        self.best = Move::NONE;
        self.depth_done = 0;
        self.deadline = (self.clock)().wrapping_add(ms);
        let mut chosen = Move::NONE;
        let legal = p.legal_moves();
        if legal.is_empty() {
            return chosen;
        }
        chosen = legal[0];
        if legal.len() == 1 {
            return chosen;
        }
        for d in 1..=max_depth {
            self.best = Move::NONE;
            let s = self.negamax(p, d as i32, -MATE - 1, MATE + 1, 0);
            if self.stop {
                // A partly searched depth still counts if it found a move
                // (the earlier best is searched first).
                if !self.best.is_none() && d > 1 {
                    chosen = self.best;
                }
                break;
            }
            if !self.best.is_none() {
                chosen = self.best;
            }
            self.depth_done = d;
            self.score = s;
            if s.abs() > MATE - 100 {
                break;
            }
        }
        chosen
    }
}

// ---- Opening book ----

/// A few standard openings, as moves in coordinate form; the computer
/// follows one that matches the game so far ("Use book openings").
const BOOK: &[&[u8]] = &[
    b"e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6 e1g1",
    b"e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 c2c3 g8f6 d2d4",
    b"e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4 g8f6 b1c3",
    b"e2e4 e7e6 d2d4 d7d5 b1c3 g8f6 c1g5",
    b"e2e4 c7c6 d2d4 d7d5 b1c3 d5e4 c3e4 c8f5",
    b"d2d4 d7d5 c2c4 e7e6 b1c3 g8f6 c1g5 f8e7",
    b"d2d4 g8f6 c2c4 e7e6 b1c3 f8b4 e2e3",
    b"d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d6",
    b"c2c4 e7e5 b1c3 g8f6 g1f3 b8c6",
    b"g1f3 d7d5 g2g3 g8f6 f1g2 e7e6 e1g1",
];

/// A book move for the game so far (`played`: the moves from the start),
/// picked with `pick` among the matching lines.
pub fn book_move(p: &mut Position, played: &[Move], pick: u32) -> Option<Move> {
    let mut cands: Vec<Move> = Vec::new();
    'lines: for line in BOOK {
        let ms: Vec<&[u8]> = line.split(|&c| c == b' ').collect();
        if ms.len() <= played.len() {
            continue;
        }
        for (i, m) in played.iter().enumerate() {
            let t = ms[i];
            if parse_square(&t[0..2]) != Some(m.from) || parse_square(&t[2..4]) != Some(m.to) {
                continue 'lines;
            }
        }
        let t = ms[played.len()];
        if let (Some(f), Some(to)) = (parse_square(&t[0..2]), parse_square(&t[2..4]))
            && let Some(m) = p.legal_moves().into_iter().find(|m| m.from == f && m.to == to)
            && !cands.contains(&m)
        {
            cands.push(m);
        }
    }
    if cands.is_empty() {
        return None;
    }
    Some(cands[(pick as usize) % cands.len()])
}
