//! Monster position, heading and posture.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native;

/// Movement posture decoded from the GU status byte, independent of target detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posture {
    Ground,
    /// Native posture 1; distinct from flight (3), but both count as airborne.
    Airborne,
    Flight,
    Other(u8),
}

impl From<u8> for Posture {
    fn from(raw: u8) -> Self {
        match raw {
            0 => Self::Ground,
            1 => Self::Airborne,
            3 => Self::Flight,
            other => Self::Other(other),
        }
    }
}

impl Posture {
    pub const fn raw(self) -> u8 {
        match self {
            Self::Ground => 0,
            Self::Airborne => 1,
            Self::Flight => 3,
            Self::Other(raw) => raw,
        }
    }

    pub const fn is_airborne(self) -> bool {
        matches!(self, Self::Airborne | Self::Flight)
    }
}
