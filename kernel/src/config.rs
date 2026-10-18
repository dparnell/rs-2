//! CONFIG.SYS processing.
//!
//! Read from the root of the boot drive at start-up, as OS/2 does. The
//! statements that matter to the kernel today are applied; device driver
//! statements (BASEDEV, DEVICE, IFS) are recorded because native drivers
//! take their place.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Default, Debug)]
pub struct Config {
    pub libpath: Option<String>,
    /// SET statements, in order.
    pub env: Vec<String>,
    /// PROTSHELL program and arguments.
    pub protshell: Option<(String, String)>,
    /// CALL= programs run (synchronously) before the shell.
    pub call: Vec<(String, String)>,
    /// RUN= programs started detached.
    pub run: Vec<(String, String)>,
    pub codepage: Vec<u16>,
    pub pause_on_error: bool,
    /// Driver statements we do not load (BASEDEV/DEVICE/IFS).
    pub drivers: Vec<String>,
}

fn split_cmd(v: &str) -> (String, String) {
    let v = v.trim();
    match v.split_once(char::is_whitespace) {
        Some((p, a)) => (p.to_string(), a.trim().to_string()),
        None => (v.to_string(), String::new()),
    }
}

pub fn parse(text: &str) -> Config {
    let mut c = Config::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let up = line.to_ascii_uppercase();
        if up.starts_with("REM ") || up == "REM" {
            continue;
        }
        if let Some(rest) = up.strip_prefix("SET ") {
            // Keep the original case of the value.
            let _ = rest;
            let orig = line[4..].trim();
            if let Some((k, v)) = orig.split_once('=') {
                c.env.push(alloc::format!("{}={}", k.trim().to_ascii_uppercase(), v.trim()));
            }
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_uppercase();
        let val = val.trim();
        match key.as_str() {
            "LIBPATH" => c.libpath = Some(val.to_string()),
            "PROTSHELL" => c.protshell = Some(split_cmd(val)),
            "CALL" => c.call.push(split_cmd(val)),
            "RUN" => c.run.push(split_cmd(val)),
            "CODEPAGE" => {
                c.codepage = val.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            }
            "PAUSEONERROR" => c.pause_on_error = val.eq_ignore_ascii_case("YES"),
            "BASEDEV" | "DEVICE" | "IFS" => c.drivers.push(alloc::format!("{key}={val}")),
            _ => {}
        }
    }
    c
}

/// Read and parse CONFIG.SYS from drive C:, if present.
pub fn load() -> Option<Config> {
    let data = crate::fs::read_file("C:\\CONFIG.SYS")?;
    let text: String = data.iter().map(|&b| b as char).collect();
    Some(parse(&text))
}
