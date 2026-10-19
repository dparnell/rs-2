//! The notebook control (WC_NOTEBOOK) as far as it can be tested on the
//! host: the list of pages with their major and minor sections
//! (inserting, deleting, the BKM_QUERYPAGEID/BKM_QUERYPAGECOUNT
//! queries, which tabs show) and the geometry (back pages, tab strips,
//! the page, the status line with the page buttons, hit testing).
//! Coordinates are PM's: origin at the bottom left of the notebook.

use alloc::vec::Vec;

use gpi::{Point, Rect};

// Page order (BKM_INSERTPAGE, BKM_QUERYPAGEID).
pub const BKA_LAST: u16 = 0x0002;
pub const BKA_FIRST: u16 = 0x0004;
pub const BKA_NEXT: u16 = 0x0008;
pub const BKA_PREV: u16 = 0x0010;
pub const BKA_TOP: u16 = 0x0020;
// Deletion (BKM_DELETEPAGE).
pub const BKA_ALL: u16 = 0x0001;
pub const BKA_SINGLE: u16 = 0x0002;
pub const BKA_TAB: u16 = 0x0004;
// Page styles.
pub const BKA_STATUSTEXTON: u16 = 0x0001;
pub const BKA_MAJOR: u16 = 0x0040;
pub const BKA_MINOR: u16 = 0x0080;
pub const BKA_AUTOPAGESIZE: u16 = 0x0100;
pub const BKA_END: u16 = 0x0200;

// Notebook styles.
pub const BKS_BACKPAGESBR: u32 = 0x0001;
pub const BKS_BACKPAGESBL: u32 = 0x0002;
pub const BKS_BACKPAGESTR: u32 = 0x0004;
pub const BKS_BACKPAGESTL: u32 = 0x0008;
pub const BKS_MAJORTABRIGHT: u32 = 0x0010;
pub const BKS_MAJORTABLEFT: u32 = 0x0020;
pub const BKS_MAJORTABTOP: u32 = 0x0040;
pub const BKS_MAJORTABBOTTOM: u32 = 0x0080;
pub const BKS_ROUNDEDTABS: u32 = 0x0100;
pub const BKS_POLYGONTABS: u32 = 0x0200;
pub const BKS_SPIRALBIND: u32 = 0x0400;
pub const BKS_TABBEDDIALOG: u32 = 0x0800;
pub const BKS_STATUSTEXTRIGHT: u32 = 0x1000;
pub const BKS_STATUSTEXTCENTER: u32 = 0x2000;
pub const BKS_TABTEXTRIGHT: u32 = 0x4000;
pub const BKS_TABTEXTCENTER: u32 = 0x8000;

/// A page loaded from a dialog (BKM_SETPAGEINFO with
/// BFA_PAGEFROMDLGRES or BFA_PAGEFROMDLGTEMPLATE) when it is turned to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageDlg {
    pub proc_: u32,
    pub id: u32,
    pub hmod: u32,
    pub params: u32,
    /// DLGTEMPLATE address, 0 for the resource.
    pub template: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub id: u32,
    pub style: u16,
    pub data: u32,
    pub hwnd: u32,
    /// Tab text as set (with its ~ mnemonic mark).
    pub tab: Vec<u8>,
    pub status: Vec<u8>,
    /// Tab bitmap handle and a copy of its pixels (rows bottom up),
    /// so that any process can paint it.
    pub bitmap: u32,
    pub bits: Vec<u32>,
    pub bits_size: (i32, i32),
    /// BKM_SETTABCOLOR (RGB).
    pub tab_color: Option<u32>,
    pub dlg: Option<PageDlg>,
}

impl Page {
    pub fn major(&self) -> bool {
        self.style & BKA_MAJOR != 0
    }

    pub fn minor(&self) -> bool {
        self.style & BKA_MINOR != 0 && !self.major()
    }
}

/// The pages of a notebook in order, and which one is on top.
#[derive(Clone, Debug)]
pub struct Book {
    pub pages: Vec<Page>,
    /// The top page's id (0: none).
    pub top: u32,
    next_id: u32,
}

impl Default for Book {
    fn default() -> Book {
        Book::new()
    }
}

impl Book {
    pub fn new() -> Book {
        Book { pages: Vec::new(), top: 0, next_id: 1 }
    }

    pub fn index(&self, id: u32) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.pages.iter().position(|p| p.id == id)
    }

    pub fn page(&self, id: u32) -> Option<&Page> {
        self.index(id).map(|i| &self.pages[i])
    }

    pub fn page_mut(&mut self, id: u32) -> Option<&mut Page> {
        let i = self.index(id)?;
        Some(&mut self.pages[i])
    }

    pub fn top_index(&self) -> Option<usize> {
        self.index(self.top)
    }

    /// Insert a page (BKM_INSERTPAGE): its id, or None for a bad
    /// reference page or order. The first page becomes the top page.
    pub fn insert(&mut self, reference: u32, style: u16, order: u16) -> Option<u32> {
        let at = match order {
            BKA_FIRST => 0,
            BKA_LAST => self.pages.len(),
            BKA_NEXT => self.index(reference)? + 1,
            BKA_PREV => self.index(reference)?,
            _ => return None,
        };
        let mut id = self.next_id;
        while id == 0 || self.index(id).is_some() {
            id = id.wrapping_add(1);
        }
        self.next_id = id.wrapping_add(1);
        self.pages.insert(at, Page { id, style, ..Page::default() });
        if self.top == 0 {
            self.top = id;
        }
        Some(id)
    }

    /// The end (exclusive) of the section that starts at page `i`: up to
    /// the next major page for a major page, the next tab of either kind
    /// otherwise.
    pub fn section_end(&self, i: usize) -> usize {
        let major = self.pages[i].major();
        (i + 1..self.pages.len())
            .find(|&j| {
                let p = &self.pages[j];
                p.major() || (!major && p.minor())
            })
            .unwrap_or(self.pages.len())
    }

    /// Delete pages (BKM_DELETEPAGE): the removed pages, or None for a bad
    /// page id or flag. A deleted top page passes the top to the page
    /// after it (or before it).
    pub fn delete(&mut self, id: u32, flag: u16) -> Option<Vec<Page>> {
        let (from, to) = match flag {
            BKA_ALL => (0, self.pages.len()),
            BKA_SINGLE => {
                let i = self.index(id)?;
                (i, i + 1)
            }
            BKA_TAB => {
                let i = self.index(id)?;
                (i, self.section_end(i))
            }
            _ => return None,
        };
        let top_gone = self.top_index().is_some_and(|t| t >= from && t < to);
        let removed: Vec<Page> = self.pages.drain(from..to).collect();
        if top_gone || self.pages.is_empty() {
            self.top = self.pages.get(from).or_else(|| from.checked_sub(1).and_then(|i| self.pages.get(i))).map_or(0, |p| p.id);
        }
        Some(removed)
    }

    /// BKM_QUERYPAGEID: Ok(0) when there is no such page, Err for a bad
    /// reference page or order.
    pub fn query_id(&self, reference: u32, order: u16, style: u16) -> Result<u32, ()> {
        let ok = |p: &Page| style & (BKA_MAJOR | BKA_MINOR) == 0 || p.style & style & (BKA_MAJOR | BKA_MINOR) != 0;
        let found = match order {
            BKA_FIRST => self.pages.iter().find(|p| ok(p)),
            BKA_LAST => self.pages.iter().rev().find(|p| ok(p)),
            BKA_TOP => return Ok(self.top),
            BKA_NEXT => {
                let i = self.index(reference).ok_or(())?;
                self.pages[i + 1..].iter().find(|p| ok(p))
            }
            BKA_PREV => {
                let i = self.index(reference).ok_or(())?;
                self.pages[..i].iter().rev().find(|p| ok(p))
            }
            _ => return Err(()),
        };
        Ok(found.map_or(0, |p| p.id))
    }

    /// BKM_QUERYPAGECOUNT: the pages from `id` (0: the first page) up to
    /// the next major tab, the next tab of either kind (BKA_MINOR) or the
    /// end of the book (BKA_END, or 0).
    pub fn count(&self, id: u32, end: u16) -> Result<u32, ()> {
        let start = if id == 0 { 0 } else { self.index(id).ok_or(())? };
        if start >= self.pages.len() {
            return Ok(0);
        }
        let stop = |p: &Page| match end {
            BKA_MAJOR => p.major(),
            BKA_MINOR => p.major() || p.minor(),
            _ => false,
        };
        let n = 1 + self.pages[start + 1..].iter().take_while(|p| !stop(p)).count();
        Ok(n as u32)
    }

    /// The major page at or before page `i`.
    pub fn major_of(&self, i: usize) -> Option<usize> {
        (0..=i.min(self.pages.len().saturating_sub(1))).rev().find(|&j| self.pages.get(j).is_some_and(|p| p.major()))
    }

    /// The minor page at or before page `i` within its major section.
    pub fn minor_of(&self, i: usize) -> Option<usize> {
        for j in (0..=i.min(self.pages.len().saturating_sub(1))).rev() {
            let p = &self.pages[j];
            if p.minor() {
                return Some(j);
            }
            if p.major() {
                return None;
            }
        }
        None
    }

    /// The pages (indices) with major tabs.
    pub fn major_tabs(&self) -> Vec<usize> {
        (0..self.pages.len()).filter(|&i| self.pages[i].major()).collect()
    }

    /// The minor tabs shown: those of the top page's major section.
    pub fn minor_tabs(&self) -> Vec<usize> {
        let Some(t) = self.top_index() else { return Vec::new() };
        let start = self.major_of(t).map_or(0, |m| m + 1);
        (start..self.pages.len()).take_while(|&i| !self.pages[i].major()).filter(|&i| self.pages[i].minor()).collect()
    }

    /// Whether any page has a minor tab (the notebook keeps room for
    /// minor tabs then).
    pub fn has_minor(&self) -> bool {
        self.pages.iter().any(|p| p.minor())
    }

    /// The tab that shows the top page: its minor tab, else its major tab.
    pub fn current_tab(&self) -> Option<usize> {
        let t = self.top_index()?;
        self.minor_of(t).or_else(|| self.major_of(t))
    }

    /// The page of the previous (`dir` < 0) or next tab from the current
    /// one: major tabs and the minor tabs of the current section.
    pub fn tab_step(&self, dir: i32) -> Option<u32> {
        let t = self.top_index()?;
        let cur = self.current_tab().unwrap_or(t);
        let section = self.major_of(cur);
        let shown = |j: usize| {
            let p = &self.pages[j];
            p.major() || (p.minor() && self.major_of(j) == section)
        };
        let j = if dir < 0 { (0..cur).rev().find(|&j| shown(j)) } else { (cur + 1..self.pages.len()).find(|&j| shown(j)) };
        j.map(|j| self.pages[j].id)
    }

    /// The page before or after the top page.
    pub fn step(&self, dir: i32) -> Option<u32> {
        let t = self.top_index()? as i32 + dir.signum();
        (t >= 0).then(|| self.pages.get(t as usize).map(|p| p.id)).flatten()
    }

    /// The page whose shown tab has mnemonic `c` (lower case).
    pub fn mnemonic(&self, c: u8) -> Option<u32> {
        let mut tabs = self.major_tabs();
        tabs.extend(self.minor_tabs());
        tabs.into_iter().find(|&i| mnemonic_of(&self.pages[i].tab) == Some(c)).map(|i| self.pages[i].id)
    }
}

/// The character after the first single `~` of a text, lower case.
pub fn mnemonic_of(t: &[u8]) -> Option<u8> {
    let mut i = 0;
    while i + 1 < t.len() {
        if t[i] == b'~' {
            if t[i + 1] == b'~' {
                i += 2;
                continue;
            }
            return Some(t[i + 1].to_ascii_lowercase());
        }
        i += 1;
    }
    None
}

// ---- Geometry ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn vertical(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// Back pages fanned out by this many pels per page, this many pages.
pub const BACK_STEP: i32 = 3;
pub const BACK_PAGES: i32 = 3;
/// Default page button size.
pub const BUTTON: (i32, i32) = (21, 21);

/// What the layout depends on.
#[derive(Clone, Debug)]
pub struct Geometry {
    pub w: i32,
    pub h: i32,
    pub style: u32,
    /// BKM_SETDIMENSIONS for major and minor tabs and the page buttons;
    /// (0, 0) for the default.
    pub major: (i32, i32),
    pub minor: (i32, i32),
    pub button: (i32, i32),
    pub char_w: i32,
    pub char_h: i32,
    /// Keep room for minor tabs.
    pub has_minor: bool,
    /// Widest major and minor tab text (pels).
    pub major_text: i32,
    pub minor_text: i32,
    /// Height of the area for notebook buttons at the bottom (0: none).
    pub button_area: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub major_side: Side,
    pub minor_side: Side,
    pub tabbed: bool,
    /// The top page's outline.
    pub book: Rect,
    /// Back page outlines, the farthest first.
    pub backs: Vec<Rect>,
    /// Where the page window goes.
    pub page: Rect,
    /// The status line, with the page buttons at its right end.
    pub status: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub major_strip: Rect,
    pub minor_strip: Rect,
    pub button_area: Rect,
    /// Major and minor tab size: across the strip, along it (fixed
    /// length, or 0 when each tab is as long as its text).
    pub major_tab: (i32, i32),
    pub minor_tab: (i32, i32),
}

/// What a point of the notebook is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A tab: the page index.
    MajorTab(usize),
    MinorTab(usize),
    Prev,
    Next,
    Page,
    Nothing,
}

impl Geometry {
    /// Major side, minor side and the two back page sides (vertical,
    /// horizontal) of a style.
    pub fn sides(style: u32) -> (Side, Side, Side, Side) {
        let tabbed = style & BKS_TABBEDDIALOG != 0;
        let (bv, bh) = if style & BKS_BACKPAGESTL != 0 {
            (Side::Left, Side::Top)
        } else if style & BKS_BACKPAGESTR != 0 {
            (Side::Right, Side::Top)
        } else if style & BKS_BACKPAGESBL != 0 {
            (Side::Left, Side::Bottom)
        } else {
            (Side::Right, Side::Bottom)
        };
        let major = if style & BKS_MAJORTABTOP != 0 || (tabbed && style & (BKS_MAJORTABLEFT | BKS_MAJORTABRIGHT | BKS_MAJORTABBOTTOM) == 0) {
            Side::Top
        } else if style & BKS_MAJORTABBOTTOM != 0 {
            Side::Bottom
        } else if style & BKS_MAJORTABLEFT != 0 {
            Side::Left
        } else {
            Side::Right
        };
        let minor = if major.vertical() { bh } else { bv };
        (major, minor, bv, bh)
    }

    fn tab_size(&self, set: (i32, i32), text: i32, side: Side) -> (i32, i32) {
        let along_default = self.char_h + 10;
        if side.vertical() {
            let across = if set.0 > 0 { set.0 } else { (text + 16).clamp(40, 160) };
            let along = if set.1 > 0 { set.1 } else { along_default };
            (across, along)
        } else {
            let across = if set.1 > 0 { set.1 } else { along_default };
            (across, set.0.max(0))
        }
    }

    pub fn layout(&self) -> Layout {
        let (major_side, minor_side, bv, bh) = Geometry::sides(self.style);
        let tabbed = self.style & BKS_TABBEDDIALOG != 0;
        let major_tab = self.tab_size(self.major, self.major_text, major_side);
        let minor_tab = self.tab_size(self.minor, self.minor_text, minor_side);
        let depth = if tabbed { 0 } else { BACK_STEP * BACK_PAGES };
        // Margins around the top page: left, right, bottom, top.
        let mut m = [0i32; 4];
        let slot = |s: Side| match s {
            Side::Left => 0,
            Side::Right => 1,
            Side::Bottom => 2,
            Side::Top => 3,
        };
        m[2] += self.button_area;
        let back_at = [depth * (bv == Side::Left) as i32, depth * (bv == Side::Right) as i32, depth * (bh == Side::Bottom) as i32, depth * (bh == Side::Top) as i32];
        for i in 0..4 {
            m[i] += back_at[i];
        }
        m[slot(major_side)] += major_tab.0;
        let minor_on = self.has_minor && !tabbed;
        if minor_on {
            m[slot(minor_side)] += minor_tab.0;
        }
        let book = Rect::new(m[0], m[2], (self.w - m[1]).max(m[0]), (self.h - m[3]).max(m[2]));
        let backs = if tabbed {
            Vec::new()
        } else {
            let (dx, dy) = (if bv == Side::Right { BACK_STEP } else { -BACK_STEP }, if bh == Side::Top { BACK_STEP } else { -BACK_STEP });
            (1..=BACK_PAGES).rev().map(|k| book.offset(dx * k, dy * k)).collect()
        };
        // The strips: outside the back pages on their side.
        let strip = |side: Side, across: i32| -> Rect {
            let off = back_at[slot(side)];
            match side {
                Side::Right => Rect::new(book.right + off, book.bottom, book.right + off + across, book.top - 6),
                Side::Left => Rect::new(book.left - off - across, book.bottom, book.left - off, book.top - 6),
                Side::Top => Rect::new(book.left + 6, book.top + off, book.right, book.top + off + across),
                Side::Bottom => Rect::new(book.left + 6, book.bottom - off - across, book.right, book.bottom - off),
            }
        };
        let major_strip = strip(major_side, major_tab.0);
        let mut minor_strip = if minor_on { strip(minor_side, minor_tab.0) } else { Rect::new(0, 0, 0, 0) };
        // Tabs of a tabbed dialog start at the page's corner.
        let major_strip = if tabbed { Rect::new(book.left + 2, major_strip.bottom, major_strip.right, major_strip.top) } else { major_strip };
        if minor_on && !minor_side.vertical() && minor_strip.right > minor_strip.left {
            minor_strip.left = book.left + 6;
        }
        let inner = Rect::new(book.left + 2, book.bottom + 2, book.right - 2, book.top - 2);
        let (bw, bhh) = (if self.button.0 > 0 { self.button.0 } else { BUTTON.0 }, if self.button.1 > 0 { self.button.1 } else { BUTTON.1 });
        let sh = (self.char_h + 6).max(bhh + 4);
        let status = Rect::new(inner.left, inner.bottom, inner.right, (inner.bottom + sh).min(inner.top));
        let by = status.bottom + (status.height() - bhh) / 2;
        let next = Rect::new(status.right - 2 - bw, by, status.right - 2, by + bhh);
        let prev = Rect::new(next.left - bw, by, next.left, by + bhh);
        let page = Rect::new(inner.left + 2, status.top + 1, (inner.right - 2).max(inner.left + 2), (inner.top - 2).max(status.top + 1));
        let button_area = Rect::new(0, 0, self.w, self.button_area);
        Layout { major_side, minor_side, tabbed, book, backs, page, status, prev, next, major_strip, minor_strip, button_area, major_tab, minor_tab }
    }
}

impl Layout {
    /// Rectangles of the tabs that fit in a strip: `lens` are the tab
    /// lengths along it (0: the strip's fixed length), `sel` the selected
    /// one; `first` is the first tab shown, moved so that `sel` shows.
    /// Returns (tab number, rectangle).
    pub fn tabs(&self, minor: bool, lens: &[i32], sel: Option<usize>, first: &mut usize) -> Vec<(usize, Rect)> {
        let (side, strip, (across, fixed)) = if minor { (self.minor_side, self.minor_strip, self.minor_tab) } else { (self.major_side, self.major_strip, self.major_tab) };
        let _ = across;
        if lens.is_empty() || strip.width() <= 0 || strip.height() <= 0 {
            *first = 0;
            return Vec::new();
        }
        let avail = if side.vertical() { strip.height() } else { strip.width() };
        let len = |i: usize| if fixed > 0 { fixed } else { lens[i].max(8) };
        let fits = |from: usize| {
            let mut used = 0;
            let mut n = 0;
            for i in from..lens.len() {
                used += len(i);
                if used > avail && n > 0 {
                    break;
                }
                n += 1;
            }
            n
        };
        *first = (*first).min(lens.len() - 1);
        if let Some(s) = sel {
            if s < *first {
                *first = s;
            }
            while s >= *first + fits(*first) && *first < s {
                *first += 1;
            }
        }
        // Fill the strip from the end when tabs before `first` would fit.
        while *first > 0 && fits(*first - 1) > fits(*first) {
            *first -= 1;
        }
        let n = fits(*first);
        let mut out = Vec::with_capacity(n);
        let mut at = 0;
        for i in *first..*first + n {
            let l = len(i);
            let r = if side.vertical() { Rect::new(strip.left, strip.top - at - l, strip.right, strip.top - at) } else { Rect::new(strip.left + at, strip.bottom, strip.left + at + l, strip.top) };
            out.push((i, r));
            at += l;
        }
        out
    }

    /// A tab's rectangle when it is the selected one: reaching over the
    /// back pages to the top page.
    pub fn selected(&self, minor: bool, r: Rect) -> Rect {
        let side = if minor { self.minor_side } else { self.major_side };
        let reach = match side {
            Side::Right => r.left - self.book.right,
            Side::Left => self.book.left - r.right,
            Side::Top => r.bottom - self.book.top,
            Side::Bottom => self.book.bottom - r.top,
        }
        .max(0)
            + 1;
        match side {
            Side::Right => Rect::new(r.left - reach, r.bottom, r.right, r.top),
            Side::Left => Rect::new(r.left, r.bottom, r.right + reach, r.top),
            Side::Top => Rect::new(r.left, r.bottom - reach, r.right, r.top),
            Side::Bottom => Rect::new(r.left, r.bottom, r.right, r.top + reach),
        }
    }

    /// What is at `p`: `major` and `minor` are the shown tabs as
    /// (page index, rectangle).
    pub fn hit(&self, p: Point, major: &[(usize, Rect)], minor: &[(usize, Rect)]) -> Hit {
        if let Some(&(i, _)) = major.iter().find(|(_, r)| r.contains(p)) {
            return Hit::MajorTab(i);
        }
        if let Some(&(i, _)) = minor.iter().find(|(_, r)| r.contains(p)) {
            return Hit::MinorTab(i);
        }
        if self.prev.contains(p) {
            return Hit::Prev;
        }
        if self.next.contains(p) {
            return Hit::Next;
        }
        if self.book.contains(p) {
            return Hit::Page;
        }
        Hit::Nothing
    }

    /// Margins of the page within the notebook window (left, bottom,
    /// right, top), for BKM_CALCPAGERECT.
    pub fn margins(&self, w: i32, h: i32) -> (i32, i32, i32, i32) {
        (self.page.left, self.page.bottom, w - self.page.right, h - self.page.top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(styles: &[u16]) -> (Book, Vec<u32>) {
        let mut b = Book::new();
        let ids = styles.iter().map(|&s| b.insert(0, s, BKA_LAST).unwrap()).collect();
        (b, ids)
    }

    #[test]
    fn insert_order() {
        let mut b = Book::new();
        let a = b.insert(0, BKA_MAJOR, BKA_LAST).unwrap();
        assert_eq!(b.top, a, "the first page goes on top");
        let c = b.insert(0, 0, BKA_LAST).unwrap();
        let first = b.insert(0, 0, BKA_FIRST).unwrap();
        let mid = b.insert(a, 0, BKA_NEXT).unwrap();
        let before = b.insert(c, 0, BKA_PREV).unwrap();
        let order: Vec<u32> = b.pages.iter().map(|p| p.id).collect();
        assert_eq!(order, [first, a, mid, before, c]);
        assert_eq!(b.insert(999, 0, BKA_NEXT), None);
        assert_eq!(b.insert(0, 0, 0x40), None);
        assert_eq!(b.top, a);
    }

    #[test]
    fn delete_and_top() {
        let (mut b, ids) = book(&[BKA_MAJOR, BKA_MINOR, 0, BKA_MINOR, BKA_MAJOR, 0]);
        b.top = ids[2];
        let gone = b.delete(ids[1], BKA_TAB).unwrap();
        assert_eq!(gone.iter().map(|p| p.id).collect::<Vec<_>>(), [ids[1], ids[2]]);
        assert_eq!(b.top, ids[3], "the top passes to the page after");
        let gone = b.delete(ids[0], BKA_TAB).unwrap();
        assert_eq!(gone.len(), 2, "a major section: up to the next major page");
        assert_eq!(b.top, ids[4]);
        b.delete(ids[5], BKA_SINGLE).unwrap();
        assert_eq!(b.top, ids[4]);
        b.top = ids[4];
        b.delete(ids[4], BKA_SINGLE).unwrap();
        assert_eq!(b.top, 0);
        assert!(b.delete(ids[4], BKA_SINGLE).is_none());
        let (mut b, ids) = book(&[0, 0, 0]);
        b.top = ids[2];
        b.delete(ids[2], BKA_SINGLE);
        assert_eq!(b.top, ids[1], "or the page before");
        assert_eq!(b.delete(0, BKA_ALL).unwrap().len(), 2);
        assert_eq!(b.top, 0);
    }

    #[test]
    fn queries() {
        let (b, ids) = book(&[BKA_MAJOR, BKA_MINOR, 0, BKA_MAJOR, 0]);
        assert_eq!(b.query_id(0, BKA_FIRST, 0), Ok(ids[0]));
        assert_eq!(b.query_id(0, BKA_LAST, 0), Ok(ids[4]));
        assert_eq!(b.query_id(0, BKA_LAST, BKA_MAJOR), Ok(ids[3]));
        assert_eq!(b.query_id(ids[0], BKA_NEXT, 0), Ok(ids[1]));
        assert_eq!(b.query_id(ids[0], BKA_NEXT, BKA_MAJOR), Ok(ids[3]));
        assert_eq!(b.query_id(ids[4], BKA_PREV, BKA_MINOR), Ok(ids[1]));
        assert_eq!(b.query_id(ids[4], BKA_NEXT, 0), Ok(0));
        assert_eq!(b.query_id(0, BKA_TOP, 0), Ok(ids[0]));
        assert_eq!(b.query_id(77, BKA_NEXT, 0), Err(()));
        assert_eq!(b.count(0, BKA_END), Ok(5));
        assert_eq!(b.count(ids[0], BKA_MAJOR), Ok(3));
        assert_eq!(b.count(ids[1], BKA_MINOR), Ok(2));
        assert_eq!(b.count(ids[3], BKA_END), Ok(2));
        assert_eq!(b.count(55, BKA_END), Err(()));
    }

    #[test]
    fn sections_and_tabs() {
        let (mut b, ids) = book(&[BKA_MAJOR, BKA_MINOR, 0, BKA_MINOR, BKA_MAJOR, BKA_MINOR]);
        b.top = ids[2];
        assert_eq!(b.major_tabs(), [0, 4]);
        assert_eq!(b.minor_tabs(), [1, 3]);
        assert_eq!(b.current_tab(), Some(1));
        assert_eq!(b.tab_step(1), Some(ids[3]));
        assert_eq!(b.tab_step(-1), Some(ids[0]));
        b.top = ids[5];
        assert_eq!(b.minor_tabs(), [5]);
        assert_eq!(b.tab_step(-1), Some(ids[4]));
        assert_eq!(b.step(1), None);
        assert_eq!(b.step(-1), Some(ids[4]));
        b.pages[4].tab = b"~Two".to_vec();
        b.pages[3].tab = b"~x".to_vec();
        assert_eq!(b.mnemonic(b't'), Some(ids[4]));
        assert_eq!(b.mnemonic(b'x'), None, "a minor tab of another section");
        assert_eq!(mnemonic_of(b"a~~b~c"), Some(b'c'));
    }

    fn geo(style: u32) -> Geometry {
        Geometry { w: 440, h: 324, style, major: (0, 0), minor: (0, 0), button: (0, 0), char_w: 8, char_h: 16, has_minor: false, major_text: 16, minor_text: 0, button_area: 0 }
    }

    #[test]
    fn layout_right_tabs() {
        let l = geo(BKS_BACKPAGESBR | BKS_MAJORTABRIGHT).layout();
        assert_eq!(l.major_side, Side::Right);
        assert_eq!(l.major_tab, (40, 26));
        assert_eq!(l.book, Rect::new(0, 9, 440 - 49, 324));
        assert_eq!(l.backs.len(), 3);
        assert_eq!(l.backs[0], l.book.offset(9, -9));
        assert_eq!(l.major_strip, Rect::new(400, 9, 440, 318));
        assert!(l.page.left > l.book.left && l.page.right < l.book.right && l.page.bottom > l.status.top - 1);
        assert!(l.next.right <= l.status.right && l.prev.right == l.next.left);
        let mut first = 0;
        let tabs = l.tabs(false, &[16, 16], Some(1), &mut first);
        assert_eq!(tabs, [(0, Rect::new(400, 292, 440, 318)), (1, Rect::new(400, 266, 440, 292))]);
        assert_eq!(l.hit(Point::new(420, 280), &tabs, &[]), Hit::MajorTab(1));
        assert_eq!(l.hit(Point::new(l.next.left + 1, l.next.bottom + 1), &tabs, &[]), Hit::Next);
        assert_eq!(l.hit(Point::new(100, 100), &tabs, &[]), Hit::Page);
        assert_eq!(l.hit(Point::new(439, 2), &tabs, &[]), Hit::Nothing);
        assert_eq!(l.selected(false, tabs[0].1).left, l.book.right - 1);
        let (ml, mb, mr, mt) = l.margins(440, 324);
        assert_eq!((l.page.left, l.page.bottom), (ml, mb));
        assert_eq!((440 - l.page.right, 324 - l.page.top), (mr, mt));
    }

    #[test]
    fn layout_sides() {
        let mut g = geo(BKS_BACKPAGESTL | BKS_MAJORTABLEFT);
        g.has_minor = true;
        let l = g.layout();
        assert_eq!((l.major_side, l.minor_side), (Side::Left, Side::Top));
        assert_eq!(l.book.left, 49);
        assert_eq!(l.book.top, 324 - 9 - 26);
        assert!(l.minor_strip.bottom >= l.book.top + 9);
        let l = geo(BKS_BACKPAGESBR | BKS_MAJORTABBOTTOM).layout();
        assert_eq!((l.major_side, l.minor_side), (Side::Bottom, Side::Right));
        assert_eq!(l.book.bottom, 9 + 26);
        let l = geo(BKS_TABBEDDIALOG).layout();
        assert_eq!(l.major_side, Side::Top);
        assert!(l.backs.is_empty());
        assert_eq!(l.book, Rect::new(0, 0, 440, 324 - 26));
        // Tabs as long as their text, scrolled to show the selected one.
        let mut first = 0;
        let lens = [100, 100, 100, 100, 100, 100];
        let tabs = l.tabs(false, &lens, Some(5), &mut first);
        assert_eq!(tabs.last().unwrap().0, 5);
        assert_eq!(first, 2);
        assert_eq!(tabs[0].1, Rect::new(2, 298, 102, 324));
        let tabs = l.tabs(false, &lens, Some(0), &mut first);
        assert_eq!((first, tabs.len()), (0, 4));
    }
}
