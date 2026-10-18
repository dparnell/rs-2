//! API names and conventions (from the generated table).

use super::names_gen::{ALIASES, ENTRIES};

fn find(module: u8, ordinal: u16) -> Option<&'static (u8, u16, bool, &'static str)> {
    ENTRIES.binary_search_by(|e| (e.0, e.1).cmp(&(module, ordinal))).ok().map(|i| &ENTRIES[i])
}

/// Whether the entry uses the 16-bit far `_Pascal` convention. Unknown
/// ordinals follow the module's general convention.
pub fn is16(module: u8, ordinal: u16) -> bool {
    match find(module, ordinal) {
        Some(e) => e.2,
        None => module != 0 || ordinal < 200,
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
