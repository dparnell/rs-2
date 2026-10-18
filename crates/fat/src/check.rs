//! Consistency check and repair of a FAT volume (what CHKDSK does).
//!
//! [`Checker::scan`] walks the directory tree, following every cluster
//! chain against an in-memory copy of the FAT. It finds chains that run
//! into free, reserved or out-of-range clusters, loops, cross-linked
//! files, sizes that do not match the allocation, bad `.`/`..` entries and
//! clusters that are allocated but belong to no file (lost chains). With
//! `fix`, chains are truncated, sizes adjusted and directory entries
//! repaired as it goes; [`Checker::recover`] then turns lost chains into
//! `FILEnnnn.CHK` files in the root directory or frees them.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::{attr, name, BlockDevice, Dir, Error, FatType, Result, Timestamp, Volume};

/// Something wrong found on the volume. Paths are relative to the root
/// (`\` separated, 8.3 names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// The first cluster of an entry is out of range; the entry was
    /// (or would be) truncated to length 0.
    BadFirstCluster(String),
    /// A chain runs into a free, reserved or out-of-range cluster, or
    /// loops; it was (or would be) ended at the last good cluster.
    BadChain(String),
    /// The file size does not match its allocation; adjusted.
    SizeAdjusted(String),
    /// Two entries use the same cluster.
    CrossLinked { path: String, cluster: u32 },
    /// A subdirectory's `.` or `..` entry is wrong.
    BadSubdirectory(String),
    /// A directory could not be read.
    UnreadableDirectory(String),
}

/// Totals for the report, in clusters.
#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub dirs: u32,
    pub dir_clusters: u32,
    pub files: u32,
    pub file_clusters: u32,
    pub hidden: u32,
    pub hidden_clusters: u32,
    /// Clusters of the OS/2 extended attribute file (`EA DATA. SF`).
    pub ea_clusters: u32,
    pub bad_clusters: u32,
    pub recovered: u32,
    pub recovered_clusters: u32,
    pub lost_clusters: u32,
    pub lost_chains: u32,
}

/// No owner yet.
const FREE_OWNER: u32 = 0;

pub struct Checker<'a, D: BlockDevice> {
    vol: &'a mut Volume<D>,
    fix: bool,
    fat: Vec<u32>,
    /// Owner (entry number + 1) of each cluster.
    owner: Vec<u32>,
    /// Paths of the owners, by entry number.
    paths: Vec<String>,
    pub findings: Vec<Finding>,
    pub totals: Totals,
    /// Lost chains: (first cluster, length).
    lost: Vec<(u32, u32)>,
}

/// A directory entry as found on disk, with its location.
struct Raw {
    name: String,
    attr: u8,
    cluster: u32,
    size: u32,
    lba: u64,
    off: usize,
}

impl<'a, D: BlockDevice> Checker<'a, D> {
    /// Load the first FAT.
    pub fn new(vol: &'a mut Volume<D>, fix: bool) -> Result<Self> {
        let bps = vol.bps as usize;
        let mut raw = vec![0u8; vol.fat_size as usize * bps];
        vol.dev.read(vol.reserved as u64, &mut raw)?;
        let n = vol.clusters as usize + 2;
        let mut fat = vec![0u32; n];
        for (c, v) in fat.iter_mut().enumerate() {
            *v = match vol.fat_type {
                FatType::Fat12 => {
                    let o = c + c / 2;
                    let w = u16::from_le_bytes([raw[o], raw[o + 1]]) as u32;
                    if c & 1 != 0 { w >> 4 } else { w & 0xfff }
                }
                FatType::Fat16 => u16::from_le_bytes([raw[c * 2], raw[c * 2 + 1]]) as u32,
                FatType::Fat32 => u32::from_le_bytes(raw[c * 4..c * 4 + 4].try_into().unwrap()) & 0x0fff_ffff,
            };
        }
        Ok(Checker {
            vol,
            fix,
            fat,
            owner: vec![FREE_OWNER; n],
            paths: Vec::new(),
            findings: Vec::new(),
            totals: Totals::default(),
            lost: Vec::new(),
        })
    }

    fn bad_marker(&self) -> u32 {
        self.vol.eoc() - 8
    }

    fn valid(&self, c: u32) -> bool {
        c >= 2 && c < self.vol.clusters + 2
    }

    fn set_fat(&mut self, c: u32, v: u32) -> Result<()> {
        self.fat[c as usize] = v;
        if self.fix { self.vol.set_fat_entry(c, v) } else { Ok(()) }
    }

    /// Patch a directory entry on disk (when fixing).
    fn patch(&mut self, e: &Raw, f: impl FnOnce(&mut [u8])) -> Result<()> {
        if !self.fix {
            return Ok(());
        }
        let mut sec = vec![0u8; self.vol.bps as usize];
        self.vol.dev.read(e.lba, &mut sec)?;
        f(&mut sec[e.off..e.off + 32]);
        self.vol.write_sector(e.lba, &sec)
    }

    fn set_entry_cluster(&mut self, e: &Raw, c: u32) -> Result<()> {
        let fat32 = self.vol.fat_type == FatType::Fat32;
        self.patch(e, |d| {
            d[26..28].copy_from_slice(&(c as u16).to_le_bytes());
            if fat32 {
                d[20..22].copy_from_slice(&((c >> 16) as u16).to_le_bytes());
            }
        })
    }

    fn set_entry_size(&mut self, e: &Raw, size: u32) -> Result<()> {
        self.patch(e, |d| d[28..32].copy_from_slice(&size.to_le_bytes()))
    }

    /// Follow and claim the chain starting at `start` for owner `id`.
    /// Returns the clusters claimed. Ends broken chains (finding reported
    /// by the caller through the flag) and stops at cross-links.
    fn claim(&mut self, start: u32, id: u32, path: &str) -> Result<(Vec<u32>, bool)> {
        let mut chain = Vec::new();
        let mut broken = false;
        let mut c = start;
        loop {
            let o = self.owner[c as usize];
            if o == id {
                // Loop back into this chain: end it at the previous cluster.
                broken = true;
                if let Some(&last) = chain.last() {
                    let eoc = self.vol.eoc();
                    self.set_fat(last, eoc)?;
                }
                break;
            }
            if o != FREE_OWNER {
                // The first owner keeps the shared clusters; this chain
                // ends before them (unless they are its very first).
                self.findings.push(Finding::CrossLinked { path: String::from(path), cluster: c });
                if let Some(&last) = chain.last() {
                    let eoc = self.vol.eoc();
                    self.set_fat(last, eoc)?;
                }
                break;
            }
            self.owner[c as usize] = id;
            chain.push(c);
            let next = self.fat[c as usize];
            if self.vol.is_eoc(next) {
                break;
            }
            if !self.valid(next) || next == self.bad_marker() {
                broken = true;
                let eoc = self.vol.eoc();
                self.set_fat(c, eoc)?;
                break;
            }
            c = next;
        }
        Ok((chain, broken))
    }

    fn new_owner(&mut self, path: &str) -> u32 {
        self.paths.push(String::from(path));
        self.paths.len() as u32
    }

    /// Read the raw entries of a directory stored in `clusters` (or the
    /// fixed root when empty and FAT12/16).
    fn read_entries(&self, clusters: &[u32], fixed_root: bool) -> Result<Vec<Raw>> {
        let bps = self.vol.bps as usize;
        let mut sectors: Vec<u64> = Vec::new();
        if fixed_root {
            sectors.extend((0..self.vol.root_sectors).map(|s| (self.vol.root_start + s) as u64));
        } else {
            for &c in clusters {
                let lba = self.vol.cluster_lba(c);
                sectors.extend((0..self.vol.spc as u64).map(|s| lba + s));
            }
        }
        let mut out = Vec::new();
        let mut sec = vec![0u8; bps];
        let fat32 = self.vol.fat_type == FatType::Fat32;
        'outer: for lba in sectors {
            self.vol.dev.read(lba, &mut sec)?;
            for off in (0..bps).step_by(32) {
                let d = &sec[off..off + 32];
                if d[0] == 0 {
                    break 'outer;
                }
                if d[0] == 0xe5 || d[11] == attr::LFN {
                    continue;
                }
                let hi = if fat32 { u16::from_le_bytes([d[20], d[21]]) as u32 } else { 0 };
                out.push(Raw {
                    name: name::from_83(&d[0..11]),
                    attr: d[11],
                    cluster: u16::from_le_bytes([d[26], d[27]]) as u32 | hi << 16,
                    size: u32::from_le_bytes([d[28], d[29], d[30], d[31]]),
                    lba,
                    off,
                });
            }
        }
        Ok(out)
    }

    /// Check the whole tree.
    pub fn scan(&mut self) -> Result<()> {
        let bad = self.bad_marker();
        self.totals.bad_clusters = self.fat[2..].iter().filter(|&&v| v == bad).count() as u32;
        // (directory clusters, path, parent cluster as stored in "..")
        let mut stack: Vec<(Vec<u32>, String, u32, u32)> = Vec::new();
        let fixed_root = self.vol.fat_type != FatType::Fat32;
        if fixed_root {
            stack.push((Vec::new(), String::new(), 0, 0));
        } else {
            let id = self.new_owner("\\");
            let root = self.vol.root_cluster;
            let (chain, broken) = if self.valid(root) { self.claim(root, id, "\\")? } else { (Vec::new(), true) };
            if broken {
                self.findings.push(Finding::BadChain(String::from("\\")));
            }
            self.totals.dir_clusters += chain.len() as u32;
            stack.push((chain, String::new(), 0, root));
        }
        while let Some((clusters, path, parent, me)) = stack.pop() {
            let is_root = path.is_empty();
            let entries = match self.read_entries(&clusters, is_root && fixed_root) {
                Ok(e) => e,
                Err(_) => {
                    self.findings.push(Finding::UnreadableDirectory(path.clone()));
                    continue;
                }
            };
            for (i, e) in entries.iter().enumerate() {
                if e.attr & attr::VOLUME != 0 {
                    continue;
                }
                if !is_root && i < 2 && (e.name == "." || e.name == "..") {
                    let want = if i == 0 { me } else { parent };
                    let want_name = if i == 0 { "." } else { ".." };
                    if e.name != want_name || e.cluster != want {
                        self.findings.push(Finding::BadSubdirectory(path.clone()));
                        self.set_entry_cluster(e, want)?;
                    }
                    continue;
                }
                if e.name == "." || e.name == ".." {
                    continue;
                }
                let full = alloc::format!("{path}\\{}", e.name);
                let dir = e.attr & attr::DIRECTORY != 0;
                let mut chain = Vec::new();
                if e.cluster != 0 {
                    if !self.valid(e.cluster) {
                        self.findings.push(Finding::BadFirstCluster(full.clone()));
                        self.set_entry_cluster(e, 0)?;
                        if !dir {
                            self.set_entry_size(e, 0)?;
                        }
                    } else {
                        let id = self.new_owner(&full);
                        let (c, broken) = self.claim(e.cluster, id, &full)?;
                        if broken {
                            self.findings.push(Finding::BadChain(full.clone()));
                        }
                        chain = c;
                    }
                }
                let cs = self.vol.cluster_size();
                if dir {
                    if chain.is_empty() {
                        // A directory without clusters cannot hold "." and "..".
                        self.findings.push(Finding::BadSubdirectory(full.clone()));
                        continue;
                    }
                    self.totals.dirs += 1;
                    self.totals.dir_clusters += chain.len() as u32;
                    // ".." of a first-level directory is 0.
                    let pc = if is_root { 0 } else { me };
                    stack.push((chain.clone(), full, pc, chain[0]));
                    continue;
                }
                // Size against allocation.
                let need = e.size.div_ceil(cs) as usize;
                let mut used = chain.len();
                if chain.len() > need && !chain.is_empty() {
                    self.findings.push(Finding::SizeAdjusted(full.clone()));
                    // Free the clusters beyond the size.
                    for &c in &chain[need..] {
                        self.owner[c as usize] = FREE_OWNER;
                        self.set_fat(c, 0)?;
                    }
                    if need == 0 {
                        self.set_entry_cluster(e, 0)?;
                    } else {
                        let eoc = self.vol.eoc();
                        self.set_fat(chain[need - 1], eoc)?;
                    }
                    used = need;
                } else if chain.len() < need {
                    self.findings.push(Finding::SizeAdjusted(full.clone()));
                    self.set_entry_size(e, chain.len() as u32 * cs)?;
                }
                let n = used as u32;
                if e.name == "EA DATA.SF" && is_root {
                    self.totals.ea_clusters += n;
                } else if e.attr & (attr::HIDDEN | attr::SYSTEM) != 0 {
                    self.totals.hidden += 1;
                    self.totals.hidden_clusters += n;
                } else {
                    self.totals.files += 1;
                    self.totals.file_clusters += n;
                }
            }
        }
        self.find_lost();
        Ok(())
    }

    /// Allocated clusters nobody owns, grouped into chains.
    fn find_lost(&mut self) {
        let bad = self.bad_marker();
        let n = self.fat.len();
        let lost = |s: &Self, c: usize| s.owner[c] == FREE_OWNER && s.fat[c] != 0 && s.fat[c] != bad;
        // Clusters that some other lost cluster points to are not heads.
        let mut pointed = vec![false; n];
        for c in 2..n {
            if lost(self, c) {
                let v = self.fat[c] as usize;
                if v < n && v >= 2 {
                    pointed[v] = true;
                }
            }
        }
        let mut seen = vec![false; n];
        let mut chains = Vec::new();
        // Heads first, then whatever is left (cycles).
        for pass in 0..2 {
            for c in 2..n {
                if !lost(self, c) || seen[c] || (pass == 0 && pointed[c]) {
                    continue;
                }
                let mut len = 0;
                let mut k = c;
                while k >= 2 && k < n && lost(self, k) && !seen[k] {
                    seen[k] = true;
                    len += 1;
                    let v = self.fat[k];
                    if self.vol.is_eoc(v) {
                        break;
                    }
                    k = v as usize;
                }
                chains.push((c as u32, len));
            }
        }
        self.totals.lost_chains = chains.len() as u32;
        self.totals.lost_clusters = chains.iter().map(|c| c.1).sum();
        self.lost = chains;
    }

    /// Deal with lost chains: convert them to `FILEnnnn.CHK` in the root
    /// (`convert`) or free them. Only writes when fixing. Returns the
    /// names of the files created.
    pub fn recover(&mut self, convert: bool, now: Timestamp) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let chains = core::mem::take(&mut self.lost);
        let cs = self.vol.cluster_size();
        let mut next = 0;
        for (head, len) in chains {
            // Make each chain well formed: end it after `len` clusters.
            let mut c = head;
            for i in 0..len {
                let v = self.fat[c as usize];
                if i + 1 == len {
                    if !self.vol.is_eoc(v) {
                        let eoc = self.vol.eoc();
                        self.set_fat(c, eoc)?;
                    }
                } else {
                    c = v;
                }
            }
            if convert {
                self.totals.recovered += 1;
                self.totals.recovered_clusters += len;
                if !self.fix {
                    continue;
                }
                loop {
                    let n = alloc::format!("FILE{next:04}.CHK");
                    next += 1;
                    match self.vol.create_file(&n, 0, now) {
                        Ok(mut e) => {
                            e.cluster = head;
                            e.size = len * cs;
                            self.vol.update_entry(&e)?;
                            names.push(n);
                            break;
                        }
                        Err(Error::Exists) if next < 10000 => continue,
                        Err(e) => return Err(e),
                    }
                }
            } else {
                // Free the chain.
                let mut c = head;
                for _ in 0..len {
                    let v = self.fat[c as usize];
                    self.set_fat(c, 0)?;
                    c = v;
                }
            }
        }
        Ok(names)
    }

    /// Free clusters per the (possibly corrected) FAT.
    pub fn free_clusters(&self) -> u32 {
        self.fat[2..].iter().filter(|&&v| v == 0).count() as u32
    }

    pub fn fat_type(&self) -> FatType {
        self.vol.fat_type
    }

    pub fn cluster_size(&self) -> u32 {
        self.vol.cluster_size()
    }

    pub fn total_clusters(&self) -> u32 {
        self.vol.clusters
    }

    /// The root directory of the volume (for callers listing it).
    pub fn root(&self) -> Dir {
        self.vol.root_dir()
    }
}
