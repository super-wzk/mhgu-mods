//! Describe world-space spheres for contacts with plugin-owned shapes.
use super::{CONTACT_GROUP, Contact};
use crate::memory::{Field, singleton};
use crate::service;

const NAME: &core::ffi::CStr = c"mhgu::WorldContactV1";
const MAGIC: u32 = u32::from_le_bytes(*b"WCT1");

/// Raw V1 result for a contact left to the next handler or to GU.
#[doc(hidden)]
pub const PASS: u32 = u32::MAX;

/// Raw V1 callback used by the runtime. The contact must be live in GU's expansion pass.
#[doc(hidden)]
pub type Expand = unsafe extern "C" fn(hit: u32) -> u32;

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub extern "C" fn(Expand));

/// # Safety
/// Provider only. Publish once during Forge startup and retain the service for the process.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Identity of a contact for this expansion. Addresses can be compared with
/// the plugin's tracked contacts and owners.
#[derive(Clone, Copy)]
pub struct ContactKey {
    pub address: u32,
    pub owner: u32,
}

pub struct Sphere {
    pub center: [f32; 3],
    pub radius: f32,
}

pub trait Handler {
    /// Describe the sphere for an owned single-shape contact. Return `None` to
    /// leave the contact to the next handler or to GU.
    fn sphere(contact: ContactKey) -> Option<Sphere>;
}

/// Register a handler once on the game thread after the runtime is ready.
///
/// Returns false if the service is unavailable. Forge keeps the registered
/// code resident for the lifetime of the process.
pub fn bind<H: Handler + 'static>() -> bool {
    if service::on_game_thread()
        && let Some(api) = unsafe { service::find::<Service>(NAME, MAGIC) }
    {
        (api.0)(expand::<H>);
        return true;
    }
    false
}

unsafe extern "C" fn expand<H: Handler>(hit: u32) -> u32 {
    let Some(contact) = (unsafe { Contact::from_raw(hit) }) else {
        return PASS;
    };
    let key = ContactKey {
        address: hit,
        owner: contact.owner_address(),
    };
    let Some(sphere) = H::sphere(key) else {
        return PASS;
    };
    if !sphere.center.into_iter().all(f32::is_finite)
        || !sphere.radius.is_finite()
        || sphere.radius <= 0.0
    {
        return PASS;
    }
    let count = u8::try_from(contact.0.get(CONTACT_GROUP))
        .ok()
        .and_then(|index| contact.shape_groups()?.shape_count(index));
    if count != Some(1) {
        return PASS;
    }
    // GU 1706A8 stores up to 64 volumes in the collision manager. Keep a
    // custom sphere's serial within that array.
    let Some(manager) = (unsafe { singleton(0x1857830) }) else {
        return 0;
    };
    if manager.get(Field::<u32>::at(10288)) >= 64 {
        return 0;
    }
    unsafe { u32::from(contact.submit_world_sphere(sphere.center, sphere.radius)) }
}
