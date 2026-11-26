//! Bind style handlers for contacts with non-bone-relative shapes.
use core::sync::atomic::Ordering;
use mhgu_sdk::{
    combat::collision::hurt::HurtScene,
    weapon::style::world_contact::{self, Expand, HurtPass},
};

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

extern "C" fn bind_hurt(id: u32, pass: HurtPass) -> bool {
    let Some(entry) = super::registered_entry(id) else {
        return false;
    };
    entry.hurt_pass.store(pass as *mut (), Ordering::Release);
    true
}

#[forge::hook(offset = 0x16abac)]
unsafe fn collision_pass(manager: u32) -> u32 {
    let mut has_hurt_pass = false;
    for entry in crate::weapon::style::registered_entries() {
        has_hurt_pass |= !entry.hurt_pass.load(Ordering::Acquire).is_null();
        let callback = entry.collision_pass.load(Ordering::Acquire);
        if !callback.is_null() {
            let pass: world_contact::CollisionPass = unsafe { core::mem::transmute(callback) };
            unsafe { pass(manager) };
        }
    }
    let result = original!(manager);
    if !has_hurt_pass {
        return result;
    }
    let owner = unsafe { crate::weapon::local_hunter() };
    if let Some(entry) = unsafe { crate::weapon::style::local_entry(owner) } {
        let callback = entry.hurt_pass.load(Ordering::Acquire);
        if !callback.is_null()
            && let Some(scene) = unsafe { HurtScene::from_raw(owner) }
        {
            let pass: HurtPass = unsafe { core::mem::transmute(callback) };
            pass(&scene);
        }
    }
    result
}

pub(super) fn install(base: u32) {
    static PASS: world_contact::PassService = world_contact::PassService(bind_pass);
    static HURT: world_contact::HurtService = world_contact::HurtService(bind_hurt);
    if unsafe { world_contact::publish_pass(&PASS) } {
        forge::install_hook!(base, collision_pass);
        unsafe { world_contact::publish_hurt(&HURT) };
    }
    unsafe { world_contact::publish(&SERVICE) };
}
