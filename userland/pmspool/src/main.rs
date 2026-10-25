//! OS2\PMSPOOL.EXE: the spooler process. As IBM's PMSPOOL it is only a
//! home for PMSPL's spooler thread: it starts PM (the queue processors
//! draw with the GPI), calls SPL32QMINITIALIZE and sleeps. It ends at
//! once when a spooler is already running.

#![no_std]
#![no_main]

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, cmsg: i32) -> u32;
    #[link_name = "PMWIN.705"]
    fn WinCancelShutdown(hmq: u32, fl: u32) -> u32;
    #[link_name = "PMSPL.312"]
    fn SplQmInitialize(r: *mut u32) -> u32;
}

fn main(_s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    unsafe { WinCancelShutdown(hmq, 1) };
    let mut r = 0;
    if unsafe { SplQmInitialize(&mut r) } != 0 {
        return 1;
    }
    loop {
        unsafe { os2::DosSleep(0xffff_ffff) };
    }
}
