use core::{
    ffi::CStr,
    sync::atomic::{AtomicU32, Ordering},
};

/// GU hunter LMT resources shared by both models. Slots 128–255 hold
/// optional female-model variants of the corresponding slots 0–127.
pub struct MotionBanks<const GROUPS: usize> {
    paths: [&'static CStr; GROUPS],
    resources: [AtomicU32; GROUPS],
}

impl<const GROUPS: usize> MotionBanks<GROUPS> {
    pub const VARIANT_BIT: u8 = 128;

    pub const fn new(paths: [&'static CStr; GROUPS]) -> Self {
        Self {
            paths,
            resources: [const { AtomicU32::new(0) }; GROUPS],
        }
    }

    /// Bind a clip to a free runtime motion bank on the hunter.
    ///
    /// # Safety
    /// The actor must be a live GU hunter with equipped gear on the game thread.
    pub unsafe fn encode(&self, actor: u32, group: usize, slot: u8) -> Option<u16> {
        if slot >= Self::VARIANT_BIT {
            return None;
        }
        unsafe {
            let equipment = ((actor + 5060) as *const u32).read();
            if equipment == 0 {
                return None;
            }
            let female = match ((equipment + 1240) as *const u8).read() {
                0 => false,
                1 => true,
                _ => return None,
            };
            let resource = self.resource(group, u32::from(slot))?;
            let variant = slot | Self::VARIANT_BIT;
            let slot = u32::from(if female && has_clip(resource, u32::from(variant)) {
                variant
            } else {
                slot
            });
            let mut free = None;
            for bank in (8..16).rev() {
                let r = ((actor + 3844 + 4 * bank) as *const u32).read();
                if r == resource {
                    return Some((bank * 256 + slot) as u16);
                }
                if r == 0 && free.is_none() {
                    free = Some(bank);
                }
            }
            let bank = free?;
            let bind: unsafe extern "C" fn(u32, u32, u32) =
                core::mem::transmute(crate::memory::entry(0x957400));
            bind(actor, resource, bank);
            Some((bank * 256 + slot) as u16)
        }
    }

    /// Cache a motion resource without taking a runtime bank on the hunter.
    ///
    /// # Safety
    /// Call on the game thread with an initialized resource manager.
    pub unsafe fn preload(&self, group: usize, slot: u8) {
        unsafe { self.resource(group, u32::from(slot)) };
    }

    /// Resolve the caller's group from a packed GU motion ID.
    ///
    /// # Safety
    /// The actor must be a live GU hunter on the game thread.
    pub unsafe fn group(&self, actor: u32, packed: u16) -> Option<usize> {
        let bank = u32::from(packed >> 8);
        if !(8..16).contains(&bank) {
            return None;
        }
        let resource = unsafe { ((actor + 3844 + 4 * bank) as *const u32).read() };
        if resource == 0 {
            return None;
        }
        self.resources
            .iter()
            .position(|bank| bank.load(Ordering::Acquire) == resource)
    }

    unsafe fn resource(&self, group: usize, slot: u32) -> Option<u32> {
        unsafe {
            let cache = self.resources.get(group)?;
            let path = self.paths[group];
            let cached = cache.load(Ordering::Acquire);
            if cached != 0 {
                return has_clip(cached, slot).then_some(cached);
            }
            let manager = (crate::memory::entry(0x2126a64) as *const u32).read();
            if manager == 0 {
                return None;
            }
            let vtable = (manager as *const *const u32).read();
            let load: unsafe extern "C" fn(u32, usize, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(vtable.add(12).read());
            let resource = load(manager, crate::memory::entry(0x2125210), path.as_ptr(), 1);
            if resource == 0 {
                return None;
            }
            let release: unsafe extern "C" fn(u32) =
                core::mem::transmute(crate::memory::entry(0x887660));
            if ((resource + 80) as *const u32).read() & 1 == 0 || !has_clip(resource, slot) {
                release(resource);
                return None;
            }
            Some(
                match cache.compare_exchange(0, resource, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => resource,
                    Err(winner) => {
                        release(resource);
                        winner
                    }
                },
            )
        }
    }
}

unsafe fn has_clip(resource: u32, slot: u32) -> bool {
    unsafe {
        let lookup: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(crate::memory::entry(0xb49e54));
        lookup(resource, slot) != 0
    }
}
