//! A DLL with a C interface and a std of its own.
use std::collections::BTreeMap;
#[unsafe(no_mangle)]
pub extern "C" fn cdy_sum(n: u32) -> u32 {
    (0..n).map(|i| (i % 7, i)).collect::<BTreeMap<_, _>>().values().sum()
}
