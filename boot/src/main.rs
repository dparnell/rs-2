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

use bootinfo::{BootInfo, Framebuffer, MemoryKind, MemoryRegion, Module, KERNEL_BASE, PHYS_MAP_BASE};
use efi::*;

static mut ST: *mut SystemTable = ptr::null_mut();

fn st() -> &'static SystemTable {
    unsafe { &*ST }
}

fn bs() -> &'static BootServices {
    unsafe { &*st().boot_services }
}

struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
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

/// Allocate zeroed pages the kernel will keep.
fn alloc_pages(count: usize) -> u64 {
    let mut addr = 0u64;
    check(
        (bs().allocate_pages)(0 /* AnyPages */, memtype::RS2_KEEP, count, &mut addr),
        "AllocatePages",
    );
    unsafe { ptr::write_bytes(addr as *mut u8, 0, count * 4096) };
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

fn find_rsdp() -> u64 {
    let s = st();
    let tables = unsafe { core::slice::from_raw_parts(s.config_table, s.table_count) };
    let find = |g: Guid| tables.iter().find(|t| t.guid == g).map(|t| t.table as u64);
    find(ACPI20_GUID).or_else(|| find(ACPI10_GUID)).unwrap_or(0)
}

fn setup_framebuffer() -> Framebuffer {
    let mut gop: *mut c_void = ptr::null_mut();
    let mut fb = Framebuffer { base: 0, size: 0, width: 0, height: 0, stride: 0, format: 0 };
    if (bs().locate_protocol)(&GOP_GUID, ptr::null_mut(), &mut gop) != SUCCESS {
        return fb;
    }
    let gop = gop as *mut Gop;
    unsafe {
        // Prefer 1024x768, the classic OS/2 desktop resolution, if offered.
        let mode = &*(*gop).mode;
        for m in 0..mode.max_mode {
            let mut size = 0;
            let mut info = ptr::null_mut();
            if ((*gop).query_mode)(gop, m, &mut size, &mut info) == SUCCESS {
                let i = &*info;
                if i.width == 1024 && i.height == 768 && i.pixel_format < 2 {
                    ((*gop).set_mode)(gop, m);
                    break;
                }
            }
        }
        let mode = &*(*gop).mode;
        let info = &*mode.info;
        if info.pixel_format < 2 {
            fb = Framebuffer {
                base: mode.fb_base,
                size: mode.fb_size as u64,
                width: info.width,
                height: info.height,
                stride: info.stride,
                format: info.pixel_format,
            };
        }
    }
    fb
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

    let info_pages = core::mem::size_of::<BootInfo>().div_ceil(4096);
    let info_phys = alloc_pages(info_pages);
    let info = unsafe { &mut *(info_phys as *mut BootInfo) };
    info.magic = bootinfo::MAGIC;

    let kf = root.open("\\RS2\\KERNEL").expect("\\RS2\\KERNEL not found");
    let (kimg, klen) = read_file(kf);
    let kernel = load_elf(unsafe { core::slice::from_raw_parts(kimg as *const u8, klen as usize) });
    (bs().free_pages)(kimg, (klen as usize).div_ceil(4096));
    info.kernel_phys = kernel.phys;
    info.kernel_len = kernel.len;
    println!("  kernel {:#x} bytes at {:#x}, entry {:#x}", kernel.len, kernel.phys, kernel.entry);

    if let Some(f) = root.open("\\RS2\\CMDLINE") {
        let (base, len) = read_file(f);
        let n = (len as usize).min(255);
        let src = unsafe { core::slice::from_raw_parts(base as *const u8, n) };
        for (d, &s) in info.cmdline.iter_mut().zip(src) {
            *d = if s == b'\r' || s == b'\n' { b' ' } else { s };
        }
    }
    load_modules(&root, info);
    info.rsdp = find_rsdp();
    info.framebuffer = setup_framebuffer();

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

    println!("  exiting boot services");
    let (mut size, mut key, mut dsize, mut ver);
    loop {
        size = map_buf.len();
        key = 0;
        dsize = 0;
        ver = 0;
        check((bs().get_memory_map)(&mut size, map_buf.as_mut_ptr(), &mut key, &mut dsize, &mut ver), "GetMemoryMap");
        if (bs().exit_boot_services)(image, key) == SUCCESS {
            break;
        }
    }
    // No firmware calls from here on.
    let mut count = 0;
    for i in 0..size / dsize {
        let d = unsafe { &*(map_buf.as_ptr().add(i * dsize) as *const MemoryDescriptor) };
        let kind = match d.ty {
            memtype::LOADER_CODE
            | memtype::LOADER_DATA
            | memtype::BOOT_CODE
            | memtype::BOOT_DATA
            | memtype::CONVENTIONAL => MemoryKind::Usable,
            memtype::RS2_KEEP => MemoryKind::Loader,
            memtype::RUNTIME_CODE | memtype::RUNTIME_DATA => MemoryKind::Runtime,
            memtype::ACPI_RECLAIM => MemoryKind::AcpiReclaim,
            _ => MemoryKind::Reserved,
        };
        let region = MemoryRegion { start: d.phys_start, len: d.pages * 4096, kind, _pad: 0 };
        // Merge with the previous region when contiguous and the same kind.
        if count > 0 {
            let prev = &mut info.memory[count - 1];
            if prev.kind == kind && prev.start + prev.len == region.start {
                prev.len += region.len;
                continue;
            }
        }
        if count < info.memory.len() {
            info.memory[count] = region;
            count += 1;
        }
    }
    info.memory_count = count as u64;

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
