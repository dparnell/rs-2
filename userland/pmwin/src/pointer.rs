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

unsafe extern "C" {
    #[link_name = "RS2API.5"]
    fn Rs2SetPointerShape(cx: u32, cy: u32, hx: u32, hy: u32, pixels: *const u32) -> u32;
    #[link_name = "RS2API.6"]
    fn Rs2ShowPointer(show: u32) -> u32;
}

/// The PM server takes the pointer over: a hardware cursor if the display
/// has one (the kernel moves it, so it never waits for PM), otherwise
/// drawn here in software.
pub fn init(s: &mut State) {
    erase(s);
    s.ptr_saved = None;
    s.ptr_hidden = 0;
    let mut px = alloc::vec::Vec::with_capacity(W * H);
    for row in ARROW {
        for &c in row.iter() {
            px.push(match c {
                b'#' => 0xff00_0000,
                b'.' => 0xffff_ffff,
                _ => 0,
            });
        }
    }
    s.ptr_hardware = unsafe { Rs2SetPointerShape(W as u32, H as u32, 0, 0, px.as_ptr()) } == 0;
    draw(s);
}

/// WinShowPointer(hwndDesktop, fShow): a show count; the pointer is seen
/// while it is not negative.
#[unsafe(no_mangle)]
pub extern "C" fn WinShowPointer(_desk: u32, show: u32) -> u32 {
    let s = crate::state::st();
    if show != 0 {
        s.ptr_level = s.ptr_level.saturating_add(1);
    } else {
        s.ptr_level = s.ptr_level.saturating_sub(1);
    }
    let on = s.ptr_level >= 0;
    if s.ptr_hardware {
        unsafe { Rs2ShowPointer(on as u32) };
    } else if on {
        draw(s);
    } else {
        erase(s);
    }
    1
}

pub fn draw(s: &mut State) {
    if s.ptr_hardware || s.ptr_hidden > 0 || s.ptr_level < 0 || s.ptr_saved.is_some() {
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
