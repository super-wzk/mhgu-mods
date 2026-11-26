//! Map paired Frontier motion groups to GU hunter LMT slots.
use core::ffi::CStr;
use mhgu_sdk::style::action::MotionBanks as GuMotionBanks;
use mhgu_sdk::style::{action, remote::Sheathe};

mod drawn_roll;
pub use drawn_roll::DrawnRoll;

/// Select the FZ sprint-sheathe clip for GU's common sheathe action.
///
/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn sprint_sheathe(actor: u32, request: u32, motions: &Sheathe) -> bool {
    request == Sheathe::MOTION
        && unsafe { action::current(actor) } == Sheathe::ACTION
        && motions.follows_sprint(unsafe { action::previous(actor) })
}

/// Explicit requests for converted Frontier motions, used by remote sheathing.
pub const fn sheathe_motions(sprints: &'static [u8], sprint_clip: u16) -> Sheathe {
    Sheathe {
        sprints,
        ordinary: 0x10000 | 1003,
        sprint: 0x10000 | sprint_clip as u32,
    }
}

pub struct MotionBanks(GuMotionBanks<3>);

impl MotionBanks {
    pub const fn new(paths: [&'static CStr; 3]) -> Self {
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
    /// Call on the game thread with an initialized resource manager.
    pub unsafe fn preload(&self, source: u16) {
        if let Some((group, slot)) = clip_slot(source) {
            unsafe { self.0.preload(group, slot) };
        }
    }

    /// Cache the locomotion, terrain, and weapon attack banks during hunter setup.
    /// `attack_clip` is a clip present in this weapon's attack bank.
    ///
    /// # Safety
    /// Call on the game thread with an initialized resource manager.
    pub unsafe fn preload_for_hunt(&self, attack_clip: u16) {
        for clip in [1002, 1211, attack_clip] {
            unsafe { self.preload(clip) };
        }
    }

    /// # Safety
    /// The actor must be a live GU hunter on the game thread.
    pub unsafe fn decode(&self, actor: u32, packed: u16) -> Option<u16> {
        let group = unsafe { self.0.group(actor, packed) }?;
        let slot = packed as u8 & !GuMotionBanks::<3>::VARIANT_BIT;
        Some(1000 + 200 * group as u16 + u16::from(slot))
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
