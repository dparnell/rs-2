//! IBMLAN.INI: `[section]` lines and `key = value` lines, as IBM's
//! requester keeps its settings (`\IBMLAN\IBMLAN.INI` on the boot drive).
//! rs2 reads `[requester]` computername, domain, othdomains, and its own
//! wins, lmhosts, smbport and nbport.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn path() -> String {
    alloc::format!("{}:\\IBMLAN\\IBMLAN.INI", super::boot_drive())
}

#[derive(Clone, Debug, Default)]
pub struct Ini {
    /// Lines as read (kept for rewriting).
    lines: Vec<String>,
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        Ini { lines: text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect() }
    }

    pub fn load(path: &str) -> Ini {
        match utils::open_read(path) {
            Ok(h) => {
                let d = utils::read_all(h);
                unsafe { os2::dos::DosClose(h) };
                Ini::parse(&d.iter().map(|&b| b as char).collect::<String>())
            }
            Err(_) => Ini::default(),
        }
    }

    fn section_of(line: &str) -> Option<String> {
        let t = line.trim();
        t.strip_prefix('[').and_then(|s| s.strip_suffix(']')).map(|s| s.trim().to_ascii_lowercase())
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let mut cur = String::new();
        for l in &self.lines {
            if let Some(s) = Self::section_of(l) {
                cur = s;
                continue;
            }
            if !cur.eq_ignore_ascii_case(section) || l.trim_start().starts_with(';') {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim());
                }
            }
        }
        None
    }

    /// Set (or add) a value.
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let mut cur = String::new();
        let mut last_in_section = None;
        for (i, l) in self.lines.iter_mut().enumerate() {
            if let Some(s) = Self::section_of(l) {
                cur = s;
                continue;
            }
            if cur.eq_ignore_ascii_case(section) {
                last_in_section = Some(i);
                if let Some((k, _)) = l.split_once('=') {
                    if k.trim().eq_ignore_ascii_case(key) {
                        *l = alloc::format!("  {} = {}", k.trim(), value);
                        return;
                    }
                }
            }
        }
        let line = alloc::format!("  {key} = {value}");
        match last_in_section {
            Some(i) => self.lines.insert(i + 1, line),
            None => {
                let header = self.lines.iter().position(|l| Self::section_of(l).is_some_and(|s| s.eq_ignore_ascii_case(section)));
                match header {
                    Some(h) => self.lines.insert(h + 1, line),
                    None => {
                        self.lines.push(String::new());
                        self.lines.push(alloc::format!("[{section}]"));
                        self.lines.push(line);
                    }
                }
            }
        }
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            s.push_str(l);
            s.push_str("\r\n");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set() {
        let mut i = Ini::parse("[networks]\r\n  net1 = x\r\n[requester]\r\n  COMPUTERNAME = RS2\r\n");
        assert_eq!(i.get("requester", "computername"), Some("RS2"));
        i.set("requester", "domain", "WG");
        assert_eq!(i.get("REQUESTER", "DOMAIN"), Some("WG"));
        i.set("rs2", "wins", "1.2.3.4");
        assert_eq!(i.get("rs2", "wins"), Some("1.2.3.4"));
    }
}
