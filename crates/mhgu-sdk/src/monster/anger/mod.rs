//! Native anger modes, gauge and transitions.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native;

/// Anger mode decoded from the GU status byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngerMode {
    Calm,
    Enraged,
    /// Secondary rage. BCFC0 requests group 17 when its separate gauge fills
    /// during ordinary rage; BCE6C uses the second parameter block's duration.
    SecondStageRage,
    Other(u8),
}

impl From<u8> for AngerMode {
    fn from(raw: u8) -> Self {
        match raw {
            0 => Self::Calm,
            1 => Self::Enraged,
            2 => Self::SecondStageRage,
            other => Self::Other(other),
        }
    }
}

impl AngerMode {
    pub const fn raw(self) -> u8 {
        match self {
            Self::Calm => 0,
            Self::Enraged => 1,
            Self::SecondStageRage => 2,
            Self::Other(raw) => raw,
        }
    }
}
