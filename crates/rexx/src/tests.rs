//! Interpreter tests on a host kept in memory.

extern crate std;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

use crate::*;

#[derive(Default)]
pub struct MemHost {
    pub out: Vec<String>,
    pub queue: VecDeque<Vec<u8>>,
    pub input: VecDeque<Vec<u8>>,
    pub files: BTreeMap<Vec<u8>, Vec<u8>>,
    pub handles: BTreeMap<u32, (Vec<u8>, u64)>,
    pub next_h: u32,
    pub env: BTreeMap<Vec<u8>, Vec<u8>>,
    pub cmds: Vec<String>,
}

impl MemHost {
    fn line(&mut self, s: &[u8]) {
        self.out.push(String::from_utf8_lossy(s).into_owned());
    }
}

impl Host for MemHost {
    fn say(&mut self, _st: &mut State, line: &[u8]) {
        self.line(line);
    }
    fn trace_out(&mut self, _st: &mut State, line: &[u8]) {
        self.line(line);
    }
    fn pull_terminal(&mut self, _st: &mut State) -> Option<Vec<u8>> {
        self.input.pop_front()
    }
    fn trace_in(&mut self, _st: &mut State) -> Option<Vec<u8>> {
        self.input.pop_front()
    }
    fn command(&mut self, _st: &mut State, env: &[u8], cmd: &[u8]) -> CmdResult {
        self.cmds.push(alloc::format!("{}:{}", String::from_utf8_lossy(env), String::from_utf8_lossy(cmd)));
        if cmd.starts_with(b"fail") {
            return CmdResult { rc: b"-3".to_vec(), cond: 2 };
        }
        if cmd.starts_with(b"err") {
            return CmdResult { rc: b"1".to_vec(), cond: 1 };
        }
        CmdResult { rc: b"0".to_vec(), cond: 0 }
    }
    fn queued(&mut self, _st: &mut State) -> usize {
        self.queue.len()
    }
    fn queue_add(&mut self, _st: &mut State, line: &[u8], lifo: bool) {
        if lifo {
            self.queue.push_front(line.to_vec());
        } else {
            self.queue.push_back(line.to_vec());
        }
    }
    fn queue_pull(&mut self, _st: &mut State) -> Option<Vec<u8>> {
        self.queue.pop_front()
    }
    fn rxqueue(&mut self, _st: &mut State, op: &[u8], _name: Option<&[u8]>) -> Result<Vec<u8>, u32> {
        if op == b"GET" { Ok(b"SESSION".to_vec()) } else { Ok(b"0".to_vec()) }
    }
    fn external(&mut self, _st: &mut State, name: &[u8], args: &[Option<Vec<u8>>], _func: bool) -> External {
        if name == b"EXTFN" {
            return External::Program { source: b"/* */\r\nparse arg a\r\nreturn a*2\r\n".to_vec(), path: b"C:\\EXTFN.CMD".to_vec() };
        }
        if name == b"NATIVE" {
            return External::Value(Some(alloc::format!("{}", args.len()).into_bytes()));
        }
        External::NotFound
    }
    fn rxfunc(&mut self, _op: u8, _name: &[u8], _module: &[u8], _proc: &[u8]) -> u32 {
        0
    }
    fn env_get(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        self.env.get(name).cloned()
    }
    fn env_set(&mut self, name: &[u8], val: Option<&[u8]>) {
        match val {
            Some(v) => {
                self.env.insert(name.to_vec(), v.to_vec());
            }
            None => {
                self.env.remove(name);
            }
        }
    }
    fn directory(&mut self, _new: Option<&[u8]>) -> Option<Vec<u8>> {
        Some(b"C:\\".to_vec())
    }
    fn setlocal(&mut self) -> bool {
        true
    }
    fn endlocal(&mut self) -> bool {
        true
    }
    fn now(&mut self) -> Now {
        Now { year: 2026, month: 11, day: 2, hour: 14, min: 5, sec: 9, micros: 120000 }
    }
    fn halt(&mut self, _st: &mut State) -> bool {
        false
    }
    fn beep(&mut self, _f: u32, _ms: u32) {}
    fn message(&mut self, num: u32) -> Option<Vec<u8>> {
        Some(alloc::format!("message {num}").into_bytes())
    }
    fn report_error(&mut self, _st: &mut State, code: u32, file: &[u8], line: u32) {
        let s = alloc::format!("REX{:04}: Error {} running {}, line {}", code, code, String::from_utf8_lossy(file), line);
        self.out.push(s);
    }
    fn f_open(&mut self, name: &[u8], mode: OpenMode) -> Result<u32, u32> {
        let key = name.to_ascii_uppercase();
        if !self.files.contains_key(&key) {
            if mode == OpenMode::Read {
                return Err(2);
            }
            self.files.insert(key.clone(), Vec::new());
        }
        self.next_h += 1;
        let h = self.next_h + 10;
        self.handles.insert(h, (key, 0));
        Ok(h)
    }
    fn f_read(&mut self, h: u32, buf: &mut [u8]) -> Result<usize, u32> {
        let (k, pos) = self.handles.get(&h).cloned().ok_or(6u32)?;
        let d = &self.files[&k];
        let p = pos as usize;
        let n = buf.len().min(d.len().saturating_sub(p));
        buf[..n].copy_from_slice(&d[p..p + n]);
        self.handles.get_mut(&h).unwrap().1 += n as u64;
        Ok(n)
    }
    fn f_write(&mut self, h: u32, data: &[u8]) -> Result<usize, u32> {
        if h == 1 || h == 2 {
            self.line(data);
            return Ok(data.len());
        }
        let (k, pos) = self.handles.get(&h).cloned().ok_or(6u32)?;
        let d = self.files.get_mut(&k).unwrap();
        let p = pos as usize;
        if d.len() < p + data.len() {
            d.resize(p + data.len(), 0);
        }
        d[p..p + data.len()].copy_from_slice(data);
        self.handles.get_mut(&h).unwrap().1 += data.len() as u64;
        Ok(data.len())
    }
    fn f_seek(&mut self, h: u32, pos: u64) -> Result<(), u32> {
        self.handles.get_mut(&h).ok_or(6u32)?.1 = pos;
        Ok(())
    }
    fn f_size(&mut self, h: u32) -> Option<u64> {
        let (k, _) = self.handles.get(&h)?;
        Some(self.files[k].len() as u64)
    }
    fn f_close(&mut self, h: u32) {
        self.handles.remove(&h);
    }
    fn f_exists(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        let k = name.to_ascii_uppercase();
        if self.files.contains_key(&k) {
            let mut v = b"C:\\".to_vec();
            v.extend_from_slice(&k);
            Some(v)
        } else {
            None
        }
    }
    fn f_info(&mut self, name: &[u8]) -> Option<FileInfo> {
        let k = name.to_ascii_uppercase();
        self.files.get(&k).map(|d| FileInfo { size: d.len() as u64, datetime: b"11-02-26 14:05:09".to_vec() })
    }
}

pub fn run_with(src: &str, h: &mut MemHost) -> (Vec<String>, Result<Option<Vec<u8>>, i32>) {
    let src = src.replace("\r\n", "\n").replace('\n', "\r\n");
    let r = {
        let mut it = Interp::new(h);
        it.run_program(src.as_bytes(), b"C:\\T.CMD", CallType::Command, Vec::new(), b"CMD")
    };
    (core::mem::take(&mut h.out), r)
}

pub fn run(src: &str) -> Vec<String> {
    let mut h = MemHost::default();
    run_with(src, &mut h).0
}

fn say(expr: &str) -> String {
    let out = run(&alloc::format!("say {expr}\n"));
    out.join("|")
}

#[test]
fn arithmetic() {
    assert_eq!(say("1/3 2/3 10/4 1e3 1.50+1 2**-1 0.1*3 1e9*10 123456789*10 1/7*7"),
        "0.333333333 0.666666667 2.5 1E3 2.50 0.5 0.3 1.0E+10 1.23456789E+9 1.00000000");
    assert_eq!(say("12345678901 + 0"), "1.23456789E+10");
    assert_eq!(say("0.000001 0.0000001+0 1e-10+0 '-0'+0 ' 3 '+0"), "0.000001 0.0000001 0.0000000001 0 3");
    assert_eq!(say("(5//-2) (-5//2) (7%2) (-7%2) 2**31 1.1**3"), "1 -1 3 -3 2.14748365E+9 1.331");
    assert_eq!(run("numeric digits 20; say 1/3 2**64").join(""), "0.33333333333333333333 18446744073709551616");
    assert_eq!(say("format(3.14159,,2) format(12345.678,,,3) format(1.5e10) format(0.000123,,,,2)"), "3.14 12345.678 1.5E+10 1.23E-4");
    assert_eq!(say("trunc(12.345,2) datatype('12') datatype(' 1e5 ', 'N') datatype('ab','x')"), "12.34 NUM 1 1");
    assert_eq!(say("1+1 = 2  '1.0' = 1 'a' = ' a ' 'a' == ' a '"), "0");
    assert_eq!(say("('1.0' = 1) ('a' = ' a ') ('a' == ' a ')"), "1 1 0");
}

#[test]
fn trace_like_ibm() {
    let out = run("trace r\nx = 1 + 2\nsay x\ntrace i\ny = x * 2 + length('abc')\ntrace o\n");
    assert_eq!(out[0], "     2 *-*   x = 1 + 2;");
    assert_eq!(out[1], "       >>>     \"3\"");
    assert_eq!(out[2], "     3 *-*   Say x;");
    let i = out.iter().position(|l| l.contains("y = x")).unwrap();
    assert_eq!(&out[i + 1..i + 8], &["       >V>     \"3\"", "       >L>     \"2\"", "       >O>     \"6\"", "       >L>     \"abc\"",
        "       >F>     \"3\"", "       >O>     \"9\"", "       >>>     \"9\""]);
}

#[test]
fn control() {
    assert_eq!(run("do i = 1 to 3; say i; end").join(","), "1,2,3");
    assert_eq!(run("do i = 3 to 1 by -1; say i; end; say i").join(","), "3,2,1,0");
    assert_eq!(run("do 2; say 'x'; end").join(","), "x,x");
    assert_eq!(run("i=0; do forever; i=i+1; if i>2 then leave; end; say i").join(","), "3");
    assert_eq!(run("do i=1 to 5; if i//2 then iterate; say i; end").join(","), "2,4");
    assert_eq!(run("do i=1 for 3 until i=2; say i; end").join(","), "1,2");
    assert_eq!(run("x=2; select; when x=1 then say 'a'; when x=2 then say 'b'; otherwise say 'c'; end").join(","), "b");
    assert_eq!(run("if 1 then say 'y'; else say 'n'\nif 0 then say 'y'; else say 'n'").join(","), "y,n");
    assert_eq!(run("do i=1 to 2; do j=1 to 3; if j=2 then leave i; say i j; end; end").join(","), "1 1");
}

#[test]
fn routines() {
    assert_eq!(run("say fact(10)\nexit\nfact: procedure; parse arg n; if n<2 then return 1; return n*fact(n-1)").join(","), "3628800");
    assert_eq!(run("a=1; call s; say a b\nexit\ns: procedure expose a; a=2; b=3; return").join(","), "2 B");
    assert_eq!(run("call s 1,,3; say result\nexit\ns: return arg() arg(2,'o') arg(3)").join(","), "3 1 3");
    assert_eq!(run("say extfn(21) native(1,2)").join(","), "42 2");
    assert_eq!(run("say nosuch(1)").last().unwrap(), "REX0043: Error 43 running C:\\T.CMD, line 1");
}

#[test]
fn parsing() {
    assert_eq!(run("parse value 'a b c' with q r . 1 s +1; say q'|'r'|'s").join(","), "a|b|a");
    assert_eq!(run("parse value '  a   b  c ' with p q; say '['p']['q']'").join(","), "[a][b  c ]");
    assert_eq!(run("parse value 'k=v;z' with k '=' v ';' z; say k v z").join(","), "k v z");
    assert_eq!(run("s='abc'; parse var s 2 x +1; say x").join(","), "b");
    assert_eq!(run("d=','; parse value 'a,b' with x (d) y; say x y").join(","), "a b");
    assert_eq!(run("push 'one two'; pull a b; say a b").join(","), "ONE TWO");
}

#[test]
fn conditions() {
    assert_eq!(run("signal on syntax\nsay 1 + ''\nexit\nsyntax: say 'syn' rc sigl condition('C')").join(","), "syn 41 2 SYNTAX");
    assert_eq!(run("signal on novalue\nsay zz\nexit\nnovalue: say condition('D') sigl").join(","), "ZZ 2");
    assert_eq!(run("call on error\n'err'\nsay 'after' rc\nexit\nerror: say 'handler' condition('D') condition('I'); return").join(","),
        "handler err CALL,after 1");
    assert_eq!(run("signal on failure\n'fail'\nexit\nfailure: say 'f' rc").join(","), "     2 *-*   'fail';,       +++     RC(-3),f -3");
    let out = run("call lvl1\nexit\nlvl1: call lvl2\nreturn\nlvl2:\n  do i = 1 to 1\n    x = 1 +\n  end\n");
    assert_eq!(out[0], "     7 +++         x = 1 +;");
    assert_eq!(out[1], "REX0035: Error 35 running C:\\T.CMD, line 7");
    assert_eq!(out[2], "     3 +++     Call lvl2;");
    assert_eq!(out[3], "     1 +++   Call lvl1;");
}

#[test]
fn builtins() {
    assert_eq!(say("substr('abc',2) substr('abc',2,4,'.') left('ab',3,'*') right('abc',2) center('ab',6,'-')"), "bc bc.. ab* bc --ab--");
    assert_eq!(say("pos('b','abcb') pos('b','abcb',3) lastpos('b','abcb') words(' a b  c ') word('a b c',2)"), "2 4 4 3 b");
    assert_eq!(say("c2x('AB') x2c('4142') d2x(255) x2d('ff') d2c(65) c2d('A') x2d('81',2) d2x(-1,4)"), "4142 AB FF 255 A 65 -127 FFFF");
    assert_eq!(say("translate('abc') translate('abc','xy','ab') reverse('xyz') copies('ab',3) space(' a  b ',2,'-')"), "ABC xyc zyx ababab a--b");
    assert_eq!(say("strip('  a  ') strip('xxaxx',,'x') strip('  a  ','l')'|' verify('abc','ab') verify('abc','c','m')"), "a a a  | 3 3");
    assert_eq!(say("delstr('abcdef',2,3) delword('a b c d',2,2) subword('a b c d',2,2) insert('x','abc',1) overlay('x','abc',2)"), "aef a d b c axbc axc");
    assert_eq!(say("abbrev('PRINT','PRI') abbrev('PRINT','PRX') compare('abc','abd') max(1,5,3) min(2,-1) sign(-3) abs(-2.50)"), "1 0 3 5 -1 -1 2.50");
    assert_eq!(say("date('B') date('W') date('S') date('N') date('O') date('E') date('U') date('D')"),
        "739921 Monday 20261102 2 Nov 2026 26/11/02 02/11/26 11/02/26 306");
    assert_eq!(say("time() time('H') time('M') time('C')"), "14:05:09 14 845 2:05pm");
    assert_eq!(say("x2b('0f') b2x('1111 0000') bitand('ab','  ') bitor('a',' ') bitxor('A',' ')"), "00001111 F0    a a");
    assert_eq!(say("symbol('x') symbol('1a') symbol('a+') value('x') wordpos('b c','a b c') wordindex('a  b',2) wordlength('a bcd',2)"), "LIT LIT BAD X 2 4 3");
    assert_eq!(say("length(xrange('a','e')) filespec('d','C:\\x\\y.z') filespec('p','C:\\x\\y.z') filespec('n','C:\\x\\y.z')"), "5 C: \\x\\ y.z");
}

#[test]
fn streams_and_queue() {
    let out = run("f='T.TXT'\ncall lineout f, 'one'\ncall lineout f, 'two'\ncall lineout f\ndo while lines(f) > 0; say linein(f); end\nsay stream(f,'c','query size') chars(f)\nqueue 'q1'; push 'q0'; say queued() linein('QUEUE:')");
    assert_eq!(out.join(","), "one,two,10 0,2 q0");
}

#[test]
fn interpret_and_signal() {
    assert_eq!(run("interpret 'x = 5; say x*2'").join(","), "10");
    assert_eq!(run("do i=1 to 3; interpret 'if i=2 then leave'; say i; end").join(","), "1");
    assert_eq!(run("signal there\nsay 'no'\nthere: say 'yes' sigl").join(","), "yes 1");
    assert_eq!(run("a.=0; a.1='x'; i=1; say a.i a.2 a.\ndrop a.1; say a.1").join(","), "x 0 0,A.1");
    assert_eq!(run("x='y'; y.='d'; y.x=1; say y.x y.y").join(","), "1 d");
    assert_eq!(run("say value('os2var',,'OS2ENVIRONMENT')'|'; call value 'os2var','v','OS2ENVIRONMENT'; say value('os2var',,'OS2ENVIRONMENT')").join(","), "|,v");
}
