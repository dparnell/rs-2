//! MMVOLUME: Volume Control. A vertical volume bar and a Mute check box
//! for the master volume of every sound rs2 plays (the kernel mixer's
//! volume, RS2API Rs2AudioSetVolume with stream 0). The setting is kept
//! in OS2.INI (`MMPM2` `MasterVolume`, `Mute`), where MDM takes it when a
//! program first plays a sound after a restart.
//!
//! `MMVOLUME n` sets the volume to n (0-100) without the window,
//! `MMVOLUME MUTE` / `UNMUTE` mute and unmute, `MMVOLUME QUERY` prints it.

#![no_std]
#![no_main]

extern crate alloc;

use mmapps::*;

os2::entry!(main);

const CLASS: &[u8] = b"Rs2VolumeControl\0";
const ID_BAR: u32 = 10;
const ID_VALUE: u32 = 11;
const ID_MUTE: u32 = 12;
const ID_HW: u32 = 13;

static mut CLIENT: u32 = 0;

fn client() -> u32 {
    unsafe { CLIENT }
}

/// The bar runs 0 (top: loudest) to 100.
fn apply(from_bar: Option<u32>) {
    let h = client();
    let (mut v, _) = master_volume();
    if let Some(p) = from_bar {
        v = 100 - p.min(100);
    }
    let mute = checked(item(h, ID_MUTE));
    set_master_volume(v, mute);
    unsafe { WinSendMsg(item(h, ID_BAR), SBM_SETPOS, 100 - v, 0) };
    set_text(item(h, ID_VALUE), &if mute { alloc::format!("{v}% (muted)") } else { alloc::format!("{v}%") });
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            fill(hps, &r, CLR_PALEGRAY);
            // A loudspeaker beside the bar.
            let (x, y) = (16, 150);
            rect(hps, x, y, x + 10, y + 16, CLR_DARKGRAY, true);
            for i in 0..10 {
                line(hps, x + 10 + i, y - i, x + 10 + i, y + 16 + i, CLR_DARKGRAY);
            }
            for k in 0..3 {
                let rx = x + 26 + k * 6;
                line(hps, rx, y + 2 - k * 3, rx + 3, y + 8, CLR_BLACK);
                line(hps, rx + 3, y + 8, rx, y + 14 + k * 3, CLR_BLACK);
            }
            unsafe { WinEndPaint(hps) };
            0
        }
        WM_VSCROLL => {
            let (code, pos) = (mp2 >> 16, mp2 & 0xffff);
            let bar = item(h, ID_BAR);
            let cur = unsafe { WinSendMsg(bar, SBM_QUERYPOS, 0, 0) } as i32;
            let new = match code {
                SB_LINEUP => cur - 2,
                SB_LINEDOWN => cur + 2,
                SB_PAGEUP => cur - 10,
                SB_PAGEDOWN => cur + 10,
                SB_SLIDERTRACK | SB_SLIDERPOSITION => pos as i32,
                _ => cur,
            };
            apply(Some(new.clamp(0, 100) as u32));
            0
        }
        WM_CONTROL => {
            if mp1 & 0xffff == ID_MUTE && mp1 >> 16 == BN_CLICKED {
                apply(None);
            }
            0
        }
        WM_COMMAND => 0,
        WM_CLOSE => {
            unsafe { WinPostMsg(h, WM_QUIT, 0, 0) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn command_line(a: &[alloc::string::String]) -> u32 {
    let (v, mute) = master_volume();
    let w = a[0].to_ascii_uppercase();
    match w.as_str() {
        "QUERY" | "/QUERY" => {
            os2::print(&alloc::format!("Volume {v}%{}\r\n", if mute { ", muted" } else { "" }));
            0
        }
        "MUTE" => {
            set_master_volume(v, true);
            0
        }
        "UNMUTE" => {
            set_master_volume(v, false);
            0
        }
        n => match n.trim_end_matches('%').parse::<u32>() {
            Ok(n) if n <= 100 => {
                set_master_volume(n, mute);
                0
            }
            _ => {
                os2::print("usage: MMVOLUME [0-100 | MUTE | UNMUTE | QUERY]\r\n");
                1
            }
        },
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if !a.is_empty() {
        return command_line(&a);
    }
    let Some(pm) = pm_start() else {
        os2::print("MMVOLUME needs Presentation Manager (or MMVOLUME n).\r\n");
        return 1;
    };
    let (frame, c) = main_window(&pm, CLASS, client_proc, "Volume Control", 200, 290, false);
    unsafe { CLIENT = c };
    let bar = scrollbar(c, true, ID_BAR, 84, 70, 24, 200);
    set_scroll(bar, 0, 100, 10);
    label(c, "", ID_VALUE, 120, 160, 76, 24, DT_LEFT);
    control(c, WC_BUTTON, "~Mute", BS_AUTOCHECKBOX | WS_TABSTOP, ID_MUTE, 16, 36, 160, 24);
    let mut n = 0u32;
    let hw = unsafe { Rs2AudioQuery(&mut n) } == 0;
    label(c, if hw { "Sound hardware: present" } else { "No sound hardware" }, ID_HW, 10, 6, 186, 24, DT_LEFT);
    let (_, mute) = master_volume();
    set_check(item(c, ID_MUTE), mute);
    apply(None);
    show(frame);
    pm.run();
    unsafe { WinDestroyWindow(frame) };
    0
}
