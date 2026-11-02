//! `OS2CHESS /SELFTEST`: checks of the rules and the computer player,
//! as PASS/FAIL lines on the serial port and "os2chess selftest done: N
//! failure(s)".

use alloc::vec::Vec;

use crate::engine::*;
use crate::{ms, trace};

fn check(fails: &mut u32, ok: bool, what: &str) {
    let mut t = if ok { b"PASS ".to_vec() } else { b"FAIL ".to_vec() };
    t.extend_from_slice(what.as_bytes());
    trace(&t);
    if !ok {
        *fails += 1;
    }
}

pub fn run() -> u32 {
    let mut fails = 0;
    // Perft: the numbers of move sequences every chess program agrees on.
    let cases: [(&[u8], u32, u64); 6] = [
        (START_FEN, 3, 8902),
        (START_FEN, 4, 197281),
        (b"r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 2, 2039),
        (b"r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 3, 97862),
        (b"8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 4, 43238),
        (b"r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", 3, 9467),
    ];
    for (fen, depth, want) in cases {
        let mut p = Position::from_fen(fen).unwrap();
        let t0 = ms();
        let n = p.perft(depth);
        let back = p.fen();
        let what = alloc::format!("perft {} depth {}: {} (want {}) in {} ms", core::str::from_utf8(&fen[..fen.len().min(24)]).unwrap_or(""), depth, n, want, ms().wrapping_sub(t0));
        check(&mut fails, n == want, &what);
        check(&mut fails, back.as_slice() == fen, "position unchanged after perft (make/unmake)");
    }
    // Mate in one: Qh5xf7#.
    let mut p = Position::from_fen(b"r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4").unwrap();
    let mut s = Search::new(ms);
    let m = s.think(&mut p, 3, 5000);
    let note = p.notation(m);
    check(&mut fails, note.as_slice() == b"Qh5xf7#", &alloc::format!("mate in one found: {}", core::str::from_utf8(&note).unwrap_or("?")));
    // A back rank mate for black: Ra1#... (black to move: Re1#).
    let mut p = Position::from_fen(b"4r1k1/5ppp/8/8/8/8/5PPP/6K1 b - - 0 1").unwrap();
    let m = s.think(&mut p, 3, 5000);
    let note = p.notation(m);
    check(&mut fails, note.as_slice() == b"Re8-e1#", &alloc::format!("back rank mate: {}", core::str::from_utf8(&note).unwrap_or("?")));
    // Status: checkmate, stalemate, insufficient material.
    let mut p = Position::from_fen(b"rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3").unwrap();
    check(&mut fails, p.status() == Status::Checkmate, "fool's mate is checkmate");
    let mut p = Position::from_fen(b"7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    check(&mut fails, p.status() == Status::Stalemate, "stalemate");
    let mut p = Position::from_fen(b"8/8/4k3/8/8/3NK3/8/8 w - - 0 1").unwrap();
    check(&mut fails, p.status() == Status::Material, "king and knight cannot mate");
    // Repetition: knights out and back twice.
    let mut p = Position::start();
    let seq: [(&[u8], &[u8]); 8] = [(b"g1", b"f3"), (b"g8", b"f6"), (b"f3", b"g1"), (b"f6", b"g8"), (b"g1", b"f3"), (b"g8", b"f6"), (b"f3", b"g1"), (b"f6", b"g8")];
    for (f, t) in seq {
        let m = p.find_move(parse_square(f).unwrap(), parse_square(t).unwrap(), 0).unwrap();
        p.make(m);
    }
    check(&mut fails, p.status() == Status::Repetition, "threefold repetition");
    // En passant and castling notation, promotion.
    let mut p = Position::from_fen(b"r3k2r/8/8/3pP3/8/8/8/R3K2R w KQkq d6 0 1").unwrap();
    let notes: Vec<Vec<u8>> = p.legal_moves().into_iter().filter(|m| m.flags & (F_EP | F_CASTLE) != 0).map(|m| p.notation(m)).collect();
    check(&mut fails, notes.iter().any(|n| n.as_slice() == b"e5xd6") && notes.iter().any(|n| n.as_slice() == b"O-O") && notes.iter().any(|n| n.as_slice() == b"O-O-O"), "en passant and castling moves");
    let mut p = Position::from_fen(b"8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
    let promos = p.legal_moves().into_iter().filter(|m| m.promo != 0).count();
    check(&mut fails, promos == 4, "a pawn promotes to four pieces");
    // The opening book answers 1. e4.
    let mut p = Position::start();
    let e4 = p.find_move(12, 28, 0).unwrap();
    p.make(e4);
    check(&mut fails, book_move(&mut p, &[e4], 1).is_some(), "book move after e2-e4");
    trace(alloc::format!("os2chess selftest done: {fails} failure(s)").as_bytes());
    fails
}
