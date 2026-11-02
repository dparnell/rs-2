//! CDPM: the CD Player. Plays audio CDs through MDM's cdaudio device
//! (`CDPM [device]`, Cdaudio01, the first CD drive, by default): the tracks in a
//! list (double-click one to play it), Play, Pause, Stop, previous and
//! next track, Eject; the track and the time in it, the disc's length.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use mmapps::*;

os2::entry!(main);

const CLASS: &[u8] = b"Rs2CdPlayer\0";
const ID_PLAY: u32 = 10;
const ID_PAUSE: u32 = 11;
const ID_STOP: u32 = 12;
const ID_PREV: u32 = 13;
const ID_NEXT: u32 = 14;
const ID_EJECT: u32 = 15;
const ID_TRACKS: u32 = 20;
const ID_DISPLAY: u32 = 21;
const ID_INFO: u32 = 22;
const LN_ENTER: u32 = 5;

struct App {
    client: u32,
    open: bool,
    tracks: u32,
    shown_tracks: u32,
    device: String,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

/// (track, minutes, seconds) of a TMSF answer "tt:mm:ss:ff".
fn tmsf(s: &str) -> (u32, u32, u32) {
    let v: alloc::vec::Vec<u32> = s.trim().split(':').map(|p| p.parse().unwrap_or(0)).collect();
    (v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0), v.get(2).copied().unwrap_or(0))
}

fn msf(s: &str) -> String {
    let v: alloc::vec::Vec<&str> = s.trim().split(':').collect();
    if v.len() >= 2 { alloc::format!("{}:{}", v[0].trim_start_matches('0').parse::<u32>().unwrap_or(0), v[1]) } else { String::from(s) }
}

fn refresh() {
    let a = app();
    let c = a.client;
    if !a.open {
        let (rc, _) = mci(&alloc::format!("open {} alias cd shareable wait", a.device));
        if rc != 0 {
            set_text(item(c, ID_DISPLAY), "--  --:--");
            set_text(item(c, ID_INFO), &alloc::format!("No CD drive ({})", mci_error(rc)));
            return;
        }
        a.open = true;
        mci("set cd time format tmsf wait");
    }
    let present = mci("status cd media present wait").1 == "TRUE";
    let tracks = if present { mci_num("status cd number of tracks wait") } else { 0 };
    a.tracks = tracks;
    let list = item(c, ID_TRACKS);
    if tracks != a.shown_tracks {
        a.shown_tracks = tracks;
        unsafe { WinSendMsg(list, LM_DELETEALL, 0, 0) };
        for t in 1..=tracks {
            let len = msf(&mci(&alloc::format!("status cd length track {t} wait")).1);
            let ty = mci(&alloc::format!("status cd type track {t} wait")).1;
            let line = zs(&alloc::format!("Track {t:2}   {len}{}", if ty == "audio" || ty.is_empty() { "" } else { "  (data)" }));
            unsafe { WinSendMsg(list, LM_INSERTITEM, LIT_END, line.as_ptr() as u32) };
        }
    }
    if !present {
        set_text(item(c, ID_DISPLAY), "--  --:--");
        set_text(item(c, ID_INFO), "No disc in the drive.");
        return;
    }
    let (track, m, s) = tmsf(&mci("status cd position wait").1);
    let mode = mci("status cd mode wait").1;
    set_text(item(c, ID_DISPLAY), &alloc::format!("{track:02}  {m:02}:{s:02}"));
    let total = msf(&mci("status cd length wait").1);
    set_text(item(c, ID_INFO), &alloc::format!("{tracks} tracks, {total} - {mode}"));
}

fn play_track(t: u32) {
    if t == 0 || t > app().tracks {
        return;
    }
    mci(&alloc::format!("play cd from {t}"));
}

fn command(id: u32) {
    let a = app();
    if !a.open && id != ID_EJECT {
        refresh();
    }
    let (track, _, _) = tmsf(&mci("status cd position wait").1);
    match id {
        ID_PLAY => {
            if mci("status cd mode wait").1 == "paused" {
                mci("resume cd wait");
            } else {
                mci("play cd");
            }
        }
        ID_PAUSE => {
            mci("pause cd wait");
        }
        ID_STOP => {
            mci("stop cd wait");
            mci("seek cd to start wait");
        }
        ID_PREV => {
            let playing = mci("status cd mode wait").1 == "playing";
            let t = if track > 1 { track - 1 } else { 1 };
            if playing { play_track(t) } else { mci(&alloc::format!("seek cd to {t} wait")); }
        }
        ID_NEXT => {
            let playing = mci("status cd mode wait").1 == "playing";
            let t = (track + 1).min(a.tracks.max(1));
            if playing { play_track(t) } else { mci(&alloc::format!("seek cd to {t} wait")); }
        }
        ID_EJECT => {
            mci("set cd door open wait");
        }
        _ => {}
    }
    refresh();
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            paint_plain(h, CLR_PALEGRAY);
            0
        }
        WM_COMMAND => {
            command(mp1 & 0xffff);
            0
        }
        WM_CONTROL => {
            if mp1 & 0xffff == ID_TRACKS && mp1 >> 16 == LN_ENTER {
                let sel = unsafe { WinSendMsg(item(h, ID_TRACKS), LM_QUERYSELECTION, LIT_FIRST, 0) };
                if sel != 0xffff && sel != LIT_FIRST {
                    play_track(sel + 1);
                    refresh();
                }
            }
            0
        }
        WM_TIMER => {
            refresh();
            0
        }
        WM_CLOSE => {
            unsafe { WinPostMsg(h, WM_QUIT, 0, 0) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    // The device (Cdaudio01 for the first drive, Cdaudio02...).
    // Without one, the first drive with a disc in it.
    let device = a.first().cloned().unwrap_or_else(|| {
        for n in 1..=4 {
            if mci(&alloc::format!("open cdaudio{n:02} alias cdprobe shareable wait")).0 != 0 {
                break;
            }
            let present = mci("status cdprobe media present wait").1 == "TRUE";
            mci("close cdprobe wait");
            if present {
                return alloc::format!("cdaudio{n:02}");
            }
        }
        String::from("cdaudio")
    });
    let Some(pm) = pm_start() else {
        os2::print("CDPM needs Presentation Manager.\r\n");
        return 1;
    };
    let (frame, c) = main_window(&pm, CLASS, client_proc, "CD Player", 400, 250, false);
    unsafe { *core::ptr::addr_of_mut!(APP) = Some(App { client: c, open: false, tracks: 0, shown_tracks: u32::MAX, device }) };
    let disp = label(c, "--  --:--", ID_DISPLAY, 10, 196, 180, 44, DT_CENTER);
    let font = b"24.Helv\0";
    // PP_FONTNAMESIZE; the display in green on black.
    unsafe {
        WinSetPresParam(disp, 15, font.len() as u32, font.as_ptr());
        WinSetPresParam(disp, 1, 4, (&0x0000ff00u32) as *const u32 as *const u8);
        WinSetPresParam(disp, 3, 4, (&0u32) as *const u32 as *const u8);
    }
    let labels = [("|<", ID_PREV), ("Play", ID_PLAY), ("Pause", ID_PAUSE), ("Stop", ID_STOP), (">|", ID_NEXT), ("Eject", ID_EJECT)];
    for (i, (t, id)) in labels.iter().enumerate() {
        button(c, t, *id, 10 + (i as i32 % 3) * 60, 120 - (i as i32 / 3) * 36, 56, 30);
    }
    control(c, WC_LISTBOX, "", WS_TABSTOP, ID_TRACKS, 200, 50, 190, 190);
    label(c, "", ID_INFO, 10, 8, 380, 30, DT_LEFT);
    show(frame);
    refresh();
    unsafe { WinStartTimer(pm.hab, c, 1, 500) };
    pm.run();
    if app().open {
        mci("close cd wait");
    }
    unsafe { WinDestroyWindow(frame) };
    0
}

unsafe extern "C" {
    #[link_name = "PMWIN.938"]
    fn WinSetPresParam(h: u32, id: u32, cb: u32, data: *const u8) -> u32;
}
