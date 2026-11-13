//! Which API calls still run under the legacy lock (smp.rs, what is left
//! of the big kernel lock): those of the session and device corner. Every
//! other call runs under the locks of the data it touches.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::loader::gate::{MODULES, ORDINALS};

/// API names (prefixes, any case) that run without the legacy lock.
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
    // Memory.
    "DOSALLOCMEM", "DOSFREEMEM", "DOSSETMEM", "DOSQUERYMEM", "DOSALLOCSHAREDMEM", "DOSGETSHAREDMEM",
    "DOSGIVESHAREDMEM", "DOSGETNAMEDSHAREDMEM", "DOSALIASMEM", "DOSSUB", "DOSALLOCSEG", "DOSALLOCSHRSEG",
    "DOSGETSHRSEG", "DOSGIVESEG", "DOSREALLOCSEG", "DOSFREESEG", "DOSALLOCHUGE", "DOSGETHUGESHIFT",
    "DOSREALLOCHUGE", "DOSCREATECSALIAS", "DOSSIZESEG", "DOSMEMAVAIL", "DOSGETSEG", "DOSLOCKSEG",
    "DOSUNLOCKSEG", "DOSHUGESHIFT", "DOSHUGEINCR", "DOSQUERYPAGEUSAGE",
    // Semaphores, queues, pipes, timers, spin locks.
    "DOSCREATEEVENTSEM", "DOSOPENEVENTSEM", "DOSCLOSEEVENTSEM", "DOSPOSTEVENTSEM", "DOSRESETEVENTSEM",
    "DOSQUERYEVENTSEM", "DOSWAITEVENTSEM", "DOSCREATEMUTEXSEM", "DOSOPENMUTEXSEM", "DOSCLOSEMUTEXSEM",
    "DOSREQUESTMUTEXSEM", "DOSRELEASEMUTEXSEM", "DOSQUERYMUTEXSEM", "DOSCREATEMUXWAITSEM", "DOSOPENMUXWAITSEM",
    "DOSCLOSEMUXWAITSEM", "DOSWAITMUXWAITSEM", "DOSADDMUXWAITSEM", "DOSDELETEMUXWAITSEM", "DOSQUERYMUXWAITSEM",
    "DOSPM", "DOSSEMSET", "DOSSEMCLEAR", "DOSSEMWAIT", "DOSSEMREQUEST", "DOSMUXSEMWAIT", "DOSCREATESEM",
    "DOSOPENSEM", "DOSCLOSESEM", "DOSCREATEQUEUE", "DOSOPENQUEUE", "DOSCLOSEQUEUE", "DOSREADQUEUE",
    "DOSPEEKQUEUE", "DOSWRITEQUEUE", "DOSPURGEQUEUE", "DOSQUERYQUEUE", "DOSCREATEPIPE", "DOSMAKEPIPE",
    "DOSCONNECTNPIPE", "DOSDISCONNECTNPIPE", "DOSPEEKNPIPE", "DOSQUERYNPHSTATE", "DOSQUERYNPIPEINFO",
    "DOSQUERYNPIPESEMSTATE", "DOSSETNPHSTATE", "DOSSETNPIPESEM", "DOSTRANSACTNPIPE", "DOSWAITNPIPE",
    "DOSCALLNPIPE", "DOSRAWREADNPIPE", "DOSRAWWRITENPIPE", "DOSASYNCTIMER", "DOSSTARTTIMER", "DOSSTOPTIMER",
    "DOSTIMERASYNC", "DOSTIMERSTART", "DOSTIMERSTOP", "DOSCREATESPINLOCK", "DOSACQUIRESPINLOCK",
    "DOSRELEASESPINLOCK", "DOSFREESPINLOCK",
    // Files, directories and handles.
    "DOSOPEN", "DOSCLOSE", "DOSREAD", "DOSWRITE", "DOSSETFILEPTR", "DOSCHGFILEPTR", "DOSSETFILESIZE",
    "DOSNEWSIZE", "DOSQUERYFILEINFO", "DOSQFILEINFO", "DOSSETFILEINFO", "DOSQUERYPATHINFO", "DOSQPATHINFO",
    "DOSSETPATHINFO", "DOSDELETE", "DOSFORCEDELETE", "DOSMOVE", "DOSCOPY", "DOSCREATEDIR", "DOSMKDIR",
    "DOSDELETEDIR", "DOSRMDIR", "DOSFINDFIRST", "DOSFINDNEXT", "DOSFINDCLOSE", "DOSQUERYCURRENTDIR",
    "DOSQCURDIR", "DOSSETCURRENTDIR", "DOSCHDIR", "DOSQUERYCURRENTDISK", "DOSQCURDISK", "DOSSETDEFAULTDISK",
    "DOSSELECTDISK", "DOSQUERYFHSTATE", "DOSQFHANDSTATE", "DOSSETFHSTATE", "DOSSETFHANDSTATE", "DOSQUERYHTYPE",
    "DOSQHANDTYPE", "DOSDUPHANDLE", "DOSRESETBUFFER", "DOSBUFRESET", "DOSQUERYFSINFO", "DOSQFSINFO",
    "DOSSETFSINFO", "DOSQUERYFSATTACH", "DOSQFSATTACH", "DOSSETFILELOCKS", "DOSCANCELLOCKREQUEST",
    "DOSPROTECT", "DOSENUMATTRIBUTE", "DOSQUERYVERIFY", "DOSSETVERIFY", "DOSQVERIFY", "DOSEDITNAME",
    "DOSSEARCHPATH", "DOSSCANENV", "DOSQFILEMODE", "DOSSETFILEMODE", "DOSFILEIO", "DOSREADASYNC",
    "DOSWRITEASYNC", "DOSCREATENPIPE", "DOSQUERYAPPTYPE", "DOSQAPPTYPE", "DOSMAPCASE", "DOSQUERYCP",
    "DOSGETCP", "DOSQUERYCTRYINFO", "DOSQUERYDBCSENV", "DOSQUERYCOLLATE",
    // Programs and modules (the loader lock, loader/mod.rs).
    "DOSEXECPGM", "DOSLOADMODULE", "DOSFREEMODULE", "DOSQUERYPROCADDR", "DOSGETPROCADDR",
    "DOSQUERYMODULEHANDLE", "DOSGETMODHANDLE", "DOSQUERYMODULENAME", "DOSGETMODNAME", "DOSQUERYPROCTYPE",
    "DOSQUERYMODFROMEIP", "DOSGETRESOURCE", "DOSFREERESOURCE", "DOSQUERYRESOURCESIZE", "DOSQUERYHEADERINFO",
    "DOSREPLACEMODULE", "DOSSETEXTLIBPATH", "DOSQUERYEXTLIBPATH", "DOSGETENV", "DOSLIBINIT",
];

/// Calls still under the legacy lock in modules that are otherwise
/// free: the session switching corner (pop-ups, display mode, the PM
/// session, windowed sessions' control), which the keyboard's hot keys
/// also reach from interrupt context.
const LOCKED: &[(u8, u16)] = &[
    (1, 11),  // VioPopUp
    (1, 1),   // VioEndPopUp
    (1, 6), (1, 45), (1, 70), (2, 8), (2, 20), (3, 14), (3, 24), // replacement subsystems
    (8, 1),   // Rs2MapDisplay (creates the PM session)
    (8, 7),   // Rs2PowerOff
    (8, 23),  // Rs2SetDisplayMode
    (8, 16),  // viowin control (windowed/full screen, select)
    (8, 18),  // Rs2PtyStart (a session)
    (8, 400), // VDM register (a DOS session)
    (8, 407), // VDM request (starts programs in DOS sessions)
    (8, 600), (8, 601), (8, 602), (8, 603), (8, 604), (8, 605), (8, 606), (8, 607), (8, 608), // LAN requester
];

/// Modules whose calls are free unless listed in LOCKED: VIOCALLS,
/// KBDCALLS, MOUCALLS, MSG, NLS, QUECALLS, RS2API, MONCALLS.
const FREE_MODULES: &[u8] = &[1, 2, 3, 4, 5, 7, 8, 9];

/// 0 not decided yet, 1 under the lock, 2 free.
static CACHE: [AtomicU8; MODULES.len() * ORDINALS as usize] = [const { AtomicU8::new(0) }; MODULES.len() * ORDINALS as usize];

/// Whether the API (canonical module and ordinal) needs the legacy lock.
pub fn needs_legacy(module: u8, ordinal: u16) -> bool {
    if crate::smp::bkl_forced() {
        return true;
    }
    let Some(slot) = CACHE.get(module as usize * ORDINALS as usize + ordinal as usize) else { return true };
    match slot.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let name = super::names::display(module, ordinal);
            let free = if LOCKED.contains(&(module, ordinal)) {
                false
            } else if FREE_MODULES.contains(&module) {
                true
            } else {
                name != "?"
                    && FREE.iter().any(|p| name.len() >= p.len() && name.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes()))
            };
            slot.store(if free { 2 } else { 1 }, Ordering::Relaxed);
            !free
        }
    }
}
