//! Monster animation banks and playback.
use crate::motion::MotionSlot;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native;

/// Packed monster motion request: high byte selects a bank, low byte a slot in its LMT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct MotionId(u16);
impl MotionId {
    pub const fn new(bank: u8, slot: u8) -> Self {
        Self(u16::from_be_bytes([bank, slot]))
    }
    pub const fn from_raw(value: u16) -> Self {
        Self(value)
    }
    pub const fn raw(self) -> u16 {
        self.0
    }
    pub const fn bank(self) -> u8 {
        self.0.to_be_bytes()[0]
    }
    pub const fn slot(self) -> MotionSlot {
        MotionSlot(self.0.to_be_bytes()[1] as u16)
    }
}
