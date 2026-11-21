//! Positional hunter sound requests on GU's audio queue.
use core::{
    ffi::CStr,
    sync::atomic::{AtomicU32, Ordering},
};

pub struct Bank {
    path: &'static CStr,
    resource: AtomicU32,
}

impl Bank {
    pub const fn new(path: &'static CStr) -> Self {
        Self {
            path,
            resource: AtomicU32::new(0),
        }
    }

    /// Cache the native sound request and its resources without playing a cue.
    ///
    /// # Safety
    /// Call on the game thread after the resource manager has initialized.
    pub unsafe fn preload(&self) {
        unsafe { self.load() };
    }

    unsafe fn load(&self) -> Option<u32> {
        unsafe {
            let cached = self.resource.load(Ordering::Acquire);
            if cached != 0 {
                return Some(cached);
            }
            let manager = (crate::memory::entry(0x2126a64) as *const u32).read();
            if manager == 0 {
                return None;
            }
            let vtable = (manager as *const *const u32).read();
            let load: unsafe extern "C" fn(u32, usize, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(vtable.add(12).read());
            let resource = load(
                manager,
                crate::memory::entry(0x2125bec),
                self.path.as_ptr(),
                1,
            );
            if resource == 0 {
                return None;
            }
            let release: unsafe extern "C" fn(u32) =
                core::mem::transmute(crate::memory::entry(0x887660));
            if ((resource + 80) as *const u32).read() & 1 == 0 {
                release(resource);
                return None;
            }
            Some(
                match self.resource.compare_exchange(
                    0,
                    resource,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => resource,
                    Err(winner) => {
                        release(resource);
                        winner
                    }
                },
            )
        }
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
            let Some(resource) = self.load() else {
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
