//! GU's live hunter manager and equipped weapon query.
use crate::memory::entry;

pub(super) unsafe fn manager() -> Option<u32> {
    let manager = unsafe { (entry(0x1885b58) as *const u32).read() };
    (manager != 0).then_some(manager)
}

pub(super) unsafe fn local_in(manager: u32) -> u32 {
    let local: unsafe extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(entry(0x276278)) };
    unsafe { local(manager) }
}

/// # Safety
/// Call on GU's game thread and use the actor within this callback.
pub unsafe fn local() -> Option<u32> {
    let actor = unsafe { local_in(manager()?) };
    (actor != 0).then_some(actor)
}

/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn equipped_weapon(actor: u32) -> Option<u8> {
    unsafe {
        let equipment = ((actor + 5060) as *const u32).read();
        (equipment != 0).then(|| ((equipment + 1236) as *const u8).read())
    }
}

/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
pub unsafe fn has_skill(actor: u32, skill: u32) -> bool {
    let has: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(entry(0x2864c4)) };
    unsafe { has(actor, skill) != 0 }
}
