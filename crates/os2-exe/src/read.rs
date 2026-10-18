use core::fmt;

/// Errors produced while parsing an executable module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A structure extends beyond the end of the file.
    Truncated { offset: usize, len: usize },
    /// The signature is not one we understand.
    BadMagic,
    /// A field holds a value that the format does not allow.
    Invalid(&'static str),
    /// A feature of the format that is not implemented.
    Unsupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { offset, len } => {
                write!(f, "truncated: {len} bytes at {offset:#x}")
            }
            Error::BadMagic => f.write_str("not an OS/2 executable"),
            Error::Invalid(what) => write!(f, "invalid {what}"),
            Error::Unsupported(what) => write!(f, "unsupported {what}"),
        }
    }
}

/// Bounds-checked little-endian reader over a byte slice.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn bytes(&self, offset: usize, len: usize) -> Result<&'a [u8], Error> {
        offset
            .checked_add(len)
            .and_then(|end| self.data.get(offset..end))
            .ok_or(Error::Truncated { offset, len })
    }

    pub fn u8(&self, offset: usize) -> Result<u8, Error> {
        Ok(self.bytes(offset, 1)?[0])
    }

    pub fn u16(&self, offset: usize) -> Result<u16, Error> {
        let b = self.bytes(offset, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&self, offset: usize) -> Result<u32, Error> {
        let b = self.bytes(offset, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a length-prefixed (Pascal) string.
    pub fn pstr(&self, offset: usize) -> Result<&'a [u8], Error> {
        let len = self.u8(offset)? as usize;
        self.bytes(offset + 1, len)
    }
}
