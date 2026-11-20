//! Frontier's great sword action program. All clocks here are motion frames.
use mhfz_sdk::action::{BranchRules, Cursor, Step};
pub const DATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/assets/great_sword.fgsa"
));

fn half(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn word(at: usize) -> usize {
    u32::from_le_bytes(DATA[at..at + 4].try_into().unwrap()) as usize
}
fn records(state: u8, field: usize, width: usize) -> &'static [u8] {
    let p = 16 + 24 * usize::from(state) + 8 * field;
    let start = word(p + 4);
    &DATA[start..start + width * word(p)]
}
pub fn row(id: u16) -> Option<[u16; 3]> {
    if id == RADIANCE {
        let p = DATA.len() - 80;
        return Some([half(DATA, p), half(DATA, p + 2), half(DATA, p + 4)]);
    }
    if usize::from(id) >= word(8) {
        return None;
    }
    let p = word(12) + 40 * usize::from(id);
    Some([half(DATA, p), half(DATA, p + 2), half(DATA, p + 4)])
}
pub const RADIANCE: u16 = 0xff08;
pub fn radiance_capsule() -> (f32, [[f32; 3]; 2]) {
    let p = DATA.len() - 28;
    let value = |i: usize| f32::from_bits(word(p + 4 * i) as u32);
    (
        value(0),
        [
            [value(1), value(2), value(3)],
            [value(4), value(5), value(6)],
        ],
    )
}
pub fn step(state: u8, index: u8) -> Option<Step> {
    Step::read(records(state, 0, 12), index)
}

// 811D4978 picks a command before comparing authored branch priorities.
static RULES: BranchRules = BranchRules {
    order: &[
        0, 1, 31, 2, 6, 17, 8, 9, 7, 11, 12, 10, 24, 25, 23, 4, 5, 3, 27, 26, 14, 13, 29, 30, 28,
        16, 15, 18, 21, 22, 20, 19,
    ],
    combo_override: None,
    art_commands: 0,
};

pub fn cursor(state: u8) -> Cursor {
    let (branches, events) = if state < 74 {
        (records(state, 1, 40), records(state, 2, 12))
    } else {
        (&[][..], &[][..])
    };
    Cursor::new(state, branches, events, &RULES)
}

pub fn hunter_art_window(cursor: &Cursor, hit: bool) -> bool {
    // GU Hunter Art policy for the port. The step and frame ranges preserve
    // the previous cancel timings. An end of zero keeps the current step open.
    let time = cursor.time;
    let frame = if time.cycles == 0 {
        time.frame
    } else {
        f32::INFINITY
    };
    let (step, start, end) = match cursor.state {
        0 => (0, 72., 0.),
        2 | 13 | 16 | 30..=32 => (1, 48., 0.),
        5..=8 => (1, 72., 0.),
        14 | 15 | 17 | 18 => (2, 48., 0.),
        19 => (0, 58., 0.),
        20 | 21 | 25..=28 | 71 => (1, 58., 0.),
        22 => (0, 24., 0.),
        24 => (0, 26., 0.),
        29 => (0, 48., 0.),
        33 => (0, 120., 0.),
        34..=36 => (1, 120., 0.),
        37 | 38 => (1, 72., if hit { 104. } else { 98. }),
        40 | 43 | 44 if frame < 74. => (1, 32., 74.),
        40 | 43 | 44 => (1, 136., 0.),
        41 | 45 => (1, 78., 0.),
        42 | 46 => (1, 68., 0.),
        47 | 49..=52 | 70 => (1, 54., 0.),
        48 | 53 => (0, 54., 0.),
        54 => (0, 68., 0.),
        55..=58 => (0, 76., 0.),
        59 => {
            return time.step == 1
                && (time.cycles > 1 || time.cycles == 1 && time.frame >= 20.)
                && (time.cycles < 4 || time.cycles == 4 && time.frame < 134.);
        }
        61 | 65 => (0, 56., 0.),
        62 | 63 => (0, 64., 0.),
        64 => (0, 86., 0.),
        66 | 72 => (0, 20., 0.),
        67 | 73 => (0, 0., 0.),
        68 => (0, 128., 0.),
        69 => (0, 84., 0.),
        _ => return false,
    };
    time.step == step && frame >= start && (end == 0. || frame < end)
}

pub fn charged_row(level: u8) -> u16 {
    [3, 91, 92, 93][usize::from(level.min(3))]
}
