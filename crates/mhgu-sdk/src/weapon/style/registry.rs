//! Optional weapon-style service. The provider owns registrations and preferences.
pub use super::RegisterError;
use super::{Style, action::Attack};
use crate::service::{self, Init, Startup};
use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
    num::NonZeroU32,
};

const NAME: &CStr = c"mhgu::StyleRegistryV3";
const MAGIC: u32 = u32::from_le_bytes(*b"SRG3");

/// The process-lifetime style registry ABI.
#[doc(hidden)]
#[repr(C)]
pub struct ServiceV3 {
    pub register: unsafe extern "C" fn(*const Style, *const c_char, *const c_char) -> u32,
    pub selected: extern "C" fn(u32) -> u32,
    pub attack: unsafe extern "C" fn(u32, *const Attack) -> u32,
    pub take_guard: unsafe extern "C" fn(u32) -> u32,
}

/// # Safety
/// Provider only, on the serialized Forge init thread. NROs must remain loaded.
#[doc(hidden)]
pub unsafe fn publish(api: &'static ServiceV3) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

// Forge can initialize content before the registry. The first such NRO hosts
// the process-wide queue and owns its allocation until startup completes.
const STARTUP: &CStr = c"mhgu::StyleStartupV3";
const STARTUP_MAGIC: u32 = u32::from_le_bytes(*b"SSI3");

static WAITING: crate::sync::Shared<alloc::vec::Vec<Init>> =
    crate::sync::Shared::new(alloc::vec::Vec::new());

extern "C" fn enqueue(init: Init) {
    WAITING.with(|waiting| waiting.push(init));
}

extern "C" fn drain() {
    for init in WAITING.with(core::mem::take) {
        init();
    }
}

/// Run a content NRO's complete setup once the registry and its hooks are ready.
/// Call once from the weapon's serialized Forge entry point.
pub fn when_ready(init: extern "C" fn()) {
    static LOCAL: Startup = Startup { enqueue, drain };
    service::when_ready(
        STARTUP,
        STARTUP_MAGIC,
        &LOCAL,
        || provider().is_some(),
        init,
    );
}

/// # Safety
/// Provider only. Call after installing every runtime service on the init thread.
#[doc(hidden)]
pub unsafe fn finish_startup() {
    unsafe { service::finish_startup(STARTUP, STARTUP_MAGIC) };
}

// Callers check the game thread before accessing the MT table.
fn provider() -> Option<&'static ServiceV3> {
    unsafe { service::find(NAME, MAGIC) }
}

/// A handle to the published registry, restricted to GU's initialized game thread.
pub struct StyleRegistry(&'static ServiceV3, PhantomData<*const ()>);

impl StyleRegistry {
    pub fn current() -> Option<Self> {
        if !service::on_game_thread() {
            return None;
        }
        Some(Self(provider()?, PhantomData))
    }

    /// Register a process-lifetime definition with UTF-8 labels.
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

    /// Effective custom ID for this weapon, if one is active.
    pub fn selected(&self, weapon: u8) -> Option<u32> {
        NonZeroU32::new((self.0.selected)(weapon.into())).map(NonZeroU32::get)
    }
}

pub(super) unsafe fn submit_attack(actor: u32, attack: &Attack) -> bool {
    StyleRegistry::current()
        .is_some_and(|registry| unsafe { (registry.0.attack)(actor, attack) != 0 })
}

pub(super) unsafe fn take_guard(actor: u32) -> u32 {
    StyleRegistry::current().map_or(0, |registry| unsafe { (registry.0.take_guard)(actor) })
}

pub fn selected(weapon: u8) -> Option<u32> {
    StyleRegistry::current()?.selected(weapon)
}
