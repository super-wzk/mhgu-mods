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

// Forge can initialize content before the registry. The first such NRO hosts
// the process-wide queue and owns its allocation until startup completes.
const STARTUP: &CStr = c"mhgu::StyleStartupV1";
const STARTUP_MAGIC: u32 = u32::from_le_bytes(*b"SSI1");
type Init = extern "C" fn();

#[repr(C)]
struct Startup {
    enqueue: extern "C" fn(Init),
    drain: extern "C" fn(),
}

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
    if !service::on_game_thread() {
        return;
    }
    if StyleRegistry::current().is_some() {
        init();
        return;
    }
    static LOCAL: Startup = Startup { enqueue, drain };
    unsafe {
        if let Some(queue) = service::find::<Startup>(STARTUP, STARTUP_MAGIC) {
            (queue.enqueue)(init);
        } else if service::publish(STARTUP, STARTUP_MAGIC, &LOCAL) {
            enqueue(init);
        }
    }
}

/// # Safety
/// Provider only. Call after installing every runtime service on the init thread.
#[doc(hidden)]
pub unsafe fn finish_startup() {
    if let Some(queue) = unsafe { service::find::<Startup>(STARTUP, STARTUP_MAGIC) } {
        (queue.drain)();
    }
}

/// A handle to the published registry, restricted to GU's initialized game thread.
pub struct StyleRegistry(&'static ServiceV1, PhantomData<*const ()>);

impl StyleRegistry {
    pub fn current() -> Option<Self> {
        if !service::on_game_thread() {
            return None;
        }
        Some(Self(unsafe { service::find(NAME, MAGIC) }?, PhantomData))
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
