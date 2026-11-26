//! Optional world-space shape expansion for contacts owned by a registered style.
use crate::service;

const NAME: &core::ffi::CStr = c"mhgu::StyleWorldContactV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SWC1");

/// Submit this contact's world shapes and return their count. Return zero to
/// leave the contact's original HTS expansion in place.
pub type Expand = unsafe extern "C" fn(owner: u32, hit: u32) -> u32;

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(u32, Expand));

/// # Safety
/// Called once by the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// # Safety
/// The style and callback must belong to the calling plugin and stay loaded.
pub unsafe fn bind(id: u32, expand: Expand) -> bool {
    if service::on_game_thread()
        && let Some(api) = (unsafe { service::find::<Service>(NAME, MAGIC) })
    {
        unsafe { (api.0)(id, expand) };
        return true;
    }
    false
}

/// Enqueue independently timed style contacts before GU expands the attack list.
pub type CollisionPass = unsafe extern "C" fn(manager: u32);
const PASS_NAME: &core::ffi::CStr = c"mhgu::StyleCollisionPassV1";
const PASS_MAGIC: u32 = u32::from_le_bytes(*b"SCP1");

#[doc(hidden)]
#[repr(C)]
pub struct PassService(pub unsafe extern "C" fn(u32, CollisionPass));

/// # Safety
/// Called once by the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_pass(api: &'static PassService) -> bool {
    unsafe { service::publish(PASS_NAME, PASS_MAGIC, api) }
}

/// # Safety
/// The callback is resident and uses only current hunt actors and stable contact storage.
pub unsafe fn bind_pass(id: u32, pass: CollisionPass) -> bool {
    if service::on_game_thread()
        && let Some(api) = unsafe { service::find::<PassService>(PASS_NAME, PASS_MAGIC) }
    {
        unsafe { (api.0)(id, pass) };
        return true;
    }
    false
}
