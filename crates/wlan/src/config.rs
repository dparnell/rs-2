//! `\MPTN\ETC\WIFI.CFG`: the networks rs2 may join, in the order it
//! prefers them. A plain text file in the style of CONFIG.SYS:
//!
//! ```text
//! ; Networks rs2 joins (the first one found wins).
//! SSID=Home Network
//! PASSPHRASE=correct horse battery staple
//!
//! SSID=Cafe
//! ```
//!
//! Lines before the first `SSID=` are settings for every network:
//! `POWERSAVE=ON` lets the chip sleep between beacons (off when the line
//! is missing or says OFF).
//!
//! Each `SSID=` line starts a network; the lines after it until the
//! next `SSID=` belong to it: `PASSPHRASE=` (8-63 characters; none for
//! an open network), `BSSID=` (join only that access point),
//! `AUTOCONNECT=NO` (only join when asked). Names are case-insensitive;
//! values run to the end of the line and may be quoted to keep leading
//! or trailing blanks. Lines starting with `;`, `#` or `REM` are
//! comments.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub ssid: Vec<u8>,
    pub passphrase: Option<String>,
    pub bssid: Option<[u8; 6]>,
    pub autoconnect: bool,
}

impl Profile {
    pub fn new(ssid: &[u8], passphrase: Option<&str>) -> Profile {
        Profile { ssid: ssid.to_vec(), passphrase: passphrase.map(String::from), bssid: None, autoconnect: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Config {
    pub profiles: Vec<Profile>,
    /// `POWERSAVE=`: None when the file does not say.
    pub powersave: Option<bool>,
    /// Lines that could not be used: (line number, why).
    pub errors: Vec<(usize, String)>,
}

fn value(v: &str) -> &str {
    let v = v.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\r');
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') { &v[1..v.len() - 1] } else { v }
}

pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut m = [0u8; 6];
    let mut parts = s.split([':', '-']);
    for b in &mut m {
        *b = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(m)
}

/// The file's text (one byte per character, as rs2 reads OS/2 text:
/// SSIDs are kept as the bytes found).
pub fn parse(text: &str) -> Config {
    let mut c = Config::default();
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        let t = line.trim_start();
        let upper = t.get(..4).map(|s| s.to_ascii_uppercase());
        if t.is_empty() || t.starts_with(';') || t.starts_with('#') || upper.as_deref() == Some("REM ") || t.eq_ignore_ascii_case("rem") {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else {
            c.errors.push((n, "no '='".to_string()));
            continue;
        };
        let k = k.trim().to_ascii_uppercase();
        let v = value(v);
        if k == "SSID" {
            if v.is_empty() || v.len() > 32 {
                c.errors.push((n, "an SSID is 1 to 32 characters".to_string()));
                // Its other lines must not attach to the network before.
                c.profiles.push(Profile { autoconnect: false, ..Profile::new(b"", None) });
            } else {
                c.profiles.push(Profile::new(&v.chars().map(|ch| ch as u32 as u8).collect::<Vec<u8>>(), None));
            }
            continue;
        }
        let Some(p) = c.profiles.last_mut() else {
            match k.as_str() {
                "POWERSAVE" => match on_off(v) {
                    Some(b) => c.powersave = Some(b),
                    None => c.errors.push((n, "POWERSAVE is ON or OFF".to_string())),
                },
                _ => c.errors.push((n, alloc::format!("{k} before the first SSID="))),
            }
            continue;
        };
        match k.as_str() {
            "PASSPHRASE" | "PASSWORD" | "PSK" | "KEY" => {
                if v.is_empty() {
                    p.passphrase = None;
                } else if !(8..=63).contains(&v.len()) {
                    c.errors.push((n, "a passphrase is 8 to 63 characters".to_string()));
                } else {
                    p.passphrase = Some(v.to_string());
                }
            }
            "BSSID" => match parse_mac(v) {
                Some(m) => p.bssid = Some(m),
                None => c.errors.push((n, "BSSID is six hex bytes, aa:bb:cc:dd:ee:ff".to_string())),
            },
            "AUTOCONNECT" => p.autoconnect = on_off(v) != Some(false),
            _ => c.errors.push((n, alloc::format!("unknown setting {k}"))),
        }
    }
    c.profiles.retain(|p| !p.ssid.is_empty());
    c
}

fn on_off(v: &str) -> Option<bool> {
    match v.to_ascii_uppercase().as_str() {
        "ON" | "YES" | "1" | "TRUE" => Some(true),
        "OFF" | "NO" | "0" | "FALSE" => Some(false),
        _ => None,
    }
}

/// The file text for `profiles` (CR LF line ends), without settings.
pub fn write(profiles: &[Profile]) -> String {
    write_config(&Config { profiles: profiles.to_vec(), ..Config::default() })
}

/// The file text for a whole configuration: settings, then networks.
pub fn write_config(c: &Config) -> String {
    let profiles = &c.profiles;
    let mut s = String::from("; Networks rs2 joins: the first one found wins (WIFI CONNECT adds them).\r\n");
    if let Some(on) = c.powersave {
        s += &alloc::format!("POWERSAVE={}\r\n", if on { "ON" } else { "OFF" });
    }
    for p in profiles {
        let ssid: String = p.ssid.iter().map(|&b| b as char).collect();
        let quote = |v: &str| if v.starts_with([' ', '\t', '"']) || v.ends_with([' ', '\t', '"']) { alloc::format!("\"{v}\"") } else { v.to_string() };
        s += &alloc::format!("\r\nSSID={}\r\n", quote(&ssid));
        if let Some(k) = &p.passphrase {
            s += &alloc::format!("PASSPHRASE={}\r\n", quote(k));
        }
        if let Some(b) = p.bssid {
            s += &alloc::format!("BSSID={}\r\n", crate::MacFmt(b));
        }
        if !p.autoconnect {
            s += "AUTOCONNECT=NO\r\n";
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profiles() {
        let c = parse(
            "; comment\r\nREM another\r\nSSID=Home Network\r\npassphrase=correct horse\r\n\r\nSSID=\" Cafe \"\r\nBSSID=02:00:00:aa:bb:cc\r\nAUTOCONNECT=no\r\n",
        );
        assert_eq!(c.errors, alloc::vec![]);
        assert_eq!(c.profiles.len(), 2);
        assert_eq!(c.profiles[0].ssid, b"Home Network");
        assert_eq!(c.profiles[0].passphrase.as_deref(), Some("correct horse"));
        assert!(c.profiles[0].autoconnect);
        assert_eq!(c.profiles[1].ssid, b" Cafe ");
        assert_eq!(c.profiles[1].passphrase, None);
        assert_eq!(c.profiles[1].bssid, Some([2, 0, 0, 0xaa, 0xbb, 0xcc]));
        assert!(!c.profiles[1].autoconnect);
    }

    #[test]
    fn reports_errors() {
        let c = parse("PASSPHRASE=x\nSSID=a\nPASSPHRASE=short\nFOO=1\nnonsense\nSSID=\nPASSPHRASE=12345678\n");
        let lines: alloc::vec::Vec<usize> = c.errors.iter().map(|e| e.0).collect();
        assert_eq!(lines, alloc::vec![1, 3, 4, 5, 6]);
        assert_eq!(c.profiles.len(), 1);
        assert_eq!(c.profiles[0].passphrase, None);
    }

    #[test]
    fn round_trip() {
        let mut p = Profile::new(b" x", Some("passphrase "));
        p.bssid = Some([1, 2, 3, 4, 5, 6]);
        p.autoconnect = false;
        let ps = alloc::vec![p, Profile::new(b"Open", None)];
        let c = parse(&write(&ps));
        assert_eq!(c.errors, alloc::vec![]);
        assert_eq!(c.profiles, ps);
        assert_eq!(c.powersave, None);
    }

    #[test]
    fn power_save_setting() {
        let c = parse("powersave=on
SSID=a
");
        assert_eq!((c.powersave, c.errors.len()), (Some(true), 0));
        assert_eq!(parse("POWERSAVE=maybe
").errors.len(), 1);
        // After an SSID= it is not a network setting.
        assert_eq!(parse("SSID=a
POWERSAVE=ON
").errors.len(), 1);
        let mut c = parse("SSID=a
SSID=b
");
        c.powersave = Some(false);
        let back = parse(&write_config(&c));
        assert_eq!((back.powersave, back.profiles.len()), (Some(false), 2));
    }
}
