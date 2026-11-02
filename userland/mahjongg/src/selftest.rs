//! `MAHJONGG /SELFTEST`: checks of the layouts, the rules, dealing and
//! the game files, as PASS/FAIL lines on the serial port and the
//! console, ending with "mahjongg selftest done: N failure(s)".

use alloc::vec::Vec;

use crate::game::{self, free_in, Game};
use crate::layout::{Layout, Pos, TILES};
use crate::trace;

struct T {
    fails: u32,
}

impl T {
    fn check(&mut self, ok: bool, what: &str) {
        let line = alloc::format!("{} {}", if ok { "PASS" } else { "FAIL" }, what);
        trace(line.as_bytes());
        os2::println!("{}", line);
        if !ok {
            self.fails += 1;
        }
    }
}

/// rs2's layouts, as shipped beside the program.
pub const LAYOUTS: [(&str, &[u8]); 3] = [
    ("PYRAMID.MAH", include_bytes!("../layouts/PYRAMID.MAH")),
    ("FORTRESS.MAH", include_bytes!("../layouts/FORTRESS.MAH")),
    ("TWINPEAK.MAH", include_bytes!("../layouts/TWINPEAK.MAH")),
];

/// Play a solvable deal's recorded order: each pair free and matching.
fn plays_out(g: &mut Game) -> bool {
    if g.solution.len() * 2 != g.pos.len() {
        return false;
    }
    for (a, b) in g.solution.clone() {
        if !g.free(a) || !g.free(b) || !game::matches(g.face[a], g.face[b]) {
            return false;
        }
        g.remove(a, b, false);
    }
    g.won()
}

fn same_positions(a: &Layout, b: &Layout) -> bool {
    let mut x: Vec<(i32, i32, i32)> = a.pos.iter().map(|p| (p.z, p.y, p.x)).collect();
    let mut y: Vec<(i32, i32, i32)> = b.pos.iter().map(|p| (p.z, p.y, p.x)).collect();
    x.sort();
    y.sort();
    x == y
}

pub fn run() -> u32 {
    let mut t = T { fails: 0 };

    // The tile set.
    let set = game::tile_set();
    t.check(set.len() == TILES, "the set has 144 tiles");
    t.check(game::matches(34, 37) && game::matches(38, 41) && !game::matches(34, 38) && game::matches(5, 5) && !game::matches(5, 6), "flowers match flowers, seasons seasons, others themselves");

    // The rules for free tiles.
    let row = [Pos { x: 0, y: 0, z: 0 }, Pos { x: 2, y: 0, z: 0 }, Pos { x: 4, y: 0, z: 0 }, Pos { x: 2, y: 0, z: 1 }];
    let mut present = [true, true, true, false];
    t.check(free_in(&row, &present, 0) && !free_in(&row, &present, 1) && free_in(&row, &present, 2), "a tile with neighbours on both sides is not free");
    present[0] = false;
    t.check(free_in(&row, &present, 1), "an open side frees it");
    present[3] = true;
    t.check(!free_in(&row, &present, 1) && free_in(&row, &present, 3), "a tile on top holds it");
    let half = [Pos { x: 0, y: 0, z: 0 }, Pos { x: 1, y: 1, z: 1 }];
    t.check(!free_in(&half, &[true, true], 0), "a tile half on top holds it too");

    // The layouts.
    let d = Layout::default_layout();
    t.check(d.pos.len() == TILES && d.check().is_ok() && d.levels() == 5, "the built-in turtle: 144 tiles on 5 levels, none overlapping");
    let mut all = alloc::vec![(alloc::string::String::from("Turtle"), d.clone())];
    for (name, text) in LAYOUTS {
        match Layout::parse(text) {
            Ok(l) => {
                let ok = l.check();
                t.check(ok.is_ok(), &alloc::format!("{name} reads: \"{}\", 144 tiles", core::str::from_utf8(&l.name).unwrap_or("?")));
                if ok.is_ok() {
                    all.push((alloc::string::String::from(name), l));
                }
            }
            Err(e) => t.check(false, &alloc::format!("{name}: {}", core::str::from_utf8(&e).unwrap_or("?"))),
        }
    }

    // Written and read back.
    for (name, l) in &all {
        let back = Layout::parse(&l.write());
        let ok = back.as_ref().is_ok_and(|b| same_positions(l, b) && b.name == l.name && b.dx == l.dx && b.dy == l.dy && b.color == l.color && b.x_stat == l.x_stat && b.y_stat == l.y_stat);
        t.check(ok, &alloc::format!("{name} written and read back unchanged"));
    }
    let path = [&[pmkit::boot_drive()][..], b":\\MJTEST.MAH"].concat();
    let saved = pmkit::write_file(&path, &d.write()).is_ok();
    let read = pmkit::read_file(&path).and_then(|b| Layout::parse(&b).ok());
    t.check(saved && read.is_some_and(|r| same_positions(&r, &d)), "a game file saved to disk reads back");
    unsafe { os2::DosDelete(pmkit::z(&path).as_ptr()) };

    // OS/2's own game files, where they are (not shipped with rs2).
    for f in ["BRIDGE", "CASTLE", "CRAZYCUB", "CUBE", "GIZEH", "TAJMAHAL", "TOWERBRI", "YINYANG"] {
        let p = alloc::format!("{}:\\IBM\\APPS\\{}.MAH", pmkit::boot_drive() as char, f);
        if let Some(text) = pmkit::read_file(p.as_bytes()) {
            let l = Layout::parse(&text);
            t.check(l.as_ref().is_ok_and(|l| l.check().is_ok()), &alloc::format!("OS/2's {f}.MAH reads, 144 tiles"));
        }
    }

    // Dealing: the same number gives the same deal; solvable deals play
    // out in their recorded order.
    let a = Game::deal(&d, 77, false);
    let b = Game::deal(&d, 77, false);
    let c = Game::deal(&d, 78, false);
    t.check(a.face == b.face && a.face != c.face, "a game number always gives the same deal");
    let mut sorted = a.face.clone();
    sorted.sort();
    let mut want = game::tile_set();
    want.sort();
    t.check(sorted == want, "a deal uses each tile of the set once");
    for (name, l) in &all {
        let mut bad = 0;
        for n in 1..=12 {
            let mut g = Game::deal(l, n, true);
            if !plays_out(&mut g) {
                bad += 1;
            }
        }
        t.check(bad == 0, &alloc::format!("{name}: 12 solvable deals play out ({bad} did not)"));
    }

    // Moves and taking them back.
    let mut g = Game::deal(&d, 5, true);
    let pairs = g.free_pairs();
    t.check(!pairs.is_empty() && g.match_count() == pairs.len(), "a solvable deal starts with a pair to take");
    if let Some(&(p, q)) = pairs.first() {
        g.remove(p, q, false);
        let left = g.left();
        g.take_back();
        t.check(left == TILES - 2 && g.left() == TILES && g.moves.is_empty(), "take back puts the pair back");
    }

    // Settings text.
    t.check(crate::parse_rgb_text(b"RGB(191,126,63)") == Some(0xbf7e3f), "colors read as OS/2 writes them");

    os2::println!("mahjongg selftest done: {} failure(s)", t.fails);
    trace(alloc::format!("mahjongg selftest done: {} failure(s)", t.fails).as_bytes());
    t.fails
}
