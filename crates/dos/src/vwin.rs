//! VWIN, OS/2's Win-OS/2 virtual device driver (VWIN.SYS), as far as
//! Win-OS/2 needs it in rs2's DOS machine: INT 66h, whose AH selects a
//! router table (IBM's names from vwin.sym):
//!
//! | AH | VWIN |
//! |----|------|
//! | 01h-08h | WINSHIELD: add/delete a user routine, send a message (blocking or not), make dirty, get the next message, send an error, fix a lost child |
//! | 80h-8Bh | WINDD: am I seamless, display driver initialization, RAM semaphore, video controller, critical sections, query support and settings |
//! | E0h-E5h | pipes for the clipboard and DDE: open, close, read, write, set blocking mode, pass data |
//!
//! WINSHELD.EXE and WINSMSG.DLL call it through WINSMSG's `_INT386`
//! (the protected-mode vector of INT 66h, 32-bit registers) and through
//! the message entry point AH=01h returns.

use x86emu::{flags::CF, EAX, EBX, ECX, EDI, EDX, ESI, DS, ES};

use crate::{Host, Machine};

impl Machine {
    /// INT 66h, in real or protected mode.
    pub(crate) fn vwin_int66<H: Host>(&mut self, host: &mut H) {
        let r = &self.cpu.regs;
        let msg = alloc::format!(
            "vwin: INT 66h eax={:08x} ebx={:08x} ecx={:08x} edx={:08x} esi={:08x} edi={:08x} ds={:04x} es={:04x} pm={}",
            r[EAX],
            r[EBX],
            r[ECX],
            r[EDX],
            r[ESI],
            r[EDI],
            self.cpu.seg[DS].sel,
            self.cpu.seg[ES].sel,
            self.cpu.pm() as u8
        );
        host.log(&msg);
        // Nothing is there yet: VWIN's "no" (AX nonzero, carry).
        self.cpu.set_ax(1);
        self.set_ret_flag(CF, true);
    }
}
