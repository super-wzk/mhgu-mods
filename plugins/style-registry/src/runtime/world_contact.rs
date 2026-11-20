//! Register a single hook for styles that submit non-bone-relative contact shapes.
use core::sync::atomic::Ordering;
use mhgu_sdk::style::world_contact::{self, Expand};

static SERVICE: world_contact::Service = world_contact::Service(bind);

unsafe extern "C" fn bind(id: u32, callback: Expand) {
    if let Some(entry) = super::registered_entry(id) {
        entry
            .world_contact
            .store(callback as u32, Ordering::Release);
    }
}

#[forge::hook(offset = 0x1696fc)]
unsafe fn expand(hit: u32) -> u32 {
    unsafe {
        let owner = *((hit + 12) as *const u32);
        if owner != 0
            && let Some(style) = if crate::runtime::local_hunter() == owner {
                crate::runtime::actor_entry(owner)
            } else {
                crate::runtime::peer_entry(owner)
            }
        {
            let callback = style.world_contact.load(Ordering::Acquire);
            if callback != 0 {
                let callback: Expand = core::mem::transmute(callback as usize);
                let shapes = callback(owner, hit);
                if shapes != 0 {
                    return shapes;
                }
            }
        }
        original!(hit)
    }
}

pub(super) fn install(base: u32) {
    if unsafe { world_contact::publish(&SERVICE) } {
        forge::install_hook!(base, expand);
    }
}
