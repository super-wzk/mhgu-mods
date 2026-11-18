//! Optional weapon-style service. The provider owns all registrations and preferences.
pub use super::RegisterError;
use super::Style;
use crate::service;
use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
};

const NAME: &CStr = c"mhgu::StyleRegistryV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SRG1");
const MENU: &CStr = c"mhgu::StyleMenuV1";
const MENU_MAGIC: u32 = u32::from_le_bytes(*b"SMN1");
const INDEPENDENT: &CStr = c"mhgu::IndependentStyleV1";
const INDEPENDENT_MAGIC: u32 = u32::from_le_bytes(*b"STI1");
const CONTROL: &CStr = c"mhgu::StyleControllerV1";
const CONTROL_MAGIC: u32 = u32::from_le_bytes(*b"SCV1");
#[doc(hidden)]
#[repr(C)]
pub struct ControlV1 {
    pub register: unsafe extern "C" fn(
        *const Style,
        *const c_char,
        *const c_char,
        super::Controller,
        Option<super::Request>,
    ) -> u32,
}
/// # Safety
/// Same publication and lifetime requirements as `publish`.
#[doc(hidden)]
pub unsafe fn publish_controller(api: &'static ControlV1) -> bool {
    unsafe { service::publish(CONTROL, CONTROL_MAGIC, api) }
}

/// Optional extension; existing V1 descriptors and registration calls keep their ABI.
#[doc(hidden)]
#[repr(C)]
pub struct MenuV1 {
    pub register: unsafe extern "C" fn(*const Style, *const c_char, *const c_char) -> u32,
}

/// # Safety
/// Same publication and lifetime requirements as `publish`.
#[doc(hidden)]
pub unsafe fn publish_menu(api: &'static MenuV1) -> bool {
    unsafe { service::publish(MENU, MENU_MAGIC, api) }
}
/// # Safety
/// Same publication and lifetime requirements as `publish`.
#[doc(hidden)]
pub unsafe fn publish_independent(api: &'static MenuV1) -> bool {
    unsafe { service::publish(INDEPENDENT, INDEPENDENT_MAGIC, api) }
}

#[doc(hidden)]
#[repr(C)]
pub struct ServiceV1 {
    pub register: unsafe extern "C" fn(*const Style) -> u32,
    pub selected: extern "C" fn(u32) -> u32,
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
    /// No provider, uninitialized engine or wrong thread: no handle.
    pub fn current() -> Option<Self> {
        if !service::on_game_thread() {
            return None;
        }
        Some(Self(service::find(NAME, MAGIC)?, PhantomData))
    }

    /// Register a process-lifetime definition. Repeating the same declaration is OK.
    pub fn register(&self, style: &'static Style) -> Result<(), RegisterError> {
        status(unsafe { (self.0.register)(style) })
    }

    /// Supply UTF-8 text for the equipment box. On older providers, still register the style.
    pub fn register_named(
        &self,
        style: &'static Style,
        name: &'static CStr,
        details: &'static CStr,
    ) -> Result<(), RegisterError> {
        if let Some(menu) = service::find::<MenuV1>(MENU, MENU_MAGIC) {
            status(unsafe { (menu.register)(style, name.as_ptr(), details.as_ptr()) })
        } else {
            self.register(style)
        }
    }

    /// Register a style which supplies its own combat rules. The saved native
    /// style stays intact; fixed-size engine tables use a legal neutral index.
    pub fn register_independent(
        &self,
        style: &'static Style,
        name: &'static CStr,
        details: &'static CStr,
    ) -> Result<(), RegisterError> {
        let api = service::find::<MenuV1>(INDEPENDENT, INDEPENDENT_MAGIC)
            .ok_or(RegisterError::Unavailable)?;
        status(unsafe { (api.register)(style, name.as_ptr(), details.as_ptr()) })
    }

    /// Register an independent weapon controller. It receives GU's resolved
    /// commands and owns weapon selection via `action::enter`; stock weapon
    /// action requests are rejected. Shared GU actions may still return PASS.
    pub fn register_controller(
        &self,
        style: &'static Style,
        name: &'static CStr,
        details: &'static CStr,
        controller: super::Controller,
        request: Option<super::Request>,
    ) -> Result<(), RegisterError> {
        let api =
            service::find::<ControlV1>(CONTROL, CONTROL_MAGIC).ok_or(RegisterError::Unavailable)?;
        status(unsafe {
            (api.register)(style, name.as_ptr(), details.as_ptr(), controller, request)
        })
    }

    /// Effective custom ID for this GU weapon, or zero to keep the native style.
    pub fn selected(&self, weapon: u8) -> u32 {
        (self.0.selected)(weapon.into())
    }
}

fn status(code: u32) -> Result<(), RegisterError> {
    match code {
        0 => Ok(()),
        2 => Err(RegisterError::Conflict),
        _ => Err(RegisterError::Invalid),
    }
}

/// Register directly, returning Unavailable if the registry is not ready.
pub fn register(style: &'static Style) -> Result<(), RegisterError> {
    StyleRegistry::current()
        .ok_or(RegisterError::Unavailable)?
        .register(style)
}

/// Convenience lookup: no available registry also means native GU behavior.
pub fn selected(weapon: u8) -> u32 {
    StyleRegistry::current().map_or(0, |registry| registry.selected(weapon))
}
