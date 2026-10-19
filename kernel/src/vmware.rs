//! The VMware "backdoor": an I/O port (0x5658) through which a guest
//! talks to the VMware hypervisor (and to QEMU with `vmport=on`). An
//! `in eax, dx` with the magic number in EAX and a command in ECX traps to
//! the hypervisor, which answers in the general registers.
//!
//! Used for the absolute pointer (`drivers::vmmouse`) and the host's
//! clock. The port is only touched when CPUID says we run under a
//! hypervisor; on real hardware it could belong to some device.

use core::sync::atomic::{AtomicU8, Ordering};

pub const MAGIC: u32 = 0x564d_5868; // "VMXh"
const PORT: u16 = 0x5658;

// Commands (ECX).
pub const CMD_GETVERSION: u32 = 10;
pub const CMD_GETTIME: u32 = 23;
pub const CMD_ABSPOINTER_DATA: u32 = 39;
pub const CMD_ABSPOINTER_STATUS: u32 = 40;
pub const CMD_ABSPOINTER_COMMAND: u32 = 41;
pub const CMD_GETTIMEFULL: u32 = 46;

#[derive(Clone, Copy, Debug, Default)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
}

/// One backdoor call: command `cmd` with argument `arg` (EBX).
fn call(cmd: u32, arg: u32) -> Regs {
    let (eax, ebx, ecx, edx, esi, edi): (u32, u64, u32, u32, u32, u32);
    // RBX is reserved by the compiler: swap it through a scratch register.
    unsafe {
        core::arch::asm!(
            "xchg {b}, rbx",
            "in eax, dx",
            "xchg {b}, rbx",
            b = inout(reg) arg as u64 => ebx,
            inout("eax") MAGIC => eax,
            inout("ecx") cmd => ecx,
            inout("edx") PORT as u32 => edx,
            inout("esi") 0u32 => esi,
            inout("edi") 0u32 => edi,
            options(nostack),
        );
    }
    Regs { eax, ebx: ebx as u32, ecx, edx, esi, edi }
}

/// Whether CPUID reports a hypervisor.
pub fn under_hypervisor() -> bool {
    let r = core::arch::x86_64::__cpuid(1);
    r.ecx & (1 << 31) != 0
}

/// The hypervisor's CPUID vendor string ("VMwareVMware", "TCGTCGTCGTCG",
/// "KVMKVMKVM", "VBoxVBoxVBox", ...).
pub fn hypervisor_vendor() -> Option<[u8; 12]> {
    if !under_hypervisor() {
        return None;
    }
    let r = core::arch::x86_64::__cpuid(0x4000_0000);
    let mut v = [0u8; 12];
    v[0..4].copy_from_slice(&r.ebx.to_le_bytes());
    v[4..8].copy_from_slice(&r.ecx.to_le_bytes());
    v[8..12].copy_from_slice(&r.edx.to_le_bytes());
    Some(v)
}

/// 0 = not probed yet, 1 = absent, 2 = present.
static PRESENT: AtomicU8 = AtomicU8::new(0);

/// Whether the backdoor answers (VMware, or QEMU with vmport).
pub fn present() -> bool {
    match PRESENT.load(Ordering::Relaxed) {
        0 => {
            // GETVERSION echoes the magic number in EBX; anything else
            // (all ones from an unclaimed port) leaves our value there.
            let ok = under_hypervisor() && {
                let r = call(CMD_GETVERSION, !MAGIC);
                r.ebx == MAGIC && r.eax != 0xffff_ffff
            };
            PRESENT.store(if ok { 2 } else { 1 }, Ordering::Relaxed);
            ok
        }
        v => v == 2,
    }
}

/// A backdoor call, if the backdoor exists.
pub fn command(cmd: u32, arg: u32) -> Option<Regs> {
    present().then(|| call(cmd, arg))
}

/// The host's clock: seconds since 1970 (UTC) and microseconds.
pub fn host_time() -> Option<(u64, u32)> {
    // GETTIMEFULL answers with the magic number in EAX and 64-bit
    // seconds in ESI:EDX; older hosts only know GETTIME (EAX = seconds).
    let r = command(CMD_GETTIMEFULL, 0)?;
    if r.eax == MAGIC {
        return Some(((r.esi as u64) << 32 | r.edx as u64, r.ebx));
    }
    let r = command(CMD_GETTIME, 0)?;
    (r.eax != 0xffff_ffff).then_some((r.eax as u64, r.ebx))
}

/// Civil date (year, month, day) of a day number since 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// Log what the backdoor tells about the host (once at boot).
pub fn init() {
    if let Some(v) = hypervisor_vendor() {
        let name: alloc::string::String = v.iter().filter(|&&b| b != 0).map(|&b| b as char).collect();
        crate::trace!("hypervisor: {name}");
    }
    if !present() {
        return;
    }
    let version = call(CMD_GETVERSION, 0).eax;
    match host_time() {
        Some((secs, _)) => {
            let (y, m, d) = civil((secs / 86_400) as i64);
            let s = secs % 86_400;
            crate::println!(
                "vmware: backdoor version {version}, host time {y}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
                s / 3600,
                s / 60 % 60,
                s % 60
            );
        }
        None => crate::println!("vmware: backdoor version {version}"),
    }
}
