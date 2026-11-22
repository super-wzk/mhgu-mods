//! Optional in-game instruction pages supplied by a registered style.
use crate::service;
use core::ffi::CStr;

const NAME: &CStr = c"mhgu::StyleManualV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SMN1");

#[repr(transparent)]
pub struct Service(pub unsafe extern "C" fn(u32, *const u8, usize) -> bool);

/// # Safety
/// Called once by the provider on Forge's serialized initialization thread.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Attach UTF-8 pages separated by blank lines. Text accepts GU message tags.
pub fn bind(id: u32, text: &'static str) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<Service>(NAME, MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, text.as_ptr(), text.len()) })
}
