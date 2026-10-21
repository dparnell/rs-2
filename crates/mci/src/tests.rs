extern crate std;

use super::playlist::*;
use super::*;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec;

#[test]
fn splits_words_and_quotes() {
    assert_eq!(words("play  \"C:\\My Movies\\a.flc\" wait"), vec!["play", "C:\\My Movies\\a.flc", "wait"]);
    assert_eq!(words("  "), Vec::<String>::new());
    assert_eq!(words("set x \"\""), vec!["set", "x", ""]);
}

#[test]
fn parses_commands() {
    let c = parse("open digitalvideo alias movie wait shareable").unwrap();
    assert_eq!(c.verb, "open");
    assert_eq!(c.object, "digitalvideo");
    assert_eq!(c.flags, MCI_WAIT);
    let mut a = c.args;
    assert_eq!(a.value("alias").unwrap().as_deref(), Some("movie"));
    assert!(a.flag("shareable"));
    assert!(a.done().is_ok());

    let c = parse("PLAY MAHJONGG.FLC WAIT FAST").unwrap();
    assert_eq!((c.verb.as_str(), c.object.as_str(), c.flags), ("play", "MAHJONGG.FLC", MCI_WAIT));
    let mut a = c.args;
    assert!(a.flag("fast"));

    assert_eq!(parse("").unwrap_err(), MCIERR_MISSING_COMMAND_STRING);
    assert_eq!(parse("close").unwrap_err(), MCIERR_MISSING_PARAMETER);
    assert_eq!(parse("play x wait notify").unwrap_err(), MCIERR_FLAGS_NOT_COMPATIBLE);
    assert!(parse("masteraudio volume 50").unwrap().object.is_empty());
}

#[test]
fn reads_keywords() {
    let mut a = parse("put movie destination at 0 0 320 200 notify").unwrap().args;
    assert_eq!(a.numbers("destination at", 4).unwrap(), Some(vec![0, 0, 320, 200]));
    assert!(a.done().is_ok());

    let mut a = parse("seek movie to start").unwrap().args;
    assert!(!a.flag("to end"));
    assert!(a.flag("to start"));

    let mut a = parse("sysinfo waveaudio quantity wait").unwrap().args;
    assert!(a.flag("quantity"));
    assert!(!a.flag("open"));

    let mut a = parse("window movie handle 0x80000012").unwrap().args;
    assert_eq!(a.number("handle").unwrap(), Some(0x8000_0012));

    let mut a = parse("open x alias").unwrap().args;
    assert_eq!(a.value("alias").unwrap_err(), MCIERR_MISSING_STRING_ARGUMENT);

    let mut a = parse("set x volume loud").unwrap().args;
    assert_eq!(a.number("volume").unwrap_err(), MCIERR_BAD_INTEGER);

    let a = parse("play x backwards").unwrap().args;
    assert_eq!(a.done().unwrap_err(), MCIERR_UNRECOGNIZED_KEYWORD);
}

#[test]
fn device_names() {
    assert_eq!(parse_device_name("waveaudio"), Some((7, 0)));
    assert_eq!(parse_device_name("Digitalvideo01"), Some((12, 1)));
    assert_eq!(parse_device_name("movie"), None);
    assert_eq!(device_name(7, 1), "Waveaudio01");
    assert_eq!(devtype_for_file("C:\\MMOS2\\SOUNDS\\BELLS.WAV"), Some(7));
    assert_eq!(devtype_for_file("OS2CHESS.FLC"), Some(12));
    assert_eq!(devtype_for_file("README"), None);
    assert_eq!(parse_number("-5"), Some((-5i32) as u32));
}

#[test]
fn error_texts() {
    assert_eq!(MCIERR_INVALID_DEVICE_ID, 5001);
    assert_eq!(error_text(MCIERR_FILE_NOT_FOUND), Some("The file cannot be found."));
    assert!(ERRORS.iter().all(|e| !e.2.is_empty()));
    assert_eq!(bool_text(true), "TRUE".to_string());
}

struct Mem(BTreeMap<u32, u32>);

impl Memory for Mem {
    fn read(&self, a: u32) -> u32 {
        *self.0.get(&a).unwrap_or(&0xdead)
    }
    fn write(&mut self, a: u32, v: u32) {
        self.0.insert(a, v);
    }
}

fn program(base: u32, ins: &[[u32; 4]]) -> Mem {
    let mut m = Mem(BTreeMap::new());
    for (i, x) in ins.iter().enumerate() {
        for (j, v) in x.iter().enumerate() {
            m.0.insert(base + i as u32 * 16 + j as u32 * 4, *v);
        }
    }
    m
}

#[test]
fn playlist_data_then_exit() {
    // What OS/2's Solitaire hands over: one buffer, then EXIT.
    let mut m = program(0x1000, &[[DATA_OPERATION, 0x5000, 1234, 0], [EXIT_OPERATION, 0, 0, 0]]);
    let mut p = Processor::new(0x1000);
    assert_eq!(p.next(&mut m), Step::Data { addr: 0x5000, len: 1234, pos_at: 0x100c });
    assert_eq!(p.next(&mut m), Step::Exit);
    p.rewind();
    assert!(matches!(p.next(&mut m), Step::Data { .. }));
}

#[test]
fn playlist_loops_and_calls() {
    // 0: LOOP 3 times, else to 4; 1: CALL 5; 2: BRANCH 0; 3: (unused);
    // 4: EXIT; 5: DATA; 6: MESSAGE 77; 7: RETURN.
    let mut m = program(
        0,
        &[
            [LOOP_OPERATION, 3, 4, 0],
            [CALL_OPERATION, 0, 5, 0],
            [BRANCH_OPERATION, 0, 0, 0],
            [NOP_OPERATION, 0, 0, 0],
            [EXIT_OPERATION, 0, 0, 0],
            [DATA_OPERATION, 0x9000, 10, 0],
            [MESSAGE_OPERATION, 0, 77, 0],
            [RETURN_OPERATION, 0, 0, 0],
        ],
    );
    let mut p = Processor::new(0);
    let mut data = 0;
    let mut msgs = 0;
    loop {
        match p.next(&mut m) {
            Step::Data { .. } => data += 1,
            Step::Message(77) => msgs += 1,
            Step::Exit => break,
            s => panic!("{s:?}"),
        }
    }
    assert_eq!((data, msgs), (3, 3));
    // The loop counter is reset when the loop ends.
    assert_eq!(m.read(12), 0);
}

#[test]
fn playlist_stops_on_nonsense() {
    let mut m = program(0, &[[BRANCH_OPERATION, 0, 0, 0]]);
    assert_eq!(Processor::new(0).next(&mut m), Step::Exit);
    let mut m = program(0, &[[RETURN_OPERATION, 0, 0, 0]]);
    assert_eq!(Processor::new(0).next(&mut m), Step::Exit);
    let mut m = program(0, &[[99, 0, 0, 0]]);
    assert_eq!(Processor::new(0).next(&mut m), Step::Exit);
}
