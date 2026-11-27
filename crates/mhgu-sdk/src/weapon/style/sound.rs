//! Positional hunter sound requests on GU's audio queue.
use super::resource::CachedResource;
use core::ffi::CStr;

pub struct Bank {
    resource: CachedResource,
}

impl Bank {
    pub const fn new(path: &'static CStr) -> Self {
        Self {
            resource: CachedResource::new(0x2125bec, path),
        }
    }

    /// Queue the native sound bank without playing a cue.
    ///
    /// # Safety
    /// Call on the game thread after the resource manager has initialized.
    pub unsafe fn preload(&self) {
        unsafe { self.resource.preload() };
    }

    /// Queue a request using the hunter as the positional audio owner.
    ///
    /// # Safety
    /// The actor must be a live hunter on GU's game thread.
    pub unsafe fn play(&self, actor: u32, id: u8) {
        unsafe {
            self.play_at(
                actor,
                id,
                ((actor + 64) as *const [f32; 3]).read_unaligned(),
            );
        }
    }

    /// Queue a hunter-owned cue at the given world position. GU copies the coordinates.
    ///
    /// # Safety
    /// The actor must be a live hunter on GU's game thread.
    pub unsafe fn play_at(&self, actor: u32, id: u8, position: [f32; 3]) {
        unsafe {
            let engine = (crate::memory::entry(0x1896f5c) as *const u32).read();
            if engine == 0 {
                return;
            }
            let Some(resource) = self.resource.get() else {
                return;
            };
            let cue = 4096 + u32::from(id);
            let lookup: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(crate::memory::entry(0xb68280));
            if lookup(resource, cue) != 0 {
                let queue: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) -> u32 =
                    core::mem::transmute(crate::memory::entry(0x4eeaa4));
                queue(engine, 7, resource, cue, actor, position.as_ptr() as u32);
            }
        }
    }
}

/// Draw from GU's hunter random source at 27C8F8.
/// # Safety
/// The call must run on GU's game thread after initialization.
pub unsafe fn random_u32() -> u32 {
    let draw: unsafe extern "C" fn() -> u32 =
        unsafe { core::mem::transmute(crate::memory::entry(0x27c8f8)) };
    unsafe { draw() }
}


/// GU SND commands 0x67 (surface) and 0x68 (equipment), dispatched with the
/// hunter's live floor material, equipped armour sound type, and area filters.
#[derive(Clone, Copy)]
pub enum ContactSound {
    Footstep(u8),
    Armour,
    WaistArmour,
}

/// # Safety
/// The hunter must be a live game-thread actor.
pub unsafe fn contact(actor: u32, sound: ContactSound) {
    let (group, variant) = match sound {
        ContactSound::Footstep(cue) => (9, u32::from(cue)),
        ContactSound::Armour => (5, 0),
        // 4F0BE8 type 5 selector 2 reads the waist sound at hunter+9364.
        ContactSound::WaistArmour => (5, 2),
    };
    unsafe {
        let engine = (crate::memory::entry(0x1896f5c) as *const u32).read();
        if engine != 0 {
            let play: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32) =
                core::mem::transmute(crate::memory::entry(0x4f0be8));
            play(engine, actor, 0, 0, 0, group, variant);
        }
    }
}
