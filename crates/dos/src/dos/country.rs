//! Country information (38h, 65h, 66h) and the case tables.

use x86emu::DS;

use super::{err, COLLATE, COUNTRY, DBCS, DOSSEG, FCHARS, FUCASE, UPCASE};
use crate::{Host, Machine};

/// Upper case of code page 437 characters 80h-FFh.
const UPPER_437: [u8; 128] = [
    0x80, 0x9a, 0x45, 0x41, 0x8e, 0x41, 0x8f, 0x80, 0x45, 0x45, 0x45, 0x49, 0x49, 0x49, 0x8e, 0x8f, //
    0x90, 0x92, 0x92, 0x4f, 0x99, 0x4f, 0x55, 0x55, 0x59, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f, //
    0x41, 0x49, 0x4f, 0x55, 0xa5, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf, //
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, //
    0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, //
    0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf, //
    0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, //
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
];

/// The country information record (38h layout, 34 bytes) for a country
/// code: date format, currency, separators, time format.
fn info(country: u16) -> [u8; 34] {
    // (date format, currency, thousands, decimal, date sep, time sep,
    //  currency format, 24-hour clock)
    let (df, cur, th, dec, ds, ts, cf, h24): (u8, &[u8], u8, u8, u8, u8, u8, bool) = match country {
        1 => (0, b"$", b',', b'.', b'-', b':', 0, false),
        44 => (1, b"\x9c", b',', b'.', b'/', b':', 0, true),
        61 => (1, b"$", b',', b'.', b'/', b':', 0, false),
        49 => (1, b"DM", b'.', b',', b'.', b'.', 3, true),
        33 => (1, b"F", b' ', b',', b'.', b':', 3, true),
        81 => (2, b"\x9d", b',', b'.', b'-', b':', 0, true),
        _ => (1, b"$", b',', b'.', b'/', b':', 0, true),
    };
    let mut r = [0u8; 34];
    r[0..2].copy_from_slice(&(df as u16).to_le_bytes());
    for (i, b) in cur.iter().take(4).enumerate() {
        r[2 + i] = *b;
    }
    r[7] = th;
    r[9] = dec;
    r[11] = ds;
    r[13] = ts;
    r[15] = cf;
    r[16] = 2;
    r[17] = h24 as u8;
    // Case map routine: DOSSEG:UPCASE-4 (a RETF stub that upcases AL).
    r[18..20].copy_from_slice(&(UPCASE - 0x10).to_le_bytes());
    r[20..22].copy_from_slice(&DOSSEG.to_le_bytes());
    r[22] = b',';
    r
}

impl Machine {
    pub(crate) fn country_init(&mut self) {
        let ds = DOSSEG;
        let i = info(self.cfg.country);
        self.hw.write_bytes(Machine::lin(ds, COUNTRY), &i);
        // The case map routine: AL >= 80h is upcased through the table.
        // cmp al,80h / jb done / push bx / mov bx,UPCASE+2-80h / cs: xlat / pop bx / done: retf
        let off = (UPCASE + 2).wrapping_sub(0x80);
        let code = [0x3c, 0x80, 0x72, 0x07, 0x53, 0xbb, off as u8, (off >> 8) as u8, 0x2e, 0xd7, 0x5b, 0xcb];
        self.hw.write_bytes(Machine::lin(ds, UPCASE - 0x10), &code);
        self.hw.wr16(Machine::lin(ds, UPCASE), 128);
        self.hw.write_bytes(Machine::lin(ds, UPCASE + 2), &UPPER_437);
        self.hw.wr16(Machine::lin(ds, FUCASE), 128);
        self.hw.write_bytes(Machine::lin(ds, FUCASE + 2), &UPPER_437);
        // File name terminators.
        let term = b"\x16\x00\x01\x00\xff\x00\x00\x20\x02\x0e.\"/\\[]:|<>+=;,";
        self.hw.wr16(Machine::lin(ds, FCHARS), term.len() as u16);
        self.hw.write_bytes(Machine::lin(ds, FCHARS + 2), term);
        self.hw.wr16(Machine::lin(ds, COLLATE), 256);
        for c in 0..256u32 {
            let v = if c >= 0x80 { UPPER_437[c as usize - 0x80] } else if (b'a' as u32..=b'z' as u32).contains(&c) { c as u8 - 32 } else { c as u8 };
            self.hw.wr(Machine::lin(ds, COLLATE + 2) + c, v);
        }
        self.hw.wr16(Machine::lin(ds, DBCS), 0);
        self.hw.wr16(Machine::lin(ds, DBCS + 2), 0);
        self.dos.codepage = self.cfg.codepage;
    }

    pub(crate) fn country_fn<H: Host>(&mut self, host: &mut H) {
        let _ = host;
        let ah = self.cpu.ah();
        let ds = DOSSEG;
        match ah {
            0x38 => {
                if self.cpu.dx() == 0xffff {
                    // Set country: accepted.
                    self.ok();
                    return;
                }
                let mut i = [0u8; 34];
                self.hw.read_bytes(Machine::lin(ds, COUNTRY), &mut i);
                self.hw.write_bytes(self.ds_dx(), &i);
                let c = self.cfg.country;
                self.cpu.set_bx(c);
                self.cpu.set_al(c.min(0xff) as u8);
                self.ok();
            }
            0x65 => {
                let al = self.cpu.al();
                let a = self.es_di();
                match al {
                    0x01 => {
                        let mut i = [0u8; 34];
                        self.hw.read_bytes(Machine::lin(ds, COUNTRY), &mut i);
                        let n = (self.cpu.cx() as usize).min(41);
                        let mut out = [0u8; 41];
                        out[0] = 1;
                        out[1..3].copy_from_slice(&38u16.to_le_bytes());
                        out[3..5].copy_from_slice(&self.cfg.country.to_le_bytes());
                        out[5..7].copy_from_slice(&self.dos.codepage.to_le_bytes());
                        out[7..41].copy_from_slice(&i);
                        self.hw.write_bytes(a, &out[..n]);
                        self.cpu.set_cx(n as u16);
                        self.ok();
                    }
                    0x02 | 0x04 | 0x05 | 0x06 | 0x07 => {
                        let table = match al {
                            0x02 => UPCASE,
                            0x04 => FUCASE,
                            0x05 => FCHARS,
                            0x06 => COLLATE,
                            _ => DBCS,
                        };
                        self.hw.wr(a, al);
                        self.hw.wr16(a + 1, table);
                        self.hw.wr16(a + 3, ds);
                        self.cpu.set_cx(5);
                        self.ok();
                    }
                    0x20 | 0xa0 => {
                        let c = self.cpu.dl();
                        let u = self.upcase(c);
                        self.cpu.set_dl(u);
                        self.ok();
                    }
                    0x21 | 0xa1 => {
                        let a = self.seg_lin(DS, self.cpu.dx());
                        for i in 0..self.cpu.cx() as u32 {
                            let c = self.hw.rd(a + i);
                            let u = self.upcase(c);
                            self.hw.wr(a + i, u);
                        }
                        self.ok();
                    }
                    0x22 | 0xa2 => {
                        let a = self.seg_lin(DS, self.cpu.dx());
                        for i in 0..65535u32 {
                            let c = self.hw.rd(a + i);
                            if c == 0 {
                                break;
                            }
                            let u = self.upcase(c);
                            self.hw.wr(a + i, u);
                        }
                        self.ok();
                    }
                    0x23 => {
                        let c = self.cpu.dl();
                        self.cpu.set_ax(match c {
                            b'Y' | b'y' => 1,
                            b'N' | b'n' => 0,
                            _ => 2,
                        });
                        self.ok();
                    }
                    _ => self.fail(err::INVALID_FUNCTION),
                }
            }
            _ => {
                // 66h: code pages.
                match self.cpu.al() {
                    1 => {
                        let cp = self.dos.codepage;
                        self.cpu.set_bx(cp);
                        self.cpu.set_dx(cp);
                        self.ok();
                    }
                    2 => {
                        self.dos.codepage = self.cpu.bx();
                        self.ok();
                    }
                    _ => self.fail(err::INVALID_FUNCTION),
                }
            }
        }
    }
}
