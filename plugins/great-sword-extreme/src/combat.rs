//! Register Frontier move events through GU's shared weapon collision volume.

use mhgu_sdk::{combat::collision::Contact, style::action::Attack};

/// Use GU's equipped sword shape, attributes, owner and collision serial.
/// FZ 81DBE87E subtracts the current motion frame from a nonzero SDT start
/// frame. A zero start uses the event time instead.
pub(super) unsafe fn emit(player: u32, source: u16, late: f32) -> Option<u32> {
    let [start, length, power] = crate::program::row(source)?;
    let elapsed = if start == 0 {
        late
    } else {
        unsafe { *((player + 1280) as *const f32) }
    };
    let until_start = f32::from(start) - elapsed;
    let left = until_start + f32::from(length);
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
        let template = match source {
            1 | 164..=168 => 5,
            2 | 261 => 6,
            _ => 1,
        };
        let delay = until_start.max(0.);
        let registered = Attack {
            template,
            power,
            delay,
            duration: left - delay,
            flags: 0x7,
            shape: extra.unwrap_or(0),
            no_sharpness_cost: u32::from(extra.is_some()),
        }
        .submit(player);
        if let Some(shape) = extra {
            release(shape);
        }
        extra.filter(|_| registered)
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
