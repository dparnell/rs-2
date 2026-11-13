//! The block cache under the file systems.
//!
//! Every volume of a fixed or USB disk reaches its device through a
//! [`Cache`], below the hard-error pop-ups. Blocks are 4 KB page frames,
//! aligned on the volume, with a bit per sector for what is valid and what
//! is dirty: a sector written needs no read of its neighbours. The volumes
//! share a budget of frames (`IFS=...HPFS.IFS /CACHE:n`, otherwise an
//! eighth of memory); a volume under its share of the budget takes frames
//! from those over it.
//!
//! The lock is held across device requests: file systems have one request
//! going at a time anyway, and it leaves nothing in flight to reason about.
//!
//! A dirty block is written by the lazy writer, under the rules of IBM's
//! HPFS: when the disk has been idle for DiskIdle, when the block has not
//! changed for BufferIdle or has been dirty for MaxAge, and at once when
//! more than DirtyMax blocks are dirty. Closing a file leaves its blocks to
//! the writer; DosResetBuffer, write-through opens and shutdown do not.
//! Only volumes whose file system says so ([`Cache::allow_lazy`]) and that
//! stay in the machine ([`CacheKind::Lazy`]) write lazily: the others, JFS
//! with its log and USB sticks that can be pulled out, write through.
//!
//! Requests over the limit (`/CRECL:n`) go straight to the device, as
//! IBM's HPFS does with large transfers: they would only push out what is
//! worth keeping.

use alloc::collections::BTreeSet;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::block::{BlockDev, CacheKind};
use crate::mm::phys;
use crate::sync::{Mutex, SpinLock};

const SECTOR: usize = 512;
/// Sectors in a block.
const SPB: u64 = 8;
const BLOCK: usize = SECTOR * SPB as usize;
const BUCKETS: usize = 8192;
const NIL: u32 = u32::MAX;
/// Blocks a sequential miss reads at once.
const READ_AHEAD: u64 = 8;
/// A volume keeps this many blocks even when the budget has run out.
const MIN_BLOCKS: usize = 16;
/// Blocks taken from another volume at a time.
const STEAL: usize = 64;

/// Blocks written in one go by the lazy writer.
const BATCH: usize = 64;
/// How often the lazy writer looks (ms).
const PERIOD: u64 = 100;

/// DiskIdle, MaxAge, BufferIdle (ms), OS/2's defaults.
static TIMES: SpinLock<(u32, u32, u32)> = SpinLock::new((1000, 5000, 500));
/// Lazy writing is on (`CACHE -LAZY:OFF`, shutdown).
static LAZY: AtomicBool = AtomicBool::new(true);

/// Frames all caches may hold, and hold.
static BUDGET: AtomicU64 = AtomicU64::new(0);
static USED: AtomicU64 = AtomicU64::new(0);
/// Largest request cached, in bytes.
static RECORD: AtomicU64 = AtomicU64::new(64 * 1024);
static CACHES: SpinLock<Vec<Weak<Cache>>> = SpinLock::new(Vec::new());

fn budget() -> u64 {
    match BUDGET.load(Ordering::Relaxed) {
        0 => {
            let b = (phys::total_frames() / 8).clamp(256, 65536);
            BUDGET.store(b, Ordering::Relaxed);
            b
        }
        b => b,
    }
}

/// `/CACHE:n` and `/CRECL:n` of the CONFIG.SYS, in KB.
pub fn configure(size_kb: Option<u64>, record_kb: Option<u64>) {
    if let Some(k) = size_kb {
        BUDGET.store((k / 4).max(MIN_BLOCKS as u64), Ordering::Relaxed);
    }
    if let Some(k) = record_kb {
        RECORD.store(k * 1024, Ordering::Relaxed);
    }
}

/// The budget, in bytes.
pub fn size() -> u64 {
    budget() * BLOCK as u64
}

struct Entry {
    block: u64,
    frame: u64,
    valid: u8,
    dirty: u8,
    /// When the block first became dirty and was last written (ms).
    dirtied: u64,
    written: u64,
    hash_next: u32,
    prev: u32,
    next: u32,
}

impl Entry {
    fn data(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(phys::phys_to_virt(self.frame), BLOCK) }
    }
}

struct Inner {
    buckets: Vec<u32>,
    e: Vec<Entry>,
    free: Vec<u32>,
    /// Most recently used, least recently used.
    head: u32,
    tail: u32,
    count: usize,
    /// Dirty blocks, in disk order.
    dirty: BTreeSet<u64>,
    /// The block after the last miss: a miss there is sequential.
    next_miss: u64,
}

pub struct Cache {
    dev: Arc<dyn BlockDev>,
    kind: CacheKind,
    /// Writes may wait for the lazy writer.
    lazy: AtomicBool,
    /// The medium went away: nothing is kept any more.
    gone: AtomicBool,
    /// The lazy writer is at work: its requests do not make the disk busy.
    writing: AtomicBool,
    /// When the disk last had a request (ms).
    last_io: AtomicU64,
    /// A lazy write failed: when to try again (ms).
    retry_at: AtomicU64,
    inner: Mutex<Inner>,
}

fn hash(block: u64) -> usize {
    (block.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) as usize % BUCKETS
}

/// The bits of sectors `first..first + n` of a block.
fn mask(first: usize, n: usize) -> u8 {
    (((1u32 << n) - 1) << first) as u8
}

impl Inner {
    fn new() -> Inner {
        Inner {
            buckets: alloc::vec![NIL; BUCKETS],
            e: Vec::new(),
            free: Vec::new(),
            head: NIL,
            tail: NIL,
            count: 0,
            dirty: BTreeSet::new(),
            next_miss: u64::MAX,
        }
    }

    fn find(&self, block: u64) -> Option<u32> {
        let mut i = self.buckets[hash(block)];
        while i != NIL {
            let e = &self.e[i as usize];
            if e.block == block {
                return Some(i);
            }
            i = e.hash_next;
        }
        None
    }

    fn unlink(&mut self, i: u32) {
        let (p, n) = (self.e[i as usize].prev, self.e[i as usize].next);
        if p == NIL { self.head = n } else { self.e[p as usize].next = n }
        if n == NIL { self.tail = p } else { self.e[n as usize].prev = p }
    }

    fn push_front(&mut self, i: u32) {
        let h = self.head;
        let e = &mut self.e[i as usize];
        e.prev = NIL;
        e.next = h;
        if h == NIL { self.tail = i } else { self.e[h as usize].prev = i }
        self.head = i;
    }

    fn touch(&mut self, i: u32) {
        if self.head != i {
            self.unlink(i);
            self.push_front(i);
        }
    }

    /// Put a frame in as `block`, which is not there yet.
    fn insert(&mut self, block: u64, frame: u64) -> u32 {
        let b = hash(block);
        let e = Entry { block, frame, valid: 0, dirty: 0, dirtied: 0, written: 0, hash_next: self.buckets[b], prev: NIL, next: NIL };
        let i = match self.free.pop() {
            Some(i) => {
                self.e[i as usize] = e;
                i
            }
            None => {
                self.e.push(e);
                self.e.len() as u32 - 1
            }
        };
        self.buckets[b] = i;
        self.push_front(i);
        self.count += 1;
        i
    }

    /// Take block `i` out: its frame.
    fn remove(&mut self, i: u32) -> u64 {
        self.unlink(i);
        let (block, next, frame) = {
            let e = &self.e[i as usize];
            (e.block, e.hash_next, e.frame)
        };
        let b = hash(block);
        if self.buckets[b] == i {
            self.buckets[b] = next;
        } else {
            let mut p = self.buckets[b];
            while self.e[p as usize].hash_next != i {
                p = self.e[p as usize].hash_next;
            }
            self.e[p as usize].hash_next = next;
        }
        self.dirty.remove(&block);
        self.free.push(i);
        self.count -= 1;
        frame
    }

    /// The least recently used clean block, looking `limit` blocks back.
    fn clean_tail(&self, limit: usize) -> Option<u32> {
        let mut i = self.tail;
        for _ in 0..limit {
            if i == NIL {
                return None;
            }
            if self.e[i as usize].dirty == 0 {
                return Some(i);
            }
            i = self.e[i as usize].prev;
        }
        None
    }

    fn set_clean(&mut self, i: u32) {
        let e = &mut self.e[i as usize];
        e.dirty = 0;
        let block = e.block;
        self.dirty.remove(&block);
    }

    fn set_dirty(&mut self, i: u32, m: u8, now: u64) {
        let e = &mut self.e[i as usize];
        if e.dirty == 0 {
            e.dirtied = now;
            let block = e.block;
            self.dirty.insert(block);
        }
        let e = &mut self.e[i as usize];
        e.dirty |= m;
        e.written = now;
    }
}

/// This volume's fair share of the budget.
fn share() -> usize {
    let n = CACHES.lock().len().max(1) as u64;
    (budget() / n) as usize
}

/// DirtyMax: four fifths of a share, as IBM's HPFS has of its buffers.
fn dirty_max() -> usize {
    share() * 8 / 10
}

/// Frames from a volume over its share, if one is not busy.
fn steal(me: *const Cache) -> Vec<u64> {
    let others: Vec<Arc<Cache>> = CACHES.lock().iter().filter_map(|w| w.upgrade()).filter(|c| Arc::as_ptr(c) != me).collect();
    let limit = share();
    for c in others {
        let Some(mut g) = c.inner.try_lock() else { continue };
        if g.count <= limit {
            continue;
        }
        let mut got = Vec::new();
        while got.len() < STEAL && g.count > limit {
            let Some(i) = g.clean_tail(64) else { break };
            got.push(g.remove(i));
        }
        if !got.is_empty() {
            return got;
        }
    }
    Vec::new()
}

impl Cache {
    fn new(dev: Arc<dyn BlockDev>, kind: CacheKind) -> Arc<Cache> {
        let c = Arc::new(Cache {
            dev,
            kind,
            lazy: AtomicBool::new(false),
            gone: AtomicBool::new(false),
            writing: AtomicBool::new(false),
            last_io: AtomicU64::new(0),
            retry_at: AtomicU64::new(0),
            inner: Mutex::new(Inner::new()),
        });
        let mut l = CACHES.lock();
        l.retain(|w| w.strong_count() > 0);
        l.push(Arc::downgrade(&c));
        c
    }

    fn note_io(&self) {
        if !self.writing.load(Ordering::Relaxed) {
            self.last_io.store(crate::rtc::mono_ms(), Ordering::Relaxed);
        }
    }

    fn dev_read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.note_io();
        let t = super::iostat::begin();
        let r = self.dev.read(lba, buf);
        super::iostat::dev_read(t, (buf.len() / SECTOR) as u64);
        r
    }

    fn dev_write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        self.note_io();
        let t = super::iostat::begin();
        let r = self.dev.write(lba, buf);
        super::iostat::dev_write(t, (buf.len() / SECTOR) as u64);
        r
    }

    /// A frame for a new block, or None: then the request goes uncached.
    fn frame(&self, g: &mut Inner, spare: &mut Vec<u64>) -> Option<u64> {
        if let Some(f) = spare.pop() {
            return Some(f);
        }
        if USED.load(Ordering::Relaxed) < budget() {
            if let Some(f) = phys::alloc_user() {
                USED.fetch_add(1, Ordering::Relaxed);
                return Some(f);
            }
        }
        if g.count < share() {
            *spare = steal(self);
            if let Some(f) = spare.pop() {
                return Some(f);
            }
        }
        if g.count >= MIN_BLOCKS {
            if let Some(i) = g.clean_tail(256) {
                return Some(g.remove(i));
            }
            // All dirty at the old end: write some out.
            if self.write_oldest(g, 32).is_ok() {
                if let Some(i) = g.clean_tail(256) {
                    return Some(g.remove(i));
                }
            }
            return None;
        }
        let f = phys::alloc_user()?;
        USED.fetch_add(1, Ordering::Relaxed);
        Some(f)
    }

    fn give_back(spare: Vec<u64>) {
        for f in spare {
            phys::free(f);
            USED.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Read `run` blocks from `block`, keeping those not there yet; valid
    /// sectors of blocks that are there stay as they are.
    fn fill(&self, g: &mut Inner, block: u64, run: u64) -> Result<(), u32> {
        let total = self.dev.sector_count();
        let lba = block * SPB;
        let sectors = (run * SPB).min(total.saturating_sub(lba));
        if sectors == 0 {
            return Err(crate::api::err::INVALID_PARAMETER);
        }
        let mut b = alloc::vec![0u8; sectors as usize * SECTOR];
        self.dev_read(lba, &mut b)?;
        let mut spare = Vec::new();
        for (k, data) in b.chunks(BLOCK).enumerate() {
            let at = block + k as u64;
            let i = match g.find(at) {
                Some(i) => i,
                None => match self.frame(g, &mut spare) {
                    Some(f) => g.insert(at, f),
                    None => break,
                },
            };
            let e = &mut g.e[i as usize];
            let dst = e.data();
            for s in 0..data.len() / SECTOR {
                if e.valid & 1 << s == 0 {
                    dst[s * SECTOR..(s + 1) * SECTOR].copy_from_slice(&data[s * SECTOR..(s + 1) * SECTOR]);
                    e.valid |= 1 << s;
                }
            }
        }
        Self::give_back(spare);
        Ok(())
    }

    /// Blocks to read for a miss at `block`.
    fn run_for(&self, g: &mut Inner, block: u64) -> u64 {
        let mut run = 1;
        if block == g.next_miss {
            while run < READ_AHEAD && g.find(block + run).is_none() {
                run += 1;
            }
        }
        g.next_miss = block + run;
        run
    }

    /// A request too large to keep: the device's sectors, with what is
    /// cached on top (it may be newer).
    fn read_past(&self, g: &mut Inner, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.dev_read(lba, buf)?;
        let n = (buf.len() / SECTOR) as u64;
        for block in lba / SPB..=(lba + n - 1) / SPB {
            let Some(i) = g.find(block) else { continue };
            let e = &g.e[i as usize];
            let src = e.data();
            for s in 0..SPB {
                let at = block * SPB + s;
                if at >= lba && at < lba + n && e.dirty & 1 << s != 0 {
                    let o = (at - lba) as usize * SECTOR;
                    let f = s as usize * SECTOR;
                    buf[o..o + SECTOR].copy_from_slice(&src[f..f + SECTOR]);
                }
            }
        }
        Ok(())
    }

    /// Forget sectors `lba..lba + n`, which the device has newer.
    fn forget(&self, g: &mut Inner, lba: u64, n: u64) {
        for block in lba / SPB..=(lba + n - 1) / SPB {
            let Some(i) = g.find(block) else { continue };
            let first = lba.max(block * SPB) - block * SPB;
            let last = (lba + n).min((block + 1) * SPB) - block * SPB;
            let m = mask(first as usize, (last - first) as usize);
            let e = &mut g.e[i as usize];
            e.valid &= !m;
            e.dirty &= !m;
            if e.dirty == 0 {
                g.dirty.remove(&block);
            }
        }
    }

    /// Write the dirty sectors of `blocks` (sorted), merging neighbours.
    fn write_blocks(&self, g: &mut Inner, blocks: &[u32]) -> Result<(), u32> {
        const MAX: usize = 128;
        let mut b: Vec<u8> = Vec::with_capacity(MAX * SECTOR);
        let (mut start, mut done): (u64, Vec<u32>) = (0, Vec::new());
        for &i in blocks {
            let (block, dirty, src) = (g.e[i as usize].block, g.e[i as usize].dirty, g.e[i as usize].data());
            for s in 0..SPB {
                if dirty & 1 << s == 0 {
                    continue;
                }
                let lba = block * SPB + s;
                if !b.is_empty() && (lba != start + (b.len() / SECTOR) as u64 || b.len() == MAX * SECTOR) {
                    self.dev_write(start, &b)?;
                    b.clear();
                    done.drain(..).for_each(|d| g.set_clean(d));
                }
                if b.is_empty() {
                    start = lba;
                }
                b.extend_from_slice(&src[s as usize * SECTOR..(s as usize + 1) * SECTOR]);
            }
            done.push(i);
        }
        if !b.is_empty() {
            self.dev_write(start, &b)?;
        }
        done.into_iter().for_each(|d| g.set_clean(d));
        Ok(())
    }

    /// The dirty blocks, in disk order.
    fn dirty_blocks(g: &Inner) -> Vec<u32> {
        g.dirty.iter().filter_map(|&b| g.find(b)).collect()
    }

    /// Write out the `n` blocks that have been dirty longest.
    fn write_oldest(&self, g: &mut Inner, n: usize) -> Result<(), u32> {
        let mut v = Self::dirty_blocks(g);
        v.sort_unstable_by_key(|&i| g.e[i as usize].dirtied);
        v.truncate(n);
        v.sort_unstable_by_key(|&i| g.e[i as usize].block);
        self.write_blocks(g, &v)
    }

    /// Write out everything dirty.
    pub fn sync(&self) -> Result<(), u32> {
        let mut g = self.inner.lock();
        if g.dirty.is_empty() {
            return Ok(());
        }
        let v = Self::dirty_blocks(&g);
        self.write_blocks(&mut g, &v)
    }

    /// Write out what is dirty and drop every block, in one critical
    /// section: a block dirtied between a separate sync and invalidate (a
    /// write on another CPU) would be lost.
    pub fn sync_invalidate(&self) -> Result<(), u32> {
        let mut g = self.inner.lock();
        let r = if g.dirty.is_empty() {
            Ok(())
        } else {
            let v = Self::dirty_blocks(&g);
            self.write_blocks(&mut g, &v)
        };
        let mut spare = Vec::with_capacity(g.count);
        while g.tail != NIL {
            let t = g.tail;
            spare.push(g.remove(t));
        }
        g.next_miss = u64::MAX;
        Self::give_back(spare);
        r
    }

    /// Drop every block, dirty or not.
    pub fn invalidate(&self) {
        let mut g = self.inner.lock();
        let mut spare = Vec::with_capacity(g.count);
        while g.tail != NIL {
            let t = g.tail;
            spare.push(g.remove(t));
        }
        g.next_miss = u64::MAX;
        Self::give_back(spare);
    }

    /// The medium is gone: nothing cached may be seen again, and what was
    /// not written is lost.
    pub fn detach(&self) {
        self.gone.store(true, Ordering::Relaxed);
        let lost = self.inner.lock().dirty.len();
        if lost != 0 {
            crate::println!("cache: {lost} blocks not written: the medium was removed");
        }
        self.invalidate();
    }

    /// One look by the lazy writer.
    fn lazy_pass(&self) {
        let (idle, age, buffer_idle) = *TIMES.lock();
        loop {
            let now = crate::rtc::mono_ms();
            if now < self.retry_at.load(Ordering::Relaxed) {
                return;
            }
            let mut g = self.inner.lock();
            if g.dirty.is_empty() {
                return;
            }
            let quiet = now.saturating_sub(self.last_io.load(Ordering::Relaxed)) >= idle as u64;
            let force = quiet || g.dirty.len() > dirty_max();
            let due = |e: &Entry| force || now.saturating_sub(e.written) >= buffer_idle as u64 || now.saturating_sub(e.dirtied) >= age as u64;
            let mut v = Self::dirty_blocks(&g);
            v.retain(|&i| due(&g.e[i as usize]));
            v.truncate(BATCH);
            if v.is_empty() {
                return;
            }
            self.writing.store(true, Ordering::Relaxed);
            let r = self.write_blocks(&mut g, &v);
            self.writing.store(false, Ordering::Relaxed);
            if let Err(e) = r {
                crate::println!("cache: lazy write failed ({e}), {} blocks wait", g.dirty.len());
                self.retry_at.store(now + 5000, Ordering::Relaxed);
                return;
            }
            if v.len() < BATCH {
                return;
            }
        }
    }

    /// May this volume's writes wait? The file system says it can live
    /// with blocks reaching the disk in another order.
    pub fn allow_lazy(&self) -> bool {
        let lazy = self.kind == CacheKind::Lazy;
        if lazy {
            self.lazy.store(true, Ordering::Relaxed);
        }
        lazy
    }
}

impl BlockDev for Cache {
    fn sector_size(&self) -> usize {
        self.dev.sector_size()
    }

    fn sector_count(&self) -> u64 {
        self.dev.sector_count()
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        if buf.is_empty() || buf.len() % SECTOR != 0 || self.gone.load(Ordering::Relaxed) {
            return self.dev.read(lba, buf);
        }
        let mut g = self.inner.lock();
        if buf.len() as u64 > RECORD.load(Ordering::Relaxed) {
            return self.read_past(&mut g, lba, buf);
        }
        let n = (buf.len() / SECTOR) as u64;
        let mut at = lba;
        while at < lba + n {
            let block = at / SPB;
            let first = (at % SPB) as usize;
            let count = ((SPB - first as u64).min(lba + n - at)) as usize;
            let m = mask(first, count);
            let o = (at - lba) as usize * SECTOR;
            let dst = &mut buf[o..o + count * SECTOR];
            let hit = g.find(block).filter(|&i| g.e[i as usize].valid & m == m);
            let i = match hit {
                Some(i) => Some(i),
                None => {
                    let run = self.run_for(&mut g, block);
                    if self.fill(&mut g, block, run).is_err() {
                        // A bad sector, perhaps not one asked for.
                        self.dev_read(at, dst)?;
                        at += count as u64;
                        continue;
                    }
                    g.find(block).filter(|&i| g.e[i as usize].valid & m == m)
                }
            };
            match i {
                Some(i) => {
                    let s = first * SECTOR;
                    dst.copy_from_slice(&g.e[i as usize].data()[s..s + count * SECTOR]);
                    g.touch(i);
                }
                None => self.dev_read(at, dst)?,
            }
            at += count as u64;
        }
        Ok(())
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        if buf.is_empty() || buf.len() % SECTOR != 0 || self.gone.load(Ordering::Relaxed) {
            return self.dev.write(lba, buf);
        }
        let n = (buf.len() / SECTOR) as u64;
        let mut g = self.inner.lock();
        if buf.len() as u64 > RECORD.load(Ordering::Relaxed) {
            let r = self.dev_write(lba, buf);
            self.forget(&mut g, lba, n);
            return r;
        }
        let lazy = self.lazy.load(Ordering::Relaxed) && LAZY.load(Ordering::Relaxed);
        if !lazy {
            if let Err(e) = self.dev_write(lba, buf) {
                self.forget(&mut g, lba, n);
                return Err(e);
            }
        }
        let now = crate::rtc::mono_ms();
        let mut spare = Vec::new();
        let mut at = lba;
        while at < lba + n {
            let block = at / SPB;
            let first = (at % SPB) as usize;
            let count = ((SPB - first as u64).min(lba + n - at)) as usize;
            let m = mask(first, count);
            let o = (at - lba) as usize * SECTOR;
            let src = &buf[o..o + count * SECTOR];
            let i = match g.find(block) {
                Some(i) => Some(i),
                None => self.frame(&mut g, &mut spare).map(|f| g.insert(block, f)),
            };
            match i {
                Some(i) => {
                    let e = &mut g.e[i as usize];
                    e.data()[first * SECTOR..(first + count) * SECTOR].copy_from_slice(src);
                    e.valid |= m;
                    if lazy {
                        g.set_dirty(i, m, now);
                    }
                    g.touch(i);
                }
                // No frame: only with lazy writing does it matter.
                None if lazy => self.dev_write(at, src)?,
                None => {}
            }
            at += count as u64;
        }
        Self::give_back(spare);
        // Too much waiting: the writer of it waits now.
        if g.dirty.len() > dirty_max() {
            self.write_oldest(&mut g, BATCH)?;
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), u32> {
        self.sync()?;
        self.dev.flush()
    }

    fn usb_device(&self) -> Option<crate::drivers::xhci::Handle> {
        self.dev.usb_device()
    }

    fn packet(&self, cdb: &[u8], buf: &mut [u8]) -> Result<usize, u32> {
        self.dev.packet(cdb, buf)
    }

    fn cache(&self) -> Option<&Cache> {
        Some(self)
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        let _ = self.sync();
        self.invalidate();
    }
}

/// `nocache` on the kernel command line: no volume gets a cache, for
/// comparing.
pub static DISABLED: AtomicBool = AtomicBool::new(false);

/// `dev` behind a cache, if its kind of device has one.
pub fn wrap(dev: Arc<dyn BlockDev>) -> Arc<dyn BlockDev> {
    if dev.cache_kind() == CacheKind::None || dev.sector_size() != SECTOR || DISABLED.load(Ordering::Relaxed) {
        return dev;
    }
    let kind = dev.cache_kind();
    Cache::new(dev, kind)
}

fn all() -> Vec<Arc<Cache>> {
    CACHES.lock().iter().filter_map(|w| w.upgrade()).collect()
}

/// Write out what every volume has dirty.
pub fn sync_all() {
    for c in all() {
        if let Err(e) = c.sync() {
            crate::println!("cache: write failed ({e})");
        }
    }
}

/// DiskIdle, MaxAge, BufferIdle, in milliseconds.
pub fn times() -> (u32, u32, u32) {
    *TIMES.lock()
}

/// `CACHE -DISKIDLE`, `-MAXAGE`, `-BUFFERIDLE`: new times, and everything
/// out, as IBM's HPFS does on any change of the parameters.
pub fn set_times(t: (u32, u32, u32)) {
    *TIMES.lock() = t;
    sync_all();
}

pub fn lazy_on() -> bool {
    LAZY.load(Ordering::Relaxed)
}

/// Turn lazy writing on or off. Off: from now on writes go to the disk.
pub fn set_lazy(on: bool) {
    LAZY.store(on, Ordering::Relaxed);
    if !on {
        sync_all();
    }
}

/// What `CACHE` shows: DirtyMax and the cache size, in blocks of 2 KB as
/// in IBM's HPFS (it counts in 16 bits).
pub fn sizes() -> (u16, u16) {
    let two_kb = |blocks: u64| (blocks * 2).min(65535) as u16;
    (two_kb(budget() * 8 / 10), two_kb(budget()))
}

/// Start the lazy writer.
pub fn start() {
    crate::task::kthread::spawn("lazywrite", || loop {
        crate::task::sched::sleep_ms(PERIOD);
        if lazy_on() {
            for c in all() {
                c.lazy_pass();
            }
        }
    });
}

/// Forget every volume's blocks.
pub fn invalidate_all() {
    for c in all() {
        if let Err(e) = c.sync_invalidate() {
            crate::println!("cache: write failed ({e})");
        }
    }
}
