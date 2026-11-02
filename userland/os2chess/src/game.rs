//! The game: its moves and record, the computer's thinking on its own
//! thread, taking moves back, saving and loading `*.PMC` files, and
//! printing the board and the record.

use alloc::vec::Vec;

use pmkit::gpi;
use pmkit::*;

use crate::engine::*;
use crate::{app, ms, side_index, trace, TITLE, WM_COMPUTERMOVE};

pub struct Game {
    /// Where the game started (a set-up position, or the usual one).
    pub start: Vec<u8>,
    pub pos: Position,
    pub moves: Vec<Move>,
    /// Each move's notation.
    pub notes: Vec<Vec<u8>>,
    /// The pieces taken, in order.
    pub captured: Vec<i8>,
    pub over: Option<Status>,
    /// The file it was loaded from or saved to.
    pub file: Option<Vec<u8>>,
}

impl Game {
    pub fn new() -> Game {
        Game::from(START_FEN)
    }

    pub fn from(fen: &[u8]) -> Game {
        Game { start: fen.to_vec(), pos: Position::from_fen(fen).unwrap_or_else(Position::start), moves: Vec::new(), notes: Vec::new(), captured: Vec::new(), over: None, file: None }
    }

    /// Play a legal move; its notation.
    pub fn play(&mut self, m: Move) -> Vec<u8> {
        let note = self.pos.notation(m);
        let cap = if m.flags & F_EP != 0 { -self.pos.side * PAWN } else { self.pos.sq[m.to as usize] };
        if self.pos.make(m) {
            self.moves.push(m);
            self.notes.push(note.clone());
            if cap != 0 {
                self.captured.push(cap);
            }
        }
        note
    }

    /// Take the last move back.
    pub fn undo(&mut self) -> bool {
        if self.moves.is_empty() {
            return false;
        }
        let moves = core::mem::take(&mut self.moves);
        let file = self.file.take();
        *self = Game::from(&self.start.clone());
        for &m in &moves[..moves.len() - 1] {
            self.play(m);
        }
        self.file = file;
        true
    }
}

impl Default for Game {
    fn default() -> Self {
        Game::new()
    }
}

pub fn side_name(side: i8) -> &'static [u8] {
    if side == WHITE { b"White" } else { b"Black" }
}

/// OS/2 Chess's message for the end of a game (`side`: the side to move).
pub fn result_text(s: Status, side: i8) -> Vec<u8> {
    match s {
        Status::Checkmate => {
            let mut t = b"Checkmate!!!  ".to_vec();
            t.extend_from_slice(side_name(-side));
            t.extend_from_slice(b" wins!");
            t
        }
        Status::Stalemate => b"Stalemate!".to_vec(),
        Status::Fifty => b"Draw!  50 moves without a capture or Pawn move.".to_vec(),
        Status::Repetition => b"Draw!  Moves have been repeated three times.".to_vec(),
        Status::Material => b"Draw!  Insufficient pieces to cause mate.".to_vec(),
        Status::Check => b"Check!".to_vec(),
        Status::Normal => Vec::new(),
    }
}

const NAMES: [&[u8]; 7] = [b"", b"Pawn", b"Knight", b"Bishop", b"Rook", b"Queen", b"King"];

pub fn piece_name(p: i8) -> &'static [u8] {
    NAMES[p.unsigned_abs() as usize % 7]
}

/// Why the move from `from` to `to` is not allowed (OS/2 Chess's texts).
pub fn why_invalid(p: &mut Position, from: u8, to: u8) -> Vec<u8> {
    let pc = p.sq[from as usize];
    let tgt = p.sq[to as usize];
    if pc == 0 || (pc > 0) != (p.side > 0) {
        return b"Wrong piece color!".to_vec();
    }
    if tgt != 0 && (tgt > 0) == (pc > 0) {
        return b"Cannot capture your own piece!".to_vec();
    }
    let mut pseudo = Vec::new();
    p.pseudo_moves(false, &mut pseudo);
    if pseudo.iter().any(|m| m.from == from && m.to == to) {
        return if p.in_check(p.side) { b"King is in check.".to_vec() } else { b"King will be in check.".to_vec() };
    }
    if pc.abs() == KING && (file(to) - file(from)).abs() == 2 && rank(to) == rank(from) {
        if p.in_check(p.side) {
            return b"King is in check.".to_vec();
        }
        let home = if pc > 0 { 4 } else { 60 };
        if from != home {
            return b"Cannot castle if King was moved.".to_vec();
        }
        let rook = if to > from { from + 3 } else { from - 4 };
        if p.sq[rook as usize] != ROOK * p.side {
            return b"Cannot castle if Rook was moved.".to_vec();
        }
        let between = if to > from { from + 1..rook } else { rook + 1..from };
        if between.clone().any(|s| p.sq[s as usize] != 0) {
            return b"Cannot jump over a piece!".to_vec();
        }
        let mut f2 = p.clone();
        f2.castling = 0xf;
        let mut ok = Vec::new();
        f2.pseudo_moves(false, &mut ok);
        if !ok.iter().any(|m| m.from == from && m.to == to) {
            return b"Cannot castle through check.".to_vec();
        }
        return if to > from { b"Cannot castle if Rook was moved.".to_vec() } else { b"Cannot castle if King was moved.".to_vec() };
    }
    // A slider or pawn with something in the way?
    let (df, dr) = (file(to) - file(from), rank(to) - rank(from));
    let straight = df == 0 || dr == 0 || df.abs() == dr.abs();
    if straight && pc.abs() != KNIGHT && pc.abs() != KING {
        let (sf, sr) = (df.signum(), dr.signum());
        let (mut f, mut r) = (file(from) + sf, rank(from) + sr);
        while (f, r) != (file(to), rank(to)) {
            if p.sq[(r * 8 + f) as usize] != 0 {
                let shaped = match pc.abs() {
                    BISHOP => df.abs() == dr.abs(),
                    ROOK => df == 0 || dr == 0,
                    QUEEN => true,
                    _ => df == 0,
                };
                if shaped {
                    return b"Cannot jump over a piece!".to_vec();
                }
                break;
            }
            f += sf;
            r += sr;
        }
    }
    let mut t = b"Illegal move for ".to_vec();
    t.extend_from_slice(piece_name(pc));
    t
}

// ---- The computer's thinking ----

pub fn pack(m: Move) -> u32 {
    m.from as u32 | (m.to as u32) << 8 | ((m.promo as u8) as u32) << 16 | (m.flags as u32) << 24
}

pub fn unpack(v: u32) -> Move {
    Move { from: v as u8, to: (v >> 8) as u8, promo: (v >> 16) as u8 as i8, flags: (v >> 24) as u8 }
}

/// What the thinking thread works on.
struct Request {
    pos: Position,
    played: Vec<Move>,
    depth: u32,
    ms: u32,
    book: bool,
    random: u32,
    id: u32,
    client: u32,
}

static mut REQUEST: Option<Request> = None;
static mut ABORT: bool = false;
/// The search in progress (for Move Status): nodes, depth, best move.
pub static mut PROGRESS: (u64, u32, u32) = (0, 0, 0);

/// (plies, milliseconds) for each playing level.
pub const LEVELS: [(u32, u32); 5] = [(1, 400), (2, 1000), (3, 3000), (5, 6000), (8, 12000)];
pub const LEVEL_NAMES: [&[u8]; 5] = [b"Beginner", b"Novice", b"Intermediate", b"Experienced", b"Advanced"];

pub fn start_thinking() {
    let a = app();
    let p = &a.players[side_index(a.game.pos.side)];
    let (depth, msec) = LEVELS[p.level as usize % 5];
    a.request = a.request.wrapping_add(1);
    a.thinking = true;
    a.think_start = ms();
    unsafe {
        PROGRESS = (0, 0, 0);
        ABORT = false;
        REQUEST = Some(Request {
            pos: a.game.pos.clone(),
            played: if a.game.start.as_slice() == START_FEN { a.game.moves.clone() } else { Vec::new() },
            depth,
            ms: msec,
            book: a.book && a.game.start.as_slice() == START_FEN,
            random: if p.level == 0 { 1 } else { 0 },
            id: a.request,
            client: a.client,
        });
    }
    let mut tid = 0u32;
    unsafe { os2::DosCreateThread(&mut tid, thinker, 0, 0, 512 * 1024) };
    crate::update_menu();
}

/// Tell a running search to end (its answer is dropped).
pub fn abort_thinking() {
    let a = app();
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(ABORT), true) };
    a.thinking = false;
}

unsafe extern "C" fn thinker(_: u32) {
    let Some(mut r) = (unsafe { (*core::ptr::addr_of_mut!(REQUEST)).take() }) else { return };
    let mut rng = Rng::new();
    let mut m = Move::NONE;
    if r.book {
        m = book_move(&mut r.pos, &r.played, rng.next()).unwrap_or(Move::NONE);
    }
    if m.is_none() {
        let mut s = Search::new(ms);
        s.abort = core::ptr::addr_of!(ABORT);
        m = s.think(&mut r.pos, r.depth, r.ms);
        unsafe { PROGRESS = (s.nodes, s.depth_done, pack(m)) };
        // A beginner sometimes plays another move than the best.
        if r.random != 0 && rng.below(3) == 0 {
            let all = r.pos.legal_moves();
            if !all.is_empty() {
                m = all[rng.below(all.len() as u32) as usize];
            }
        }
    }
    if !unsafe { core::ptr::read_volatile(core::ptr::addr_of!(ABORT)) } {
        unsafe { WinPostMsg(r.client, WM_COMPUTERMOVE, pack(m), r.id) };
    }
}

/// Moves > Take back: the last move, and the computer's answer before
/// it when the user plays against the computer.
pub fn take_back() {
    let a = app();
    if a.thinking {
        a.request = a.request.wrapping_add(1);
        abort_thinking();
    }
    if !a.game.undo() {
        return;
    }
    a.game.over = None;
    while a.players[side_index(a.game.pos.side)].computer && !a.players.iter().all(|p| p.computer) && a.game.undo() {}
    trace(b"take back");
    // Taken back, a computer to move waits for the user (New or Set
    // players starts it again), as OS/2 Chess does.
    a.selected = None;
    crate::update_menu();
    crate::dialogs::refresh_record();
    crate::new_status();
}

// ---- Saving and loading (*.PMC in the current directory) ----

const MAGIC: &[u8] = b"RS2 OS/2 Chess game 1";

pub fn date_text() -> Vec<u8> {
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    alloc::format!("{:04}-{:02}-{:02} {:02}:{:02}", dt.year, dt.month, dt.day, dt.hours, dt.minutes).into_bytes()
}

/// Game > Save: the game into its file, or a new GAMEnnnn.PMC.
pub fn save_game() {
    let a = app();
    let g = &a.game;
    let name = match &g.file {
        Some(f) => f.clone(),
        None => {
            let mut n = 1;
            loop {
                let f = alloc::format!("GAME{n:04}.PMC").into_bytes();
                if read_file(&f).is_none() || n > 9999 {
                    break f;
                }
                n += 1;
            }
        }
    };
    let mut t = Vec::new();
    for line in [MAGIC, b"White ", &a.players[0].name, b"\r\nBlack ", &a.players[1].name, b"\r\nDate ", &date_text(), b"\r\nStart ", &g.start, b"\r\nMoves"] {
        t.extend_from_slice(line);
        if line == MAGIC {
            t.extend_from_slice(b"\r\n");
        }
    }
    for m in &g.moves {
        t.push(b' ');
        t.extend_from_slice(&square_name(m.from));
        t.extend_from_slice(&square_name(m.to));
        if m.promo != 0 {
            t.push(PIECE_LETTERS[m.promo.unsigned_abs() as usize].to_ascii_lowercase());
        }
    }
    t.extend_from_slice(b"\r\n");
    match write_file(&name, &t) {
        Ok(()) => {
            a.game.file = Some(name.clone());
            let mut m = b"saved ".to_vec();
            m.extend_from_slice(&name);
            trace(&m);
        }
        Err(rc) => {
            message(a.frame, alloc::format!("The game could not be saved (error {rc}).").as_bytes(), TITLE, MB_OK | MB_ERROR);
        }
    }
}

/// A saved game's description for the Load and Delete lists:
/// "White vs Black  date".
pub fn describe(file: &[u8]) -> Option<Vec<u8>> {
    let t = read_file(file)?;
    if !t.starts_with(MAGIC) {
        return None;
    }
    let field = |k: &[u8]| -> Vec<u8> {
        t.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)).find_map(|l| l.strip_prefix(k).map(|v| v.to_vec())).unwrap_or_default()
    };
    let mut d = field(b"White ");
    d.extend_from_slice(b" vs ");
    d.extend_from_slice(&field(b"Black "));
    d.extend_from_slice(b"   ");
    d.extend_from_slice(&field(b"Date "));
    Some(d)
}

/// The saved games in the current directory.
pub fn saved_games() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    let mut hdir: u32 = 0xffff_ffff;
    let mut count: u32 = 64;
    let rc = unsafe { os2::DosFindFirst(b"*.PMC\0".as_ptr(), &mut hdir, 0x20, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    if rc != 0 {
        return out;
    }
    loop {
        // FILEFINDBUF3: oNextEntryOffset, dates, sizes, attributes, cchName at 28, name at 29.
        let mut off = 0usize;
        for _ in 0..count {
            let next = rd32(&buf, off) as usize;
            let n = buf[off + 28] as usize;
            out.push(buf[off + 29..off + 29 + n].to_vec());
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 64;
        if unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) } != 0 {
            break;
        }
    }
    unsafe { os2::DosFindClose(hdir) };
    out.sort();
    out
}

/// Load a saved game; false if the file is not one.
pub fn load_game(file: &[u8]) -> bool {
    let Some(t) = read_file(file) else { return false };
    if !t.starts_with(MAGIC) {
        return false;
    }
    let mut white = b"White".to_vec();
    let mut black = b"Black".to_vec();
    let mut start = START_FEN.to_vec();
    let mut moves: Vec<Vec<u8>> = Vec::new();
    for l in t.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)) {
        if let Some(v) = l.strip_prefix(b"White ") {
            white = v.to_vec();
        } else if let Some(v) = l.strip_prefix(b"Black ") {
            black = v.to_vec();
        } else if let Some(v) = l.strip_prefix(b"Start ") {
            start = v.to_vec();
        } else if let Some(v) = l.strip_prefix(b"Moves") {
            moves = v.split(|&c| c == b' ').filter(|s| s.len() >= 4).map(|s| s.to_vec()).collect();
        }
    }
    let a = app();
    a.request = a.request.wrapping_add(1);
    abort_thinking();
    let mut g = Game::from(&start);
    for m in moves {
        let (Some(f), Some(to)) = (parse_square(&m[0..2]), parse_square(&m[2..4])) else { break };
        let promo = match m.get(4) {
            Some(b'r') => ROOK,
            Some(b'b') => BISHOP,
            Some(b'n') => KNIGHT,
            _ => QUEEN,
        };
        match g.pos.find_move(f, to, promo) {
            Some(mv) => {
                g.play(mv);
            }
            None => break,
        }
    }
    g.file = Some(file.to_vec());
    a.players[0].name = white;
    a.players[1].name = black;
    a.game = g;
    a.setting = false;
    let mut m = b"loaded ".to_vec();
    m.extend_from_slice(file);
    trace(&m);
    true
}

// ---- Printing ----

/// Game > Print: the board and the game record on the default printer.
pub fn print() {
    let a = app();
    let mut buf = [0u8; 64];
    let n = unsafe { PrfQueryProfileString(HINI_SYSTEMPROFILE, b"PM_SPOOLER\0".as_ptr(), b"QUEUE\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 64) } as usize;
    let q: Vec<u8> = buf[..n.min(64)].iter().copied().take_while(|&c| c != 0 && c != b';').collect();
    if q.is_empty() {
        message(a.frame, b"No default printer installed!", TITLE, MB_OK | MB_ERROR);
        return;
    }
    let qz = z(&q);
    // DEVOPENSTRUC: logical address (the queue), driver name, data...
    let mut dop = [0u32; 9];
    dop[0] = qz.as_ptr() as u32;
    dop[3] = b"PM_Q_STD\0".as_ptr() as u32;
    let hdc = unsafe { DevOpenDC(a.hab, OD_QUEUED, b"*\0".as_ptr(), 4, dop.as_ptr(), 0) };
    if hdc == 0 {
        message(a.frame, b"Unable to open printer!", TITLE, MB_OK | MB_ERROR);
        return;
    }
    let mut caps = [0i32; 2];
    unsafe { DevQueryCaps(hdc, CAPS_WIDTH, 2, caps.as_mut_ptr()) };
    let (w, h) = (caps[0].max(500), caps[1].max(700));
    let hps = unsafe { GpiCreatePS(a.hab, hdc, &SizeL { cx: 0, cy: 0 }, PU_PELS | GPIA_ASSOC) };
    let doc = b"OS/2 Chess\0";
    unsafe { DevEscape(hdc, DEVESC_STARTDOC, doc.len() as i32 - 1, doc.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut()) };
    gpi::rgb_mode(hps);
    // The board in the upper half, flat, black and white.
    let side = w.min(h / 2) * 4 / 5;
    let area = RectL::new((w - side) / 2, h - side - h / 20, (w + side) / 2, h - h / 20);
    let view = crate::draw::View { three_d: false, flipped: false, angle: 0, labels: true };
    let l = crate::draw::layout(area, &view);
    let c = crate::draw::Colors { background: 0xffffff, light: 0xffffff, dark: 0xb0b0b0, edge: 0x000000, labels: 0x000000, white: 0xffffff, black: 0x404040, highlight: 0x000000 };
    crate::draw::board(hps, &l, &a.game.pos, &view, &c, &crate::draw::Marks { selected: None, targets: &[], last: None });
    // The record below it, in two columns per line.
    gpi::select_font(hps, 2, b"Courier", (h / 70).max(8));
    let (lh, _) = gpi::font_height(hps);
    let mut y = area.b - lh * 2;
    let title = alloc::format!("{} (White) - {} (Black)", core::str::from_utf8(&a.players[0].name).unwrap_or(""), core::str::from_utf8(&a.players[1].name).unwrap_or(""));
    gpi::text_at(hps, w / 10, y, title.as_bytes(), 0);
    y -= lh * 2;
    let mut col = 0;
    for (i, pair) in a.game.notes.chunks(2).enumerate() {
        let mut t = alloc::format!("{:3}. ", i + 1).into_bytes();
        t.extend_from_slice(&pair[0]);
        while t.len() < 18 {
            t.push(b' ');
        }
        if let Some(b) = pair.get(1) {
            t.extend_from_slice(b);
        }
        gpi::text_at(hps, w / 10 + col * w * 2 / 5, y, &t, 0);
        y -= lh;
        if y < h / 20 {
            if col == 1 {
                break;
            }
            col = 1;
            y = area.b - lh * 4;
        }
    }
    unsafe {
        let mut id = [0u8; 2];
        let mut n = 2;
        DevEscape(hdc, DEVESC_ENDDOC, 0, core::ptr::null(), &mut n, id.as_mut_ptr());
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    trace(b"printed");
}
