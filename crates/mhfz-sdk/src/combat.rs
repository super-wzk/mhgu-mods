//! Frontier attack, guard and evade timing on GU's motion clock.

/// FZ 816FD8A2 consumes the first matching contact of either blade, even
/// when the effective hitzone is soft enough to keep swinging.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub extern "C" fn deflection(factor: f32) -> mhgu_sdk::style::rules::Deflection {
    use mhgu_sdk::style::rules::Deflection;
    if factor < 0.23 {
        Deflection::Recoil
    } else {
        Deflection::CloseWindow
    }
}

/// Extra motion frames for Frontier guard windows. GU 2F4A2C applies the
/// stronger Guard skill first, including the two compound skills.
///
/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn guard_bonus_frames(actor: u32) -> u16 {
    use mhgu_sdk::style::hunter::has_skill;
    if [168, 279, 297]
        .into_iter()
        .any(|id| unsafe { has_skill(actor, id) })
    {
        4
    } else if unsafe { has_skill(actor, 169) } {
        2
    } else {
        0
    }
}

/// Extra GU clock frames for a Frontier action that benefits from Evasion.
///
/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn evade_bonus_frames(actor: u32) -> f32 {
    match unsafe { mhgu_sdk::style::hunter::evasion_level(actor) } {
        2.. => 10.,
        1 => 6.,
        _ => 0.,
    }
}

pub struct HitWindow {
    pub delay: f32,
    pub duration: f32,
}

impl HitWindow {
    /// A zero start is relative to the action event. Other rows use the clip frame.
    pub fn new(start: u16, length: u16, frame: f32, late: f32) -> Option<Self> {
        let until_start = if start == 0 {
            -late
        } else {
            f32::from(start) - frame
        };
        let duration = f32::from(length) + until_start.min(0.);
        (duration > 0.).then_some(Self {
            delay: until_start.max(0.),
            duration,
        })
    }
}
