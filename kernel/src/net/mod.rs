//! Networking: the network interface drivers and the TCP/IP stack.
//!
//! Drivers (`drivers::e1000`, `drivers::virtio::net`) implement [`Nic`]
//! and [`register`] their devices while the PCI bus is probed. The
//! protocol stack is the `tcpip` crate: one [`Stack`] behind a sleeping
//! lock, with the loopback interface as interface 0 and the NICs as
//! interfaces 1.. in registration order.
//!
//! Nothing interrupts yet (no MSI/MSI-X, no INTx routing): the timer tick
//! asks every NIC whether frames are waiting ([`Nic::rx_pending`], which
//! only uses `try_lock`) and wakes the network thread, which takes the
//! frames into the stack, runs its timers (at least every [`POLL_MS`]),
//! transmits what the stack produced and wakes the threads waiting on
//! sockets. An interrupt handler would do exactly what the tick does.
//! Socket calls run the stack in the calling thread ([`with_stack`]),
//! transmitting at once.
//!
//! Waiting: every state change bumps [`EVENTS`] and wakes [`WAIT`]; a
//! blocked socket call re-checks its condition then (one queue for all
//! sockets keeps select() simple; per-socket queues can come later).

pub mod socket;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tcpip::{Ipv4, Mac, MacFmt, Stack};

use crate::sync::{Mutex, SpinLock};
use crate::task::WaitQueue;

/// A network interface card.
pub trait Nic: Send + Sync {
    /// "e1000 00:02.0".
    fn name(&self) -> String;
    fn mac(&self) -> Mac;
    fn link_up(&self) -> bool;
    fn mtu(&self) -> usize {
        1500
    }
    /// Queue a frame (without FCS) for transmission; false if the
    /// transmit ring is full.
    fn send(&self, frame: &[u8]) -> bool;
    /// Hand every received frame to `f`; returns how many there were.
    fn receive(&self, f: &mut dyn FnMut(&[u8])) -> usize;
    /// Frames are waiting. Called from the timer interrupt: must not
    /// block or take a lock it might find held (try_lock only).
    fn rx_pending(&self) -> bool;
    /// The NIC interrupts on receive (and calls [`kick`]): the timer tick
    /// need not poll it.
    fn interrupt_driven(&self) -> bool {
        false
    }
}

/// Timer interval of the network thread.
pub const POLL_MS: u64 = 20;

static NICS: SpinLock<Vec<Arc<dyn Nic>>> = SpinLock::new(Vec::new());
static STACK: Mutex<Option<Stack>> = Mutex::new(None);
/// Bumped on every change socket waiters may care about.
pub static EVENTS: AtomicU64 = AtomicU64::new(0);
/// Threads waiting on sockets (and for DNS answers).
pub static WAIT: WaitQueue = WaitQueue::new();
/// Frames arrived (set by the tick, cleared by the network thread).
static KICK: AtomicBool = AtomicBool::new(false);
static THREAD_WAIT: WaitQueue = WaitQueue::new();
static RUNNING: AtomicBool = AtomicBool::new(false);

/// A driver found a NIC.
pub fn register(nic: Arc<dyn Nic>) {
    crate::println!("net: {} {} (link {})", nic.name(), MacFmt(nic.mac()), if nic.link_up() { "up" } else { "down" });
    NICS.lock().push(nic);
}

pub fn nic_count() -> usize {
    NICS.lock().len()
}

/// How interfaces are configured: `net=` on the kernel command line or
/// NETWORK= in CONFIG.SYS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Config {
    Off,
    Dhcp,
    /// address/prefix[,gateway[,dns...]] for the first interface.
    Static { addr: Ipv4, mask: Ipv4, gateway: Option<Ipv4>, dns: Vec<Ipv4> },
}

impl Config {
    pub fn parse(s: &str) -> Option<Config> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("off") || s.eq_ignore_ascii_case("none") {
            return Some(Config::Off);
        }
        if s.eq_ignore_ascii_case("dhcp") || s.is_empty() {
            return Some(Config::Dhcp);
        }
        let mut parts = s.split(',').map(str::trim);
        let (a, prefix) = match parts.next()?.split_once('/') {
            Some((a, p)) => (a, p.parse::<u8>().ok().filter(|&p| p <= 32)?),
            None => (s.split(',').next()?, 24),
        };
        let addr = Ipv4::parse(a)?;
        let gateway = match parts.next() {
            Some(g) if !g.is_empty() => Some(Ipv4::parse(g)?),
            _ => None,
        };
        let dns = parts.map(Ipv4::parse).collect::<Option<Vec<_>>>()?;
        Some(Config::Static { addr, mask: Ipv4::mask(prefix), gateway, dns })
    }
}

fn seed() -> u32 {
    let t = crate::arch::rdtsc();
    (t ^ (t >> 32) ^ crate::irq::uptime_ms().rotate_left(17)) as u32
}

/// Bring networking up: the loopback interface always, the NICs as
/// `cfg` says. Called once at boot, after CONFIG.SYS is read.
pub fn start(cfg: Config) {
    let now = crate::irq::uptime_ms();
    let nics = NICS.lock().clone();
    {
        let mut g = STACK.lock();
        let mut st = Stack::new(seed(), "rs2");
        for (n, nic) in nics.iter().enumerate() {
            let i = st.add_ethernet(&alloc::format!("eth{n}"), nic.mac(), nic.mtu());
            st.set_link(i, nic.link_up());
            match &cfg {
                Config::Off => st.iface_up(i, false),
                Config::Dhcp => st.start_dhcp(i, now),
                Config::Static { addr, mask, gateway, dns } if n == 0 => {
                    st.configure(i, *addr, *mask, *gateway, dns);
                    let gw = gateway.map_or(String::from("none"), |g| alloc::format!("{g}"));
                    let dns: Vec<String> = dns.iter().map(|d| alloc::format!("{d}")).collect();
                    crate::println!("net: eth0 {}/{} gateway {gw} dns {} (static)", addr, mask.prefix_len(), dns.join(","));
                }
                Config::Static { .. } => st.iface_up(i, false),
            }
        }
        st.take_log();
        *g = Some(st);
    }
    if nics.is_empty() {
        crate::println!("net: no network adapter; loopback only");
    } else if cfg == Config::Off {
        crate::println!("net: interfaces disabled (net=off)");
    }
    flush_and_log();
    RUNNING.store(true, Ordering::Release);
    crate::task::kthread::spawn("network", thread_main);
}

/// Whether the stack is up (sockets work).
pub fn running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// Timer tick (interrupt context): wake the network thread when frames
/// are waiting.
pub fn tick() {
    if !RUNNING.load(Ordering::Relaxed) || KICK.load(Ordering::Relaxed) {
        return;
    }
    let Some(nics) = NICS.try_lock() else { return };
    if nics.iter().any(|n| !n.interrupt_driven() && n.rx_pending()) {
        drop(nics);
        KICK.store(true, Ordering::Release);
        THREAD_WAIT.wake_all();
    }
}

/// A NIC's interrupt (receive, link change): wake the network thread.
/// Interrupt context.
pub fn kick() {
    KICK.store(true, Ordering::Release);
    THREAD_WAIT.wake_all();
}

fn thread_main() {
    let mut links: Vec<bool> = Vec::new();
    loop {
        let deadline = crate::irq::ticks() + POLL_MS * crate::irq::HZ / 1000;
        THREAD_WAIT.wait_until(Some(deadline), || KICK.load(Ordering::Acquire));
        KICK.store(false, Ordering::Release);
        let nics = NICS.lock().clone();
        let now = crate::irq::uptime_ms();
        {
            let mut g = STACK.lock();
            let st = g.as_mut().unwrap();
            links.resize(nics.len(), false);
            for (n, nic) in nics.iter().enumerate() {
                let up = nic.link_up();
                if up != links[n] {
                    links[n] = up;
                    st.set_link(n + 1, up);
                }
                // Bounded, so one busy NIC cannot starve the timers.
                for _ in 0..8 {
                    if nic.receive(&mut |f| st.receive(n + 1, f, now)) == 0 {
                        break;
                    }
                }
            }
            st.poll(now);
        }
        flush_and_log();
    }
}

/// Transmit the stack's frames, print its messages, wake socket waiters.
fn flush_and_log() {
    let nics = NICS.lock().clone();
    let (log, changed) = {
        let mut g = STACK.lock();
        let Some(st) = g.as_mut() else { return };
        transmit(st, &nics);
        (st.take_log(), st.take_changed())
    };
    for l in log {
        crate::println!("net: {l}");
    }
    if changed {
        notify();
    }
}

fn transmit(st: &mut Stack, nics: &[Arc<dyn Nic>]) {
    while let Some((i, f)) = st.take_frame() {
        if let Some(nic) = i.checked_sub(1).and_then(|n| nics.get(n)) {
            if !nic.send(&f) {
                st.ifaces[i].stats.tx_packets -= 1;
            }
        }
    }
}

/// Wake everything waiting on sockets.
pub fn notify() {
    EVENTS.fetch_add(1, Ordering::Release);
    WAIT.wake_all();
}

/// Run `f` on the stack (thread context), then transmit and wake.
pub fn with_stack<R>(f: impl FnOnce(&mut Stack, u64) -> R) -> Option<R> {
    let nics = NICS.lock().clone();
    let (r, log, changed) = {
        let mut g = STACK.lock();
        let st = g.as_mut()?;
        let r = f(st, crate::irq::uptime_ms());
        transmit(st, &nics);
        (r, st.take_log(), st.take_changed())
    };
    for l in log {
        crate::println!("net: {l}");
    }
    if changed {
        notify();
    }
    Some(r)
}

/// Interface information for Rs2NetQueryIface.
pub struct IfaceInfo {
    pub name: String,
    pub mac: Mac,
    pub addr: Ipv4,
    pub mask: Ipv4,
    pub gateway: Ipv4,
    pub dns: Vec<Ipv4>,
    pub mtu: u32,
    /// Bit 0 link up, 1 administratively up, 2 loopback, 3 DHCP, 4 bound.
    pub flags: u32,
    pub stats: tcpip::stack::Stats,
}

pub fn iface_info(i: usize) -> Option<IfaceInfo> {
    with_stack(|st, _| {
        let f = st.ifaces.get(i)?;
        let mut flags = f.link as u32 | (f.up as u32) << 1 | (f.loopback as u32) << 2;
        if let Some(c) = &f.dhcp {
            flags |= 8;
            if c.lease.is_some() {
                flags |= 16;
            }
        }
        Some(IfaceInfo {
            name: f.name.clone(),
            mac: f.mac,
            addr: f.addr,
            mask: f.mask,
            gateway: f.gateway.unwrap_or_default(),
            dns: f.dns.clone(),
            mtu: f.mtu as u32,
            flags,
            stats: f.stats,
        })
    })
    .flatten()
}
