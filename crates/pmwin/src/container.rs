//! The container control's geometry and selection logic (WC_CONTAINER;
//! the window procedure is in PMWIN's container.rs): laying out the
//! records for each view, hit testing, keyboard navigation, icon
//! arrangement and the selection rules of the three selection styles.
//!
//! Coordinates are the container's workspace, y up. In the icon view an
//! item is where its record's ptlIcon puts it (the lower-left corner of
//! its icon, its text centred below). The other views are laid out from
//! the workspace's top-left corner (0, 0) downwards (negative y).

use alloc::vec::Vec;

use gpi::Rect;

/// The view (CV_* of flWindowAttr).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Icon,
    Name,
    Text,
    Detail,
    Tree,
}

pub const CV_TEXT: u32 = 0x0001;
pub const CV_NAME: u32 = 0x0002;
pub const CV_ICON: u32 = 0x0004;
pub const CV_DETAIL: u32 = 0x0008;
pub const CV_FLOW: u32 = 0x0010;
pub const CV_MINI: u32 = 0x0020;
pub const CV_TREE: u32 = 0x0040;

impl View {
    pub fn from_attr(fl: u32) -> View {
        if fl & CV_TREE != 0 {
            View::Tree
        } else if fl & CV_DETAIL != 0 {
            View::Detail
        } else if fl & CV_NAME != 0 {
            View::Name
        } else if fl & CV_TEXT != 0 {
            View::Text
        } else {
            View::Icon
        }
    }
}

/// Font and sizes for a layout.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Character cell.
    pub cw: i32,
    pub ch: i32,
    /// Icon size in this view (0: no icons).
    pub icon: i32,
    /// Extra space between rows (cyLineSpacing).
    pub spacing: i32,
    /// Indent per tree level.
    pub indent: i32,
    /// Width of the item area (for CV_FLOW and arranging).
    pub area_w: i32,
    /// Height of the item area.
    pub area_h: i32,
    /// CV_FLOW: the name and text views fill columns.
    pub flow: bool,
    /// The font text is measured in (a proportional font), else
    /// characters times `cw`.
    pub measure: Option<&'static gpi::font::Font>,
}

impl Metrics {
    pub fn text_width(&self, t: &[u8]) -> i32 {
        match self.measure {
            Some(f) => f.text_width(t),
            None => t.len() as i32 * self.cw,
        }
    }
}

/// What the layout needs to know about a record.
#[derive(Clone, Debug, Default)]
pub struct Item {
    /// Text for the view (lines separated by '\n' or '\r\n').
    pub text: Vec<u8>,
    /// Icon view position (ptlIcon).
    pub pos: (i32, i32),
    /// Tree: depth (0 = root) and children.
    pub depth: u32,
    pub has_children: bool,
    pub expanded: bool,
    /// Detail view: the text of each column (icon columns are empty).
    pub cells: Vec<Vec<u8>>,
}

/// A detail view column.
#[derive(Clone, Debug, Default)]
pub struct Column {
    /// Width in pels (0: as wide as its widest content).
    pub width: i32,
    /// The column shows an icon rather than text.
    pub icon: bool,
    pub title: Vec<u8>,
}

/// An item placed in the workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    /// Index into the items passed to the layout.
    pub index: usize,
    /// Everything the item covers.
    pub rect: Rect,
    /// Its icon (empty when none).
    pub icon: Rect,
    /// Its text.
    pub text: Rect,
    /// The text lines (icon view text wraps).
    pub lines: Vec<Vec<u8>>,
    /// Tree: the expand/collapse button (empty when none).
    pub button: Rect,
}

/// Width of icon view text before it wraps.
pub fn icon_text_width(m: &Metrics) -> i32 {
    // At least SV_CXICONTEXTWIDTH (75), whatever the font.
    (m.cw * 10).max(75).max(m.icon + 8)
}

/// Lines of a text: split at line ends, each wrapped at blanks to at most
/// `cols` characters (0: no wrapping).
pub fn lines(t: &[u8], cols: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for raw in t.split(|&c| c == b'\n') {
        let l = raw.strip_suffix(b"\r").unwrap_or(raw);
        if cols == 0 || l.len() <= cols {
            out.push(l.to_vec());
            continue;
        }
        let mut rest = l;
        while rest.len() > cols {
            let cut = rest[..=cols].iter().rposition(|&c| c == b' ').filter(|&i| i > 0).unwrap_or(cols);
            out.push(rest[..cut].to_vec());
            rest = &rest[cut..];
            while rest.first() == Some(&b' ') {
                rest = &rest[1..];
            }
        }
        if !rest.is_empty() {
            out.push(rest.to_vec());
        }
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}

fn text_w(l: &[Vec<u8>], m: &Metrics) -> i32 {
    l.iter().map(|x| m.text_width(x)).max().unwrap_or(0)
}

/// Lines of a text: split at line ends, each wrapped at blanks to at most
/// `width` pels as `m` measures them.
pub fn lines_in(t: &[u8], width: i32, m: &Metrics) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for raw in t.split(|&c| c == b'\n') {
        let mut rest = raw.strip_suffix(b"\r").unwrap_or(raw);
        while rest.len() > 1 && m.text_width(rest) > width {
            // The longest start that fits (at least one character).
            let mut fit = 1;
            while fit < rest.len() && m.text_width(&rest[..fit + 1]) <= width {
                fit += 1;
            }
            let cut = rest[..=fit.min(rest.len() - 1)].iter().rposition(|&c| c == b' ').filter(|&i| i > 0 && i <= fit).unwrap_or(fit);
            out.push(rest[..cut].to_vec());
            rest = &rest[cut..];
            while rest.first() == Some(&b' ') {
                rest = &rest[1..];
            }
        }
        if !rest.is_empty() || out.is_empty() {
            out.push(rest.to_vec());
        }
    }
    out
}

/// The icon view item for `it` at its position.
pub fn icon_item(index: usize, it: &Item, m: &Metrics) -> Placed {
    let (x, y) = it.pos;
    let n = m.icon;
    let icon = if n > 0 { Rect::new(x, y, x + n, y + n) } else { Rect::new(x, y, x, y) };
    let ls = lines_in(&it.text, icon_text_width(m), m);
    let tw = text_w(&ls, m) + 4;
    let th = ls.len() as i32 * m.ch;
    let cx = x + n / 2;
    let top = if n > 0 { y - 1 } else { y + th };
    let text = Rect::new(cx - tw / 2, top - th, cx - tw / 2 + tw, top);
    let rect = Rect::new(icon.left.min(text.left), text.bottom, icon.right.max(text.right), icon.top.max(text.top));
    Placed { index, rect, icon, text, lines: ls, button: Rect::default() }
}

/// Height of one row of the name, text, tree and detail views.
pub fn row_height(m: &Metrics) -> i32 {
    m.ch.max(m.icon) + 2 + m.spacing
}

/// Lay out the items for a view. Returns the placed items (in the order
/// given; for the tree only the visible ones) and the bounds of the
/// workspace they cover.
pub fn layout(view: View, items: &[Item], m: &Metrics, cols: &[Column]) -> (Vec<Placed>, Rect) {
    let mut out = Vec::with_capacity(items.len());
    match view {
        View::Icon => {
            for (i, it) in items.iter().enumerate() {
                out.push(icon_item(i, it, m));
            }
        }
        View::Name | View::Text | View::Tree => {
            let rh = row_height(m);
            let gap = 4;
            let rows_per_col = if m.flow && view != View::Tree { (m.area_h / rh).max(1) as usize } else { usize::MAX };
            let mut col_x = 0;
            let mut col_w = 0;
            let mut row = 0usize;
            // Tree: a child is shown only when every ancestor is expanded.
            let mut hidden_below: Option<u32> = None;
            for (i, it) in items.iter().enumerate() {
                if view == View::Tree {
                    if let Some(d) = hidden_below {
                        if it.depth > d {
                            continue;
                        }
                        hidden_below = None;
                    }
                    if it.has_children && !it.expanded {
                        hidden_below = Some(it.depth);
                    }
                }
                if row >= rows_per_col {
                    row = 0;
                    col_x += col_w + gap * 4;
                    col_w = 0;
                }
                let top = -(row as i32) * rh;
                let indent = if view == View::Tree { it.depth as i32 * m.indent } else { 0 };
                let mut x = col_x + 2 + indent;
                let mut button = Rect::default();
                if view == View::Tree {
                    let b = 9;
                    let by = top - rh / 2 - b / 2 - 1;
                    if it.has_children {
                        button = Rect::new(x, by, x + b, by + b);
                    }
                    x += b + 4;
                }
                let icon = if m.icon > 0 && view != View::Text {
                    let r = Rect::new(x, top - 1 - (rh - m.spacing - 2 + m.icon) / 2, x + m.icon, top - 1 - (rh - m.spacing - 2 - m.icon) / 2);
                    x += m.icon + gap;
                    r
                } else {
                    Rect::new(x, top - rh, x, top - rh)
                };
                let ls = lines(&it.text, 0);
                let first: Vec<Vec<u8>> = ls.into_iter().take(1).collect();
                let tw = text_w(&first, m) + 4;
                let ty = top - 1 - (rh - m.spacing - 2 + m.ch) / 2;
                let text = Rect::new(x, ty, x + tw, ty + m.ch);
                let rect = Rect::new(col_x, top - rh, x + tw, top);
                col_w = col_w.max(rect.right - col_x);
                out.push(Placed { index: i, rect, icon, text, lines: first, button });
                row += 1;
            }
        }
        View::Detail => {
            let rh = row_height(m);
            let widths = column_widths(items, m, cols);
            let total: i32 = widths.iter().sum();
            for (i, it) in items.iter().enumerate() {
                let top = -(i as i32) * rh;
                let rect = Rect::new(0, top - rh, total, top);
                let first = it.cells.iter().find(|c| !c.is_empty()).cloned().unwrap_or_default();
                out.push(Placed { index: i, rect, icon: Rect::default(), text: rect, lines: alloc::vec![first], button: Rect::default() });
            }
        }
    }
    let mut b = out.first().map_or(Rect::default(), |p| p.rect);
    for p in &out {
        b = union(&b, &p.rect);
    }
    if view == View::Detail {
        let total: i32 = column_widths(items, m, cols).iter().sum();
        b = Rect::new(0, b.bottom.min(0), total.max(b.right), 0);
    }
    (out, b)
}

/// Widths of the detail view columns (a column of width 0 is as wide as
/// its title or widest cell).
pub fn column_widths(items: &[Item], m: &Metrics, cols: &[Column]) -> Vec<i32> {
    cols.iter()
        .enumerate()
        .map(|(c, col)| {
            if col.width > 0 {
                return col.width;
            }
            let content = if col.icon {
                m.icon.max(16)
            } else {
                items.iter().map(|it| it.cells.get(c).map_or(0, |t| m.text_width(t))).max().unwrap_or(0)
            };
            content.max(m.text_width(&col.title)) + 12
        })
        .collect()
}

pub fn union(a: &Rect, b: &Rect) -> Rect {
    Rect::new(a.left.min(b.left), a.bottom.min(b.bottom), a.right.max(b.right), a.top.max(b.top))
}

/// The topmost item at workspace point (x, y): the last one placed wins
/// (icon view items later in the z-order are on top).
pub fn hit(placed: &[Placed], x: i32, y: i32, icon_view: bool) -> Option<usize> {
    let p = gpi::Point::new(x, y);
    placed.iter().rev().position(|it| if icon_view { it.icon.contains(p) || it.text.contains(p) } else { it.rect.contains(p) }).map(|i| placed.len() - 1 - i)
}

/// Direction keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// The item next to `from` (index into `placed`) in direction `d`: the
/// nearest one whose centre lies that way, preferring ones in line.
pub fn neighbour(placed: &[Placed], from: usize, d: Dir) -> Option<usize> {
    let c = |r: &Rect| (r.left + r.right, r.bottom + r.top);
    let (fx, fy) = c(&placed.get(from)?.rect);
    let mut best: Option<(i64, usize)> = None;
    for (i, p) in placed.iter().enumerate() {
        if i == from {
            continue;
        }
        let (x, y) = c(&p.rect);
        let (dx, dy) = ((x - fx) as i64, (y - fy) as i64);
        let (along, across) = match d {
            Dir::Up => (dy, dx),
            Dir::Down => (-dy, dx),
            Dir::Left => (-dx, dy),
            Dir::Right => (dx, dy),
        };
        if along <= 0 {
            continue;
        }
        let score = along + across.abs() * 4;
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, i));
        }
    }
    best.map(|b| b.1)
}

/// Positions for arranging `items` in the icon view (CM_ARRANGE): rows
/// from the top-left of the area, left to right.
pub fn arrange(items: &[Item], m: &Metrics) -> Vec<(i32, i32)> {
    let cell_w = icon_text_width(m) + 8;
    let max_lines = items.iter().map(|it| lines_in(&it.text, icon_text_width(m), m).len()).max().unwrap_or(1).min(3) as i32;
    let cell_h = m.icon + 4 + max_lines * m.ch + 8;
    let per_row = ((m.area_w - 4) / cell_w).max(1) as usize;
    (0..items.len())
        .map(|i| {
            let (row, col) = ((i / per_row) as i32, (i % per_row) as i32);
            let x = 4 + col * cell_w + (cell_w - m.icon) / 2;
            let y = m.area_h - 4 - row * cell_h - m.icon;
            (x, y)
        })
        .collect()
}

// ---- Selection ----

pub const CRA_SELECTED: u32 = 0x0001;
pub const CRA_TARGET: u32 = 0x0002;
pub const CRA_CURSORED: u32 = 0x0004;
pub const CRA_INUSE: u32 = 0x0008;
pub const CRA_FILTERED: u32 = 0x0010;
pub const CRA_DISABLED: u32 = 0x1000;
pub const CRA_SOURCE: u32 = 0x4000;

/// Selection style (CCS_SINGLESEL is the default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelMode {
    Single,
    Extended,
    Multiple,
}

impl SelMode {
    pub fn from_style(style: u32) -> SelMode {
        if style & 0x1 != 0 {
            SelMode::Extended
        } else if style & 0x2 != 0 {
            SelMode::Multiple
        } else {
            SelMode::Single
        }
    }
}

/// How an item was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A click (or the cursor moved there): `ctrl` toggles, `shift`
    /// extends from the anchor (extended selection).
    Click { ctrl: bool, shift: bool },
    /// The space bar: toggle the cursored item.
    Toggle,
    /// The cursor moved by keyboard; `shift` extends.
    Move { shift: bool, ctrl: bool },
}

/// The new selected flags of all items (in display order `order`) after
/// picking item `i`, with the anchor of shift-selection. Returns the
/// indices whose selection changed and the new anchor.
pub fn pick(selected: &mut [bool], order: &[usize], i: usize, anchor: Option<usize>, mode: SelMode, how: Pick) -> (Vec<usize>, Option<usize>) {
    let old: Vec<bool> = selected.to_vec();
    let mut anchor = anchor;
    let set_only = |sel: &mut [bool], i: usize| {
        for s in sel.iter_mut() {
            *s = false;
        }
        sel[i] = true;
    };
    match (mode, how) {
        (SelMode::Single, Pick::Toggle) | (SelMode::Single, _) => set_only(selected, i),
        (SelMode::Multiple, Pick::Click { .. }) | (SelMode::Multiple, Pick::Toggle) => selected[i] = !selected[i],
        (SelMode::Multiple, Pick::Move { .. }) => {}
        (SelMode::Extended, Pick::Toggle) => {
            selected[i] = !selected[i];
            anchor = Some(i);
        }
        (SelMode::Extended, Pick::Click { ctrl, shift }) | (SelMode::Extended, Pick::Move { shift, ctrl }) => {
            if shift && anchor.is_some() {
                let pos = |x: usize| order.iter().position(|&o| o == x);
                let (a, b) = (pos(anchor.unwrap()), pos(i));
                if let (Some(a), Some(b)) = (a, b) {
                    if !ctrl {
                        for s in selected.iter_mut() {
                            *s = false;
                        }
                    }
                    for &k in &order[a.min(b)..=a.max(b)] {
                        selected[k] = true;
                    }
                }
            } else if ctrl {
                if matches!(how, Pick::Click { .. }) {
                    selected[i] = !selected[i];
                    anchor = Some(i);
                }
            } else {
                set_only(selected, i);
                anchor = Some(i);
            }
        }
    }
    let changed = (0..selected.len()).filter(|&k| selected[k] != old[k]).collect();
    (changed, anchor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn m() -> Metrics {
        Metrics { cw: 8, ch: 16, icon: 32, spacing: 0, indent: 16, area_w: 400, area_h: 200, flow: false, measure: None }
    }

    fn item(t: &str, pos: (i32, i32)) -> Item {
        Item { text: t.as_bytes().to_vec(), pos, ..Default::default() }
    }

    #[test]
    fn icon_view_places_text_below_icon() {
        let (p, b) = layout(View::Icon, &[item("Hello", (10, 100))], &m(), &[]);
        assert_eq!(p[0].icon, Rect::new(10, 100, 42, 132));
        assert!(p[0].text.top <= 100 && p[0].text.bottom < 100);
        assert_eq!((p[0].text.left + p[0].text.right) / 2, 26);
        assert_eq!(b, p[0].rect);
        // Long text wraps.
        let (p, _) = layout(View::Icon, &[item("A rather long name for an icon", (0, 0))], &m(), &[]);
        assert!(p[0].lines.len() > 1);
        assert!(p[0].text.width() <= icon_text_width(&m()) + 4);
    }

    #[test]
    fn name_view_rows_and_flow() {
        let items: Vec<Item> = (0..30).map(|i| item(if i == 3 { "longer text here" } else { "x" }, (0, 0))).collect();
        let (p, b) = layout(View::Name, &items, &m(), &[]);
        let rh = row_height(&m());
        assert_eq!(p[1].rect.top, -rh);
        assert!(p.iter().all(|x| x.rect.left == 0));
        assert_eq!(b.bottom, -30 * rh);
        let fm = Metrics { flow: true, ..m() };
        let (p, _) = layout(View::Name, &items, &fm, &[]);
        let per = (200 / rh) as usize;
        assert_eq!(p[per].rect.top, 0);
        assert!(p[per].rect.left > p[0].rect.right);
        // Text view has no icons.
        let (p, _) = layout(View::Text, &items, &m(), &[]);
        assert!(p[0].icon.is_empty());
    }

    #[test]
    fn tree_hides_collapsed_children() {
        let mut items = vec![item("root", (0, 0)), item("child", (0, 0)), item("grandchild", (0, 0)), item("root2", (0, 0))];
        items[0].has_children = true;
        items[1].depth = 1;
        items[1].has_children = true;
        items[1].expanded = true;
        items[2].depth = 2;
        let (p, _) = layout(View::Tree, &items, &m(), &[]);
        assert_eq!(p.iter().map(|x| x.index).collect::<Vec<_>>(), vec![0, 3]);
        assert!(!p[0].button.is_empty());
        items[0].expanded = true;
        let (p, _) = layout(View::Tree, &items, &m(), &[]);
        assert_eq!(p.len(), 4);
        assert!(p[2].text.left > p[1].text.left && p[1].text.left > p[0].text.left);
    }

    #[test]
    fn detail_columns() {
        let mut a = item("", (0, 0));
        a.cells = vec![b"file.txt".to_vec(), b"12345".to_vec()];
        let mut b2 = item("", (0, 0));
        b2.cells = vec![b"a".to_vec(), b"1".to_vec()];
        let cols = vec![Column { width: 0, icon: false, title: b"Name".to_vec() }, Column { width: 100, icon: false, title: b"Size".to_vec() }];
        let w = column_widths(&[a.clone(), b2.clone()], &m(), &cols);
        assert_eq!(w, vec![8 * 8 + 12, 100]);
        let (p, b) = layout(View::Detail, &[a, b2], &m(), &cols);
        assert_eq!(p[0].rect.right, 176);
        assert_eq!(b.right, 176);
        assert_eq!(p[1].rect.top, p[0].rect.bottom);
    }

    #[test]
    fn hit_and_navigation() {
        let items: Vec<Item> = (0..6).map(|i| item("n", ((i % 3) * 100, 100 - (i / 3) * 80))).collect();
        let (p, _) = layout(View::Icon, &items, &m(), &[]);
        assert_eq!(hit(&p, 110, 110, true), Some(1));
        assert_eq!(hit(&p, 90, 50, true), None);
        assert_eq!(neighbour(&p, 0, Dir::Right), Some(1));
        assert_eq!(neighbour(&p, 0, Dir::Down), Some(3));
        assert_eq!(neighbour(&p, 4, Dir::Up), Some(1));
        assert_eq!(neighbour(&p, 2, Dir::Right), None);
    }

    #[test]
    fn arrange_rows() {
        let items: Vec<Item> = (0..7).map(|_| item("abc", (0, 0))).collect();
        let pos = arrange(&items, &m());
        assert_eq!(pos[0].1, pos[1].1);
        assert!(pos[1].0 > pos[0].0);
        let per = pos.iter().filter(|p| p.1 == pos[0].1).count();
        assert!(per >= 2 && per < 7);
        assert!(pos[per].1 < pos[0].1);
        assert!(pos[0].1 + m().icon <= m().area_h);
    }

    #[test]
    fn selection_modes() {
        let order: Vec<usize> = (0..5).collect();
        let mut s = vec![false; 5];
        let (c, a) = pick(&mut s, &order, 2, None, SelMode::Single, Pick::Click { ctrl: false, shift: false });
        assert_eq!((c, a), (vec![2], None));
        pick(&mut s, &order, 3, None, SelMode::Single, Pick::Click { ctrl: true, shift: false });
        assert_eq!(s, vec![false, false, false, true, false]);
        // Multiple: clicks toggle.
        let mut s = vec![false; 5];
        pick(&mut s, &order, 1, None, SelMode::Multiple, Pick::Click { ctrl: false, shift: false });
        pick(&mut s, &order, 3, None, SelMode::Multiple, Pick::Click { ctrl: false, shift: false });
        pick(&mut s, &order, 1, None, SelMode::Multiple, Pick::Click { ctrl: false, shift: false });
        assert_eq!(s, vec![false, false, false, true, false]);
        // Extended: click, shift-click range, ctrl-click toggles.
        let mut s = vec![false; 5];
        let (_, a) = pick(&mut s, &order, 1, None, SelMode::Extended, Pick::Click { ctrl: false, shift: false });
        let (_, a) = pick(&mut s, &order, 3, a, SelMode::Extended, Pick::Click { ctrl: false, shift: true });
        assert_eq!(s, vec![false, true, true, true, false]);
        let (_, a) = pick(&mut s, &order, 2, a, SelMode::Extended, Pick::Click { ctrl: true, shift: false });
        assert_eq!(s, vec![false, true, false, true, false]);
        assert_eq!(a, Some(2));
        pick(&mut s, &order, 4, a, SelMode::Extended, Pick::Move { shift: false, ctrl: false });
        assert_eq!(s, vec![false, false, false, false, true]);
    }
}
