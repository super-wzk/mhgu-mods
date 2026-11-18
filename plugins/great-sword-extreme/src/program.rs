//! Frontier's great sword action program. All clocks here are motion frames.
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
    (value(0), [[value(1), value(2), value(3)], [value(4), value(5), value(6)]])
}
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub op: u16,
    pub clip: u16,
    pub start: u16,
    pub blend: u16,
    pub until: u16,
    pub loops: u16,
}
pub fn step(state: u8, index: u8) -> Option<Step> {
    let s = records(state, 0, 12).chunks_exact(12).nth(index.into())?;
    Some(Step {
        op: half(s, 0),
        clip: half(s, 2),
        start: half(s, 4),
        blend: half(s, 6),
        until: half(s, 8),
        loops: half(s, 10),
    })
}
#[derive(Default, Clone, Copy)]
pub struct Time {
    pub step: u8,
    pub cycles: u16,
    pub frame: f32,
    pub blend: f32,
}
impl Time {
    fn reached(self, s: &[u8]) -> bool {
        let step = half(s, 0);
        if u16::from(self.step) != step {
            return u16::from(self.step) > step;
        }
        match s[2] {
            1 => true,
            2 => false,
            0 if (s[3] as i8) > 0 => self.blend <= f32::from(s[3]),
            0 => {
                self.cycles > half(s, 6)
                    || (self.cycles == half(s, 6) && self.frame >= f32::from(half(s, 4)))
            }
            _ => false,
        }
    }
}
#[derive(Default, Clone, Copy)]
pub struct Cursor {
    pub state: u8,
    pub time: Time,
    pub event: u16,
    buffer: Option<u8>,
}
impl Cursor {
    pub fn new(state: u8) -> Self {
        Self {
            state,
            ..Self::default()
        }
    }
    pub fn buffered_roll(&self) -> bool {
        self.buffer
            .is_some_and(|i| half(records(self.state, 1, 40), usize::from(i) * 40 + 22) == 74)
    }
    pub fn next_event(&mut self) -> Option<(u16, u16, f32)> {
        let e = records(self.state, 2, 12)
            .chunks_exact(12)
            .nth(self.event.into())?;
        if !self.time.reached(e) {
            return None;
        }
        self.event += 1;
        let late = if half(e, 0) == u16::from(self.time.step)
            && e[2] == 0
            && e[3] == 0
            && self.time.cycles == half(e, 6)
        {
            (self.time.frame - f32::from(half(e, 4))).max(0.)
        } else {
            0.
        };
        Some((half(e, 8), half(e, 10), late))
    }
    // The original buffers the highest priority accepted input separately from
    // its execution window. A predicate is checked again before the branch.
    pub fn branch(&mut self, commands: u32, predicate: impl Fn(u16) -> bool) -> Option<u16> {
        let table = records(self.state, 1, 40);
        let previous = self.buffer.map(|i| half(table, usize::from(i) * 40));
        // 811D4978 chooses one available command before the state scheduler
        // compares branch priorities. A direction must win over its neutral alias.
        const ORDER: [u8; 32] = [
            0, 1, 31, 2, 6, 17, 8, 9, 7, 11, 12, 10, 24, 25, 23, 4, 5, 3, 27, 26, 14, 13, 29, 30,
            28, 16, 15, 18, 21, 22, 20, 19,
        ];
        let mut candidate = None;
        let mut rank = ORDER.len();
        for (index, b) in table.chunks_exact(40).enumerate() {
            if previous.is_some_and(|p| p >= half(b, 0))
                || !predicate(half(b, 2))
                || commands & (1u32 << half(b, 4)) == 0
                || !self.time.reached(&b[6..])
                || self.time.reached(&b[14..])
            {
                continue;
            }
            let order = ORDER.iter().position(|&c| u16::from(c) == half(b, 4))?;
            if order < rank
                || (order == rank
                    && candidate.is_none_or(|i: usize| half(table, i * 40) <= half(b, 0)))
            {
                candidate = Some(index);
                rank = order;
            }
        }
        if let Some(i) = candidate {
            self.buffer = Some(i as u8);
        }
        let i = usize::from(self.buffer?) * 40;
        let b = &table[i..i + 40];
        (predicate(half(b, 2)) && self.time.reached(&b[24..]) && !self.time.reached(&b[32..]))
            .then(|| half(b, 22))
    }
}
pub fn charged_row(level: u8) -> u16 {
    [3, 91, 92, 93][usize::from(level.min(3))]
}

// 81ECC418: pass through neutral, deflect, release, then deflect again in the
// same direction. Holding the stick or tapping in opposite directions fails.
#[derive(Default, Clone, Copy)]
pub struct Flick {
    phase: u8,
    direction: u16,
    remaining: f32,
}
impl Flick {
    pub fn sample(&mut self, strength: i16, angle: u16, elapsed: f32) -> bool {
        self.remaining = (self.remaining - elapsed).max(0.);
        if self.remaining == 0. && self.phase >= 2 {
            self.phase = 0
        }
        match self.phase {
            0 if strength < 48 => self.phase = 1,
            1 if strength >= 80 => {
                self.phase = 2;
                self.direction = angle;
                self.remaining = 12.;
            }
            2 if strength < 48 => {
                self.phase = 3;
                self.remaining = 12.;
            }
            2 if (angle.wrapping_sub(self.direction) as i16).unsigned_abs() > 0x3000 => {
                self.phase = 0
            }
            3 if strength >= 80 => {
                self.phase = 0;
                return (angle.wrapping_sub(self.direction) as i16).unsigned_abs() <= 0x3000;
            }
            _ => {}
        }
        false
    }
}
