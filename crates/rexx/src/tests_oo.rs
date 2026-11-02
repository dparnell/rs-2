//! Object REXX tests on the in-memory host.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use crate::tests::{MemHost, run_oo, run_oo_with};

fn say(expr: &str) -> String {
    let out = run_oo(&alloc::format!("say {expr}\n"));
    out.join("|")
}

/// Run a test program that prints `ok - ...`/`FAIL - ...` lines and
/// return the failures (and the whole output for the message).
fn checks(h: &mut MemHost, src: &str) -> (Vec<String>, String) {
    let (out, r) = run_oo_with(src, h);
    let all = out.join("\n");
    let mut fails: Vec<String> = out.iter().filter(|l| !l.starts_with("ok - ")).cloned().collect();
    if r.is_err() {
        fails.push(alloc::format!("result {r:?}"));
    }
    (fails, all)
}

#[test]
fn basics() {
    assert_eq!(say("'abc'~reverse"), "cba");
    assert_eq!(say(".array~of(1,2,3)~items"), "3");
    assert_eq!(say(".nil"), "The NIL object");
    assert_eq!(say("(3+4)~d2x"), "7");
    assert_eq!(say("10~format(3,2)"), " 10.00");
    assert_eq!(say(".true .false"), "1 0");
    assert_eq!(say("'a-b'~changestr('-', '+')"), "a+b");
    assert_eq!(say(".array~new~class~id"), "Array");
    assert_eq!(say(".array~new"), "an Array");
    let out = run_oo("parse version v\nsay v\n");
    assert_eq!(out, ["OBJREXX 6.00 18 May 1999"]);
}

#[test]
fn classes_and_methods() {
    let out = run_oo(
        "c = .counter~new(5)\nc~step\nc~step\nsay c~value c~class~id c\nd = .big~new(10)\nd~step\nsay d~value\n\
         say .counter~instances .big~instances\nsay .big~superclasses[1]~id\n\
         ::class counter public\n::method init class\n  expose count\n  count = 0\n::method instances class\n  expose count\n  return count\n\
         ::method new class\n  expose count\n  count = count + 1\n  forward class (super)\n::method value attribute\n\
         ::method init\n  expose value\n  use arg value\n::method step\n  expose value\n  value = value + 1\n\
         ::class big subclass counter\n::method step\n  self~step:super\n  self~value = self~value * 2 - 2\n",
    );
    assert_eq!(out, ["7 COUNTER a COUNTER", "20", "1 1", "COUNTER"]);
}

#[test]
fn collections() {
    let out = run_oo(
        "l = .list~new; l~insert('one'); l~insert('two')\nsay l~items l~firstItem\n\
         q = .queue~new; q~queue('a'); q~push('b'); say q~pull q~pull\n\
         d = .directory~new; d~colour = 'blue'; d['SIZE'] = 3; say d~colour d['SIZE'] d~hasIndex('COLOUR')\n\
         t = .table~new; t[1] = 'first'; t['k'] = 'second'; say t~items t['k']\n\
         say .set~of(1,2,2,3)~items .bag~of(1,2,2,3)~items\n\
         r = .relation~new; r['x'] = 1; r['x'] = 2; say r~allAt('x')~items\n\
         s = .stem~new; s[1] = 'one'; say s[1]\n\
         a = .array~of('b','a','c'); x = ''; do i over a; x = x || i; end; say x\n",
    );
    assert_eq!(out, ["2 one", "b a", "blue 3 1", "2 second", "3 4", "2", "one", "bac"]);
}

#[test]
fn use_arg_and_stems() {
    let out = run_oo(
        "a = .array~new\ncall fill a, 3\nsay a~items a[3]\ns. = 0\ns.1 = 'x'\ncall usestem s.\nsay s.2 s.1\nexit\n\
         fill: procedure\n  use arg arr, n\n  do i = 1 to n\n    arr[i] = 'item'i\n  end\n  return\n\
         usestem: procedure\n  use arg st.\n  st.2 = 'set'\n  return\n",
    );
    assert_eq!(out, ["3 item3", "set x"]);
}

#[test]
fn conditions_oo() {
    let out = run_oo(
        "signal on syntax name trapped\nx = 1 / 0\nsay 'no'\nresume1:\nsay condition('C') rc\nsay catcher()\nexit\n\
         trapped:\n  signal resume1\n\
         catcher: procedure\n  signal on user myerror name gotit\n  call raiser\n  return 'not caught'\ngotit:\n  return 'caught' condition('C')~substr(6)\n\
         raiser: procedure\n  raise user myerror return\n",
    );
    assert_eq!(out, ["SYNTAX 42", "caught MYERROR"]);
}

#[test]
fn messages_and_reply() {
    let out = run_oo(
        "w = .worker~new\nm = w~start('COMPUTE', 6)\nsay m~result\nsay w~early\nmsg = .message~new(w, 'COMPUTE', 'I', 3)\nsay msg~send\n\
         ::class worker\n::method compute\n  use arg n\n  return n * n\n::method early\n  reply 'replied'\n  nop\n",
    );
    assert_eq!(out, ["36", "replied", "9"]);
}

#[test]
fn requires_and_routines() {
    let mut h = MemHost::default();
    h.files.insert(b"REQ.CMD".to_vec(), b"::routine hello public\n  return 'hello' arg(1)\n::class greeter public\n::method greet\n  return 'Hi,' arg(1)\n".to_vec());
    let (out, _) = run_oo_with("say twice(21) hello('rs2') .greeter~new~greet('OS/2')\n::requires 'req.cmd'\n::routine twice\n  return arg(1) * 2\n", &mut h);
    assert_eq!(out, ["42 hello rs2 Hi, OS/2"]);
}

#[test]
fn errors_oo() {
    let out = run_oo("x = .object~new\nx~nope\n");
    assert_eq!(out.len(), 3, "{out:?}");
    assert!(out[1].starts_with("REX0097"), "{out:?}");
    assert!(out[2].starts_with("Error 97.1"), "{out:?}");
}

#[test]
fn orxtest_script() {
    let mut h = MemHost::default();
    h.files.insert(b"ORXREQ.CMD".to_vec(), include_str!("../../../tests/rexx/orxreq.cmd").as_bytes().to_vec());
    let src = include_str!("../../../tests/rexx/orxtest.cmd");
    // the host here has no REXXC or REXXUTIL
    let a = src.find("/* REXXC").unwrap();
    let b = src.find("say 'ORXTEST:'").unwrap();
    let src = alloc::format!("{}{}", &src[..a], &src[b..]);
    let (fails, all) = checks(&mut h, &src);
    // the host here has no REXXC, REXXUTIL or CMD
    let known = ["REXXC", "a tokenized", "REXXUTIL", "commands to CMD", "ORXTEST:", "value() of the environment", "result Ok"];
    let real: Vec<&String> = fails.iter().filter(|l| !known.iter().any(|k| l.contains(k))).collect();
    assert!(real.is_empty(), "{all}");
}
