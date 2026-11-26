//! Bind style handlers for contacts with non-bone-relative shapes.
use core::sync::atomic::Ordering;
use mhgu_sdk::weapon::style::world_contact::{self, Expand};

static SERVICE: world_contact::Service = world_contact::Service(bind);

unsafe extern "C" fn bind(id: u32, callback: Expand) {
    if let Some(entry) = super::registered_entry(id) {
        entry
            .world_contact
            .store(callback as *mut (), Ordering::Release);
    }
}

pub(crate) unsafe fn expand(owner: u32, hit: u32) -> u32 {
    if owner == 0 {
        return 0;
    }
    let entry = unsafe {
        if super::local_hunter() == owner {
            super::actor_entry(owner)
        } else {
            super::multiplayer::peer_entry(owner)
        }
    };
    let Some(entry) = entry else {
        return 0;
    };
    let callback = entry.world_contact.load(Ordering::Acquire);
    if callback.is_null() {
        return 0;
    }
    let expand: Expand = unsafe { core::mem::transmute(callback) };
    unsafe { expand(owner, hit) }
}

unsafe extern "C" fn bind_pass(id: u32, pass: world_contact::CollisionPass) {
    if let Some(entry) = super::registered_entry(id) {
        entry
            .collision_pass
            .store(pass as *mut (), Ordering::Release);
    }
}

#[forge::hook(offset = 0x16abac)]
unsafe fn collision_pass(manager: u32) -> u32 {
    for entry in crate::weapon::style::registered_entries() {
        let callback = entry.collision_pass.load(Ordering::Acquire);
        if !callback.is_null() {
            let pass: world_contact::CollisionPass = unsafe { core::mem::transmute(callback) };
            unsafe { pass(manager) };
        }
    }
    original!(manager)
}

pub(super) fn install(base: u32) {
    static PASS: world_contact::PassService = world_contact::PassService(bind_pass);
    if unsafe { world_contact::publish_pass(&PASS) } {
        forge::install_hook!(base, collision_pass);
    }
    unsafe { world_contact::publish(&SERVICE) };
}
