//! Select a registered weapon model's skeletal clip at GU's part update.

use crate::service;
use core::ffi::CStr;

const NAME: &CStr = c"mhgu::WeaponPartMotionV1";
const MAGIC: u32 = u32::from_le_bytes(*b"WPM1");

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Pose {
    /// Borrowed hunter identity for looking up the style's own state.
    pub actor: u32,
    /// Decoded hunter motion request, including the style's private requests.
    pub motion: u32,
    pub drawn: bool,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Clip {
    pub bank: u8,
    pub slot: u8,
}

impl Clip {
    pub const NONE: Self = Self {
        bank: u8::MAX,
        slot: 0,
    };
}

pub struct Motions {
    pub banks: &'static [&'static CStr],
    pub select: extern "C" fn(Pose) -> Clip,
}

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(u32, *const Motions) -> bool);

/// # Safety
/// Publish once from the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Install a static motion selector for a registered expansion weapon style.
pub fn bind(id: u32, motions: &'static Motions) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<Service>(NAME, MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, motions) })
}
