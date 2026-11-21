//! Resource preparation for the local hunter's selected style.
use crate::service;

const NAME: &core::ffi::CStr = c"mhgu::StylePreloadV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SPL1");

pub type Prepare = unsafe extern "C" fn(actor: u32);

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(u32, Prepare) -> bool);

/// # Safety
/// Publish once from the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Register a callback to cache resources after native hunt setup. GU may
/// initialize the local hunter again during the same process.
/// # Safety
/// Call during style registration. The callback must remain loaded.
pub unsafe fn bind(id: u32, prepare: Prepare) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<Service>(NAME, MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, prepare) })
}
