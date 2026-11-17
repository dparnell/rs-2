//! RC: the resource compiler (crates/os2-rc). As IBM's \OS2\RC.EXE:
//!
//! ```text
//! rc [options] file.rc [file.exe|file.dll]   compile and bind
//! rc [options] -r file.rc [file.res]         compile only
//! rc [options] file.res [file.exe|file.dll]  bind compiled resources
//! ```
//!
//! Options: -r, -d NAME[=VALUE] (or -dNAME), -i PATH, -p, -x[1|2], -cp CP,
//! -cc CC, -n, -w2, -h/-?. A script name without an extension gets .RC,
//! the module name .EXE; without a module name the script's (or .RES
//! file's) name with .EXE is used. The INCLUDE variable is searched after
//! -i. Icon 1 (or DEFAULTICON) also goes into the module's .ICON extended
//! attribute, the last ASSOCTABLE into .ASSOC. The return code is the
//! number of errors (1 for a usage or file error), 0 if all went well.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::*;
use os2_rc::bind::{BindError, BindOptions};
use os2_rc::{msg, res, Files, Options};
use utils::files::*;
use utils::*;
os2::entry!(main);

struct OsFiles;

impl Files for OsFiles {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let st = status(path)?;
        if st.attr & 0x10 != 0 {
            return None;
        }
        read_file(path).ok()
    }
}

fn say(s: &str) {
    os2::write_text(1, s);
}

/// A message by number, rs2's wording.
fn note(num: u16, arg: &str) {
    let d = os2_rc::Diag { num, file: String::new(), line: 0, arg: String::from(arg) };
    say(&d.format());
    say("\r\n");
}

fn usage() {
    say("Usage:  rc [<options>] <.RC input file> [<.EXE or .DLL output file>]\r\n");
    say("   or:  rc [<options>] <.RES input file> [<.EXE or .DLL output file>]\r\n");
    say("   or:  rc [<options>] -r <.RC input file> [<.RES output file>]\r\n");
    say("Options:\r\n");
    say("  -d name[=value]  define a macro (also -dname)\r\n");
    say("  -i path          add an include directory\r\n");
    say("  -r               compile to a .RES file only\r\n");
    say("  -p               keep 32-bit resources within 64K boundaries\r\n");
    say("  -x[1|2]          compress resources (method 1 or 2)\r\n");
    say("  -cp cp           code page of the script\r\n");
    say("  -cc cc           country code\r\n");
    say("  -n               no logo\r\n");
    say("  -w2              no warnings\r\n");
    say("  -h, -?           this help\r\n");
    say("Environment: INCLUDE=directories for #include, separated by ;\r\n");
}

fn env(name: &str) -> Option<String> {
    let c = os2::path::cstr(name);
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(c.as_ptr(), &mut v) } != 0 || v.is_null() {
        return None;
    }
    let mut s = String::new();
    unsafe {
        let mut p = v;
        while *p != 0 {
            s.push(*p as char);
            p = p.add(1);
        }
    }
    Some(s)
}

/// `name` with `ext` added if it has no extension.
fn with_ext(name: &str, ext: &str) -> String {
    let base = base_name(name);
    if base.contains('.') { String::from(name) } else { alloc::format!("{name}{ext}") }
}

/// `name` with its extension replaced by `ext`.
fn swap_ext(name: &str, ext: &str) -> String {
    let base = base_name(name);
    match base.rfind('.') {
        Some(p) => alloc::format!("{}{ext}", &name[..name.len() - base.len() + p]),
        None => alloc::format!("{name}{ext}"),
    }
}

fn ext_is(name: &str, ext: &str) -> bool {
    base_name(name).rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext))
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut opts = Options::default();
    let (mut res_only, mut logo, mut warnings) = (false, true, true);
    let mut bo = BindOptions::default();
    let mut files: Vec<String> = Vec::new();
    let mut help = a.is_empty();
    let mut i = 0;
    while i < a.len() {
        let x = &a[i];
        let is_opt = (x.starts_with('-') || x.starts_with('/')) && x.len() > 1;
        if !is_opt {
            files.push(x.clone());
            i += 1;
            continue;
        }
        let o = &x[1..];
        let lo = o.to_ascii_lowercase();
        let value = |i: &mut usize| -> Option<String> {
            *i += 1;
            a.get(*i).cloned()
        };
        match lo.as_str() {
            "r" => res_only = true,
            "n" => logo = false,
            "p" => bo.pack = true,
            "x" | "x1" => bo.compress = 1,
            "x2" => bo.compress = 2,
            "w2" => warnings = false,
            "h" | "?" => help = true,
            "i" => match value(&mut i) {
                Some(v) => opts.include.push(v),
                None => {
                    note(2002, x);
                    return 1;
                }
            },
            "cp" | "k" => match value(&mut i).and_then(|v| v.split(',').next().and_then(|n| n.parse::<u16>().ok())) {
                Some(cp) => opts.codepage = cp,
                None => {
                    note(2011, "");
                    return 1;
                }
            },
            "cc" => {
                if value(&mut i).is_none() {
                    note(2002, x);
                    return 1;
                }
            }
            _ if lo.starts_with('d') => {
                let def = if o.len() > 1 {
                    String::from(&o[1..])
                } else {
                    match value(&mut i) {
                        Some(v) => v,
                        None => {
                            note(2002, x);
                            return 1;
                        }
                    }
                };
                let (n, v) = def.split_once('=').unwrap_or((&def, ""));
                opts.defines.push((String::from(n), String::from(v)));
            }
            _ if lo.starts_with('i') => opts.include.push(String::from(&o[1..])),
            _ => {
                note(2003, x);
                return 1;
            }
        }
        i += 1;
    }
    if logo {
        say("rs2 Resource Compiler for OS/2, version 1.0\r\n\r\n");
    }
    if help {
        usage();
        return 1;
    }
    if files.is_empty() {
        note(1004, "");
        return 1;
    }
    if files.len() > 2 {
        note(1007, "");
        return 1;
    }
    if let Some(inc) = env("INCLUDE") {
        opts.include.extend(inc.split(';').filter(|p| !p.is_empty()).map(String::from));
    }
    if opts.codepage == 0 {
        let (mut cp, mut cb) = (0u32, 0u32);
        if unsafe { DosQueryCp(4, &mut cp, &mut cb) } == 0 && cp != 0 {
            opts.codepage = cp as u16;
        }
    }
    let input = files[0].clone();
    let output = files.get(1).cloned();
    let _ = warnings;

    // A .RES file to bind?
    let is_res_file = ext_is(&input, "RES") || (!ext_is(&input, "RC") && read_file(&input).is_ok_and(|d| res::is_res(&d)));
    if is_res_file && !res_only {
        let Ok(data) = read_file(&input) else {
            note(1001, &input);
            return 1;
        };
        let Some(resources) = res::read(&data) else {
            note(2027, &input);
            return 1;
        };
        let exe = with_ext(&output.unwrap_or_else(|| swap_ext(&input, ".EXE")), ".EXE");
        return bind_into(&exe, &resources, None, None, bo);
    }

    // Compile.
    let script = with_ext(&input, ".RC");
    let res_name = if res_only { output.clone().map(|o| with_ext(&o, ".RES")).unwrap_or_else(|| swap_ext(&script, ".RES")) } else { swap_ext(&script, ".RES") };
    if res_name.eq_ignore_ascii_case(&script) {
        note(1008, &script);
        return 1;
    }
    say(&alloc::format!("Creating binary resource file {res_name}\r\n"));
    let out = os2_rc::compile(&mut OsFiles, &script, &opts);
    let mut errors = 0u32;
    for d in &out.diags {
        let sev = d.severity();
        if sev < os2_rc::Severity::Error && !warnings {
            continue;
        }
        if sev >= os2_rc::Severity::Error {
            errors += 1;
        }
        say(&d.format());
        say("\r\n");
    }
    if errors > 0 {
        say(&alloc::format!("RC: {errors} error(s); {}\r\n", msg::expand(1009, "")));
        return errors;
    }
    let data = res::write(&out.resources);
    if let Err(rc) = write_file(&res_name, &data) {
        note(2028, &res_name);
        return rc.min(255).max(1);
    }
    if res_only {
        return 0;
    }
    let exe = with_ext(&output.unwrap_or_else(|| swap_ext(&script, ".EXE")), ".EXE");
    bind_into(&exe, &out.resources, out.default_icon.as_deref(), out.assoc.as_deref(), bo)
}

/// Binds `resources` into the module `exe` (rewritten in place) and sets
/// its .ICON / .ASSOC extended attributes.
fn bind_into(exe: &str, resources: &[res::Resource], icon: Option<&[u8]>, assoc: Option<&[u8]>, bo: BindOptions) -> u32 {
    let Ok(old) = read_file(exe) else {
        note(1001, exe);
        return 1;
    };
    if os2_rc::bind::is_ne(&old) && bo.compress != 0 {
        note(3011, exe);
    }
    if os2_rc::bind::is_ne(&old) && bo.pack {
        note(3012, exe);
    }
    let new = match os2_rc::bind::bind(&old, resources, bo) {
        Ok(n) => n,
        Err(BindError::Format) => {
            note(2031, exe);
            return 1;
        }
        Err(_) => {
            note(2026, exe);
            return 1;
        }
    };
    say(&alloc::format!("Writing {} resource(s) to {exe}\r\n", resources.len()));
    // Keep the module's own extended attributes.
    let eas = read_eas(exe).unwrap_or_default();
    if let Err(rc) = write_file(exe, &new) {
        note(2026, exe);
        return rc.min(255).max(1);
    }
    let mut eas: Vec<Ea> = eas.into_iter().filter(|e| !e.name.eq_ignore_ascii_case(b".ICON") && !e.name.eq_ignore_ascii_case(b".ASSOC")).collect();
    // Icon 1 from a .RES file being bound counts as the default icon too.
    let icon = icon.or_else(|| resources.iter().find(|r| r.ty == 22 && r.name == os2_rc::Name::Id(22)).map(|r| &r.data[..]));
    if let Some(i) = icon {
        let mut v = Vec::new();
        v.extend_from_slice(&0xfff9u16.to_le_bytes());
        v.extend_from_slice(&(i.len() as u16).to_le_bytes());
        v.extend_from_slice(i);
        eas.push(Ea { flags: 0, name: b".ICON".to_vec(), value: v });
    }
    if let Some(t) = assoc {
        let mut v = Vec::new();
        v.extend_from_slice(&0xffdeu16.to_le_bytes());
        v.extend_from_slice(&(t.len() as u16).to_le_bytes());
        v.extend_from_slice(t);
        eas.push(Ea { flags: 0, name: b".ASSOC".to_vec(), value: v });
    }
    if !eas.is_empty() && write_eas(exe, &eas) != 0 {
        note(2021, exe);
    }
    0
}
