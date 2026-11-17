//! Species-80 normal quest hitzones and anger gauge from mhfemd.bin.
use super::{Actors, package::Directory};
use mhgu_sdk::monster::damage::HitzoneRow;
use mhgu_sdk::monster::{Monster, MonsterState, anger::AngerMode};

// The GU setter can run inside a hunt update or independently during perception.
static RAGE_ENTRIES: Actors<()> = Actors::new();

pub(super) fn take_rage_entry(actor: u32) -> bool {
    RAGE_ENTRIES.with(|entries| entries.remove(&actor).is_some())
}

pub(super) fn forget(actor: u32) {
    RAGE_ENTRIES.remove(actor);
}

unsafe fn hunt_state<'a>(actor: u32) -> Option<(Monster<'a>, Directory<'a>, MonsterState<'a>)> {
    let monster = unsafe { Monster::from_raw(actor)? };
    Some((monster, Directory::read(monster)?, monster.state()?))
}

// GU already initialized the normal rows from this actor's DTT (BABF4).
// Select its alternate rows on rage transitions only; BAB60 remains native.
fn select_hitzones(monster: Monster, enraged: bool) {
    for part in 0..7 {
        monster.select_hitzone(
            part,
            if enraged {
                HitzoneRow::Alternate(part)
            } else {
                HitzoneRow::Default
            },
        );
    }
}

// Native GU damage dispatch supplies the amount before its own anger multipliers.
// 81699694/81699828: truncate HP percent and the per-hit contribution separately.
#[forge::hook(offset = 0xbcc04)]
pub unsafe fn add_anger(actor: u32, amount: i32) -> u32 {
    unsafe {
        if let Some((_, d, state)) = hunt_state(actor) {
            let max_hp = state.max_hp() as u32;
            if max_hp != 0 && state.anger_mode() == AngerMode::Calm {
                let percent = u64::from(state.hp() as u32) * 100 / u64::from(max_hp);
                let factor = d.anger_gain_factor(percent);
                let next = (i32::from(state.anger_gauge()) + (amount as f32 * factor) as i32)
                    .clamp(0, i32::from(d.anger_threshold()));
                state.set_anger_gauge(next as u16);
                return next as u32;
            }
            return u32::from(state.anger_gauge());
        }
    }
    original!(actor, amount)
}

#[forge::hook(offset = 0xbce6c)]
pub unsafe fn mode(actor: u32, next: u32) -> u32 {
    unsafe {
        let change = hunt_state(actor).and_then(|(monster, _, state)| {
            let was = state.anger_mode() == AngerMode::Enraged;
            let enraged = next == u32::from(AngerMode::Enraged.raw());
            (was != enraged).then_some((monster, state, enraged))
        });
        let result = original!(actor, next);
        if let Some((monster, state, enraged)) = change
            && (state.anger_mode() == AngerMode::Enraged) == enraged
        {
            select_hitzones(monster, enraged);
            if enraged {
                RAGE_ENTRIES.with(|entries| entries.insert(actor, ()));
            } else {
                forget(actor);
            }
        }
        result
    }
}

// GU 76154 also ends ordinary rage through BA874, bypassing BCE6C.
#[forge::hook(offset = 0xba874)]
pub unsafe fn reset_anger(actor: u32, keep_secondary: u32) -> u32 {
    let enraged = unsafe { hunt_state(actor) }.and_then(|(monster, _, state)| {
        (state.anger_mode() == AngerMode::Enraged).then_some(monster)
    });
    let result = original!(actor, keep_secondary);
    if let Some(monster) = enraged {
        forget(actor);
        select_hitzones(monster, false);
    }
    result
}
