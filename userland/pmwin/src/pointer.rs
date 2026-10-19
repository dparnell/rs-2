//! The mouse pointer, drawn in software with a save-under. Drawing code
//! hides it first so it never ends up in saved pixels.

use gpi::Point;

use crate::state::State;

const W: usize = 12;
const H: usize = 19;

/// Arrow: '#' black outline, '.' white fill.
const ARROW: [&[u8; W]; H] = [
    b"#           ",
    b"##          ",
    b"#.#         ",
    b"#..#        ",
    b"#...#       ",
    b"#....#      ",
    b"#.....#     ",
    b"#......#    ",
    b"#.......#   ",
    b"#........#  ",
    b"#.........# ",
    b"#......#####",
    b"#...#..#    ",
    b"#..# #..#   ",
    b"#.#  #..#   ",
    b"##    #..#  ",
    b"#     #..#  ",
    b"       #..# ",
    b"        ##  ",
];

/// Pixels of the arrow with the hot spot (top-left) at `p`: (x, y, i, j).
fn cells(p: Point) -> impl Iterator<Item = (i32, i32, usize, usize)> {
    (0..H).flat_map(move |j| (0..W).map(move |i| (p.x + i as i32, p.y - j as i32, i, j)))
}

/// The PM server takes the pointer over.
pub fn init(s: &mut State) {
    s.ptr_saved = None;
    s.ptr_hidden = 0;
    draw(s);
}

pub fn draw(s: &mut State) {
    if s.ptr_hardware || s.ptr_hidden > 0 || s.ptr_saved.is_some() {
        return;
    }
    let p = s.pointer;
    let mut saved = alloc::vec::Vec::with_capacity(W * H);
    for (x, y, _, _) in cells(p) {
        saved.push(s.surface.get(x, y).unwrap_or(0));
    }
    for (x, y, i, j) in cells(p) {
        match ARROW[j][i] {
            b'#' => s.surface.set(x, y, 0x000000),
            b'.' => s.surface.set(x, y, 0xffffff),
            _ => {}
        }
    }
    s.ptr_saved = Some((p, saved));
}

pub fn erase(s: &mut State) {
    let Some((p, saved)) = s.ptr_saved.take() else { return };
    for ((x, y, _, _), c) in cells(p).zip(saved) {
        s.surface.set(x, y, c);
    }
}

/// Hide the pointer around drawing (only ever within one PMWIN call:
/// never across application code).
pub fn hide(s: &mut State) {
    erase(s);
    s.ptr_hidden += 1;
}

pub fn show(s: &mut State) {
    s.ptr_hidden = s.ptr_hidden.saturating_sub(1);
    draw(s);
}

pub fn move_to(s: &mut State, p: Point) {
    erase(s);
    s.pointer = p;
    draw(s);
}
