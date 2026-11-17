use std::prelude::rust_2024::*;
use std::vec;
use std::collections::HashMap;

use crate::asm::*;
use crate::*;

#[derive(Default)]
struct Mock {
    mem: HashMap<(u8, u64), u8>,
    log: Vec<String>,
    writes: Vec<(u8, u64, u8, u64)>,
}

impl Host for Mock {
    fn read(&mut self, space: u8, addr: u64, bits: u8) -> Option<u64> {
        let mut v = 0;
        for i in 0..bits as u64 / 8 {
            v |= (*self.mem.get(&(space, addr + i)).unwrap_or(&0) as u64) << (8 * i);
        }
        Some(v)
    }
    fn write(&mut self, space: u8, addr: u64, bits: u8, value: u64) -> bool {
        self.writes.push((space, addr, bits, value));
        for i in 0..bits as u64 / 8 {
            self.mem.insert((space, addr + i), (value >> (8 * i)) as u8);
        }
        true
    }
    fn debug(&mut self, s: &str) {
        self.log.push(s.to_string());
    }
}

fn load(aml: &[Vec<u8>]) -> Interp<Mock> {
    let mut i = Interp::new(Mock::default(), 2);
    i.load(&aml.concat());
    assert_eq!(i.load_errors, 0, "{:?}", i.host.log);
    i
}

#[test]
fn battery_ssdt_charging() {
    let t = battery_ssdt(false);
    let mut i = Interp::new(Mock::default(), 2);
    i.load(&t[36..]);
    assert_eq!(i.load_errors, 0, "{:?}", i.host.log);
    assert_eq!(i.find_devices("PNP0C0A"), vec!["_SB_.BAT0".to_string()]);
    assert_eq!(i.find_devices("ACPI0003"), vec!["_SB_.ADP1".to_string()]);
    assert_eq!(i.sta("_SB_.BAT0"), 0x1f);
    let bst = i.eval("_SB_.BAT0._BST", vec![]).unwrap().elements().unwrap();
    let v: Vec<u64> = bst.iter().map(|o| o.int().unwrap()).collect();
    assert_eq!(v, vec![2, 1500, 3456, 11400]);
    let bif = i.eval("_SB_.BAT0._BIF", vec![]).unwrap().elements().unwrap();
    assert_eq!(bif[2].int(), Some(4800));
    assert_eq!(bif[9].text().as_deref(), Some("RS2 Test Battery"));
    assert_eq!(i.eval_int("_SB_.ADP1._PSR", vec![]).unwrap(), 1);
    // Checksum.
    assert_eq!(t.iter().fold(0u8, |a, &c| a.wrapping_add(c)), 0);
}

#[test]
fn battery_ssdt_discharging() {
    let t = battery_ssdt(true);
    let mut i = Interp::new(Mock::default(), 2);
    i.load(&t[36..]);
    let bst = i.eval("_SB_.BAT0._BST", vec![]).unwrap().elements().unwrap();
    assert_eq!(bst[0].int(), Some(1));
    assert_eq!(bst[2].int(), Some(1680));
    assert_eq!(i.eval_int("_SB_.ADP1._PSR", vec![]).unwrap(), 0);
}

#[test]
fn arithmetic_and_locals() {
    let mut i = load(&[method(
        "TEST",
        2,
        &[
            op2(0x72, arg(0), arg(1), local(0)),
            op2(0x77, local(0), int(3), local(1)),
            divide(local(1), int(7), local(2), local(3)),
            op2(0x79, local(3), int(4), local(4)),
            op2(0x7d, local(4), local(2), local(5)),
            ret(local(5)),
        ],
    )]);
    // (5+9)*3 = 42; 42/7 = 6 r 0; 6<<4 = 96 | 0
    assert_eq!(i.eval_int("TEST", vec![Obj::Int(5), Obj::Int(9)]).unwrap(), 96);
}

#[test]
fn int32_wraps() {
    let mut i = Interp::new(Mock::default(), 1);
    i.load(&method("TEST", 0, &[ret(op2(0x74, int(0), int(1), none()))]));
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 0xffff_ffff);
}

#[test]
fn if_else_while_break() {
    let mut i = load(&[method(
        "TEST",
        1,
        &[
            store(int(0), local(0)),
            while_(int(1), &[increment(local(0)), if_(cmp(0x93, local(0), arg(0)), &[brk()])]),
            if_(cmp(0x94, local(0), int(10)), &[ret(int(1))]),
            else_(&[ret(local(0))]),
        ],
    )]);
    assert_eq!(i.eval_int("TEST", vec![Obj::Int(7)]).unwrap(), 7);
    assert_eq!(i.eval_int("TEST", vec![Obj::Int(12)]).unwrap(), 1);
}

#[test]
fn search_rules_and_nested_calls() {
    let mut i = load(&[
        def_name("GVAL", int(40)),
        scope(
            "\\_SB",
            &[device(
                "DEV0",
                &[
                    method("HELP", 1, &[ret(op2(0x72, arg(0), name("GVAL"), none()))]),
                    method("_INI", 0, &[store(call("HELP", &[int(2)]), name("^^^GVAL"))]),
                ],
            )],
        ),
    ]);
    assert_eq!(i.initialize(), 0);
    assert_eq!(i.eval_int("GVAL", vec![]).unwrap(), 42);
}

#[test]
fn osi() {
    let mut i = load(&[method("TEST", 0, &[if_(call("\\_OSI", &[string("Windows 2015")]), &[ret(int(1))]), ret(int(0))])]);
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 1);
    let mut i = load(&[method("TEST", 0, &[if_(call("\\_OSI", &[string("Linux")]), &[ret(int(1))]), ret(int(0))])]);
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 0);
}

#[test]
fn system_io_fields_preserve() {
    let mut i = load(&[
        op_region("PRT0", space::IO, int(0x80), int(4)),
        field("PRT0", 0x01, &[unit("LOW_", 4), unit("HIGH", 4), unit("BYT1", 8), unit("WRD1", 16)]),
        method("TEST", 0, &[store(int(0xa), name("HIGH")), store(int(0x1234), name("WRD1")), ret(name("BYT1"))]),
    ]);
    i.host.mem.insert((space::IO, 0x80), 0x05);
    i.host.mem.insert((space::IO, 0x81), 0x77);
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 0x77);
    assert_eq!(i.host.mem[&(space::IO, 0x80)], 0xa5);
    assert_eq!(i.host.mem[&(space::IO, 0x82)], 0x34);
    assert_eq!(i.host.mem[&(space::IO, 0x83)], 0x12);
    assert_eq!(i.eval_int("LOW_", vec![]).unwrap(), 5);
}

#[test]
fn ec_region_word_field_by_bytes() {
    // An EmbeddedControl region: byte access, a 16-bit field.
    let mut i = load(&[
        op_region("ECOR", space::EC, int(0), int(0x100)),
        field("ECOR", 0x01, &[unit("", 0x60 * 8), unit("BRC0", 16), unit("BST0", 8)]),
    ]);
    i.host.mem.insert((space::EC, 0x60), 0x34);
    i.host.mem.insert((space::EC, 0x61), 0x12);
    i.host.mem.insert((space::EC, 0x62), 0x02);
    assert_eq!(i.eval_int("BRC0", vec![]).unwrap(), 0x1234);
    assert_eq!(i.eval_int("BST0", vec![]).unwrap(), 2);
    assert!(i.host.writes.is_empty());
}

#[test]
fn index_field_cmos() {
    let mut i = load(&[
        op_region("CMS0", space::IO, int(0x70), int(2)),
        field("CMS0", 0x01, &[unit("IDX0", 8), unit("DAT0", 8)]),
        index_field("IDX0", "DAT0", 0x01, &[unit("", 0x40 * 8), unit("CM40", 8), unit("CM41", 8)]),
        method("TEST", 0, &[store(int(0x99), name("CM41")), ret(name("CM41"))]),
    ]);
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 0x99);
    assert!(i.host.writes.contains(&(space::IO, 0x70, 8, 0x41)));
    assert!(i.host.writes.contains(&(space::IO, 0x71, 8, 0x99)));
}

#[test]
fn buffers_and_buffer_fields() {
    let mut i = load(&[method(
        "TEST",
        0,
        &[
            store(buffer(&[0, 0, 0, 0, 0, 0, 0, 0]), local(0)),
            create_dword_field(local(0), int(4), "DW01"),
            store(int(0xdeadbeef), name("DW01")),
            store(int(0x11), index(local(0), int(0), none())),
            ret(local(0)),
        ],
    )]);
    let b = i.eval("TEST", vec![]).unwrap().bytes().unwrap();
    assert_eq!(b, vec![0x11, 0, 0, 0, 0xef, 0xbe, 0xad, 0xde]);
    // The method's buffer field is gone.
    assert!(!i.exists("TEST.DW01"));
}

#[test]
fn packages_sizeof_and_store_copies() {
    let mut i = load(&[
        def_name("PKG0", package(&[int(1), int(2), string("three")])),
        method(
            "TEST",
            0,
            &[
                store(name("PKG0"), local(0)),
                store(int(9), index(local(0), int(0), none())),
                op2(0x72, deref(index(name("PKG0"), int(0), none())), sizeof(local(0)), local(1)),
                ret(local(1)),
            ],
        ),
    ]);
    // PKG0[0] is still 1 (Local0 was a copy): 1 + 3.
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 4);
}

#[test]
fn notify_recorded_and_mutex() {
    let mut i = load(&[
        scope("\\_SB", &[device("BAT0", &[def_name("_HID", eisa("PNP0C0A"))]), mutex("MTX0")]),
        method("TEST", 0, &[acquire("\\_SB.MTX0"), notify("\\_SB.BAT0", int(0x80)), release("\\_SB.MTX0")]),
    ]);
    i.eval("TEST", vec![]).unwrap();
    assert_eq!(i.take_notifications(), vec![("_SB_.BAT0".to_string(), 0x80)]);
}

#[test]
fn strings_compare_and_concat() {
    let mut i = load(&[method(
        "TEST",
        0,
        &[
            op2(0x73, string("ab"), string("cd"), local(0)),
            if_(cmp(0x93, local(0), string("abcd")), &[ret(int(1))]),
            ret(int(0)),
        ],
    )]);
    assert_eq!(i.eval_int("TEST", vec![]).unwrap(), 1);
}

#[test]
fn load_skips_bad_terms() {
    // An unknown opcode inside a method body does not stop the load;
    // a broken top-level term inside a scope only ends that scope.
    let mut i = Interp::new(Mock::default(), 2);
    i.load(
        &[
            method("BAD0", 0, &[vec![0x5b, 0x99]]),
            def_name("AFTR", int(5)),
            if_(cmp(0x93, name("NOPE"), int(1)), &[def_name("INIF", int(1))]),
            def_name("LAST", int(6)),
        ]
        .concat(),
    );
    assert_eq!(i.eval_int("AFTR", vec![]).unwrap(), 5);
    assert_eq!(i.eval_int("LAST", vec![]).unwrap(), 6);
    assert!(i.eval("BAD0", vec![]).is_err());
    assert_eq!(i.load_errors, 1);
}

#[test]
fn endless_loop_stops() {
    let mut i = load(&[method("SPIN", 0, &[while_(int(1), &[])])]);
    assert_eq!(i.eval("SPIN", vec![]).unwrap_err(), Error::Limit);
}

#[test]
fn eisa_round_trip() {
    let b = eisa("PNP0C0A");
    let v = u32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    assert_eq!(eisa_id(v), "PNP0C0A");
}
