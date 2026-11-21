//! Frontier w07 Extreme action program, in authored motion frames.
use mhfz_sdk::action::{BranchRules, Cursor, Step};
use mhgu_sdk::style::action::Actions;

const DATA: &[u8] = include_bytes!("../target/assets/long_sword.flsa");
const PARAMETERS: usize = DATA.len() - 64;
pub(super) const FULL_DURATION: f32 = 2. * half(DATA, PARAMETERS - 6) as f32;
const WAVE: usize = PARAMETERS - 6 - 16 - 160;
const _: () = assert!(word(WAVE - 4) == u32::from_le_bytes(*b"LS85") as usize);
const INSTANT: usize = WAVE - 48;
const _: () = assert!(word(INSTANT - 4) == u32::from_le_bytes(*b"LS04") as usize);
pub(super) fn instant() -> Strike {
    strike_at(INSTANT)
}
pub(super) const SWEEP_RADIUS: f32 = f32::from_bits(word(INSTANT + 40) as u32);
pub(super) const SWEEP_START: f32 = f32::from_bits(word(PARAMETERS + 32) as u32);
pub(super) const SWEEP_END: f32 = f32::from_bits(word(PARAMETERS + 36) as u32);
pub(super) fn wave(tier: usize) -> Strike {
    strike_at(WAVE + tier * 40)
}
pub(super) const DISCHARGE_REACH: f32 =
    (f32::from_bits(word(WAVE + 160) as u32) + f32::from_bits(word(WAVE + 164) as u32)) * 0.5;
pub(super) const STAB_REACH: f32 =
    f32::from_bits(word(WAVE + 168) as u32) + f32::from_bits(word(WAVE + 172) as u32);
const FRONTIER_STATE_COUNT: u8 = 85;
pub(super) const SPRINT: u8 = FRONTIER_STATE_COUNT;
pub(super) const BRAKE: u8 = SPRINT + 1;
pub(super) const CUT_MAX: u32 = word(PARAMETERS + 16) as u32;
pub(super) const CUT_FACTOR: f32 = f32::from_bits(word(PARAMETERS) as u32);
pub(super) const GUARD_GAIN: u32 = word(PARAMETERS + 48) as u32;
pub(super) const CHARGE_TICKS: u32 = word(PARAMETERS + 24) as u32;
pub(super) const DASH_COST: i32 = word(PARAMETERS + 28) as i32;
pub(super) const fn release(points: u32) -> u8 {
    if points >= CUT_MAX {
        84
    } else if points >= CUT_MAX / 2 {
        83
    } else {
        76
    }
}

const fn half(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

const fn word(at: usize) -> usize {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]]) as usize
}

pub(super) const ACTIONS: Actions = {
    assert!(half(DATA, 4) == 2 && half(DATA, 6) == FRONTIER_STATE_COUNT as u16);
    assert!(CUT_MAX > 0 && CUT_FACTOR > 0.);
    let mut actions = Actions::new(0).with(SPRINT).with(BRAKE);
    let mut state = 0;
    while state < FRONTIER_STATE_COUNT {
        if word(16 + 24 * state as usize) != 0 || (state >= 54 && state <= 65) {
            actions = actions.with(state);
        }
        state += 1;
    }
    actions
};

fn records(state: u8, field: usize, width: usize) -> &'static [u8] {
    if state >= FRONTIER_STATE_COUNT {
        return &[];
    }
    let descriptor = 16 + 24 * usize::from(state) + 8 * field;
    let start = word(descriptor + 4);
    &DATA[start..start + width * word(descriptor)]
}

pub(super) struct Strike {
    pub start: u16,
    pub duration: u16,
    pub power: u16,
    pub spirit: u8,
    // Independent contacts read the exported value. Blade contacts use the GU template.
    pub source_hitstop: u8,
    pub shape: u16,
}

pub(super) fn strike(id: u16) -> Option<Strike> {
    if usize::from(id) >= word(8) {
        return None;
    }
    Some(strike_at(word(12) + 40 * usize::from(id)))
}

fn strike_at(at: usize) -> Strike {
    Strike {
        start: half(DATA, at),
        duration: half(DATA, at + 2),
        power: half(DATA, at + 4),
        spirit: DATA[at + 30],
        source_hitstop: DATA[at + 29],
        shape: half(DATA, at + 16),
    }
}

pub(super) fn step(state: u8, index: u8, lead: u16) -> Option<Step> {
    if matches!(state, SPRINT | BRAKE | 54..=65) {
        // 811F5B06 joins the preceding attack into the release motion.
        let (clip, op) = match (state, index) {
            (SPRINT, 0) => (1409, 5),
            (BRAKE, 0) => (1411, 3),
            (54..=65, 0) if lead != 0 => (lead, 3),
            (54..=65, 1) if lead != 0 => (1437, 3),
            (54..=65, 0) => (1437, 3),
            _ => return None,
        };
        return Some(Step {
            op,
            clip,
            start: if clip == 1437 && index == 1 { 40 } else { 0 },
            blend: if clip != 1437 {
                2
            } else if lead == 0 {
                4
            } else {
                0
            },
            until: 0,
            loops: 1,
        });
    }
    Step::read(records(state, 0, 12), index)
}

pub(super) const CMD_SPRINT: u16 = 2;
pub(super) const CMD_X: u16 = 3;
pub(super) const CMD_STEP_X: u16 = 4;
pub(super) const CMD_EVADE_SLASH: u16 = 5;
pub(super) const CMD_EVADE_RIGHT: u16 = 6;
pub(super) const CMD_EVADE_LEFT: u16 = 7;
pub(super) const CMD_EVADE_BACK: u16 = 8;
pub(super) const CMD_A: u16 = 10;
pub(super) const CMD_SPIRIT: u16 = 14;
pub(super) const CMD_DISCHARGE: u16 = 15;
pub(super) const CMD_PARRY: u16 = 21;
pub(super) const CMD_RELEASE: u16 = 23;

const ORDER: [u16; 26] = [
    0,
    1,
    CMD_SPRINT,
    24,
    CMD_STEP_X,
    CMD_X,
    CMD_EVADE_RIGHT,
    CMD_EVADE_LEFT,
    CMD_EVADE_BACK,
    CMD_EVADE_SLASH,
    18,
    17,
    CMD_DISCHARGE,
    CMD_SPIRIT,
    19,
    12,
    13,
    11,
    CMD_A,
    9,
    20,
    16,
    25,
    CMD_PARRY,
    22,
    CMD_RELEASE,
];

static RULES: BranchRules = BranchRules {
    order: &ORDER,
    combo_override: Some((CMD_X..=CMD_STEP_X, CMD_EVADE_SLASH..=CMD_EVADE_BACK)),
    art_commands: 1 << 1 | 1 << CMD_SPRINT | 1 << CMD_PARRY,
};

pub(super) fn cursor(state: u8) -> Cursor {
    Cursor::new(state, records(state, 1, 40), records(state, 2, 12), &RULES)
}

pub(super) fn allows_discharge(state: u8) -> bool {
    records(state, 1, 40)
        .chunks_exact(40)
        .any(|row| half(row, 4) == CMD_DISCHARGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_x_only_executes_inside_the_source_window() {
        for (press, frame, want) in [
            (43., 80., None),
            (44., 79., None),
            (44., 80., Some(1)),
            (127., 127., Some(1)),
            (128., 128., None),
        ] {
            let mut c = cursor(0);
            c.time.frame = press;
            let early = c.branch(1 << 3, |id| id == 0);
            c.time.frame = frame;
            assert_eq!(early.or_else(|| c.branch(0, |id| id == 0)), want);
        }
    }
    #[test]
    fn parry_success_has_distinct_followups_and_paused_frames_do_not_repeat_events() {
        for state in [79, 80] {
            for (key, dest) in [(3, 112), (10, 81)] {
                let mut c = cursor(state);
                assert_eq!(c.branch(1 << key, |id| id == 6), None);
                c.time.frame = 20.;
                assert_eq!(c.branch(0, |id| id == 6), Some(dest));
            }
        }
        for (points, state, hits) in [
            (1, 76, 1),
            (424, 76, 1),
            (425, 83, 2),
            (849, 83, 2),
            (850, 84, 3),
        ] {
            assert_eq!(release(points), state);
            let mut c = cursor(state);
            let mut armed = 0;
            let mut count = 0;
            for step in 0..2 {
                c.time.step = step;
                for f in 0..300 {
                    c.time.frame = f as f32;
                    for _pause in 0..3 {
                        while let Some((event, _, _)) = c.next_event() {
                            count += usize::from(event == 17);
                            armed += usize::from(event == 25);
                        }
                    }
                }
            }
            assert_eq!((armed, count), (1, hits));
        }
        assert_eq!(FULL_DURATION, 1800.);
    }
}
