//! Command line and .DEF parsing.
use omflink::cmd;
use omflink::def::{AppType, Def, Kind};
use omflink::omf::ImpEntry;
use std::collections::HashMap;

#[derive(Default)]
struct Mem {
    files: HashMap<String, Vec<u8>>,
    input: Vec<String>,
    out: String,
}

impl omflink::Host for Mem {
    fn read(&mut self, p: &str) -> Option<Vec<u8>> {
        self.files.get(&p.to_ascii_uppercase()).cloned()
    }
    fn write(&mut self, p: &str, d: &[u8]) -> bool {
        self.files.insert(p.to_ascii_uppercase(), d.to_vec());
        true
    }
    fn delete(&mut self, p: &str) {
        self.files.remove(&p.to_ascii_uppercase());
    }
    fn env(&mut self, _: &str) -> Option<String> {
        None
    }
    fn out(&mut self, s: &str) {
        self.out.push_str(s);
    }
    fn prompt(&mut self) -> Option<String> {
        if self.input.is_empty() { None } else { Some(self.input.remove(0)) }
    }
}

fn parse(s: &str) -> cmd::Fields {
    let mut h = Mem::default();
    cmd::parse(&mut h, s, None, false).unwrap()
}

#[test]
fn all_fields() {
    let f = parse("exe16.obj,exe16.exe,exe16.map,E:\\LIB\\os2.lib dll32.lib,exe16.def");
    assert_eq!(f.objs, ["exe16.obj"]);
    assert_eq!(f.exe.as_deref(), Some("exe16.exe"));
    assert_eq!(f.map.as_deref(), Some("exe16.map"));
    assert_eq!(f.libs, ["E:\\LIB\\os2.lib", "dll32.lib"]);
    assert_eq!(f.def.as_deref(), Some("exe16.def"));
}

#[test]
fn map_placeholder() {
    // "FUN,,;" makes FUN.MAP; "FUN,;" and "FUN;" make none.
    assert_eq!(parse("fun,,;").map.as_deref(), Some(""));
    assert_eq!(parse("fun,;").map, None);
    assert_eq!(parse("fun;").map, None);
    let f = parse("fun+text+table+care, ,funlist, xlib.lib;");
    assert_eq!(f.objs.len(), 4);
    assert_eq!(f.map.as_deref(), Some("funlist"));
}

#[test]
fn options_anywhere() {
    let f = parse("/NOD a /map b,c /ST:4096;");
    assert_eq!(f.objs, ["a", "b"]);
    assert_eq!(f.exe.as_deref(), Some("c"));
    assert_eq!(f.opts, ["NOD", "map", "ST:4096"]);
}

#[test]
fn prompts() {
    let mut h = Mem::default();
    h.input = vec!["fun text".into(), "".into(), "".into(), "c:\\testlib\\ newlibv3".into(), "".into()];
    let f = cmd::parse(&mut h, "", None, false).unwrap();
    assert_eq!(f.objs, ["fun", "text"]);
    assert_eq!(f.exe, None);
    assert_eq!(f.map, None);
    assert_eq!(f.libs, ["c:\\testlib\\", "newlibv3"]);
    assert!(h.out.contains("Run File [FUN.EXE]: "), "{}", h.out);
    assert!(h.out.contains("List File [NUL.MAP]: "));
    // A semicolon takes the defaults for the rest.
    let mut h = Mem::default();
    h.input = vec!["a;".into()];
    let f = cmd::parse(&mut h, "", None, false).unwrap();
    assert_eq!(f.objs, ["a"]);
    assert!(!h.out.contains("Run File"));
}

#[test]
fn response_file() {
    let mut h = Mem::default();
    h.files.insert("FUN.LNK".into(), b"FUN TEXT TABLE CARE\r\n/DEBUG /MAP\r\nFUNLIST\r\nGRAF.LIB;\r\n".to_vec());
    let f = cmd::parse(&mut h, "@FUN.LNK", None, false).unwrap();
    assert_eq!(f.objs.len(), 4);
    assert_eq!(f.exe, None);
    assert_eq!(f.map.as_deref(), Some("FUNLIST"));
    assert_eq!(f.libs, ["GRAF.LIB"]);
    assert_eq!(f.opts, ["DEBUG", "MAP"]);
}

#[test]
fn option_names() {
    let t = &[cmd::OptDef { name: "NODEFAULTLIBRARYSEARCH", min: 3 }, cmd::OptDef { name: "NOEXTDICTIONARY", min: 3 }];
    assert_eq!(cmd::match_option(t, "NOD").unwrap().name, "NODEFAULTLIBRARYSEARCH");
    assert_eq!(cmd::match_option(t, "noe").unwrap().name, "NOEXTDICTIONARY");
    assert!(cmd::match_option(t, "NO").is_err());
}

#[test]
fn def_free_form() {
    let d = Def::parse(b"NAME EXE16\r\nWINDOWCOMPAT\r\nSTACKSIZE 4096\r\n").unwrap();
    assert_eq!(d.name.as_deref(), Some("EXE16"));
    assert_eq!(d.app, AppType::Vio);
    assert_eq!(d.stacksize, Some(4096));
}

#[test]
fn def_dll() {
    let d = Def::parse(
        b"LIBRARY MYDLL INITINSTANCE TERMINSTANCE\n\
          DESCRIPTION 'My ''own'' DLL'\n\
          PROTMODE\n\
          DATA MULTIPLE NONSHARED\n\
          CODE LOADONCALL\n\
          SEGMENTS\n  'IOSEG' CLASS 'CODE' IOPL\n  _DATA SHARED\n\
          EXPORTS\n  Func1 @1\n  Func2 = _func2 @5 RESIDENTNAME\n  Hidden @7 NONAME\n  IoFunc @8 2\n  PRIV PRIVATE\n\
          IMPORTS\n  DosBeep = DOSCALLS.286\n  PMWIN.WinInitialize\n  my = OTHER.Name ; comment\n",
    )
    .unwrap();
    assert_eq!(d.kind, Kind::Dll);
    assert_eq!(d.init_instance, Some(true));
    assert_eq!(d.term_instance, Some(true));
    assert_eq!(d.description.as_deref(), Some("My 'own' DLL"));
    assert_eq!(d.data.instance, Some(2));
    assert_eq!(d.data.shared, Some(false));
    assert_eq!(d.segments.len(), 2);
    assert_eq!(d.segments[0].class.as_deref(), Some("CODE"));
    assert_eq!(d.segments[0].attrs.iopl, Some(true));
    assert_eq!(d.exports.len(), 5);
    assert_eq!(d.exports[1].internal, "_func2");
    assert!(d.exports[1].resident);
    assert_eq!(d.exports[1].ordinal, Some(5));
    assert!(d.exports[2].noname);
    assert_eq!(d.exports[3].pwords, 2);
    assert!(d.exports[4].private);
    assert_eq!(d.imports.len(), 3);
    assert_eq!(d.imports[0].entry, ImpEntry::Ordinal(286));
    assert_eq!(d.imports[1].internal, "WinInitialize");
    assert_eq!(d.imports[2].entry, ImpEntry::Name("Name".into()));
}
