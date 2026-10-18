//! Process trees and DosWaitChild; the child modes of this program.

use alloc::string::String;
use core::ptr::{null, null_mut};

use os2::*;


use crate::{check, counters, run_child, start_child, wait_child};

pub fn my_pid() -> u32 {
    let mut tib = null_mut();
    let mut pib = null_mut();
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pid
    }
}

fn num(args: &[String], i: usize) -> u32 {
    args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Child entry: `STRESS -child MODE ARGS...`.
pub fn child(args: &[String]) -> u32 {
    let mode = args.first().map(String::as_str).unwrap_or("");
    let rest = if args.is_empty() { &args[..] } else { &args[1..] };
    match mode {
        "exit" => num(rest, 0),
        "postsem" => crate::sems::child_postsem(),
        "holdmutex" => crate::sems::child_holdmutex(),
        "sig" => crate::signals::child_sig(rest),
        "sigsem" => crate::signals::child_sigsem(),
        "crash" => crate::xcpt::child_crash(),
        "raise" => crate::xcpt::child_raise(),
        "block" => block(rest.first().map(String::as_str).unwrap_or("")),
        "tree" => tree(num(rest, 0)),
        "tree-block" => tree_block(num(rest, 0)),
        "orphans" => orphans(),
        "mem" => crate::mem::child_storm(num(rest, 0)),
        "getshm" => crate::mem::child_getshm(num(rest, 0)),
        "namedshm" => crate::mem::child_namedshm(),
        "files" => crate::files::child_files(),
        "oom" => crate::mem::child_oom(),
        "flood" => crate::more::child_flood(rest),
        "npserver" => crate::more::child_npserver(),
        _ => 255,
    }
}

unsafe extern "C" fn forever(_: u32) {
    let mut ev = 0;
    unsafe {
        DosCreateEventSem(null(), &mut ev, 0, 0);
        DosWaitEventSem(ev, SEM_INDEFINITE_WAIT);
    }
}

/// Block in the kernel until killed.
fn block(what: &str) -> u32 {
    unsafe {
        match what {
            "pipe" => {
                let (mut r, mut w) = (0, 0);
                DosCreatePipe(&mut r, &mut w, 512);
                let mut b = [0u8; 4];
                let mut n = 0;
                DosRead(r, b.as_mut_ptr(), 4, &mut n);
            }
            "sleep" => {
                DosSleep(SEM_INDEFINITE_WAIT);
            }
            "queue" => {
                let mut q = 0;
                let name = alloc::format!("\\QUEUES\\STRESS\\BLOCK{}\0", my_pid());
                DosCreateQueue(&mut q, QUE_FIFO, name.as_ptr());
                let mut req = RequestData::default();
                let (mut cb, mut pb, mut prio) = (0, null(), 0u8);
                DosReadQueue(q, &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0);
            }
            "threads" => {
                for _ in 0..8 {
                    crate::thread(forever, 0);
                }
                forever(0);
            }
            _ => forever(0),
        }
    }
    100
}

/// A binary tree of processes `depth` deep; returns the number of nodes.
fn tree(depth: u32) -> u32 {
    if depth == 0 {
        return 1;
    }
    let a = start_child(&alloc::format!("tree {}", depth - 1));
    let b = start_child(&alloc::format!("tree {}", depth - 1));
    let mut n = 1;
    for _ in 0..2 {
        let (rc, cause, code, pid) = wait_child(0);
        if rc != 0 || cause != 0 || (pid != a && pid != b) {
            return 200;
        }
        n += code;
    }
    n
}

/// Like `tree`, but every node blocks once its children are started.
fn tree_block(depth: u32) -> u32 {
    if depth > 0 {
        start_child(&alloc::format!("tree-block {}", depth - 1));
        start_child(&alloc::format!("tree-block {}", depth - 1));
    }
    block("event")
}

/// Start children with EXEC_ASYNCRESULT and end without waiting for them.
fn orphans() -> u32 {
    for _ in 0..5 {
        start_child("exit 0");
    }
    unsafe { DosSleep(100) };
    0
}

pub fn run() {
    let before = counters();
    // Sequential children.
    let mut ok = true;
    for i in 0..40 {
        let (cause, code) = run_child(&alloc::format!("exit {i}"));
        ok &= cause == 0 && code == i;
    }
    check("40 sequential children", ok);

    // Many asynchronous children at once, collected in any order.
    let mut pids = [0u32; 24];
    for (i, p) in pids.iter_mut().enumerate() {
        *p = start_child(&alloc::format!("exit {}", 100 + i));
    }
    let mut seen = 0u32;
    let mut ok = pids.iter().all(|&p| p != 0);
    for _ in 0..pids.len() {
        let (rc, cause, code, pid) = wait_child(0);
        match pids.iter().position(|&p| p == pid) {
            Some(i) if rc == 0 && cause == 0 && code == 100 + i as u32 => seen |= 1 << i,
            _ => ok = false,
        }
    }
    check("24 concurrent children collected", ok && seen == (1 << pids.len()) - 1);
    check("no children left", wait_child(0).0 == 128);

    // DCWW_NOWAIT on a running child.
    let pid = start_child("block sleep");
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut got = 0;
    let rc = unsafe { DosWaitChild(DCWA_PROCESS, DCWW_NOWAIT, &mut res, &mut got, pid) };
    check("DosWaitChild NOWAIT on a running child", rc == 129);
    unsafe { DosKillProcess(0, pid) };
    wait_child(pid);
    check("DosWaitChild unknown pid", wait_child(9999).0 == 303);

    // A tree of processes, each waiting for its own children.
    let (cause, code) = run_child("tree 3");
    check("process tree of 15", cause == 0 && code == 15);

    // Children whose parent never waits for them must not stay behind.
    let (cause, _) = run_child("orphans");
    unsafe { DosSleep(300) };
    let after = counters();
    if after.processes != before.processes || after.threads != before.threads {
        println!("  before {:?}", before);
        println!("  after  {:?}", after);
    }
    check("orphaned children are reaped", cause == 0 && after.processes == before.processes);
    check("no processes leaked by the process tests", after.processes == before.processes && after.threads == before.threads);
}
