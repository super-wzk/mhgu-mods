//! Frontier attack, guard and evade timing on GU's motion clock.

/// Convert the SDT attacker's stop time (byte 29) to a GU HDP factor.
/// FZ 816F9840 / 81820D7A count 30 Hz ticks. GU 16DB68 uses three 60 Hz
/// frames per factor for a full stop and keeps its native slowdown rules.
pub const fn attacker_hitstop(source_ticks: u8) -> u8 {
    ((2 * source_ticks as u16 + 1) / 3) as u8
}

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

/// GU launch directions eligible for the FZ drawn recovery (817E9CB4 / 817EAA30).
///
/// # Safety
/// The actor must be a live GU quest hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn knockback_recovery(
    actor: u32,
    current: (u16, u16),
    drawn: bool,
    input: &mhgu_sdk::style::Input,
    states: [u8; 2],
) -> Option<u8> {
    use mhgu_sdk::style::movement;
    if !drawn || !input.command(3) || !matches!(current, (64, 6 | 7)) {
        return None;
    }
    unsafe {
        // 2BB164 selects motion 1015 after landing. Leave airborne, lethal
        // and other GU hit reactions alone.
        if ((actor + 5) as *const u8).read() != 2
            || ((actor + 11988) as *const u8).read() == 0
            || ((actor + 5078) as *const i16).read() <= 0
        {
            return None;
        }
        let address = forge_sys::mem::forge_mem_getMainTextAddr() + 0x53a058;
        let motion: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(address as usize);
        if motion(actor) != 1015 || !movement::has_evade_stamina(actor, 150.) {
            return None;
        }
    }
    Some(states[usize::from(current.1 - 6)])
}

#[repr(u32)]
pub enum DrawnEvade {
    Roll = 0,
    DemonStep = 6,
}

/// Apply GU's Bladescale Hone and weapon-specific evade progress once on entry.
///
/// # Safety
/// Call for an accepted local drawn evade that bypasses GU's evade action.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn bladescale_evade(actor: u32, kind: DrawnEvade) {
    let address = unsafe { forge_sys::mem::forge_mem_getMainTextAddr() } + 0x2df9d0;
    let apply: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(address as usize) };
    unsafe { apply(actor, kind as u32) };
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
