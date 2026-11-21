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

/// Read the hunter's current sharpness grade through the virtual used by GU 178030.
///
/// # Safety
/// The actor must be a live, equipped quest hunter on the game thread.
pub unsafe fn sharpness_grade(actor: u32) -> u32 {
    unsafe {
        let vtable = (actor as *const *const usize).read();
        let grade: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(vtable.add(508 / 4).read());
        grade(actor)
    }
}

/// Whether this actor has the task hunter layout used by weapon actions.
///
/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn is_quest(actor: u32) -> bool {
    unsafe {
        let vtable = (actor as *const *const usize).read();
        if vtable.is_null() {
            return false;
        }
        let get_type: unsafe extern "C" fn(u32) -> *const usize =
            core::mem::transmute(vtable.add(5).read());
        let quest = entry(0x1886bb8) as *const usize; // uPlayerQuest
        let mut kind = get_type(actor);
        while !kind.is_null() {
            if kind == quest {
                return true;
            }
            // MtDTI's parent descriptor is set by GU 7AEDCC.
            kind = kind.add(4).read() as *const usize;
        }
        false
    }
}

/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
pub unsafe fn has_skill(actor: u32, skill: u32) -> bool {
    let has: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(entry(0x2864c4)) };
    unsafe { has(actor, skill) != 0 }
}

/// Resolve the effective Evasion level, including GU's conditional bonuses.
///
/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
pub unsafe fn evasion_level(actor: u32) -> i32 {
    let query: unsafe extern "C" fn(u32) -> i32 = unsafe { core::mem::transmute(entry(0x2a69dc)) };
    unsafe { query(actor) }
}
