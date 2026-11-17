//! A quest species is not a native actor class or an index into a GU table.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct SpeciesId(u16);

impl SpeciesId {
    pub const fn new(packed: u16) -> Option<Self> {
        // GU 4ECF8: base in bits 0..7, variant in 8..11, small-monster flag in 12.
        if packed & 0xff == 0 || packed > 0x1fff {
            None
        } else {
            Some(Self(packed))
        }
    }
    pub const fn packed(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HuntCounts {
    pub slain: u16,
    pub captured: u16,
}

/// Namespaced ASCII keys identify species across installations and in player records.
pub fn valid_key(key: &str) -> bool {
    let Some((namespace, name)) = key.split_once(':') else {
        return false;
    };
    key.len() <= 96
        && [namespace, name].iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_' | b'.')
                })
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Species {
    Native(SpeciesId),
    Added(SpeciesId),
}

impl Species {
    pub fn id(self) -> SpeciesId {
        match self {
            Self::Native(id) | Self::Added(id) => id,
        }
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod registry;
