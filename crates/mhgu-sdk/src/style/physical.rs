//! Physical power adjustments for a registered hunter style.
use crate::service;

const NAME: &core::ffi::CStr = c"mhgu::StylePhysicalPowerV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SPP1");
pub type Adjust = unsafe extern "C" fn(actor: u32, raw: i32) -> i32;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum Scope {
    BladeSlots,
    WeaponContacts,
}

#[repr(C)]
pub struct Rule {
    pub scope: Scope,
    pub adjust: Adjust,
}

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(u32, *const Rule) -> bool);

/// # Safety
/// Called once by the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// # Safety
/// Call during style registration. The rule and its callback must remain loaded.
pub unsafe fn bind(id: u32, rule: &'static Rule) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<Service>(NAME, MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, rule) })
}
