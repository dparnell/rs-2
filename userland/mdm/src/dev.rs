//! The open devices of the process: their IDs, aliases, pending
//! notifications, and the commands every device answers the same way.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::req::*;
use crate::sys::*;
use crate::{sequencer, video, wave};
use mci::*;

pub const MCI_NOTIFY_SUCCESSFUL: u32 = 0;
pub const MCI_NOTIFY_SUPERSEDED: u32 = 1;
pub const MCI_NOTIFY_ABORTED: u32 = 2;

pub const MCI_MODE_NOT_READY: u32 = 1;
pub const MCI_MODE_PAUSE: u32 = 2;
pub const MCI_MODE_PLAY: u32 = 3;
pub const MCI_MODE_STOP: u32 = 4;

/// A notification to send when an asynchronous command ends.
#[derive(Clone, Copy)]
pub struct Notify {
    pub hwnd: u32,
    pub user: u16,
    pub msg: u16,
}

pub enum Kind {
    Wave(wave::Wave),
    Video(video::Video),
    Seq(sequencer::Seq),
}

pub struct Device {
    pub id: u16,
    pub typ: u16,
    pub ordinal: u16,
    pub alias: Option<String>,
    pub element: Option<String>,
    /// Opened by a string command on a file name: closed when the command
    /// has run.
    pub auto_close: bool,
    pub shareable: bool,
    pub time_format: u32,
    pub mode: u32,
    /// The pending PLAY (or other asynchronous) command's notification.
    pub pending: Option<Notify>,
    /// Bumped by every PLAY, STOP, SEEK and CLOSE: a player thread whose
    /// generation is no longer the device's has been stopped.
    pub epoch: u32,
    pub kind: Kind,
}

pub static LOCK: Lock = Lock::new();
static mut TABLE: Vec<Box<Device>> = Vec::new();
static mut NEXT_ID: u16 = 1;

/// The device table; hold LOCK.
#[allow(static_mut_refs)]
pub fn table() -> &'static mut Vec<Box<Device>> {
    unsafe { &mut *core::ptr::addr_of_mut!(TABLE) }
}

/// Run `f` on device `id` with the lock held.
pub fn with<R>(id: u16, f: impl FnOnce(&mut Device) -> R) -> Option<R> {
    let _g = LOCK.hold();
    table().iter_mut().find(|d| d.id == id).map(|d| f(d))
}

/// A device by alias or name ("movie", "Waveaudio01"), for the string
/// interface and mciGetDeviceID.
pub fn find(name: &str) -> Option<u16> {
    let _g = LOCK.hold();
    let t = table();
    if let Some(d) = t.iter().find(|d| d.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(name))) {
        return Some(d.id);
    }
    if let Some(d) = t.iter().find(|d| d.alias.is_none() && d.element.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(name))) {
        return Some(d.id);
    }
    let (ty, ord) = parse_device_name(name)?;
    t.iter().find(|d| d.alias.is_none() && d.typ == ty && (ord == 0 || d.ordinal == ord)).map(|d| d.id)
}

pub fn post(n: Notify, code: u32, id: u16) {
    if n.hwnd != 0 {
        unsafe { WinPostMsg(n.hwnd, MM_MCINOTIFY, mp(code, n.user as u32), mp(id as u32, n.msg as u32)) };
    }
}

impl Device {
    /// Settle the pending notification (if any) with `code`.
    pub fn finish(&mut self, code: u32) {
        if let Some(n) = self.pending.take() {
            post(n, code, self.id);
        }
    }

    /// A new asynchronous command takes the pending notification's place.
    pub fn take_notify(&mut self, r: &Req) {
        if r.flags & MCI_NOTIFY != 0 {
            self.finish(MCI_NOTIFY_SUPERSEDED);
            self.pending = Some(Notify { hwnd: r.callback, user: r.user, msg: r.msg as u16 });
        }
    }
}

/// The devices MDM has: one wave audio device (even without sound
/// hardware, which plays silently in real time) and one digital video
/// device (FLI/FLC animations).
pub const INSTALLED: &[u16] = &[DEVTYPE_WAVEFORM_AUDIO, DEVTYPE_SEQUENCER, DEVTYPE_DIGITAL_VIDEO];

/// MCI_OPEN: a new device of `typ` (and the element, if any).
pub fn open(typ: u16, ordinal: u16, r: &Req, playlist: Option<u32>) -> Result<u16, u32> {
    if !INSTALLED.contains(&typ) {
        return Err(MCIERR_DEVICE_NOT_FOUND);
    }
    if ordinal > 1 {
        return Err(MCIERR_INVALID_DEVICE_ORDINAL);
    }
    if r.text.as_deref().is_some_and(table_has_alias) {
        return Err(MCIERR_DUPLICATE_ALIAS);
    }
    let kind = match typ {
        DEVTYPE_WAVEFORM_AUDIO => Kind::Wave(wave::Wave::new(r.element.as_deref(), playlist)?),
        DEVTYPE_SEQUENCER => Kind::Seq(sequencer::Seq::new(r.element.as_deref())?),
        _ => Kind::Video(video::Video::new(r.element.as_deref(), r.hwnd)?),
    };
    let _g = LOCK.hold();
    let id = unsafe {
        let id = NEXT_ID;
        NEXT_ID = NEXT_ID.wrapping_add(1).max(1);
        id
    };
    let time_format = if typ == DEVTYPE_WAVEFORM_AUDIO { MCI_FORMAT_MMTIME } else { MCI_FORMAT_MMTIME };
    table().push(Box::new(Device {
        id,
        typ,
        ordinal: 1,
        alias: r.text.clone(),
        element: if playlist.is_some() { None } else { r.element.clone() },
        auto_close: false,
        shareable: r.flags & MCI_OPEN_SHAREABLE != 0,
        time_format,
        mode: MCI_MODE_STOP,
        pending: None,
        epoch: 0,
        kind,
    }));
    Ok(id)
}

fn table_has_alias(a: &str) -> bool {
    let _g = LOCK.hold();
    table().iter().any(|d| d.alias.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(a)))
}

/// MCI_CLOSE: stop the device and forget it.
pub fn close(id: u16) -> Result<(), u32> {
    let d = {
        let _g = LOCK.hold();
        let t = table();
        let Some(i) = t.iter().position(|d| d.id == id) else { return Err(MCIERR_INVALID_DEVICE_ID) };
        let mut d = t.remove(i);
        d.epoch = d.epoch.wrapping_add(1);
        d.finish(MCI_NOTIFY_ABORTED);
        d
    };
    match d.kind {
        Kind::Wave(mut w) => w.stop(),
        Kind::Video(mut v) => v.close(),
        Kind::Seq(mut q) => q.stop(),
    }
    Ok(())
}

/// Close every device (the process is ending).
pub fn close_all() {
    let ids: Vec<u16> = {
        let _g = LOCK.hold();
        table().iter().map(|d| d.id).collect()
    };
    for id in ids {
        let _ = close(id);
    }
}

/// A command to an open device.
pub fn command(id: u16, r: &Req) -> Result<Reply, u32> {
    let typ = with(id, |d| d.typ).ok_or(MCIERR_INVALID_DEVICE_ID)?;
    let res = match r.msg {
        MCI_CLOSE => close(id).map(|_| Reply::None),
        MCI_ACQUIREDEVICE | MCI_RELEASEDEVICE => Ok(Reply::None),
        MCI_GETDEVCAPS if r.flags & MCI_GETDEVCAPS_MESSAGE != 0 => Ok(Reply::Bool(supports(typ, r.item))),
        MCI_GETDEVCAPS if r.flags & MCI_GETDEVCAPS_ITEM != 0 => common_caps(typ, r.item).map_or_else(|| specific(id, typ, r), Ok),
        MCI_INFO if r.flags & MCI_INFO_PRODUCT != 0 => Ok(Reply::Text(String::from(match typ {
            DEVTYPE_WAVEFORM_AUDIO => "rs2 Wave Audio",
            DEVTYPE_SEQUENCER => "rs2 MIDI Sequencer",
            _ => "rs2 Digital Video",
        }))),
        MCI_INFO if r.flags & MCI_INFO_FILE != 0 => Ok(Reply::Text(with(id, |d| d.element.clone().unwrap_or_default()).unwrap_or_default())),
        MCI_STATUS if r.flags & MCI_STATUS_ITEM == 0 => Err(MCIERR_MISSING_FLAG),
        MCI_STATUS if r.item == MCI_STATUS_MODE => Ok(with(id, |d| Reply::Named(d.mode, mode_name(d.mode))).unwrap()),
        MCI_STATUS if r.item == MCI_STATUS_READY => Ok(Reply::Bool(true)),
        MCI_STATUS if r.item == MCI_STATUS_TIME_FORMAT => Ok(with(id, |d| Reply::Named(d.time_format, time_format_name(d.time_format))).unwrap()),
        MCI_STATUS if r.item == MCI_STATUS_MEDIA_PRESENT => Ok(Reply::Bool(true)),
        MCI_STATUS if r.item == MCI_STATUS_NUMBER_OF_TRACKS => Ok(Reply::Num(1)),
        MCI_SET if r.flags & MCI_SET_TIME_FORMAT != 0 => {
            let ok = match typ {
                DEVTYPE_WAVEFORM_AUDIO => matches!(r.item, MCI_FORMAT_MILLISECONDS | MCI_FORMAT_MMTIME | MCI_FORMAT_BYTES | MCI_FORMAT_SAMPLES),
                DEVTYPE_SEQUENCER => matches!(r.item, MCI_FORMAT_MILLISECONDS | MCI_FORMAT_MMTIME),
                _ => matches!(r.item, MCI_FORMAT_MILLISECONDS | MCI_FORMAT_MMTIME | MCI_FORMAT_FRAMES),
            };
            if !ok {
                return Err(MCIERR_INVALID_TIME_FORMAT_FLAG);
            }
            with(id, |d| d.time_format = r.item);
            if r.flags & !(MCI_SET_TIME_FORMAT | MCI_WAIT | MCI_NOTIFY) != 0 { specific(id, typ, r) } else { Ok(Reply::None) }
        }
        _ => specific(id, typ, r),
    };
    // Commands that end at once notify at once; PLAY notifies when it ends.
    if res.is_ok() && r.flags & MCI_NOTIFY != 0 && r.msg != MCI_PLAY && r.msg != MCI_CLOSE {
        post(Notify { hwnd: r.callback, user: r.user, msg: r.msg as u16 }, MCI_NOTIFY_SUCCESSFUL, id);
    }
    if res.is_ok() && r.msg == MCI_CLOSE && r.flags & MCI_NOTIFY != 0 {
        post(Notify { hwnd: r.callback, user: r.user, msg: r.msg as u16 }, MCI_NOTIFY_SUCCESSFUL, id);
    }
    res
}

fn specific(id: u16, typ: u16, r: &Req) -> Result<Reply, u32> {
    match typ {
        DEVTYPE_WAVEFORM_AUDIO => wave::command(id, r),
        DEVTYPE_SEQUENCER => sequencer::command(id, r),
        _ => video::command(id, r),
    }
}

/// GETDEVCAPS MESSAGE: whether the device understands a message.
fn supports(typ: u16, msg: u32) -> bool {
    let common = [MCI_OPEN, MCI_CLOSE, MCI_PLAY, MCI_SEEK, MCI_STOP, MCI_PAUSE, MCI_RESUME, MCI_INFO, MCI_GETDEVCAPS, MCI_STATUS, MCI_SET, MCI_ACQUIREDEVICE, MCI_RELEASEDEVICE, MCI_LOAD, MCI_CUE];
    common.contains(&msg) || (typ == DEVTYPE_DIGITAL_VIDEO && [MCI_PUT, MCI_WHERE, MCI_WINDOW].contains(&msg))
}

/// GETDEVCAPS items every device answers.
fn common_caps(typ: u16, item: u32) -> Option<Reply> {
    let video = typ == DEVTYPE_DIGITAL_VIDEO;
    Some(match item {
        MCI_GETDEVCAPS_CAN_PLAY | MCI_GETDEVCAPS_USES_FILES | MCI_GETDEVCAPS_CAN_STREAM => Reply::Bool(true),
        MCI_GETDEVCAPS_CAN_EJECT | MCI_GETDEVCAPS_CAN_LOCKEJECT | MCI_GETDEVCAPS_CAN_RECORD | MCI_GETDEVCAPS_CAN_RECORD_INSERT | MCI_GETDEVCAPS_CAN_SAVE | MCI_GETDEVCAPS_HAS_IMAGE => {
            Reply::Bool(false)
        }
        MCI_GETDEVCAPS_CAN_PROCESS_INTERNAL => Reply::Bool(false),
        MCI_GETDEVCAPS_CAN_SETVOLUME | MCI_GETDEVCAPS_HAS_AUDIO => Reply::Bool(!video),
        MCI_GETDEVCAPS_HAS_VIDEO => Reply::Bool(video),
        MCI_GETDEVCAPS_DEVICE_TYPE => Reply::Named(typ as u32, devtype_name(typ).unwrap_or("other")),
        _ => return None,
    })
}

/// SYSINFO (a system command: no device needs to be open).
pub fn sysinfo(r: &Req, typ: Option<u16>) -> Result<Reply, u32> {
    let all = typ.is_none();
    let open_only = r.flags & MCI_SYSINFO_OPEN != 0;
    let matching = |t: u16| all || typ == Some(t);
    if r.flags & MCI_SYSINFO_QUANTITY != 0 {
        let n = if open_only {
            let _g = LOCK.hold();
            table().iter().filter(|d| matching(d.typ)).count()
        } else {
            INSTALLED.iter().filter(|&&t| matching(t)).count()
        };
        return Ok(Reply::Num(n as u32));
    }
    if r.flags & MCI_SYSINFO_NAME != 0 {
        if open_only {
            let _g = LOCK.hold();
            let names: Vec<String> = table().iter().filter(|d| matching(d.typ)).map(|d| d.alias.clone().unwrap_or_else(|| device_name(d.typ, d.ordinal))).collect();
            if all {
                return Ok(Reply::Text(names.join(" ")));
            }
            return names.get(r.value.saturating_sub(1) as usize).cloned().map(Reply::Text).ok_or(MCIERR_OUTOFRANGE);
        }
        if all {
            let names: Vec<String> = INSTALLED.iter().map(|&t| device_name(t, 1)).collect();
            return Ok(Reply::Text(names.join(" ")));
        }
        if r.value != 1 {
            return Err(MCIERR_OUTOFRANGE);
        }
        return Ok(Reply::Text(device_name(typ.unwrap(), 1)));
    }
    if r.flags & MCI_SYSINFO_INSTALLNAME != 0 {
        let t = typ.ok_or(MCIERR_DEVICE_TYPE_REQUIRED)?;
        return Ok(Reply::Text(alloc::format!("rs2{}", device_name(t, 1))));
    }
    Err(MCIERR_MISSING_FLAG)
}
