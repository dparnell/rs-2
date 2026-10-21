//! MDM.DLL: the media device manager of OS/2 multimedia (MMPM/2), the
//! media control interface (MCI) applications use: mciSendCommand with
//! message numbers and parameter blocks, mciSendString with string
//! commands ("play movie wait"), mciGetErrorString, mciGetDeviceID.
//!
//! rs2 has two devices: wave audio (WAV files and memory playlists,
//! played through the kernel's sound driver; silent in real time without
//! one) and digital video (FLI/FLC animations, drawn into the
//! application's window or MDM's own). Devices belong to the process that
//! opens them. IBM's MDM loads device drivers (MCDs) listed in MMPM2.INI;
//! rs2's are built in.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod dev;
mod exports;
mod req;
mod sys;
mod video;
mod wave;

use alloc::string::String;
use alloc::vec::Vec;

use dev::*;
use mci::*;
use req::*;
use sys::*;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, flag: u32) -> u32 {
    // The process is ending: its devices go.
    if flag == 1 {
        dev::close_all();
    }
    1
}

// ---- mciSendCommand ----

/// mciSendCommand(usDeviceID, usMessage, ulParam1, pParam2, usUserParm).
#[unsafe(no_mangle)]
pub extern "C" fn mciSendCommand(dev: u32, msg: u32, flags: u32, p: u32, user: u32) -> u32 {
    let (dev, msg, user) = (dev as u16, msg & 0xffff, user as u16);
    let r = send_command(dev, msg, flags, p, user);
    r.unwrap_or_else(|e| e)
}

fn base_req(msg: u32, flags: u32, p: u32, user: u16) -> Req {
    Req { msg, flags, callback: if p != 0 { rd(p, 0) } else { 0 }, user, ..Default::default() }
}

fn send_command(id: u16, msg: u32, flags: u32, p: u32, user: u16) -> Result<u32, u32> {
    if flags & MCI_WAIT != 0 && flags & MCI_NOTIFY != 0 {
        return Err(MCIERR_FLAGS_NOT_COMPATIBLE);
    }
    if p == 0 && (flags & MCI_NOTIFY != 0 || matches!(msg, MCI_OPEN | MCI_STATUS | MCI_SYSINFO | MCI_GETDEVCAPS | MCI_INFO | MCI_SET | MCI_LOAD | MCI_PUT | MCI_WHERE | MCI_WINDOW)) {
        return Err(MCIERR_MISSING_PARAMETER);
    }
    let mut r = base_req(msg, flags, p, user);
    match msg {
        MCI_OPEN => return open_command(&mut r, p),
        MCI_SYSINFO => {
            r.value = rd(p, 12);
            r.item = rd(p, 20);
            let t = rd16(p, 16);
            let reply = dev::sysinfo(&r, (t != 0).then_some(t))?;
            write_text(&reply.text(), rd(p, 4), rd(p, 8))?;
            return Ok(0);
        }
        MCI_MASTERAUDIO => return Ok(0),
        _ => {}
    }
    match msg {
        MCI_PLAY => {
            r.from = rd(p, 4);
            r.to = rd(p, 8);
        }
        MCI_SEEK => r.to = rd(p, 4),
        MCI_STATUS => r.item = rd(p, 8),
        MCI_GETDEVCAPS => r.item = if flags & MCI_GETDEVCAPS_MESSAGE != 0 { rd16(p, 12) as u32 } else { rd(p, 8) },
        MCI_SET => {
            r.item = rd(p, 4);
            r.value = rd(p, 16);
            r.wave = [rd16(p, 40) as u32, rd16(p, 44) as u32, rd(p, 48), rd(p, 52), rd16(p, 56) as u32, rd16(p, 60) as u32];
        }
        MCI_LOAD => r.element = Some(cstr(rd(p, 4) as *const u8)),
        MCI_WINDOW => {
            r.hwnd = rd(p, 4);
            r.value = rd16(p, 8) as u32;
            r.text = (flags & MCI_VID_WINDOW_TEXT != 0).then(|| cstr(rd(p, 12) as *const u8));
        }
        MCI_PUT => r.rect = [rd(p, 4) as i32, rd(p, 8) as i32, rd(p, 12) as i32, rd(p, 16) as i32],
        _ => {}
    }
    let reply = dev::command(id, &r)?;
    match msg {
        MCI_STATUS | MCI_GETDEVCAPS => wr(p, 4, reply.num()),
        MCI_INFO => write_text(&reply.text(), rd(p, 4), rd(p, 8))?,
        MCI_WHERE => {
            if let Reply::Rect(rc) = reply {
                for (i, v) in rc.iter().enumerate() {
                    wr(p, 4 + i as u32 * 4, *v as u32);
                }
            }
        }
        _ => {}
    }
    Ok(0)
}

fn write_text(t: &str, buf: u32, cb: u32) -> Result<(), u32> {
    if buf == 0 || cb == 0 {
        return Err(MCIERR_INVALID_BUFFER);
    }
    if !copy_out(t, buf as *mut u8, cb) {
        return Err(MCIERR_PARAM_OVERFLOW);
    }
    Ok(())
}

/// MCI_OPEN: MCI_OPEN_PARMS (hwndCallback, usDeviceID, pszDeviceType,
/// pszElementName, pszAlias, hwndParent for video).
fn open_command(r: &mut Req, p: u32) -> Result<u32, u32> {
    let flags = r.flags;
    let dtype = rd(p, 8);
    let elem = rd(p, 12);
    if flags & MCI_OPEN_MMIO != 0 {
        return Err(MCIERR_UNSUPPORTED_FLAG);
    }
    let playlist = (flags & MCI_OPEN_PLAYLIST != 0).then_some(elem);
    if flags & MCI_OPEN_ELEMENT != 0 && elem != 0 {
        r.element = Some(cstr(elem as *const u8));
    }
    if flags & MCI_OPEN_ALIAS != 0 {
        r.text = Some(cstr(rd(p, 16) as *const u8));
    }
    if flags & MCI_DGV_OPEN_PARENT != 0 {
        r.hwnd = rd(p, 20);
    }
    let (typ, ord) = if flags & MCI_OPEN_TYPE_ID != 0 {
        (dtype as u16, (dtype >> 16) as u16)
    } else if dtype != 0 {
        let s = cstr(dtype as *const u8);
        parse_device_name(&s).ok_or(MCIERR_INVALID_DEVICE_NAME)?
    } else if let Some(e) = &r.element {
        (devtype_for_file(e).ok_or(MCIERR_EXTENSION_NOT_FOUND)?, 0)
    } else {
        return Err(MCIERR_MISSING_PARAMETER);
    };
    let id = dev::open(typ, ord, r, playlist)?;
    wr16(p, 4, id);
    if flags & MCI_NOTIFY != 0 {
        post(Notify { hwnd: r.callback, user: r.user, msg: MCI_OPEN as u16 }, MCI_NOTIFY_SUCCESSFUL, id);
    }
    Ok(0)
}

// ---- mciSendString ----

/// mciSendString(pszCommandBuf, pszReturnString, usReturnLength,
/// hwndCallBack, usUserParm).
#[unsafe(no_mangle)]
pub extern "C" fn mciSendString(cmd: *const u8, ret: *mut u8, cb: u32, hwnd: u32, user: u32) -> u32 {
    let cb = cb & 0xffff;
    if !ret.is_null() && cb > 0 {
        unsafe { *ret = 0 };
    }
    match send_string(&cstr(cmd), hwnd, user as u16) {
        Ok(t) => {
            if !ret.is_null() && cb > 0 && !copy_out(&t, ret, cb) {
                return MCIERR_PARAM_OVERFLOW;
            }
            0
        }
        Err(e) => e,
    }
}

fn send_string(s: &str, hwnd: u32, user: u16) -> Result<String, u32> {
    let c = mci::parse(s)?;
    let mut a = c.args.clone();
    let mut r = Req { flags: c.flags, callback: hwnd, user, from_string: true, ..Default::default() };
    if c.flags & MCI_NOTIFY != 0 && hwnd == 0 {
        return Err(MCIERR_INVALID_CALLBACK_HANDLE);
    }
    match c.verb.as_str() {
        "open" => return string_open(&c.object, &mut a, &mut r).map(|id| alloc::format!("{id}")),
        "sysinfo" => {
            if a.flag("quantity") {
                r.flags |= MCI_SYSINFO_QUANTITY;
            } else if let Some(n) = a.number("name")? {
                r.flags |= MCI_SYSINFO_NAME;
                r.value = n;
            } else if a.flag("installname") {
                r.flags |= MCI_SYSINFO_INSTALLNAME;
            } else {
                return Err(MCIERR_MISSING_PARAMETER);
            }
            if a.flag("open") {
                r.flags |= MCI_SYSINFO_OPEN;
            }
            a.done()?;
            let typ = if c.object.eq_ignore_ascii_case("all") { None } else { Some(parse_device_name(&c.object).ok_or(MCIERR_INVALID_DEVICE_NAME)?.0) };
            return Ok(dev::sysinfo(&r, typ)?.text());
        }
        "masteraudio" | "acquire" | "release" if c.verb == "masteraudio" => return Ok(String::new()),
        _ => {}
    }
    // The object: an alias or open device, else a file or a device type
    // opened for this command only.
    let (id, temp) = match dev::find(&c.object) {
        Some(id) => (id, false),
        None => {
            let mut o = Req { callback: hwnd, user, ..Default::default() };
            let typ = match devtype_for_file(&c.object) {
                Some(t) => {
                    o.element = Some(c.object.clone());
                    t
                }
                None => parse_device_name(&c.object).ok_or(MCIERR_INVALID_DEVICE_NAME)?.0,
            };
            (dev::open(typ, 0, &o, None)?, true)
        }
    };
    let res = string_command(id, &c.verb, &mut a, &mut r);
    if temp {
        // Opened for a PLAY that runs on: closed when it ends.
        let playing = res.is_ok() && c.verb == "play" && r.flags & MCI_WAIT == 0;
        if playing {
            dev::with(id, |d| d.auto_close = true);
        } else {
            let _ = dev::close(id);
        }
    }
    res
}

fn string_open(object: &str, a: &mut Args, r: &mut Req) -> Result<u16, u32> {
    r.msg = MCI_OPEN;
    r.text = a.value("alias")?;
    if a.flag("shareable") {
        r.flags |= MCI_OPEN_SHAREABLE;
    }
    let _ = a.flag("readonly");
    let ty = a.value("type")?;
    let typ = if let Some(t) = ty {
        r.element = Some(String::from(object));
        parse_device_name(&t).ok_or(MCIERR_INVALID_DEVICE_TYPE)?
    } else if let Some(t) = parse_device_name(object) {
        t
    } else {
        r.element = Some(String::from(object));
        (devtype_for_file(object).ok_or(MCIERR_EXTENSION_NOT_FOUND)?, 0)
    };
    if let Some(p) = a.number("parent")? {
        r.hwnd = p;
    }
    a.done()?;
    let id = dev::open(typ.0, typ.1, r, None)?;
    if r.flags & MCI_NOTIFY != 0 {
        post(Notify { hwnd: r.callback, user: r.user, msg: MCI_OPEN as u16 }, MCI_NOTIFY_SUCCESSFUL, id);
    }
    Ok(id)
}

fn time_format(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "milliseconds" | "ms" => MCI_FORMAT_MILLISECONDS,
        "mmtime" => MCI_FORMAT_MMTIME,
        "frames" => MCI_FORMAT_FRAMES,
        "bytes" => MCI_FORMAT_BYTES,
        "samples" => MCI_FORMAT_SAMPLES,
        _ => return None,
    })
}

/// A string command to an open device: its keywords become the message's
/// flags and fields.
fn string_command(id: u16, verb: &str, a: &mut Args, r: &mut Req) -> Result<String, u32> {
    r.msg = match verb {
        "close" => MCI_CLOSE,
        "play" => MCI_PLAY,
        "stop" => MCI_STOP,
        "pause" => MCI_PAUSE,
        "resume" => MCI_RESUME,
        "seek" => MCI_SEEK,
        "status" => MCI_STATUS,
        "set" => MCI_SET,
        "capability" => MCI_GETDEVCAPS,
        "info" => MCI_INFO,
        "load" => MCI_LOAD,
        "cue" => MCI_CUE,
        "acquire" => MCI_ACQUIREDEVICE,
        "release" => MCI_RELEASEDEVICE,
        "window" => MCI_WINDOW,
        "put" => MCI_PUT,
        "where" => MCI_WHERE,
        _ => return Err(MCIERR_UNRECOGNIZED_COMMAND),
    };
    match r.msg {
        MCI_PLAY => {
            if let Some(v) = a.number("from")? {
                r.flags |= MCI_FROM;
                r.from = v;
            }
            if let Some(v) = a.number("to")? {
                r.flags |= MCI_TO;
                r.to = v;
            }
            if a.flag("fast") {
                r.flags |= MCI_DGV_PLAY_FAST;
            }
            if a.flag("slow") {
                r.flags |= MCI_DGV_PLAY_SLOW;
            }
            if a.flag("repeat") {
                r.flags |= MCI_DGV_PLAY_REPEAT;
            }
        }
        MCI_SEEK => {
            if a.flag("to start") {
                r.flags |= MCI_TO_START;
            } else if a.flag("to end") {
                r.flags |= MCI_TO_END;
            } else if let Some(v) = a.number("to")? {
                r.flags |= MCI_TO;
                r.to = v;
            } else {
                return Err(MCIERR_MISSING_PARAMETER);
            }
        }
        MCI_STATUS => {
            r.flags |= MCI_STATUS_ITEM;
            r.item = [
                ("mode", MCI_STATUS_MODE),
                ("length", MCI_STATUS_LENGTH),
                ("position", MCI_STATUS_POSITION),
                ("ready", MCI_STATUS_READY),
                ("time format", MCI_STATUS_TIME_FORMAT),
                ("volume", MCI_STATUS_VOLUME),
                ("media present", MCI_STATUS_MEDIA_PRESENT),
                ("number of tracks", MCI_STATUS_NUMBER_OF_TRACKS),
                ("hwnd", MCI_VID_STATUS_HWND),
                ("horizontal video extent", MCI_VID_STATUS_VIDEO_X_EXTENT),
                ("vertical video extent", MCI_VID_STATUS_VIDEO_Y_EXTENT),
                ("normal rate", MCI_VID_STATUS_NORMAL_RATE),
                ("samplespersec", 0x4004),
                ("bitspersample", 0x4007),
                ("channels", 0x4003),
                ("format tag", 0x4002),
                ("bytespersec", 0x4005),
                ("alignment", 0x4006),
            ]
            .iter()
            .find(|(k, _)| a.flag(k))
            .map(|x| x.1)
            .ok_or(MCIERR_MISSING_PARAMETER)?;
        }
        MCI_SET => {
            if let Some(f) = a.value("time format")? {
                r.flags |= MCI_SET_TIME_FORMAT;
                r.item = time_format(&f).ok_or(MCIERR_INVALID_TIME_FORMAT_FLAG)?;
            }
            if let Some(v) = a.number("audio volume")?.or(a.number("volume")?) {
                r.flags |= MCI_SET_AUDIO | MCI_SET_VOLUME;
                r.value = v;
            }
            for (k, f, i) in [("samplespersec", MCI_WAVE_SET_SAMPLESPERSEC, 2), ("channels", MCI_WAVE_SET_CHANNELS, 1), ("bitspersample", MCI_WAVE_SET_BITSPERSAMPLE, 5)] {
                if let Some(v) = a.number(k)? {
                    r.flags |= f;
                    r.wave[i] = v;
                }
            }
            if let Some(t) = a.value("format tag")? {
                r.flags |= MCI_WAVE_SET_FORMATTAG;
                r.wave[0] = if t.eq_ignore_ascii_case("pcm") { 1 } else { parse_number(&t).ok_or(MCIERR_BAD_INTEGER)? };
            }
            // Video and audio on/off: accepted, nothing to do.
            for k in ["video on", "video off", "audio all on", "audio all off", "audio on", "audio off"] {
                let _ = a.flag(k);
            }
        }
        MCI_GETDEVCAPS => {
            r.flags |= MCI_GETDEVCAPS_ITEM;
            r.item = [
                ("can play", MCI_GETDEVCAPS_CAN_PLAY),
                ("can record", MCI_GETDEVCAPS_CAN_RECORD),
                ("can save", MCI_GETDEVCAPS_CAN_SAVE),
                ("can eject", MCI_GETDEVCAPS_CAN_EJECT),
                ("can lockeject", MCI_GETDEVCAPS_CAN_LOCKEJECT),
                ("can setvolume", MCI_GETDEVCAPS_CAN_SETVOLUME),
                ("can stream", MCI_GETDEVCAPS_CAN_STREAM),
                ("compound device", MCI_GETDEVCAPS_USES_FILES),
                ("uses files", MCI_GETDEVCAPS_USES_FILES),
                ("device type", MCI_GETDEVCAPS_DEVICE_TYPE),
                ("has audio", MCI_GETDEVCAPS_HAS_AUDIO),
                ("has video", MCI_GETDEVCAPS_HAS_VIDEO),
                ("horizontal video extent", MCI_VID_GETDEVCAPS_VIDEO_X_EXTENT),
                ("vertical video extent", MCI_VID_GETDEVCAPS_VIDEO_Y_EXTENT),
                ("normal rate", MCI_VID_GETDEVCAPS_NORMAL_RATE),
            ]
            .iter()
            .find(|(k, _)| a.flag(k))
            .map(|x| x.1)
            .ok_or(MCIERR_MISSING_PARAMETER)?;
        }
        MCI_INFO => {
            if a.flag("product") {
                r.flags |= MCI_INFO_PRODUCT;
            } else if a.flag("file") {
                r.flags |= MCI_INFO_FILE;
            } else {
                return Err(MCIERR_MISSING_PARAMETER);
            }
        }
        MCI_LOAD => {
            let _ = a.flag("readonly");
            let _ = a.flag("new");
            r.element = Some(a.next().ok_or(MCIERR_MISSING_PARAMETER)?);
        }
        MCI_CUE => {
            if a.flag("output") {
                r.flags |= MCI_CUE_OUTPUT;
            } else if a.flag("input") {
                r.flags |= MCI_CUE_INPUT;
            }
        }
        MCI_ACQUIREDEVICE | MCI_RELEASEDEVICE => {
            for k in ["exclusive instance", "exclusive", "queue", "return resource"] {
                let _ = a.flag(k);
            }
        }
        MCI_WINDOW => {
            if a.flag("handle default") {
                r.flags |= MCI_VID_WINDOW_DEFAULT;
            } else if let Some(h) = a.number("handle")? {
                r.flags |= MCI_VID_WINDOW_HWND;
                r.hwnd = h;
            }
            for (k, v) in [("state hide", SWP_HIDE), ("state show", SWP_SHOW), ("state minimize", SWP_MINIMIZE), ("state minimized", SWP_MINIMIZE), ("state maximize", SWP_MAXIMIZE), ("state restore", SWP_RESTORE), ("state activate", SWP_ACTIVATE), ("state deactivate", SWP_DEACTIVATE)] {
                if a.flag(k) {
                    r.flags |= MCI_VID_WINDOW_STATE;
                    r.value = v;
                }
            }
            if let Some(t) = a.value("text")? {
                r.flags |= MCI_VID_WINDOW_TEXT;
                r.text = Some(t);
            }
        }
        MCI_PUT => {
            let window = a.flag("window");
            let _ = a.flag("destination") || a.flag("source");
            if a.peek().is_some_and(|w| w.eq_ignore_ascii_case("at")) {
                let _ = a.next();
                let mut v = Vec::new();
                while v.len() < 4 {
                    match a.peek().and_then(parse_number) {
                        Some(n) => {
                            v.push(n as i32);
                            let _ = a.next();
                        }
                        None => break,
                    }
                }
                if v.len() < 2 {
                    return Err(MCIERR_MISSING_PARAMETER);
                }
                v.resize(4, 0);
                r.flags |= MCI_VID_PUT_RECT;
                r.rect = [v[0], v[1], v[2], v[3]];
            }
            r.flags |= if window { MCI_VID_PUT_WINDOW_MOVE } else { MCI_VID_PUT_DESTINATION };
            let _ = a.flag("move");
            let _ = a.flag("size");
        }
        MCI_WHERE => {
            r.flags |= if a.flag("source") {
                MCI_VID_WHERE_SOURCE
            } else if a.flag("window") {
                MCI_VID_WHERE_WINDOW
            } else {
                let _ = a.flag("destination");
                MCI_VID_WHERE_DESTINATION
            };
        }
        _ => {}
    }
    a.done()?;
    let reply = dev::command(id, r)?;
    Ok(reply.text())
}

// ---- The rest of the API ----

/// mciGetErrorString(ulError, pszBuffer, usLength).
#[unsafe(no_mangle)]
pub extern "C" fn mciGetErrorString(err: u32, buf: *mut u8, cb: u32) -> u32 {
    let cb = cb & 0xffff;
    if buf.is_null() || cb == 0 {
        return MCIERR_INVALID_BUFFER;
    }
    let text = if err & 0xffff == 0 { Some("The command completed successfully.") } else { error_text(err & 0xffff) };
    match text {
        Some(t) => {
            copy_out(t, buf, cb);
            MCIERR_SUCCESS
        }
        None => MCIERR_INVALID_FLAG,
    }
}

/// mciGetDeviceID(pszName): the ID of an open device or alias, 0 if none.
#[unsafe(no_mangle)]
pub extern "C" fn mciGetDeviceID(name: *const u8) -> u32 {
    dev::find(&cstr(name)).unwrap_or(0) as u32
}

/// mciQuerySysValue(iSysValue, pValue): MSV_* system settings.
#[unsafe(no_mangle)]
pub extern "C" fn mciQuerySysValue(item: u32, value: *mut u32) -> u32 {
    if value.is_null() {
        return 0;
    }
    let v = match item & 0xffff {
        // MSV_CLOSEDCAPTION, MSV_MASTERVOLUME, MSV_HEADPHONES,
        // MSV_SPEAKERS, MSV_WORKPATH, MSV_SYSQOSVALUE, MSV_SYSQOSERRORFLAG
        0 => 0,
        1 => 100,
        2 => 0,
        3 => 1,
        5 | 6 => 0,
        4 => {
            copy_out("C:\\MMOS2", value as *mut u8, 9);
            return 1;
        }
        _ => return 0,
    };
    unsafe { value.write_unaligned(v) };
    1
}

/// mciSetSysValue(iSysValue, pValue).
#[unsafe(no_mangle)]
pub extern "C" fn mciSetSysValue(_item: u32, _value: u32) -> u32 {
    1
}

/// Connections and groups: rs2's devices have none.
#[unsafe(no_mangle)]
pub extern "C" fn mciQueryDefaultConnections(_a: u32, _b: u32, _c: u32) -> u32 {
    MCIERR_UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mciDefaultConnection(_a: u32, _b: u32, _c: u32, _d: u32, _e: u32) -> u32 {
    MCIERR_UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mciQueryConnections(_a: u32, _b: u32, _c: u32, _d: u32) -> u32 {
    MCIERR_UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mciConnection(_a: u32, _b: u32, _c: u32, _d: u32) -> u32 {
    MCIERR_UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mciMakeGroup(_a: u32, _b: u32, _c: u32, _d: u32, _e: u32) -> u32 {
    MCIERR_UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mciDeleteGroup(_a: u32) -> u32 {
    MCIERR_UNSUPPORTED_FUNCTION
}

#[unsafe(no_mangle)]
pub extern "C" fn mdmDriverNotify(_a: u32, _b: u32, _c: u32, _d: u32, _e: u32) -> u32 {
    0
}
