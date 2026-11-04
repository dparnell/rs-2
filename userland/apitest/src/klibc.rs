//! What kLIBC (the GCC C library of OS/2, LIBCN0.DLL) and the programs
//! built with it expect of DOSCALLS beyond the documented API.

use core::ptr::null;

use os2::*;

use crate::check;

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    // DOSCALLS' constant exports (kLIBC's dos-data.imp): the import's
    // "address" is the value.
    #[link_name = "DOSCALLS.214"]
    static DosMaxPathLen: u8;
    #[link_name = "DOSCALLS.215"]
    static DosPageSize: u8;
    #[link_name = "DOSCALLS.216"]
    static DosLocalInfo: u8;
    #[link_name = "DOSCALLS.217"]
    static DosGlobalInfo: u8;
    #[link_name = "DOSCALLS.369"]
    static DosFlatCS: u8;
    #[link_name = "DOSCALLS.419"]
    static Dos32TIB: u8;
}

unsafe extern "system" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> APIRET;
}

unsafe extern "C" {
    #[link_name = "UCONV.1"]
    fn UniCreateUconvObject(name: *const u16, obj: *mut u32) -> i32;
    #[link_name = "UCONV.2"]
    fn UniUconvToUcs(obj: u32, inp: *mut *const u8, inb: *mut u32, out: *mut *mut u16, outc: *mut u32, nonid: *mut u32) -> i32;
    #[link_name = "UCONV.4"]
    fn UniFreeUconvObject(obj: u32) -> i32;
    #[link_name = "LIBUNI.58"]
    fn UniTransUpper(lo: u32, uc: u32) -> u32;
    #[link_name = "LIBUNI.53"]
    fn UniStrxfrm(lo: u32, d: *mut u16, s: *const u16, n: u32) -> u32;
    #[link_name = "LIBUNI.74"]
    fn UniQueryAttr(name: *const u16) -> u32;
}

pub fn run() {
    unsafe {
        let v = |p: &u8| p as *const u8 as u32;
        check("DOSCALLS.214 DosMaxPathLen", v(&DosMaxPathLen) == 260);
        check("DOSCALLS.215 DosPageSize", v(&DosPageSize) == 4096);
        let fs: u32;
        let cs: u32;
        core::arch::asm!("mov {0:e}, fs", "mov {1:e}, cs", out(reg) fs, out(reg) cs);
        check("DOSCALLS.419 DOS32TIB is FS", v(&Dos32TIB) == fs & 0xffff);
        check("DOSCALLS.369 DosFlatCS is CS", v(&DosFlatCS) == cs & 0xffff);
        // kLIBC's GETLINFOSEG/GETGINFOSEG: the selector's tile is the
        // segment's flat address.
        let lis = ((v(&DosLocalInfo) & 0xfff8) << 13) as *const u16;
        let gis = ((v(&DosGlobalInfo) & 0xfff8) << 13) as *const u8;
        let (mut tib, mut pib): (*mut Tib, *mut Pib) = (core::ptr::null_mut(), core::ptr::null_mut());
        DosGetInfoBlocks(&mut tib, &mut pib);
        check("DosLocalInfo: LINFOSEG pid", *lis == (*pib).pid as u16);
        check("DosLocalInfo: LINFOSEG tid", *lis.add(3) == 1);
        // GINFOSEG: major/minor version at 21/22.
        check("DosGlobalInfo: GINFOSEG version", *gis.add(21) == 20 && *gis.add(22) == 45);

        // The tiled selector of an address in a 32-bit code object is a
        // code selector (kLIBC's 16-bit thunk RETFs through it).
        let sel = ((run as usize as u32) >> 13) | 7;
        let ar: u32;
        let zf: u8;
        core::arch::asm!("lar {0:e}, {1:e}", "setz {2}", out(reg) ar, in(reg) sel, out(reg_byte) zf);
        check("tile of 32-bit code is a code selector", zf == 1 && ar & 0x0800 != 0);

        // A private event semaphore opened again by its handle in the
        // creating process (kLIBC's heap: _fmutex_open).
        let mut hev = 0u32;
        DosCreateEventSem(null(), &mut hev, 0, 0);
        let mut h2 = hev;
        check("DosOpenEventSem private by handle", DosOpenEventSem(null(), &mut h2) == 0 && h2 == hev);
        check("close opened private sem twice", DosCloseEventSem(hev) == 0 && DosCloseEventSem(hev) == 0);
        check("private sem gone after last close", DosCloseEventSem(hev) == 6);

        // Private handles are per process, the lowest free one first
        // (kLIBC's fork recreates them in the child by number).
        let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
        DosCreateMutexSem(null(), &mut a, 0, 0);
        DosCreateMutexSem(null(), &mut b, 0, 0);
        DosCloseMutexSem(a);
        DosCreateMutexSem(null(), &mut c, 0, 0);
        check("private sem handle reused lowest first", c == a && b != a);
        DosCloseMutexSem(b);
        DosCloseMutexSem(c);

        // kLIBC's locale set-up: one byte at a time through UCONV (its
        // count of substitutions left uninitialised), UniTransUpper,
        // UniStrxfrm into a 7-character buffer.
        let mut uo = 0u32;
        let name: [u16; 1] = [0];
        let ok = UniCreateUconvObject(name.as_ptr(), &mut uo) == 0;
        let byte = [b'1'];
        let (mut ip, mut il) = (byte.as_ptr(), 1u32);
        let mut out = [0u16; 1];
        let (mut op, mut ol, mut nonid) = (out.as_mut_ptr(), 1u32, 0x5555u32);
        let rc = UniUconvToUcs(uo, &mut ip, &mut il, &mut op, &mut ol, &mut nonid);
        check("UniUconvToUcs one byte (nonid set to 0)", ok && rc == 0 && out[0] == b'1' as u16 && il == 0 && ol == 0 && nonid == 0);
        UniFreeUconvObject(uo);
        check("UniTransUpper", UniTransUpper(0, b'a' as u32) == b'A' as u32);
        let src: [u16; 2] = [b'B' as u16, 0];
        let mut w = [0xffffu16; 7];
        let n = UniStrxfrm(0, w.as_mut_ptr(), src.as_ptr(), 7);
        check("UniStrxfrm one character", n * 2 <= 7 && w[0] == b'b' as u16 && w[3] == 0);
        let xd: [u16; 7] = [b'x' as u16, b'd' as u16, b'i' as u16, b'g' as u16, b'i' as u16, b't' as u16, 0];
        check("UniQueryAttr xdigit", UniQueryAttr(xd.as_ptr()) == 0x80);
    }
}
