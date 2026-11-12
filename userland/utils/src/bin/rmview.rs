//! RMVIEW: show the resource manager's view of the hardware.
//! RMVIEW [/P | /D | /L | /HW] [/IRQ] [/IO] [/IOA] [/DMA] [/MEM]
//!
//! Reads the kernel's `RESMGR$` (IOCtl category 0x80: 2 enumerates a
//! view's nodes with their depths, 1 gives a node's data; IBM's rmbase.h
//! structures, kernel/src/resmgr.rs) and prints, as IBM's RMVIEW does: the
//! physical view (adapters, the devices on them, their resources), the
//! driver view (each driver and its adapters), the logical view, the
//! hardware tree (names only) or the interrupts, I/O ports (those from
//! 100h up, or with /IOA all), DMA channels or memory ranges in use, each
//! with its owner.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use utils::*;
os2::entry!(main);

struct Node {
    depth: u32,
    ty: u32,
    desc: String,
    /// Adapter: base type, subtype, bus, width; device: type, flags;
    /// logical device: class; driver: type, subtype.
    w: [u16; 4],
    driver: String,
    vendor: String,
    version: (u8, u8),
    res: Vec<Res>,
}

#[derive(Clone, Copy)]
enum Res {
    Io(u16, u16, u16, u16),
    Irq(u16, u16, u16),
    Mem(u32, u32, u16),
    Dma(u16, u16),
}

fn rm_open() -> Option<u32> {
    let (mut h, mut act) = (0u32, 0u32);
    (unsafe { os2::DosOpen(b"RESMGR$\0".as_ptr(), &mut h, &mut act, 0, 0, 1, 0x42, core::ptr::null_mut()) } == 0).then_some(h)
}

fn enum_nodes(h: u32, cmd: u16) -> Vec<(u32, u32)> {
    let mut n = 64usize;
    loop {
        let mut p = cmd.to_le_bytes();
        let mut d = alloc::vec![0u8; 4 + n * 8];
        let (mut pl, mut dl) = (2u32, d.len() as u32);
        let rc = unsafe { os2::DosDevIOCtl(h, 0x80, 2, p.as_mut_ptr(), 2, &mut pl, d.as_mut_ptr(), d.len() as u32, &mut dl) };
        let count = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
        if rc != 0 && count > n && n < 8192 {
            n = count;
            continue;
        }
        let rd = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        return (0..count.min(n)).map(|i| (rd(4 + i * 8), rd(8 + i * 8))).collect();
    }
}

fn node(h: u32, handle: u32, depth: u32) -> Option<Node> {
    let mut size = 0x400u32;
    let mut b;
    loop {
        b = alloc::vec![0u8; size as usize];
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&handle.to_le_bytes());
        p[4..8].copy_from_slice(&(b.as_ptr() as u32).to_le_bytes());
        let (mut pl, mut dl) = (8u32, size);
        let rc = unsafe { os2::DosDevIOCtl(h, 0x80, 1, p.as_mut_ptr(), 8, &mut pl, b.as_mut_ptr(), size, &mut dl) };
        let need = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if rc == 0 {
            break;
        }
        if need <= size || need > 0x10000 {
            return None;
        }
        size = need;
    }
    let base = b.as_ptr() as u32;
    let u8_ = |o: usize| b.get(o).copied().unwrap_or(0);
    let u16_ = |o: usize| u16::from_le_bytes([u8_(o), u8_(o + 1)]);
    let u32_ = |o: usize| u32::from_le_bytes([u8_(o), u8_(o + 1), u8_(o + 2), u8_(o + 3)]);
    let off = |p: u32| p.checked_sub(base).map(|o| o as usize).filter(|&o| o < b.len());
    let text = |p: u32| -> String {
        let Some(o) = off(p) else { return String::new() };
        b[o..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim_end().into()
    };
    let ty = u32_(8);
    let s = off(u32_(16))?;
    let mut n = Node { depth, ty, desc: text(u32_(s)), w: [0; 4], driver: String::new(), vendor: String::new(), version: (0, 0), res: Vec::new() };
    match ty {
        0 => n.w = [u16_(s + 6), u16_(s + 8), u16_(s + 12), u16_(s + 14)],
        1 => n.w = [u16_(s + 6), u16_(s + 4), 0, 0],
        2 => n.w = [u16_(s + 6), 0, 0, 0],
        4 => {
            n.driver = text(u32_(s));
            n.desc = text(u32_(s + 4));
            n.vendor = text(u32_(s + 8));
            n.version = (u8_(s + 12), u8_(s + 13));
            n.w = [u16_(s + 20), u16_(s + 22), 0, 0];
        }
        _ => {}
    }
    if let Some(r) = off(u32_(20)) {
        for k in 0..u32_(r).min(256) as usize {
            let e = r + 4 + k * 20;
            let w = |x: usize| u16_(e + 4 + 2 * x);
            n.res.push(match u32_(e) {
                1 => Res::Io(w(0), w(1), w(2), w(3)),
                2 => Res::Irq(w(0), w(1), w(2)),
                3 => Res::Mem(u32_(e + 4), u32_(e + 8), w(4)),
                4 => Res::Dma(w(0), w(1)),
                _ => continue,
            });
        }
    }
    Some(n)
}

fn view(cmd: u16) -> Vec<Node> {
    let Some(h) = rm_open() else {
        outln("RMVIEW: the resource manager (RESMGR$) is not available.");
        return Vec::new();
    };
    let v = enum_nodes(h, cmd).into_iter().filter_map(|(handle, depth)| node(h, handle, depth)).collect();
    unsafe { os2::DosClose(h) };
    v
}

fn adapter_kind(w: &[u16; 4]) -> &'static str {
    match (w[0], w[1]) {
        (1, 1) => "SCSI controller",
        (1, 2) => "disk controller",
        (1, 3) => "diskette controller",
        (1, _) => "storage controller",
        (2, _) => "network adapter",
        (3, _) => "display adapter",
        (4, 1) => "video device",
        (4, 2) => "audio device",
        (4, _) => "multimedia device",
        (5, _) => "memory",
        (6, _) => "bridge",
        (7, 1) => "serial port",
        (7, 2) => "parallel port",
        (7, _) => "communication device",
        (8, 1) => "interrupt controller",
        (8, 2) => "DMA controller",
        (8, 3) => "timer",
        (8, 4) => "real-time clock",
        (8, _) => "system peripheral",
        (9, 1) => "keyboard",
        (9, 3) => "pointing device",
        (9, _) => "input device",
        _ => "other",
    }
}

fn bus(w: &[u16; 4]) -> String {
    let b = match w[2] {
        1 => "ISA",
        2 => "EISA",
        3 => "Micro Channel",
        4 => "PCI",
        5 => "SCSI",
        6 => "PCMCIA",
        7 => "parallel",
        8 => "system board",
        _ => "other",
    };
    let bits = match w[3] {
        0x10 => " 8-bit",
        0x20 => " 16-bit",
        0x30 => " 32-bit",
        0x40 => " 64-bit",
        _ => "",
    };
    format!("{b}{bits}")
}

fn device_kind(t: u16) -> &'static str {
    match t {
        0 => "disk",
        1 => "tape",
        2 => "printer",
        3 => "processor",
        4 => "WORM",
        5 => "CD-ROM",
        6 => "scanner",
        7 => "optical memory",
        8 => "changer",
        9 => "communication",
        0x0a => "ATAPI",
        0x0b => "SCSI",
        0x0c => "socket",
        0x0d => "slot",
        0x0e => "chipset",
        0x0f => "input/output",
        0x10 => "audio",
        _ => "other",
    }
}

fn flags(f: u16) -> &'static str {
    if f & 4 != 0 {
        "shared"
    } else if f & 1 != 0 {
        "exclusive"
    } else {
        "-"
    }
}

fn res_text(r: &Res) -> String {
    match *r {
        Res::Io(base, len, f, lines) => format!("I/O    {base:04X}-{:04X}  {} ({lines} address lines)", (base as u32 + len.max(1) as u32 - 1) & 0xffff, flags(f)),
        Res::Irq(level, pin, f) => {
            let pin = if (1..=8).contains(&pin) { format!(", PCI pin {}", (b'A' + pin as u8 - 1) as char) } else { String::new() };
            let t = format!("{level}{pin}");
            format!("IRQ    {t:<4} {}", flags(f))
        }
        Res::Mem(base, size, f) => format!("Memory {base:08X}-{:08X}  {}", base.wrapping_add(size.max(1) - 1), flags(f)),
        Res::Dma(ch, f) => format!("DMA    {ch}  {}", flags(f)),
    }
}

fn indent(d: u32) -> String {
    " ".repeat(2 * d as usize)
}

fn show(n: &Node) {
    let i = indent(n.depth);
    match n.ty {
        0 => {
            outln(&format!("{i}{}", n.desc));
            outln(&format!("{i}  adapter: {}, {} bus", adapter_kind(&n.w), bus(&n.w)));
        }
        1 => {
            let removable = if n.w[1] & 1 != 0 { ", removable" } else { "" };
            outln(&format!("{i}{}", n.desc));
            outln(&format!("{i}  device: {}{removable}", device_kind(n.w[0])));
        }
        2 => {
            let class = match n.w[0] {
                2 => "disk",
                3 => "CD-ROM",
                4 => "serial",
                5 => "parallel",
                6 => "tape",
                _ => "other",
            };
            outln(&format!("{i}{}  ({class})", n.desc));
        }
        4 => {
            let kind = match (n.w[0], n.w[1]) {
                (3, 1) => "character device driver",
                (3, 2) => "block device driver",
                (4, _) => "network driver",
                (5, _) => "video driver",
                (6, _) => "audio driver",
                (7, _) => "service",
                _ => "driver",
            };
            outln(&format!("{i}Driver {}: {}", n.driver, n.desc));
            outln(&format!("{i}  {kind}, {} version {}.{}", n.vendor, n.version.0, n.version.1));
        }
        _ => {}
    }
    for r in &n.res {
        outln(&format!("{i}  {}", res_text(r)));
    }
    outln("");
}

fn usage() {
    outln("Shows the hardware as the resource manager knows it.\r\n");
    outln("RMVIEW [/P | /D | /L | /HW] [/IRQ] [/IO] [/IOA] [/DMA] [/MEM]\r\n");
    outln(" /P    - adapters and devices with their resources (the default)");
    outln(" /D    - each driver and the adapters it runs");
    outln(" /L    - logical devices");
    outln(" /HW   - the hardware tree, names only");
    outln(" /IRQ  - interrupt levels in use");
    outln(" /IO   - I/O ports in use from 100h up (/IOA: all of them)");
    outln(" /DMA  - DMA channels in use");
    outln(" /MEM  - memory ranges in use\r\n");
}

fn main(st: &os2::Startup) -> u32 {
    let mut cmd = 0u16;
    let mut tree = false;
    // Resource lists: IRQ, I/O, DMA, memory; all I/O ports.
    let mut lists = [false; 4];
    let mut io_all = false;
    for x in args(st) {
        match x.to_ascii_uppercase().trim_start_matches(['/', '-']) {
            "P" => cmd = 0,
            "D" => cmd = 2,
            "L" => cmd = 1,
            "HW" => tree = true,
            "IRQ" => lists[0] = true,
            "IO" => lists[1] = true,
            "IOA" => {
                lists[1] = true;
                io_all = true;
            }
            "DMA" => lists[2] = true,
            "MEM" => lists[3] = true,
            _ => {
                usage();
                return 1;
            }
        }
    }
    if lists.iter().any(|&l| l) {
        let nodes = view(0);
        for (k, title) in ["Interrupt levels", "I/O ports", "DMA channels", "Memory ranges"].iter().enumerate() {
            if !lists[k] {
                continue;
            }
            outln(&format!("{title} in use:"));
            let mut rows: Vec<(u32, String)> = Vec::new();
            for n in &nodes {
                for r in &n.res {
                    let key = match (*r, k) {
                        (Res::Irq(l, ..), 0) => l as u32,
                        (Res::Io(b, ..), 1) if io_all || b >= 0x100 => b as u32,
                        (Res::Dma(c, _), 2) => c as u32,
                        (Res::Mem(b, ..), 3) => b,
                        _ => continue,
                    };
                    rows.push((key, format!("  {:<50} {}", res_text(r), n.desc)));
                }
            }
            rows.sort_by_key(|r| r.0);
            for (_, r) in rows {
                outln(&r);
            }
            outln("");
        }
        return 0;
    }
    let nodes = view(cmd);
    if tree {
        for n in nodes.iter().filter(|n| n.ty <= 1) {
            outln(&format!("{}{}", indent(n.depth), n.desc));
        }
        return 0;
    }
    outln(match cmd {
        2 => "Drivers and the adapters they run:\r\n",
        1 => "Logical devices:\r\n",
        _ => "Adapters and devices:\r\n",
    });
    for n in &nodes {
        show(n);
    }
    0
}
