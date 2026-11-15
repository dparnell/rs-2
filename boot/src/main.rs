//! rs2 UEFI loader.
//!
//! Loads `\RS2\KERNEL` (an ELF64 image linked at `bootinfo::KERNEL_BASE`) and
//! every file in `\RS2\MODULES`, sets up the kernel's page tables, exits boot
//! services and jumps to the kernel entry point with a `BootInfo` pointer in
//! `rdi`.

#![no_std]
#![no_main]

mod efi;

use core::ffi::c_void;
use core::fmt::{self, Write};
use core::ptr;

use bootinfo::{BootDevice, BootInfo, Framebuffer, MemoryKind, MemoryRegion, Module, KERNEL_BASE, PHYS_MAP_BASE};
use efi::*;

static mut ST: *mut SystemTable = ptr::null_mut();

fn st() -> &'static SystemTable {
    unsafe { &*ST }
}

fn bs() -> &'static BootServices {
    unsafe { &*st().boot_services }
}

struct Console;

/// Everything printed, for `\LOADER.LOG` (`bootdiag`: the Surface's
/// mode list scrolled off the screen too fast to read).
static mut LOG: [u8; 32 * 1024] = [0; 32 * 1024];
static mut LOG_LEN: usize = 0;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            let log = &mut *ptr::addr_of_mut!(LOG);
            let n = s.len().min(log.len() - LOG_LEN);
            log[LOG_LEN..LOG_LEN + n].copy_from_slice(&s.as_bytes()[..n]);
            LOG_LEN += n;
        }
        let out = st().con_out;
        let mut buf = [0u16; 128];
        let mut n = 0;
        let flush = |buf: &mut [u16; 128], n: &mut usize| {
            buf[*n] = 0;
            unsafe { ((*out).output_string)(out, buf.as_ptr()) };
            *n = 0;
        };
        for c in s.chars() {
            if c == '\n' {
                buf[n] = b'\r' as u16;
                n += 1;
            }
            buf[n] = if (c as u32) < 0x10000 { c as u16 } else { b'?' as u16 };
            n += 1;
            if n >= 120 {
                flush(&mut buf, &mut n);
            }
        }
        flush(&mut buf, &mut n);
        Ok(())
    }
}

macro_rules! println {
    ($($arg:tt)*) => {{
        let _ = writeln!(Console, $($arg)*);
    }};
}

fn check(status: Status, what: &str) {
    if status != SUCCESS {
        panic!("{what} failed: status {status:#x}");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        if !ST.is_null() {
            println!("rs2 loader panic: {info}");
        }
    }
    loop {
        unsafe { core::arch::asm!("cli; hlt") };
    }
}

/// Pages the kernel keeps (its image, modules, page tables, stack, boot
/// info), merged where they touch. They are allocated as plain
/// EfiLoaderData and told apart here: a Surface Pro 4 refused
/// ExitBootServices for good once the loader had pages of an OS-defined
/// type (0x80000001, as it used to), as some vendors' firmware does;
/// OVMF and VMware never minded.
static mut KEPT: [(u64, u64); 256] = [(0, 0); 256];
static mut KEPT_N: usize = 0;

fn kept() -> &'static mut [(u64, u64)] {
    unsafe { &mut (&mut *ptr::addr_of_mut!(KEPT))[..KEPT_N] }
}

fn keep(start: u64, len: u64) {
    for r in kept().iter_mut() {
        if r.0 + r.1 == start {
            r.1 += len;
            return;
        }
        if start + len == r.0 {
            *r = (start, r.1 + len);
            return;
        }
    }
    unsafe {
        if KEPT_N == (*ptr::addr_of!(KEPT)).len() {
            panic!("too many separate allocations");
        }
        (*ptr::addr_of_mut!(KEPT))[KEPT_N] = (start, len);
        KEPT_N += 1;
    }
}

/// Give back pages from `alloc_pages`.
fn free_pages(start: u64, count: usize) {
    (bs().free_pages)(start, count);
    let (a, b) = (start, start + count as u64 * 4096);
    if let Some(i) = kept().iter().position(|r| r.0 <= a && b <= r.0 + r.1) {
        let r = kept()[i];
        // What is left on each side.
        kept()[i] = (r.0, a - r.0);
        if r.0 + r.1 > b {
            keep(b, r.0 + r.1 - b);
        }
    }
}

/// Allocate zeroed pages the kernel will keep.
fn alloc_pages(count: usize) -> u64 {
    let mut addr = 0u64;
    check((bs().allocate_pages)(0 /* AnyPages */, memtype::LOADER_DATA, count, &mut addr), "AllocatePages");
    unsafe { ptr::write_bytes(addr as *mut u8, 0, count * 4096) };
    keep(addr, count as u64 * 4096);
    addr
}

/// Convert an ASCII path into a NUL-terminated UCS-2 buffer.
fn ucs2(s: &str, buf: &mut [u16; 260]) -> *const u16 {
    let mut n = 0;
    for b in s.bytes() {
        buf[n] = b as u16;
        n += 1;
    }
    buf[n] = 0;
    buf.as_ptr()
}

struct Dir(*mut File);

impl Dir {
    /// Replace `path` with what the loader printed so far. Errors (a CD,
    /// a read-only stick) are ignored.
    fn write_log(&self, path: &str) {
        let mut name = [0u16; 260];
        let mut f = ptr::null_mut();
        unsafe {
            // Delete first, so a shorter log leaves no tail of an old one.
            if ((*self.0).open)(self.0, &mut f, ucs2(path, &mut name), FILE_MODE_READ | FILE_MODE_WRITE, 0) == SUCCESS {
                ((*f).delete)(f);
            }
            let mode = FILE_MODE_READ | FILE_MODE_WRITE | FILE_MODE_CREATE;
            if ((*self.0).open)(self.0, &mut f, ucs2(path, &mut name), mode, 0) != SUCCESS {
                return;
            }
            let mut len = LOG_LEN;
            ((*f).write)(f, &mut len, ptr::addr_of!(LOG) as *const u8);
            ((*f).close)(f);
        }
    }

    fn open(&self, path: &str) -> Option<*mut File> {
        let mut name = [0u16; 260];
        let mut f = ptr::null_mut();
        let s = unsafe { ((*self.0).open)(self.0, &mut f, ucs2(path, &mut name), FILE_MODE_READ, 0) };
        (s == SUCCESS).then_some(f)
    }
}

/// Read a whole file into freshly allocated pages. Returns (phys, len).
fn read_file(f: *mut File) -> (u64, u64) {
    let mut info = [0u8; 512];
    let mut size = info.len();
    unsafe {
        check(((*f).get_info)(f, &FILE_INFO_GUID, &mut size, info.as_mut_ptr()), "GetInfo");
        let len = (*(info.as_ptr() as *const FileInfo)).file_size;
        let base = alloc_pages((len as usize).div_ceil(4096).max(1));
        let mut done = 0u64;
        while done < len {
            let mut chunk = (len - done).min(16 << 20) as usize;
            check(((*f).read)(f, &mut chunk, (base + done) as *mut u8), "Read");
            if chunk == 0 {
                break;
            }
            done += chunk as u64;
        }
        ((*f).close)(f);
        (base, len)
    }
}

/// Iterate a directory, loading every regular file as a module.
fn load_modules(root: &Dir, info: &mut BootInfo) {
    let Some(dir) = root.open("\\RS2\\MODULES") else { return };
    let mut buf = [0u8; 1024];
    loop {
        let mut size = buf.len();
        let s = unsafe { ((*dir).read)(dir, &mut size, buf.as_mut_ptr()) };
        if s != SUCCESS || size == 0 {
            break;
        }
        let fi = unsafe { &*(buf.as_ptr() as *const FileInfo) };
        if fi.attribute & FILE_DIRECTORY != 0 {
            continue;
        }
        let name16 = unsafe { buf.as_ptr().add(core::mem::size_of::<FileInfo>()) as *const u16 };
        let mut name = [0u8; 32];
        let mut path = [0u16; 260];
        let prefix = "\\RS2\\MODULES\\";
        let mut n = 0;
        for b in prefix.bytes() {
            path[n] = b as u16;
            n += 1;
        }
        let mut i = 0;
        loop {
            let c = unsafe { *name16.add(i) };
            if c == 0 || n >= 259 {
                break;
            }
            path[n] = c;
            n += 1;
            if i < 31 {
                name[i] = c as u8;
            }
            i += 1;
        }
        path[n] = 0;
        let mut f = ptr::null_mut();
        if unsafe { ((*root.0).open)(root.0, &mut f, path.as_ptr(), FILE_MODE_READ, 0) } != SUCCESS {
            continue;
        }
        let (base, len) = read_file(f);
        let idx = info.module_count as usize;
        if idx < info.modules.len() {
            info.modules[idx] = Module { name, base, len };
            info.module_count += 1;
            println!("  module {} {:#x} bytes", info.modules[idx].name(), len);
        }
    }
    unsafe { ((*dir).close)(dir) };
}

// ---- ELF ----

struct Kernel {
    entry: u64,
    phys: u64,
    vbase: u64,
    len: u64,
}

fn load_elf(image: &[u8]) -> Kernel {
    let rd16 = |o: usize| u16::from_le_bytes([image[o], image[o + 1]]);
    let rd32 = |o: usize| u32::from_le_bytes(image[o..o + 4].try_into().unwrap());
    let rd64 = |o: usize| u64::from_le_bytes(image[o..o + 8].try_into().unwrap());
    assert!(&image[..4] == b"\x7fELF" && image[4] == 2, "kernel is not ELF64");
    let entry = rd64(0x18);
    let phoff = rd64(0x20) as usize;
    let phentsize = rd16(0x36) as usize;
    let phnum = rd16(0x38) as usize;
    let (mut lo, mut hi) = (u64::MAX, 0u64);
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if rd32(ph) != 1 {
            continue;
        }
        let va = rd64(ph + 0x10);
        let memsz = rd64(ph + 0x28);
        lo = lo.min(va & !0xfff);
        hi = hi.max((va + memsz + 0xfff) & !0xfff);
    }
    assert!(lo >= KERNEL_BASE && hi > lo, "kernel has bad load addresses");
    let len = hi - lo;
    let phys = alloc_pages((len / 4096) as usize);
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if rd32(ph) != 1 {
            continue;
        }
        let off = rd64(ph + 0x08) as usize;
        let va = rd64(ph + 0x10);
        let filesz = rd64(ph + 0x20) as usize;
        unsafe {
            ptr::copy_nonoverlapping(
                image.as_ptr().add(off),
                (phys + (va - lo)) as *mut u8,
                filesz,
            );
        }
    }
    Kernel { entry, phys, vbase: lo, len }
}

// ---- Paging ----

const PRESENT: u64 = 1;
const WRITABLE: u64 = 2;
const HUGE: u64 = 1 << 7;

unsafe fn table(entry: *mut u64) -> *mut u64 {
    unsafe {
        if *entry & PRESENT == 0 {
            *entry = alloc_pages(1) | PRESENT | WRITABLE;
        }
        (*entry & 0x000f_ffff_ffff_f000) as *mut u64
    }
}

/// Build the kernel's page tables. Returns the physical address of the PML4.
fn build_page_tables(kernel: &Kernel, phys_top: u64) -> u64 {
    let pml4 = alloc_pages(1) as *mut u64;
    unsafe {
        // Direct map of physical memory using 2 MB pages; the same PDPTs are
        // used for the identity map the loader needs while switching.
        let gigs = phys_top.div_ceil(1 << 30);
        for g in 0..gigs {
            let pml4_i = (g / 512) as usize;
            let high = table(pml4.add(256 + pml4_i));
            *pml4.add(pml4_i) = *pml4.add(256 + pml4_i);
            let pd = table(high.add((g % 512) as usize));
            for m in 0..512u64 {
                *pd.add(m as usize) = ((g << 30) | (m << 21)) | PRESENT | WRITABLE | HUGE;
            }
        }
        // Kernel image with 4 KB pages.
        for off in (0..kernel.len).step_by(4096) {
            let va = kernel.vbase + off;
            let pdpt = table(pml4.add(((va >> 39) & 511) as usize));
            let pd = table(pdpt.add(((va >> 30) & 511) as usize));
            let pt = table(pd.add(((va >> 21) & 511) as usize));
            *pt.add(((va >> 12) & 511) as usize) = (kernel.phys + off) | PRESENT | WRITABLE;
        }
    }
    pml4 as u64
}

/// Walk the boot device's device path for a hard drive or CD-ROM node.
fn boot_device(device: Handle) -> BootDevice {
    let mut bd = BootDevice::default();
    let mut dp: *mut c_void = ptr::null_mut();
    if (bs().handle_protocol)(device, &DEVICE_PATH_GUID, &mut dp) != SUCCESS {
        return bd;
    }
    let mut p = dp as *const u8;
    unsafe {
        loop {
            let ty = *p;
            let sub = *p.add(1);
            let len = u16::from_le_bytes([*p.add(2), *p.add(3)]) as usize;
            if ty == 0x7f || len < 4 {
                break;
            }
            if ty == 4 && sub == 1 && len >= 42 {
                // Media / Hard Drive node.
                bd.kind = 1;
                bd.partition = (p.add(4) as *const u32).read_unaligned();
                bd.start = (p.add(8) as *const u64).read_unaligned();
                bd.size = (p.add(16) as *const u64).read_unaligned();
                core::ptr::copy_nonoverlapping(p.add(24), bd.signature.as_mut_ptr(), 16);
                bd.sig_type = *p.add(41);
            } else if ty == 4 && sub == 2 && bd.kind == 0 {
                bd.kind = 2;
            }
            p = p.add(len);
        }
    }
    bd
}

fn find_rsdp() -> u64 {
    let s = st();
    let tables = unsafe { core::slice::from_raw_parts(s.config_table, s.table_count) };
    let find = |g: Guid| tables.iter().find(|t| t.guid == g).map(|t| t.table as u64);
    find(ACPI20_GUID).or_else(|| find(ACPI10_GUID)).unwrap_or(0)
}

/// Set the GOP mode `want` (from `video=` on the command line) if the
/// firmware offers it, else 1024x768, the classic OS/2 desktop
/// resolution, if offered; otherwise keep the current mode.
/// Running in a virtual machine (CPUID's "hypervisor present" bit).
fn hypervisor() -> bool {
    core::arch::x86_64::__cpuid(1).ecx & (1 << 31) != 0
}

/// The pixel layout of a GOP mode as `Framebuffer::format` (0 RGBX,
/// 1 BGRX), counting a bitmask mode whose masks are one of those two;
/// None for anything the kernel cannot draw on.
fn pixel_layout(i: &efi::GopModeInfo) -> Option<u32> {
    match i.pixel_format {
        0 | 1 => Some(i.pixel_format),
        2 if i.masks[..3] == [0xff, 0xff00, 0xff_0000] => Some(0),
        2 if i.masks[..3] == [0xff_0000, 0xff00, 0xff] => Some(1),
        _ => None,
    }
}

fn setup_framebuffer(want: Option<(u32, u32)>, diag: bool) -> Framebuffer {
    let mut gop: *mut c_void = ptr::null_mut();
    let mut fb = Framebuffer { base: 0, size: 0, width: 0, height: 0, stride: 0, format: 0 };
    if (bs().locate_protocol)(&GOP_GUID, ptr::null_mut(), &mut gop) != SUCCESS {
        return fb;
    }
    let gop = gop as *mut Gop;
    unsafe {
        let mode = &*(*gop).mode;
        let find = |w: u32, h: u32| {
            (0..mode.max_mode).find(|&m| {
                let mut size = 0;
                let mut info = ptr::null_mut();
                ((*gop).query_mode)(gop, m, &mut size, &mut info) == SUCCESS && {
                    let i = &*info;
                    i.width == w && i.height == h && pixel_layout(i).is_some()
                }
            })
        };
        let booted = mode.mode;
        // Without `video=`, real hardware gets its largest mode (the
        // panel's own): a Surface Pro 4 boots in 1024x768, but that mode
        // is a buffer its Blt scales onto the 2736x1824 panel, so the
        // firmware's text shows and nothing written straight into the
        // "framebuffer" ever does. A virtual machine's modes are all
        // real; 1024x768 there, as before.
        let largest = || {
            (0..mode.max_mode)
                .filter_map(|m| {
                    let mut size = 0;
                    let mut info = ptr::null_mut();
                    (((*gop).query_mode)(gop, m, &mut size, &mut info) == SUCCESS && pixel_layout(&*info).is_some())
                        .then(|| (m, (*info).width as u64 * (*info).height as u64))
                })
                .max_by_key(|&(_, area)| area)
                .map(|(m, _)| m)
        };
        let default = if hypervisor() { find(1024, 768) } else { largest() };
        if let Some(m) = want.and_then(|(w, h)| find(w, h)).or(default) {
            if m != mode.mode {
                ((*gop).set_mode)(gop, m);
            }
        }
        // Listed after the switch, which clears the screen.
        if diag {
            println!("  video modes (firmware booted in {booted}, now {}):", (*(*gop).mode).mode);
            for m in 0..mode.max_mode {
                let mut size = 0;
                let mut info = ptr::null_mut();
                if ((*gop).query_mode)(gop, m, &mut size, &mut info) == SUCCESS {
                    let i = &*info;
                    println!("    {m}: {}x{} format {} masks {:x?} stride {}", i.width, i.height, i.pixel_format, i.masks, i.stride);
                }
            }
        }
        let mode = &*(*gop).mode;
        let info = &*mode.info;
        if let Some(format) = pixel_layout(info) {
            fb = Framebuffer {
                base: mode.fb_base,
                size: mode.fb_size as u64,
                width: info.width,
                height: info.height,
                stride: info.stride,
                format,
            };
        }
    }
    fb
}

/// Wait a moment for T (a text-mode boot: the command prompt instead of
/// the desktop), which adds "textmode" to the kernel command line, or R
/// (or F1, as OS/2's Alt+F1) for the Recovery Choices before the desktop
/// starts, which adds "recovery". "bootwait=MS" on the command line sets
/// the wait (0: none).
fn boot_key(info: &mut BootInfo) {
    let cmd = info.cmdline();
    let ms = cmd.split_whitespace().find_map(|w| w.strip_prefix("bootwait=")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1000);
    if ms == 0 || cmd.split_whitespace().any(|w| w == "textmode") {
        return;
    }
    let input = st().con_in;
    if input.is_null() {
        return;
    }
    println!("Press T for a text-mode boot (a command prompt instead of the desktop), R for Recovery Choices.");
    let mut add: Option<&[u8]> = None;
    for _ in 0..ms.div_ceil(20) {
        let mut k = efi::InputKey::default();
        while unsafe { ((*input).read_key_stroke)(input, &mut k) } == 0 {
            match k.unicode_char {
                0x74 | 0x54 => add = Some(b"textmode"),
                0x72 | 0x52 => add = Some(b"recovery"),
                // SCAN_F1
                0 if k.scan_code == 0x0b => add = Some(b"recovery"),
                _ => {}
            }
        }
        if add.is_some() {
            break;
        }
        (bs().stall)(20_000);
    }
    if let Some(word) = add {
        println!("  {}", if word == b"textmode" { "text-mode boot" } else { "Recovery Choices" });
        let n = info.cmdline.iter().position(|&b| b == 0).unwrap_or(info.cmdline.len());
        let sep: &[u8] = if n == 0 { b"" } else { b" " };
        if n + sep.len() + word.len() < info.cmdline.len() {
            info.cmdline[n..n + sep.len()].copy_from_slice(sep);
            info.cmdline[n + sep.len()..n + sep.len() + word.len()].copy_from_slice(word);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main(image: Handle, system_table: *mut SystemTable) -> Status {
    unsafe { ST = system_table };
    (bs().set_watchdog_timer)(0, 0, 0, ptr::null());
    println!("rs2 loader");

    let mut li: *mut c_void = ptr::null_mut();
    check((bs().handle_protocol)(image, &LOADED_IMAGE_GUID, &mut li), "LoadedImage");
    let device = unsafe { (*(li as *mut LoadedImage)).device };
    let mut fs: *mut c_void = ptr::null_mut();
    check((bs().handle_protocol)(device, &SIMPLE_FS_GUID, &mut fs), "SimpleFileSystem");
    let mut root = ptr::null_mut();
    check(unsafe { ((*(fs as *mut SimpleFs)).open_volume)(fs as *mut SimpleFs, &mut root) }, "OpenVolume");
    let root = Dir(root);
    let boot_dev = boot_device(device);

    let info_pages = core::mem::size_of::<BootInfo>().div_ceil(4096);
    let info_phys = alloc_pages(info_pages);
    let info = unsafe { &mut *(info_phys as *mut BootInfo) };
    info.magic = bootinfo::MAGIC;

    let kf = root.open("\\RS2\\KERNEL").expect("\\RS2\\KERNEL not found");
    let (kimg, klen) = read_file(kf);
    let kernel = load_elf(unsafe { core::slice::from_raw_parts(kimg as *const u8, klen as usize) });
    free_pages(kimg, (klen as usize).div_ceil(4096));
    info.kernel_phys = kernel.phys;
    info.kernel_len = kernel.len;
    println!("  kernel {:#x} bytes at {:#x}, entry {:#x}", kernel.len, kernel.phys, kernel.entry);

    if let Some(f) = root.open("\\RS2\\CMDLINE") {
        let (base, len) = read_file(f);
        let n = (len as usize).min(bootinfo::CMDLINE_MAX - 1);
        let src = unsafe { core::slice::from_raw_parts(base as *const u8, n) };
        for (d, &s) in info.cmdline.iter_mut().zip(src) {
            *d = if s == b'\r' || s == b'\n' { b' ' } else { s };
        }
    }
    boot_key(info);
    load_modules(&root, info);
    info.rsdp = find_rsdp();
    info.boot_device = boot_dev;
    println!("  boot device kind {} partition {} start {}", boot_dev.kind, boot_dev.partition, boot_dev.start);
    info.framebuffer = setup_framebuffer(bootinfo::video_mode(info.cmdline()), bootinfo::diag_on(info.cmdline()));

    // Kernel stack: 256 KB, reached through the direct map.
    const STACK_PAGES: usize = 64;
    let stack = alloc_pages(STACK_PAGES);
    let stack_top = PHYS_MAP_BASE + stack + (STACK_PAGES * 4096) as u64;

    // The memory map must be fetched last; allocating page tables changes
    // it, so size the tables from a first look at the map, then fetch again.
    let mut map_buf = [0u8; 32 * 1024];
    let mut phys_top = 4u64 << 30;
    {
        let (mut size, mut key, mut dsize, mut ver) = (map_buf.len(), 0, 0, 0);
        check((bs().get_memory_map)(&mut size, map_buf.as_mut_ptr(), &mut key, &mut dsize, &mut ver), "GetMemoryMap");
        for i in 0..size / dsize {
            let d = unsafe { &*(map_buf.as_ptr().add(i * dsize) as *const MemoryDescriptor) };
            // Map RAM (and firmware areas); device windows are mapped by
            // the kernel on demand.
            if !matches!(d.ty, 0 | 11 | 12) {
                phys_top = phys_top.max(d.phys_start + d.pages * 4096);
            }
        }
    }
    phys_top = phys_top.max(info.framebuffer.base + info.framebuffer.size);
    let pml4 = build_page_tables(&kernel, phys_top);

    // `bootdiag`: what the kernel will be handed, while the firmware can
    // still print it, then a square for each step after (bootdiag.rs in
    // the kernel draws the rest).
    let diag = bootinfo::diag_on(info.cmdline());
    if diag {
        let fb = &info.framebuffer;
        println!(
            "  framebuffer {}x{} stride {} format {} at {:#x} ({:#x} bytes){}",
            fb.width,
            fb.height,
            fb.stride,
            fb.format,
            fb.base,
            fb.size,
            if fb.base == 0 { ": none usable, the kernel will show nothing" } else { "" }
        );
        println!("  memory up to {:#x}, page tables at {:#x}, stack top {:#x}, info at {:#x}", phys_top, pml4, stack_top, info_phys);
        println!("  bootdiag: squares at the top: blue = loader, green (every 5th yellow) = kernel stages");
    }
    println!("  exiting boot services");
    if diag {
        root.write_log("\\LOADER.LOG");
    }
    if diag {
        bootinfo::diag_square(&info.framebuffer, 0, 0, 0x0060ff);
    }
    let (mut size, mut key, mut dsize, mut ver);
    // The map changes between the two calls only if the firmware does
    // something meanwhile; a few tries, not forever.
    let mut tries = 0;
    loop {
        size = map_buf.len();
        key = 0;
        dsize = 0;
        ver = 0;
        check((bs().get_memory_map)(&mut size, map_buf.as_mut_ptr(), &mut key, &mut dsize, &mut ver), "GetMemoryMap");
        let status = (bs().exit_boot_services)(image, key);
        if status == SUCCESS {
            break;
        }
        tries += 1;
        // A red square 1 for each refusal (the firmware may not print
        // any more after one).
        if diag {
            bootinfo::diag_square(&info.framebuffer, 0, 1, 0xe00000);
            // The firmware's error number as that many red squares on
            // the second row (2 = invalid parameter: a stale map key).
            if tries == 1 {
                for k in 0..(status & 0xff).min(48) as u32 {
                    bootinfo::diag_square(&info.framebuffer, 0, 48 + k, 0xe00000);
                }
            }
        }
        if tries == 16 {
            panic!("ExitBootServices failed 16 times, last status {status:#x}");
        }
    }
    // No firmware calls from here on.
    if diag {
        bootinfo::diag_square(&info.framebuffer, 0, 1, 0x0060ff);
    }
    let mut count = 0;
    let mut push = |start: u64, len: u64, kind: MemoryKind| {
        if len == 0 {
            return;
        }
        // Merge with the previous region when contiguous and the same kind.
        if count > 0 {
            let prev = &mut info.memory[count - 1];
            if prev.kind == kind && prev.start + prev.len == start {
                prev.len += len;
                return;
            }
        }
        if count < info.memory.len() {
            info.memory[count] = MemoryRegion { start, len, kind, _pad: 0 };
            count += 1;
        }
    };
    for i in 0..size / dsize {
        let d = unsafe { &*(map_buf.as_ptr().add(i * dsize) as *const MemoryDescriptor) };
        let (start, end) = (d.phys_start, d.phys_start + d.pages * 4096);
        if d.ty == memtype::LOADER_DATA {
            // The kernel's pieces in it are kept, the rest is free.
            let mut at = start;
            while at < end {
                let next = kept().iter().filter(|r| r.0 + r.1 > at && r.0 < end).min_by_key(|r| r.0).copied();
                match next {
                    Some((ks, kl)) if ks <= at => {
                        let e = (ks + kl).min(end);
                        push(at, e - at, MemoryKind::Loader);
                        at = e;
                    }
                    Some((ks, _)) => {
                        push(at, ks - at, MemoryKind::Usable);
                        at = ks;
                    }
                    None => {
                        push(at, end - at, MemoryKind::Usable);
                        at = end;
                    }
                }
            }
            continue;
        }
        let kind = match d.ty {
            memtype::LOADER_CODE | memtype::BOOT_CODE | memtype::BOOT_DATA | memtype::CONVENTIONAL => MemoryKind::Usable,
            memtype::RUNTIME_CODE | memtype::RUNTIME_DATA => MemoryKind::Runtime,
            memtype::ACPI_RECLAIM => MemoryKind::AcpiReclaim,
            _ => MemoryKind::Reserved,
        };
        push(start, end - start, kind);
    }
    info.memory_count = count as u64;
    if diag {
        bootinfo::diag_square(&info.framebuffer, 0, 2, 0x0060ff);
    }

    unsafe {
        core::arch::asm!(
            "mov cr3, {pml4}",
            "mov rsp, {stack}",
            "xor ebp, ebp",
            "jmp {entry}",
            pml4 = in(reg) pml4,
            stack = in(reg) stack_top,
            entry = in(reg) kernel.entry,
            in("rdi") PHYS_MAP_BASE + info_phys,
            options(noreturn)
        );
    }
}
