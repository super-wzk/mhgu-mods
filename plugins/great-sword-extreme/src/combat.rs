//! Register Frontier move events through GU's shared weapon collision volume.

use mhgu_sdk::combat::collision::Contact;

/// Use GU's actual equipped sword shape, attributes, owner and collision serial.
/// A delayed FZ event is relative to its dispatch time; GU's normal HDP start
/// time is absolute within the clip. Set the two remaining timers after setup.
pub(super) unsafe fn emit(player: u32, source: u16, late: f32) -> Option<u32> {
    let [delay, length, power] = crate::program::row(source)?;
    let left = f32::from(delay) + f32::from(length) - late;
    if left <= 0.0 {
        return None;
    }
    unsafe {
        if *((player + 12696) as *const u32) == 0 || *((player + 12700) as *const u32) == 0 {
            return None;
        }
        let base = forge::mem::text_addr();
        let release: unsafe extern "C" fn(u32) = core::mem::transmute(base + 0x887660);
        let extra = if source == crate::program::RADIANCE {
            let mgr = *((base + 0x2126a64) as *const u32);
            if mgr == 0 {
                return None;
            }
            let vt = *(mgr as *const u32);
            let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(*((vt + 48) as *const u32));
            let r = load(
                mgr,
                base + 0x1857810,
                c"player\\hit\\frontier_gs\\radiance".as_ptr(),
                1,
            );
            if r == 0 {
                return None;
            }
            if *((r + 80) as *const u32) & 1 == 0 {
                release(r);
                return None;
            }
            Some(r)
        } else {
            None
        };
        let slot = match source {
            1 | 164..=168 => 5,
            2 | 261 => 6,
            _ => 1,
        };
        let native: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            core::mem::transmute(forge::mem::text_addr() + 0x2b7240);
        native(player, 0, slot, 0);
        let hit = player + 12320;
        if *((hit + 132) as *const u32) == 0 || *((hit + 12) as *const u32) != player {
            if let Some(r) = extra {
                release(r);
            }
            return None;
        }
        if let Some(r) = extra {
            let old = core::ptr::replace((hit + 132) as *mut u32, r);
            *((hit + 136) as *mut u32) = 0;
            // GU's hit resolver checks this flag before applying sharpness
            // loss. Each Frontier radiance pulse is a non-consuming hit.
            *((hit + 48) as *mut u32) |= 0x08000000;
            release(old);
        }
        let wait = (f32::from(delay) - late).max(0.);
        *((hit + 92) as *mut f32) = wait;
        *((hit + 96) as *mut f32) = left - wait;
        *((hit + 56) as *mut u16) = power;
        *((hit + 5) as *mut u8) = 1;
        extra
    }
}

#[forge::hook(offset = 0x1696fc)]
unsafe fn expand(hit: u32) -> u32 {
    unsafe {
        let owner = *((hit + 12) as *const u32);
        if owner != 0
            && hit == owner + 12320
            && let Some(origin) =
                crate::battle::radiance_origin(owner, *((hit + 132) as *const u32))
            && let Some(contact) = Contact::from_raw(hit)
        {
            let (radius, offsets) = crate::program::radiance_capsule();
            let ends = offsets.map(|end| core::array::from_fn(|i| origin[i] + end[i]));
            return u32::from(contact.submit_world_capsule(ends, radius));
        }
        original!(hit)
    }
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), expand);
}
