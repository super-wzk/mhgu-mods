//! Register Frontier move events through GU's shared weapon collision volume.

use mhfz_sdk::combat::{HitWindow, attacker_hitstop};
use mhgu_sdk::{
    combat::collision::{CachedHitShape, Contact, HitShape},
    style::action::Attack,
};

static GUARD_COUNTER: CachedHitShape =
    CachedHitShape::new(c"player\\hit\\frontier_gs\\guard_counter");
static RADIANCE: CachedHitShape = CachedHitShape::new(c"player\\hit\\frontier_gs\\radiance");

pub(super) unsafe fn preload() {
    unsafe {
        GUARD_COUNTER.preload();
        RADIANCE.preload();
    }
}

/// Use GU's equipped sword shape, attributes, owner and collision serial.
/// FZ 81DBE87E subtracts the current motion frame from a nonzero SDT start
/// frame. A zero start uses the event time instead.
pub(super) unsafe fn emit(player: u32, source: u16, late: f32, deflectable: bool) -> Option<u32> {
    let row = crate::program::strike(source)?;
    let frame = unsafe { *((player + 1280) as *const f32) };
    let HitWindow { delay, duration } = HitWindow::new(row.start, row.duration, frame, late)?;
    unsafe {
        if *((player + 12696) as *const u32) == 0 || *((player + 12700) as *const u32) == 0 {
            return None;
        }
        let shape_override = match source {
            247 => Some(GUARD_COUNTER.load()?),
            crate::program::RADIANCE => Some(RADIANCE.load()?),
            _ => None,
        };
        let shape = shape_override.as_ref().map_or(0, HitShape::as_raw);
        let template = match source {
            1 | 164..=168 => 5,
            2 | 261 => 6,
            _ => 1,
        };
        let radiance = source == crate::program::RADIANCE;
        let mut flags = Attack::WEAPON_MODIFIERS;
        if matches!(source, 164..=168) {
            flags |= Attack::PUNISHING_DRAW;
        }
        if radiance {
            flags |= Attack::NO_ATTACKER_HITSTOP;
        } else if deflectable {
            flags |= Attack::DEFLECTABLE;
        }
        let registered = Attack {
            template,
            power: row.power,
            delay,
            duration,
            flags,
            shape,
            no_sharpness_cost: u32::from(radiance),
        }
        .submit_with_hitstop(player, attacker_hitstop(row.source_hitstop));
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
