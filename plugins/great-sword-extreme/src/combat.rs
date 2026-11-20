//! Register Frontier move events through GU's shared weapon collision volume.

use mhfz_sdk::combat::HitWindow;
use mhgu_sdk::{
    combat::collision::{Contact, HitShape},
    style::action::Attack,
};

/// Use GU's equipped sword shape, attributes, owner and collision serial.
/// FZ 81DBE87E subtracts the current motion frame from a nonzero SDT start
/// frame. A zero start uses the event time instead.
pub(super) unsafe fn emit(player: u32, source: u16, late: f32, deflectable: bool) -> Option<u32> {
    let [start, length, power] = crate::program::row(source)?;
    let frame = unsafe { *((player + 1280) as *const f32) };
    let HitWindow { delay, duration } = HitWindow::new(start, length, frame, late)?;
    unsafe {
        if *((player + 12696) as *const u32) == 0 || *((player + 12700) as *const u32) == 0 {
            return None;
        }
        let shape_path = match source {
            247 => Some(c"player\\hit\\frontier_gs\\guard_counter"),
            crate::program::RADIANCE => Some(c"player\\hit\\frontier_gs\\radiance"),
            _ => None,
        };
        let shape_override = match shape_path {
            Some(path) => Some(HitShape::load(path)?),
            None => None,
        };
        let shape = shape_override.as_ref().map_or(0, HitShape::as_raw);
        let template = match source {
            1 | 164..=168 => 5,
            2 | 261 => 6,
            _ => 1,
        };
        let radiance = source == crate::program::RADIANCE;
        let mut flags = 0x7;
        if radiance {
            flags |= Attack::NO_ATTACKER_HITSTOP;
        } else if deflectable {
            flags |= Attack::DEFLECTABLE;
        }
        let registered = Attack {
            template,
            power,
            delay,
            duration,
            flags,
            shape,
            no_sharpness_cost: u32::from(radiance),
        }
        .submit(player);
        shape_override
            .filter(|_| registered)
            .map(|shape| shape.as_raw())
    }
}

pub(super) unsafe extern "C" fn expand(owner: u32, hit: u32) -> u32 {
    unsafe {
        if hit == owner + 12320
            && let Some(origin) =
                crate::battle::radiance_origin(owner, *((hit + 132) as *const u32))
            && let Some(contact) = Contact::from_raw(hit)
        {
            let (radius, offsets) = crate::program::radiance_capsule();
            let ends = offsets.map(|end| core::array::from_fn(|i| origin[i] + end[i]));
            return u32::from(contact.submit_world_capsule(ends, radius));
        }
        0
    }
}
