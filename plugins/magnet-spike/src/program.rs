use mhfz_sdk::action::{BranchRules, Cursor, Step, Time};
use mhgu_sdk::weapon::style::action::{Actions, MotionFrame};

const DATA: &[u8] = include_bytes!("../target/assets/magnet_spike.fmsa");
const STATES: u8 = 165;
const PARAMETERS: usize = DATA.len() - (4 + 83 * 2 + STATES as usize);
const COSTS: usize = PARAMETERS - (4 + STATES as usize);
const MODES: usize = PARAMETERS + 4 + 83 * 2;
const EFFECTS: usize = {
    // The exporter appends the state arrays in descriptor order.
    let mut index = STATES as usize * 3;
    loop {
        index -= 1;
        let descriptor = 16 + index * 8;
        let count = word(descriptor);
        if count != 0 {
            let stride = if index % 3 == 1 { 40 } else { 12 };
            break word(descriptor + 4) + count * stride;
        }
    }
};

const fn half(at: usize) -> u16 {
    u16::from_le_bytes([DATA[at], DATA[at + 1]])
}

const fn word(at: usize) -> usize {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]]) as usize
}

pub const ACTIONS: Actions = {
    assert!(word(0) == u32::from_le_bytes(*b"FMSA") as usize);
    assert!(half(4) == 4 && half(6) == STATES as u16);
    assert!(word(COSTS) == u32::from_le_bytes(*b"MSCT") as usize);
    assert!(word(PARAMETERS) == u32::from_le_bytes(*b"MSPR") as usize);
    let mut actions = Actions::new(0);
    let mut id = 0;
    while id < STATES {
        if word(16 + 24 * id as usize) != 0 {
            actions = actions.with(id);
        }
        id += 1;
    }
    actions
};

fn records(state: u8, field: usize, width: usize) -> &'static [u8] {
    let desc = 16 + 24 * usize::from(state) + 8 * field;
    let at = word(desc + 4);
    &DATA[at..at + width * word(desc)]
}

pub fn step(state: u8, index: u8) -> Option<Step> {
    Step::read(records(state, 0, 12), index)
}

pub fn melee_row(id: u16) -> Option<&'static [u8; 40]> {
    if usize::from(id) >= word(8) {
        return None;
    }
    let at = word(12) + usize::from(id) * 40;
    DATA[at..at + 40].try_into().ok()
}

pub const fn parameter(id: usize) -> u16 {
    assert!(id < 83);
    half(PARAMETERS + 4 + id * 2)
}

pub fn cost(state: u8) -> u8 {
    DATA[COSTS + 4 + usize::from(state)]
}

pub fn is_attraction(state: u8) -> bool {
    matches!(state, 58 | 61 | 62 | 68 | 71 | 72)
}

pub fn flight_landing(state: u8) -> Option<u8> {
    // Landing table from FZ 8121FA84.
    match state {
        58 | 61 | 62 => Some(65),
        68 | 71 | 72 => Some(75),
        59 | 63 | 69 | 73 => Some(state + 1),
        _ => None,
    }
}

/// Family 121, row/shape selector 1 is the magnetic shot.
pub const SHOT: (f32, f32) = {
    assert!(word(EFFECTS) == u32::from_le_bytes(*b"MSFX") as usize);
    let shot = EFFECTS + 8 + 40;
    assert!(half(shot) == 0 && half(shot + 2) != 0);
    let duration = half(shot + 2) as f32;
    let mut shape = EFFECTS + 8 + word(EFFECTS + 4) * 40;
    assert!(word(shape) == u32::from_le_bytes(*b"MSHT") as usize);
    shape += 4;
    assert!(half(shape) == 120);
    let selectors = half(shape + 2);
    shape += 4;
    let mut selector = 0;
    while selector < selectors {
        shape += 2 + half(shape) as usize * 40;
        selector += 1;
    }
    assert!(half(shape) == 121 && half(shape + 2) > 1);
    shape += 4;
    shape += 2 + half(shape) as usize * 40;
    assert!(half(shape) == 1 && half(shape + 2) == 126 && half(shape + 6) == 0);
    let radius = f32::from_bits(word(shape + 2 + 12) as u32);
    (duration, radius)
};

pub fn step_complete(step: Step, time: Time, sample: MotionFrame, wrapped: bool) -> bool {
    // FZ 81815D60 gates op 4 on the cycle count and 817332B0's frame check.
    if step.op == 4 && step.loops != 0 {
        let cycles = time
            .cycles
            .saturating_add(u16::from(sample.complete() && !wrapped));
        return cycles >= step.loops && sample.blend <= 0. && step.reached_frame(sample.frame);
    }
    step.complete(time, sample, wrapped)
}

pub fn cursor(state: u8) -> Cursor {
    // FZ 8122482C, ordered by the original command dispatch table at 82B3AED8.
    static RULES: BranchRules = BranchRules {
        order: &[
            23, 19, 24, 10, 17, 1, 7, 8, 9, 11, 0, 22, 2, 6, 15, 4, 3, 5, 18, 12, 16, 14, 13, 21,
            20,
        ],
        combo_override: None,
        art_commands: 1 << 0,
    };
    Cursor::new(state, records(state, 1, 40), records(state, 2, 12), &RULES)
}

#[derive(Clone, Copy, Default)]
pub enum Mode {
    #[default]
    Slash,
    Impact,
}

impl Mode {
    pub fn coast_clip(self) -> u16 {
        match self {
            Self::Slash => 1471,
            Self::Impact => 1461,
        }
    }

    pub fn for_state(self, state: u8) -> Self {
        match DATA[MODES + usize::from(state)] {
            0 => Self::Slash,
            1 => Self::Impact,
            _ => self,
        }
    }

    pub fn opener(self, command: u8, drawn: bool) -> Option<u8> {
        // Draw: FZ 8121E6F8. Drawn: columns of the table at 824F0C28.
        match (self, command, drawn) {
            (_, 2, false) => Some(78),
            (_, 4, false) => Some(79),
            (Self::Slash, 2, true) => Some(0),
            (Self::Slash, 3, true) => Some(6),
            (Self::Slash, 4, true) => Some(2),
            (Self::Slash, 5, true) => Some(90),
            (Self::Slash, 9, true) => Some(12),
            (Self::Slash, 19, true) => Some(82),
            (Self::Slash, 23, true) => Some(66),
            (Self::Slash, 24, true) => Some(58),
            (Self::Impact, 2, true) => Some(18),
            (Self::Impact, 3, true) => Some(32),
            (Self::Impact, 4, true) => Some(26),
            (Self::Impact, 5, true) => Some(91),
            (Self::Impact, 9, true) => Some(38),
            (Self::Impact, 19, true) => Some(84),
            (Self::Impact, 23, true) => Some(67),
            (Self::Impact, 24, true) => Some(68),
            _ => None,
        }
    }
}
