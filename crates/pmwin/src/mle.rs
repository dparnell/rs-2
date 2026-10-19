//! The text engine of the multi-line entry field (WC_MLE): the text, the
//! insertion point and selection, line layout with word wrap and tab
//! stops, one level of undo, import and export in the MLFIE_* formats and
//! searching. Fonts are monospaced (all rs2 fonts are the system font,
//! scaled), so layout works with a character width in pels.
//!
//! The text keeps line breaks as one LF byte, so an insertion point (IPT)
//! is a byte offset and a line break counts as one character, as in the
//! OS/2 MLE. Export formats add CR where they need it.

use alloc::vec::Vec;

/// Import/export formats (MLFIE_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// MLFIE_CFTEXT: lines end with CR LF.
    CfText,
    /// MLFIE_NOTRANS: lines end with LF.
    NoTrans,
    /// MLFIE_WINFMT: CR LF ends lines, CR CR LF marks a soft (wrap) break.
    WinFmt,
}

impl Format {
    pub fn from_mlfie(v: u32) -> Format {
        match v {
            1 => Format::NoTrans,
            2 => Format::WinFmt,
            _ => Format::CfText,
        }
    }

    pub fn mlfie(self) -> u32 {
        match self {
            Format::CfText => 0,
            Format::NoTrans => 1,
            Format::WinFmt => 2,
        }
    }
}

/// A display line: text[start..end], followed by a line break when
/// `hard` (the break itself is at `end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub hard: bool,
}

/// What an undo puts back: text[at..at + inserted.len()] was `removed`.
#[derive(Clone, Debug)]
struct Undo {
    at: usize,
    removed: Vec<u8>,
    inserted: Vec<u8>,
    /// Selection before the change (anchor, cursor).
    sel: (usize, usize),
    /// The operation (a message number) for MLM_QUERYUNDO.
    op: u32,
    /// Typing may extend this record.
    open: bool,
}

/// Result of an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Done,
    /// Nothing to do (empty change).
    Nothing,
    /// Read-only.
    ReadOnly,
    /// The text limit would be exceeded by this many bytes.
    Overflow(usize),
}

pub struct Mle {
    text: Vec<u8>,
    pub anchor: usize,
    pub cursor: usize,
    /// Text limit in characters (None: no limit).
    pub limit: Option<usize>,
    pub wrap: bool,
    /// Width available for text (pels; wrapping happens at it).
    width: i32,
    /// Character cell width (pels).
    cw: i32,
    /// Tab stop interval (pels).
    tab: i32,
    lines: Vec<Line>,
    pub changed: bool,
    pub readonly: bool,
    pub undo_enabled: bool,
    undo: Option<Undo>,
    pub format: Format,
    /// Insert (true) or overtype mode.
    pub insert: bool,
    /// First display line shown and the horizontal scroll offset (pels).
    pub first_line: usize,
    pub x_offset: i32,
    /// Pel column kept while moving up and down.
    pub goal_x: Option<i32>,
}

pub const OP_TYPING: u32 = 0x007a;

impl Default for Mle {
    fn default() -> Mle {
        Mle::new()
    }
}

impl Mle {
    pub fn new() -> Mle {
        let mut m = Mle {
            text: Vec::new(),
            anchor: 0,
            cursor: 0,
            limit: None,
            wrap: false,
            width: 0,
            cw: 8,
            tab: 64,
            lines: Vec::new(),
            changed: false,
            readonly: false,
            undo_enabled: true,
            undo: None,
            format: Format::CfText,
            insert: true,
            first_line: 0,
            x_offset: 0,
            goal_x: None,
        };
        m.relayout();
        m
    }

    // ---- Text and layout parameters ----

    pub fn text(&self) -> &[u8] {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replace everything (no undo); the cursor goes to the start.
    pub fn set_text(&mut self, t: &[u8]) {
        self.text = t.to_vec();
        if let Some(l) = self.limit {
            self.text.truncate(l);
        }
        self.anchor = 0;
        self.cursor = 0;
        self.undo = None;
        self.first_line = 0;
        self.x_offset = 0;
        self.relayout();
    }

    pub fn char_width(&self) -> i32 {
        self.cw
    }

    pub fn tab_stop(&self) -> i32 {
        self.tab
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    /// Change the layout parameters (format width, cell width, tab stop
    /// interval, wrap) and lay the text out again if they changed.
    pub fn set_layout(&mut self, width: i32, cw: i32, tab: i32, wrap: bool) {
        let (cw, tab) = (cw.max(1), tab.max(1));
        if (width, cw, tab, wrap) != (self.width, self.cw, self.tab, self.wrap) {
            (self.width, self.cw, self.tab, self.wrap) = (width, cw, tab, wrap);
            self.relayout();
        }
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Advance of byte `b` at pel position `x`.
    fn advance(&self, b: u8, x: i32) -> i32 {
        if b == b'\t' { (x / self.tab + 1) * self.tab - x } else { self.cw }
    }

    /// Lay out the paragraph (hard line) text[p0..p1] (p1 at its LF or the
    /// end of the text) into display lines.
    fn layout_para(&self, p0: usize, p1: usize, hard: bool, out: &mut Vec<Line>) {
        if !self.wrap || self.width <= 0 {
            out.push(Line { start: p0, end: p1, hard });
            return;
        }
        let mut start = p0;
        loop {
            let mut x = 0;
            let mut i = start;
            let mut space = None;
            while i < p1 {
                let w = self.advance(self.text[i], x);
                if x + w > self.width && i > start {
                    break;
                }
                if self.text[i] == b' ' || self.text[i] == b'\t' {
                    space = Some(i);
                }
                x += w;
                i += 1;
            }
            if i >= p1 {
                out.push(Line { start, end: p1, hard });
                return;
            }
            // Break after the last blank on the line, else in the word.
            let mut end = match space {
                Some(s) if s >= start => s + 1,
                _ => i,
            };
            // Blanks that follow stay on this line.
            while end < p1 && self.text[end] == b' ' {
                end += 1;
            }
            if end >= p1 {
                out.push(Line { start, end: p1, hard });
                return;
            }
            out.push(Line { start, end, hard: false });
            start = end;
        }
    }

    /// Paragraph bounds (start, end at the LF or text end) around `at`.
    fn para_bounds(&self, at: usize) -> (usize, usize) {
        let at = at.min(self.text.len());
        let s = self.text[..at].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let e = self.text[at..].iter().position(|&b| b == b'\n').map_or(self.text.len(), |i| at + i);
        (s, e)
    }

    pub fn relayout(&mut self) {
        let mut out = Vec::new();
        let mut p0 = 0;
        loop {
            match self.text[p0..].iter().position(|&b| b == b'\n') {
                Some(i) => {
                    self.layout_para(p0, p0 + i, true, &mut out);
                    p0 += i + 1;
                }
                None => {
                    self.layout_para(p0, self.text.len(), false, &mut out);
                    break;
                }
            }
        }
        self.lines = out;
    }

    /// Re-lay out after text[at..at + removed] became text[at..at +
    /// inserted]: only the paragraphs touched change, later lines shift.
    fn relayout_edit(&mut self, at: usize, removed: usize, inserted: usize) {
        let old_end = at + removed;
        // First and last old lines touched (by paragraph).
        let first = self.line_of(at);
        let mut first = first;
        while first > 0 && !self.lines[first - 1].hard {
            first -= 1;
        }
        let mut last = first;
        while last < self.lines.len() && !(self.lines[last].hard && self.lines[last].end >= old_end) {
            last += 1;
        }
        let last = last.min(self.lines.len() - 1);
        let p_start = self.lines[first].start;
        let delta = inserted as isize - removed as isize;
        let old_tail_start = if self.lines[last].hard { self.lines[last].end + 1 } else { usize::MAX };
        // New paragraphs from p_start to the end of the paragraph holding
        // the end of the inserted text.
        let (_, new_end) = self.para_bounds(at + inserted);
        let mut out = Vec::new();
        let mut p0 = p_start;
        loop {
            let e = self.text[p0..new_end].iter().position(|&b| b == b'\n').map_or(new_end, |i| p0 + i);
            let hard = e < self.text.len();
            self.layout_para(p0, e, hard, &mut out);
            if e >= new_end {
                break;
            }
            p0 = e + 1;
        }
        let tail: Vec<Line> = if old_tail_start == usize::MAX {
            Vec::new()
        } else {
            self.lines[last + 1..]
                .iter()
                .map(|l| Line { start: (l.start as isize + delta) as usize, end: (l.end as isize + delta) as usize, hard: l.hard })
                .collect()
        };
        // An edit that removed the last line break leaves the old tail
        // behind the new end: lay everything out again then.
        let consistent = match (out.last(), tail.first()) {
            (Some(o), Some(t)) => o.hard && o.end + 1 == t.start,
            (Some(o), None) => !o.hard || o.end == self.text.len(),
            _ => false,
        };
        if !consistent {
            self.relayout();
            return;
        }
        self.lines.truncate(first);
        self.lines.extend(out);
        self.lines.extend(tail);
        if self.lines.last().is_some_and(|l| l.hard) {
            let n = self.text.len();
            self.lines.push(Line { start: n, end: n, hard: false });
        }
    }

    // ---- Positions ----

    /// Display line holding insertion point `ipt` (a point at a soft
    /// break belongs to the line it starts).
    pub fn line_of(&self, ipt: usize) -> usize {
        let ipt = ipt.min(self.text.len());
        // Binary search on line starts.
        let (mut lo, mut hi) = (0, self.lines.len());
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.lines[mid].start <= ipt {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Pel x of `ipt` within its line.
    pub fn x_of(&self, ipt: usize) -> i32 {
        let l = self.lines[self.line_of(ipt)];
        let mut x = 0;
        for &b in &self.text[l.start..ipt.min(l.end)] {
            x += self.advance(b, x);
        }
        x
    }

    /// Width of a display line in pels.
    pub fn line_width(&self, n: usize) -> i32 {
        let Some(l) = self.lines.get(n) else { return 0 };
        let mut x = 0;
        for &b in &self.text[l.start..l.end] {
            x += self.advance(b, x);
        }
        x
    }

    pub fn max_width(&self) -> i32 {
        (0..self.lines.len()).map(|i| self.line_width(i)).max().unwrap_or(0)
    }

    /// Insertion point nearest to pel x on display line `n`.
    pub fn ipt_at(&self, n: usize, x: i32) -> usize {
        let n = n.min(self.lines.len() - 1);
        let l = self.lines[n];
        let mut cx = 0;
        for i in l.start..l.end {
            let w = self.advance(self.text[i], cx);
            if x < cx + w / 2 + w % 2 {
                return i;
            }
            cx += w;
        }
        // At a soft break the end of the line is the start of the next:
        // stay before the last character so the point stays on this line.
        if !l.hard && n + 1 < self.lines.len() && l.end > l.start { l.end - 1 } else { l.end }
    }

    /// Visible pel x positions of the characters of display line `n`
    /// (start of each character and the end).
    pub fn cells(&self, n: usize) -> Vec<(usize, i32, i32)> {
        let Some(l) = self.lines.get(n) else { return Vec::new() };
        let mut x = 0;
        let mut v = Vec::with_capacity(l.end - l.start);
        for i in l.start..l.end {
            let w = self.advance(self.text[i], x);
            v.push((i, x, w));
            x += w;
        }
        v
    }

    pub fn line_start(&self, n: usize) -> usize {
        self.lines.get(n).map_or(self.text.len(), |l| l.start)
    }

    /// Hard line (paragraph) number of `ipt`, counting LFs.
    pub fn para_of(&self, ipt: usize) -> usize {
        self.text[..ipt.min(self.text.len())].iter().filter(|&&b| b == b'\n').count()
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    pub fn selected(&self) -> &[u8] {
        let (a, b) = self.selection();
        &self.text[a..b]
    }

    pub fn set_sel(&mut self, anchor: usize, cursor: usize) {
        let n = self.text.len();
        self.anchor = anchor.min(n);
        self.cursor = cursor.min(n);
        self.goal_x = None;
    }

    // ---- Editing ----

    /// Replace text[a..b] with `s` (LF line breaks); the cursor goes after
    /// it. `op` names the operation for undo (OP_TYPING extends an open
    /// typing record).
    pub fn replace(&mut self, a: usize, b: usize, s: &[u8], op: u32) -> Edit {
        if self.readonly {
            return Edit::ReadOnly;
        }
        let n = self.text.len();
        let (a, b) = (a.min(n), b.min(n).max(a.min(n)));
        if a == b && s.is_empty() {
            return Edit::Nothing;
        }
        if let Some(l) = self.limit {
            let new_len = n - (b - a) + s.len();
            if new_len > l && s.len() > b - a {
                return Edit::Overflow(new_len - l);
            }
        }
        let sel = (self.anchor, self.cursor);
        let removed: Vec<u8> = self.text[a..b].to_vec();
        if self.undo_enabled {
            let extend = op == OP_TYPING
                && removed.is_empty()
                && self.undo.as_ref().is_some_and(|u| u.open && u.op == OP_TYPING && u.at + u.inserted.len() == a);
            if extend {
                let u = self.undo.as_mut().expect("undo");
                u.inserted.extend_from_slice(s);
            } else {
                self.undo = Some(Undo { at: a, removed, inserted: s.to_vec(), sel, op, open: op == OP_TYPING });
            }
        }
        self.text.splice(a..b, s.iter().copied());
        self.relayout_edit(a, b - a, s.len());
        self.cursor = a + s.len();
        self.anchor = self.cursor;
        self.changed = true;
        self.goal_x = None;
        Edit::Done
    }

    /// Replace the selection (typing, MLM_INSERT, paste).
    pub fn insert(&mut self, s: &[u8], op: u32) -> Edit {
        let (a, b) = self.selection();
        self.replace(a, b, s, op)
    }

    /// Type a character: replaces the selection, or overtypes the
    /// character under the cursor in overtype mode (not a line break).
    pub fn type_char(&mut self, c: u8) -> Edit {
        let (a, mut b) = self.selection();
        if !self.insert && a == b && c != b'\n' && b < self.text.len() && self.text[b] != b'\n' {
            b += 1;
            // Overtyping does not grow the text.
            if let Some(u) = self.undo.as_mut() {
                u.open = false;
            }
            return self.replace(a, b, &[c], OP_TYPING + 1);
        }
        self.replace(a, b, &[c], OP_TYPING)
    }

    /// Delete the selection, or the character after the cursor.
    pub fn delete_forward(&mut self) -> Edit {
        let (a, b) = self.selection();
        if a != b {
            return self.replace(a, b, b"", OP_TYPING + 2);
        }
        if a >= self.text.len() {
            return Edit::Nothing;
        }
        self.replace(a, a + 1, b"", OP_TYPING + 2)
    }

    /// Backspace: delete the selection or the character before the cursor.
    pub fn delete_back(&mut self) -> Edit {
        let (a, b) = self.selection();
        if a != b {
            return self.replace(a, b, b"", OP_TYPING + 2);
        }
        if a == 0 {
            return Edit::Nothing;
        }
        self.replace(a - 1, a, b"", OP_TYPING + 2)
    }

    /// MLM_DELETE(ipt, cch): characters deleted.
    pub fn delete(&mut self, at: usize, n: usize) -> usize {
        let at = at.min(self.text.len());
        let n = n.min(self.text.len() - at);
        match self.replace(at, at + n, b"", 0x01c6) {
            Edit::Done => n,
            _ => 0,
        }
    }

    /// Close the open typing record (a cursor move ends a typing run).
    pub fn seal_undo(&mut self) {
        if let Some(u) = self.undo.as_mut() {
            u.open = false;
        }
    }

    /// MLM_QUERYUNDO: the operation that can be undone.
    pub fn undo_op(&self) -> Option<u32> {
        self.undo.as_ref().map(|u| u.op)
    }

    pub fn reset_undo(&mut self) {
        self.undo = None;
    }

    /// MLM_UNDO: undo the last change; undoing again redoes it.
    pub fn undo(&mut self) -> bool {
        let Some(u) = self.undo.take() else { return false };
        if self.readonly {
            self.undo = Some(u);
            return false;
        }
        let end = u.at + u.inserted.len();
        let now = (self.anchor, self.cursor);
        self.text.splice(u.at..end, u.removed.iter().copied());
        self.relayout_edit(u.at, u.inserted.len(), u.removed.len());
        let (a, c) = u.sel;
        let n = self.text.len();
        (self.anchor, self.cursor) = (a.min(n), c.min(n));
        self.changed = true;
        self.undo = Some(Undo { at: u.at, removed: u.inserted, inserted: u.removed, sel: now, op: u.op, open: false });
        true
    }

    // ---- Cursor movement ----

    fn moved(&mut self, to: usize, extend: bool, keep_goal: bool) {
        self.cursor = to.min(self.text.len());
        if !extend {
            self.anchor = self.cursor;
        }
        if !keep_goal {
            self.goal_x = None;
        }
        self.seal_undo();
    }

    pub fn left(&mut self, extend: bool) {
        let (a, b) = self.selection();
        let to = if !extend && a != b { a } else { self.cursor.saturating_sub(1) };
        self.moved(to, extend, false);
    }

    pub fn right(&mut self, extend: bool) {
        let (a, b) = self.selection();
        let to = if !extend && a != b { b } else { (self.cursor + 1).min(self.text.len()) };
        self.moved(to, extend, false);
    }

    fn is_word(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
    }

    pub fn word_left(&mut self, extend: bool) {
        let mut i = self.cursor;
        while i > 0 && !Self::is_word(self.text[i - 1]) {
            i -= 1;
        }
        while i > 0 && Self::is_word(self.text[i - 1]) {
            i -= 1;
        }
        self.moved(i, extend, false);
    }

    pub fn word_right(&mut self, extend: bool) {
        let n = self.text.len();
        let mut i = self.cursor;
        while i < n && Self::is_word(self.text[i]) {
            i += 1;
        }
        while i < n && !Self::is_word(self.text[i]) {
            i += 1;
        }
        self.moved(i, extend, false);
    }

    /// The word around `ipt` (for double clicks).
    pub fn word_at(&self, ipt: usize) -> (usize, usize) {
        let n = self.text.len();
        let ipt = ipt.min(n);
        let word = |i: usize| i < n && Self::is_word(self.text[i]);
        if !word(ipt) {
            return (ipt, (ipt + 1).min(n).max(ipt));
        }
        let mut a = ipt;
        while a > 0 && Self::is_word(self.text[a - 1]) {
            a -= 1;
        }
        let mut b = ipt;
        while word(b) {
            b += 1;
        }
        (a, b)
    }

    pub fn home(&mut self, extend: bool) {
        let l = self.lines[self.line_of(self.cursor)];
        self.moved(l.start, extend, false);
    }

    pub fn end(&mut self, extend: bool) {
        let n = self.line_of(self.cursor);
        let l = self.lines[n];
        let to = if !l.hard && n + 1 < self.lines.len() && l.end > l.start { l.end - 1 } else { l.end };
        // A soft line's end is the next line's start; keep the point on
        // this line (before its last blank) unless it is the text end.
        let to = if !l.hard && n + 1 < self.lines.len() && self.text.get(to) != Some(&b' ') { l.end } else { to };
        self.moved(to, extend, false);
    }

    pub fn doc_start(&mut self, extend: bool) {
        self.moved(0, extend, false);
    }

    pub fn doc_end(&mut self, extend: bool) {
        let n = self.text.len();
        self.moved(n, extend, false);
    }

    /// Move `dy` display lines (negative: up), keeping the pel column.
    pub fn vertical(&mut self, dy: isize, extend: bool) {
        let x = self.goal_x.unwrap_or_else(|| self.x_of(self.cursor));
        let n = self.line_of(self.cursor) as isize + dy;
        let n = n.clamp(0, self.lines.len() as isize - 1) as usize;
        let to = self.ipt_at(n, x);
        self.moved(to, extend, true);
        self.goal_x = Some(x);
    }

    // ---- Import and export ----

    /// Convert text in format `f` to the internal form. Returns the text
    /// and how many input bytes were used (a CR at the very end of the
    /// input is kept back, its LF may follow in the next import).
    pub fn from_format(f: Format, data: &[u8]) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            match (f, b) {
                (Format::NoTrans, _) => out.push(b),
                (_, b'\r') => {
                    if i + 1 >= data.len() {
                        break;
                    }
                    if data[i + 1] == b'\n' {
                        out.push(b'\n');
                        i += 2;
                        continue;
                    }
                    if f == Format::WinFmt && data[i + 1] == b'\r' && i + 2 < data.len() && data[i + 2] == b'\n' {
                        // A soft break: dropped.
                        i += 3;
                        continue;
                    }
                    if f == Format::WinFmt && data[i + 1] == b'\r' && i + 2 >= data.len() {
                        break;
                    }
                    out.push(b);
                }
                _ => out.push(b),
            }
            i += 1;
        }
        (out, i)
    }

    /// Export characters [start, start + count) in format `f`, at most
    /// `max` bytes. Returns the bytes and the characters exported.
    pub fn export(&self, start: usize, count: usize, f: Format, max: usize) -> (Vec<u8>, usize) {
        let n = self.text.len();
        let start = start.min(n);
        let end = start.saturating_add(count).min(n);
        let mut out = Vec::new();
        let mut i = start;
        let soft: Vec<usize> = if f == Format::WinFmt { self.lines.iter().filter(|l| !l.hard && l.end < n).map(|l| l.end).collect() } else { Vec::new() };
        while i < end {
            let b = self.text[i];
            let piece: &[u8] = match (f, b) {
                (Format::NoTrans, _) => core::slice::from_ref(&self.text[i]),
                (_, b'\n') => b"\r\n",
                _ => core::slice::from_ref(&self.text[i]),
            };
            let soft_here = f == Format::WinFmt && i > start && soft.binary_search(&i).is_ok();
            let need = piece.len() + if soft_here { 3 } else { 0 };
            if out.len() + need > max {
                break;
            }
            if soft_here {
                out.extend_from_slice(b"\r\r\n");
            }
            out.extend_from_slice(piece);
            i += 1;
        }
        (out, i - start)
    }

    /// Bytes [start, start + count) take in format `f`.
    pub fn format_len(&self, start: usize, count: usize, f: Format) -> usize {
        let n = self.text.len();
        let start = start.min(n);
        let end = start.saturating_add(count).min(n);
        let lf = self.text[start..end].iter().filter(|&&b| b == b'\n').count();
        let soft = if f == Format::WinFmt { self.lines.iter().filter(|l| !l.hard && l.end > start && l.end < end).count() } else { 0 };
        match f {
            Format::NoTrans => end - start,
            Format::CfText => end - start + lf,
            Format::WinFmt => end - start + lf + 3 * soft,
        }
    }

    /// Import `data` (format `f`) at `at`, replacing nothing: returns the
    /// input bytes used and the characters inserted (0, 0 on overflow).
    pub fn import(&mut self, at: usize, data: &[u8], f: Format) -> Result<(usize, usize), Edit> {
        let (t, used) = Self::from_format(f, data);
        let at = at.min(self.text.len());
        let sel = (self.anchor, self.cursor);
        match self.replace(at, at, &t, 0x01c3) {
            Edit::Done | Edit::Nothing => {}
            e => return Err(e),
        }
        // The selection stays where it was (shifted if after `at`).
        let shift = |p: usize| if p > at { p + t.len() } else { p };
        (self.anchor, self.cursor) = (shift(sel.0), shift(sel.1));
        Ok((used, t.len()))
    }

    // ---- Searching ----

    /// Find `pat` in [start, stop) (case-insensitively unless `case`).
    pub fn find(&self, pat: &[u8], start: usize, stop: usize, case: bool) -> Option<usize> {
        let n = self.text.len();
        let stop = stop.min(n);
        if pat.is_empty() || start >= stop || stop - start < pat.len() {
            return None;
        }
        let eq = |a: u8, b: u8| if case { a == b } else { a.eq_ignore_ascii_case(&b) };
        (start..=stop - pat.len()).find(|&i| self.text[i..i + pat.len()].iter().zip(pat).all(|(&a, &b)| eq(a, b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn mle(t: &[u8]) -> Mle {
        let mut m = Mle::new();
        m.set_text(t);
        m
    }

    #[test]
    fn lines_without_wrap() {
        let m = mle(b"one\ntwo\n\nfour");
        assert_eq!(m.line_count(), 4);
        assert_eq!(m.lines()[1], Line { start: 4, end: 7, hard: true });
        assert_eq!(m.lines()[3], Line { start: 9, end: 13, hard: false });
        assert_eq!(m.line_of(8), 2);
        assert_eq!(m.line_of(13), 3);
        let m = mle(b"a\n");
        assert_eq!(m.line_count(), 2);
        assert_eq!(m.lines()[1], Line { start: 2, end: 2, hard: false });
    }

    #[test]
    fn wrap_at_blanks() {
        let mut m = mle(b"the quick brown fox");
        // 10 columns of 8 pels.
        m.set_layout(80, 8, 64, true);
        let l: Vec<&[u8]> = m.lines().iter().map(|l| &m.text()[l.start..l.end]).collect();
        assert_eq!(l, vec![&b"the quick "[..], b"brown fox"]);
        // A word longer than the line is broken.
        m.set_text(b"abcdefghijklmnop");
        let l: Vec<&[u8]> = m.lines().iter().map(|l| &m.text()[l.start..l.end]).collect();
        assert_eq!(l, vec![&b"abcdefghij"[..], b"klmnop"]);
        m.set_layout(80, 8, 64, false);
        assert_eq!(m.line_count(), 1);
    }

    #[test]
    fn tabs() {
        let mut m = mle(b"a\tb");
        m.set_layout(0, 8, 32, false);
        assert_eq!(m.x_of(2), 32);
        assert_eq!(m.x_of(3), 40);
        assert_eq!(m.ipt_at(0, 30), 2);
        assert_eq!(m.ipt_at(0, 10), 1);
    }

    #[test]
    fn editing_keeps_layout() {
        let mut m = mle(b"hello world\nsecond line here\nthird");
        m.set_layout(80, 8, 64, true);
        m.set_sel(5, 5);
        for &c in b" there my dear" {
            m.type_char(c);
        }
        let mut fresh = mle(m.text());
        fresh.set_layout(80, 8, 64, true);
        assert_eq!(m.lines(), fresh.lines());
        m.set_sel(0, 20);
        m.insert(b"x\ny\n", 0);
        let mut fresh = mle(m.text());
        fresh.set_layout(80, 8, 64, true);
        assert_eq!(m.lines(), fresh.lines());
        // Joining two paragraphs.
        let p = m.text().iter().position(|&b| b == b'\n').unwrap();
        m.set_sel(p, p + 1);
        m.delete_forward();
        let mut fresh = mle(m.text());
        fresh.set_layout(80, 8, 64, true);
        assert_eq!(m.lines(), fresh.lines());
        // Removing the last break.
        let mut m = mle(b"a\n");
        m.set_sel(1, 2);
        m.delete_forward();
        assert_eq!(m.lines(), &[Line { start: 0, end: 1, hard: false }]);
        let mut m = mle(b"ab");
        m.set_sel(2, 2);
        m.type_char(b'\n');
        assert_eq!(m.line_count(), 2);
    }

    #[test]
    fn typing_and_undo() {
        let mut m = mle(b"abc");
        m.set_sel(3, 3);
        m.type_char(b'd');
        m.type_char(b'e');
        assert_eq!(m.text(), b"abcde");
        assert_eq!(m.undo_op(), Some(OP_TYPING));
        assert!(m.undo());
        assert_eq!(m.text(), b"abc");
        assert_eq!(m.cursor, 3);
        // Undo again redoes.
        assert!(m.undo());
        assert_eq!(m.text(), b"abcde");
        m.set_sel(0, 2);
        m.delete_back();
        assert_eq!(m.text(), b"cde");
        m.undo();
        assert_eq!(m.text(), b"abcde");
        assert_eq!(m.selection(), (0, 2));
        m.reset_undo();
        assert!(!m.undo());
    }

    #[test]
    fn overtype_and_limit() {
        let mut m = mle(b"abc");
        m.insert = false;
        m.set_sel(1, 1);
        m.type_char(b'X');
        assert_eq!(m.text(), b"aXc");
        m.limit = Some(4);
        m.insert = true;
        assert_eq!(m.type_char(b'1'), Edit::Done);
        assert_eq!(m.type_char(b'2'), Edit::Overflow(1));
        m.readonly = true;
        assert_eq!(m.delete_back(), Edit::ReadOnly);
    }

    #[test]
    fn import_export() {
        let (t, used) = Mle::from_format(Format::CfText, b"a\r\nb\r");
        assert_eq!((t.as_slice(), used), (&b"a\nb"[..], 4));
        let (t, _) = Mle::from_format(Format::CfText, b"x\ny");
        assert_eq!(t, b"x\ny");
        let (t, _) = Mle::from_format(Format::WinFmt, b"ab\r\r\ncd\r\n");
        assert_eq!(t, b"abcd\n");
        let (t, _) = Mle::from_format(Format::NoTrans, b"a\r\nb");
        assert_eq!(t, b"a\r\nb");
        let mut m = mle(b"one\ntwo");
        assert_eq!(m.format_len(0, usize::MAX, Format::CfText), 8);
        let (out, n) = m.export(0, usize::MAX, Format::CfText, 100);
        assert_eq!((out.as_slice(), n), (&b"one\r\ntwo"[..], 7));
        // A small buffer stops before a break that does not fit.
        let (out, n) = m.export(0, usize::MAX, Format::CfText, 4);
        assert_eq!((out.as_slice(), n), (&b"one"[..], 3));
        let (out, _) = m.export(2, 3, Format::NoTrans, 100);
        assert_eq!(out, b"e\nt");
        m.set_text(b"aaaa bbbb");
        m.set_layout(40, 8, 64, true);
        let (out, _) = m.export(0, usize::MAX, Format::WinFmt, 100);
        assert_eq!(out, b"aaaa \r\r\nbbbb");
        assert_eq!(m.format_len(0, usize::MAX, Format::WinFmt), 12);
        let mut m = mle(b"ac");
        m.set_sel(2, 2);
        assert_eq!(m.import(1, b"b\r\n", Format::CfText), Ok((3, 2)));
        assert_eq!(m.text(), b"ab\nc");
        assert_eq!(m.cursor, 4);
    }

    #[test]
    fn movement() {
        let mut m = mle(b"one two\nthree\nx");
        m.set_sel(5, 5);
        m.vertical(1, false);
        assert_eq!(m.cursor, 13);
        m.vertical(1, false);
        assert_eq!(m.cursor, 15);
        // The column is remembered.
        m.vertical(-1, false);
        assert_eq!(m.cursor, 13);
        m.home(false);
        assert_eq!(m.cursor, 8);
        m.end(true);
        assert_eq!(m.selection(), (8, 13));
        m.set_sel(0, 0);
        m.word_right(false);
        assert_eq!(m.cursor, 4);
        m.word_left(false);
        assert_eq!(m.cursor, 0);
        assert_eq!(m.word_at(5), (4, 7));
        m.doc_end(false);
        assert_eq!(m.cursor, m.len());
    }

    #[test]
    fn soft_break_positions() {
        let mut m = mle(b"aaaa bbbb");
        m.set_layout(40, 8, 64, true);
        assert_eq!(m.line_count(), 2);
        assert_eq!(m.line_of(5), 1);
        m.set_sel(0, 0);
        m.end(false);
        assert_eq!(m.line_of(m.cursor), 0);
        assert_eq!(m.ipt_at(0, 1000), 4);
    }

    #[test]
    fn search() {
        let m = mle(b"Hello hello HELLO");
        assert_eq!(m.find(b"hello", 0, 100, true), Some(6));
        assert_eq!(m.find(b"hello", 0, 100, false), Some(0));
        assert_eq!(m.find(b"hello", 7, 100, false), Some(12));
        assert_eq!(m.find(b"hello", 7, 15, false), None);
    }
}
