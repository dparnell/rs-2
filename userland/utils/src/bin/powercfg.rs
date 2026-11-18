//! POWERCFG: show and change the processor power plan (rs2's own).
//!
//!   POWERCFG                       the plan, what power management does, the
//!                                  power source and battery
//!   POWERCFG /SET:plan [/SAVE]     change the plan now (/SAVE: also at boot)
//!   POWERCFG /SAVE                 keep the current plan for the next boot
//!   POWERCFG /WATCH[:n]            readings every second, n times (10)
//!
//! Plans: PERFORMANCE, BALANCED, POWERSAVER. The plan at boot is
//! CONFIG.SYS's POWERPLAN= (Balanced without one); the Power object in
//! System Setup changes the same setting.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use os2::power::{self, PowerInfo};
use utils::*;
os2::entry!(main);

fn usage() {
    out(concat!(
        "POWERCFG shows and changes the processor power plan.\r\n",
        "Syntax:\r\n",
        "  POWERCFG [/SET:plan] [/SAVE] [/WATCH[:n]]\r\n",
        "where:\r\n",
        "  SET:plan   Uses the plan now: PERFORMANCE, BALANCED or POWERSAVER.\r\n",
        "  SAVE       Also uses it when the system starts (POWERPLAN= in CONFIG.SYS).\r\n",
        "  WATCH:n    Shows the readings every second, n times (10 by default).\r\n",
    ));
}

fn watts(mw: u32) -> String {
    format!("{}.{:02} W", mw / 1000, mw % 1000 / 10)
}

fn show(i: &PowerInfo, saved: Option<u32>) {
    let name = |p: u32| power::NAMES.get(p as usize).copied().unwrap_or("?");
    outln(&format!("Power plan:     {}", name(i.plan)));
    match saved {
        Some(p) if p != i.plan => outln(&format!("At start-up:    {} (POWERPLAN= in CONFIG.SYS)", name(p))),
        Some(_) => {}
        None => outln("At start-up:    Balanced (no POWERPLAN= in CONFIG.SYS)"),
    }
    let f = i.flags;
    let idle = if f & power::F_MWAIT != 0 { format!("MWAIT, down to C{}", i.cstate) } else { String::from("HLT (C1)") };
    let tick = if f & power::F_TICKLESS != 0 { ", idle CPUs stop their timer" } else { "" };
    outln(&format!("Idle:           {idle}{tick}"));
    let speed = if f & power::F_HWP != 0 {
        format!("hardware P-states (HWP), {}-{} MHz, guaranteed {} MHz", i.min_mhz, i.max_mhz, i.base_mhz)
    } else if f & power::F_SPEEDSTEP != 0 {
        format!("Enhanced SpeedStep, {}-{} MHz, turbo to {} MHz", i.min_mhz, i.base_mhz, i.max_mhz)
    } else {
        String::from("not controlled (the processor offers no interface rs2 knows)")
    };
    outln(&format!("Speed:          {speed}"));
    if f & power::F_EPB != 0 {
        outln("                energy/performance bias follows the plan");
    }
    if f & power::F_HYPERVISOR != 0 {
        outln("                (a virtual machine: the host decides most of this)");
    }
    readings(i);
}

/// The power source and battery lines (from APM$).
fn battery() {
    let Some(a) = power::apm_status(0) else { return };
    outln(&format!("Power source:   {}", a.source_text()));
    let mut s = format!("Battery:        {}", a.battery_text());
    if a.batteries > 1 {
        s += &format!(" ({} batteries)", a.batteries);
    }
    outln(&s);
}

fn readings(i: &PowerInfo) {
    battery();
    let f = i.flags;
    let mut s = format!("CPUs: {} ({} idle without a tick)", i.cpus, i.tickless_now);
    s += &format!(", timer interrupts: {}/s", i.timer_irqs);
    if i.cpu0_mhz != 0 {
        s += &format!(", CPU 0: {} MHz", i.cpu0_mhz);
    }
    outln(&s);
    if f & power::F_RAPL_PKG != 0 {
        let mut s = format!("Power:          package {}", watts(i.pkg_mw));
        if f & power::F_RAPL_CORE != 0 {
            s += &format!(", cores {}", watts(i.core_mw));
        }
        if f & power::F_RAPL_DRAM != 0 {
            s += &format!(", memory {}", watts(i.dram_mw));
        }
        outln(&s);
    } else {
        outln("Power:          no energy counters (RAPL) on this processor");
    }
}

fn main(s: &os2::Startup) -> u32 {
    let mut set = None;
    let mut save = false;
    let mut watch = 0u32;
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        let Some(o) = u.strip_prefix('/').or_else(|| u.strip_prefix('-')) else {
            usage();
            return 1;
        };
        if let Some(p) = o.strip_prefix("SET:").or_else(|| o.strip_prefix("SET=")) {
            match power::parse(p) {
                Some(p) => set = Some(p),
                None => {
                    eprint(&format!("POWERCFG: unknown plan {p}: use PERFORMANCE, BALANCED or POWERSAVER\r\n"));
                    return 87;
                }
            }
        } else if o == "SAVE" {
            save = true;
        } else if o == "WATCH" {
            watch = 10;
        } else if let Some(n) = o.strip_prefix("WATCH:") {
            watch = n.parse().unwrap_or(10).max(1);
        } else if o == "?" {
            usage();
            return 0;
        } else {
            usage();
            return 1;
        }
    }
    let Some(mut info) = power::query() else {
        eprint("POWERCFG: this system does not report its power management\r\n");
        return 1;
    };
    if let Some(p) = set {
        let rc = power::set_plan(p);
        if rc != 0 {
            error(rc);
            return rc;
        }
        info = power::query().unwrap_or(info);
    }
    if save {
        let rc = power::save(info.plan);
        if rc != 0 {
            eprint("POWERCFG: CONFIG.SYS could not be changed\r\n");
            error(rc);
            return rc;
        }
    }
    show(&info, power::query_saved());
    for _ in 0..watch {
        unsafe { os2::dos::DosSleep(1000) };
        if let Some(i) = power::query() {
            readings(&i);
        }
    }
    0
}
