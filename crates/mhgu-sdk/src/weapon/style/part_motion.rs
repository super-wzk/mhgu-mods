//! Select a registered weapon model's skeletal clip at GU's part update.

use crate::service;
use core::ffi::CStr;

const NAME: &CStr = c"mhgu::WeaponPartMotion";
const MAGIC: u32 = u32::from_le_bytes(*b"WPMT");

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Pose {
    /// Borrowed hunter identity for looking up the style's own state.
    pub actor: u32,
    /// Decoded hunter motion request, including the style's private requests.
    pub motion: u32,
    /// Visual drawn pose, including the hand-held phase of native sharpening.
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

/// Local transform of the primary weapon relative to a hunter joint.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Socket {
    pub joint: u32,
    pub offset: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
    /// Whether the part clip inherits the hunter's incoming motion blend.
    pub blend_motion: bool,
}

pub struct Motions {
    /// Select the parent for the displayed part frame and the hunter blend
    /// remaining. Adjacent keys attached to different joints cannot be
    /// interpolated in local space.
    pub socket: Option<extern "C" fn(Pose, f32, f32) -> Socket>,
    pub banks: &'static [&'static CStr],
    pub select: extern "C" fn(Pose) -> Clip,
}

#[doc(hidden)]
#[repr(C)]
pub struct Service(
    pub unsafe extern "C" fn(u32, *const Motions) -> bool,
    pub unsafe extern "C" fn(u32, u16, f32),
);

/// Restart the linked part and hold the incoming hunter pose during a Frontier blend.
/// # Safety
/// Call just after starting `packed` on a live game-thread hunter.
pub unsafe fn begin_motion(actor: u32, packed: u16, blend: f32) {
    if let Some(api) = unsafe { service::find::<Service>(NAME, MAGIC) } {
        unsafe { (api.1)(actor, packed, blend) };
    }
}

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
