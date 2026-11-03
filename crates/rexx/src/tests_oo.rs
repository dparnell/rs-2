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
fn uninit() {
    let out = run_oo(
        "a = .res~new('A')\nb = .res~new('B')\ndrop a\nsay 'after drop'\nb = 'x'\nsay 'after assign'\n\
         c = .res~new('C')\nd = .res~new('D'); d~other = d\nsay 'end'\n\
         ::class res\n::method other attribute\n::method init\n  expose n\n  use arg n\n\
         ::method uninit\n  expose n\n  say 'uninit' n\n",
    );
    assert_eq!(out, ["uninit A", "after drop", "uninit B", "after assign", "end", "uninit C", "uninit D"]);
    // UNINIT given by SETMETHOD; an object only referenced from a dropped
    // collection
    let out = run_oo(
        "o = .object~enhanced(.table~new~~put(\"say 'bye'\", 'UNINIT'))\nl = .array~of(o)\ndrop o\nsay 'held'\ndrop l\nsay 'gone'\n",
    );
    assert_eq!(out, ["held", "bye", "gone"]);
}

#[test]
fn redirected_streams() {
    let out = run_oo(
        "say 'one'\nlog = .log~new\n.local['OUTPUT'] = log\nsay 'two'\ncall lineout , 'three'\n.output~lineout('four')\n\
         .local['OUTPUT'] = .stdout\nsay 'lines:' log~lines~items log~lines[1] log~lines[3]\n\
         .local['INPUT'] = .feed~new\npull a\nparse pull b\nparse linein c\nsay a b c linein()\n\
         m = .monitor~new(.stdout)\n.local['OUTPUT'] = m\nsay 'via monitor'\nold = m~destination(log)\nsay 'to log'\n\
         m~destination\n.local['OUTPUT'] = .stdout\nsay log~lines[log~lines~last]\n\
         .local['ERROR'] = log\ntrace r\nx = 1\ntrace o\n.local['ERROR'] = .stderr\nsay log~lines[log~lines~last - 1]~space\n\
         ::class log\n::method lines attribute\n::method init\n  self~lines = .array~new\n\
         ::method say\n  forward message ('LINEOUT')\n::method lineout\n  use arg l\n  self~lines[self~lines~items+1] = l\n  return 0\n\
         ::class feed\n::method linein\n  expose n\n  if symbol('n') <> 'VAR' then n = 0\n  n = n + 1\n  return 'line'n\n",
    );
    assert_eq!(
        out,
        ["one", "lines: 3 two four", "LINE1 line2 line3 line4", "via monitor", "to log", ">>> \"1\""]
    );
}

#[test]
fn image_classes() {
    let out = run_oo(
        "say .stdque~string .stdque~get .stdque~query .rx_queue~id .rx_queue~new~string\n\
         say .stdque~queue('a') .stdque~push('b') .stdque~query queued()\n\
         say .stdque~pull .stdque~linein .stdque~pull~string\n\
         q = .rx_queue~new\nsay q~get q~set('SESSION') q~get\n\
         say .system~version .system~name .system~intname\n\
         say .stream_supplier~id .stream_supplier~superclasses[1]~id\n\
         d = .directory~new\nd~setmethod('X', 'return 7')\nsay d~x\nd~unsetmethod('X')\nsay d~hasindex('X')\n\
         say .stdout~say('said')\n",
    );
    assert_eq!(
        out,
        [
            "SESSION SESSION 0 RX_QUEUE a RX_QUEUE",
            "0 0 2 2",
            "b a The NIL object",
            "NAMED_QUEUE NAMED_QUEUE SESSION",
            "2.30 OS/2 OS/2 Intel",
            "Stream_Supplier Object",
            "7",
            "0",
            "said\r\n",
            "0",
        ]
    );
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

/// The texts of messages are rs2's own: compare what comes before them.
fn no_text(l: &str) -> String {
    if let Some(p) = l.find(" => ") {
        if let Some(q) = l[p..].find(" | ") {
            return alloc::format!("{} | *", &l[..p + q]);
        }
    }
    if l.starts_with("97.1 | ") {
        let parts: Vec<&str> = l.split(" | ").collect();
        return alloc::format!("{} | * | * | {}", parts[0], parts.last().unwrap());
    }
    String::from(l)
}

/// tests/../testdata/probe2.cmd behaves as on IBM's Object REXX
/// (probe2.out: its output there, message texts left out).
#[test]
fn ibm_probe2() {
    let src = include_str!("../testdata/probe2.cmd");
    let want: Vec<&str> = include_str!("../testdata/probe2.out").lines().collect();
    let out = run_oo(src);
    let got: Vec<String> = out.iter().map(|l| no_text(l)).collect();
    let n = want.len() - 2;
    for i in 0..n.min(got.len()) {
        assert_eq!(got[i], want[i], "line {}:\n{}", i + 1, got.join("\n"));
    }
    assert!(got.len() >= n + 2, "{}", got.join("\n"));
    assert!(got[n].starts_with("REX0042") && got[n].contains("line 78"), "{}", got[n]);
    assert!(got[n + 1].starts_with("Error 42.3:"), "{}", got[n + 1]);
}

/// testdata/probe3.cmd: default names, class hierarchy, string methods
/// and traces (TRACE I and R, nested IF, DO and SELECT) as IBM's Object
/// REXX shows them (probe3.out).
#[test]
fn ibm_probe3() {
    let src = include_str!("../testdata/probe3.cmd");
    let want: Vec<String> = include_str!("../testdata/probe3.out").lines().map(|l| String::from(l.trim_end_matches('\r'))).collect();
    let out = run_oo(src);
    for (i, w) in want.iter().enumerate() {
        assert_eq!(out.get(i).map(|s| s.as_str()), Some(w.as_str()), "line {}:\n{}", i + 1, out.join("\n"));
    }
}

/// Activities on threads: START, REPLY going on after the sender,
/// GUARD WHEN waiting for another activity, an alarm.
#[test]
fn activities() {
    let out = crate::tests::run_oo_threads(
        "b = .buffer~new\nm = b~start('TAKE')\nb~put('item')\nsay 'took' m~result\n\
         t = .ticker~new\nsay t~go\ndo i = 1 to 2000; if t~count >= 3 then leave; x = i; end\nsay 'ticks' (t~count >= 3)\n\
         a = .alarm~new(0.05, .message~new(.stdout, 'LINEOUT', 'I', 'alarm!'))\n\
         do i = 1 to 200000; if \\.local~hasindex('NOPE') & i // 1000 = 0 then nop; end\n\
         ::class buffer\n::method init\n  expose item\n  item = .nil\n::method put\n  expose item\n  use arg item\n\
         ::method take\n  expose item\n  guard on when item \\= .nil\n  return item\n\
         ::class ticker\n::method init\n  expose n\n  n = 0\n::method count unguarded\n  expose n\n  return n\n\
         ::method go\n  expose n\n  reply 'going'\n  guard off\n  do 3\n    n = n + 1\n  end\n",
    );
    let all = out.join("|");
    assert!(out.contains(&String::from("took item")), "{all}");
    assert!(out.contains(&String::from("going")), "{all}");
    assert!(out.contains(&String::from("ticks 1")), "{all}");
    assert!(out.iter().any(|l| l.starts_with("alarm!")), "{all}");
}

/// tests/rexx/ORXLANG.CMD (which passes on IBM's Object REXX).
#[test]
fn orxlang_script() {
    let src = include_str!("../../../tests/rexx/orxlang.cmd");
    let out = crate::tests::run_oo_threads(src);
    let all = out.join("\n");
    assert!(out.iter().any(|l| l.starts_with("ORXLANG: ") && l.ends_with(" passed, 0 failed")), "{all}");
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
