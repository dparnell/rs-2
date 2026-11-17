//! OMF linker: the code behind rs2's LINK.EXE (16-bit NE output) and
//! LINK386.EXE (32-bit LX output). `no_std` + `alloc`, so the OS/2 programs
//! (userland/link) and host tests share it; file access and the console go
//! through [`Host`].

#![no_std]

extern crate alloc;

pub mod cmd;
pub mod def;
mod defparse;
pub mod link;
pub mod lx;
pub mod map;
pub mod msg;
pub mod ne;
pub mod omf;
pub mod pack;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// What the linker needs from the system it runs on.
pub trait Host {
    /// A whole file, or None if it cannot be opened.
    fn read(&mut self, path: &str) -> Option<Vec<u8>>;
    fn write(&mut self, path: &str, data: &[u8]) -> bool;
    fn delete(&mut self, path: &str);
    fn env(&mut self, name: &str) -> Option<String>;
    /// Text to standard output.
    fn out(&mut self, s: &str);
    /// A line typed at a prompt (None at end of input).
    fn prompt(&mut self) -> Option<String>;
}

/// Which linker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prog {
    /// LINK.EXE: segmented (NE) executables.
    Link,
    /// LINK386.EXE: linear (LX) executables.
    Link386,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub lx: bool,
    pub dll: bool,
    pub case_sensitive: bool,
    pub information: bool,
    pub nod: bool,
    pub nod_libs: Vec<String>,
    pub pending_default_libs: Vec<String>,
    pub lib_dirs: Vec<String>,
    pub packcode: Option<u32>,
    pub packdata: Option<u32>,
    pub nonulls: bool,
    pub dosseg: bool,
    pub map: bool,
    pub map_full: bool,
    pub linenumbers: bool,
    pub stack: Option<u32>,
    pub align: Option<u32>,
    pub base: Option<u32>,
    pub pmtype: Option<def::AppType>,
    pub exepack: u8,
    pub debug: bool,
    pub farcall: bool,
    pub nologo: bool,
    pub batch: bool,
    pub noout_on_error: bool,
    pub runfromvdm: bool,
    pub warnfixup: bool,
    pub segments: Option<u32>,
}

/// Output file contents.
pub struct Output {
    pub exe: Vec<u8>,
    pub map: Option<String>,
}

use cmd::OptDef;

const OPTIONS: &[OptDef] = &[
    OptDef { name: "ALIGNMENT", min: 1 },
    OptDef { name: "BASE", min: 3 },
    OptDef { name: "BATCH", min: 3 },
    OptDef { name: "CODEVIEW", min: 2 },
    OptDef { name: "CPARMAXALLOC", min: 2 },
    OptDef { name: "DEBUG", min: 2 },
    OptDef { name: "DLL", min: 3 },
    OptDef { name: "DOSSEG", min: 2 },
    OptDef { name: "DSALLOCATE", min: 2 },
    OptDef { name: "EXEPACK", min: 1 },
    OptDef { name: "FARCALLTRANSLATION", min: 1 },
    OptDef { name: "HELP", min: 1 },
    OptDef { name: "HIGH", min: 2 },
    OptDef { name: "INCREMENTAL", min: 3 },
    OptDef { name: "INFORMATION", min: 1 },
    OptDef { name: "LINENUMBERS", min: 2 },
    OptDef { name: "MAP", min: 1 },
    OptDef { name: "NODEFAULTLIBRARYSEARCH", min: 3 },
    OptDef { name: "NOEXTDICTIONARY", min: 3 },
    OptDef { name: "NOFARCALLTRANSLATION", min: 3 },
    OptDef { name: "NOGROUPASSOCIATION", min: 3 },
    OptDef { name: "NOIGNORECASE", min: 3 },
    OptDef { name: "NOLOGO", min: 3 },
    OptDef { name: "NONULLSDOSSEG", min: 3 },
    OptDef { name: "NOOUTPUTONERROR", min: 3 },
    OptDef { name: "NOPACKCODE", min: 3 },
    OptDef { name: "NOSECTORALIGNCODE", min: 3 },
    OptDef { name: "OLDOVERLAY", min: 3 },
    OptDef { name: "OVERLAYINTERRUPT", min: 1 },
    OptDef { name: "PACKCODE", min: 5 },
    OptDef { name: "PACKDATA", min: 5 },
    OptDef { name: "PADCODE", min: 4 },
    OptDef { name: "PADDATA", min: 4 },
    OptDef { name: "PAUSE", min: 3 },
    OptDef { name: "PMTYPE", min: 2 },
    OptDef { name: "QUICKLIBRARY", min: 1 },
    OptDef { name: "RUNFROMVDM", min: 2 },
    OptDef { name: "SEGMENTS", min: 2 },
    OptDef { name: "STACK", min: 2 },
    OptDef { name: "TINY", min: 1 },
    OptDef { name: "WARNFIXUP", min: 1 },
];

fn help(prog: Prog) -> String {
    let mut s = String::from("Options (any unique abbreviation; numbers in C syntax):\r\n\r\n");
    let lines: &[&str] = &[
        "/ALIGNMENT:n      file alignment of segments or pages (default 512)",
        "/BASE:n           preferred load address (LINK386)",
        "/BATCH            do not prompt for missing files",
        "/CODEVIEW         keep debugging information (same as /DEBUG)",
        "/DEBUG            keep debugging information",
        "/DLL              make a dynamic link library",
        "/DOSSEG           order segments as DOS and OS/2 compilers expect",
        "/EXEPACK          compress the executable",
        "/FARCALLTRANSLATION  turn far calls within a segment into near calls",
        "/HELP or /?       show this list",
        "/INFORMATION      show what the linker is doing",
        "/LINENUMBERS      line numbers in the map file",
        "/MAP[:FULL]       public symbols in the map file",
        "/NODEFAULTLIBRARYSEARCH[:lib]  do not search the default libraries",
        "/NOEXTDICTIONARY  do not use extended dictionaries",
        "/NOFARCALLTRANSLATION  leave far calls alone (the default)",
        "/NOIGNORECASE     names are case sensitive",
        "/NOLOGO           do not show the sign-on line",
        "/NONULLSDOSSEG    /DOSSEG without the 16 null bytes in _TEXT",
        "/NOOUTPUTONERROR  write no executable when there are errors",
        "/NOPACKCODE       one segment per code segment",
        "/PACKCODE[:n]     pack code segments into segments up to n bytes",
        "/PACKDATA[:n]     pack data segments into segments up to n bytes",
        "/PAUSE            wait before writing the executable",
        "/PMTYPE:{PM|VIO|NOVIO}  application type",
        "/SEGMENTS:n       maximum number of segments",
        "/STACK:n          stack size in bytes",
        "/WARNFIXUP        warn about questionable fix-ups",
    ];
    for l in lines {
        if prog == Prog::Link && l.starts_with("/BASE") {
            continue;
        }
        s.push_str(l);
        s.push_str("\r\n");
    }
    s
}

fn banner(prog: Prog) -> String {
    let name = match prog {
        Prog::Link => "LINK",
        Prog::Link386 => "LINK386",
    };
    let what = match prog {
        Prog::Link => "segmented (NE)",
        Prog::Link386 => "linear (LX)",
    };
    format!("rs2 {name} - OMF linker for {what} executables, version 1.0\r\n\r\n")
}

fn apply_options(opts: &[String], o: &mut Options, prog: Prog, msgs: &mut msg::Msgs, host: &mut dyn Host) -> Result<bool, cmd::CmdError> {
    for g in opts {
        let d = match cmd::match_option(OPTIONS, g) {
            Ok(d) => d,
            Err(cmd::CmdError::Fatal(1002, _)) => {
                msgs.warn(host, "", 4017, &format!("/{g} : unknown option, ignored"));
                continue;
            }
            Err(e) => return Err(e),
        };
        let num = || cmd::opt_number(g);
        match d.name {
            "HELP" => return Ok(true),
            "ALIGNMENT" => {
                let n = num()?.unwrap_or(512);
                if !n.is_power_of_two() || n < 2 || n > 32768 {
                    msgs.warn(host, "", 4010, &format!("/{g} : alignment must be a power of 2; using 512"));
                    o.align = Some(512);
                } else {
                    o.align = Some(n);
                }
            }
            "BASE" => o.base = num()?,
            "BATCH" => o.batch = true,
            "CODEVIEW" | "DEBUG" => o.debug = true,
            "DLL" => o.dll = true,
            "DOSSEG" => o.dosseg = true,
            "EXEPACK" => o.exepack = num()?.unwrap_or(1) as u8,
            "FARCALLTRANSLATION" => o.farcall = true,
            "NOFARCALLTRANSLATION" => o.farcall = false,
            "INFORMATION" => o.information = true,
            "LINENUMBERS" => o.linenumbers = true,
            "MAP" => {
                o.map = true;
                if cmd::opt_value(g).is_some_and(|v| v.eq_ignore_ascii_case("FULL") || v.eq_ignore_ascii_case("F")) {
                    o.map_full = true;
                }
            }
            "NODEFAULTLIBRARYSEARCH" => match cmd::opt_value(g) {
                Some(l) => o.nod_libs.push(l),
                None => o.nod = true,
            },
            "NOIGNORECASE" => o.case_sensitive = true,
            "NOLOGO" => o.nologo = true,
            "NONULLSDOSSEG" => {
                o.dosseg = true;
                o.nonulls = true;
            }
            "NOOUTPUTONERROR" => o.noout_on_error = true,
            "NOPACKCODE" => o.packcode = None,
            "PACKCODE" => o.packcode = Some(num()?.unwrap_or(if prog == Prog::Link { 65500 } else { u32::MAX })),
            "PACKDATA" => o.packdata = Some(num()?.unwrap_or(if prog == Prog::Link { 65536 } else { u32::MAX })),
            "PMTYPE" => {
                o.pmtype = match cmd::opt_value(g).map(|v| v.to_ascii_uppercase()).as_deref() {
                    Some("PM") => Some(def::AppType::Pm),
                    Some("VIO") => Some(def::AppType::Vio),
                    Some("NOVIO") => Some(def::AppType::NoVio),
                    _ => {
                        msgs.warn(host, "", 4018, &format!("/{g} : application type must be PM, VIO or NOVIO; ignored"));
                        None
                    }
                }
            }
            "RUNFROMVDM" => o.runfromvdm = true,
            "SEGMENTS" => o.segments = num()?,
            "STACK" => {
                let n = num()?.unwrap_or(0);
                if prog == Prog::Link && n > 0xffff {
                    return Err(cmd::CmdError::Fatal(1006, format!("/{g} : the stack cannot be larger than 65535 bytes")));
                }
                o.stack = Some(n);
            }
            "WARNFIXUP" => o.warnfixup = true,
            // Real-mode and overlay options mean nothing for OS/2 modules.
            _ => msgs.warn(host, "", 4013, &format!("/{g} : option ignored for OS/2 executables")),
        }
    }
    Ok(false)
}

/// Run the linker with the program's command line; returns the exit code.
pub fn run(host: &mut dyn Host, prog: Prog, args: &str) -> i32 {
    let name = match prog {
        Prog::Link => "LINK",
        Prog::Link386 => "LINK386",
    };
    let mut msgs = msg::Msgs::new(name);
    let env_opts = host.env(name);
    let up = args.to_ascii_uppercase();
    let quiet = up.contains("/NOL") || up.contains("/BAT") || env_opts.as_deref().is_some_and(|e| {
        let e = e.to_ascii_uppercase();
        e.contains("/NOL") || e.contains("/BAT")
    });
    if !quiet {
        host.out(&banner(prog));
    }
    let trimmed = args.trim();
    if trimmed == "/?" || trimmed.eq_ignore_ascii_case("/HELP") || trimmed.eq_ignore_ascii_case("/H") {
        host.out(&help(prog));
        return 0;
    }
    let fields = match cmd::parse(host, args, env_opts.as_deref(), false) {
        Ok(f) => f,
        Err(cmd::CmdError::Fatal(n, t)) => {
            msgs.fatal(host, "", n, &t);
            return 4;
        }
    };
    let mut o = Options { lx: prog == Prog::Link386, ..Default::default() };
    if prog == Prog::Link386 {
        o.packcode = Some(u32::MAX);
    }
    match apply_options(&fields.opts, &mut o, prog, &mut msgs, host) {
        Ok(true) => {
            host.out(&help(prog));
            return 0;
        }
        Ok(false) => {}
        Err(cmd::CmdError::Fatal(n, t)) => {
            msgs.fatal(host, "", n, &t);
            return 4;
        }
    }
    // Library directories: those named in the libraries field, then LIB.
    let mut libs = Vec::new();
    for l in &fields.libs {
        if l.ends_with(['\\', '/', ':']) {
            o.lib_dirs.push(l.clone());
        } else {
            libs.push(l.clone());
        }
    }
    if let Some(e) = host.env("LIB") {
        for d in e.split(';').map(str::trim).filter(|d| !d.is_empty()) {
            o.lib_dirs.push(String::from(d));
        }
    }
    let r = run_link(host, prog, o, msgs, fields, libs);
    r
}

fn run_link(host: &mut dyn Host, prog: Prog, o: Options, msgs: msg::Msgs, fields: cmd::Fields, libs: Vec<String>) -> i32 {
    // The module definition file.
    let mut def = def::Def::default();
    if let Some(d) = fields.def.as_ref().filter(|d| !d.is_empty() && !d.eq_ignore_ascii_case("NUL")) {
        let p = cmd::with_ext(d, "DEF");
        let mut m = msgs;
        match cmd::find_file(host, &p, &o.lib_dirs) {
            Some((_, data)) => match def::Def::parse(&data) {
                Ok(d) => def = d,
                Err(e) => {
                    m.fatal(host, &format!("{p}({})", e.line), 1101, &format!("module definition file: {}", e.text));
                    return 4;
                }
            },
            None => {
                m.fatal(host, "", 1092, &format!("{p} : cannot open the module definition file"));
                return 4;
            }
        }
        return link_with(host, prog, o, m, fields, libs, def);
    }
    link_with(host, prog, o, msgs, fields, libs, def)
}

fn link_with(host: &mut dyn Host, prog: Prog, mut o: Options, msgs: msg::Msgs, fields: cmd::Fields, libs: Vec<String>, def: def::Def) -> i32 {
    if def.kind == def::Kind::Dll {
        o.dll = true;
    }
    let exe = match fields.exe.as_ref().filter(|e| !e.is_empty()) {
        Some(e) => cmd::with_ext(e, if o.dll { "DLL" } else { "EXE" }),
        None => cmd::set_ext(cmd::base_name(&fields.objs[0]), if o.dll { "DLL" } else { "EXE" }),
    };
    let mapname = match &fields.map {
        Some(m) if m.eq_ignore_ascii_case("NUL") || m.eq_ignore_ascii_case("NUL.MAP") => None,
        Some(m) if !m.is_empty() => Some(cmd::with_ext(m, "MAP")),
        Some(_) => Some(cmd::set_ext(&exe, "MAP")),
        None if o.map || o.linenumbers => Some(cmd::set_ext(cmd::base_name(&exe), "MAP")),
        None => None,
    };
    let mut l = link::Linker::new(host, o, msgs);
    l.def = def.clone();
    if l.opt.dosseg {
        l.dosseg = true;
    }
    // Imports named in the .DEF file.
    for i in &def.imports {
        let s = l.sym(&i.internal);
        l.syms[s].def = link::SymDef::Import { module: i.module.clone(), entry: i.entry.clone() };
    }
    for ob in &fields.objs {
        let p = cmd::with_ext(ob, "OBJ");
        let dirs = l.opt.lib_dirs.clone();
        let Some((path, data)) = cmd::find_file(l.host, &p, &dirs) else {
            l.msgs.fatal(l.host, "", 1093, &format!("{p} : object file not found"));
            return 4;
        };
        if l.opt.information {
            let s = format!("Reading {path}\r\n");
            l.host.out(&s);
        }
        if omf::is_library(&data) {
            if !l.add_whole_library(&path, &data) {
                return 4;
            }
            continue;
        }
        let mut off = 0;
        // An object file may hold several modules one after another.
        while off < data.len() && matches!(data[off], 0x80 | 0x82) {
            match omf::Object::parse(&data[off..]) {
                Ok(obj) => {
                    let size = obj.size;
                    let libs: Vec<String> = obj.libs.clone();
                    l.add_module(obj, path.clone());
                    for lb in libs {
                        l.want_default_library(&lb);
                    }
                    off += size;
                }
                Err(omf::Error::Eof) => {
                    l.msgs.fatal(l.host, &path, 1102, "unexpected end of file");
                    return 4;
                }
                Err(omf::Error::Invalid(t, w)) => {
                    l.msgs.fatal(l.host, &path, 1101, &format!("invalid object module (record {t:02X}H: {w})"));
                    return 4;
                }
            }
        }
    }
    for lb in &libs {
        let p = cmd::with_ext(lb, "LIB");
        let dirs = l.opt.lib_dirs.clone();
        match cmd::find_file(l.host, &p, &dirs) {
            Some((path, data)) => {
                if !l.add_library(path, data, false) {
                    return 4;
                }
            }
            None => l.msgs.warn(l.host, "", 4051, &format!("{p} : library not found")),
        }
    }
    loop {
        l.open_default_libraries();
        l.search_libraries();
        if l.msgs.fatal {
            return 4;
        }
        if l.opt.pending_default_libs.is_empty() {
            break;
        }
    }
    let Some(mut linked) = link::link(&mut l) else { return 4 };
    linked.module_name = match &def.name {
        Some(n) => n.to_ascii_uppercase(),
        None => {
            let b = cmd::base_name(&exe);
            let b = String::from(b.split('.').next().unwrap_or(b));
            // LINK386 names a module without NAME/LIBRARY after its file
            // in lower case, LINK in upper case.
            if prog == Prog::Link386 { b.to_ascii_lowercase() } else { b.to_ascii_uppercase() }
        }
    };
    // Without DESCRIPTION, LINK describes the module by its file name.
    linked.description = match &def.description {
        Some(d) => d.clone(),
        None if prog == Prog::Link => {
            let b = String::from(cmd::base_name(&exe));
            let dflt = if linked.dll { "DLL" } else { "EXE" };
            match b.rfind('.') {
                Some(i) if b[i + 1..].eq_ignore_ascii_case(dflt) => format!("{}.{dflt}", &b[..i]),
                _ => b,
            }
        }
        None => String::new(),
    };
    let errors = l.msgs.errors;
    let image = match prog {
        Prog::Link => ne::write(&mut l, &mut linked),
        Prog::Link386 => lx::write(&mut l, &mut linked),
    };
    let errors = errors.max(l.msgs.errors);
    if let Some(m) = &mapname {
        let text = map::write(&l, &linked, &exe);
        if !l.host.write(m, text.as_bytes()) {
            l.msgs.fatal(l.host, "", 1080, &format!("{m} : cannot open the list file"));
            return 4;
        }
    }
    if let Some(image) = image {
        if errors > 0 && l.opt.noout_on_error {
            l.host.delete(&exe);
        } else if !l.host.write(&exe, &image) {
            l.msgs.fatal(l.host, "", 1083, &format!("{exe} : cannot open the run file"));
            return 4;
        }
    }
    if let Some(s) = l.msgs.summary() {
        l.host.out(&s);
    }
    if l.msgs.fatal {
        4
    } else if l.msgs.errors > 0 {
        2
    } else {
        0
    }
}

/// Little-endian helpers for the writers.
pub(crate) fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
pub(crate) fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
pub(crate) fn pstr(v: &mut Vec<u8>, s: &str) {
    let b: Vec<u8> = s.chars().map(|c| c as u8).take(255).collect();
    v.push(b.len() as u8);
    v.extend_from_slice(&b);
}

#[allow(dead_code)]
fn unused(_: &dyn ToString) {}
