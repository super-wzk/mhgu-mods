//! Attacker stop times for contacts imported from Frontier.

const FRONTIER: u8 = 0x80;
const MAX_TICKS: u8 = 15;

/// Preserve the source hitstop in 30 Hz ticks until the contact is resolved.
pub const fn frontier(ticks: u8) -> u8 {
    match ticks {
        0 => 0,
        1..=MAX_TICKS => FRONTIER | ticks,
        _ => FRONTIER | MAX_TICKS,
    }
}

#[doc(hidden)]
pub const fn frames(value: u8) -> Option<f32> {
    if value > FRONTIER && value <= (FRONTIER | MAX_TICKS) {
        Some(((value & MAX_TICKS) * 2) as f32)
    } else {
        None
    }
}

#[doc(hidden)]
pub const fn native_factor(value: u8) -> u8 {
    match frames(value) {
        Some(frames) => (frames as u8 + 1) / 3,
        None => value,
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
const SERVICE: &core::ffi::CStr = c"mhgu::FrontierHitstop";
#[cfg(all(target_arch = "arm", target_endian = "little"))]
const MAGIC: u32 = u32::from_le_bytes(*b"FHS1");

/// # Safety
/// Call on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
#[doc(hidden)]
pub unsafe fn contact_value(value: u8) -> u8 {
    if frames(value).is_some() && unsafe { crate::service::find::<u32>(SERVICE, MAGIC) }.is_none() {
        return native_factor(value);
    }
    value
}

/// # Safety
/// Publish on the game thread after installing the contact hook.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
#[doc(hidden)]
pub unsafe fn publish() -> bool {
    static VERSION: u32 = 1;
    unsafe { crate::service::publish(SERVICE, MAGIC, &VERSION) }
}
