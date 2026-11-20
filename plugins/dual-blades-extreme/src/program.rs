//! Frontier w06 Extreme action program, in authored motion frames.
use mhfz_sdk::action::{BranchRules, Cursor, Step};
use mhgu_sdk::style::action::Actions;

const DATA: &[u8] = include_bytes!("../target/assets/dual_blades.fdba");
const PARAMETERS: usize = DATA.len() - 216;
const FRONTIER_STATE_COUNT: u8 = 89;
const AERIAL_MOUNT_FINISH_SPEED: f32 = 6.0;
pub(super) const AERIAL_DANCE: u8 = 78;
pub(super) const AERIAL_FINISH: u8 = 80;
pub(super) const SPRINT: u8 = FRONTIER_STATE_COUNT;
pub(super) const BRAKE: u8 = SPRINT + 1;
pub(super) const AERIAL_MOUNT_FINISH: u8 = BRAKE + 1;

const fn half(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

const fn word(at: usize) -> usize {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]]) as usize
}

pub(super) const ACTIONS: Actions = {
    assert!(half(DATA, 4) == 2 && half(DATA, 6) == FRONTIER_STATE_COUNT as u16);
    let mut actions = Actions::new(0)
        .with(SPRINT)
        .with(BRAKE)
        .with(AERIAL_MOUNT_FINISH);
    let mut state = 0;
    while state < FRONTIER_STATE_COUNT {
        if word(16 + 24 * state as usize) != 0 {
            actions = actions.with(state);
        }
        state += 1;
    }
    actions
};

fn records(state: u8, field: usize, width: usize) -> &'static [u8] {
    let state = if state == AERIAL_MOUNT_FINISH {
        AERIAL_FINISH
    } else {
        state
    };
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
    pub shape: u16,
}

pub(super) fn strike(id: u16) -> Option<Strike> {
    if usize::from(id) >= word(8) {
        return None;
    }
    let at = word(12) + 40 * usize::from(id);
    Some(Strike {
        start: half(DATA, at),
        duration: half(DATA, at + 2),
        power: half(DATA, at + 4),
        shape: half(DATA, at + 16),
    })
}

const fn parameter(offset: usize) -> f32 {
    f32::from_bits(word(PARAMETERS + offset) as u32)
}

pub(super) struct AerialMotion {
    pub landing: u8,
    pub forward_speed: f32,
    pub vertical_speed: f32,
    pub vertical_acceleration: f32,
}

pub(super) const fn aerial_motion(state: u8) -> Option<AerialMotion> {
    match state {
        AERIAL_DANCE => Some(AerialMotion {
            landing: AERIAL_DANCE + 1,
            forward_speed: parameter(196) / 2.,
            vertical_speed: 0.,
            vertical_acceleration: parameter(200) / 4.,
        }),
        AERIAL_FINISH | AERIAL_MOUNT_FINISH => Some(AerialMotion {
            landing: AERIAL_FINISH + 1,
            forward_speed: if state == AERIAL_MOUNT_FINISH {
                AERIAL_MOUNT_FINISH_SPEED
            } else {
                parameter(204) / 2.
            },
            vertical_speed: parameter(208) / 2.,
            vertical_acceleration: parameter(212) / 4.,
        }),
        _ => None,
    }
}

// w06 parameters read by 811F0FEC and its logic-frame and hit callers.
pub(super) const FLAME_MAX: u32 = half(DATA, PARAMETERS + 180) as u32;
pub(super) const FLAME_PER_TICK: u32 = half(DATA, PARAMETERS + 182) as u32;
pub(super) const FLAME_PER_HIT: u32 = half(DATA, PARAMETERS + 184) as u32;
const _: () = assert!(FLAME_MAX > 0);

pub(super) fn finish_window() -> f32 {
    2. * f32::from(half(DATA, PARAMETERS + 192))
}

pub(super) fn step(state: u8, index: u8) -> Option<Step> {
    if matches!(state, SPRINT | BRAKE) && index == 0 {
        return Some(Step {
            op: if state == SPRINT { 5 } else { 3 },
            clip: if state == SPRINT { 1030 } else { 1031 },
            start: 0,
            blend: 8,
            until: 0,
            loops: 0,
        });
    }
    Step::read(records(state, 0, 12), index)
}

const ORDER: [u16; 28] = [
    23, 25, 24, 22, 0, 12, 11, 17, 18, 19, 20, 1, 4, 3, 2, 7, 6, 5, 8, 9, 10, 13, 15, 16, 14, 26,
    27, 21,
];

static RULES: BranchRules = BranchRules {
    order: &ORDER,
    combo_override: Some((2..=4, 5..=7)),
    art_commands: 1 | 0xf << 22,
};

pub(super) fn cursor(state: u8) -> Cursor {
    Cursor::new(state, records(state, 1, 40), records(state, 2, 12), &RULES)
}
