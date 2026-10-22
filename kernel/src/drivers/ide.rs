//! PCI IDE controllers (class 1, subclass 1): ATA disks and ATAPI CD/DVD
//! drives on the primary and secondary channels, master and slave, as
//! QEMU/Proxmox (PIIX3/4, the default for CD drives there) and VMware
//! (PIIX4) present them.
//!
//! Programmed I/O, polled, interrupts off (nIEN), one command at a time per
//! channel. Each channel uses the legacy ports (0x1F0/0x3F6, 0x170/0x376)
//! unless the controller runs it in native mode (programming interface
//! bits 0 and 2), when BARs 0-3 give them. Disks: READ/WRITE SECTORS (EXT
//! for LBA48), FLUSH CACHE (EXT); CD drives: PACKET commands with READ(10),
//! READ CAPACITY and REQUEST SENSE after a failure (a unit attention after
//! a disc change is cleared that way); any other PACKET command with data
//! in for CD audio (`BlockDev::packet`).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::pci;
use crate::api::err;
use crate::arch::{inb, inw, outb, outw};
use crate::fs::block::BlockDev;
use crate::sync::Mutex;

const DATA: u16 = 0;
const ERROR: u16 = 1;
const FEATURES: u16 = 1;
const COUNT: u16 = 2;
const LBA0: u16 = 3;
const LBA1: u16 = 4;
const LBA2: u16 = 5;
const DEVICE: u16 = 6;
const STATUS: u16 = 7;
const COMMAND: u16 = 7;

const BSY: u8 = 0x80;
const DRDY: u8 = 0x40;
const DF: u8 = 0x20;
const DRQ: u8 = 0x08;
const ERR: u8 = 0x01;

const TIMEOUT_MS: u64 = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ata,
    Atapi,
}

struct Channel {
    cmd: u16,
    ctl: u16,
}

impl Channel {
    fn rd(&self, r: u16) -> u8 {
        unsafe { inb(self.cmd + r) }
    }

    fn wr(&self, r: u16, v: u8) {
        unsafe { outb(self.cmd + r, v) }
    }

    fn alt_status(&self) -> u8 {
        unsafe { inb(self.ctl) }
    }

    /// The 400 ns a status read needs to settle after a command.
    fn settle(&self) {
        for _ in 0..4 {
            self.alt_status();
        }
    }

    fn wait_not_busy(&self) -> Result<u8, u32> {
        let mut st = 0;
        if !crate::msi::poll(TIMEOUT_MS, || self.settle(), || {
            st = self.alt_status();
            st & BSY == 0
        }) {
            return Err(err::NOT_READY);
        }
        Ok(st)
    }

    /// Wait for DRQ (data to move) or an error.
    fn wait_drq(&self) -> Result<(), u32> {
        let mut st = 0;
        if !crate::msi::poll(TIMEOUT_MS, || self.settle(), || {
            st = self.alt_status();
            st & BSY == 0 && (st & (DRQ | ERR | DF) != 0)
        }) {
            return Err(err::NOT_READY);
        }
        if st & (ERR | DF) != 0 {
            return Err(err::NOT_READY);
        }
        Ok(())
    }

    fn select(&self, slave: bool, lba_bits: u8) -> Result<(), u32> {
        self.wait_not_busy()?;
        self.wr(DEVICE, 0xa0 | 0x40 | if slave { 0x10 } else { 0 } | (lba_bits & 0x0f));
        self.settle();
        self.wait_not_busy().map(|_| ())
    }

    fn read_words(&self, buf: &mut [u8]) {
        for c in buf.chunks_exact_mut(2) {
            let w = unsafe { inw(self.cmd + DATA) };
            c.copy_from_slice(&w.to_le_bytes());
        }
    }

    fn write_words(&self, buf: &[u8]) {
        for c in buf.chunks_exact(2) {
            unsafe { outw(self.cmd + DATA, u16::from_le_bytes([c[0], c[1]])) };
        }
    }

    /// IDENTIFY (DEVICE or PACKET DEVICE): the 256 words, or None.
    fn identify(&self, slave: bool, cmd: u8) -> Option<[u16; 256]> {
        self.select(slave, 0).ok()?;
        self.wr(COMMAND, cmd);
        self.settle();
        if self.alt_status() == 0 {
            return None; // no device
        }
        self.wait_drq().ok()?;
        let mut b = [0u8; 512];
        self.read_words(&mut b);
        let mut w = [0u16; 256];
        for (i, c) in b.chunks_exact(2).enumerate() {
            w[i] = u16::from_le_bytes([c[0], c[1]]);
        }
        Some(w)
    }

    /// An ATAPI PACKET command reading up to `buf.len()` bytes (`buf`
    /// empty: no data). Returns the bytes read.
    fn packet(&self, slave: bool, pk: &[u8; 12], buf: &mut [u8]) -> Result<usize, u32> {
        self.select(slave, 0)?;
        self.wr(FEATURES, 0); // PIO
        let limit = buf.len().clamp(2, 0xfffe) as u16 & !1;
        self.wr(LBA1, limit as u8);
        self.wr(LBA2, (limit >> 8) as u8);
        self.wr(COMMAND, 0xa0);
        self.settle();
        self.wait_drq()?;
        self.write_words(pk);
        let mut got = 0;
        loop {
            self.settle();
            let st = self.wait_not_busy()?;
            if st & (ERR | DF) != 0 {
                return Err(err::NOT_READY);
            }
            if st & DRQ == 0 {
                break;
            }
            let n = (self.rd(LBA1) as usize | (self.rd(LBA2) as usize) << 8) & !1;
            let take = n.min(buf.len() - got);
            self.read_words(&mut buf[got..got + take]);
            // Anything the drive sends beyond the buffer is dropped.
            for _ in 0..(n - take) / 2 {
                unsafe { inw(self.cmd + DATA) };
            }
            got += take;
        }
        Ok(got)
    }

    /// An ATA command with sector data in or out (PIO), LBA28 or LBA48.
    fn ata_rw(&self, slave: bool, lba: u64, n: usize, buf: &mut [u8], write: bool, lba48: bool) -> Result<(), u32> {
        if lba48 {
            self.select(slave, 0)?;
            self.wr(COUNT, (n >> 8) as u8);
            self.wr(LBA0, (lba >> 24) as u8);
            self.wr(LBA1, (lba >> 32) as u8);
            self.wr(LBA2, (lba >> 40) as u8);
            self.wr(COUNT, n as u8);
            self.wr(LBA0, lba as u8);
            self.wr(LBA1, (lba >> 8) as u8);
            self.wr(LBA2, (lba >> 16) as u8);
            self.wr(COMMAND, if write { 0x34 } else { 0x24 });
        } else {
            self.select(slave, (lba >> 24) as u8)?;
            self.wr(COUNT, n as u8);
            self.wr(LBA0, lba as u8);
            self.wr(LBA1, (lba >> 8) as u8);
            self.wr(LBA2, (lba >> 16) as u8);
            self.wr(COMMAND, if write { 0x30 } else { 0x20 });
        }
        for s in 0..n {
            self.settle();
            self.wait_drq()?;
            let sec = &mut buf[s * 512..(s + 1) * 512];
            if write {
                self.write_words(sec);
            } else {
                self.read_words(sec);
            }
        }
        self.settle();
        let st = self.wait_not_busy()?;
        if st & (ERR | DF) != 0 { Err(err::NOT_READY) } else { Ok(()) }
    }

    /// A command without data (FLUSH CACHE).
    fn ata_nodata(&self, slave: bool, cmd: u8) -> Result<(), u32> {
        self.select(slave, 0)?;
        self.wr(COMMAND, cmd);
        self.settle();
        let st = self.wait_not_busy()?;
        if st & (ERR | DF) != 0 { Err(err::NOT_READY) } else { Ok(()) }
    }
}

/// A drive on an IDE channel.
pub struct Drive {
    pub kind: Kind,
    pub model: String,
    pub sector_size: usize,
    pub sectors: u64,
    slave: bool,
    lba48: bool,
    flush_ext: bool,
    chan: Arc<Mutex<Channel>>,
}

unsafe impl Send for Drive {}
unsafe impl Sync for Drive {}

fn ata_string(w: &[u16]) -> String {
    let mut s = String::new();
    for x in w {
        for b in [(x >> 8) as u8, *x as u8] {
            if b != 0 {
                s.push(b as char);
            }
        }
    }
    String::from(s.trim())
}

fn read10(lba: u64, n: u16) -> [u8; 12] {
    let mut pk = [0u8; 12];
    pk[0] = 0x28;
    pk[2..6].copy_from_slice(&(lba as u32).to_be_bytes());
    pk[7..9].copy_from_slice(&n.to_be_bytes());
    pk
}

impl Drive {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: bool) -> Result<(), u32> {
        let ss = self.sector_size;
        if buf.len() % ss != 0 {
            return Err(err::INVALID_PARAMETER);
        }
        let c = self.chan.lock();
        let total = buf.len() / ss;
        let mut done = 0;
        while done < total {
            match self.kind {
                Kind::Ata => {
                    let n = (total - done).min(if self.lba48 { 256 } else { 128 });
                    let at = lba + done as u64;
                    c.ata_rw(self.slave, at, n, &mut buf[done * ss..(done + n) * ss], write, self.lba48)?;
                    done += n;
                }
                Kind::Atapi => {
                    if write {
                        return Err(err::WRITE_PROTECT);
                    }
                    let n = (total - done).min(16);
                    let at = lba + done as u64;
                    let chunk = &mut buf[done * ss..(done + n) * ss];
                    match c.packet(self.slave, &read10(at, n as u16), chunk) {
                        Ok(got) if got == chunk.len() => {}
                        r => {
                            // REQUEST SENSE clears a unit attention (a disc
                            // was put in or changed), so a retry can work.
                            let mut rs = [0u8; 12];
                            rs[0] = 0x03;
                            rs[4] = 18;
                            let mut sense = [0u8; 18];
                            let _ = c.packet(self.slave, &rs, &mut sense);
                            return Err(r.err().unwrap_or(err::NOT_READY));
                        }
                    }
                    done += n;
                }
            }
        }
        Ok(())
    }
}

impl BlockDev for Drive {
    fn sector_size(&self) -> usize {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), u32> {
        self.transfer(lba, buf, false)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), u32> {
        let mut tmp = buf.to_vec();
        self.transfer(lba, &mut tmp, true)
    }

    fn flush(&self) -> Result<(), u32> {
        if self.kind != Kind::Ata {
            return Ok(());
        }
        let c = self.chan.lock();
        c.ata_nodata(self.slave, if self.flush_ext { 0xea } else { 0xe7 })
    }

    fn packet(&self, cdb: &[u8], buf: &mut [u8]) -> Result<usize, u32> {
        if self.kind != Kind::Atapi || cdb.len() > 12 {
            return Err(err::NOT_SUPPORTED);
        }
        let mut pk = [0u8; 12];
        pk[..cdb.len()].copy_from_slice(cdb);
        let c = self.chan.lock();
        c.packet(self.slave, &pk, buf).inspect_err(|_| {
            // Clear the sense (a unit attention after a disc change).
            let mut rs = [0u8; 12];
            rs[0] = 0x03;
            rs[4] = 18;
            let mut sense = [0u8; 18];
            let _ = c.packet(self.slave, &rs, &mut sense);
        })
    }
}

/// The drives of a PCI IDE controller.
pub fn probe(dev: &pci::Device) -> Vec<Arc<Drive>> {
    dev.enable();
    let mut out = Vec::new();
    for ch in 0..2u8 {
        let native = dev.progif & (1 << (ch * 2)) != 0;
        let (cmd, ctl) = if native {
            (dev.bar(ch * 2) as u16, dev.bar(ch * 2 + 1) as u16 + 2)
        } else if ch == 0 {
            (0x1f0, 0x3f6)
        } else {
            (0x170, 0x376)
        };
        if cmd == 0 {
            continue;
        }
        let c = Channel { cmd, ctl };
        // nIEN: no interrupts, everything is polled.
        unsafe { outb(ctl, 0x02) };
        // A floating bus reads 0xFF: no drives on this channel.
        if c.rd(STATUS) == 0xff {
            continue;
        }
        let chan = Arc::new(Mutex::new(c));
        for slave in [false, true] {
            let c = chan.lock();
            let (kind, w) = match c.identify(slave, 0xec) {
                Some(w) => (Kind::Ata, w),
                None => {
                    // An ATAPI device aborts IDENTIFY DEVICE and leaves its
                    // signature (0x14, 0xEB) in the LBA registers.
                    if (c.rd(LBA1), c.rd(LBA2)) != (0x14, 0xeb) {
                        continue;
                    }
                    match c.identify(slave, 0xa1) {
                        Some(w) => (Kind::Atapi, w),
                        None => continue,
                    }
                }
            };
            let model = ata_string(&w[27..47]);
            let lba48 = w[83] & (1 << 10) != 0;
            let flush_ext = w[83] & (1 << 13) != 0;
            let (sector_size, sectors) = match kind {
                Kind::Ata if w[49] & (1 << 9) == 0 => continue, // no LBA
                Kind::Ata if lba48 => (512, w[100] as u64 | (w[101] as u64) << 16 | (w[102] as u64) << 32 | (w[103] as u64) << 48),
                Kind::Ata => (512, w[60] as u64 | (w[61] as u64) << 16),
                Kind::Atapi => {
                    // READ CAPACITY(10); no disc leaves the size at 0 (after
                    // the unit attention of a fresh drive is cleared).
                    let mut pk = [0u8; 12];
                    pk[0] = 0x25;
                    let mut cap = [0u8; 8];
                    let mut n = 0u64;
                    for _ in 0..2 {
                        if c.packet(slave, &pk, &mut cap) == Ok(8) {
                            n = u32::from_be_bytes([cap[0], cap[1], cap[2], cap[3]]) as u64 + 1;
                            break;
                        }
                        let mut rs = [0u8; 12];
                        rs[0] = 0x03;
                        rs[4] = 18;
                        let mut sense = [0u8; 18];
                        let _ = c.packet(slave, &rs, &mut sense);
                    }
                    (2048, n)
                }
            };
            drop(c);
            crate::println!(
                "ide {:02x}:{:02x}.{}: {} {}: {} {model}, {} MB",
                dev.bus,
                dev.dev,
                dev.func,
                if ch == 0 { "primary" } else { "secondary" },
                if slave { "slave" } else { "master" },
                if kind == Kind::Ata { "disk" } else { "optical" },
                sectors * sector_size as u64 >> 20
            );
            out.push(Arc::new(Drive { kind, model, sector_size, sectors, slave, lba48, flush_ext, chan: chan.clone() }));
        }
    }
    out
}
