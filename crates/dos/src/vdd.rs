//! Virtual device driver entry points, as OS/2's VDDs register them
//! (VDHRegisterAPI): INT 2Fh AX=4011h with DS:SI = a VDD's name gives
//! ES:DI = its API entry point, a far procedure, in real and in
//! protected mode. Win-OS/2's drivers ask for them (MOUSE.DRV for
//! `VMOUSE$`, WINSHELD for `VWIN`); rs2 answers the ones its DOS
//! machine implements.

use x86emu::{flags::CF, Seg, DS, ES, ESI};

use crate::{bios, Host, Machine};

/// Callback numbers of the entry points, one per VDD.
pub const CB_VDD: u16 = 0x600;
pub const CB_VDD_LAST: u16 = 0x60f;
/// Real-mode entry stubs in the ROM segment (callback; RETF), 8 bytes
/// each.
pub const VDD_CODE: u16 = bios::SPECIAL + 0x200;

/// The VDDs with entry points (the last has no name: VWIN's message
/// entry, which WINSMSG calls like an interrupt, flags pushed: IRET).
const VDDS: [&[u8]; 3] = [b"VMOUSE$", b"VWIN", b""];
const VMOUSE: u16 = 0;
pub const VWIN: u16 = 1;
pub const VWIN_INT: u16 = 2;

/// The stub's return: RETF, or IRET for an entry called like an
/// interrupt.
pub(crate) fn vdd_ret(k: u16) -> u8 {
    if k == VWIN_INT { 0xcf } else { 0xcb }
}

impl Machine {
    pub(crate) fn vdd_rom_init(&mut self) {
        for k in 0..VDDS.len() as u16 {
            let n = CB_VDD + k;
            let a = Machine::lin(bios::ROM, VDD_CODE + k * 8);
            self.hw.write_bytes(a, &[0xfe, 0x38, n as u8, (n >> 8) as u8, vdd_ret(k)]);
        }
    }

    /// The VDD named at DS:SI (NUL-terminated, case-insensitive).
    fn vdd_named(&mut self) -> Option<u16> {
        let si = if self.cpu.pm() && self.cpu.seg[DS].big() { self.cpu.regs[ESI] } else { self.cpu.regs[ESI] & 0xffff };
        let a = self.cpu.seg[DS].base.wrapping_add(si);
        let mut name = [0u8; 16];
        let mut n = 0;
        while n < name.len() {
            let b = self.hw.rd(a + n as u32);
            if b == 0 {
                break;
            }
            name[n] = b.to_ascii_uppercase();
            n += 1;
        }
        VDDS.iter().position(|v| n > 0 && *v == &name[..n]).map(|k| k as u16)
    }

    /// INT 2Fh AX=4011h in real mode.
    pub(crate) fn vdd_entry_rm(&mut self) {
        match self.vdd_named() {
            Some(k) => {
                self.cpu.seg[ES] = Seg::real(bios::ROM);
                self.cpu.set_di(VDD_CODE + k * 8);
            }
            None => {
                self.cpu.seg[ES] = Seg::real(0);
                self.cpu.set_di(0);
            }
        }
    }

    /// INT 2Fh AX=4011h from a DPMI client: a protected-mode entry.
    pub(crate) fn vdd_entry_pm(&mut self) {
        let k = self.vdd_named();
        let (sel, off) = match k {
            Some(k) => self.dpmi_vdd_entry(k),
            None => (0, 0),
        };
        let _ = self.cpu.load_seg(&mut self.hw, ES, sel);
        self.cpu.set_di(off);
    }

    /// A far call to VDD `k`'s entry point.
    pub(crate) fn vdd_call<H: Host>(&mut self, host: &mut H, k: u16) {
        match k {
            VMOUSE => self.vmouse_api(),
            VWIN => self.vwin_entry(host),
            VWIN_INT => self.vwin_int66(host),
            _ => self.cpu.flags |= CF,
        }
    }

    /// VMOUSE$: AH=02h gives the factors MOUSE.DRV multiplies the
    /// positions of absolute events by (CX for x, DX for y): 1, as the
    /// mouse driver hands it positions in 0-65535 already.
    fn vmouse_api(&mut self) {
        match self.cpu.ah() {
            0x02 => {
                self.cpu.set_cx(1);
                self.cpu.set_dx(1);
                self.mouse.absolute = true;
            }
            _ => self.cpu.set_cx(0),
        }
        self.cpu.flags &= !CF;
    }
}
