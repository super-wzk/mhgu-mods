//! Optional weapon-style service. The provider owns registrations and preferences.
pub use super::RegisterError;
use super::{Style, action::Attack};
use crate::service;
use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
};

const NAME: &CStr = c"mhgu::StyleRegistryV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SRG1");

/// The process-lifetime style registry ABI.
#[doc(hidden)]
#[repr(C)]
pub struct ServiceV1 {
    pub register: unsafe extern "C" fn(*const Style, *const c_char, *const c_char) -> u32,
    pub selected: extern "C" fn(u32) -> u32,
    pub attack: unsafe extern "C" fn(u32, *const Attack) -> u32,
    pub take_guard: unsafe extern "C" fn(u32) -> u32,
}

/// # Safety
/// Provider only, on the serialized Forge init thread. NROs must remain loaded.
#[doc(hidden)]
pub unsafe fn publish(api: &'static ServiceV1) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// A handle to the published registry, restricted to GU's initialized game thread.
pub struct StyleRegistry(&'static ServiceV1, PhantomData<*const ()>);

impl StyleRegistry {
    pub fn current() -> Option<Self> {
        if !service::on_game_thread() {
            return None;
        }
        Some(Self(service::find(NAME, MAGIC)?, PhantomData))
    }

    /// Register a process-lifetime definition with UTF-8 equipment-box labels.
    pub fn register(
        &self,
        style: &'static Style,
        name: &'static CStr,
        details: &'static CStr,
    ) -> Result<(), RegisterError> {
        match unsafe { (self.0.register)(style, name.as_ptr(), details.as_ptr()) } {
            0 => Ok(()),
            2 => Err(RegisterError::Conflict),
            _ => Err(RegisterError::Invalid),
        }
    }

    /// Effective custom ID for this weapon. Zero keeps the selected retail style.
    pub fn selected(&self, weapon: u8) -> u32 {
        (self.0.selected)(weapon.into())
    }
}

pub(super) unsafe fn submit_attack(actor: u32, attack: &Attack) -> bool {
    StyleRegistry::current()
        .is_some_and(|registry| unsafe { (registry.0.attack)(actor, attack) != 0 })
}

pub(super) unsafe fn take_guard(actor: u32) -> u32 {
    StyleRegistry::current().map_or(0, |registry| unsafe { (registry.0.take_guard)(actor) })
}

pub fn selected(weapon: u8) -> u32 {
    StyleRegistry::current().map_or(0, |registry| registry.selected(weapon))
}
