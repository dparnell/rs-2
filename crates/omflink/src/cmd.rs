//! IBM's command-line syntax: `objs,exe,map,libs,def;`, prompts for the
//! fields not given, response files (`@file`), options anywhere (`/NOD`,
//! `/ST:4096`, ...) and the LINK / LINK386 environment variables.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::Host;

/// File-name comparison as OS/2 does it (case-insensitive).
pub fn same_file(a: &str, b: &str) -> bool {
    base_name(a).eq_ignore_ascii_case(base_name(b)) && ext_of(a).eq_ignore_ascii_case(ext_of(b))
}

/// The name without drive and directories.
pub fn base_name(p: &str) -> &str {
    let i = p.rfind(['\\', '/', ':']).map(|i| i + 1).unwrap_or(0);
    &p[i..]
}

fn ext_of(p: &str) -> &str {
    let b = base_name(p);
    match b.rfind('.') {
        Some(i) => &b[i + 1..],
        None => "",
    }
}

/// Add `ext` unless the name has an extension (a trailing '.' means none).
pub fn with_ext(p: &str, ext: &str) -> String {
    let b = base_name(p);
    if b.ends_with('.') {
        return String::from(&p[..p.len() - 1]);
    }
    if b.contains('.') { String::from(p) } else { format!("{p}.{ext}") }
}

/// Replace the extension.
pub fn set_ext(p: &str, ext: &str) -> String {
    let b = base_name(p);
    let stem = match b.rfind('.') {
        Some(i) => &p[..p.len() - b.len() + i],
        None => p,
    };
    format!("{stem}.{ext}")
}

fn has_dir(p: &str) -> bool {
    p.contains(['\\', '/', ':'])
}

/// Search for a file: as given, then (without a path in the name) the
/// current directory, the directories named on the command line and LIB.
pub fn find_file(host: &mut dyn Host, name: &str, dirs: &[String]) -> Option<(String, Vec<u8>)> {
    if let Some(d) = host.read(name) {
        return Some((String::from(name), d));
    }
    if has_dir(name) {
        // IBM's linkers also try the bare name along the paths.
    }
    let bare = base_name(name);
    for d in dirs {
        let mut p = String::from(d.as_str());
        if !p.ends_with(['\\', '/', ':']) {
            p.push('\\');
        }
        p.push_str(bare);
        if let Some(data) = host.read(&p) {
            return Some((p, data));
        }
    }
    None
}

#[derive(Debug, Clone, Default)]
pub struct Fields {
    pub objs: Vec<String>,
    pub exe: Option<String>,
    /// None: no map field; Some(""): the default name.
    pub map: Option<String>,
    pub libs: Vec<String>,
    pub def: Option<String>,
    pub opts: Vec<String>,
}

#[derive(Debug)]
pub enum CmdError {
    /// Message number and text.
    Fatal(u32, String),
}

/// Split one line of input into items: options, separators, names.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Name(String),
    Opt(String),
    Comma,
    Semi,
    /// A `+` at the end of a line: the field continues.
    Plus,
    Newline,
}

fn lex(s: &str, out: &mut Vec<Item>) {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            ' ' | '\t' | '\r' | '\x1a' => i += 1,
            '\n' => {
                out.push(Item::Newline);
                i += 1;
            }
            ',' => {
                out.push(Item::Comma);
                i += 1;
            }
            ';' => {
                out.push(Item::Semi);
                i += 1;
            }
            '+' => {
                out.push(Item::Plus);
                i += 1;
            }
            '/' => {
                let mut o = String::new();
                i += 1;
                while i < c.len() && !matches!(c[i], ' ' | '\t' | '\r' | '\n' | ',' | ';' | '+' | '/') {
                    o.push(c[i]);
                    i += 1;
                }
                out.push(Item::Opt(o));
            }
            '"' => {
                let mut o = String::new();
                i += 1;
                while i < c.len() && c[i] != '"' {
                    o.push(c[i]);
                    i += 1;
                }
                i += 1;
                out.push(Item::Name(o));
            }
            _ => {
                let mut o = String::new();
                while i < c.len() && !matches!(c[i], ' ' | '\t' | '\r' | '\n' | ',' | ';' | '+' | '/' | '"') {
                    o.push(c[i]);
                    i += 1;
                }
                out.push(Item::Name(o));
            }
        }
    }
}

const PROMPTS: [&str; 5] = ["Object Modules [.OBJ]: ", "Run File [{}]: ", "List File [{}]: ", "Libraries [.LIB]: ", "Definitions File [NUL.DEF]: "];

/// Expand `@file` response files (one level) into text; their line ends
/// separate fields as a prompt's Enter does.
fn expand_responses(host: &mut dyn Host, s: &str, nested: bool) -> Result<String, CmdError> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('@') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find([' ', '\t', '\r', '\n', ',', ';', '+']).unwrap_or(after.len());
        let name = &after[..end];
        if nested {
            return Err(CmdError::Fatal(1021, String::from("response files cannot be nested")));
        }
        let data = host
            .read(name)
            .ok_or_else(|| CmdError::Fatal(1089, format!("{name} : cannot open the response file")))?;
        let text: String = data.iter().map(|&b| b as char).collect();
        let text = expand_responses(host, &text, true)?;
        // The response file's text, as typed, its line ends separating fields.
        out.push_str(text.trim_end_matches(['\r', '\n', '\x1a']));
        rest = &after[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Parse the command line (prompting for missing fields when allowed).
pub fn parse(host: &mut dyn Host, args: &str, env_opts: Option<&str>, batch_default: bool) -> Result<Fields, CmdError> {
    let mut f = Fields::default();
    let mut items = Vec::new();
    if let Some(e) = env_opts {
        let mut ei = Vec::new();
        lex(e, &mut ei);
        for it in ei {
            match it {
                Item::Opt(o) => f.opts.push(o),
                Item::Name(n) => return Err(CmdError::Fatal(1002, format!("{n} : unrecognized option name"))),
                _ => {}
            }
        }
    }
    let text = expand_responses(host, args, false)?;
    lex(&text, &mut items);
    let mut st = State::default();
    st.feed(&items, &mut f.opts);
    let map_given = st.field >= 2;
    let batch = batch_default || f.opts.iter().any(|o| o.to_ascii_uppercase().starts_with("BA"));
    let mut have = st.field + if st.vals.get(st.field).is_some_and(|v| !v.is_empty()) { 1 } else { 0 };
    let mut prompted_map_empty = false;
    if !st.ended && !batch {
        while have < 5 {
            let first_obj = st.vals[0].first().cloned().unwrap_or_default();
            let exe_def = st.vals[1].first().cloned().unwrap_or_else(|| set_ext(base_name(&first_obj), "EXE"));
            let p = match have {
                1 => PROMPTS[1].replace("{}", &exe_def.to_ascii_uppercase()),
                2 => PROMPTS[2].replace("{}", "NUL.MAP"),
                n => String::from(PROMPTS[n]),
            };
            host.out(&p);
            let Some(line) = host.prompt() else {
                break;
            };
            let mut li = Vec::new();
            lex(&line, &mut li);
            st.field = have;
            st.cont = false;
            st.feed(&li, &mut f.opts);
            if st.cont {
                // '+' at the end of the line: the same prompt again.
                continue;
            }
            if have == 0 && st.vals[0].is_empty() && !st.ended {
                continue;
            }
            if have == 2 && st.field == 2 && st.vals[2].is_empty() {
                prompted_map_empty = true;
            }
            have = st.field + 1;
            if st.ended {
                break;
            }
        }
    }
    if st.vals[0].is_empty() {
        return Err(CmdError::Fatal(1020, String::from("no object modules were given")));
    }
    f.objs = st.vals[0].clone();
    f.exe = st.vals[1].first().cloned();
    f.map = if !st.vals[2].is_empty() {
        st.vals[2].first().cloned()
    } else if map_given && !prompted_map_empty {
        Some(String::new())
    } else {
        None
    };
    f.libs = st.vals[3].clone();
    f.def = st.vals[4].first().cloned();
    Ok(f)
}

#[derive(Default)]
struct State {
    field: usize,
    vals: [Vec<String>; 5],
    ended: bool,
    cont: bool,
}

impl State {
    fn feed(&mut self, items: &[Item], opts: &mut Vec<String>) {
        for it in items {
            match it {
                Item::Opt(o) => opts.push(o.clone()),
                _ if self.ended => {}
                Item::Name(n) => {
                    if self.field < 5 {
                        self.vals[self.field].push(n.clone());
                    }
                    self.cont = false;
                }
                Item::Comma => {
                    self.field += 1;
                    self.cont = false;
                }
                Item::Semi => self.ended = true,
                Item::Plus => self.cont = true,
                // Response files: each line answers one prompt.
                Item::Newline => {
                    if !self.cont {
                        self.field += 1;
                    }
                    self.cont = false;
                }
            }
        }
    }
}

/// One option of the table: full name, shortest abbreviation.
pub struct OptDef {
    pub name: &'static str,
    pub min: usize,
}

/// Look up an option by its (possibly abbreviated) name.
pub fn match_option<'a>(table: &'a [OptDef], given: &str) -> Result<&'a OptDef, CmdError> {
    let (name, _) = given.split_once(':').unwrap_or((given, ""));
    let u = name.to_ascii_uppercase();
    if u.is_empty() {
        return Err(CmdError::Fatal(1002, format!("/{given} : unrecognized option name")));
    }
    if u == "?" {
        if let Some(h) = table.iter().find(|o| o.name == "HELP") {
            return Ok(h);
        }
    }
    let hits: Vec<&OptDef> = table.iter().filter(|o| o.name.starts_with(&u) && u.len() >= o.min.min(o.name.len())).collect();
    if let Some(exact) = hits.iter().find(|o| o.name == u) {
        return Ok(exact);
    }
    match hits.len() {
        1 => Ok(hits[0]),
        0 => {
            // Shorter than the minimum but unique?
            let loose: Vec<&OptDef> = table.iter().filter(|o| o.name.starts_with(&u)).collect();
            if loose.len() > 1 {
                Err(CmdError::Fatal(1001, format!("/{given} : option name ambiguous")))
            } else {
                Err(CmdError::Fatal(1002, format!("/{given} : unrecognized option name")))
            }
        }
        _ => Err(CmdError::Fatal(1001, format!("/{given} : option name ambiguous"))),
    }
}

/// The value after `:` as a number (C syntax).
pub fn opt_number(given: &str) -> Result<Option<u32>, CmdError> {
    match given.split_once(':') {
        None => Ok(None),
        Some((_, v)) => crate::def::number(v)
            .map(Some)
            .ok_or_else(|| CmdError::Fatal(1004, format!("/{given} : invalid numeric value"))),
    }
}

pub fn opt_value(given: &str) -> Option<String> {
    given.split_once(':').map(|(_, v)| v.to_string())
}
