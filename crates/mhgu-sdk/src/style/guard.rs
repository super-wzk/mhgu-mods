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
