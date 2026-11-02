//! `KLONDIKE /SELFTEST`: checks of the game logic (dealing, the rules,
//! drawing and turning with each variation, the score, Take back and
//! Replay, auto-flip, Auto finish and Auto play) and of the card
//! drawing; PASS/FAIL lines on the serial port and stdout.

use alloc::vec::Vec;

use crate::game::*;
use crate::{app, cards, trace};

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    let line = alloc::format!("{} {}", if ok { "PASS" } else { "FAIL" }, name);
    trace(line.as_bytes());
    os2::println!("{}", line);
    if !ok {
        unsafe { FAILS += 1 };
    }
}

fn c(id: u8, up: bool) -> Card {
    Card { id, up }
}

/// Card ids: suit 0 clubs, 1 diamonds, 2 hearts, 3 spades.
fn card(suit: u8, rank: u8) -> u8 {
    suit * 13 + rank
}

fn empty() -> Game {
    let mut g = Game::new();
    for p in g.piles.iter_mut() {
        p.clear();
    }
    g
}

pub fn run() -> u32 {
    // Dealing.
    let mut g = Game::new();
    let deck: Vec<u8> = (0..52).collect();
    g.deal(&deck);
    check("deal: 24 cards on the talon", g.piles[TALON].len() == 24);
    check("deal: tableau piles of 1..7", (0..7).all(|i| g.piles[TABLEAU + i].len() == i + 1));
    check("deal: only the tableau's top cards face up", (0..7).all(|i| g.piles[TABLEAU + i].iter().enumerate().all(|(k, c)| c.up == (k == i))));
    check("deal: costs 52", g.score == START_SCORE - DEAL_COST);
    let mut all: Vec<u8> = g.piles.iter().flatten().map(|c| c.id).collect();
    all.sort();
    check("deal: every card once", all == (0..52).collect::<Vec<u8>>());
    let mut rng = pmkit::Rng::seeded(12345);
    let d1 = shuffled(&mut rng);
    let mut s = d1.clone();
    s.sort();
    check("shuffle: a permutation", s == (0..52).collect::<Vec<u8>>() && d1 != (0..52).collect::<Vec<u8>>());

    // The rules.
    let mut g = empty();
    g.piles[TABLEAU].push(c(card(2, 7), true)); // 8 of hearts
    g.piles[TABLEAU + 1].push(c(card(3, 6), true)); // 7 of spades
    g.piles[TABLEAU + 2].push(c(card(1, 6), true)); // 7 of diamonds
    g.piles[TABLEAU + 3].push(c(card(0, 12), true)); // king of clubs
    g.piles[WASTE].push(c(card(1, 0), true)); // ace of diamonds
    check("rules: black 7 on red 8", g.legal(TABLEAU + 1, TABLEAU, 1));
    check("rules: red 7 not on red 8", !g.legal(TABLEAU + 2, TABLEAU, 1));
    check("rules: king to an empty pile", g.legal(TABLEAU + 3, TABLEAU + 4, 1));
    check("rules: no other card to an empty pile", !g.legal(TABLEAU + 1, TABLEAU + 4, 1));
    check("rules: ace to an empty foundation", g.legal(WASTE, FOUNDATION, 1));
    check("rules: 7 not to an empty foundation", !g.legal(TABLEAU + 1, FOUNDATION, 1));
    check("rules: nothing onto the waste pile", !g.legal(TABLEAU + 1, WASTE, 1));
    let s0 = g.score;
    check("move: ace to foundation", g.play(Move::Transfer { from: WASTE as u8, to: FOUNDATION as u8, n: 1, flipped: false, cheat: false }));
    check("score: +5 for a foundation card", g.score == s0 + FOUNDATION_CARD);
    g.piles[TABLEAU + 5].push(c(card(1, 1), true)); // 2 of diamonds
    check("rules: 2 on its ace", g.legal(TABLEAU + 5, FOUNDATION, 1));
    check("rules: not on another suit's ace", {
        g.piles[FOUNDATION + 1].push(c(card(0, 0), true));
        !g.legal(TABLEAU + 5, FOUNDATION + 1, 1)
    });
    check("move: illegal refused", !g.play(Move::Transfer { from: TABLEAU as u8 + 2, to: TABLEAU as u8, n: 1, flipped: false, cheat: false }));
    check("move: cheat allowed", g.play(Move::Transfer { from: TABLEAU as u8 + 2, to: TABLEAU as u8, n: 1, flipped: false, cheat: true }) && g.cheats == 1);
    g.take_back();
    check("take back: cheat undone", g.piles[TABLEAU + 2].len() == 1 && g.piles[TABLEAU].len() == 1);

    // Runs and auto-flip.
    let mut g = empty();
    g.piles[TABLEAU].extend([c(card(0, 3), false), c(card(2, 9), true), c(card(3, 8), true), c(card(1, 7), true)]); // x, 10H 9S 8D
    g.piles[TABLEAU + 1].push(c(card(0, 10), true)); // jack of clubs
    check("runs: three cards movable", g.movable(TABLEAU) == 3);
    check("runs: the run onto a black jack", g.legal(TABLEAU, TABLEAU + 1, 3));
    check("runs: part of it does not fit", !g.legal(TABLEAU, TABLEAU + 1, 2));
    g.play(Move::Transfer { from: TABLEAU as u8, to: TABLEAU as u8 + 1, n: 3, flipped: false, cheat: false });
    check("auto-flip: the card left is turned up", g.piles[TABLEAU].len() == 1 && g.piles[TABLEAU][0].up && g.piles[TABLEAU + 1].len() == 4);
    g.take_back();
    check("take back: the run and the flip undone", g.piles[TABLEAU].len() == 4 && !g.piles[TABLEAU][0].up && g.piles[TABLEAU + 1].len() == 1);
    check("replay: possible after take back", g.can_replay());
    g.replay();
    check("replay: the move again", g.piles[TABLEAU + 1].len() == 4 && g.piles[TABLEAU][0].up);
    let mut g2 = g.clone();
    g2.piles[TABLEAU + 2].push(c(card(3, 2), false));
    g2.take_back();
    check("a new move drops the moves taken back", g2.play(Move::Flip { pile: TABLEAU as u8 + 2 }) && !g2.can_replay());
    let mut g = empty();
    g.auto_flip = false;
    g.piles[TABLEAU].extend([c(card(0, 3), false), c(card(2, 9), true)]);
    g.piles[TABLEAU + 1].push(c(card(0, 10), true));
    g.play(Move::Transfer { from: TABLEAU as u8, to: TABLEAU as u8 + 1, n: 1, flipped: false, cheat: false });
    check("no auto-flip: the card stays down", !g.piles[TABLEAU][0].up);
    check("no auto-flip: turned up by hand", g.play(Move::Flip { pile: TABLEAU as u8 }) && g.piles[TABLEAU][0].up);

    // Drawing and turning.
    for (draw, turns, name) in [(3u32, 0u32, "draw 3, flip none"), (3, 3, "draw 3, flip 3"), (3, MANY, "draw 3, flip many"), (1, MANY, "draw 1, flip many")] {
        let mut g = Game::new();
        g.draw = draw;
        g.turns_allowed = turns;
        g.deal(&deck);
        let mut draws = 0;
        while g.play(Move::Draw { n: 0 }) {
            draws += 1;
        }
        check(&alloc::format!("{name}: the talon in {draws} draws"), draws == 24usize.div_ceil(draw as usize) && g.piles[WASTE].len() == 24);
        check(&alloc::format!("{name}: waste cards face up"), g.piles[WASTE].iter().all(|c| c.up));
        let before = g.score;
        let turned = g.play(Move::Turn { n: 0, paid: true });
        check(&alloc::format!("{name}: turning the talon"), turned == (turns > 0));
        if turned {
            check(&alloc::format!("{name}: a turn costs 25"), g.score == before - TURN_COST && g.piles[TALON].len() == 24 && g.piles[TALON].iter().all(|c| !c.up));
            g.take_back();
            check(&alloc::format!("{name}: taking a turn back refunds it"), g.score == before && g.piles[WASTE].len() == 24);
        }
    }
    let mut g = Game::new();
    g.turns_allowed = 3;
    g.deal(&deck);
    let mut turns = 0;
    loop {
        while g.play(Move::Draw { n: 0 }) {}
        if !g.play(Move::Turn { n: 0, paid: true }) {
            break;
        }
        turns += 1;
    }
    check("draw 3, flip 3: three turns only", turns == 3);
    // The waste's order survives a turn: the first card drawn comes first again.
    let mut g = Game::new();
    g.draw = 1;
    g.deal(&deck);
    let first = g.top(TALON).unwrap().id;
    while g.play(Move::Draw { n: 0 }) {}
    g.play(Move::Turn { n: 0, paid: true });
    g.play(Move::Draw { n: 0 });
    check("turning keeps the talon's order", g.top(WASTE).map(|c| c.id) == Some(first));

    // Auto finish: a game with everything face up finishes.
    let mut g = empty();
    for s in 0..4u8 {
        for r in 0..13u8 {
            // Each tableau pile from king down to ace of one suit... in
            // alternating colours that is not a legal run, so put whole
            // suits as face-up piles: only tops matter for Auto finish.
            g.piles[TABLEAU + s as usize].insert(0, c(card(s, r), true));
        }
    }
    let mut steps = 0;
    while let Some(m) = g.finish_step() {
        if !g.play(m) || steps > 200 {
            break;
        }
        steps += 1;
    }
    check("auto finish: all 52 cards home", g.won() && steps == 52);
    check("auto finish: score +260", g.score == START_SCORE + 52 * FOUNDATION_CARD);
    while g.take_back().is_some() {}
    check("take back to the start", g.on_foundations() == 0 && g.score == START_SCORE);

    // Auto play makes legal moves until it is stuck or has won.
    let mut wins = 0;
    for seed in 1..=10u32 {
        let mut g = Game::new();
        g.draw = 1;
        let mut rng = pmkit::Rng::seeded(seed * 7919);
        g.deal(&shuffled(&mut rng));
        let mut idle = 0;
        let mut n = 0;
        while let Some(m) = g.auto_step(idle) {
            if !g.play(m) {
                break;
            }
            idle = if Game::progress(&m) { 0 } else { idle + 1 };
            n += 1;
            if n > 5000 {
                break;
            }
        }
        let total: usize = g.piles.iter().map(|p| p.len()).sum();
        if total != 52 || n > 5000 {
            check(&alloc::format!("auto play seed {seed}: ends with 52 cards"), false);
        }
        if g.won() {
            wins += 1;
        }
    }
    check(&alloc::format!("auto play: 10 games end ({wins} won)"), true);

    // The pictures: an atlas at a small and a large size.
    for (w, h) in [(40, 56), (100, 140)] {
        let ok = cards::Atlas::new(app().hab, w, h, 0x1f6f3a).is_some();
        check(&alloc::format!("cards: drawn at {w}x{h}"), ok);
    }
    let _ = cards::all();

    let fails = unsafe { FAILS };
    let done = alloc::format!("klondike selftest done: {fails} failure(s)");
    trace(done.as_bytes());
    os2::println!("{}", done);
    fails
}
