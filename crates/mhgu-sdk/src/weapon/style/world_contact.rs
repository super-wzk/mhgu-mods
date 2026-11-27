//! Optional world-space shape expansion for contacts owned by a registered style.
use crate::{
    combat::collision::{attack::AttackScene, hurt::HurtScene},
    service,
};

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

/// Observe monster hurt volumes in the active local style's collision pass.
pub type HurtPass = extern "C" fn(&HurtScene<'_>);
const HURT_NAME: &core::ffi::CStr = c"mhgu::StyleHurtPassV1";
const HURT_MAGIC: u32 = u32::from_le_bytes(*b"SHP1");

#[doc(hidden)]
#[repr(C)]
pub struct HurtService(pub extern "C" fn(u32, HurtPass) -> bool);

/// # Safety
/// Called once by the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_hurt(api: &'static HurtService) -> bool {
    unsafe { service::publish(HURT_NAME, HURT_MAGIC, api) }
}

pub fn bind_hurt(id: u32, pass: HurtPass) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<HurtService>(HURT_NAME, HURT_MAGIC) }
            .is_some_and(|api| (api.0)(id, pass))
}

/// Observe the whole collision pass for any local quest hunter. `active`
/// reports whether that hunter currently uses the observer's style.
pub type HurtObserver = extern "C" fn(&HurtScene<'_>, active: bool);
const OBSERVER_NAME: &core::ffi::CStr = c"mhgu::StyleHurtObserverV1";
const OBSERVER_MAGIC: u32 = u32::from_le_bytes(*b"SHO1");

#[doc(hidden)]
#[repr(transparent)]
pub struct HurtObservers(pub extern "C" fn(u32, HurtObserver) -> bool);

/// # Safety
/// Called once by the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_hurt_observers(api: &'static HurtObservers) -> bool {
    unsafe { service::publish(OBSERVER_NAME, OBSERVER_MAGIC, api) }
}

pub fn observe_hurt(id: u32, observe: HurtObserver) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<HurtObservers>(OBSERVER_NAME, OBSERVER_MAGIC) }
            .is_some_and(|api| (api.0)(id, observe))
}

/// Observe expanded attacks before GU runs its damage and invulnerability checks.
pub type AttackPass = extern "C" fn(&AttackScene<'_>);
const ATTACK_NAME: &core::ffi::CStr = c"mhgu::StyleAttackPassV1";
const ATTACK_MAGIC: u32 = u32::from_le_bytes(*b"SAP1");

#[doc(hidden)]
#[repr(C)]
pub struct AttackService(pub extern "C" fn(u32, AttackPass) -> bool);

/// # Safety
/// Called once by the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_attack(api: &'static AttackService) -> bool {
    unsafe { service::publish(ATTACK_NAME, ATTACK_MAGIC, api) }
}

pub fn bind_attack(id: u32, pass: AttackPass) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<AttackService>(ATTACK_NAME, ATTACK_MAGIC) }
            .is_some_and(|api| (api.0)(id, pass))
}
