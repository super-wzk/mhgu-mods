use mhfz_sdk::action::{BranchRules, Cursor, Step, Time};
use mhgu_sdk::weapon::style::action::{Actions, MotionFrame};

const DATA: &[u8] = include_bytes!("../target/assets/magnet_spike.fmsa");
const STATES: u8 = 165;
const PARAMETERS: usize = DATA.len() - (4 + 83 * 2 + STATES as usize);
const MODES: usize = PARAMETERS + 4 + 83 * 2;

const fn half(at: usize) -> u16 {
    u16::from_le_bytes([DATA[at], DATA[at + 1]])
}

const fn word(at: usize) -> usize {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]]) as usize
}

pub const ACTIONS: Actions = {
    assert!(word(0) == u32::from_le_bytes(*b"FMSA") as usize);
    assert!(half(4) == 3 && half(6) == STATES as u16);
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
            (Self::Impact, 2, true) => Some(18),
            (Self::Impact, 3, true) => Some(32),
            (Self::Impact, 4, true) => Some(26),
            (Self::Impact, 5, true) => Some(91),
            _ => None,
        }
    }
}
