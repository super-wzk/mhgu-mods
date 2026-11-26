//! Client resource formats. File offsets and encoded values are not native pointers.
//!
//! Parsers retain source bytes and unknown fields. A field receives a semantic
//! name only when its use is established by the client or a verified sample.

use std::fmt;

pub mod crypto;
mod decoded;
pub mod fmod;
pub mod fskl;
pub mod jkr;
pub mod motion;
pub mod pac;
pub mod rig;
pub use decoded::Decoded;

/// An offset in an input resource, not a pointer into the game process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl Error {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
pub mod test_support;

fn read_u32(bytes: &[u8], pos: usize) -> Result<u32> {
    let v = bytes
        .get(pos..)
        .and_then(|s| s.get(..4))
        .ok_or_else(|| Error::new(pos, "truncated 32-bit field"))?;
    Ok(u32::from_le_bytes(v.try_into().unwrap()))
}

pub mod image;
