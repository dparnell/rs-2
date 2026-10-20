//! The window tree.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use gpi::{Point, Rect};

use crate::region::Region;
use crate::ws;

pub type Hwnd = u32;

pub const HWND_DESKTOP: Hwnd = 1;
pub const HWND_OBJECT: Hwnd = 2;

pub struct Window {
    pub hwnd: Hwnd,
    /// 0 for the desktop and the object window.
    pub parent: Hwnd,
    pub owner: Hwnd,
    pub class: u32,
    pub style: u32,
    pub id: u32,
    /// Position relative to the parent's origin, and size.
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub text: Vec<u8>,
    /// Children, bottom of the z-order first.
    pub children: Vec<Hwnd>,
    /// Area that needs painting, in window coordinates.
    pub update: Region,
    /// Message queue that owns the window.
    pub queue: u32,
    /// Window procedure.
    pub proc_: u32,
    /// Window words (QWL_USER and the class's extra bytes, as u32s).
    pub words: Vec<u32>,
    /// Part of the update region the PM server has already painted with
    /// a placeholder while the window's thread does not respond.
    pub ghost: Region,
}

impl Window {
    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.cx, self.cy)
    }

    pub fn visible_style(&self) -> bool {
        self.style & ws::VISIBLE != 0
    }
}

pub struct Desktop {
    windows: BTreeMap<Hwnd, Window>,
    next: Hwnd,
    /// Queues with windows that got something to paint since the set was
    /// last taken (their threads must be woken).
    wake: BTreeSet<u32>,
    /// Bumped whenever window positions, sizes, z-order or visibility
    /// change (visible regions computed before are stale).
    pub generation: u32,
    /// A top-level window kept below all others (a desktop shell's
    /// desktop; 0: none): it never comes to the top, and windows sent to
    /// the bottom go just above it.
    pub pinned: Hwnd,
}

impl Desktop {
    /// A desktop of `w` x `h` pixels with the desktop and object windows.
    pub fn new(w: i32, h: i32) -> Desktop {
        let mut d = Desktop { windows: BTreeMap::new(), next: 3, wake: BTreeSet::new(), generation: 0, pinned: 0 };
        for (hwnd, style, cx, cy) in [(HWND_DESKTOP, ws::VISIBLE | ws::CLIPCHILDREN, w, h), (HWND_OBJECT, 0, 0, 0)] {
            d.windows.insert(hwnd, Window {
                hwnd,
                parent: 0,
                owner: 0,
                class: 0,
                style,
                id: 0,
                x: 0,
                y: 0,
                cx,
                cy,
                text: Vec::new(),
                children: Vec::new(),
                update: Region::from_rect(Rect::new(0, 0, cx, cy)),
                queue: 0,
                proc_: 0,
                words: Vec::new(),
                ghost: Region::new(),
            });
        }
        d
    }

    pub fn wake_pending(&self) -> bool {
        !self.wake.is_empty()
    }

    /// Take the set of queues to wake (see `wake`).
    pub fn take_wake(&mut self) -> BTreeSet<u32> {
        core::mem::take(&mut self.wake)
    }

    /// Note that a window got something to paint.
    fn dirty(&mut self, h: Hwnd) {
        if let Some(w) = self.windows.get(&h) {
            if !w.update.is_empty() {
                self.wake.insert(w.queue);
            }
        }
    }

    pub fn all_mut(&mut self) -> impl Iterator<Item = &mut Window> {
        self.windows.values_mut()
    }

    pub fn get(&self, h: Hwnd) -> Option<&Window> {
        self.windows.get(&h)
    }

    pub fn get_mut(&mut self, h: Hwnd) -> Option<&mut Window> {
        self.windows.get_mut(&h)
    }

    pub fn exists(&self, h: Hwnd) -> bool {
        self.windows.contains_key(&h)
    }

    pub fn all(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }

    /// Create a window on top of its siblings.
    #[allow(clippy::too_many_arguments)]
    pub fn create(&mut self, parent: Hwnd, owner: Hwnd, class: u32, style: u32, pos: Rect, id: u32, queue: u32, proc_: u32, words: usize) -> Option<Hwnd> {
        if !self.exists(parent) {
            return None;
        }
        let hwnd = self.next;
        self.next += 1;
        self.windows.insert(hwnd, Window {
            hwnd,
            parent,
            owner,
            class,
            style,
            id,
            x: pos.left,
            y: pos.bottom,
            cx: pos.width().max(0),
            cy: pos.height().max(0),
            text: Vec::new(),
            children: Vec::new(),
            update: Region::from_rect(Rect::new(0, 0, pos.width().max(0), pos.height().max(0))),
            queue,
            proc_,
            words: alloc::vec![0; words],
            ghost: Region::new(),
        });
        self.windows.get_mut(&parent)?.children.push(hwnd);
        self.generation = self.generation.wrapping_add(1);
        self.wake.insert(queue);
        Some(hwnd)
    }

    /// Remove a window and its descendants; returns them, children first.
    pub fn destroy(&mut self, h: Hwnd) -> Vec<Hwnd> {
        let mut out = Vec::new();
        self.collect(h, &mut out);
        if let Some(p) = self.get(h).map(|w| w.parent) {
            if let Some(pw) = self.windows.get_mut(&p) {
                pw.children.retain(|&c| c != h);
            }
        }
        for c in &out {
            self.windows.remove(c);
        }
        if out.contains(&self.pinned) {
            self.pinned = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        out
    }

    fn collect(&self, h: Hwnd, out: &mut Vec<Hwnd>) {
        if let Some(w) = self.get(h) {
            for &c in &w.children {
                self.collect(c, out);
            }
            out.push(h);
        }
    }

    /// Device position of the window's origin (bottom-left).
    pub fn origin(&self, h: Hwnd) -> Point {
        let mut p = Point::new(0, 0);
        let mut cur = h;
        while let Some(w) = self.get(cur) {
            p.x += w.x;
            p.y += w.y;
            cur = w.parent;
        }
        p
    }

    pub fn screen_rect(&self, h: Hwnd) -> Rect {
        let o = self.origin(h);
        self.get(h).map_or(Rect::default(), |w| Rect::new(o.x, o.y, o.x + w.cx, o.y + w.cy))
    }

    /// Visible itself and through all its ancestors up to the desktop.
    pub fn is_showing(&self, h: Hwnd) -> bool {
        let mut cur = h;
        loop {
            let Some(w) = self.get(cur) else { return false };
            if !w.visible_style() {
                return false;
            }
            if cur == HWND_DESKTOP {
                return true;
            }
            if w.parent == 0 {
                return false;
            }
            cur = w.parent;
        }
    }

    /// The part of the window that can be seen, in device coordinates:
    /// clipped to its ancestors, minus siblings above it at every level,
    /// and minus its own visible children if `clip_children`.
    pub fn visible_region(&self, h: Hwnd, clip_children: bool) -> Region {
        if !self.is_showing(h) {
            return Region::new();
        }
        let mut g = Region::from_rect(self.screen_rect(h));
        let mut cur = h;
        while let Some(w) = self.get(cur) {
            let Some(p) = self.get(w.parent) else { break };
            g.intersect(&self.screen_rect(w.parent));
            let above = p.children.iter().skip_while(|&&c| c != cur).skip(1);
            for &s in above {
                if self.get(s).is_some_and(|x| x.visible_style()) {
                    g.subtract(&self.screen_rect(s));
                }
            }
            cur = w.parent;
        }
        if clip_children {
            for &c in &self.get(h).map_or(Vec::new(), |w| w.children.clone()) {
                if self.get(c).is_some_and(|x| x.visible_style()) {
                    g.subtract(&self.screen_rect(c));
                }
            }
        }
        g
    }

    /// Mark a window rectangle (window coordinates; None = all) for
    /// repainting.
    pub fn invalidate(&mut self, h: Hwnd, r: Option<Rect>, children: bool) {
        let Some(w) = self.windows.get_mut(&h) else { return };
        let r = r.unwrap_or(w.rect()).intersect(&w.rect());
        w.update.add(r);
        // Drawn over since: a placeholder there is gone too.
        w.ghost.subtract(&r);
        self.dirty(h);
        let Some(w) = self.windows.get(&h) else { return };
        if children {
            let kids = w.children.clone();
            let (wx, wy) = (0, 0);
            for c in kids {
                let Some(cw) = self.get(c) else { continue };
                let local = r.offset(wx - cw.x, wy - cw.y);
                self.invalidate(c, Some(local), true);
            }
        }
    }

    /// Mark a device rectangle for repainting in every showing window it
    /// touches.
    pub fn invalidate_screen(&mut self, r: Rect) {
        let hs: Vec<Hwnd> = self.windows.keys().copied().collect();
        for h in hs {
            if !self.is_showing(h) {
                continue;
            }
            let sr = self.screen_rect(h);
            let i = r.intersect(&sr);
            if !i.is_empty() {
                let local = i.offset(-sr.left, -sr.bottom);
                if let Some(w) = self.windows.get_mut(&h) {
                    w.update.add(local);
                    w.ghost.subtract(&local);
                }
                self.dirty(h);
            }
        }
    }

    /// The topmost showing window at a device point.
    pub fn window_at(&self, p: Point) -> Hwnd {
        let mut cur = HWND_DESKTOP;
        'down: loop {
            let Some(w) = self.get(cur) else { return HWND_DESKTOP };
            for &c in w.children.iter().rev() {
                if self.get(c).is_some_and(|x| x.visible_style()) && self.screen_rect(c).contains(p) {
                    cur = c;
                    continue 'down;
                }
            }
            return cur;
        }
    }

    /// Move/size a window; the old and new screen areas are invalidated.
    pub fn set_pos(&mut self, h: Hwnd, x: i32, y: i32, cx: i32, cy: i32) {
        let old = self.screen_rect(h);
        let showing = self.is_showing(h);
        if let Some(w) = self.windows.get_mut(&h) {
            w.x = x;
            w.y = y;
            w.cx = cx.max(0);
            w.cy = cy.max(0);
        }
        self.generation = self.generation.wrapping_add(1);
        if showing {
            self.invalidate_screen(old);
            let new = self.screen_rect(h);
            self.invalidate_screen(new);
        }
    }

    /// Move a window (keeping its position relative to the new parent)
    /// to the top of a new parent's children.
    pub fn set_parent(&mut self, h: Hwnd, parent: Hwnd) -> bool {
        if !self.exists(parent) || self.is_ancestor(h, parent) {
            return false;
        }
        let Some(old) = self.get(h).map(|w| w.parent) else { return false };
        let r = self.screen_rect(h);
        if let Some(pw) = self.windows.get_mut(&old) {
            pw.children.retain(|&c| c != h);
        }
        if let Some(pw) = self.windows.get_mut(&parent) {
            pw.children.push(h);
        }
        if let Some(w) = self.windows.get_mut(&h) {
            w.parent = parent;
        }
        self.generation = self.generation.wrapping_add(1);
        self.invalidate_screen(r);
        self.invalidate(h, None, true);
        true
    }

    /// Bring a window to the top of its siblings (not the pinned one).
    pub fn to_top(&mut self, h: Hwnd) {
        if h == self.pinned && h != 0 {
            return;
        }
        let Some(p) = self.get(h).map(|w| w.parent) else { return };
        if let Some(pw) = self.windows.get_mut(&p) {
            if pw.children.last() == Some(&h) {
                return;
            }
            pw.children.retain(|&c| c != h);
            pw.children.push(h);
        }
        self.generation = self.generation.wrapping_add(1);
        self.invalidate(h, None, true);
    }

    /// Move a window to the bottom of its siblings; what it uncovers is
    /// repainted.
    pub fn to_bottom(&mut self, h: Hwnd) {
        let Some(p) = self.get(h).map(|w| w.parent) else { return };
        let pinned = self.pinned;
        if let Some(pw) = self.windows.get_mut(&p) {
            // Above the pinned window, if it is a sibling.
            let at = usize::from(pinned != h && pw.children.first() == Some(&pinned) && pinned != 0);
            if pw.children.get(at) == Some(&h) {
                return;
            }
            pw.children.retain(|&c| c != h);
            pw.children.insert(at, h);
        }
        self.generation = self.generation.wrapping_add(1);
        let r = self.screen_rect(h);
        self.invalidate_screen(r);
    }

    /// Keep a child of the desktop below all others (0: none any more).
    /// Returns the previous one.
    pub fn pin_bottom(&mut self, h: Hwnd) -> Hwnd {
        let old = core::mem::replace(&mut self.pinned, 0);
        if h != 0 && self.get(h).is_some_and(|w| w.parent == HWND_DESKTOP) {
            self.to_bottom(h);
            self.pinned = h;
        }
        old
    }

    /// The windows of a message queue.
    pub fn windows_of(&self, queue: u32) -> Vec<Hwnd> {
        self.windows.values().filter(|w| w.queue == queue).map(|w| w.hwnd).collect()
    }

    pub fn set_visible(&mut self, h: Hwnd, on: bool) {
        let r = self.screen_rect(h);
        if let Some(w) = self.windows.get_mut(&h) {
            if on {
                w.style |= ws::VISIBLE;
            } else {
                w.style &= !ws::VISIBLE;
            }
        }
        self.generation = self.generation.wrapping_add(1);
        self.invalidate_screen(r);
    }

    /// The next window of `queue` that needs painting (parents first).
    pub fn paint_pending(&self, queue: u32) -> Option<Hwnd> {
        let mut stack = alloc::vec![HWND_DESKTOP];
        while let Some(h) = stack.pop() {
            let Some(w) = self.get(h) else { continue };
            if !w.visible_style() {
                continue;
            }
            if w.queue == queue && !w.update.is_empty() {
                return Some(h);
            }
            stack.extend(w.children.iter().rev().copied());
        }
        None
    }

    /// Ancestors chain check: is `a` an ancestor of (or equal to) `h`?
    pub fn is_ancestor(&self, a: Hwnd, h: Hwnd) -> bool {
        let mut cur = h;
        while cur != 0 {
            if cur == a {
                return true;
            }
            cur = self.get(cur).map_or(0, |w| w.parent);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desk() -> Desktop {
        let mut d = Desktop::new(640, 480);
        d.get_mut(HWND_DESKTOP).unwrap().update = Region::new();
        d
    }

    #[test]
    fn overlapping_windows_clip() {
        let mut d = desk();
        let a = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(10, 10, 110, 110), 1, 1, 0, 0).unwrap();
        let b = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(60, 60, 160, 160), 2, 1, 0, 0).unwrap();
        assert_eq!(d.visible_region(b, false).area(), 100 * 100);
        assert_eq!(d.visible_region(a, false).area(), 100 * 100 - 50 * 50);
        assert_eq!(d.window_at(Point::new(70, 70)), b);
        assert_eq!(d.window_at(Point::new(20, 20)), a);
        assert_eq!(d.window_at(Point::new(300, 300)), HWND_DESKTOP);
        d.to_top(a);
        assert_eq!(d.window_at(Point::new(70, 70)), a);
    }

    #[test]
    fn child_origin_and_clip_children() {
        let mut d = desk();
        let f = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(100, 100, 300, 300), 1, 1, 0, 0).unwrap();
        let c = d.create(f, f, 2, ws::VISIBLE, Rect::new(10, 10, 190, 170), 0x8008, 1, 0, 0).unwrap();
        assert_eq!(d.origin(c), Point::new(110, 110));
        assert_eq!(d.visible_region(f, true).area(), 200 * 200 - 180 * 160);
        let destroyed = d.destroy(f);
        assert_eq!(destroyed, alloc::vec![c, f]);
        assert!(!d.exists(c));
    }

    #[test]
    fn invalidation_wakes_the_owning_queue() {
        let mut d = desk();
        let a = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(0, 0, 50, 50), 1, 7, 0, 0).unwrap();
        let b = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(100, 0, 150, 50), 2, 8, 0, 0).unwrap();
        d.take_wake();
        for h in [a, b] {
            d.get_mut(h).unwrap().update = Region::new();
        }
        let g = d.generation;
        // Moving b over a: both repaint, both queues are woken.
        d.set_pos(b, 25, 0, 50, 50);
        assert_ne!(d.generation, g);
        let w = d.take_wake();
        assert!(w.contains(&7) && w.contains(&8));
        assert!(d.take_wake().is_empty());
        d.invalidate(a, None, false);
        assert_eq!(d.take_wake().into_iter().collect::<Vec<_>>(), alloc::vec![7]);
        assert_eq!(d.windows_of(8), alloc::vec![b]);
        d.to_bottom(b);
        assert_eq!(d.window_at(Point::new(30, 10)), a);
    }

    #[test]
    fn pinned_window_stays_at_the_bottom() {
        let mut d = desk();
        let a = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(0, 0, 50, 50), 1, 7, 0, 0).unwrap();
        let desktop = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(0, 0, 640, 480), 2, 7, 0, 0).unwrap();
        let b = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(0, 0, 50, 50), 3, 7, 0, 0).unwrap();
        assert_eq!(d.pin_bottom(desktop), 0);
        assert_eq!(d.get(HWND_DESKTOP).unwrap().children, alloc::vec![desktop, a, b]);
        d.to_top(desktop);
        assert_eq!(d.get(HWND_DESKTOP).unwrap().children[0], desktop);
        d.to_bottom(b);
        assert_eq!(d.get(HWND_DESKTOP).unwrap().children, alloc::vec![desktop, b, a]);
        assert_eq!(d.window_at(Point::new(10, 10)), a);
        assert_eq!(d.window_at(Point::new(300, 300)), desktop);
        assert_eq!(d.pin_bottom(0), desktop);
        d.to_top(desktop);
        assert_eq!(d.window_at(Point::new(10, 10)), desktop);
    }

    #[test]
    fn move_invalidates_uncovered_desktop() {
        let mut d = desk();
        let a = d.create(HWND_DESKTOP, 0, 1, ws::VISIBLE, Rect::new(0, 0, 50, 50), 1, 7, 0, 0).unwrap();
        d.get_mut(a).unwrap().update = Region::new();
        d.set_pos(a, 100, 100, 50, 50);
        assert!(!d.get(HWND_DESKTOP).unwrap().update.is_empty());
        assert_eq!(d.paint_pending(7), Some(a));
    }
}
