//! Bind style handlers for contacts with non-bone-relative shapes.
use super::Subscribers;
use core::sync::atomic::{AtomicBool, Ordering};
use mhgu_sdk::{
    combat::collision::{attack::AttackScene, hurt::HurtScene},
    weapon::style::{
        hunter,
        world_contact::{self, AttackPass, Expand, HurtObserver, HurtPass},
    },
};

static SERVICE: world_contact::Service = world_contact::Service(bind);
static PASSES: Subscribers = Subscribers::new();
static HURT_OBSERVERS: Subscribers = Subscribers::new();
static LOCAL_QUERIES: AtomicBool = AtomicBool::new(false);

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
        PASSES.include(entry);
    }
}

extern "C" fn bind_hurt(id: u32, pass: HurtPass) -> bool {
    let Some(entry) = super::registered_entry(id) else {
        return false;
    };
    entry.hurt_pass.store(pass as *mut (), Ordering::Release);
    LOCAL_QUERIES.store(true, Ordering::Release);
    true
}

extern "C" fn observe_hurt(id: u32, observe: HurtObserver) -> bool {
    let Some(entry) = super::registered_entry(id) else {
        return false;
    };
    entry
        .hurt_observer
        .store(observe as *mut (), Ordering::Release);
    HURT_OBSERVERS.include(entry);
    true
}

extern "C" fn bind_attack(id: u32, pass: AttackPass) -> bool {
    let Some(entry) = super::registered_entry(id) else {
        return false;
    };
    entry.attack_pass.store(pass as *mut (), Ordering::Release);
    LOCAL_QUERIES.store(true, Ordering::Release);
    true
}

#[forge::hook(offset = 0x16abac)]
unsafe fn collision_pass(manager: u32) -> u32 {
    for entry in PASSES.entries() {
        let callback = entry.collision_pass.load(Ordering::Acquire);
        if !callback.is_null() {
            let pass: world_contact::CollisionPass = unsafe { core::mem::transmute(callback) };
            unsafe { pass(manager) };
        }
    }
    let result = original!(manager);
    if !LOCAL_QUERIES.load(Ordering::Acquire) && HURT_OBSERVERS.is_empty() {
        return result;
    }
    let owner = unsafe { crate::weapon::local_hunter() };
    if owner == 0 || !unsafe { hunter::is_quest(owner) } {
        return result;
    }
    let active = unsafe { crate::weapon::style::local_entry(owner) };
    let Some(hurt_scene) = (unsafe { HurtScene::from_raw(owner) }) else {
        return result;
    };
    if let Some(entry) = active {
        let callback = entry.attack_pass.load(Ordering::Acquire);
        if !callback.is_null()
            && let Some(scene) = unsafe { AttackScene::from_raw(manager, owner) }
        {
            let pass: AttackPass = unsafe { core::mem::transmute(callback) };
            pass(&scene);
        }
        let callback = entry.hurt_pass.load(Ordering::Acquire);
        if !callback.is_null() {
            let pass: HurtPass = unsafe { core::mem::transmute(callback) };
            pass(&hurt_scene);
        }
    }
    for entry in HURT_OBSERVERS.entries() {
        let callback = entry.hurt_observer.load(Ordering::Acquire);
        if !callback.is_null() {
            let observe: HurtObserver = unsafe { core::mem::transmute(callback) };
            observe(
                &hurt_scene,
                active.is_some_and(|active| core::ptr::eq(active, entry)),
            );
        }
    }
    result
}

pub(super) fn install(base: u32) {
    static PASS: world_contact::PassService = world_contact::PassService(bind_pass);
    static HURT: world_contact::HurtService = world_contact::HurtService(bind_hurt);
    static OBSERVERS: world_contact::HurtObservers = world_contact::HurtObservers(observe_hurt);
    static ATTACK: world_contact::AttackService = world_contact::AttackService(bind_attack);
    if unsafe { world_contact::publish_pass(&PASS) } {
        forge::install_hook!(base, collision_pass);
        unsafe { world_contact::publish_hurt(&HURT) };
        unsafe { world_contact::publish_hurt_observers(&OBSERVERS) };
        unsafe { world_contact::publish_attack(&ATTACK) };
    }
    unsafe { world_contact::publish(&SERVICE) };
}
