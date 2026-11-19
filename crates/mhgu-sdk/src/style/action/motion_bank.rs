use core::{
    ffi::CStr,
    sync::atomic::{AtomicU32, Ordering},
};

/// Three paired Frontier motion groups, with one set of resources per sex.
pub struct MotionBanks {
    paths: [[&'static CStr; 3]; 2],
    resources: [[AtomicU32; 3]; 2],
}

impl MotionBanks {
    pub const fn new(paths: [[&'static CStr; 3]; 2]) -> Self {
        Self {
            paths,
            resources: [const { [const { AtomicU32::new(0) }; 3] }; 2],
        }
    }

    /// Bind a clip to a free runtime motion bank on the hunter.
    ///
    /// # Safety
    /// The actor must be a live GU hunter with equipped gear on the game thread.
    pub unsafe fn encode(&self, actor: u32, source: u16) -> Option<u16> {
        let group = match source / 100 {
            10 => 0,
            12 => 1,
            14 => 2,
            _ => return None,
        };
        let slot = u32::from(source % 100);
        unsafe {
            let equipment = ((actor + 5060) as *const u32).read();
            if equipment == 0 {
                return None;
            }
            let sex = usize::from(((equipment + 1240) as *const u8).read());
            let resource = self.resource(sex, group, slot)?;
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

    /// Resolve a packed motion ID only when the bank belongs to this style.
    ///
    /// # Safety
    /// The actor must be a live GU hunter on the game thread.
    pub unsafe fn decode(&self, actor: u32, packed: u16) -> Option<u16> {
        let bank = u32::from(packed >> 8);
        if !(8..16).contains(&bank) {
            return None;
        }
        let resource = unsafe { ((actor + 3844 + 4 * bank) as *const u32).read() };
        if resource == 0 {
            return None;
        }
        (0..3)
            .find(|&group| {
                self.resources
                    .iter()
                    .any(|sex| sex[group].load(Ordering::Acquire) == resource)
            })
            .map(|group| 1000 + 200 * group as u16 + (packed & 255))
    }

    unsafe fn resource(&self, sex: usize, group: usize, slot: u32) -> Option<u32> {
        let cache = &self.resources.get(sex)?[group];
        let path = self.paths[sex][group];
        unsafe {
            let has_clip: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(crate::memory::entry(0xb49e54));
            let cached = cache.load(Ordering::Acquire);
            if cached != 0 {
                return (has_clip(cached, slot) != 0).then_some(cached);
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
            if ((resource + 80) as *const u32).read() & 1 == 0 || has_clip(resource, slot) == 0 {
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
