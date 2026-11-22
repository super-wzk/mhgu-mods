//! Frontier w09 action steps, input windows and melee rows.
use mhfz_sdk::action::{BranchRules, Cursor, Step};
use mhgu_sdk::style::action::Actions;

const DATA: &[u8] = include_bytes!("../target/assets/gunlance.fgla");
const STATES: u8 = 82;
pub const SPRINT: u8 = 82;
pub const BRAKE: u8 = 83;
pub const DRAW: u8 = 20;

pub const EMPTY: u8 = 14;
pub const HEAT_BLADE: u8 = 28;
pub const RECOIL: u8 = 7;
const PARAMS: usize = DATA.len() - 220;

pub const QUICK_RELOAD: usize = 0;
pub const WYVERN_WINDUP: usize = 2;
pub const WYVERN_COOLDOWN: usize = 3;
pub const FIRST_CHAMBER: usize = 4;
pub const NEXT_CHAMBER: usize = 5;
pub const IGNITION: usize = 6;
// FZ 8181EE9E reads the same base value for the separate ignition timer.
pub const IGNITION_COOLDOWN: usize = WYVERN_COOLDOWN;
// FZ w09 parameters +284 and +288. The port maps these two reductions
// to GU's artillery skill levels.
pub const WYVERN_COOLDOWN_STRONG_REDUCTION: f32 = f32::from_bits(word(PARAMS + 20));
pub const WYVERN_COOLDOWN_WEAK_REDUCTION: f32 = f32::from_bits(word(PARAMS + 24));

pub fn timer(index: usize) -> f32 {
    // FZ timers tick at 30 Hz, while GU elapsed time is in 60 Hz frames.
    2. * f32::from(half(PARAMS + 4 + 2 * index))
}

pub fn burst_limit(shell_type: u32) -> u8 {
    [8, 4, 5][shell_type as usize] // FZ 824F0594.
}

pub fn fire_cooldown(shell_type: u32, charge: u8, barrage: bool) -> f32 {
    let scale = if charge == 0 {
        1.
    } else {
        let offset = PARAMS + 28 + 96 * usize::from(barrage);
        f32::from_bits(word(
            offset + 32 * shell_type as usize + 4 * usize::from(charge - 1),
        ))
    };
    // Frontier truncates the shell multiplier at 30 Hz before applying skills.
    2. * ((timer(WYVERN_COOLDOWN) * 0.5 * scale) as u16) as f32
}

const fn half(at: usize) -> u16 {
    u16::from_le_bytes([DATA[at], DATA[at + 1]])
}

const fn word(at: usize) -> u32 {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]])
}

pub const ACTIONS: Actions = {
    assert!(word(0) == u32::from_le_bytes(*b"FGLA") && half(4) == 2 && half(6) == STATES as u16);
    assert!(word(PARAMS) == u32::from_le_bytes(*b"GLP1"));
    let mut actions = Actions::new(0).with(SPRINT).with(BRAKE);
    let mut state = 0;
    while state < STATES {
        if word(16 + 24 * state as usize) != 0 || native_motion(state, 0).is_some() {
            actions = actions.with(state);
        }
        state += 1;
    }
    actions
};

static BRANCH_RULES: BranchRules = BranchRules {
    // Local player command priority in Vita 81205A28.
    order: &[1, 0, 10, 9, 2, 4, 8, 3, 5, 7, 6, 12, 11],
    combo_override: Some((3..=5, 6..=8)),
    art_commands: 1,
};

fn records(state: u8, field: usize, stride: usize) -> &'static [u8] {
    if state >= STATES {
        return &[];
    }
    let row = 16 + 24 * usize::from(state) + 8 * field;
    let offset = word(row + 4) as usize;
    &DATA[offset..offset + stride * word(row) as usize]
}

pub fn cursor(state: u8) -> Cursor {
    Cursor::new(
        state,
        records(state, 1, 40),
        records(state, 2, 12),
        &BRANCH_RULES,
    )
}

// FZ native steps are outside the w09 action array. The first two columns
// of 824F05AC are the neutral Wyvern Fire poses, and 824F05D0 holds discharge.
const fn native_motion(state: u8, index: u8) -> Option<(u16, u16, u16)> {
    match (state, index) {
        (3, 0) => Some((1407, 4, 3)),
        (3, 1) => Some((1418, 0, 5)),
        (37, 0) => Some((1442, 4, 3)),
        (37, 1) => Some((1444, 0, 5)),
        (38, 0) => Some((1441, 4, 3)),
        (38, 1) => Some((1443, 0, 5)),
        (59, 0) => Some((1408, 0, 3)),
        (60, 0) => Some((1452, 0, 3)),
        (61, 0) => Some((1451, 0, 3)),
        // 824F04BC and 824F04D0, loading and short discharge.
        (47, 0) => Some((1437, 4, 3)),
        (47, 1) => Some((1438, 0, 5)),
        (49, 0) => Some((1460, 4, 3)),
        (49, 1) => Some((1461, 0, 5)),
        (48, 0) => Some((1440, 0, 3)),
        (50, 0) => Some((1463, 0, 3)),
        (SPRINT, 0) => Some((1030, 2, 5)),
        (BRAKE, 0) => Some((1031, 4, 3)),
        _ => None,
    }
}

pub fn step(state: u8, index: u8) -> Option<Step> {
    if let Some((clip, blend, op)) = native_motion(state, index) {
        Some(Step {
            op,
            clip,
            start: 0,
            blend,
            until: 0,
            loops: 1,
        })
    } else {
        Step::read(records(state, 0, 12), index)
    }
}

pub struct Strike {
    pub start: u16,
    pub duration: u16,
    pub power: u16,
}

pub fn strike(id: u16) -> Option<Strike> {
    if u32::from(id) >= word(8) {
        return None;
    }
    let at = word(12) as usize + 40 * usize::from(id);
    Some(Strike {
        start: half(at),
        duration: half(at + 2),
        power: half(at + 4),
    })
}

// FZ 81206314. The GU overlay uses the standard reload and shell routes.
pub fn eligible(condition: u16, heat_blade: bool) -> bool {
    matches!(condition, 0 | 1 | 2 | 4 | 6) || (condition == 7 && heat_blade)
}

// FZ 812063F8 resolves transition IDs 82..107 before playing an action.
pub fn destination(
    id: u16,
    loaded: bool,
    can_reload: bool,
    heat_blade: bool,
    shell_type: u32,
    cooled: bool,
    ignition_ready: bool,
) -> u8 {
    match id {
        85 => 0,
        86 => 4,
        87 => {
            if can_reload {
                27
            } else {
                4
            }
        }
        91..=99 if !loaded => EMPTY,
        91..=99 => {
            const NORMAL: [u8; 9] = [1, 24, 53, 12, 25, 54, 13, 26, 55];
            const CHARGED: [u8; 9] = [34, 34, 56, 35, 35, 57, 36, 36, 58];
            let table = if heat_blade { CHARGED } else { NORMAL };
            table[usize::from(id - 91)]
        }
        100 if !loaded => 32,
        100 => {
            (if heat_blade {
                [31, 44, 45]
            } else {
                [30, 42, 43]
            })[shell_type as usize]
        }
        101..=103 if !cooled => 19,
        101..=103 => [3, 37, 38][usize::from(id - 101)],
        104 | 105 if !loaded => EMPTY,
        104 => 47,
        105 => 49,
        106 if ignition_ready && !heat_blade => HEAT_BLADE,
        106 => 80,
        107 if loaded => {
            if heat_blade {
                81
            } else {
                72
            }
        }
        107 => 71,
        _ => id as u8,
    }
}
