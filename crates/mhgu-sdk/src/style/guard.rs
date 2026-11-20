//! A style's response to a block already accepted by GU.

#[derive(Default)]
#[repr(C)]
pub struct GuardResult {
    /// Invulnerability to grant as soon as GU enters recoil, in motion frames.
    pub invulnerability: f32,
    /// Native recoil tier. Tier zero prevents chip damage.
    pub recoil: u32,
    /// Minimum effective power for sharpness loss. Guard Up hits retain the
    /// native cost regardless of power.
    pub sharpness_power: u32,
}

#[repr(u32)]
pub enum Decision {
    Native,
    Reject,
    Precise,
}

/// For a precise block, fill the result for the collision that invoked the policy.
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
