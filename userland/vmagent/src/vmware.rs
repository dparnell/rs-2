//! VMware: GuestRPC and copy/paste (guestagent::vmware) over the
//! backdoor, whose calls the kernel makes (`\DEV\VMWARE$`, IOCtl 0x80/1;
//! kernel/src/vmware.rs). The reader thread polls the host's TCLO
//! channels; the main thread answers the host's requests for the guest's
//! text. Both use the protocol state under a mutex semaphore.

use core::cell::UnsafeCell;

use guestagent::text;
use guestagent::vmware::{Agent, Backdoor, Event, Regs};

struct Port(u32);

impl Backdoor for Port {
    fn call(&mut self, r: Regs) -> Regs {
        let mut p = [0u8; 24];
        for (i, v) in [r.eax, r.ebx, r.ecx, r.edx, r.esi, r.edi].iter().enumerate() {
            p[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        let (mut cbp, mut cbd) = (24u32, 0u32);
        let rc = unsafe { os2::DosDevIOCtl(self.0, 0x80, 1, p.as_mut_ptr(), 24, &mut cbp, core::ptr::null_mut(), 0, &mut cbd) };
        if rc != 0 {
            // As a host that does not answer: no status bits.
            return Regs { eax: u32::MAX, ..r };
        }
        let w = |i: usize| u32::from_le_bytes([p[4 * i], p[4 * i + 1], p[4 * i + 2], p[4 * i + 3]]);
        Regs { eax: w(0), ebx: w(1), ecx: w(2), edx: w(3), esi: w(4), edi: w(5) }
    }
}

pub struct VMware {
    mtx: u32,
    inner: UnsafeCell<(Port, Agent)>,
}

// The state is only touched with the mutex semaphore held.
unsafe impl Sync for VMware {}

impl VMware {
    pub fn open() -> Option<VMware> {
        let (mut h, mut action) = (0u32, 0u32);
        let rc = unsafe { os2::DosOpen(b"\\DEV\\VMWARE$\0".as_ptr(), &mut h, &mut action, 0, 0, 0x01, 0x0042, core::ptr::null_mut()) };
        if rc != 0 {
            return None;
        }
        let mut port = Port(h);
        // QEMU's vmport answers the backdoor but has no GuestRPC.
        if !Agent::probe(&mut port) {
            unsafe { os2::DosClose(h) };
            return None;
        }
        let mut mtx = 0;
        unsafe { os2::DosCreateMutexSem(core::ptr::null(), &mut mtx, 0, 0) };
        Some(VMware { mtx, inner: UnsafeCell::new((port, Agent::new())) })
    }

    fn with<R>(&self, f: impl FnOnce(&mut Port, &mut Agent) -> R) -> R {
        unsafe { os2::DosRequestMutexSem(self.mtx, u32::MAX) };
        let (p, a) = unsafe { &mut *self.inner.get() };
        let r = f(p, a);
        unsafe { os2::DosReleaseMutexSem(self.mtx) };
        r
    }

    pub fn start(&self) {}

    /// The guest's clipboard changed: version 1 gives it to the host now
    /// (version 4 waits for the host to ask).
    pub fn local_changed(&self, t: Option<&[u8]>) {
        let utf8 = t.map(|t| text::to_utf8(crate::code_page(), t));
        self.with(|p, a| {
            if a.version == 1 {
                a.send_clipboard(p, utf8.as_deref());
            }
        });
    }

    pub fn reply(&self, t: Option<&[u8]>) {
        let utf8 = t.map(|t| text::to_utf8(crate::code_page(), t));
        self.with(|p, a| a.send_clipboard(p, utf8.as_deref()));
    }

    pub fn read_loop(&self) {
        let mut delay = 0u32;
        loop {
            let (events, busy) = self.with(|p, a| a.poll(p));
            for e in events {
                match e {
                    Event::HostText(t) => {
                        self.with(|_, a| a.note_host_text(&t));
                        crate::host_text(text::from_utf8(crate::code_page(), &t));
                    }
                    Event::HostRequest => crate::host_request(),
                    Event::Ready(_) => crate::host_ready(),
                }
            }
            // open-vm-tools' back-off: 10, 20, 40, 80, 100 ms.
            delay = if busy { 0 } else { (delay * 2).clamp(10, 100) };
            unsafe { os2::DosSleep(delay.max(1)) };
        }
    }
}
