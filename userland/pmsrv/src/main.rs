//! PMSRV.EXE: the Presentation Manager server. PMWIN.DLL starts it with
//! the first PM program; all of it is in PMWIN (`Rs2PmServer`, see
//! userland/pmwin/src/server.rs), which runs until the last PM program
//! has ended.

#![no_std]
#![no_main]

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.9000"]
    fn Rs2PmServer() -> u32;
    #[link_name = "DOSCALLS.382"]
    fn DosSetRelMaxFH(req: *mut i32, max: *mut u32) -> u32;
}

fn main(_s: &os2::Startup) -> u32 {
    // The server is a child of whichever program first used PM and
    // inherits that program's handles: one started as `prog >> LOG` left
    // LOG open in the server, and the next command could not open it
    // (SYS0032) until the last PM program had ended. Open Watcom 2.0's C
    // runtime DLL (CLBR20) imports PMWIN, so even its console programs
    // start the server. Nothing of the starter's is the server's business.
    let (mut req, mut max) = (0i32, 0u32);
    unsafe { DosSetRelMaxFH(&mut req, &mut max) };
    for h in 0..max {
        unsafe { os2::dos::DosClose(h) };
    }
    unsafe { Rs2PmServer() }
}
