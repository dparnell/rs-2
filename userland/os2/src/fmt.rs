//! `core::fmt` output to standard output.

pub struct Stdout;

impl core::fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        crate::print(s);
        Ok(())
    }
}
