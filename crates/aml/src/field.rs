//! Field units on operation regions, index/data pairs and banks, and
//! buffer fields.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::*;

fn get_bits(src: &[u8], off: u64, n: u64) -> u64 {
    let mut v = 0u64;
    for i in 0..n.min(64) {
        let bit = off + i;
        let byte = src.get((bit / 8) as usize).copied().unwrap_or(0);
        if byte >> (bit % 8) & 1 != 0 {
            v |= 1 << i;
        }
    }
    v
}

fn put_bits(dst: &mut [u8], off: u64, n: u64, v: u64) {
    for i in 0..n.min(64) {
        let bit = off + i;
        let Some(byte) = dst.get_mut((bit / 8) as usize) else { return };
        if v >> i & 1 != 0 {
            *byte |= 1 << (bit % 8);
        } else {
            *byte &= !(1 << (bit % 8));
        }
    }
}

fn low_mask(n: u64) -> u64 {
    if n >= 64 { u64::MAX } else { (1u64 << n) - 1 }
}

impl<H: Host> Interp<H> {
    fn int_bits(&self) -> u64 {
        if self.int64 { 64 } else { 32 }
    }

    fn bits_to_obj(&self, bytes: Vec<u8>, bits: u64) -> Obj {
        if bits <= self.int_bits() {
            Obj::Int(get_bits(&bytes, 0, bits))
        } else {
            Obj::buffer(bytes)
        }
    }

    pub(crate) fn read_buffield(&self, bf: &BufField) -> Obj {
        let buf = bf.buf.borrow();
        let mut out = vec![0u8; bf.bits.div_ceil(8) as usize];
        let mut done = 0;
        while done < bf.bits {
            let n = (bf.bits - done).min(64);
            put_bits(&mut out, done, n, get_bits(&buf, bf.bit_off + done, n));
            done += n;
        }
        self.bits_to_obj(out, bf.bits)
    }

    pub(crate) fn write_buffield(&self, bf: &BufField, src: &[u8]) {
        let mut buf = bf.buf.borrow_mut();
        let mut done = 0;
        while done < bf.bits {
            let n = (bf.bits - done).min(64);
            put_bits(&mut buf, bf.bit_off + done, n, get_bits(src, done, n));
            done += n;
        }
    }

    /// The access units a field touches: (bit offset of the unit, low
    /// bit and number of the field's bits in it).
    fn units(fl: &Field) -> Vec<(u64, u64, u64)> {
        let w = fl.access as u64 * 8;
        let mut v = Vec::new();
        let mut unit = fl.bit_off / w * w;
        let end = fl.bit_off + fl.bits;
        while unit < end {
            let lo = unit.max(fl.bit_off);
            let hi = (unit + w).min(end);
            v.push((unit, lo - unit, hi - lo));
            unit += w;
        }
        v
    }

    pub(crate) fn read_field(&mut self, fl: &Field) -> Result<Obj> {
        let mut out = vec![0u8; fl.bits.div_ceil(8) as usize];
        for (unit, shift, n) in Self::units(fl) {
            let v = self.unit_io(fl, unit / 8, None)?;
            put_bits(&mut out, unit + shift - fl.bit_off, n, (v >> shift) & low_mask(n));
        }
        Ok(self.bits_to_obj(out, fl.bits))
    }

    pub(crate) fn write_field(&mut self, fl: &Field, v: &Obj) -> Result<()> {
        let src = match v {
            Obj::Int(n) => n.to_le_bytes().to_vec(),
            o => self.to_bytes(o)?,
        };
        let w = fl.access as u64 * 8;
        for (unit, shift, n) in Self::units(fl) {
            let chunk = get_bits(&src, unit + shift - fl.bit_off, n);
            let value = if n == w {
                chunk
            } else {
                let base = match fl.update {
                    1 => u64::MAX,
                    2 => 0,
                    _ => self.unit_io(fl, unit / 8, None)?,
                };
                (base & !(low_mask(n) << shift)) | chunk << shift
            };
            self.unit_io(fl, unit / 8, Some(value & low_mask(w)))?;
        }
        Ok(())
    }

    /// Read (`value` None) or write one access unit at byte offset `off`
    /// of the field's region.
    fn unit_io(&mut self, fl: &Field, off: u64, value: Option<u64>) -> Result<u64> {
        match &fl.kind {
            FieldKind::Region(r) => self.region_io(r, off, fl.access, value),
            FieldKind::Bank { region, bank, value: sel } => {
                let (bank, sel, region) = (bank.clone(), *sel, region.clone());
                self.store_named(&bank, Obj::Int(sel))?;
                self.region_io(&region, off, fl.access, value)
            }
            FieldKind::Index { index, data } => {
                let (index, data) = (index.clone(), data.clone());
                self.store_named(&index, Obj::Int(off))?;
                let Some(Obj::Field(df)) = self.get(&data).cloned() else { return Err(Error::NotFound(data)) };
                match value {
                    None => {
                        let o = self.read_field(&df)?;
                        self.to_int(&o)
                    }
                    Some(v) => {
                        self.write_field(&df, &Obj::Int(v))?;
                        Ok(0)
                    }
                }
            }
        }
    }

    fn region_io(&mut self, r: &Region, off: u64, width: u8, value: Option<u64>) -> Result<u64> {
        let addr = if r.space == space::PCI {
            ((self.pci_bdf(r) as u64) << 16) | (r.base + off)
        } else {
            r.base.wrapping_add(off)
        };
        let bits = width * 8;
        match value {
            None => self.host.read(r.space, addr, bits).ok_or(Error::Io),
            Some(v) => {
                if self.host.write(r.space, addr, bits, v) {
                    Ok(0)
                } else {
                    Err(Error::Io)
                }
            }
        }
    }

    /// The PCI function a PCI_Config region belongs to: the `_ADR` of the
    /// device it is declared in, on the bus of the root bridge's `_BBN`
    /// and the secondary buses of the bridges between.
    fn pci_bdf(&mut self, r: &Region) -> u32 {
        if let Some(v) = r.pci.get() {
            return v;
        }
        let mut chain: Vec<String> = Vec::new();
        let mut bus = 0u32;
        let mut s = r.scope.clone();
        while !s.is_empty() {
            if matches!(self.get(&s), Some(Obj::Device)) {
                let ids = self.ids(&s);
                if ids.iter().any(|i| i == "PNP0A03" || i == "PNP0A08") {
                    bus = self.eval_inner(&alloc::format!("{s}._BBN")).ok().and_then(|o| o.int()).unwrap_or(0) as u32;
                    break;
                }
                chain.push(s.clone());
            }
            s = String::from(parent_of(&s));
        }
        let mut bdf = 0;
        let n = chain.len();
        for (i, d) in chain.iter().rev().enumerate() {
            let adr = self.eval_inner(&alloc::format!("{d}._ADR")).ok().and_then(|o| o.int()).unwrap_or(0);
            bdf = (bus & 0xff) << 8 | ((adr >> 16) as u32 & 0x1f) << 3 | (adr as u32 & 7);
            if i + 1 < n {
                bus = self.host.read(space::PCI, (bdf as u64) << 16 | 0x19, 8).unwrap_or(0) as u32;
            }
        }
        r.pci.set(Some(bdf));
        bdf
    }
}
