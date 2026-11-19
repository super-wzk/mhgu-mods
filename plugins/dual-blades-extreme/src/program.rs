//! Frontier w06 Extreme action program, in authored motion frames.
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

#[derive(Clone, Copy)]
pub(super) struct Step {
    pub op: u16,
    pub clip: u16,
    pub start: u16,
    pub blend: u16,
    pub until: u16,
    pub loops: u16,
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
    let row = records(state, 0, 12).chunks_exact(12).nth(index.into())?;
    Some(Step {
        op: half(row, 0),
        clip: half(row, 2),
        start: half(row, 4),
        blend: half(row, 6),
        until: half(row, 8),
        loops: half(row, 10),
    })
}

#[derive(Clone, Copy, Default)]
pub(super) struct Time {
    pub step: u8,
    pub cycles: u16,
    pub frame: f32,
    pub blend: f32,
}

impl Time {
    fn reached(self, record: &[u8]) -> bool {
        let step = half(record, 0);
        if u16::from(self.step) != step {
            return u16::from(self.step) > step;
        }
        match record[2] {
            1 => true,
            0 if record[3] as i8 > 0 => self.blend <= f32::from(record[3]),
            0 => {
                let cycle = half(record, 6);
                self.cycles > cycle
                    || self.cycles == cycle && self.frame >= f32::from(half(record, 4))
            }
            _ => false,
        }
    }
}

pub(super) struct Cursor {
    pub state: u8,
    pub time: Time,
    event: usize,
    pending: Option<usize>,
}

impl Cursor {
    pub fn new(state: u8) -> Self {
        Self {
            state,
            time: Time::default(),
            event: 0,
            pending: None,
        }
    }

    pub fn next_event(&mut self) -> Option<(u16, u16, f32)> {
        let row = records(self.state, 2, 12)
            .chunks_exact(12)
            .nth(self.event)?;
        if !self.time.reached(row) {
            return None;
        }
        self.event += 1;
        let late = if half(row, 0) == u16::from(self.time.step)
            && row[2] == 0
            && row[3] == 0
            && half(row, 6) == self.time.cycles
        {
            (self.time.frame - f32::from(half(row, 4))).max(0.)
        } else {
            0.
        };
        Some((half(row, 8), half(row, 10), late))
    }

    pub fn branch(&mut self, commands: u32, eligible: impl Fn(u16) -> bool) -> Option<u16> {
        const ORDER: [u16; 28] = [
            23, 25, 24, 22, 0, 12, 11, 17, 18, 19, 20, 1, 4, 3, 2, 7, 6, 5, 8, 9, 10, 13, 15, 16,
            14, 26, 27, 21,
        ];
        let table = records(self.state, 1, 40);
        self.pending = self
            .pending
            .filter(|&at| !self.time.reached(&table[at + 32..]));
        let previous = self
            .pending
            .map(|at| (half(table, at), half(table, at + 4)));
        for command in ORDER {
            if commands & (1 << command) == 0 {
                continue;
            }
            let candidate = table
                .chunks_exact(40)
                .enumerate()
                .filter(|(_, row)| {
                    half(row, 4) == command
                        && previous.is_none_or(|(priority, pending)| {
                            let next = half(row, 0);
                            // GU can resolve X+A after X was already queued.
                            next > priority
                                || (next == priority
                                    && matches!(pending, 2..=4)
                                    && matches!(command, 5..=7))
                        })
                        && eligible(half(row, 2))
                        && self.time.reached(&row[6..])
                        && !self.time.reached(&row[14..])
                })
                .max_by_key(|(_, row)| half(row, 0));
            if let Some((i, _)) = candidate {
                self.pending = Some(i * 40);
                break;
            }
        }
        let at = self.pending?;
        let row = &table[at..at + 40];
        if eligible(half(row, 2)) && self.time.reached(&row[24..]) && !self.time.reached(&row[32..])
        {
            // GU may defer the change during hitstop. The next action gets a
            // new cursor on acceptance, so keep this input until its window ends.
            Some(half(row, 22))
        } else {
            None
        }
    }

    pub fn art_window(&self, eligible: impl Fn(u16) -> bool) -> bool {
        records(self.state, 1, 40).chunks_exact(40).any(|row| {
            matches!(half(row, 4), 0 | 22..=25)
                && eligible(half(row, 2))
                && self.time.reached(&row[24..])
                && !self.time.reached(&row[32..])
        })
    }
}
