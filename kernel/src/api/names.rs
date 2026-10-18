//! API names and conventions (from the generated table).

use super::names_gen::{ALIASES, DOS_ALIASES, ENTRIES};

fn find(module: u8, ordinal: u16) -> Option<&'static (u8, u16, bool, &'static str)> {
    ENTRIES.binary_search_by(|e| (e.0, e.1).cmp(&(module, ordinal))).ok().map(|i| &ENTRIES[i])
}

/// Map DOSCALLS ordinals that alias another module's API (implemented in
/// DOSCALL1 under extra ordinals) to that API.
pub fn canonical(module: u8, ordinal: u16) -> (u8, u16) {
    if module == 0 {
        if let Ok(i) = DOS_ALIASES.binary_search_by(|a| a.0.cmp(&ordinal)) {
            return (DOS_ALIASES[i].1, DOS_ALIASES[i].2);
        }
    }
    (module, ordinal)
}

/// Whether the entry uses the 16-bit far `_Pascal` convention. Unknown
/// ordinals follow the module's general convention.
pub fn is16(module: u8, ordinal: u16) -> bool {
    let (module, ordinal) = canonical(module, ordinal);
    // DOSCALLS.567 (Dos16OpenL) is a forwarder in DOSCALL1, absent from
    // the generated table.
    if module == 0 && ordinal == 567 {
        return true;
    }
    match find(module, ordinal) {
        Some(e) => e.2,
        // RS2API (8) is 32-bit.
        None => (module != 0 && module != 8) || (module == 0 && ordinal < 200),
    }
}

pub fn display(module: u8, ordinal: u16) -> &'static str {
    find(module, ordinal).map_or("?", |e| e.3)
}

/// Ordinal for an import by name.
pub fn ordinal(module: &str, name: &str) -> Option<u16> {
    let up = name.to_ascii_uppercase();
    let m = module.to_ascii_uppercase();
    ALIASES
        .binary_search_by(|a| (a.0, a.1).cmp(&(m.as_str(), up.as_str())))
        .ok()
        .map(|i| ALIASES[i].2)
}
