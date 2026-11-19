//! FZ holds the incoming frame during blending; GU normally advances both clocks.
use super::{Actors, package::Directory};
use mhgu_sdk::monster::{Monster, motion::MotionId};

struct Clock {
    motion: MotionId,
    advance: f32,
}
static CLOCKS: Actors<Clock> = Actors::new();

pub(super) fn forget(actor: u32) {
    CLOCKS.remove(actor);
}

pub(super) fn begin(monster: Monster) {
    record(
        monster,
        if monster.motion_blend_remaining() > 2. {
            0.
        } else {
            1.
        },
    );
}

fn record(monster: Monster, advance: f32) {
    CLOCKS.with(|clocks| {
        clocks.insert(
            monster.as_raw(),
            Clock {
                motion: monster.motion(),
                advance,
            },
        );
    });
}

/// The collision pass uses the same fraction of this update as the motion clock.
pub(super) fn advance(monster: Monster) -> f32 {
    CLOCKS.with(|clocks| {
        clocks
            .get(&monster.as_raw())
            .filter(|clock| clock.motion == monster.motion())
            .map_or(1., |clock| clock.advance)
    })
}

#[forge::hook(offset = 0x951a80)]
pub unsafe fn tick(actor: u32) -> u32 {
    // 951A80 also updates players and small scene objects. Inspect only their
    // common vtable word before borrowing the much larger monster layout.
    if actor == 0
        || unsafe { crate::u32at(actor, 0) } != forge::mem::text_addr() + crate::ACTOR_VTABLE_OFFSET
    {
        return original!(actor);
    }
    let Some(monster) = (unsafe { Monster::from_raw(actor) }) else {
        return original!(actor);
    };
    let Some(directory) = Directory::read(monster) else {
        return original!(actor);
    };
    if !unsafe { directory.contains_motion(monster.motion()) } {
        return original!(actor);
    }
    let dt = monster.frame_delta();
    // 8173208A: n+1 FZ ticks blend to the fixed initial frame. The final
    // tick advances it by 2*rate. Preserve partial GU updates at that boundary.
    let held = (monster.motion_blend_remaining() - 2.).max(0.);
    let fraction = if dt > 0. {
        (1. - held / dt).clamp(0., 1.)
    } else {
        0.
    };
    record(monster, fraction);
    monster.with_motion_clock_rate(monster.motion_rate() * fraction, || original!(actor))
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, tick);
}
