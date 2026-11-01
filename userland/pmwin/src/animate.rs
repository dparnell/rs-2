//! Window animation (SV_ANIMATION, the System object's Window page): a
//! frame that is minimized, restored, maximized or hidden zooms there as
//! a few outlines growing or shrinking from where it was to where it
//! goes, drawn inverted on the screen and taken away again.

use gpi::Rect;

use crate::state::st;

unsafe extern "C" {
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
}

/// SV_ANIMATION.
pub const SV_ANIMATION: i32 = 93;

/// Outlines drawn on the way and the time each stays (ms).
const STEPS: i32 = 8;
const STEP_MS: u32 = 12;

fn outline(r: Rect) {
    let s = st();
    let inv = |_: i32, _: i32, c: u32| c ^ 0xff_ffff;
    let (l, b, rt, t) = (r.left, r.bottom, r.right, r.top);
    if rt - l < 2 || t - b < 2 {
        return;
    }
    // Two pixels wide, the corners once each.
    s.surface.map(Rect::new(l, t - 2, rt, t), inv);
    s.surface.map(Rect::new(l, b, rt, b + 2), inv);
    s.surface.map(Rect::new(l, b + 2, l + 2, t - 2), inv);
    s.surface.map(Rect::new(rt - 2, b + 2, rt, t - 2), inv);
}

/// Zoom from `from` to `to` (screen coordinates) when windows are
/// animated. Called with the PM lock held; the screen shows each outline
/// at once.
pub fn zoom(from: Rect, to: Rect) {
    if crate::misc::WinQuerySysValue(1, SV_ANIMATION) == 0 {
        return;
    }
    // Only while PM draws on the screen (not in the shadow buffer of a
    // full-screen session).
    let s = st();
    if s.surface.pixels() as usize != s.screen {
        return;
    }
    crate::pointer::hide(s);
    // What was drawn before goes to the screen first.
    crate::display::flush_now();
    let lerp = |a: i32, b: i32, k: i32| a + (b - a) * k / STEPS;
    for k in 1..=STEPS {
        let r = Rect::new(lerp(from.left, to.left, k), lerp(from.bottom, to.bottom, k), lerp(from.right, to.right, k), lerp(from.top, to.top, k));
        outline(r);
        crate::display::flush_now();
        unsafe { DosSleep(STEP_MS) };
        outline(r);
    }
    crate::display::flush_now();
    crate::pointer::show(st());
}
