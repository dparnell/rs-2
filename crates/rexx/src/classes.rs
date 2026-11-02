//! The methods of Object REXX's built-in classes (.Object, .Class,
//! .String, the collections, .Stem, .Supplier, .Message, .Method,
//! .Monitor, .Stream, .Alarm), and the secondary error messages.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::Err;
use crate::exec::*;
use crate::lex::upper;
use crate::num::{Num, int_str};
use crate::oo::*;
use crate::value::*;

type Args = Vec<Option<Value>>;
type RV = R<Option<Value>>;

const OBJECT_M: &[&[u8]] = &[
    b"CLASS", b"COPY", b"DEFAULTNAME", b"HASMETHOD", b"INIT", b"OBJECTNAME", b"OBJECTNAME=", b"REQUEST", b"RUN", b"SETMETHOD", b"START",
    b"STRING", b"UNSETMETHOD", b"=", b"==", b"\\=", b"\\==", b"<>", b"><",
];
const CLASS_M: &[&[u8]] = &[
    b"BASECLASS", b"DEFAULTNAME", b"DEFINE", b"DELETE", b"ENHANCED", b"ID", b"INHERIT", b"INIT", b"METACLASS", b"METHOD", b"METHODS",
    b"MIXINCLASS", b"NEW", b"QUERYMIXINCLASS", b"SUBCLASS", b"SUBCLASSES", b"SUPERCLASSES", b"UNINHERIT",
];
const STRING_M: &[&[u8]] = &[
    b"+", b"-", b"*", b"/", b"%", b"//", b"**", b"||", b" ", b"", b"=", b"\\=", b"==", b"\\==", b">", b"<", b">=", b"<=", b">>", b"<<",
    b">>=", b"<<=", b"\\<", b"\\>", b"\\<<", b"\\>>", b"<>", b"><", b"&", b"|", b"&&", b"\\", b"ABBREV", b"ABS", b"BITAND", b"BITOR",
    b"BITXOR", b"B2X", b"CENTER", b"CENTRE", b"CHANGESTR", b"COMPARE", b"COPIES", b"COUNTSTR", b"C2D", b"C2X", b"DATATYPE", b"DELSTR",
    b"DELWORD", b"D2C", b"D2X", b"FORMAT", b"INIT", b"INSERT", b"LASTPOS", b"LEFT", b"LENGTH", b"MAKESTRING", b"MAX", b"MIN", b"OVERLAY",
    b"POS", b"REVERSE", b"RIGHT", b"SIGN", b"SPACE", b"STRING", b"STRIP", b"SUBSTR", b"SUBWORD", b"TRANSLATE", b"TRUNC", b"VERIFY",
    b"WORD", b"WORDINDEX", b"WORDLENGTH", b"WORDPOS", b"WORDS", b"X2B", b"X2C", b"X2D",
];
const ARRAY_M: &[&[u8]] = &[
    b"INIT", b"[]", b"[]=", b"AT", b"DIMENSION", b"FIRST", b"HASINDEX", b"ITEMS", b"LAST", b"MAKEARRAY", b"NEXT", b"PREVIOUS", b"PUT",
    b"REMOVE", b"SECTION", b"SIZE", b"SUPPLIER",
];
const LIST_M: &[&[u8]] = &[
    b"INIT", b"[]", b"[]=", b"AT", b"FIRST", b"FIRSTITEM", b"HASINDEX", b"INSERT", b"ITEMS", b"LAST", b"LASTITEM", b"MAKEARRAY", b"NEXT",
    b"PREVIOUS", b"PUT", b"REMOVE", b"SECTION", b"SUPPLIER",
];
const QUEUE_M: &[&[u8]] = &[
    b"INIT", b"[]", b"[]=", b"AT", b"HASINDEX", b"ITEMS", b"MAKEARRAY", b"PEEK", b"PULL", b"PUSH", b"PUT", b"QUEUE", b"REMOVE", b"SUPPLIER",
];
const DIR_M: &[&[u8]] = &[
    b"INIT", b"[]", b"[]=", b"AT", b"DIFFERENCE", b"ENTRY", b"HASENTRY", b"HASINDEX", b"INTERSECTION", b"ITEMS", b"MAKEARRAY", b"PUT",
    b"REMOVE", b"SETENTRY", b"SETMETHOD", b"SUBSET", b"SUPPLIER", b"UNION", b"UNKNOWN", b"XOR",
];
const TABLE_M: &[&[u8]] = &[
    b"INIT", b"[]", b"[]=", b"AT", b"DIFFERENCE", b"HASINDEX", b"INTERSECTION", b"ITEMS", b"MAKEARRAY", b"PUT", b"REMOVE", b"SUBSET",
    b"SUPPLIER", b"UNION", b"XOR",
];
const REL_M: &[&[u8]] = &[
    b"INIT", b"[]", b"[]=", b"AT", b"ALLAT", b"ALLINDEX", b"DIFFERENCE", b"HASINDEX", b"HASITEM", b"INDEX", b"INTERSECTION", b"ITEMS",
    b"MAKEARRAY", b"PUT", b"REMOVE", b"REMOVEITEM", b"SUBSET", b"SUPPLIER", b"UNION", b"XOR",
];
const SETBAG_M: &[&[u8]] = &[b"[]", b"[]=", b"AT", b"HASINDEX", b"ITEMS", b"MAKEARRAY", b"PUT", b"REMOVE", b"SUPPLIER"];
const STEM_M: &[&[u8]] = &[b"INIT", b"[]", b"[]=", b"MAKEARRAY", b"REQUEST", b"UNKNOWN"];
const SUPPLIER_M: &[&[u8]] = &[b"INIT", b"AVAILABLE", b"INDEX", b"ITEM", b"NEXT"];
const MESSAGE_M: &[&[u8]] = &[b"INIT", b"COMPLETED", b"NOTIFY", b"RESULT", b"SEND", b"START"];
const METHOD_M: &[&[u8]] = &[b"INIT", b"SETGUARDED", b"SETPRIVATE", b"SETPROTECTED", b"SETUNGUARDED", b"SETUNPROTECTED", b"SOURCE"];
const MONITOR_M: &[&[u8]] = &[b"INIT", b"CURRENT", b"DESTINATION", b"UNKNOWN"];
const STREAM_M: &[&[u8]] = &[
    b"INIT", b"ARRAYIN", b"ARRAYOUT", b"CHARIN", b"CHAROUT", b"CHARS", b"CLOSE", b"COMMAND", b"DESCRIPTION", b"FLUSH", b"LINEIN",
    b"LINEOUT", b"LINES", b"MAKEARRAY", b"OPEN", b"POSITION", b"QUALIFY", b"QUERY", b"SEEK", b"STATE", b"SUPPLIER",
];
const ALARM_M: &[&[u8]] = &[b"INIT", b"CANCEL"];

/// The methods a built-in class defines (`side`: class methods).
pub fn natives(b: Builtin, side: bool) -> &'static [&'static [u8]] {
    if side {
        return match b {
            Builtin::Array | Builtin::List | Builtin::Set | Builtin::Bag | Builtin::Queue => &[b"OF"],
            _ => &[],
        };
    }
    match b {
        Builtin::Object => OBJECT_M,
        Builtin::Class => CLASS_M,
        Builtin::String => STRING_M,
        Builtin::Array => ARRAY_M,
        Builtin::List => LIST_M,
        Builtin::Queue => QUEUE_M,
        Builtin::Directory => DIR_M,
        Builtin::Table => TABLE_M,
        Builtin::Relation => REL_M,
        Builtin::Set | Builtin::Bag => SETBAG_M,
        Builtin::Stem => STEM_M,
        Builtin::Supplier => SUPPLIER_M,
        Builtin::Message => MESSAGE_M,
        Builtin::Method => METHOD_M,
        Builtin::Monitor => MONITOR_M,
        Builtin::Stream => STREAM_M,
        Builtin::Alarm => ALARM_M,
    }
}

pub fn has_native(b: Builtin, side: bool, name: &[u8]) -> bool {
    natives(b, side).contains(&name)
}

/// The secondary messages of the errors Object REXX raises (rs2's
/// wording), by code (major * 1000 + minor).
pub fn secondary(code: u32) -> Option<&'static [u8]> {
    if let Some(t) = crate::msgs::secondary(code) {
        return Some(t);
    }
    Some(match code {
        40001 => b"The external routine \"&1\" failed",
        40003 => b"&1 needs at least &2 arguments",
        40004 => b"&1 takes at most &2 arguments",
        40005 => b"&1 needs argument &2",
        41001 => b"\"&1\" is not a number and cannot be used in arithmetic",
        42003 => b"A number cannot be divided by zero",
        43001 => b"There is no routine called \"&1\"",
        43901 => b"There is no program \"&1\" for ::REQUIRES",
        44001 => b"The function \"&1\" returned no data",
        90997 => b"The external class \"&1\" was not found",
        91999 => b"The message \"&1\" gave no result",
        93901 => b"The method needs at least &1 arguments",
        93902 => b"The method takes at most &1 arguments",
        93903 => b"The method needs argument &1",
        93904 => b"Argument &1 of the method must be a number, not \"&2\"",
        93905 => b"Argument &1 of the method must be a whole number, not \"&2\"",
        93906 => b"Argument &1 of the method must be zero or a positive whole number, not \"&2\"",
        93907 => b"Argument &1 of the method must be a positive whole number, not \"&2\"",
        93914 => b"Argument &1 of the method must be one of &2, not \"&3\"",
        93918 => b"\"&1\" is not an index of this list",
        93919 => b"\"&1\" is not a position in this array",
        93925 => b"The array needs &1 subscripts",
        93926 => b"The array takes only &1 subscripts",
        93937 => b"The supplier has no more items",
        93938 => b"Argument &1 of the method must have a string value",
        93939 => b"Argument &1 of the method must be an array",
        93944 => b"Argument &1 of the method must be a message object",
        93948 => b"Argument &1 of the method must be an instance of the &2 class",
        93949 => b"An index-only collection needs the same object as index and item",
        93951 => b"\"&1\" is not a time an alarm can be set for",
        97001 => b"The object \"&1\" has no method for the message \"&2\"",
        98905 => b"A guarded method waits for something no other activity can change",
        98909 => b"There is no class \"&1\"",
        98913 => b"The object \"&1\" cannot be made an array",
        98914 => b"The object \"&1\" has no string value",
        98915 => b"A message object can be sent or started only once",
        98918 => b"There is no trapped condition to propagate",
        98919 => b"The object \"&1\" cannot be made a method",
        98935 => b"A method can REPLY only once",
        98936 => b"RETURN cannot give a result after REPLY",
        98937 => b"EXIT cannot give a result after REPLY",
        98941 => b"\"&1\" is not an error number RAISE SYNTAX knows",
        98942 => b"The class \"&1\" is not a mixin class and cannot be inherited",
        98944 => b"The class \"&1\" cannot inherit itself, a superclass or a subclass (\"&2\")",
        98945 => b"The class \"&1\" did not inherit the class \"&2\"",
        98947 => b"FORWARD can be used only in a method",
        99901 => b"There is already a ::CLASS directive for this class",
        99902 => b"There is already a ::METHOD directive for this method",
        99903 => b"There is already a ::ROUTINE directive for this routine",
        99905 => b"::METHOD with CLASS needs a ::CLASS directive before it",
        99907 => b"EXPOSE must be the first instruction of a method",
        99911 => b"GUARD can be used only in a method",
        99914 => b"INTERPRET cannot run directives",
        99916 => b"The directive is not known",
        99919 => b"REPLY can be used only in a method",
        99922 => b"::REQUIRES must come before the other directives",
        _ => return None,
    })
}

fn nil(it: &Interp) -> Value {
    it.w.as_ref().unwrap().nil()
}

fn bool_v(b: bool) -> Value {
    Value::S(if b { b"1".to_vec() } else { b"0".to_vec() })
}

fn err93(it: &mut Interp, minor: u32, subs: &[&[u8]]) -> Err {
    if let Some(t) = secondary(93000 + minor) {
        it.err_msg = msg_subst(t, subs);
    }
    Err::Syntax(93000 + minor)
}

fn arg(args: &Args, i: usize) -> Option<Value> {
    args.get(i).cloned().flatten()
}

fn need(it: &mut Interp, args: &Args, i: usize) -> R<Value> {
    match arg(args, i) {
        Some(v) => Ok(v),
        None => {
            let n = int_str(i as i64 + 1);
            Err(err93(it, 903, &[&n]))
        }
    }
}

fn max_args(it: &mut Interp, args: &Args, n: usize) -> R<()> {
    if args.len() > n {
        let s = int_str(n as i64);
        return Err(err93(it, 902, &[&s]));
    }
    Ok(())
}

fn str_arg(it: &mut Interp, f: &mut Frame, args: &Args, i: usize) -> R<Val> {
    let v = need(it, args, i)?;
    it.to_str(f, &v)
}

fn whole_arg(it: &mut Interp, f: &mut Frame, args: &Args, i: usize) -> R<i64> {
    let s = str_arg(it, f, args, i)?;
    match Num::parse(&s).and_then(|n| n.to_whole(9)) {
        Some(n) => Ok(n),
        None => {
            let k = int_str(i as i64 + 1);
            Err(err93(it, 905, &[&k, &s]))
        }
    }
}

fn pos_arg(it: &mut Interp, f: &mut Frame, args: &Args, i: usize) -> R<usize> {
    let n = whole_arg(it, f, args, i)?;
    if n < 1 {
        let k = int_str(i as i64 + 1);
        let s = int_str(n);
        return Err(err93(it, 907, &[&k, &s]));
    }
    Ok(n as usize)
}

fn obj_of(v: &Value) -> Option<&ObjRef> {
    v.obj()
}

/// The built-in class whose instance data an object has.
pub fn kind_of(o: &ObjRef) -> Option<Builtin> {
    let c = o.class.borrow().clone()?;
    for k in mro(&c) {
        if let Some(b) = class_info(&k, |ci| ci.builtin).flatten() {
            if b != Builtin::Object {
                return Some(b);
            }
        }
    }
    Some(Builtin::Object)
}

/// New instance data for a class.
fn new_data(b: Option<Builtin>) -> Data {
    match b {
        Some(Builtin::Array) => Data::Array(ArrayData::default()),
        Some(Builtin::List) => Data::List(ListData::default()),
        Some(Builtin::Queue) => Data::Queue(VecDeque::new()),
        Some(Builtin::Directory) => Data::Directory(DirData::default()),
        Some(Builtin::Table | Builtin::Relation | Builtin::Set | Builtin::Bag) => Data::Coll(BTreeMap::new()),
        Some(Builtin::Stem) => Data::Stem(Rc::new(RefCell::new(crate::vars::Stem::default())), Vec::new()),
        Some(Builtin::Supplier) => Data::Supplier(SupplierData { items: Vec::new(), indexes: Vec::new(), pos: 0 }),
        Some(Builtin::Message) => Data::Message(alloc::boxed::Box::new(MsgData {
            target: Value::S(Vec::new()),
            name: Vec::new(),
            scope: None,
            args: Vec::new(),
            state: 0,
            result: None,
            notify: Vec::new(),
        })),
        Some(Builtin::Method) => Data::Method(Method { body: Body::Null, private: false, guarded: true, protected: false }),
        Some(Builtin::Monitor) => Data::Monitor(Value::S(Vec::new())),
        Some(Builtin::Stream) => Data::Stream(StreamData::default()),
        Some(Builtin::Alarm) => Data::Alarm(AlarmData { msg: Value::S(Vec::new()), cancelled: false }),
        Some(Builtin::String) => Data::Str(Vec::new()),
        _ => Data::Plain,
    }
}

fn class_builtin(c: &ObjRef) -> Option<Builtin> {
    for k in mro(c) {
        if let Some(b) = class_info(&k, |ci| ci.builtin).flatten() {
            if b != Builtin::Object {
                return Some(b);
            }
        }
    }
    None
}

/// Run a built-in method.
#[allow(clippy::too_many_arguments)]
pub fn native(it: &mut Interp, f: &mut Frame, b: Builtin, side: bool, recv: &Value, name: &[u8], args: Args, found: &Found, lvl: u32) -> RV {
    if side {
        return class_side(it, f, b, recv, name, args);
    }
    match b {
        Builtin::Object => object_m(it, f, recv, name, args, lvl),
        Builtin::Class => class_m(it, f, recv, name, args, lvl),
        Builtin::String => string_m(it, f, recv, name, args),
        Builtin::Array => array_m(it, f, recv, name, args),
        Builtin::List => list_m(it, f, recv, name, args),
        Builtin::Queue => queue_m(it, f, recv, name, args),
        Builtin::Directory => dir_m(it, f, recv, name, args, lvl),
        Builtin::Table | Builtin::Relation | Builtin::Set | Builtin::Bag => coll_m(it, f, recv, name, args),
        Builtin::Stem => stem_m(it, f, recv, name, args, lvl),
        Builtin::Supplier => supplier_m(it, f, recv, name, args),
        Builtin::Message => message_m(it, f, recv, name, args, lvl),
        Builtin::Method => method_m(it, f, recv, name, args),
        Builtin::Monitor => monitor_m(it, f, recv, name, args, found, lvl),
        Builtin::Stream => stream_m(it, f, recv, name, args),
        Builtin::Alarm => alarm_m(it, f, recv, name, args, lvl),
    }
}

fn class_side(it: &mut Interp, f: &mut Frame, b: Builtin, recv: &Value, name: &[u8], args: Args) -> RV {
    let _ = f;
    let w = it.w.clone().unwrap();
    let Some(cls) = recv.obj().cloned() else { return Ok(None) };
    match name {
        b"OF" => {
            let o = w.new_obj(&cls, new_data(Some(b)));
            match &mut *o.data.borrow_mut() {
                Data::Array(a) => a.items = args,
                Data::List(l) => {
                    for v in args.into_iter().flatten() {
                        l.items.push_back((l.next, v));
                        l.next += 1;
                    }
                }
                Data::Queue(q) => q.extend(args.into_iter().flatten()),
                Data::Coll(m) => {
                    for v in args.into_iter().flatten() {
                        let e = m.entry(v.key()).or_default();
                        if b == Builtin::Set {
                            e.clear();
                        }
                        e.push((v.clone(), v));
                    }
                }
                _ => {}
            }
            Ok(Some(Value::O(o)))
        }
        _ => Ok(None),
    }
}

// ---- .Object ----

fn object_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, lvl: u32) -> RV {
    let w = it.w.clone().unwrap();
    match name {
        b"INIT" => Ok(None),
        b"CLASS" => Ok(Some(match recv {
            Value::S(_) => Value::O(w.class(Builtin::String)),
            Value::O(o) => Value::O(o.class()),
        })),
        b"=" | b"==" => Ok(Some(bool_v(arg(&args, 0).is_some_and(|a| a.same(recv))))),
        b"\\=" | b"\\==" | b"<>" | b"><" => Ok(Some(bool_v(!arg(&args, 0).is_some_and(|a| a.same(recv))))),
        b"STRING" | b"DEFAULTNAME" | b"OBJECTNAME" => Ok(Some(Value::S(match recv {
            Value::S(s) => s.clone(),
            Value::O(o) => {
                if name == b"DEFAULTNAME" {
                    o.default_name()
                } else {
                    o.fast_string()
                }
            }
        }))),
        b"OBJECTNAME=" => {
            let s = str_arg(it, f, &args, 0)?;
            if let Value::O(o) = recv {
                *o.name.borrow_mut() = Some(s);
            }
            Ok(None)
        }
        b"HASMETHOD" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            Ok(Some(bool_v(lookup(&w, recv, &n, &Start::Normal).is_some())))
        }
        b"REQUEST" => {
            let c = upper(&str_arg(it, f, &args, 0)?);
            match c.as_slice() {
                b"STRING" => {
                    if let Value::S(_) = recv {
                        return Ok(Some(recv.clone()));
                    }
                    let mk = lookup(&w, recv, b"MAKESTRING", &Start::Normal);
                    match mk {
                        Some(_) => it.send(f, recv, b"MAKESTRING", Vec::new(), Start::Normal, lvl),
                        None => Ok(Some(nil(it))),
                    }
                }
                b"ARRAY" => match lookup(&w, recv, b"MAKEARRAY", &Start::Normal) {
                    Some(_) => it.send(f, recv, b"MAKEARRAY", Vec::new(), Start::Normal, lvl),
                    None => Ok(Some(nil(it))),
                },
                _ => {
                    let cls = match recv {
                        Value::S(_) => w.class(Builtin::String),
                        Value::O(o) => o.class(),
                    };
                    if mro(&cls).iter().any(|k| upper(&k.class_id()) == c) {
                        Ok(Some(recv.clone()))
                    } else {
                        Ok(Some(nil(it)))
                    }
                }
            }
        }
        b"COPY" => match recv {
            Value::S(_) => Ok(Some(recv.clone())),
            Value::O(o) => {
                let data = copy_data(&o.data.borrow());
                let n = w.new_obj(&o.class(), data);
                *n.methods.borrow_mut() = o.methods.borrow().clone();
                let vars: Vec<(u64, crate::vars::PoolRef)> = o
                    .vars
                    .borrow()
                    .iter()
                    .map(|(k, p)| {
                        let src = p.borrow();
                        let np = crate::vars::Pool { vars: src.vars.clone(), stems: src.stems.clone(), exposed: Default::default(), parent: None };
                        (*k, Rc::new(RefCell::new(np)))
                    })
                    .collect();
                *n.vars.borrow_mut() = vars;
                Ok(Some(Value::O(n)))
            }
        },
        b"SETMETHOD" => {
            let Value::O(o) = recv else { return Ok(None) };
            let n = upper(&str_arg(it, f, &args, 0)?);
            let m = match arg(&args, 1) {
                Some(src) => to_method(it, f, &n, &src)?,
                None => Method { body: Body::Null, private: false, guarded: true, protected: false },
            };
            o.methods.borrow_mut().get_or_insert_with(BTreeMap::new).insert(n, m);
            Ok(None)
        }
        b"UNSETMETHOD" => {
            let Value::O(o) = recv else { return Ok(None) };
            let n = upper(&str_arg(it, f, &args, 0)?);
            if let Some(t) = o.methods.borrow_mut().as_mut() {
                t.remove(&n);
            }
            Ok(None)
        }
        b"RUN" => {
            let mv = need(it, &args, 0)?;
            let m = to_method(it, f, b"RUN", &mv)?;
            let rest = method_args(it, f, &args, 1)?;
            let found = Found { m, scope: None, class_side: false };
            invoke(it, f, recv, b"RUN", rest, found, lvl)
        }
        b"START" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            let rest: Args = args.into_iter().skip(1).collect();
            let md = MsgData { target: recv.clone(), name: n, scope: None, args: rest, state: 0, result: None, notify: Vec::new() };
            let m = w.obj(Builtin::Message, Data::Message(alloc::boxed::Box::new(md)));
            start_message(it, f, &m, lvl)?;
            Ok(Some(m))
        }
        _ => Ok(None),
    }
}

fn copy_data(d: &Data) -> Data {
    match d {
        Data::Plain => Data::Plain,
        Data::Nil => Data::Nil,
        Data::Str(s) => Data::Str(s.clone()),
        Data::Array(a) => Data::Array(a.clone()),
        Data::List(l) => Data::List(ListData { items: l.items.clone(), next: l.next }),
        Data::Queue(q) => Data::Queue(q.clone()),
        Data::Directory(dd) => Data::Directory(DirData { map: dd.map.clone(), methods: dd.methods.clone(), unknown: dd.unknown.clone() }),
        Data::Coll(m) => Data::Coll(m.clone()),
        Data::Stem(s, n) => {
            let src = s.borrow();
            Data::Stem(Rc::new(RefCell::new(crate::vars::Stem { default: src.default.clone(), tails: src.tails.clone() })), n.clone())
        }
        Data::Supplier(s) => Data::Supplier(SupplierData { items: s.items.clone(), indexes: s.indexes.clone(), pos: s.pos }),
        Data::Method(m) => Data::Method(m.clone()),
        Data::Monitor(v) => Data::Monitor(v.clone()),
        Data::Stream(s) => Data::Stream(StreamData { name: s.name.clone() }),
        Data::Ext(k, h) => Data::Ext(*k, *h),
        _ => Data::Plain,
    }
}

/// `'I'` then arguments, or `'A'` then an array of them (RUN, Message
/// INIT).
fn method_args(it: &mut Interp, f: &mut Frame, args: &Args, from: usize) -> R<Args> {
    let Some(o) = arg(args, from) else { return Ok(Vec::new()) };
    let o = upper(&it.to_str(f, &o)?);
    match o.first() {
        Some(b'A') => {
            let a = need(it, args, from + 1)?;
            Ok(array_items(&a).unwrap_or_default())
        }
        _ => Ok(args.iter().skip(from + 1).cloned().collect()),
    }
}

fn array_items(v: &Value) -> Option<Args> {
    match &*v.obj()?.data.borrow() {
        Data::Array(a) => Some(a.items.clone()),
        _ => None,
    }
}

/// A method object, or source code (a string or an array of lines) made
/// one.
pub fn to_method(it: &mut Interp, f: &mut Frame, name: &[u8], v: &Value) -> R<Method> {
    if let Value::O(o) = v {
        if let Data::Method(m) = &*o.data.borrow() {
            return Ok(m.clone());
        }
    }
    let lines: Vec<Val> = match v {
        Value::S(s) => alloc::vec![s.clone()],
        Value::O(o) => match &*o.data.borrow() {
            Data::Array(a) => a.items.iter().map(|x| x.as_ref().map(|v| v.fast_string()).unwrap_or_default()).collect(),
            Data::Str(s) => alloc::vec![s.clone()],
            _ => {
                let s = o.fast_string();
                it.err_msg = msg_subst(secondary(98919).unwrap(), &[&s]);
                return Err(Err::Syntax(98919));
            }
        },
    };
    let mut src = Vec::new();
    for l in &lines {
        src.extend_from_slice(l);
        src.extend_from_slice(b"\r\n");
    }
    let prog = match crate::parse::program_mode(&src, true) {
        Ok(p) => Rc::new(p),
        Err((e, _)) => return Err(e),
    };
    let pkg = match &f.pkg {
        Some(p) => p.clone(),
        None => crate::oo::load_package(b"", b"").map_err(|e| e.0)?,
    };
    let code = Rc::new(Code { prog, pkg, name: name.to_vec(), source: lines });
    Ok(Method::rexx(code))
}

// ---- .Class ----

fn class_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, lvl: u32) -> RV {
    let w = it.w.clone().unwrap();
    let Some(cls) = recv.obj().cloned().filter(|o| o.is_class()) else { return Ok(None) };
    match name {
        b"INIT" => Ok(None),
        b"ID" => Ok(Some(Value::S(cls.class_id()))),
        b"DEFAULTNAME" => Ok(Some(Value::S(cls.default_name()))),
        b"NEW" => {
            // an external class: the other system makes the object
            for k in mro(&cls) {
                if let Some((kind, h)) = class_info(&k, |ci| ci.ext).flatten() {
                    let mut xa = Vec::new();
                    for a in &args {
                        xa.push(match a {
                            None => crate::ExtArg::None,
                            Some(v) => match v {
                                Value::O(o) => match &*o.data.borrow() {
                                    Data::Ext(k2, h2) => crate::ExtArg::Obj(*k2, *h2, false, Vec::new()),
                                    Data::Class(ci) if ci.ext.is_some() => crate::ExtArg::Obj(ci.ext.unwrap().0, ci.ext.unwrap().1, true, ci.id.clone()),
                                    _ => crate::ExtArg::Str(v.fast_string()),
                                },
                                Value::S(s) => crate::ExtArg::Str(s.clone()),
                            },
                        });
                    }
                    let mut st = core::mem::replace(&mut it.st, State::empty());
                    let r = it.host.ext_send(&mut st, kind, h, b"NEW", &xa);
                    it.st = st;
                    return match r {
                        Ok(crate::ExtArg::Obj(k2, h2, _, _)) => {
                            let o = w.new_obj(&cls, Data::Ext(k2, h2));
                            Ok(Some(Value::O(o)))
                        }
                        Ok(v) => Ok(it.from_ext(v)),
                        Err(crate::ExtErr::Error(c, m)) => {
                            it.err_msg = m;
                            Err(Err::Syntax(c))
                        }
                        Err(_) => Err(Err::new(98)),
                    };
                }
            }
            let b = class_builtin(&cls);
            if Rc::ptr_eq(&cls, &w.class(Builtin::String)) {
                let s = match arg(&args, 0) {
                    Some(v) => it.to_str(f, &v)?,
                    None => Vec::new(),
                };
                return Ok(Some(Value::S(s)));
            }
            if b == Some(Builtin::Class) {
                // a metaclass's instance: a new class
                let id = match arg(&args, 0) {
                    Some(v) => it.to_str(f, &v)?,
                    None => b"Unnamed".to_vec(),
                };
                let c = w.new_class(id, alloc::vec![w.class(Builtin::Object)], cls.clone());
                return Ok(Some(Value::O(c)));
            }
            let o = Value::O(w.new_obj(&cls, new_data(b)));
            it.send(f, &o, b"INIT", args, Start::Normal, lvl)?;
            Ok(Some(o))
        }
        b"SUBCLASS" | b"MIXINCLASS" => {
            let id = str_arg(it, f, &args, 0)?;
            let meta = match arg(&args, 1) {
                Some(Value::O(m)) if m.is_class() => m,
                _ => class_info(&cls, |ci| ci.meta.clone()).flatten().unwrap_or_else(|| cls.class()),
            };
            let c = w.new_class(id, alloc::vec![cls.clone()], meta);
            if let Data::Class(ci) = &mut *c.data.borrow_mut() {
                ci.mixin = name == b"MIXINCLASS";
                ci.meta = class_info(&cls, |p| p.meta.clone()).flatten();
                ci.pkg = class_info(&cls, |p| p.pkg.clone()).flatten();
            }
            let v = Value::O(c);
            it.send(f, &v, b"INIT", Vec::new(), Start::Normal, lvl)?;
            Ok(Some(v))
        }
        b"SUPERCLASSES" => {
            let s = class_info(&cls, |ci| ci.supers.clone()).unwrap_or_default();
            Ok(Some(w.array(s.into_iter().map(Value::O).collect())))
        }
        b"SUBCLASSES" => {
            let s = class_info(&cls, |ci| ci.subs.clone()).unwrap_or_default();
            Ok(Some(w.array(s.iter().filter_map(|x| x.upgrade()).map(Value::O).collect())))
        }
        b"INHERIT" => {
            let m = need(it, &args, 0)?;
            let Value::O(mc) = m else { return Err(Err::new(98)) };
            if !class_info(&mc, |ci| ci.mixin).unwrap_or(false) {
                let id = mc.class_id();
                it.err_msg = msg_subst(secondary(98942).unwrap(), &[&id]);
                return Err(Err::Syntax(98942));
            }
            if mro(&cls).iter().any(|k| Rc::ptr_eq(k, &mc)) || mro(&mc).iter().any(|k| Rc::ptr_eq(k, &cls)) {
                let (a, b) = (cls.class_id(), mc.class_id());
                it.err_msg = msg_subst(secondary(98944).unwrap(), &[&a, &b]);
                return Err(Err::Syntax(98944));
            }
            if let Data::Class(ci) = &mut *cls.data.borrow_mut() {
                ci.supers.push(mc.clone());
            }
            if let Data::Class(ci) = &mut *mc.data.borrow_mut() {
                ci.subs.push(Rc::downgrade(&cls));
            }
            Ok(None)
        }
        b"UNINHERIT" => {
            let m = need(it, &args, 0)?;
            let Value::O(mc) = m else { return Err(Err::new(98)) };
            if let Data::Class(ci) = &mut *cls.data.borrow_mut() {
                match ci.supers.iter().skip(1).position(|k| Rc::ptr_eq(k, &mc)) {
                    Some(p) => {
                        ci.supers.remove(p + 1);
                    }
                    None => {
                        let (a, b) = (ci.id.clone(), mc.class_id());
                        it.err_msg = msg_subst(secondary(98945).unwrap(), &[&a, &b]);
                        return Err(Err::Syntax(98945));
                    }
                }
            }
            Ok(None)
        }
        b"METACLASS" => Ok(Some(Value::O(class_info(&cls, |ci| ci.meta.clone()).flatten().unwrap_or_else(|| cls.class())))),
        b"BASECLASS" => {
            let mut c = cls.clone();
            while class_info(&c, |ci| ci.mixin).unwrap_or(false) {
                match class_info(&c, |ci| ci.supers.first().cloned()).flatten() {
                    Some(s) => c = s,
                    None => break,
                }
            }
            Ok(Some(Value::O(c)))
        }
        b"QUERYMIXINCLASS" => Ok(Some(bool_v(class_info(&cls, |ci| ci.mixin).unwrap_or(false)))),
        b"DEFINE" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            let m = match arg(&args, 1) {
                Some(src) => to_method(it, f, &n, &src)?,
                None => Method { body: Body::Null, private: false, guarded: true, protected: false },
            };
            if let Data::Class(ci) = &mut *cls.data.borrow_mut() {
                ci.inst.insert(n, m);
            }
            Ok(None)
        }
        b"DELETE" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            if let Data::Class(ci) = &mut *cls.data.borrow_mut() {
                ci.inst.remove(&n);
            }
            Ok(None)
        }
        b"METHOD" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            let m = class_info(&cls, |ci| {
                ci.inst.get(&n).cloned().or_else(|| {
                    ci.builtin.filter(|b| has_native(*b, false, &n)).map(|b| Method { body: Body::Native(b), private: false, guarded: false, protected: false })
                })
            })
            .flatten();
            match m {
                Some(m) => Ok(Some(w.obj(Builtin::Method, Data::Method(m)))),
                None => {
                    it.err_msg = msg_subst(b"There is no method \"&1\"", &[&n]);
                    Err(Err::Syntax(93917))
                }
            }
        }
        b"METHODS" => {
            // this class's methods (with an argument .nil: this class's;
            // without: inherited too)
            let mut names = Vec::new();
            let mut items = Vec::new();
            let classes = match arg(&args, 0) {
                Some(Value::O(c)) if c.is_class() => alloc::vec![c],
                Some(_) => alloc::vec![cls.clone()],
                None => mro(&cls),
            };
            for k in classes {
                let ms: Vec<(Vec<u8>, Method)> = class_info(&k, |ci| {
                    let mut v: Vec<(Vec<u8>, Method)> = ci.inst.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
                    if let Some(b) = ci.builtin {
                        for n in natives(b, false) {
                            v.push((n.to_vec(), Method { body: Body::Native(b), private: false, guarded: false, protected: false }));
                        }
                    }
                    v
                })
                .unwrap_or_default();
                for (n, m) in ms {
                    names.push(Value::S(n));
                    items.push(w.obj(Builtin::Method, Data::Method(m)));
                }
            }
            Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items, indexes: names, pos: 0 }))))
        }
        b"ENHANCED" => {
            let ms = need(it, &args, 0)?;
            let o = Value::O(w.new_obj(&cls, new_data(class_builtin(&cls))));
            let pairs = coll_pairs(it, f, &ms)?;
            let mut t = BTreeMap::new();
            for (k, v) in pairs {
                let n = upper(&k.fast_string());
                let m = to_method(it, f, &n, &v)?;
                t.insert(n, m);
            }
            if let Value::O(oo) = &o {
                *oo.methods.borrow_mut() = Some(t);
            }
            let rest: Args = args.into_iter().skip(1).collect();
            it.send(f, &o, b"INIT", rest, Start::Normal, lvl)?;
            Ok(Some(o))
        }
        _ => Ok(None),
    }
}

/// (index, item) pairs of a collection (via its supplier).
fn coll_pairs(it: &mut Interp, f: &mut Frame, c: &Value) -> R<Vec<(Value, Value)>> {
    if let Value::O(o) = c {
        let d = o.data.borrow();
        match &*d {
            Data::Directory(dd) => return Ok(dd.map.iter().map(|(k, v)| (Value::S(k.clone()), v.clone())).collect()),
            Data::Coll(m) => return Ok(m.values().flatten().cloned().collect()),
            Data::Array(a) => {
                return Ok(a.items.iter().enumerate().filter_map(|(i, v)| v.clone().map(|v| (Value::S(int_str(i as i64 + 1)), v))).collect());
            }
            _ => {}
        }
    }
    let s = it.send(f, c, b"SUPPLIER", Vec::new(), Start::Normal, 0)?.ok_or(Err::new(98))?;
    let mut out = Vec::new();
    if let Value::O(o) = &s {
        if let Data::Supplier(sd) = &*o.data.borrow() {
            for i in sd.pos..sd.items.len() {
                out.push((sd.indexes.get(i).cloned().unwrap_or(Value::S(Vec::new())), sd.items[i].clone()));
            }
        }
    }
    Ok(out)
}

// ---- .String ----

fn string_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let s = match recv {
        Value::S(s) => s.clone(),
        Value::O(o) => match &*o.data.borrow() {
            Data::Str(s) => s.clone(),
            _ => o.fast_string(),
        },
    };
    if name == b"INIT" {
        if let Value::O(o) = recv {
            if let Some(v) = arg(&args, 0) {
                let v = it.to_str(f, &v)?;
                *o.data.borrow_mut() = Data::Str(v);
            }
        }
        return Ok(None);
    }
    if name == b"STRING" || name == b"MAKESTRING" {
        return Ok(Some(Value::S(s)));
    }
    // operators
    let op = match name {
        b"+" => Some(crate::parse::BinOp::Add),
        b"-" => Some(crate::parse::BinOp::Sub),
        b"*" => Some(crate::parse::BinOp::Mul),
        b"/" => Some(crate::parse::BinOp::Div),
        b"%" => Some(crate::parse::BinOp::IDiv),
        b"//" => Some(crate::parse::BinOp::Rem),
        b"**" => Some(crate::parse::BinOp::Pow),
        b"||" | b"" => Some(crate::parse::BinOp::Concat),
        b" " => Some(crate::parse::BinOp::Blank),
        b"=" => Some(crate::parse::BinOp::Eq),
        b"\\=" | b"<>" | b"><" => Some(crate::parse::BinOp::Ne),
        b"==" => Some(crate::parse::BinOp::SEq),
        b"\\==" => Some(crate::parse::BinOp::SNe),
        b">" => Some(crate::parse::BinOp::Gt),
        b"<" => Some(crate::parse::BinOp::Lt),
        b">=" | b"\\<" => Some(crate::parse::BinOp::Ge),
        b"<=" | b"\\>" => Some(crate::parse::BinOp::Le),
        b">>" => Some(crate::parse::BinOp::SGt),
        b"<<" => Some(crate::parse::BinOp::SLt),
        b">>=" | b"\\<<" => Some(crate::parse::BinOp::SGe),
        b"<<=" | b"\\>>" => Some(crate::parse::BinOp::SLe),
        b"&" => Some(crate::parse::BinOp::And),
        b"|" => Some(crate::parse::BinOp::Or),
        b"&&" => Some(crate::parse::BinOp::Xor),
        _ => None,
    };
    if let Some(op) = op {
        match arg(&args, 0) {
            Some(y) => {
                let y = it.to_str(f, &y)?;
                return Ok(Some(Value::S(binop(op, &s, &y, &f.numeric)?)));
            }
            None => {
                // prefix + and -
                return match name {
                    b"-" | b"+" => {
                        let n = Num::parse(&s).ok_or(Err::new(41))?;
                        let n = if name == b"-" { n.neg() } else { n };
                        let mut r = n.operand(f.numeric.digits);
                        r.round(f.numeric.digits);
                        Ok(Some(Value::S(r.format(&f.numeric))))
                    }
                    _ => Err(Err::new(93)),
                };
            }
        }
    }
    if name == b"\\" {
        return match s.as_slice() {
            b"0" => Ok(Some(Value::S(b"1".to_vec()))),
            b"1" => Ok(Some(Value::S(b"0".to_vec()))),
            _ => Err(Err::new(34)),
        };
    }
    // the built-in functions, the receiver as the string argument
    let mut sargs: Vec<Option<Val>> = Vec::new();
    for a in &args {
        sargs.push(match a {
            Some(v) => Some(it.to_str(f, v)?),
            None => None,
        });
    }
    let bif_args: Vec<Option<Val>> = match name {
        // the receiver is the second argument
        b"INSERT" | b"OVERLAY" | b"POS" | b"LASTPOS" | b"COUNTSTR" | b"CHANGESTR" | b"WORDPOS" => {
            let mut v = Vec::new();
            v.push(sargs.first().cloned().flatten());
            v.push(Some(s));
            v.extend(sargs.into_iter().skip(1));
            v
        }
        _ => {
            let mut v = alloc::vec![Some(s)];
            v.extend(sargs);
            v
        }
    };
    let bif: &[u8] = if name == b"CENTRE" { b"CENTER" } else { name };
    if let Some(r) = crate::oo::oo_bif(bif, &bif_args) {
        return r.map(|v| Some(Value::S(v)));
    }
    match crate::builtins::call(it, f, bif, &bif_args) {
        Some(r) => r.map(|v| Some(Value::S(v))),
        None => Ok(None),
    }
}

// ---- .Array ----

fn arr_index(it: &mut Interp, f: &mut Frame, o: &ObjRef, idx: &[Option<Value>]) -> R<usize> {
    let dims = match &*o.data.borrow() {
        Data::Array(a) => a.dims.clone(),
        _ => Vec::new(),
    };
    let mut ns = Vec::new();
    for (i, x) in idx.iter().enumerate() {
        let Some(v) = x else {
            let k = int_str(i as i64 + 1);
            return Err(err93(it, 903, &[&k]));
        };
        let s = it.to_str(f, v)?;
        match Num::parse(&s).and_then(|n| n.to_whole(9)).filter(|n| *n >= 1) {
            Some(n) => ns.push(n as usize),
            None => return Err(err93(it, 919, &[&s])),
        }
    }
    if ns.is_empty() {
        return Err(err93(it, 903, &[b"1"]));
    }
    if dims.len() <= 1 {
        if ns.len() > 1 {
            let k = int_str(1);
            return Err(err93(it, 926, &[&k]));
        }
        return Ok(ns[0] - 1);
    }
    if ns.len() != dims.len() {
        let k = int_str(dims.len() as i64);
        return Err(err93(it, if ns.len() < dims.len() { 925 } else { 926 }, &[&k]));
    }
    let mut p = 0usize;
    for (n, d) in ns.iter().zip(&dims) {
        if n > d {
            let s = int_str(*n as i64);
            return Err(err93(it, 919, &[&s]));
        }
        p = p * d + (n - 1);
    }
    Ok(p)
}

fn array_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let items = || match &*o.data.borrow() {
        Data::Array(a) => a.items.clone(),
        _ => Vec::new(),
    };
    match name {
        b"INIT" => {
            let mut dims = Vec::new();
            for i in 0..args.len() {
                let n = whole_arg(it, f, &args, i)?;
                dims.push(n.max(0) as usize);
            }
            if let Data::Array(a) = &mut *o.data.borrow_mut() {
                let size = if dims.is_empty() { 0 } else { dims.iter().product() };
                a.items = alloc::vec![None; size];
                a.dims = if dims.len() > 1 { dims } else { Vec::new() };
            }
            Ok(None)
        }
        b"[]" | b"AT" => {
            let i = arr_index(it, f, &o, &args)?;
            let v = items().get(i).cloned().flatten();
            Ok(Some(v.unwrap_or_else(|| nil(it))))
        }
        b"[]=" | b"PUT" => {
            let v = need(it, &args, 0)?;
            let i = arr_index(it, f, &o, &args[1..])?;
            if let Data::Array(a) = &mut *o.data.borrow_mut() {
                if i >= a.items.len() {
                    a.items.resize(i + 1, None);
                }
                a.items[i] = Some(v);
            }
            Ok(None)
        }
        b"REMOVE" => {
            let i = arr_index(it, f, &o, &args)?;
            let v = match &mut *o.data.borrow_mut() {
                Data::Array(a) => a.items.get_mut(i).and_then(|x| x.take()),
                _ => None,
            };
            Ok(Some(v.unwrap_or_else(|| nil(it))))
        }
        b"HASINDEX" => {
            let i = arr_index(it, f, &o, &args)?;
            Ok(Some(bool_v(items().get(i).is_some_and(|x| x.is_some()))))
        }
        b"ITEMS" => Ok(Some(Value::S(int_str(items().iter().filter(|x| x.is_some()).count() as i64)))),
        b"SIZE" => Ok(Some(Value::S(int_str(items().len() as i64)))),
        b"DIMENSION" => {
            let dims = match &*o.data.borrow() {
                Data::Array(a) => a.dims.clone(),
                _ => Vec::new(),
            };
            match arg(&args, 0) {
                None => Ok(Some(Value::S(int_str(if dims.is_empty() { 1 } else { dims.len() as i64 })))),
                Some(_) => {
                    let n = pos_arg(it, f, &args, 0)?;
                    let d = if dims.is_empty() { if n == 1 { items().len() } else { 0 } } else { dims.get(n - 1).copied().unwrap_or(0) };
                    Ok(Some(Value::S(int_str(d as i64))))
                }
            }
        }
        b"FIRST" | b"LAST" => {
            let v = items();
            let p = if name == b"FIRST" { v.iter().position(|x| x.is_some()) } else { v.iter().rposition(|x| x.is_some()) };
            Ok(Some(p.map(|p| Value::S(int_str(p as i64 + 1))).unwrap_or_else(|| nil(it))))
        }
        b"NEXT" | b"PREVIOUS" => {
            let i = arr_index(it, f, &o, &args)?;
            let v = items();
            let p = if name == b"NEXT" {
                (i + 1..v.len()).find(|&k| v[k].is_some())
            } else {
                (0..i.min(v.len())).rev().find(|&k| v[k].is_some())
            };
            Ok(Some(p.map(|p| Value::S(int_str(p as i64 + 1))).unwrap_or_else(|| nil(it))))
        }
        b"MAKEARRAY" => Ok(Some(w.array(items().into_iter().flatten().collect()))),
        b"SECTION" => {
            let start = pos_arg(it, f, &args, 0)? - 1;
            let v = items();
            let n = match arg(&args, 1) {
                Some(_) => whole_arg(it, f, &args, 1)?.max(0) as usize,
                None => v.len().saturating_sub(start),
            };
            let sec: Vec<Option<Value>> = v.into_iter().skip(start).take(n).collect();
            Ok(Some(w.array_opt(sec)))
        }
        b"SUPPLIER" => {
            let v = items();
            let mut its = Vec::new();
            let mut idx = Vec::new();
            for (i, x) in v.into_iter().enumerate() {
                if let Some(x) = x {
                    its.push(x);
                    idx.push(Value::S(int_str(i as i64 + 1)));
                }
            }
            Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items: its, indexes: idx, pos: 0 }))))
        }
        _ => Ok(None),
    }
}

// ---- .List ----

fn list_idx(it: &mut Interp, f: &mut Frame, v: &Value) -> R<u64> {
    let s = it.to_str(f, v)?;
    match Num::parse(&s).and_then(|n| n.to_whole(18)) {
        Some(n) if n >= 0 => Ok(n as u64),
        _ => Err(err93(it, 918, &[&s])),
    }
}

fn list_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let snapshot = || match &*o.data.borrow() {
        Data::List(l) => l.items.iter().cloned().collect::<Vec<(u64, Value)>>(),
        _ => Vec::new(),
    };
    let ix = |n: u64| Value::S(int_str(n as i64));
    match name {
        b"INIT" => Ok(None),
        b"INSERT" => {
            let v = need(it, &args, 0)?;
            let after = match args.get(1) {
                None => None,
                Some(None) => None,
                Some(Some(Value::O(n))) if matches!(&*n.data.borrow(), Data::Nil) => Some(None),
                Some(Some(x)) => Some(Some(list_idx(it, f, x)?)),
            };
            let mut bad = None;
            let id = if let Data::List(l) = &mut *o.data.borrow_mut() {
                let id = l.next;
                l.next += 1;
                match after {
                    None => l.items.push_back((id, v)),
                    Some(None) => l.items.push_front((id, v)),
                    Some(Some(a)) => match l.items.iter().position(|(k, _)| *k == a) {
                        Some(p) => l.items.insert(p + 1, (id, v)),
                        None => bad = Some(a),
                    },
                }
                id
            } else {
                0
            };
            if let Some(a) = bad {
                let s = int_str(a as i64);
                return Err(err93(it, 918, &[&s]));
            }
            Ok(Some(ix(id)))
        }
        b"[]" | b"AT" => {
            let kv = need(it, &args, 0)?; let k = list_idx(it, f, &kv)?;
            Ok(Some(snapshot().into_iter().find(|(i, _)| *i == k).map(|x| x.1).unwrap_or_else(|| nil(it))))
        }
        b"[]=" | b"PUT" => {
            let v = need(it, &args, 0)?;
            let kv = need(it, &args, 1)?; let k = list_idx(it, f, &kv)?;
            let mut ok = false;
            if let Data::List(l) = &mut *o.data.borrow_mut() {
                if let Some(e) = l.items.iter_mut().find(|(i, _)| *i == k) {
                    e.1 = v;
                    ok = true;
                }
            }
            if !ok {
                let s = int_str(k as i64);
                return Err(err93(it, 918, &[&s]));
            }
            Ok(None)
        }
        b"REMOVE" => {
            let kv = need(it, &args, 0)?; let k = list_idx(it, f, &kv)?;
            let r = match &mut *o.data.borrow_mut() {
                Data::List(l) => l.items.iter().position(|(i, _)| *i == k).and_then(|p| l.items.remove(p)).map(|x| x.1),
                _ => None,
            };
            Ok(Some(r.unwrap_or_else(|| nil(it))))
        }
        b"HASINDEX" => {
            let kv = need(it, &args, 0)?; let k = list_idx(it, f, &kv)?;
            Ok(Some(bool_v(snapshot().iter().any(|(i, _)| *i == k))))
        }
        b"ITEMS" => Ok(Some(Value::S(int_str(snapshot().len() as i64)))),
        b"FIRST" => Ok(Some(snapshot().first().map(|x| ix(x.0)).unwrap_or_else(|| nil(it)))),
        b"LAST" => Ok(Some(snapshot().last().map(|x| ix(x.0)).unwrap_or_else(|| nil(it)))),
        b"FIRSTITEM" => Ok(Some(snapshot().first().map(|x| x.1.clone()).unwrap_or_else(|| nil(it)))),
        b"LASTITEM" => Ok(Some(snapshot().last().map(|x| x.1.clone()).unwrap_or_else(|| nil(it)))),
        b"NEXT" | b"PREVIOUS" => {
            let kv = need(it, &args, 0)?; let k = list_idx(it, f, &kv)?;
            let v = snapshot();
            let p = v.iter().position(|(i, _)| *i == k);
            let q = match p {
                Some(p) if name == b"NEXT" => v.get(p + 1),
                Some(p) if p > 0 => v.get(p - 1),
                _ => None,
            };
            Ok(Some(q.map(|x| ix(x.0)).unwrap_or_else(|| nil(it))))
        }
        b"MAKEARRAY" => Ok(Some(w.array(snapshot().into_iter().map(|x| x.1).collect()))),
        b"SECTION" => {
            let kv = need(it, &args, 0)?; let k = list_idx(it, f, &kv)?;
            let v = snapshot();
            let p = v.iter().position(|(i, _)| *i == k).unwrap_or(v.len());
            let n = match arg(&args, 1) {
                Some(_) => whole_arg(it, f, &args, 1)?.max(0) as usize,
                None => usize::MAX,
            };
            let mut l = ListData::default();
            for (_, x) in v.into_iter().skip(p).take(n) {
                l.items.push_back((l.next, x));
                l.next += 1;
            }
            Ok(Some(w.obj(Builtin::List, Data::List(l))))
        }
        b"SUPPLIER" => {
            let v = snapshot();
            let idx = v.iter().map(|x| ix(x.0)).collect();
            let its = v.into_iter().map(|x| x.1).collect();
            Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items: its, indexes: idx, pos: 0 }))))
        }
        _ => Ok(None),
    }
}

// ---- .Queue ----

fn queue_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let snapshot = || match &*o.data.borrow() {
        Data::Queue(q) => q.iter().cloned().collect::<Vec<Value>>(),
        _ => Vec::new(),
    };
    match name {
        b"INIT" => Ok(None),
        b"PUSH" | b"QUEUE" => {
            let v = need(it, &args, 0)?;
            if let Data::Queue(q) = &mut *o.data.borrow_mut() {
                if name == b"PUSH" {
                    q.push_front(v);
                } else {
                    q.push_back(v);
                }
            }
            Ok(None)
        }
        b"PULL" => {
            let v = match &mut *o.data.borrow_mut() {
                Data::Queue(q) => q.pop_front(),
                _ => None,
            };
            Ok(Some(v.unwrap_or_else(|| nil(it))))
        }
        b"PEEK" => Ok(Some(snapshot().first().cloned().unwrap_or_else(|| nil(it)))),
        b"[]" | b"AT" => {
            let n = pos_arg(it, f, &args, 0)?;
            Ok(Some(snapshot().get(n - 1).cloned().unwrap_or_else(|| nil(it))))
        }
        b"[]=" | b"PUT" => {
            let v = need(it, &args, 0)?;
            let n = pos_arg(it, f, &args, 1)?;
            let mut ok = false;
            if let Data::Queue(q) = &mut *o.data.borrow_mut() {
                if let Some(e) = q.get_mut(n - 1) {
                    *e = v;
                    ok = true;
                }
            }
            if !ok {
                let s = int_str(n as i64);
                return Err(err93(it, 918, &[&s]));
            }
            Ok(None)
        }
        b"REMOVE" => {
            let n = pos_arg(it, f, &args, 0)?;
            let v = match &mut *o.data.borrow_mut() {
                Data::Queue(q) => q.remove(n - 1),
                _ => None,
            };
            Ok(Some(v.unwrap_or_else(|| nil(it))))
        }
        b"HASINDEX" => {
            let n = whole_arg(it, f, &args, 0)?;
            Ok(Some(bool_v(n >= 1 && (n as usize) <= snapshot().len())))
        }
        b"ITEMS" => Ok(Some(Value::S(int_str(snapshot().len() as i64)))),
        b"MAKEARRAY" => Ok(Some(w.array(snapshot()))),
        b"SUPPLIER" => {
            let v = snapshot();
            let idx = (1..=v.len()).map(|i| Value::S(int_str(i as i64))).collect();
            Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items: v, indexes: idx, pos: 0 }))))
        }
        _ => Ok(None),
    }
}

// ---- .Directory ----

fn dir_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, lvl: u32) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let get = |k: &[u8]| -> (Option<Value>, Option<Method>) {
        match &*o.data.borrow() {
            Data::Directory(d) => (d.map.get(k).cloned(), d.methods.get(k).cloned()),
            _ => (None, None),
        }
    };
    let lookup_entry = |it: &mut Interp, f: &mut Frame, k: &[u8]| -> RV {
        let (v, m) = get(k);
        if let Some(v) = v {
            return Ok(Some(v));
        }
        if let Some(m) = m {
            let found = Found { m, scope: None, class_side: false };
            return invoke(it, f, recv, k, Vec::new(), found, lvl);
        }
        let um = match &*o.data.borrow() {
            Data::Directory(d) => d.unknown.clone(),
            _ => None,
        };
        if let Some(m) = um {
            let found = Found { m, scope: None, class_side: false };
            return invoke(it, f, recv, k, alloc::vec![Some(Value::S(k.to_vec()))], found, lvl);
        }
        Ok(Some(nil(it)))
    };
    let set = |k: Vec<u8>, v: Option<Value>| {
        if let Data::Directory(d) = &mut *o.data.borrow_mut() {
            d.methods.remove(&k);
            match v {
                Some(v) => {
                    d.map.insert(k, v);
                }
                None => {
                    d.map.remove(&k);
                }
            }
        }
    };
    match name {
        b"INIT" => Ok(None),
        b"[]" | b"AT" => {
            let k = str_arg(it, f, &args, 0)?;
            lookup_entry(it, f, &k)
        }
        b"ENTRY" => {
            let k = upper(&str_arg(it, f, &args, 0)?);
            lookup_entry(it, f, &k)
        }
        b"[]=" | b"PUT" => {
            let v = need(it, &args, 0)?;
            let k = str_arg(it, f, &args, 1)?;
            set(k, Some(v));
            Ok(None)
        }
        b"SETENTRY" => {
            let k = upper(&str_arg(it, f, &args, 0)?);
            set(k, arg(&args, 1));
            Ok(None)
        }
        b"SETMETHOD" => {
            let k = upper(&str_arg(it, f, &args, 0)?);
            match arg(&args, 1) {
                Some(src) => {
                    let m = to_method(it, f, &k, &src)?;
                    if let Data::Directory(d) = &mut *o.data.borrow_mut() {
                        if k.as_slice() == b"UNKNOWN" {
                            d.unknown = Some(m);
                        } else {
                            d.map.remove(&k);
                            d.methods.insert(k, m);
                        }
                    }
                }
                None => set(k, None),
            }
            Ok(None)
        }
        b"REMOVE" => {
            let k = str_arg(it, f, &args, 0)?;
            let (v, _) = get(&k);
            set(k, None);
            Ok(Some(v.unwrap_or_else(|| nil(it))))
        }
        b"HASINDEX" => {
            let k = str_arg(it, f, &args, 0)?;
            let (v, m) = get(&k);
            Ok(Some(bool_v(v.is_some() || m.is_some())))
        }
        b"HASENTRY" => {
            let k = upper(&str_arg(it, f, &args, 0)?);
            let (v, m) = get(&k);
            Ok(Some(bool_v(v.is_some() || m.is_some())))
        }
        b"ITEMS" => Ok(Some(Value::S(int_str(match &*o.data.borrow() {
            Data::Directory(d) => (d.map.len() + d.methods.len()) as i64,
            _ => 0,
        })))),
        b"MAKEARRAY" => {
            let keys: Vec<Value> = match &*o.data.borrow() {
                Data::Directory(d) => d.map.keys().chain(d.methods.keys()).map(|k| Value::S(k.clone())).collect(),
                _ => Vec::new(),
            };
            Ok(Some(w.array(keys)))
        }
        b"SUPPLIER" => {
            let (its, idx): (Vec<Value>, Vec<Value>) = match &*o.data.borrow() {
                Data::Directory(d) => d.map.iter().map(|(k, v)| (v.clone(), Value::S(k.clone()))).unzip(),
                _ => (Vec::new(), Vec::new()),
            };
            Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items: its, indexes: idx, pos: 0 }))))
        }
        b"UNKNOWN" => {
            // dir~name: the entry; dir~name = v: set it
            let n = upper(&str_arg(it, f, &args, 0)?);
            if let Some(k) = n.strip_suffix(b"=") {
                let a = arg(&args, 1).and_then(|a| array_items(&a)).unwrap_or_default();
                let v = a.into_iter().next().flatten();
                set(k.to_vec(), v);
                return Ok(None);
            }
            lookup_entry(it, f, &n)
        }
        b"UNION" | b"INTERSECTION" | b"XOR" | b"DIFFERENCE" | b"SUBSET" => {
            let other = need(it, &args, 0)?;
            let theirs: Vec<(Vec<u8>, Value)> = coll_pairs(it, f, &other)?.into_iter().map(|(k, v)| (k.fast_string(), v)).collect();
            let mine: BTreeMap<Vec<u8>, Value> = match &*o.data.borrow() {
                Data::Directory(d) => d.map.clone(),
                _ => BTreeMap::new(),
            };
            let has_t = |k: &[u8]| theirs.iter().any(|(x, _)| x.as_slice() == k);
            if name == b"SUBSET" {
                return Ok(Some(bool_v(mine.keys().all(|k| has_t(k)))));
            }
            let mut out: BTreeMap<Vec<u8>, Value> = BTreeMap::new();
            match name {
                b"UNION" => {
                    out = mine.clone();
                    for (k, v) in &theirs {
                        out.entry(k.clone()).or_insert(v.clone());
                    }
                }
                b"INTERSECTION" => {
                    for (k, v) in &mine {
                        if has_t(k) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
                b"DIFFERENCE" => {
                    for (k, v) in &mine {
                        if !has_t(k) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
                _ => {
                    for (k, v) in &mine {
                        if !has_t(k) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    for (k, v) in &theirs {
                        if !mine.contains_key(k) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            Ok(Some(w.obj(Builtin::Directory, Data::Directory(DirData { map: out, methods: BTreeMap::new(), unknown: None }))))
        }
        _ => Ok(None),
    }
}

// ---- .Table, .Relation, .Set, .Bag ----

fn coll_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let kind = kind_of(&o).unwrap_or(Builtin::Table);
    let multi = matches!(kind, Builtin::Relation | Builtin::Bag);
    let index_only = matches!(kind, Builtin::Set | Builtin::Bag);
    let all = || match &*o.data.borrow() {
        Data::Coll(m) => m.values().flatten().cloned().collect::<Vec<(Value, Value)>>(),
        _ => Vec::new(),
    };
    let at = |k: &Key| match &*o.data.borrow() {
        Data::Coll(m) => m.get(k).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    match name {
        b"INIT" => Ok(None),
        b"[]=" | b"PUT" => {
            let v = need(it, &args, 0)?;
            let idx = if index_only {
                match arg(&args, 1) {
                    Some(i) => {
                        if !i.same(&v) && i.key() != v.key() {
                            return Err(err93(it, 949, &[]));
                        }
                        i
                    }
                    None => v.clone(),
                }
            } else {
                need(it, &args, 1)?
            };
            if let Data::Coll(m) = &mut *o.data.borrow_mut() {
                let e = m.entry(idx.key()).or_default();
                if !multi {
                    e.clear();
                } else if kind == Builtin::Relation {
                    // the same item at the same index once
                    e.retain(|(_, x)| !x.same(&v));
                }
                e.push((idx, v));
            }
            Ok(None)
        }
        b"[]" | b"AT" => {
            let k = need(it, &args, 0)?.key();
            Ok(Some(at(&k).into_iter().next().map(|x| x.1).unwrap_or_else(|| nil(it))))
        }
        b"ALLAT" => {
            let k = need(it, &args, 0)?.key();
            Ok(Some(w.array(at(&k).into_iter().map(|x| x.1).collect())))
        }
        b"HASINDEX" => {
            let k = need(it, &args, 0)?.key();
            Ok(Some(bool_v(!at(&k).is_empty())))
        }
        b"HASITEM" => {
            let v = need(it, &args, 0)?;
            let k = need(it, &args, 1)?.key();
            Ok(Some(bool_v(at(&k).iter().any(|(_, x)| x.same(&v)))))
        }
        b"ALLINDEX" => {
            let v = need(it, &args, 0)?;
            Ok(Some(w.array(all().into_iter().filter(|(_, x)| x.same(&v)).map(|x| x.0).collect())))
        }
        b"INDEX" => {
            let v = need(it, &args, 0)?;
            Ok(Some(all().into_iter().find(|(_, x)| x.same(&v)).map(|x| x.0).unwrap_or_else(|| nil(it))))
        }
        b"REMOVE" => {
            let k = need(it, &args, 0)?.key();
            let r = match &mut *o.data.borrow_mut() {
                Data::Coll(m) => {
                    let r = m.get_mut(&k).and_then(|e| if e.is_empty() { None } else { Some(e.remove(0)) });
                    if m.get(&k).is_some_and(|e| e.is_empty()) {
                        m.remove(&k);
                    }
                    r
                }
                _ => None,
            };
            Ok(Some(r.map(|x| x.1).unwrap_or_else(|| nil(it))))
        }
        b"REMOVEITEM" => {
            let v = need(it, &args, 0)?;
            let k = need(it, &args, 1)?.key();
            let r = match &mut *o.data.borrow_mut() {
                Data::Coll(m) => {
                    let r = m.get_mut(&k).and_then(|e| e.iter().position(|(_, x)| x.same(&v)).map(|p| e.remove(p)));
                    if m.get(&k).is_some_and(|e| e.is_empty()) {
                        m.remove(&k);
                    }
                    r
                }
                _ => None,
            };
            Ok(Some(r.map(|x| x.1).unwrap_or_else(|| nil(it))))
        }
        b"ITEMS" => {
            let n = match arg(&args, 0) {
                Some(i) => at(&i.key()).len(),
                None => all().len(),
            };
            Ok(Some(Value::S(int_str(n as i64))))
        }
        b"MAKEARRAY" => Ok(Some(w.array(all().into_iter().map(|x| x.0).collect()))),
        b"SUPPLIER" => {
            let (idx, its): (Vec<Value>, Vec<Value>) = match arg(&args, 0) {
                Some(i) => at(&i.key()).into_iter().unzip(),
                None => all().into_iter().unzip(),
            };
            Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items: its, indexes: idx, pos: 0 }))))
        }
        b"UNION" | b"INTERSECTION" | b"XOR" | b"DIFFERENCE" | b"SUBSET" => {
            let other = need(it, &args, 0)?;
            let theirs = coll_pairs(it, f, &other)?;
            let mine = all();
            let has_t = |k: &Value| theirs.iter().any(|(x, _)| x.key() == k.key());
            let has_m = |k: &Value| mine.iter().any(|(x, _)| x.key() == k.key());
            if name == b"SUBSET" {
                return Ok(Some(bool_v(mine.iter().all(|(k, _)| has_t(k)))));
            }
            let mut out: Vec<(Value, Value)> = Vec::new();
            match name {
                b"UNION" => {
                    out.extend(mine.iter().cloned());
                    for (k, v) in &theirs {
                        if multi || !has_m(k) {
                            out.push((k.clone(), v.clone()));
                        }
                    }
                }
                b"INTERSECTION" => out.extend(mine.iter().filter(|(k, _)| has_t(k)).cloned()),
                b"DIFFERENCE" => out.extend(mine.iter().filter(|(k, _)| !has_t(k)).cloned()),
                _ => {
                    out.extend(mine.iter().filter(|(k, _)| !has_t(k)).cloned());
                    out.extend(theirs.iter().filter(|(k, _)| !has_m(k)).cloned());
                }
            }
            let mut m: BTreeMap<Key, Vec<(Value, Value)>> = BTreeMap::new();
            for (k, v) in out {
                let e = m.entry(k.key()).or_default();
                if !multi {
                    e.clear();
                }
                e.push((k, v));
            }
            Ok(Some(Value::O(w.new_obj(&o.class(), Data::Coll(m)))))
        }
        _ => Ok(None),
    }
}

// ---- .Stem ----

fn stem_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, lvl: u32) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let (stem, sname) = match &*o.data.borrow() {
        Data::Stem(s, n) => (s.clone(), n.clone()),
        _ => return Ok(None),
    };
    let tail = |it: &mut Interp, f: &mut Frame, a: &[Option<Value>]| -> R<Val> {
        let mut t = Vec::new();
        for (i, x) in a.iter().enumerate() {
            if i > 0 {
                t.push(b'.');
            }
            if let Some(x) = x {
                t.extend_from_slice(&it.to_str(f, x)?);
            }
        }
        Ok(t)
    };
    match name {
        b"INIT" => {
            let n = match arg(&args, 0) {
                Some(v) => it.to_str(f, &v)?,
                None => Vec::new(),
            };
            if let Data::Stem(_, nm) = &mut *o.data.borrow_mut() {
                *nm = n;
            }
            Ok(None)
        }
        b"[]" => {
            if args.is_empty() {
                return Ok(Some(stem.borrow().default.clone().unwrap_or_else(|| Value::S(sname.clone()))));
            }
            let t = tail(it, f, &args)?;
            let s = stem.borrow();
            Ok(Some(match s.tails.get(&t) {
                Some(Some(v)) => v.clone(),
                Some(None) | None => match &s.default {
                    Some(d) if !s.tails.contains_key(&t) => d.clone(),
                    _ => {
                        let mut n = sname.clone();
                        n.extend_from_slice(&t);
                        Value::S(n)
                    }
                },
            }))
        }
        b"[]=" => {
            let v = need(it, &args, 0)?;
            if args.len() == 1 {
                let mut s = stem.borrow_mut();
                s.tails.clear();
                s.default = Some(v);
                return Ok(None);
            }
            let t = tail(it, f, &args[1..])?;
            stem.borrow_mut().tails.insert(t, Some(v));
            Ok(None)
        }
        b"MAKEARRAY" => {
            let items: Vec<Value> = stem.borrow().tails.iter().filter_map(|(k, v)| v.as_ref().map(|_| Value::S(k.clone()))).collect();
            Ok(Some(w.array(items)))
        }
        b"REQUEST" => {
            let c = upper(&str_arg(it, f, &args, 0)?);
            match c.as_slice() {
                b"STRING" => Ok(Some(stem.borrow().default.clone().unwrap_or(Value::S(sname)))),
                b"ARRAY" => stem_m(it, f, recv, b"MAKEARRAY", Vec::new(), lvl),
                b"STEM" => Ok(Some(recv.clone())),
                _ => Ok(Some(nil(it))),
            }
        }
        b"UNKNOWN" => {
            // messages to a stem go to its default value
            let n = str_arg(it, f, &args, 0)?;
            let a = arg(&args, 1).and_then(|a| array_items(&a)).unwrap_or_default();
            let d = stem.borrow().default.clone().unwrap_or(Value::S(sname));
            it.send(f, &d, &upper(&n), a, Start::Normal, lvl)
        }
        _ => Ok(None),
    }
}

// ---- .Supplier ----

fn supplier_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    match name {
        b"INIT" => {
            let items = arg(&args, 0).and_then(|a| array_items(&a)).unwrap_or_default();
            let idx = arg(&args, 1).and_then(|a| array_items(&a)).unwrap_or_default();
            if let Data::Supplier(s) = &mut *o.data.borrow_mut() {
                s.items = items.into_iter().map(|x| x.unwrap_or(Value::S(Vec::new()))).collect();
                s.indexes = idx.into_iter().map(|x| x.unwrap_or(Value::S(Vec::new()))).collect();
                s.pos = 0;
            }
            let _ = f;
            Ok(None)
        }
        b"AVAILABLE" => Ok(Some(bool_v(match &*o.data.borrow() {
            Data::Supplier(s) => s.pos < s.items.len(),
            _ => false,
        }))),
        b"NEXT" => {
            let ok = match &mut *o.data.borrow_mut() {
                Data::Supplier(s) if s.pos < s.items.len() => {
                    s.pos += 1;
                    true
                }
                _ => false,
            };
            if !ok {
                return Err(err93(it, 937, &[]));
            }
            Ok(None)
        }
        b"ITEM" | b"INDEX" => {
            let v = match &*o.data.borrow() {
                Data::Supplier(s) if s.pos < s.items.len() => Some(if name == b"ITEM" { s.items[s.pos].clone() } else { s.indexes.get(s.pos).cloned().unwrap_or(Value::S(Vec::new())) }),
                _ => None,
            };
            match v {
                Some(v) => Ok(Some(v)),
                None => Err(err93(it, 937, &[])),
            }
        }
        _ => Ok(None),
    }
}

// ---- .Message ----

/// Run a message object (SEND and START: one activity runs at a time, so
/// a started message runs to its end before START returns, unless the
/// host gives it an activity of its own).
pub fn start_message(it: &mut Interp, f: &mut Frame, m: &Value, lvl: u32) -> RV {
    let Some(o) = m.obj().cloned() else { return Ok(None) };
    let (target, name, scope, args, state) = match &mut *o.data.borrow_mut() {
        Data::Message(md) => {
            let s = md.state;
            if s == 0 {
                md.state = 1;
            }
            (md.target.clone(), md.name.clone(), md.scope.clone(), md.args.clone(), s)
        }
        _ => return Ok(None),
    };
    if state != 0 {
        it.err_msg = secondary(98915).unwrap().to_vec();
        return Err(Err::Syntax(98915));
    }
    let start = match scope {
        Some(s) => Start::At(s),
        None => Start::Normal,
    };
    let r = it.send(f, &target, &name, args, start, lvl);
    let notify = match &mut *o.data.borrow_mut() {
        Data::Message(md) => {
            match &r {
                Ok(v) => {
                    md.state = 2;
                    md.result = v.clone();
                }
                Err(_) => md.state = 3,
            }
            core::mem::take(&mut md.notify)
        }
        _ => Vec::new(),
    };
    let r = r?;
    for n in notify {
        it.send(f, &n, b"SEND", Vec::new(), Start::Normal, lvl)?;
    }
    Ok(r)
}

fn message_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, lvl: u32) -> RV {
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    match name {
        b"INIT" => {
            let target = need(it, &args, 0)?;
            let mv = need(it, &args, 1)?;
            // the message name, or an array of name and class
            let (n, scope) = match array_items(&mv) {
                Some(a) => {
                    let n = a.first().cloned().flatten().map(|v| v.fast_string()).unwrap_or_default();
                    let s = a.get(1).cloned().flatten().and_then(|v| v.obj().cloned());
                    (n, s)
                }
                None => (it.to_str(f, &mv)?, None),
            };
            let rest = method_args(it, f, &args, 2)?;
            if let Data::Message(md) = &mut *o.data.borrow_mut() {
                md.target = target;
                md.name = upper(&n);
                md.scope = scope;
                md.args = rest;
            }
            Ok(None)
        }
        b"SEND" | b"START" => {
            if let Some(t) = arg(&args, 0) {
                if let Data::Message(md) = &mut *o.data.borrow_mut() {
                    md.target = t;
                }
            }
            let r = start_message(it, f, recv, lvl)?;
            Ok(if name == b"SEND" { r } else { None })
        }
        b"RESULT" => {
            let (state, res) = match &*o.data.borrow() {
                Data::Message(md) => (md.state, md.result.clone()),
                _ => (0, None),
            };
            if state == 0 {
                start_message(it, f, recv, lvl)?;
                let r = match &*o.data.borrow() {
                    Data::Message(md) => md.result.clone(),
                    _ => None,
                };
                return Ok(r);
            }
            if state == 3 {
                let s = o.fast_string();
                it.err_msg = msg_subst(b"The message object \"&1\" failed", &[&s]);
                return Err(Err::Syntax(98916));
            }
            Ok(res)
        }
        b"COMPLETED" => Ok(Some(bool_v(matches!(&*o.data.borrow(), Data::Message(md) if md.state >= 2)))),
        b"NOTIFY" => {
            let n = need(it, &args, 0)?;
            let done = matches!(&*o.data.borrow(), Data::Message(md) if md.state >= 2);
            if done {
                it.send(f, &n, b"SEND", Vec::new(), Start::Normal, lvl)?;
            } else if let Data::Message(md) = &mut *o.data.borrow_mut() {
                md.notify.push(n);
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

// ---- .Method ----

fn method_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    match name {
        b"INIT" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            let src = need(it, &args, 1)?;
            let m = to_method(it, f, &n, &src)?;
            *o.data.borrow_mut() = Data::Method(m);
            Ok(None)
        }
        b"SETGUARDED" | b"SETUNGUARDED" | b"SETPRIVATE" | b"SETPROTECTED" | b"SETUNPROTECTED" => {
            if let Data::Method(m) = &mut *o.data.borrow_mut() {
                match name {
                    b"SETGUARDED" => m.guarded = true,
                    b"SETUNGUARDED" => m.guarded = false,
                    b"SETPRIVATE" => m.private = true,
                    b"SETPROTECTED" => m.protected = true,
                    _ => m.protected = false,
                }
            }
            Ok(None)
        }
        b"SOURCE" => {
            let lines = match &*o.data.borrow() {
                Data::Method(Method { body: Body::Rexx(c), .. }) => c.source.iter().map(|l| Value::S(l.clone())).collect(),
                _ => Vec::new(),
            };
            Ok(Some(w.array(lines)))
        }
        _ => Ok(None),
    }
}

// ---- .Monitor ----

fn monitor_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, _found: &Found, lvl: u32) -> RV {
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let dest = || match &*o.data.borrow() {
        Data::Monitor(v) => v.clone(),
        _ => Value::S(Vec::new()),
    };
    match name {
        b"INIT" => {
            if let Some(v) = arg(&args, 0) {
                *o.data.borrow_mut() = Data::Monitor(v);
            }
            Ok(None)
        }
        b"CURRENT" => Ok(Some(dest())),
        b"DESTINATION" => {
            let old = dest();
            if let Some(v) = arg(&args, 0) {
                *o.data.borrow_mut() = Data::Monitor(v);
            }
            Ok(Some(old))
        }
        b"UNKNOWN" => {
            let n = upper(&str_arg(it, f, &args, 0)?);
            let a = arg(&args, 1).and_then(|a| array_items(&a)).unwrap_or_default();
            let d = dest();
            it.send(f, &d, &n, a, Start::Normal, lvl)
        }
        _ => Ok(None),
    }
}

// ---- .Stream ----

fn stream_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args) -> RV {
    let w = it.w.clone().unwrap();
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    let sname = match &*o.data.borrow() {
        Data::Stream(s) => s.name.clone(),
        _ => return Ok(None),
    };
    let mut sargs: Vec<Option<Val>> = alloc::vec![Some(sname.clone())];
    for a in &args {
        sargs.push(match a {
            Some(v) => Some(it.to_str(f, v)?),
            None => None,
        });
    }
    let cmd = |it: &mut Interp, f: &mut Frame, c: &[u8]| -> R<Val> { crate::stream::stream_fn(it, f, &[Some(sname.clone()), Some(b"C".to_vec()), Some(c.to_vec())]) };
    let s = |v: Val| Ok(Some(Value::S(v)));
    match name {
        b"INIT" => {
            let n = str_arg(it, f, &args, 0)?;
            if let Data::Stream(sd) = &mut *o.data.borrow_mut() {
                sd.name = n;
            }
            Ok(None)
        }
        b"OPEN" => {
            let mut c = b"OPEN".to_vec();
            if let Some(Some(opts)) = sargs.get(1) {
                c.push(b' ');
                c.extend_from_slice(opts);
            }
            s(cmd(it, f, &c)?)
        }
        b"CLOSE" => {
            let r = cmd(it, f, b"CLOSE")?;
            s(if r.is_empty() { b"READY:".to_vec() } else { r })
        }
        b"FLUSH" => s(b"READY:".to_vec()),
        b"COMMAND" => {
            let c = sargs.get(1).cloned().flatten().unwrap_or_default();
            s(cmd(it, f, &c)?)
        }
        b"QUERY" => {
            let mut c = b"QUERY ".to_vec();
            c.extend_from_slice(&sargs.get(1).cloned().flatten().unwrap_or_default());
            s(cmd(it, f, &c)?)
        }
        b"SEEK" | b"POSITION" => {
            let mut c = b"SEEK ".to_vec();
            c.extend_from_slice(&sargs.get(1).cloned().flatten().unwrap_or_default());
            s(cmd(it, f, &c)?)
        }
        b"QUALIFY" => {
            let q = it.host.f_exists(&sname).unwrap_or(sname.clone());
            s(q)
        }
        b"STATE" => s(crate::stream::stream_fn(it, f, &[Some(sname.clone()), Some(b"S".to_vec())])?),
        b"DESCRIPTION" => s(crate::stream::stream_fn(it, f, &[Some(sname.clone()), Some(b"D".to_vec())])?),
        b"LINEIN" => s(crate::stream::linein(it, f, &sargs)?),
        b"LINEOUT" => s(crate::stream::lineout(it, f, &sargs)?),
        b"LINES" => s(crate::stream::lines(it, f, &sargs)?),
        b"CHARS" => s(crate::stream::chars(it, f, &sargs)?),
        b"CHARIN" => s(crate::stream::charin(it, f, &sargs)?),
        b"CHAROUT" => s(crate::stream::charout(it, f, &sargs)?),
        b"ARRAYIN" | b"MAKEARRAY" | b"SUPPLIER" => {
            if name == b"SUPPLIER" {
                // a supplier goes through the stream from its start
                let _ = cmd(it, f, b"SEEK =1");
            }
            let chars = sargs.get(1).cloned().flatten().is_some_and(|o| o.first().is_some_and(|c| c.eq_ignore_ascii_case(&b'C')));
            let mut items = Vec::new();
            loop {
                let n = crate::stream::lines(it, f, &[Some(sname.clone())])?;
                if n.as_slice() == b"0" {
                    break;
                }
                if chars {
                    items.push(Value::S(crate::stream::charin(it, f, &[Some(sname.clone())])?));
                } else {
                    items.push(Value::S(crate::stream::linein(it, f, &[Some(sname.clone())])?));
                }
            }
            if name == b"SUPPLIER" {
                let idx = (1..=items.len()).map(|i| Value::S(int_str(i as i64))).collect();
                return Ok(Some(w.obj(Builtin::Supplier, Data::Supplier(SupplierData { items, indexes: idx, pos: 0 }))));
            }
            Ok(Some(w.array(items)))
        }
        b"ARRAYOUT" => {
            let a = need(it, &args, 0)?;
            let items = array_items(&a).unwrap_or_default();
            let chars = sargs.get(2).cloned().flatten().is_some_and(|o| o.first().is_some_and(|c| c.eq_ignore_ascii_case(&b'C')));
            for x in items.into_iter().flatten() {
                let v = it.to_str(f, &x)?;
                if chars {
                    crate::stream::charout(it, f, &[Some(sname.clone()), Some(v)])?;
                } else {
                    crate::stream::lineout(it, f, &[Some(sname.clone()), Some(v)])?;
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

// ---- .Alarm ----

fn alarm_m(it: &mut Interp, f: &mut Frame, recv: &Value, name: &[u8], args: Args, _lvl: u32) -> RV {
    let Some(o) = obj_of(recv).cloned() else { return Ok(None) };
    match name {
        b"INIT" => {
            let t = str_arg(it, f, &args, 0)?;
            let m = need(it, &args, 1)?;
            if Num::parse(&t).is_none() && !t.contains(&b':') {
                return Err(err93(it, 951, &[&t]));
            }
            // there is no other activity to run it later: the alarm is
            // kept but goes off only where the host runs activities
            *o.data.borrow_mut() = Data::Alarm(AlarmData { msg: m, cancelled: false });
            Ok(None)
        }
        b"CANCEL" => {
            if let Data::Alarm(a) = &mut *o.data.borrow_mut() {
                a.cancelled = true;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}
