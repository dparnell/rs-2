//! STPM: Multimedia Setup, the notebook of rs2's multimedia devices (as
//! IBM's STPM shows MMPM/2's).
//!
//! The System page: the sound hardware, the master volume and mute (as
//! Volume Control keeps them), the recording source (microphone or line
//! in, Rs2AudioSetInput) and a test sound. Then a page for each device
//! MDM has (MCI_SYSINFO): its name, what it plays, the file types that
//! open on it and whether it is the default device of its type. rs2's
//! devices are built in, so nothing is installed or removed here.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use mmapps::*;

os2::entry!(main);

const CLASS: &[u8] = b"Rs2MultimediaSetup\0";
const PAGE_CLASS: &[u8] = b"Rs2MultimediaSetupPage\0";
const ID_BOOK: u32 = 100;
const ID_CLOSE: u32 = 101;
const ID_VOL: u32 = 200;
const ID_VOLTEXT: u32 = 201;
const ID_MUTE: u32 = 202;
const ID_MIC: u32 = 203;
const ID_LINE: u32 = 204;
const ID_TEST: u32 = 205;
const ID_HW: u32 = 206;
const ID_RESULT: u32 = 207;

const BKS_BACKPAGESBR: u32 = 0x0001;
const BKS_MAJORTABRIGHT: u32 = 0x0010;
const BKS_SPIRALBIND: u32 = 0x0400;
const BKS_ROUNDEDTABS: u32 = 0x0100;
const BKS_STATUSTEXTCENTER: u32 = 0x2000;
const BKS_TABTEXTCENTER: u32 = 0x8000;
const BKM_CALCPAGERECT: u32 = 0x0353;
const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_SETDIMENSIONS: u32 = 0x035e;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKA_LAST: u32 = 0x0002;
const BKA_MAJOR: u32 = 0x0040;
const BKA_STATUSTEXTON: u32 = 0x0001;
const BKA_AUTOPAGESIZE: u32 = 0x0100;
const BKA_MAJORTAB: u32 = 0x0001;

static mut SYSTEM_PAGE: u32 = 0;

fn page() -> u32 {
    unsafe { SYSTEM_PAGE }
}

/// What rs2's device of a type is (product, plays, file types).
fn describe(name: &str) -> (&'static str, &'static str, &'static str) {
    let n = name.to_ascii_lowercase();
    if n.starts_with("waveaudio") {
        ("rs2 Wave Audio", "Digital audio: plays and records sound files through the sound card (or silently in real time without one).", "WAV, AU, SND, VOC, AIF, IFF")
    } else if n.starts_with("sequencer") {
        ("rs2 MIDI Sequencer", "MIDI songs, played by rs2's General MIDI software synthesizer through the sound card.", "MID, RMI")
    } else if n.starts_with("cdaudio") {
        ("rs2 CD Audio", "Audio CDs, read digitally from the CD drive and played through the sound card.", "(audio CDs)")
    } else if n.starts_with("digitalvideo") {
        ("rs2 Digital Video", "Movies: AVI (uncompressed, RLE, Video 1, Ultimotion) with their sound, and FLI/FLC animations.", "AVI, FLC, FLI")
    } else {
        ("rs2 device", "", "")
    }
}

fn devices() -> Vec<String> {
    let (rc, names) = mci("sysinfo all name 1 wait");
    if rc != 0 {
        return Vec::new();
    }
    names.split_whitespace().map(String::from).collect()
}

fn show_volume() {
    let p = page();
    let (v, mute) = master_volume();
    unsafe { WinSendMsg(item(p, ID_VOL), SBM_SETPOS, v, 0) };
    set_text(item(p, ID_VOLTEXT), &alloc::format!("{v}%"));
    set_check(item(p, ID_MUTE), mute);
}

/// A second of a 440 Hz tone, as a WAV file in %MMBASE%, played and
/// deleted.
fn test_sound() {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    let n = 22050u32;
    data.extend_from_slice(&(36 + n).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    for v in [16u32] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    for v in [1u16, 1] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&22050u32.to_le_bytes());
    data.extend_from_slice(&22050u32.to_le_bytes());
    for v in [1u16, 8] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(b"data");
    data.extend_from_slice(&n.to_le_bytes());
    // A triangle wave at 441 Hz (50 samples a period), faded at the ends.
    for i in 0..n {
        let ph = (i % 50) as i32;
        let tri = if ph < 25 { ph * 8 - 100 } else { (50 - ph) * 8 - 100 };
        let env = (i.min(n - i).min(2000)) as i32;
        data.push((128 + tri * env / 2000) as u8);
    }
    let mut dir: *const u8 = core::ptr::null();
    let base = if unsafe { os2::DosScanEnv(b"MMBASE\0".as_ptr(), &mut dir) } == 0 {
        text_of(unsafe { core::slice::from_raw_parts(dir, 260) }).split(';').next().unwrap_or("").trim_end_matches('\\').into()
    } else {
        String::from("C:\\MMOS2")
    };
    let path = alloc::format!("{base}\\STPMTEST.WAV");
    let written = write_file(&path, &data);
    let msg = if !written {
        alloc::format!("Cannot write {path}.")
    } else {
        let (rc, _) = mci(&alloc::format!("play \"{path}\" wait"));
        let z = zs(&path);
        unsafe { os2::DosDelete(z.as_ptr()) };
        if rc == 0 { String::from("The test sound was played.") } else { alloc::format!("The test sound failed: {}", mci_error(rc)) }
    };
    set_text(item(page(), ID_RESULT), &msg);
}

extern "C" fn page_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_plain(h, CLR_PALEGRAY);
            0
        }
        WM_HSCROLL => {
            let (code, pos) = (mp2 >> 16, mp2 & 0xffff);
            let bar = item(h, ID_VOL);
            let cur = unsafe { WinSendMsg(bar, SBM_QUERYPOS, 0, 0) } as i32;
            let new = match code {
                SB_LINEUP => cur - 2,
                SB_LINEDOWN => cur + 2,
                SB_PAGEUP => cur - 10,
                SB_PAGEDOWN => cur + 10,
                SB_SLIDERTRACK | SB_SLIDERPOSITION => pos as i32,
                _ => cur,
            }
            .clamp(0, 100) as u32;
            set_master_volume(new, checked(item(h, ID_MUTE)));
            show_volume();
            0
        }
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            if code == BN_CLICKED {
                match id {
                    ID_MUTE => {
                        let (v, _) = master_volume();
                        set_master_volume(v, checked(item(h, ID_MUTE)));
                        show_volume();
                    }
                    ID_MIC | ID_LINE => {
                        let mut taken = 0;
                        unsafe { Rs2AudioSetInput(if id == ID_MIC { 2 } else { 1 }, &mut taken) };
                        ini_set("RecordSource", if id == ID_MIC { "MICROPHONE" } else { "LINE" });
                    }
                    _ => {}
                }
            }
            0
        }
        WM_COMMAND => {
            if mp1 & 0xffff == ID_TEST {
                test_sound();
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_plain(h, CLR_PALEGRAY);
            0
        }
        WM_COMMAND => {
            if mp1 & 0xffff == ID_CLOSE {
                unsafe { WinPostMsg(h, WM_QUIT, 0, 0) };
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn system_page(p: u32, ph: i32) {
    let mut n = 0u32;
    let hw = unsafe { Rs2AudioQuery(&mut n) } == 0;
    let top = ph - 34;
    label(p, if hw { "Sound hardware: a sound card is present." } else { "Sound hardware: none (sounds play silently, in real time)." }, ID_HW, 12, top, 400, 24, DT_LEFT);
    label(p, "Master volume:", 0xffff, 12, top - 40, 120, 24, DT_LEFT);
    let bar = scrollbar(p, false, ID_VOL, 136, top - 36, 200, 18);
    set_scroll(bar, 0, 100, 10);
    label(p, "", ID_VOLTEXT, 344, top - 40, 60, 24, DT_LEFT);
    control(p, WC_BUTTON, "~Mute", BS_AUTOCHECKBOX | WS_TABSTOP, ID_MUTE, 136, top - 70, 120, 24);
    label(p, "Record from:", 0xffff, 12, top - 110, 120, 24, DT_LEFT);
    let mic = control(p, WC_BUTTON, "M~icrophone", BS_AUTORADIOBUTTON | WS_TABSTOP | WS_GROUP, ID_MIC, 136, top - 110, 130, 24);
    let line_in = control(p, WC_BUTTON, "~Line in", BS_AUTORADIOBUTTON, ID_LINE, 270, top - 110, 120, 24);
    let line_chosen = ini_get("RecordSource").is_some_and(|s| s == "LINE");
    set_check(if line_chosen { line_in } else { mic }, true);
    button(p, "~Test sound", ID_TEST, 12, top - 160, 120, 30);
    label(p, "", ID_RESULT, 140, top - 158, 280, 26, DT_LEFT);
    show_volume();
}

fn device_page(p: u32, ph: i32, name: &str) {
    let (product, what, types) = describe(name);
    let top = ph - 34;
    let rows: [(&str, String); 4] = [
        ("Device:", alloc::format!("{name} ({product})")),
        ("Default:", String::from("Yes: the only device of its type")),
        ("File types:", String::from(types)),
        ("Status:", String::from("Installed (built into rs2's MDM.DLL)")),
    ];
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = top - i as i32 * 30;
        label(p, k, 0xffff, 12, y, 100, 24, DT_LEFT);
        label(p, v, 0xffff, 116, y, 310, 24, DT_LEFT);
    }
    control(p, WC_STATIC, what, SS_TEXT | DT_LEFT | DT_WORDBREAK, 0xffff, 12, top - 190, 410, 64);
}

fn main(_s: &os2::Startup) -> u32 {
    let Some(pm) = pm_start() else {
        os2::print("STPM needs Presentation Manager.\r\n");
        return 1;
    };
    unsafe { WinRegisterClass(pm.hab, PAGE_CLASS.as_ptr(), page_proc, WS_CLIPCHILDREN, 0) };
    let (cw, ch) = (600, 420);
    let (frame, client) = main_window(&pm, CLASS, client_proc, "Multimedia Setup", cw, ch, false);
    let nb_style = BKS_BACKPAGESBR | BKS_MAJORTABRIGHT | BKS_SPIRALBIND | BKS_ROUNDEDTABS | BKS_STATUSTEXTCENTER | BKS_TABTEXTCENTER | WS_TABSTOP;
    let nb = control(client, WC_NOTEBOOK, "", nb_style, ID_BOOK, 6, 44, cw - 12, ch - 50);
    unsafe { WinSendMsg(nb, BKM_SETDIMENSIONS, (110 | 26 << 16) as u32, BKA_MAJORTAB) };
    let mut pr = RectL { l: 0, b: 0, r: cw - 12, t: ch - 50 };
    unsafe { WinSendMsg(nb, BKM_CALCPAGERECT, &mut pr as *mut RectL as u32, 1) };
    let (pw, ph) = ((pr.r - pr.l).max(200), (pr.t - pr.b).max(200));
    let mut tabs: Vec<(String, String)> = alloc::vec![(String::from("System"), String::new())];
    for d in devices() {
        let tab = match d.to_ascii_lowercase() {
            n if n.starts_with("waveaudio") => "Wave Audio",
            n if n.starts_with("sequencer") => "MIDI",
            n if n.starts_with("cdaudio") => "CD Audio",
            n if n.starts_with("digitalvideo") => "Video",
            _ => "Device",
        };
        tabs.push((String::from(tab), d));
    }
    let count = tabs.len();
    for (i, (tab, dev)) in tabs.iter().enumerate() {
        let p = unsafe { WinCreateWindow(nb, PAGE_CLASS.as_ptr(), b"\0".as_ptr(), WS_CLIPCHILDREN, 0, 0, pw, ph, nb, HWND_TOP, 1000 + i as u32, core::ptr::null(), core::ptr::null()) };
        if i == 0 {
            unsafe { SYSTEM_PAGE = p };
            system_page(p, ph);
        } else {
            device_page(p, ph, dev);
        }
        let id = unsafe { WinSendMsg(nb, BKM_INSERTPAGE, 0, (BKA_MAJOR | BKA_STATUSTEXTON | BKA_AUTOPAGESIZE) | BKA_LAST << 16) };
        let (tt, st) = (zs(tab), zs(&alloc::format!("Page {} of {}", i + 1, count)));
        unsafe {
            WinSendMsg(nb, BKM_SETPAGEWINDOWHWND, id, p);
            WinSendMsg(nb, BKM_SETTABTEXT, id, tt.as_ptr() as u32);
            WinSendMsg(nb, BKM_SETSTATUSLINETEXT, id, st.as_ptr() as u32);
        }
    }
    button(client, "Close", ID_CLOSE, 8, 6, 96, 30);
    show(frame);
    pm.run();
    unsafe { WinDestroyWindow(frame) };
    0
}
