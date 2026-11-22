//! Source action, equipment, parameter and SDT tables for Frontier weapon 12.
use mhfz_sdk::action::{BranchRules, Cursor, Step, Time};
use mhgu_sdk::style::action::{Actions, MotionFrame};

const DATA: &[u8] = include_bytes!("../target/assets/switch_axe.fsaa");
const COUNT: usize = 178;
const PARAMS: usize = DATA.len() - 150;
const BURSTS: usize = PARAMS - 6 * 40;
const GEOMETRY: usize = BURSTS - 4 - 4 * 16;
const MODELS: usize = GEOMETRY - 72 * 8;
pub const AXE_SPRINT: u8 = 103;
pub const SWORD_SPRINT: u8 = 148;
pub const AXE_BRAKE: u8 = 105;
pub const SWORD_BRAKE: u8 = 151;
const AXE_SPRINT_RESUME: u8 = 178;
const SWORD_SPRINT_RESUME: u8 = 179;
// Both FZ sprint clips loop from frame 90.
const SPRINT_LOOP_START: u16 = 90;

pub const fn source_state(state: u8) -> u8 {
    match state {
        AXE_SPRINT_RESUME => AXE_SPRINT,
        SWORD_SPRINT_RESUME => SWORD_SPRINT,
        _ => state,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SprintForm {
    Axe,
    Sword,
}

impl SprintForm {
    pub const fn from_action(state: u8) -> Option<Self> {
        match source_state(state) {
            AXE_SPRINT => Some(Self::Axe),
            SWORD_SPRINT => Some(Self::Sword),
            _ => None,
        }
    }

    pub fn resume(self) -> u8 {
        match self {
            Self::Axe => AXE_SPRINT_RESUME,
            Self::Sword => SWORD_SPRINT_RESUME,
        }
    }

    pub fn brake(self) -> u8 {
        match self {
            Self::Axe => AXE_BRAKE,
            Self::Sword => SWORD_BRAKE,
        }
    }
}

pub const fn is_sprint(state: u8) -> bool {
    SprintForm::from_action(state).is_some()
}

pub fn burst_geometry(id: u16) -> Option<([f32; 3], f32)> {
    let index = match id {
        1 | 2 => 0,
        3..=5 => usize::from(id - 2),
        _ => return None,
    };
    let at = GEOMETRY + 16 * index;
    let field = |n: usize| f32::from_bits(word(at + 4 * n) as u32);
    Some(([field(0), field(1), field(2)], field(3)))
}

const fn half(at: usize) -> u16 {
    u16::from_le_bytes([DATA[at], DATA[at + 1]])
}
const fn word(at: usize) -> usize {
    u32::from_le_bytes([DATA[at], DATA[at + 1], DATA[at + 2], DATA[at + 3]]) as usize
}
pub const ACTIONS: Actions = {
    assert!(
        word(0) == u32::from_le_bytes(*b"FSAA") as usize
            && half(4) == 4
            && half(6) as usize == COUNT
    );
    assert!(word(BURSTS - 4) == u32::from_le_bytes(*b"SAFX") as usize && word(8) == 81);
    let mut actions = Actions::new(0);
    let mut state = 0;
    while state < COUNT {
        if word(16 + state * 24) != 0 {
            actions = actions.with(state as u8);
        }
        state += 1;
    }
    actions.with(AXE_SPRINT_RESUME).with(SWORD_SPRINT_RESUME)
};

pub const fn param(i: usize) -> u16 {
    half(PARAMS + 2 * i)
}

fn records(state: u8, field: usize, width: usize) -> &'static [u8] {
    let desc = 16 + 24 * usize::from(source_state(state)) + 8 * field;
    let at = word(desc + 4);
    &DATA[at..at + width * word(desc)]
}
pub fn step(state: u8, index: u8) -> Option<Step> {
    let mut step = Step::read(records(state, 0, 12), index)?;
    if state != source_state(state) {
        step.start = SPRINT_LOOP_START;
    }
    Some(step)
}
pub fn step_complete(step: Step, time: Time, sample: MotionFrame, wrapped: bool) -> bool {
    // FZ 81815E78 checks the completed cycle count, then 817332B0 checks
    // the displayed frame. GU 951A80 also sets its end bit when it wraps.
    if step.op == 4 && step.loops != 0 {
        return time.cycles >= step.loops && sample.blend <= 0. && step.reached_frame(sample.frame);
    }
    step.complete(time, sample, wrapped)
}

// FZ 8121B914 uses whole degrees, rounding toward zero before testing sectors.
pub fn stick_degrees(yaw: i16) -> i16 {
    (i32::from(yaw) * 360 / 65536) as i16
}

pub fn discharge_dodge(state: u8) -> bool {
    matches!(state, 70 | 74 | 77 | 98)
}

pub fn cursor(state: u8) -> Cursor {
    // 8121B914 checks motion, unconditional and button requests in this order.
    static RULES: BranchRules = BranchRules {
        order: &[
            1, 0, 25, 23, 12, 13, 14, 6, 26, 11, 24, 27, 17, 18, 19, 5, 20, 22, 21, 7, 8, 9, 2, 10,
            3, 4, 15, 16,
        ],
        combo_override: None,
        art_commands: (1 << 12) | (1 << 23) | (1 << 26),
    };
    static DISCHARGE_RULES: BranchRules = BranchRules {
        // Let a completed directional chord replace an equal-priority X or A
        // follow-up while it is still waiting to execute.
        combo_override: Some((2..=3, 17..=19)),
        ..RULES
    };
    let rules = if discharge_dodge(state) {
        &DISCHARGE_RULES
    } else {
        &RULES
    };
    Cursor::new(state, records(state, 1, 40), records(state, 2, 12), rules)
}

pub struct Strike {
    pub start: u16,
    pub duration: u16,
    pub power: u16,
    pub shape: usize,
}
pub fn strike(burst: bool, id: u16) -> Option<Strike> {
    let (base, count) = if burst {
        (BURSTS, 6)
    } else {
        (word(12), word(8))
    };
    if usize::from(id) >= count {
        return None;
    }
    let row = base + 40 * usize::from(id);
    Some(Strike {
        start: half(row),
        duration: half(row + 2),
        power: half(row + 4),
        shape: usize::from(half(row + 16)),
    })
}

pub const AXE_IDLE_MOTION: u8 = 14;
pub const SWORD_IDLE_MOTION: u8 = 15;
pub const SWORD_TO_AXE: u8 = 31;
pub const RETRACT_START: f32 = 64.;
pub const RETRACT_END: f32 = 76.;

pub fn idle_model_motion(sword: bool) -> u8 {
    if sword {
        SWORD_IDLE_MOTION
    } else {
        AXE_IDLE_MOTION
    }
}

// FZ 8121CC50 selects an equipment clip, then 824F0A8C supplies its ending form.
pub fn equipment_motion(clip: u16, sword: bool) -> (u8, bool) {
    let Some(at) = (MODELS..GEOMETRY).step_by(8).find(|&at| half(at) == clip) else {
        return (idle_model_motion(sword), sword);
    };
    let alt = DATA[at + 3];
    let id = if sword && alt != 0 { alt } else { DATA[at + 2] };
    const FORM: [u8; 19] = [0, 0, 0, 1, 1, 2, 1, 2, 2, 1, 2, 1, 2, 1, 1, 2, 0, 2, 1];
    (id, FORM[id as usize] == 2)
}

// Visible sword intervals of the weapon model motion (8121CF7E).
pub fn sword_pose(model_motion: u8, frame: f32) -> bool {
    match model_motion {
        5 | 13 => frame < 10.,
        7 | 8 | SWORD_IDLE_MOTION => true,
        9 => frame < 50.,
        10 => frame >= 60.,
        11 => frame < RETRACT_START,
        12 => frame >= 30.,
        17 => frame >= 38.,
        18 => (10. ..62.).contains(&frame),
        _ => false,
    }
}

pub fn charge_clip(clip: u16) -> bool {
    matches!(clip, 1488 | 1489 | 1063 | 1064)
}

pub fn is_fade_slash_charge(state: u8) -> bool {
    matches!(state, 123 | 124 | 128 | 129)
}

// FZ 812132C4 uses param 33 for the light-sword sharpness cutoff.
pub const AXE_RECOIL: u8 = 34;
pub const LIGHT_SWORD_RECOIL: u8 = 35;
pub const LIGHT_SWORD_RECOIL_CUTOFF: u32 = param(33) as u32;

// FZ w12's recoil exclusion list is at directory offset 56.
pub fn ignores_deflection(state: u8) -> bool {
    matches!(
        source_state(state),
        23 | 57 | 60 | 63 | 70..=72 | 74 | 75 | 77 | 85 | 86
    )
}

// The low two bits of the peer phial snapshot carry this tier.
#[derive(Clone, Copy)]
pub enum DischargeTier {
    Base = 0,
    Middle = 1,
    High = 2,
}

impl DischargeTier {
    pub fn from_packet(bits: u8) -> Self {
        match bits & 3 {
            1 => Self::Middle,
            2 => Self::High,
            _ => Self::Base,
        }
    }

    pub fn min_hits(self) -> u16 {
        match self {
            Self::Base => 0,
            Self::Middle => param(41),
            Self::High => param(40),
        }
    }

    pub fn release_state(self) -> u8 {
        match self {
            Self::Base => 75,
            Self::Middle => 85,
            Self::High => 86,
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct Gauges {
    pub light: u16,
    pub unlocked: bool,
    pub sword: bool,
    pub discharge_hits: u16,
}
impl Gauges {
    pub fn energy(self, slash: u16) -> u16 {
        if self.sword { self.light } else { slash }
    }
    pub fn full(self) -> bool {
        self.light >= param(18)
    }
    pub fn stance_action(self, drawn: bool) -> Option<u8> {
        match (self.unlocked, self.sword, drawn) {
            (true, true, _) => Some(SWORD_TO_AXE),
            (true, false, true) => Some(30),
            (true, false, false) => Some(4),
            (false, _, _) if self.full() => Some(if drawn { 94 } else { 93 }),
            _ => None,
        }
    }
    pub fn gain_light(&mut self, light: u16) {
        self.light = self.light.saturating_add(light).min(param(18));
    }
    pub fn eligible(self, condition: u16, energy: u16, stamina: bool, steps: u8) -> bool {
        match condition {
            0 | 10 | 11 | 29 => true,
            // 823A0F00 requires energy below the style's threshold.
            // For Extreme, 8121477A returns zero and both gauges clamp at zero.
            19 => false,
            1 => stamina,
            2 => !stamina,
            3 | 24 => energy > 0,
            5 => energy == 0,
            6 | 13 | 14 | 30 => !self.sword,
            7 | 16 | 17 | 31 => self.sword && energy > 0,
            8 => u16::from(steps) < param(34),
            26 | 44 => self.unlocked,
            28 => !self.unlocked && self.full(),
            _ => false,
        }
    }
    pub fn discharge_tier(self) -> DischargeTier {
        if self.discharge_hits >= DischargeTier::High.min_hits() {
            DischargeTier::High
        } else if self.discharge_hits >= DischargeTier::Middle.min_hits() {
            DischargeTier::Middle
        } else {
            DischargeTier::Base
        }
    }
}

// FZ 812132C4 advances 30 Hz charge ticks before it evaluates branches.
// Only level two (between the ready and overcharge thresholds) gets the strong release.
pub fn charge_release(branch: u16, sword: bool, ready: bool) -> u8 {
    if !ready {
        return if sword { 165 } else { 121 };
    }
    match (sword, branch) {
        (false, 211) => 116,
        (false, 212) => 122,
        (false, _) => 125,
        (true, 211) => 155,
        (true, 212) => 157,
        (true, _) => 166,
    }
}

// The w12 exclusion table is read by 81213F18. Branch 210 turns in 81214070.
pub fn accepts_side_turn(branch: u16) -> bool {
    branch == 210
        || (branch < COUNT as u16
            && DATA[MODELS - 48 + usize::from(branch / 8)] & (1 << (branch % 8)) == 0)
}

// 81213F18 reads 818173F2 (previous action), so a charge release still
// sees the dodge slash that started the charge.
pub fn turn_delta(branch: u16, previous_action: Option<u8>, yaw: i16) -> i16 {
    if !accepts_side_turn(branch) {
        return 0;
    }
    let magnitude = if !matches!(previous_action, Some(106 | 152))
        && matches!(branch, 130 | 136 | 140 | 141 | 158..=161)
    {
        0x2000
    } else {
        0x1000
    };
    match yaw {
        0x2000..=0x6000 => -magnitude,
        -0x6000..=-0x2000 => magnitude,
        _ => 0,
    }
}

// FZ 812102C2 selects these actions from the w12 parameter directory.
pub fn uses_slash_gauge(state: u8) -> bool {
    let state = source_state(state);
    DATA[MODELS - 80 + usize::from(state / 8)] & (1 << (state % 8)) != 0
}

pub fn counts_for_discharge(row: u16) -> bool {
    row < 128 && DATA[MODELS - 16 + usize::from(row / 8)] & (1 << (row % 8)) != 0
}
