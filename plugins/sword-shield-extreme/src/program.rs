//! Paired motion steps, buffered branches and melee events from the w04 tables.
use mhfz_sdk::action::{BranchRules, Cursor, Step};
use mhgu_sdk::style::action::Actions;

const DATA: &[u8] = include_bytes!("../target/assets/sword_shield.fsns");
const STATES: u8 = 53;
pub const SPRINT: u8 = 53;
pub const BRAKE: u8 = 54;
pub const SPIN: u8 = 2;
pub const DRAW: u8 = 9;
pub const SPRINT_SLASH: u8 = 36;
pub const VACUUM: u8 = 55; // also identifies the enhanced move in GU peer action packets

pub const JUMP_SLASH: u8 = 56;
pub const BACKSTEP: u8 = 57;

const fn half(at: usize) -> u16 {
    u16::from_le_bytes([DATA[at], DATA[at + 1]])
}
const fn word(at: usize) -> u32 {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]])
}

pub const ACTIONS: Actions = {
    assert!(word(0) == u32::from_le_bytes(*b"FSNS") && half(4) == 2 && half(6) == STATES as u16);
    let mut bits = Actions::new(0)
        .with(SPRINT)
        .with(BRAKE)
        .with(VACUUM)
        .with(JUMP_SLASH)
        .with(BACKSTEP);
    let mut i = 0;
    while i < STATES {
        if word(16 + 24 * i as usize) > 0 {
            bits = bits.with(i);
        }
        i += 1;
    }
    bits
};
const RULES: BranchRules = BranchRules {
    // FZ 811EA5D4, first matching enabled command.
    order: &[
        15, 16, 17, 0, 18, 10, 1, 14, 11, 13, 3, 4, 2, 6, 5, 7, 8, 9, 12, 19, 20, 21,
    ],
    combo_override: None,
    art_commands: 1 | (1 << 1) | (1 << 2),
};
fn records(state: u8, field: usize, width: usize) -> &'static [u8] {
    let state = match state {
        VACUUM => SPIN,
        JUMP_SLASH => SPRINT_SLASH,
        _ => state,
    };
    if state >= STATES {
        return &[];
    }
    let at = 16 + 24 * usize::from(state) + 8 * field;
    let start = word(at + 4) as usize;
    &DATA[start..start + width * word(at) as usize]
}
pub fn cursor(state: u8) -> Cursor {
    Cursor::new(state, records(state, 1, 40), records(state, 2, 12), &RULES)
}
pub fn step(state: u8, index: u8) -> Option<Step> {
    if index == 0 {
        let extra = match state {
            SPRINT => Some((5, 1448, 2)),
            BRAKE => Some((3, 1449, 2)),
            // FZ common action 0:145 starts 1422 at 817E6A72.
            BACKSTEP => Some((3, 1422, 4)),
            _ => None,
        };
        if let Some((op, clip, blend)) = extra {
            return Some(Step {
                op,
                clip,
                blend,
                start: 0,
                until: 0,
                loops: 1,
            });
        }
    }
    Step::read(records(state, 0, 12), index)
}
// 811EB426 adds 4 units forward per FZ tick in the advancing shield loop.
pub fn play_clip(state: u8, clip: u16) -> u16 {
    if (47..=50).contains(&state) && (1454..=1457).contains(&clip) {
        clip + 11
    } else {
        clip
    }
}

pub struct Strike {
    pub start: u16,
    pub duration: u16,
    pub power: u16,
    pub shape: u16,
    pub stun: u8,
    pub source_hitstop: u8,
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
        shape: half(at + 16),
        stun: DATA[at + 33],
        source_hitstop: DATA[at + 29],
    })
}
const PARAM: usize = DATA.len() - 64;
pub const WAVE_HEIGHT: f32 = f32::from_bits(word(PARAM + 8));
pub const WAVE_DURATION: u16 = word(PARAM + 20) as u16;
pub const WAVE_SPEED: f32 = f32::from_bits(word(PARAM + 32));
pub const WAVE_POWER: u16 = word(PARAM + 48) as u16;

// 8172C3D4 supplies the sectors used by w04's input selector, 811EA5D4.
pub fn input_sector(yaw: u16) -> usize {
    match yaw.wrapping_add(0x2000) {
        0..=0x4000 => 0,
        0x4001..=0x8000 => 1,
        0x8001..=0xbfff => 2,
        _ => 3,
    }
}

// 811EAD82 resolves table destinations 53..66 for style 3.
pub fn destination(id: u16, yaw: Option<u16>) -> Option<(u8, i16)> {
    let (state, turn) = match id {
        0..=52 => (id as u8, 0),
        55 => (SPRINT, 0),
        56 => (13, 0),
        57 => (8, 0),
        58..=61 => {
            let turn = match yaw {
                Some(1..=0x7fff) => -8192,
                Some(0x8001..=0xffff) => 8192,
                _ => 0,
            };
            ([1, SPIN, 11, 13][usize::from(id - 58)], turn)
        }
        62..=66 => {
            // 811EACAE uses eight directions for shield combo steering.
            let sector = yaw.map(|angle| angle.wrapping_add(0x1000) >> 13);
            let index = usize::from(id - 62);
            let state = if matches!(sector, Some(0 | 1 | 7)) {
                [47, 48, 49, 50, 52][index]
            } else {
                [43, 44, 45, 46, 51][index]
            };
            let turn = match sector {
                Some(1..=3) => -1024,
                Some(5..=7) => 1024,
                _ => 0,
            };
            (state, turn)
        }
        _ => return None,
    };
    Some((state, turn))
}
