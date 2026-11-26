//! A style's response to an incoming guardable hit and GU's confirmed recoil.

#[derive(Default)]
#[repr(C)]
pub struct GuardResult {
    /// Invulnerability to grant when GU confirms the block, in motion frames.
    pub invulnerability: f32,
    /// Native recoil tier. Tier zero prevents chip damage.
    pub recoil: u32,
    /// Minimum effective power for sharpness loss on ordinary hits, or
    /// [`Self::NO_SHARPNESS_COST`] to waive the cost for every accepted hit.
    pub sharpness_power: u32,
    /// Called once for each settled in-place block while its action is active.
    pub on_block: Option<unsafe extern "C" fn(u32)>,
}

impl GuardResult {
    /// Waive sharpness loss on a precise block, including Guard Up hits.
    pub const NO_SHARPNESS_COST: u32 = 0x8000_0000;
}

#[repr(u32)]
pub enum Decision {
    Native,
    Reject,
    Precise,
    /// Block in an extension weapon's private action without GU recoil or costs.
    InPlace,
}

/// Fill the result for a precise or in-place block.
/// For in-place blocks the policy can be queried during both collision and
/// damage settlement. Use `GuardResult::on_block` to record a confirmed hit.
pub type Policy = unsafe extern "C" fn(u32, *mut GuardResult) -> Decision;

pub enum Contact {
    Other,
    Monster,
}

/// Consume the precise block confirmed by GU's recoil transition.
///
/// # Safety
/// The actor must be the live local hunter during the style controller callback.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn take(actor: u32) -> Option<Contact> {
    match unsafe { super::registry::take_guard(actor) } {
        1 => Some(Contact::Other),
        2 => Some(Contact::Monster),
        _ => None,
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod impact {
    use crate::service;
    const NAME: &core::ffi::CStr = c"mhgu::GuardImpactV1";
    const MAGIC: u32 = u32::from_le_bytes(*b"GIP1");
    #[doc(hidden)]
    #[repr(C)]
    pub struct Impact(pub unsafe extern "C" fn(u32, *mut [f32; 3]) -> bool);

    /// # Safety
    /// Only the registry may publish this service during game-thread initialization.
    pub unsafe fn publish_impact(api: &'static Impact) -> bool {
        unsafe { service::publish(NAME, MAGIC, api) }
    }
    /// # Safety
    /// Call inside the policy for the hunter currently resolving a guard.
    pub unsafe fn impact_position(actor: u32) -> Option<[f32; 3]> {
        let api = unsafe { service::find::<Impact>(NAME, MAGIC) }?;
        let mut position = [0.; 3];
        unsafe { (api.0)(actor, &mut position) }.then_some(position)
    }
}
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use impact::{Impact, impact_position, publish_impact};
