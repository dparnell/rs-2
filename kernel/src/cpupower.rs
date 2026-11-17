//! Processor power management: the power plan and what it sets.
//!
//! A power plan (Performance, Balanced, Power saver) is chosen at boot
//! by CONFIG.SYS's `POWERPLAN=` (rs2's own statement; Balanced without
//! one) and changed at run time with `Rs2PowerControl` (the Power
//! object's Power page, POWERCFG). Changing it takes effect at once: the
//! CPU that asks applies it to itself and kicks the others, which apply
//! it on their next timer tick or before they next go idle.
//!
//! What a plan sets, where the processor can do it:
//!
//! - **Idle state.** Idle CPUs wait with `MWAIT` and a C-state hint
//!   from CPUID leaf 5 (Intel, not under a hypervisor: in a virtual
//!   machine `MWAIT` exits to the host or is missing; `HLT` there and on
//!   other processors). Performance stays in C1 (`HLT`), Balanced goes
//!   down to the third MWAIT C-state (C6 on Skylake), Power saver to the
//!   deepest the CPU lists. Below C1 needs an always-running APIC timer
//!   (ARAT) and an invariant TSC, else C1 is the limit: the boot CPU's
//!   tick keeps the time and the clock counts with the TSC. The boot
//!   CPU, which keeps its 1 ms tick, goes no deeper than the third state:
//!   deeper ones take longer to leave than the tick leaves it there.
//! - **Tickless idle.** Outside Performance, an idle CPU other than the
//!   boot one stops its LAPIC timer while it waits: it only counts its
//!   time slice, and whatever wakes it (a thread made ready, a TLB
//!   shootdown, a sleeper due on the boot CPU) sends it an IPI. That is
//!   1000 fewer interrupts a second per idle CPU. The boot CPU keeps its
//!   tick (the clock, sleepers and the drivers polled every tick).
//! - **Speed.** With Intel's hardware P-states (HWP) the plan sets the
//!   request: Performance from the guaranteed to the highest level,
//!   energy/performance preference 0; Balanced lowest to highest, 0x80;
//!   Power saver lowest to guaranteed (no turbo), 0xC0. Without HWP but
//!   with Enhanced SpeedStep a small governor on each CPU's tick picks
//!   the ratio from the share of its last 32 ticks that were busy
//!   (Performance always the highest turbo ratio; Power saver no turbo).
//!   The energy/performance bias (`IA32_ENERGY_PERF_BIAS`) follows the
//!   plan where the CPU has it (0, 6, 15). AMD processors get the idle
//!   part only.
//! - **Readings.** RAPL energy counters (Intel package, cores and DRAM;
//!   AMD family 17h+ package) are sampled every second by the boot CPU,
//!   with the boot CPU's effective clock (APERF/MPERF) and the timer
//!   interrupts all CPUs took in that second.
//!
//! Model-specific registers are read and written with
//! [`crate::arch::rdmsr_safe`] / [`crate::arch::wrmsr_safe`]: a register
//! a CPU (or an emulator) lacks faults and is skipped.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed};

use crate::arch::{cpuid, rdmsr_safe, wrmsr_safe};
use crate::smp::{self, MAX_CPUS};
use crate::sync::Once;

pub const PERFORMANCE: u32 = 0;
pub const BALANCED: u32 = 1;
pub const POWER_SAVER: u32 = 2;

pub const PLAN_NAMES: [&str; 3] = ["performance", "balanced", "power saver"];

static PLAN: AtomicU32 = AtomicU32::new(BALANCED);
/// Bumped by every plan change; each CPU applies the plan when its copy
/// differs.
static GEN: AtomicU32 = AtomicU32::new(1);
static APPLIED: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];
static TICK_STOPPED: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
/// `mwait` on the kernel command line: use MWAIT in a virtual machine
/// too (for testing the idle path under QEMU).
static FORCE_MWAIT: AtomicBool = AtomicBool::new(false);

// Model-specific registers.
const MSR_MPERF: u32 = 0xe7;
const MSR_APERF: u32 = 0xe8;
const MSR_PLATFORM_INFO: u32 = 0xce;
const MSR_PERF_CTL: u32 = 0x199;
const MSR_MISC_ENABLE: u32 = 0x1a0;
const MSR_TURBO_RATIO_LIMIT: u32 = 0x1ad;
const MSR_ENERGY_PERF_BIAS: u32 = 0x1b0;
const MSR_PM_ENABLE: u32 = 0x770;
const MSR_HWP_CAPABILITIES: u32 = 0x771;
const MSR_HWP_REQUEST: u32 = 0x774;
const MSR_RAPL_POWER_UNIT: u32 = 0x606;
const MSR_PKG_ENERGY_STATUS: u32 = 0x611;
const MSR_DRAM_ENERGY_STATUS: u32 = 0x619;
const MSR_PP0_ENERGY_STATUS: u32 = 0x639;
const MSR_AMD_RAPL_POWER_UNIT: u32 = 0xc001_0299;
const MSR_AMD_PKG_ENERGY_STATUS: u32 = 0xc001_029b;

/// What the processor can do (from the boot CPU).
pub struct Caps {
    pub intel: bool,
    pub amd: bool,
    pub hypervisor: bool,
    pub mwait: bool,
    /// MWAIT sub-states per C-state (CPUID 5 EDX nibbles; [0] is C0).
    pub substates: [u8; 8],
    pub arat: bool,
    pub invariant_tsc: bool,
    pub hwp: bool,
    pub hwp_epp: bool,
    pub epb: bool,
    pub eist: bool,
    pub aperf: bool,
    /// Ratios (100 MHz units) for the SpeedStep governor: lowest, the
    /// highest without turbo, the highest with it.
    pub ratio_min: u8,
    pub ratio_base: u8,
    pub ratio_turbo: u8,
    /// Energy counters: unit shift (1/2^n J) and the registers there are.
    pub energy_shift: u8,
    pub pkg_msr: u32,
    pub core_msr: u32,
    pub dram_msr: u32,
}

static CAPS: Once<Caps> = Once::new();

/// Energy and clock readings, refreshed every second by the boot CPU.
static PKG_MW: AtomicU32 = AtomicU32::new(0);
static CORE_MW: AtomicU32 = AtomicU32::new(0);
static DRAM_MW: AtomicU32 = AtomicU32::new(0);
static CPU0_MHZ: AtomicU32 = AtomicU32::new(0);
static TIMER_IRQS_PER_S: AtomicU32 = AtomicU32::new(0);
static TIMER_IRQS: AtomicU64 = AtomicU64::new(0);
/// HWP levels of the boot CPU (lowest, guaranteed, highest), for reports.
static HWP_LEVELS: [AtomicU8; 3] = [const { AtomicU8::new(0) }; 3];

struct Sample {
    ms: u64,
    pkg: u32,
    core: u32,
    dram: u32,
    aperf: u64,
    mperf: u64,
    irqs: u64,
}
static mut LAST: Sample = Sample { ms: 0, pkg: 0, core: 0, dram: 0, aperf: 0, mperf: 0, irqs: 0 };

/// SpeedStep governor state per CPU: busy and total ticks in the
/// current window, the ratio last written.
static BUSY: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];
static WINDOW: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];
static RATIO: [AtomicU8; MAX_CPUS] = [const { AtomicU8::new(0) }; MAX_CPUS];

fn caps() -> Option<&'static Caps> {
    CAPS.get()
}

pub fn plan() -> u32 {
    PLAN.load(Relaxed)
}

/// Parse a plan name (CONFIG.SYS, POWERCFG): its number.
pub fn parse_plan(s: &str) -> Option<u32> {
    let s = s.trim().to_ascii_uppercase();
    match s.as_str() {
        "PERFORMANCE" | "HIGH" | "MAX" | "0" => Some(PERFORMANCE),
        "BALANCED" | "1" => Some(BALANCED),
        "POWERSAVER" | "POWER SAVER" | "SAVER" | "LOW" | "2" => Some(POWER_SAVER),
        _ => None,
    }
}

/// Detect what the processor can do; the boot CPU, once the LAPIC is up.
pub fn init(cmdline: &str) {
    if cmdline.split_whitespace().any(|w| w == "mwait") {
        FORCE_MWAIT.store(true, Relaxed);
    }
    let (max, b, c, d) = cpuid(0, 0);
    let intel = (b, d, c) == (0x756e_6547, 0x4965_6e69, 0x6c65_746e);
    let amd = (b, d, c) == (0x6874_7541, 0x6974_6e65, 0x444d_4163);
    let (eax1, _, ecx1, _) = if max >= 1 { cpuid(1, 0) } else { (0, 0, 0, 0) };
    let mut family = (eax1 >> 8) & 0xf;
    if family == 0xf {
        family += (eax1 >> 20) & 0xff;
    }
    let hypervisor = ecx1 & (1 << 31) != 0;
    let monitor = ecx1 & (1 << 3) != 0;
    let mut substates = [0u8; 8];
    if monitor && max >= 5 {
        let (_, _, c5, d5) = cpuid(5, 0);
        if c5 & 1 != 0 {
            for (i, s) in substates.iter_mut().enumerate() {
                *s = ((d5 >> (4 * i)) & 0xf) as u8;
            }
        }
    }
    let (a6, _, c6, _) = if max >= 6 { cpuid(6, 0) } else { (0, 0, 0, 0) };
    let (ext, _, _, _) = cpuid(0x8000_0000, 0);
    let invariant_tsc = ext >= 0x8000_0007 && cpuid(0x8000_0007, 0).3 & (1 << 8) != 0;
    let mut caps = Caps {
        intel,
        amd,
        hypervisor,
        mwait: monitor && intel && substates[1] > 0,
        substates,
        arat: a6 & (1 << 2) != 0,
        invariant_tsc,
        hwp: intel && a6 & (1 << 7) != 0,
        hwp_epp: intel && a6 & (1 << 10) != 0,
        epb: intel && c6 & (1 << 3) != 0,
        eist: intel && ecx1 & (1 << 7) != 0,
        aperf: c6 & 1 != 0,
        ratio_min: 0,
        ratio_base: 0,
        ratio_turbo: 0,
        energy_shift: 0,
        pkg_msr: 0,
        core_msr: 0,
        dram_msr: 0,
    };
    if caps.eist && !caps.hwp {
        match rdmsr_safe(MSR_PLATFORM_INFO) {
            Some(p) if (p >> 8) & 0xff != 0 => {
                caps.ratio_base = (p >> 8) as u8;
                caps.ratio_min = ((p >> 40) as u8).clamp(1, caps.ratio_base);
                let turbo = rdmsr_safe(MSR_TURBO_RATIO_LIMIT).map_or(0, |t| t as u8);
                caps.ratio_turbo = turbo.max(caps.ratio_base);
                // SpeedStep must be on for PERF_CTL to count.
                if let Some(m) = rdmsr_safe(MSR_MISC_ENABLE) {
                    if m & (1 << 16) == 0 && !wrmsr_safe(MSR_MISC_ENABLE, m | (1 << 16)) {
                        caps.eist = false;
                    }
                }
            }
            _ => caps.eist = false,
        }
    }
    // RAPL: Intel family 6, AMD family 17h and later.
    let (unit_msr, pkg) = if intel && family == 6 {
        (MSR_RAPL_POWER_UNIT, MSR_PKG_ENERGY_STATUS)
    } else if amd && family >= 0x17 {
        (MSR_AMD_RAPL_POWER_UNIT, MSR_AMD_PKG_ENERGY_STATUS)
    } else {
        (0, 0)
    };
    if unit_msr != 0 {
        // Emulators may answer the registers with zeros.
        if let (Some(u @ 1..), Some(_)) = (rdmsr_safe(unit_msr), rdmsr_safe(pkg)) {
            caps.energy_shift = ((u >> 8) & 0x1f) as u8;
            caps.pkg_msr = pkg;
            if intel {
                if rdmsr_safe(MSR_PP0_ENERGY_STATUS).is_some_and(|v| v != 0) {
                    caps.core_msr = MSR_PP0_ENERGY_STATUS;
                }
                if rdmsr_safe(MSR_DRAM_ENERGY_STATUS).is_some_and(|v| v != 0) {
                    caps.dram_msr = MSR_DRAM_ENERGY_STATUS;
                }
            }
        }
    }
    CAPS.set(caps);
    apply_here();
    sample();
    crate::println!("cpupower: {}", summary());
}

/// One line on what power management does on this machine.
pub fn summary() -> alloc::string::String {
    use alloc::format;
    use alloc::string::String;
    let Some(c) = caps() else { return String::from("not initialised") };
    let mut s = format!("plan {}; idle ", PLAN_NAMES[plan() as usize]);
    match mwait_hint(false) {
        Some(h) => s += &format!("MWAIT C{} (hint {h:#04x})", (h >> 4) + 1),
        None => s += "HLT",
    }
    if tickless() {
        s += ", tickless";
    }
    if c.hwp {
        let l = |i: usize| HWP_LEVELS[i].load(Relaxed);
        s += &format!("; HWP {}-{} (guaranteed {})", l(0), l(2), l(1));
    } else if c.eist {
        s += &format!("; SpeedStep ratios {}-{} (turbo {})", c.ratio_min, c.ratio_base, c.ratio_turbo);
    } else {
        s += "; no speed control";
    }
    if c.epb {
        s += ", EPB";
    }
    if c.pkg_msr != 0 {
        s += "; RAPL";
    }
    if c.hypervisor {
        s += "; virtual machine";
    }
    s
}

/// The MWAIT hint for an idle wait under the current plan, None for HLT.
/// `ticking`: the CPU keeps its 1 ms tick while it waits.
fn mwait_hint(ticking: bool) -> Option<u32> {
    let c = caps()?;
    if !c.mwait || (c.hypervisor && !FORCE_MWAIT.load(Relaxed)) {
        return None;
    }
    let mut max = match plan() {
        PERFORMANCE => return None,
        BALANCED => 3,
        _ => 7,
    };
    if !c.arat || !c.invariant_tsc {
        max = 1;
    }
    if ticking {
        max = max.min(3);
    }
    let i = (1..=max).rev().find(|&i| c.substates[i] > 0)?;
    Some(((i as u32) - 1) << 4)
}

/// Whether idle CPUs other than the boot one stop their tick.
fn tickless() -> bool {
    plan() != PERFORMANCE && crate::apic::active()
}

/// Change the plan; takes effect at once on every CPU.
pub fn set_plan(p: u32) -> bool {
    if p > POWER_SAVER {
        return false;
    }
    let old = PLAN.swap(p, Relaxed);
    GEN.fetch_add(1, Relaxed);
    crate::arch::without_interrupts(apply_here);
    for c in smp::online() {
        smp::kick(c);
    }
    if old != p {
        crate::println!("cpupower: {}", summary());
    }
    true
}

/// Apply the plan's speed settings to this CPU (interrupts off).
fn apply_here() {
    let Some(c) = caps() else { return };
    let i = smp::this().index();
    let g = GEN.load(Relaxed);
    let p = plan();
    if c.hwp {
        if rdmsr_safe(MSR_PM_ENABLE).is_some_and(|v| v & 1 == 0) {
            wrmsr_safe(MSR_PM_ENABLE, 1);
        }
        if let Some(hc) = rdmsr_safe(MSR_HWP_CAPABILITIES) {
            let (highest, guaranteed, lowest) = (hc as u8, (hc >> 8) as u8, (hc >> 24) as u8);
            let guaranteed = guaranteed.clamp(lowest, highest);
            if i == 0 {
                HWP_LEVELS[0].store(lowest, Relaxed);
                HWP_LEVELS[1].store(guaranteed, Relaxed);
                HWP_LEVELS[2].store(highest, Relaxed);
            }
            let (min, max, epp) = match p {
                PERFORMANCE => (guaranteed, highest, 0u8),
                BALANCED => (lowest, highest, 0x80),
                _ => (lowest, guaranteed, 0xc0),
            };
            let epp = if c.hwp_epp { epp as u64 } else { 0 };
            wrmsr_safe(MSR_HWP_REQUEST, min as u64 | (max as u64) << 8 | epp << 24);
        }
    } else if c.eist {
        // The governor picks the ratio on the next window; start there.
        RATIO[i].store(0, Relaxed);
        set_ratio(c, i, if p == PERFORMANCE { c.ratio_turbo } else { c.ratio_base });
    }
    if c.epb {
        let bias = [0u64, 6, 15][p as usize];
        if let Some(v) = rdmsr_safe(MSR_ENERGY_PERF_BIAS) {
            wrmsr_safe(MSR_ENERGY_PERF_BIAS, (v & !0xf) | bias);
        }
    }
    APPLIED[i].store(g, Relaxed);
}

fn set_ratio(c: &Caps, i: usize, r: u8) {
    if RATIO[i].swap(r, Relaxed) == r {
        return;
    }
    // Bit 32 disengages turbo (IDA): set above the base ratio only when
    // the plan wants turbo.
    let no_turbo = if r > c.ratio_base { 0 } else { 1u64 << 32 };
    let v = rdmsr_safe(MSR_PERF_CTL).unwrap_or(0);
    wrmsr_safe(MSR_PERF_CTL, (v & !0x1_0000_ffff) | (r as u64) << 8 | no_turbo);
}

fn sync_here() {
    let i = smp::this().index();
    if APPLIED[i].load(Relaxed) != GEN.load(Relaxed) {
        apply_here();
    }
}

/// Every CPU's timer interrupt (interrupts off): count it, apply a plan
/// change, run the SpeedStep governor.
pub fn tick(idle: bool) {
    TIMER_IRQS.fetch_add(1, Relaxed);
    let Some(c) = caps() else { return };
    sync_here();
    if c.hwp || !c.eist {
        return;
    }
    let i = smp::this().index();
    if !idle {
        BUSY[i].fetch_add(1, Relaxed);
    }
    if WINDOW[i].fetch_add(1, Relaxed) + 1 < 32 {
        return;
    }
    let load = BUSY[i].swap(0, Relaxed) * 100 / WINDOW[i].swap(0, Relaxed).max(1);
    let (lo, hi) = (c.ratio_min, if plan() == POWER_SAVER { c.ratio_base } else { c.ratio_turbo });
    let r = match plan() {
        PERFORMANCE => c.ratio_turbo,
        BALANCED if load >= 80 => hi,
        POWER_SAVER if load >= 90 => hi,
        _ => lo + ((hi - lo) as u32 * load / 100) as u8,
    };
    set_ratio(c, i, r);
}

/// The idle wait of [`crate::task::sched::idle_loop`]: entered with
/// interrupts off once nothing is ready, returns after an interrupt with
/// them on.
pub fn idle_wait() {
    let c = smp::this();
    let i = c.index();
    sync_here();
    let stop = i != 0 && tickless();
    if stop {
        crate::apic::timer_stop();
        TICK_STOPPED[i].store(true, Relaxed);
    }
    match mwait_hint(!stop) {
        Some(hint) => unsafe {
            let addr = &c.need_resched as *const AtomicBool;
            core::arch::asm!("monitor", in("rax") addr, in("ecx") 0, in("edx") 0, options(nostack));
            core::arch::asm!("sti", "mwait", in("eax") hint, in("ecx") 0, options(nostack));
        },
        None => unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) },
    }
    if stop {
        crate::arch::without_interrupts(|| {
            TICK_STOPPED[i].store(false, Relaxed);
            crate::apic::timer_start();
        });
    }
}

/// Once a second on the boot CPU (from its tick): energy and clock.
pub fn sample() {
    let Some(c) = caps() else { return };
    let now = crate::rtc::mono_ms();
    let rd = |m: u32| if m == 0 { 0 } else { rdmsr_safe(m).unwrap_or(0) as u32 };
    let (pkg, core, dram) = (rd(c.pkg_msr), rd(c.core_msr), rd(c.dram_msr));
    let (aperf, mperf) = if c.aperf {
        (rdmsr_safe(MSR_APERF).unwrap_or(0), rdmsr_safe(MSR_MPERF).unwrap_or(0))
    } else {
        (0, 0)
    };
    let irqs = TIMER_IRQS.load(Relaxed);
    // Only the boot CPU's tick calls this.
    #[allow(static_mut_refs)]
    let last = unsafe { &mut LAST };
    let ms = now.saturating_sub(last.ms);
    if last.ms != 0 && ms > 0 {
        let mw = |new: u32, old: u32| {
            // The counters are 32 bits wide and wrap.
            let d = new.wrapping_sub(old) as u64;
            (d * 1_000_000 / (ms << c.energy_shift)) as u32
        };
        PKG_MW.store(mw(pkg, last.pkg), Relaxed);
        CORE_MW.store(mw(core, last.core), Relaxed);
        DRAM_MW.store(mw(dram, last.dram), Relaxed);
        let dm = mperf.wrapping_sub(last.mperf);
        if dm != 0 {
            // MPERF counts at the TSC's rate while the CPU runs.
            let khz = crate::rtc::tsc_per_ms() as u128 * aperf.wrapping_sub(last.aperf) as u128 / dm as u128;
            CPU0_MHZ.store((khz / 1000) as u32, Relaxed);
        }
        TIMER_IRQS_PER_S.store((irqs.wrapping_sub(last.irqs) * 1000 / ms) as u32, Relaxed);
    }
    *last = Sample { ms: now, pkg, core, dram, aperf, mperf, irqs };
}

/// Flags of [`Info::flags`].
pub const F_MWAIT: u32 = 0x1;
pub const F_HWP: u32 = 0x2;
pub const F_SPEEDSTEP: u32 = 0x4;
pub const F_EPB: u32 = 0x8;
pub const F_RAPL_PKG: u32 = 0x10;
pub const F_RAPL_CORE: u32 = 0x20;
pub const F_RAPL_DRAM: u32 = 0x40;
pub const F_TICKLESS: u32 = 0x80;
pub const F_APERF: u32 = 0x100;
pub const F_HYPERVISOR: u32 = 0x200;
pub const F_ARAT: u32 = 0x400;
pub const F_INVARIANT_TSC: u32 = 0x800;

/// RS2POWERINFO, as Rs2PowerControl returns it: ULONGs.
pub fn info() -> [u32; 14] {
    let mut r = [0u32; 14];
    r[0] = (r.len() * 4) as u32;
    r[1] = plan();
    let Some(c) = caps() else { return r };
    let hint = mwait_hint(false);
    let mut f = 0;
    for (on, bit) in [
        (hint.is_some(), F_MWAIT),
        (c.hwp, F_HWP),
        (c.eist && !c.hwp, F_SPEEDSTEP),
        (c.epb, F_EPB),
        (c.pkg_msr != 0, F_RAPL_PKG),
        (c.core_msr != 0, F_RAPL_CORE),
        (c.dram_msr != 0, F_RAPL_DRAM),
        (tickless(), F_TICKLESS),
        (c.aperf, F_APERF),
        (c.hypervisor, F_HYPERVISOR),
        (c.arat, F_ARAT),
        (c.invariant_tsc, F_INVARIANT_TSC),
    ] {
        if on {
            f |= bit;
        }
    }
    r[2] = f;
    r[3] = hint.map_or(1, |h| (h >> 4) + 1);
    r[4] = PKG_MW.load(Relaxed);
    r[5] = CORE_MW.load(Relaxed);
    r[6] = DRAM_MW.load(Relaxed);
    r[7] = CPU0_MHZ.load(Relaxed);
    let (lo, base, hi) = if c.hwp {
        (HWP_LEVELS[0].load(Relaxed), HWP_LEVELS[1].load(Relaxed), HWP_LEVELS[2].load(Relaxed))
    } else {
        (c.ratio_min, c.ratio_base, c.ratio_turbo)
    };
    r[8] = lo as u32 * 100;
    r[9] = base as u32 * 100;
    r[10] = hi as u32 * 100;
    r[11] = smp::online_count();
    r[12] = TICK_STOPPED.iter().filter(|t| t.load(Relaxed)).count() as u32;
    r[13] = TIMER_IRQS_PER_S.load(Relaxed);
    r
}
