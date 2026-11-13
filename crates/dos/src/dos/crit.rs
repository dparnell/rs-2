//! Sharing and lock violations: the retries (INT 21h 440Bh) and the
//! critical error (INT 24h) for compatibility-mode and FCB files.
//!
//! As IBM's VDM does (its kernel's SHARE_ERROR, Share_Violation,
//! Read/Write_Lock_Violation): a sharing violation on an open, or a lock
//! violation on a read or write, is tried again "retry count" times in
//! all (a "delay" of waiting between tries, 440Bh sets both; 3 and 1 by
//! default). If it still fails and the file is an FCB's or was opened in
//! compatibility mode (sharing bits 0), DOS raises a critical error:
//! INT 24h with AH = 18h (Fail and Retry allowed) | 06h (data area) | 1
//! when writing, AL = the drive, DI = 0Ch (general failure: DOS gives
//! INT 24h no codes above 0Ch, the real one, 20h or 21h, is the
//! extended error 59h returns) and BP:SI a block device header. The
//! handler's answer: Retry runs the call again (another round of
//! retries), Fail returns error 53h ("fail on INT 24h"; an FCB call its
//! failure AL), Abort ends the program (termination type 2), and Ignore
//! (not allowed for these) counts as Fail. Files opened with a sharing
//! mode get the plain error (20h, 21h) after the retries, no INT 24h.
//!
//! The stack the handler sees is DOS's: its IRET frame, then the
//! caller's AX BX CX DX SI DI BP DS ES, then the caller's INT 21h frame;
//! the IRET returns to a callback (CB_I24_RET) that carries out the
//! answer. Without a handler of the program's, INT 24h is DOS's own
//! prompt ("Abort, Retry, Fail?"), as COMMAND.COM's.

use x86emu::{CS, DS, EAX, EBP, EBX, ECX, EDI, EDX, ES, ESI};

use super::{err, DOSSEG, SDA};
use crate::{Cb, Host, Machine};

/// Default retry count and delay (440Bh).
pub const RETRIES: u16 = 3;
pub const DELAY: u16 = 1;
/// A delay unit: about one time slice (IBM's VDM yields once per unit).
const DELAY_US: u64 = 1000;
/// The error a call returns after its INT 24h handler said Fail.
pub const FAIL_I24: u16 = 0x53;
/// A call's internal "critical error raised" result.
pub const RAISED: u16 = 0xfffe;
/// A block device header for BP:SI (DOS's data segment).
pub const BLOCK_DEV: u16 = super::DEVICES + 0xa0;

/// How a failed call reports Fail.
#[derive(Clone, Copy, Debug)]
pub enum FailWith {
    /// CF set, AX = 53h.
    Handle,
    /// AL = this (FCB calls).
    Fcb(u8),
}

/// A critical error about to be raised (set where the call failed).
#[derive(Clone, Copy, Debug)]
pub struct Req {
    /// The extended error (20h, 21h).
    pub code: u16,
    pub write: bool,
    pub drive: u8,
}

/// A critical error in progress: the INT 21h caller's registers.
#[derive(Clone, Debug)]
pub struct Crit {
    regs: [u32; 8],
    ds: u16,
    es: u16,
    fail: FailWith,
    code: u16,
    /// The default handler's prompt was shown.
    prompted: bool,
}

impl Machine {
    /// Run `f` (one host file operation) and try again while it fails
    /// with a sharing or lock violation, as many times as 440Bh says.
    pub(crate) fn share_retry<H: Host, T>(&mut self, host: &mut H, mut f: impl FnMut(&mut H) -> Result<T, u16>) -> Result<T, u16> {
        let tries = self.dos.retries.max(1);
        let mut n = 1;
        loop {
            match f(host) {
                Err(err::SHARING_VIOLATION | err::LOCK_VIOLATION) if n < tries => {
                    n += 1;
                    let us = self.dos.retry_delay as u64 * DELAY_US;
                    if us > 0 {
                        let end = host.micros() + us;
                        loop {
                            let now = host.micros();
                            if now >= end {
                                break;
                            }
                            host.wait(end - now);
                        }
                    }
                }
                r => return r,
            }
        }
    }

    /// A sharing or lock violation `e` after the retries on a file opened
    /// with `mode` on drive `drive`: RAISED when it becomes a critical
    /// error (compatibility mode or FCB, a real-mode call, none in
    /// progress), else `e`.
    pub(crate) fn share_error(&mut self, e: u16, mode: u8, write: bool, drive: u8) -> u16 {
        if !matches!(e, err::SHARING_VIOLATION | err::LOCK_VIOLATION) || (mode >> 4) & 7 != 0 {
            return e;
        }
        if self.cpu.pm() || self.pm_flags_at.is_some() || self.dos.crit.is_some() {
            return e;
        }
        self.dos.crit_req = Some(Req { code: e, write, drive });
        RAISED
    }

    /// Take INT 24h for the critical error share_error set up: the
    /// caller's registers back as at its INT 21h, DOS's frame on its
    /// stack, and the handler entered returning to CB_I24_RET.
    pub(crate) fn raise_crit<H: Host>(&mut self, host: &mut H, fail: FailWith) -> Cb {
        let Some(req) = self.dos.crit_req.take() else { return Cb::Done };
        let _ = host;
        let (regs, ds, es) = self.dos.entry;
        self.cpu.regs = regs;
        self.cpu.set_seg_real(DS, ds);
        self.cpu.set_seg_real(ES, es);
        for v in [es as u32, ds as u32, regs[EBP], regs[EDI], regs[ESI], regs[EDX], regs[ECX], regs[EBX], regs[EAX]] {
            let _ = self.cpu.push(&mut self.hw, v & 0xffff, 2);
        }
        // The real error for 59h.
        self.dos_error(req.code);
        self.dos.crit = Some(Crit { regs, ds, es, fail, code: req.code, prompted: false });
        self.cpu.set_ah(0x18 | 0x06 | req.write as u8);
        self.cpu.set_al(req.drive);
        self.cpu.set_di(0x0c);
        self.cpu.set16(EBP, DOSSEG);
        self.cpu.set_si(BLOCK_DEV);
        // DOS's critical error flag (ErrorMode) while the handler runs.
        self.hw.wr(Machine::lin(DOSSEG, SDA), 1);
        self.cpu.set_seg_real(CS, crate::bios::ROM);
        self.cpu.eip = crate::bios::I24_RET_CODE as u32;
        self.chain_int = Some(0x24);
        Cb::Done
    }

    /// The INT 24h handler returned (AL = its answer).
    pub(crate) fn crit_done<H: Host>(&mut self, host: &mut H) -> Cb {
        let Some(c) = self.dos.crit.take() else { return Cb::Done };
        self.hw.wr(Machine::lin(DOSSEG, SDA), 0);
        let mut act = self.cpu.al();
        // Retry and Fail are allowed; Ignore counts as Fail (DOS 3.1+),
        // anything else too.
        if act != 1 && act != 2 {
            act = 3;
        }
        // The caller's registers (and stack: its INT 21h frame on top).
        self.cpu.regs = c.regs;
        self.cpu.set_seg_real(DS, c.ds);
        self.cpu.set_seg_real(ES, c.es);
        match act {
            1 => {
                // Run the call again: back to INT 21h's callback.
                self.cpu.set_seg_real(CS, crate::bios::ROM);
                self.cpu.eip = (crate::bios::STUBS + 0x21 * 8) as u32;
            }
            2 => self.dos_exit(host, 0, 2),
            _ => {
                let (_, class, action, locus) = self.dos.last_error;
                match c.fail {
                    FailWith::Handle => self.ret_cf(Err(FAIL_I24)),
                    FailWith::Fcb(al) => self.cpu.set_al(al),
                }
                self.dos.last_error = (FAIL_I24, class, action, locus);
            }
        }
        Cb::Done
    }

    /// DOS's own INT 24h handler: "Abort, Retry, Fail?" until one of
    /// the keys comes (Ignore when the error allows it).
    pub(crate) fn crit_default(&mut self) -> Cb {
        let ah = self.cpu.ah();
        let Some(c) = self.dos.crit.as_mut() else {
            // A critical error DOS did not raise (a program calling INT
            // 24h itself): fail it.
            self.cpu.set_al(3);
            return Cb::Done;
        };
        if !c.prompted {
            c.prompted = true;
            let what = match c.code {
                err::SHARING_VIOLATION => "Sharing violation",
                err::LOCK_VIOLATION => "Lock violation",
                _ => "General failure",
            };
            let msg = alloc::format!(
                "\r\n{what} {} drive {}\r\nAbort, Retry, {}Fail?",
                if ah & 1 != 0 { "writing" } else { "reading" },
                (b'A' + self.cpu.al()) as char,
                if ah & 0x20 != 0 { "Ignore, " } else { "" },
            );
            for b in msg.bytes() {
                self.tty_out(b);
            }
        }
        loop {
            let Some(k) = self.peek_key(true) else {
                self.idle_poll();
                return Cb::Retry;
            };
            self.polls = 0;
            let ch = (k as u8).to_ascii_uppercase();
            let act = match ch {
                b'A' => 2,
                b'R' => 1,
                b'F' if ah & 0x08 != 0 => 3,
                b'I' if ah & 0x20 != 0 => 0,
                _ => continue,
            };
            for b in [ch, b'\r', b'\n'] {
                self.tty_out(b);
            }
            self.cpu.set_al(act);
            return Cb::Done;
        }
    }

    /// INT 21h 440Bh: DX tries, CX delay between them.
    pub(crate) fn set_retries(&mut self) -> Result<(), u16> {
        let (tries, delay) = (self.cpu.dx(), self.cpu.cx());
        if tries == 0 {
            return Err(err::INVALID_FUNCTION);
        }
        self.dos.retries = tries;
        self.dos.retry_delay = delay;
        Ok(())
    }
}
