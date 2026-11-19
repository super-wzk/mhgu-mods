//! Custom hunter styles. ID zero always means GU's equipped native style.
pub mod action;
pub mod guard;

/// Return this from a motion callback to let GU handle the request.
pub const PASS: u32 = u32::MAX;

/// Receives the local hunter and either the decimal request (encode) or the
/// packed bank/slot (decode). The actor is borrowed only for this call.
pub type Motion = unsafe extern "C" fn(u32, u32) -> u32;

/// GU's command banks, after bindings, combination buffering and weapon mapping.
/// Command IDs are the arguments to 285E2C (common) and 285E78 (weapon).
#[repr(C)]
pub struct Input {
    pub common: [u32; 4],
    pub weapon: [u32; 4],
    pub stick_angle: u16,
    pub stick_strength: i16,
    /// Extra right shoulder, resolved by GU's binding query (65FA04).
    /// This is available for moves, such as Frontier brace, absent from GU.
    pub auxiliary_pressed: u32,
}
impl Input {
    pub fn command(&self, id: u8) -> bool {
        self.common[usize::from(id / 32)] & (1 << (id % 32)) != 0
    }
    pub fn weapon_command(&self, id: u8) -> bool {
        self.weapon[usize::from(id / 32)] & (1 << (id % 32)) != 0
    }
}

/// Return PASS to run GU's shared action. A null input ends this hunter's session;
/// the actor is zero if the previous scene's hunter is no longer safe to access.
/// Controllers own weapon selection and use `action::enter` for their moves.
pub type Controller = unsafe extern "C" fn(u32, *const Input) -> u32;

/// An action route already accepted by GU's input and movement controllers.
#[repr(C)]
pub struct ActionRequest {
    pub group: u16,
    pub slot: u16,
    pub flags: u16,
    pub draw_route: bool,
}

/// Return a registered style move ID (0–255) or PASS. PASS leaves a draw
/// route to GU and rejects an unhandled stock weapon action.
pub type Request = unsafe extern "C" fn(u32, *const ActionRequest) -> u32;

/// Immutable process-lifetime descriptor. IDs are global across all weapons.
/// `weapon` uses GU's equipment byte, not the Frontier weapon number.
#[repr(C)]
pub struct Style {
    pub id: u32,
    pub weapon: u32,
    pub encode: Option<Motion>,
    pub decode: Option<Motion>,
    /// Use a neutral index for the engine's fixed-size native style tables.
    pub independent: bool,
    pub art_slots: u8,
    pub controller: Option<Controller>,
    pub request: Option<Request>,
    pub actions: action::Actions,
    pub guard: Option<guard::Policy>,
    pub commands: Option<action::command::Bind>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegisterError {
    Invalid,
    Conflict,
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod registry;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
#[doc(hidden)]
pub use registry::{ServiceV1, publish};
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use registry::{StyleRegistry, selected};
