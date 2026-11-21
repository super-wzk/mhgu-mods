//! Map paired Frontier motion groups to GU hunter LMT slots.
use core::ffi::CStr;
use mhgu_sdk::style::action::MotionBanks as GuMotionBanks;

mod drawn_roll;
pub use drawn_roll::DrawnRoll;

pub struct MotionBanks(GuMotionBanks<3>);

impl MotionBanks {
    pub const fn new(paths: [[&'static CStr; 3]; 2]) -> Self {
        Self(GuMotionBanks::new(paths))
    }

    /// # Safety
    /// The actor must be a live GU hunter with equipped gear on the game thread.
    pub unsafe fn encode(&self, actor: u32, source: u16) -> Option<u16> {
        let (group, slot) = clip_slot(source)?;
        unsafe { self.0.encode(actor, group, slot) }
    }

    /// Cache the bank for a source clip before it is played.
    ///
    /// # Safety
    /// Call on the game thread with a live hunter and initialized resource manager.
    pub unsafe fn preload(&self, actor: u32, source: u16) {
        if let Some((group, slot)) = clip_slot(source) {
            unsafe { self.0.preload(actor, group, slot) };
        }
    }

    /// # Safety
    /// The actor must be a live GU hunter on the game thread.
    pub unsafe fn decode(&self, actor: u32, packed: u16) -> Option<u16> {
        let group = unsafe { self.0.group(actor, packed) }?;
        Some(1000 + 200 * group as u16 + (packed & 255))
    }
}

fn clip_slot(source: u16) -> Option<(usize, u8)> {
    let group = match source / 100 {
        10 => 0,
        12 => 1,
        14 => 2,
        _ => return None,
    };
    Some((group, (source % 100) as u8))
}
