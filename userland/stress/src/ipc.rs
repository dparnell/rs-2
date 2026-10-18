//! Pipes, queues and named pipes under load.

use core::ptr::null;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::{check, join, thread, Rng};

const BYTES: u32 = 256 * 1024;

static PIPES: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static SUMS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static QUEUE: AtomicU32 = AtomicU32::new(0);
static NPIPE: AtomicU32 = AtomicU32::new(0);

fn checksum(sum: u32, b: &[u8]) -> u32 {
    b.iter().fold(sum, |s, &x| s.rotate_left(3) ^ x as u32)
}

/// Write BYTES pseudo-random bytes in random chunks to pipe `n`.
unsafe extern "C" fn writer(n: u32) {
    let h = PIPES[n as usize * 2 + 1].load(Ordering::Relaxed);
    let mut rng = Rng(n + 1);
    let mut buf = [0u8; 1500];
    let mut left = BYTES;
    let mut sum = 0;
    while left > 0 {
        let k = (rng.below(1500) + 1).min(left);
        for b in buf[..k as usize].iter_mut() {
            *b = rng.next() as u8;
        }
        sum = checksum(sum, &buf[..k as usize]);
        let mut done = 0;
        while done < k {
            let mut w = 0;
            if unsafe { DosWrite(h, buf[done as usize..].as_ptr(), k - done, &mut w) } != 0 {
                return;
            }
            done += w;
        }
        left -= k;
    }
    SUMS[n as usize * 2 + 1].store(sum, Ordering::Relaxed);
    unsafe { DosClose(h) };
}

/// Read pipe `n` to its end.
unsafe extern "C" fn reader(n: u32) {
    let h = PIPES[n as usize * 2].load(Ordering::Relaxed);
    let mut rng = Rng(n + 77);
    let mut buf = [0u8; 4096];
    let mut sum = 0;
    let mut total = 0;
    loop {
        let want = rng.below(4096) + 1;
        let mut got = 0;
        if unsafe { DosRead(h, buf.as_mut_ptr(), want, &mut got) } != 0 || got == 0 {
            break;
        }
        sum = checksum(sum, &buf[..got as usize]);
        total += got;
    }
    SUMS[n as usize * 2].store(if total == BYTES { sum } else { !sum }, Ordering::Relaxed);
    unsafe { DosClose(h) };
}

unsafe extern "C" fn queue_writer(n: u32) {
    let q = QUEUE.load(Ordering::Relaxed);
    for i in 0..500u32 {
        let data = n << 16 | i;
        let rc = unsafe { DosWriteQueue(q, data, 4, &data as *const u32 as *const u8, (i % 16) as u32) };
        if rc != 0 {
            return;
        }
        if i % 50 == 0 {
            unsafe { DosSleep(0) };
        }
    }
}

unsafe extern "C" fn npipe_server(rounds: u32) {
    let h = NPIPE.load(Ordering::Relaxed);
    unsafe {
        if DosConnectNPipe(h) != 0 {
            return;
        }
        let mut buf = [0u8; 64];
        // Serve until the client closes its end (read returns 0 bytes), so
        // the last reply is not thrown away by the disconnect.
        let _ = rounds;
        loop {
            let mut n = 0;
            if DosRead(h, buf.as_mut_ptr(), 64, &mut n) != 0 || n == 0 {
                break;
            }
            for b in buf[..n as usize].iter_mut() {
                *b = b.to_ascii_uppercase();
            }
            let mut w = 0;
            DosWrite(h, buf.as_ptr(), n, &mut w);
        }
        DosDisConnectNPipe(h);
    }
}

pub fn run() {
    // Four pipe pairs, each with a writer and a reader thread.
    unsafe {
        let mut ts = alloc::vec::Vec::new();
        for n in 0..4u32 {
            let (mut r, mut w) = (0, 0);
            DosCreatePipe(&mut r, &mut w, 512 << n);
            PIPES[n as usize * 2].store(r, Ordering::Relaxed);
            PIPES[n as usize * 2 + 1].store(w, Ordering::Relaxed);
            ts.push(thread(reader, n));
            ts.push(thread(writer, n));
        }
        for t in ts {
            join(t);
        }
        let ok = (0..4).all(|n| {
            let (r, w) = (SUMS[n * 2].load(Ordering::Relaxed), SUMS[n * 2 + 1].load(Ordering::Relaxed));
            r == w && w != 0
        });
        check("4 pipes x 256 KB with random chunk sizes", ok);
    }

    // Eight writers, one reader on a priority queue.
    unsafe {
        let mut q = 0;
        DosCreateQueue(&mut q, QUE_PRIORITY, b"\\QUEUES\\STRESS\\LOAD\0".as_ptr());
        QUEUE.store(q, Ordering::Relaxed);
        let ts: alloc::vec::Vec<u32> = (0..8).map(|n| thread(queue_writer, n)).collect();
        let mut next = [0u32; 8];
        let mut ok = true;
        let mut got = 0;
        while got < 4000 {
            let mut req = RequestData::default();
            let (mut cb, mut pb, mut prio) = (0, null(), 0u8);
            let rc = DosReadQueue(q, &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0);
            if rc != 0 || cb != 4 {
                println!("  DosReadQueue rc {rc}");
                ok = false;
                break;
            }
            // The element is the writer's pointer (not copied, as on OS/2):
            // only the request data is checked.
            let v = req.data;
            let w = (v >> 16) as usize;
            if w < 8 {
                next[w] += 1;
            }
            got += 1;
        }
        for t in ts {
            join(t);
        }
        let mut n = 99;
        DosQueryQueue(q, &mut n);
        check("8 writers x 500 queue elements", ok && next.iter().all(|&c| c == 500) && n == 0);
        DosCloseQueue(q);
    }

    // Named pipe: 500 transactions.
    unsafe {
        let name = b"\\PIPE\\STRESS\0".as_ptr();
        let mut h = 0;
        DosCreateNPipe(name, &mut h, NP_ACCESS_DUPLEX, NP_TYPE_MESSAGE | NP_READMODE_MESSAGE | 1, 512, 512, 0);
        NPIPE.store(h, Ordering::Relaxed);
        let t = thread(npipe_server, 500);
        DosWaitNPipe(name, 2000);
        let (mut hc, mut act) = (0, 0);
        let rc = DosOpen(name, &mut hc, &mut act, 0, 0, 1, 0x42, core::ptr::null_mut());
        DosSetNPHState(hc, NP_READMODE_MESSAGE);
        let mut ok = rc == 0;
        for i in 0..500u32 {
            let msg = alloc::format!("message {i}");
            let mut buf = [0u8; 64];
            let mut n = 0;
            let rc = DosTransactNPipe(hc, msg.as_ptr(), msg.len() as u32, buf.as_mut_ptr(), 64, &mut n);
            ok &= rc == 0 && &buf[..n as usize] == msg.to_ascii_uppercase().as_bytes();
            if !ok {
                println!("  transaction {i}: rc {rc}");
                break;
            }
        }
        DosClose(hc);
        join(t);
        DosClose(h);
        check("500 named pipe transactions", ok);
    }
}
