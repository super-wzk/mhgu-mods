//! Species-80 normal quest hitzones and anger gauge from mhfemd.bin.
use super::*;

unsafe fn hunt_state<'a>(actor: u32) -> Option<(Directory<'a>, MonsterState<'a>)> {
    let monster = unsafe { Monster::from_raw(actor)? };
    Some((Directory::read(monster)?, monster.state()?))
}

// GU asks for the current ten-byte row separately for each contacting body part.
#[forge::hook(offset = 0xbab60)]
pub unsafe fn hitzone(actor: u32, part: u32) -> u32 {
    unsafe {
        if part < 7
            && let Some((d, state)) = hunt_state(actor)
        {
            let enraged = u32::from(state.anger_mode() == AngerMode::Enraged);
            return d.physiology() + 60 + (7 * enraged + part) * 10;
        }
    }
    original!(actor, part)
}

// Native GU damage dispatch supplies the amount before its own anger multipliers.
// 81699694/81699828: truncate HP percent and the per-hit contribution separately.
#[forge::hook(offset = 0xbcc04)]
pub unsafe fn add_anger(actor: u32, amount: i32) -> u32 {
    unsafe {
        if let Some((d, state)) = hunt_state(actor) {
            let p = d.physiology();
            let max_hp = state.max_hp() as u32;
            if max_hp != 0 && state.anger_mode() == AngerMode::Calm {
                let percent = u64::from(state.hp() as u32) * 100 / u64::from(max_hp);
                let lane = 10 - (percent.min(100) as u32 / 10);
                let factor = f32::from_bits(u32at(p, 16 + 4 * lane));
                let next = (i32::from(state.anger_gauge()) + (amount as f32 * factor) as i32)
                    .clamp(0, i32::from(u16at(p, 0)));
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
        let entering = if next == u32::from(AngerMode::Enraged.raw()) {
            hunt_state(actor)
                .and_then(|(d, state)| (state.anger_mode() != AngerMode::Enraged).then_some(d))
        } else {
            None
        };
        let result = original!(actor, next);
        if let Some(d) = entering
            && let Some(state) = Monster::from_raw(actor).and_then(Monster::state)
        {
            // GU counts animation frames; Vita duration is in 30 Hz logic ticks.
            let frames = 2.0 * f32::from(u16at(d.physiology(), 2));
            state.set_anger_timer(frames);
        }
        result
    }
}
