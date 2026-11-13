//! A slot in a GU rMotionList (LMT), independent of which actor loaded it.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod list;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use list::MotionList;

/// LMT stores its slot count as a u16. The resource itself has no bank number.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionSlot(pub u16);
