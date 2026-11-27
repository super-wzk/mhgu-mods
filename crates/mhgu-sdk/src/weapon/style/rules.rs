//! Optional behavior supplied by a registered style.
use crate::service;

const NAME: &core::ffi::CStr = c"mhgu::StyleRulesV2";
const MAGIC: u32 = u32::from_le_bytes(*b"SRL2");

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Deflection {
    KeepWindow,
    CloseWindow,
    Recoil,
}

/// A pure rule for a matched blade contact's effective hitzone multiplier.
/// GU's deflection exemptions still apply to `Recoil`.
pub type DeflectionPolicy = extern "C" fn(f32) -> Deflection;
/// Decide whether GU's shared action is waiting for this decoded motion to end.
pub type GroundMotion = extern "C" fn(slot: u16, phase: u8, motion: u32) -> bool;
/// Return the vertical displacement to apply to the eye and target.
pub type CameraOffset = unsafe extern "C" fn(actor: u32, elapsed: f32, eye: *const [f32; 3]) -> f32;
/// Return the desired horizontal view angle in GU units, or -1 for stock behavior.
pub type CameraYaw = unsafe extern "C" fn(actor: u32) -> i32;
/// Ordinary quest orbit before the native collision pass. Heights are relative to the hunter.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CameraLens {
    /// Field of view in degrees.
    pub fov: f32,
    /// Horizontal distance between the camera eye and target.
    pub radius: f32,
    pub eye_height: f32,
    pub target_height: f32,
}
/// Adjust the current orbit geometry and lens once per camera update.
pub type CameraLensUpdate = unsafe extern "C" fn(actor: u32, elapsed: f32, lens: &mut CameraLens);

#[repr(C)]
pub struct Rules {
    /// Half-angle in GU binary angle units for in-place blocks or a fallback
    /// when GU rejects a precise block. A shared GU guard action uses the
    /// fallback only with its guard flag set and below the native stamina threshold.
    /// Zero disables the custom cone.
    pub guard_half_angle: u16,
    pub ground_motion: Option<GroundMotion>,
    pub deflection: Option<DeflectionPolicy>,
    pub camera_offset: Option<CameraOffset>,
    pub camera_yaw: Option<CameraYaw>,
    pub camera_lens: Option<CameraLensUpdate>,
}

impl Rules {
    pub const EMPTY: Self = Self {
        guard_half_angle: 0,
        ground_motion: None,
        deflection: None,
        camera_offset: None,
        camera_yaw: None,
        camera_lens: None,
    };
}

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(u32, *const Rules) -> bool);

/// # Safety
/// Publish once from the registry on the Forge init thread.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Attach a resident rule descriptor after registering the style.
pub fn bind(id: u32, rules: &'static Rules) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<Service>(NAME, MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, rules) })
}
