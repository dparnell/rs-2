//! Which API calls still run under the big kernel lock (smp.rs) while the
//! kernel moves to finer locks: a call is converted when every piece of
//! shared state it touches has its own lock.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::loader::gate::{MODULES, ORDINALS};

/// API names (prefixes, any case) that run without the big kernel lock.
const FREE: &[&str] = &[
    // Processes, threads, exceptions and signals.
    "DOSCREATETHREAD", "DOSEXIT", "DOSENTERCRITSEC", "DOSENTERMUSTCOMPLETE", "DOSWAITTHREAD", "DOSKILLTHREAD",
    "DOSSUSPENDTHREAD", "DOSRESUMETHREAD", "DOSSETPRIORITY", "DOSSETPRTY", "DOSGETPRTY", "DOSGETINFOBLOCKS",
    "DOSSLEEP", "DOSWAITCHILD", "DOSCWAIT", "DOSKILLPROCESS", "DOSALLOCTHREADLOCALMEMORY",
    "DOSFREETHREADLOCALMEMORY", "DOSSETSIGNALEXCEPTIONFOC", "DOSSETEXCEPTIONHANDLER", "DOSUNSETEXCEPTIONHANDLER",
    "DOSRAISEEXCEPTION", "DOSUNWINDEXCEPTION", "DOSACKNOWLEDGESIGNALEXC", "DOSSENDSIGNALEXCEPTION", "DOSGETPID",
    "DOSGETPPID", "DOSQUERYSYSINFO", "DOSQSYSINFO", "DOSGETDATETIME", "DOSFSRAMSEM", "DOSSETMAXFH",
    "DOSSETRELMAXFH", "DOSISETRELMAXFH", "DOSGETINFOSEG", "DOSTMRQUERY", "DOSERROR", "DOSGETVERSION",
    "DOSGETMACHINEMODE", "DOSQUERYTHREADCONTEXT", "DOSVERIFYPIDTID", "DOSHOLDSIGNAL", "DOSSETSIGHANDLER",
    "DOSFLAGPROCESS", "DOSSENDSIGNAL", "DOSTIB", "DOSLOCALINFO", "DOSGLOBALINFO", "DOSPAGESIZE",
    "DOSMAXPATHLEN", "DOSSELTOFLAT", "DOSFLATTOSEL",
];

/// 0 not decided yet, 1 under the lock, 2 free.
static CACHE: [AtomicU8; MODULES.len() * ORDINALS as usize] = [const { AtomicU8::new(0) }; MODULES.len() * ORDINALS as usize];

/// Whether the API (canonical module and ordinal) needs the big kernel lock.
pub fn needs_bkl(module: u8, ordinal: u16) -> bool {
    let Some(slot) = CACHE.get(module as usize * ORDINALS as usize + ordinal as usize) else { return true };
    match slot.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let name = super::names::display(module, ordinal);
            let free = name != "?"
                && FREE.iter().any(|p| name.len() >= p.len() && name.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes()));
            slot.store(if free { 2 } else { 1 }, Ordering::Relaxed);
            !free
        }
    }
}
