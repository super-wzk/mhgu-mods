use mhfz_sdk::action::{BranchRules, Cursor, Step, Time};
use mhgu_sdk::weapon::style::action::{
    Actions, MotionFrame,
    melee::{DamageKind, MonsterReaction, Strike, StrikePower},
};

const DATA: &[u8] = include_bytes!("../target/assets/magnet_spike.fmsa");
const STATES: u8 = 165;
const PARAMETERS: usize = DATA.len() - (4 + 83 * 2 + STATES as usize);
const COSTS: usize = PARAMETERS - (4 + STATES as usize);
const TURNS: usize = COSTS - (4 + STATES as usize * 2);
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
    assert!(half(4) == 7 && half(6) == STATES as u16);
    assert!(word(TURNS) == u32::from_le_bytes(*b"MSTR") as usize);
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

pub fn combo_turn(state: u8, yaw: u16) -> u16 {
    let limit = half(TURNS + 4 + 2 * usize::from(state)) as i16;
    match limit {
        0 => 0,
        -1 => match yaw {
            0x2000..=0x6000 => parameter(11).wrapping_neg(),
            0xa000..=0xe000 => parameter(11),
            _ => 0,
        },
        _ if yaw >= 0x8000 => (u16::MAX - yaw).min(limit as u16),
        _ => yaw.min(limit as u16).wrapping_neg(),
    }
}

pub fn boost_mode(state: u8) -> Option<Mode> {
    match state {
        163 => Some(Mode::Slash),
        164 => Some(Mode::Impact),
        _ => None,
    }
}

pub fn power_bonus(boosts: u8, mode: Mode) -> u8 {
    if boosts & (1 << mode as u8) != 0 {
        parameter(61) as u8
    } else {
        0
    }
}

pub fn is_attraction(state: u8) -> bool {
    matches!(state, 58 | 61 | 62 | 68 | 71 | 72)
}

pub fn is_bind_drop(state: u8) -> bool {
    matches!(state, 94 | 97)
}

pub fn flight_landing(state: u8) -> Option<u8> {
    // Landing table from FZ 8121FA84.
    match state {
        58 | 61 | 62 => Some(65),
        68 | 71 | 72 => Some(75),
        59 | 63 | 69 | 73 => Some(state + 1),
        94 | 97 => Some(98),
        _ => None,
    }
}

/// Family 121, row/shape selector 1 is the magnetic shot.
pub const SHOT: (f32, f32) = {
    assert!(word(EFFECTS) == u32::from_le_bytes(*b"MSFX") as usize);
    let shot = EFFECTS + 8 + 40;
    assert!(half(shot) == 0 && half(shot + 2) != 0);
    let duration = half(shot + 2) as f32;
    let shape = effect_shape(1);
    assert!(half(shape) == 126 && half(shape + 4) == 0);
    (duration, f32::from_bits(word(shape + 12) as u32))
};

/// Family 121, selector 3 is the zero-damage bind sphere placed at landing.
pub const BIND_CONTACT: (f32, f32) = {
    let row = EFFECTS + 8 + 3 * 40;
    assert!(half(row + 4) == 0 && half(row + 6) == 225);
    let shape = effect_shape(3);
    assert!(half(shape) == 127 && half(shape + 2) == 0 && half(shape + 4) == 0);
    assert!(word(shape + 16) == 0 && word(shape + 20) == 0 && word(shape + 24) == 0);
    (
        half(row + 2) as f32,
        f32::from_bits(word(shape + 12) as u32),
    )
};

pub fn bind_finisher_row() -> &'static [u8; 40] {
    const ROW: usize = EFFECTS + 8 + 8 * 40;
    const { assert!(half(ROW + 6) == 236) };
    DATA[ROW..ROW + 40].try_into().unwrap()
}

/// Family 121, selector 11 is the sphere created by evade event 42.
pub const EVADE_RADIUS: f32 = {
    let shape = effect_shape(11);
    assert!(half(shape) == 0 && half(shape + 4) == 0);
    assert!(word(shape + 16) == 0 && word(shape + 20) == 0 && word(shape + 24) == 0);
    f32::from_bits(word(shape + 12) as u32)
};

/// Family 121 row and shape 12 are the independent enhanced magnetic impact.
pub const SHOCK_STRIKE: Strike = {
    let row = EFFECTS + 8 + 12 * 40;
    assert!(half(row) == 0 && DATA[row + 15] == 1 && DATA[row + 28] == 12);
    Strike {
        delay: 0.,
        duration: half(row + 2) as f32,
        power: StrikePower {
            motion: half(row + 4),
            bonus: 0,
        },
        damage: DamageKind::Impact,
        stun: DATA[row + 33],
        hitstop: mhfz_sdk::combat::attacker_hitstop(DATA[row + 29]),
        deflectable: false,
        reaction: MonsterReaction::Native,
    }
};

pub const SHOCK_CAPSULE: (f32, f32) = {
    let shape = effect_shape(12);
    assert!(half(shape) == 127 && half(shape + 2) == 1);
    assert!(word(shape + 16) == 0 && word(shape + 24) == 0);
    assert!(word(shape + 28) == 0 && word(shape + 32) == 0 && word(shape + 36) == 0);
    (
        f32::from_bits(word(shape + 12) as u32),
        f32::from_bits(word(shape + 20) as u32),
    )
};

pub fn detached_strike(selector: u8) -> Strike {
    let row = EFFECTS + 8 + usize::from(selector) * 40;
    let (delay, duration) = mhfz_sdk::combat::contact_window(half(row), half(row + 2));
    Strike {
        delay,
        duration,
        power: StrikePower {
            motion: half(row + 4),
            bonus: 0,
        },
        damage: DamageKind::Slash,
        stun: DATA[row + 33],
        hitstop: mhfz_sdk::combat::attacker_hitstop(DATA[row + 29]),
        deflectable: false,
        reaction: MonsterReaction::Native,
    }
}

// These family-121 objects have identical world capsule geometry to row 12.
const _: () = {
    let mut i = 0;
    while i < 4 {
        let selector = [5, 6, 7, 10][i];
        let shape = effect_shape(selector);
        let mut offset = 0;
        while offset < 40 {
            assert!(DATA[shape + offset] == DATA[effect_shape(12) + offset]);
            offset += 1;
        }
        assert!(DATA[EFFECTS + 8 + selector * 40 + 15] == 2);
        i += 1;
    }
};

const fn effect_shape(index: usize) -> usize {
    let mut shape = EFFECTS + 8 + word(EFFECTS + 4) * 40;
    assert!(word(shape) == u32::from_le_bytes(*b"MSHT") as usize);
    shape += 4;
    assert!(half(shape) == 120);
    let selectors = half(shape + 2) as usize;
    shape += 4;
    let mut selector = 0;
    while selector < selectors {
        shape += 2 + half(shape) as usize * 40;
        selector += 1;
    }
    assert!(half(shape) == 121 && half(shape + 2) as usize > index);
    shape += 4;
    selector = 0;
    while selector < index {
        shape += 2 + half(shape) as usize * 40;
        selector += 1;
    }
    assert!(half(shape) == 1);
    shape + 2
}

pub fn confirmed_evade_branch(state: u8, target: u16) -> u16 {
    // X and A share the same execution window in each pair. A press can
    // be buffered before the collision callback sets condition 12.
    let branches = records(state, 1, 40).chunks_exact(40);
    for (ordinary, enhanced) in branches.clone().zip(branches.skip(1)) {
        if ordinary[22..24] == target.to_le_bytes()
            && ordinary[2..4] == [0, 0]
            && enhanced[2..4] == 12u16.to_le_bytes()
            && ordinary[..2] == enhanced[..2]
            && ordinary[4..22] == enhanced[4..22]
            && ordinary[24..] == enhanced[24..]
        {
            return u16::from_le_bytes([enhanced[22], enhanced[23]]);
        }
    }
    target
}

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

// FZ 8122482C scans the original command dispatch table at 82B3AED8.
pub const COMMAND_ORDER: &[u16] = &[
    23, 19, 24, 10, 17, 1, 7, 8, 9, 11, 0, 22, 2, 6, 15, 4, 3, 5, 18, 12, 16, 14, 13, 21, 20,
];

pub fn cursor(state: u8) -> Cursor {
    static RULES: BranchRules = BranchRules {
        order: COMMAND_ORDER,
        combo_override: None,
        art_commands: 1 << 0,
    };
    Cursor::new(state, records(state, 1, 40), records(state, 2, 12), &RULES)
}

#[repr(u8)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Slash,
    Impact,
}

impl Mode {
    pub fn recoil(self) -> u16 {
        // FZ 8121F57C branches to the recoil state for the current form.
        match self {
            Self::Slash => 92,
            Self::Impact => 93,
        }
    }

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

    // 8121ED84 tests these rows in their original order at 824F0CD4.
    pub fn roll_branches(self) -> impl Iterator<Item = (u16, u8)> {
        [
            (13, 14, 112),
            (24, 62, 72),
            (23, 66, 67),
            (11, 163, 164),
            (7, 135, 146),
            (9, 136, 138),
            (4, 141, 144),
            (6, 142, 145),
            (3, 142, 148),
            (2, 140, 143),
            (5, 55, 56),
            (1, 41, 45),
        ]
        .into_iter()
        .map(move |(cmd, slash, impact)| (cmd, if self == Self::Impact { impact } else { slash }))
    }

    pub fn opener(self, command: u16, drawn: bool) -> Option<u8> {
        // Draw: FZ 8121E6F8. Drawn: columns of the table at 824F0C28.
        match (self, command, drawn) {
            (_, 2, false) => Some(78),
            (_, 4, false) => Some(79),
            (_, 9, false) => Some(112),
            (Self::Slash, 1, true) => Some(40),
            (Self::Slash, 6, true) => Some(6),
            (Self::Slash, 2, true) => Some(0),
            (Self::Slash, 3, true) => Some(6),
            (Self::Slash, 4, true) => Some(2),
            (Self::Slash, 5, true) => Some(90),
            (Self::Slash, 7, true) => Some(11),
            (Self::Slash, 8, true) => Some(108),
            (Self::Slash, 9, true) => Some(12),
            (Self::Slash, 10, true) => Some(106),
            (Self::Slash, 11, true) => Some(163),
            (Self::Slash, 13, true) => Some(14),
            (Self::Slash, 17, true) => Some(107),
            (Self::Slash, 19, true) => Some(82),
            (Self::Slash, 23, true) => Some(66),
            (Self::Slash, 24, true) => Some(58),
            (Self::Impact, 1, true) => Some(44),
            (Self::Impact, 6, true) => Some(23),
            (Self::Impact, 2, true) => Some(18),
            (Self::Impact, 3, true) => Some(32),
            (Self::Impact, 4, true) => Some(26),
            (Self::Impact, 5, true) => Some(91),
            (Self::Impact, 7, true) => Some(39),
            (Self::Impact, 8, true) => Some(117),
            (Self::Impact, 9, true) => Some(38),
            (Self::Impact, 10, true) => Some(158),
            (Self::Impact, 11, true) => Some(164),
            (Self::Impact, 13, true) => Some(112),
            (Self::Impact, 17, true) => Some(116),
            (Self::Impact, 19, true) => Some(84),
            (Self::Impact, 23, true) => Some(67),
            (Self::Impact, 24, true) => Some(68),
            _ => None,
        }
    }
}
