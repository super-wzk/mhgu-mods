//! Frontier effects played by GU's native particle manager.
use crate::read;
use mhfz_sdk::effects::{self, MotionClock};
use mhgu_sdk::style::{
    action::MotionFrame,
    particle::{self, Anchor, Bank, TrackedUnits},
};
mod trail;

static PARTICLES: Bank = Bank::new(c"effect\\frontier_db\\particles", ROUTES);
static PEER_BANKS: [Bank; 4] = [
    Bank::new(c"effect\\frontier_db\\peer1", ROUTES),
    Bank::new(c"effect\\frontier_db\\peer2", ROUTES),
    Bank::new(c"effect\\frontier_db\\peer3", ROUTES),
    Bank::new(c"effect\\frontier_db\\peer4", ROUTES),
];
const ROUTES: &[u8] = include_bytes!("../target/assets/effects/routes.bin");
const MOTION_ROWS: &[[u8; 18]] = include_bytes!("../target/assets/effects/motion_fx.bin")
    .as_chunks::<18>()
    .0;
const BLADE_JOINTS: [u8; 2] = [8, 12];
const SHARPEN_SPARK: usize = 1;
const SHARPEN_FLASH: usize = 4;
const SHARPEN_SLASH: usize = 5;
const DEMON_GLOW: usize = 6;
const DEMON_TRAIL: usize = 9;
const DEMON_PHASES: usize = 3;
const DEMON_PERIOD: f32 = 12.;
const SLIDE_SELECTORS: [u16; 5] = [2, 5, 6, 12, 13];
const SLIDE_DRY: usize = 17;
const SLIDE_WET: usize = SLIDE_DRY + SLIDE_SELECTORS.len();
const SWING_ROUTE: usize = SLIDE_WET + SLIDE_SELECTORS.len();
static SWINGS: TrackedUnits<10> = TrackedUnits::new();

unsafe fn stop_swings(slot: usize) {
    unsafe { SWINGS.stop_slots(2 * slot..2 * slot + 2) };
}

extern "C" fn release(unit: u32) {
    SWINGS.forget(unit);
}

pub(super) fn install(style: u32) {
    if !SWINGS.bind(style, release) {
        forge::log::prelude::warn!("Extreme DB: particle release service unavailable");
    }
    trail::install();
}

#[derive(Default)]
pub(super) struct Effects {
    phase: u8,
    until_demon: f32,
    spark: usize,
    history: trail::History,
    motion: MotionClock,
}

impl Effects {
    pub(super) fn set_slot(&mut self, slot: usize) {
        self.history.slot = slot;
    }
    fn bank(&self) -> &'static Bank {
        match self.history.slot.checked_sub(1) {
            Some(index) => &PEER_BANKS[index],
            None => &PARTICLES,
        }
    }

    pub(super) fn seek(&mut self, clip: u16, frame: f32) {
        self.motion.seek(clip, frame);
    }

    pub(super) fn clear_action(&mut self) {
        self.motion = MotionClock::default();
        self.history.clear_action();
    }

    pub(super) unsafe fn motion_events(&mut self, actor: u32, clip: u16, sample: MotionFrame) {
        unsafe {
            for (e, late) in self.motion.advance(MOTION_ROWS, clip, sample) {
                match (e.kind, e.bank) {
                    // The four swing windows each have paired type-2 and type-1 rows.
                    (0, 2) => {
                        let duration = 2. * f32::from(e.selector + 2) - late;
                        if duration > 0. {
                            stop_swings(self.history.slot);
                            self.history.swing(actor, self.phase, duration);
                            let anchor = Anchor::World(read(actor, 64), 0);
                            for hand in 0..2 {
                                SWINGS.emit_layers(
                                    self.bank(),
                                    SWING_ROUTE + hand,
                                    anchor,
                                    1,
                                    2 * self.history.slot + hand,
                                );
                            }
                        }
                    }
                    (3, 4021) => {
                        let joint = match e.node {
                            2 => 0,
                            10 => 2,
                            _ => continue,
                        };
                        if let Some(parent) = particle::parent(actor) {
                            PARTICLES.emit(
                                if e.selector == 1 {
                                    0
                                } else {
                                    13 + usize::from(e.selector)
                                },
                                Anchor::Joint {
                                    parent,
                                    joint,
                                    offset: Some(e.offset),
                                },
                            );
                        }
                    }
                    (5, 4035) => {
                        if let Some(selector) =
                            SLIDE_SELECTORS.iter().position(|s| *s == e.selector)
                            && let Some((anchor, wet)) = effects::motion_ground(actor, &e)
                        {
                            PARTICLES
                                .emit(if wet { SLIDE_WET } else { SLIDE_DRY } + selector, anchor);
                        }
                    }
                    (7, 4021) => {
                        let (position, yaw) = particle::world_offset(actor, e.offset);
                        // 817BCA7C supplies the inverted root basis' Y component;
                        // mode 3 instead supplies the hunter yaw (81FD3B8E).
                        let rotation = if e.mode == 2 {
                            let (sin, _) = mhgu_sdk::geometry::Angle::from_units(yaw).sin_cos();
                            (-sin * (65536. / core::f32::consts::TAU)) as i32 as u16
                        } else {
                            yaw
                        };
                        PARTICLES.emit(0, Anchor::World(position, rotation));
                    }
                    _ => {}
                }
            }
        }
    }

    pub(super) unsafe fn demon(&mut self, actor: u32, phase: u8, dt: f32) {
        unsafe {
            self.history.sample(actor, phase, dt);
        }
        if phase != self.phase {
            self.phase = phase;
            self.until_demon = 0.;
        } else if dt <= 0. {
            return;
        }
        self.until_demon = (self.until_demon - dt).max(0.);
        if phase == 0 || self.until_demon > 0. {
            return;
        }
        unsafe {
            let Some(parent) = particle::parent(actor) else {
                return;
            };
            let phase_index = usize::from(phase - 1);
            for (hand, joint) in BLADE_JOINTS.into_iter().enumerate() {
                PARTICLES.emit(
                    DEMON_GLOW + phase_index,
                    Anchor::Joint {
                        parent,
                        joint: i16::from(joint),
                        offset: None,
                    },
                );
                let route = DEMON_TRAIL + hand * DEMON_PHASES + phase_index;
                self.bank().emit(route, Anchor::World(read(actor, 64), 0));
            }
        }
        // The final two 30 Hz keys overlap the next emission as they fade out.
        self.until_demon = DEMON_PERIOD;
    }

    pub(super) unsafe fn sharpen_slash(&mut self, actor: u32, ticks: u16) {
        unsafe {
            self.history.sharpen(actor, self.phase, ticks);
            self.bank()
                .emit(SHARPEN_SLASH, Anchor::World(read(actor, 64), 0));
        }
    }

    pub(super) unsafe fn sharpen(&mut self, actor: u32, first: bool) {
        unsafe {
            if let Some(matrix) = particle::joint_matrix(actor, u32::from(BLADE_JOINTS[0])) {
                PARTICLES.emit(
                    SHARPEN_SPARK + self.spark,
                    Anchor::World(read(matrix, 48), read(actor, 4076)),
                );
                self.spark = (self.spark + 1) % 3;
            }
            if first && let Some(parent) = particle::parent(actor) {
                PARTICLES.emit(
                    SHARPEN_FLASH,
                    Anchor::Joint {
                        parent,
                        joint: i16::from(BLADE_JOINTS[0]),
                        offset: None,
                    },
                );
            }
        }
    }
}

pub(super) unsafe fn evade(actor: u32) {
    unsafe {
        // FZ 811EE130 translates the scene root by (0, 90, 130).
        let (position, yaw) = particle::world_offset(actor, [0., 90., 130.]);
        PARTICLES.emit(0, Anchor::World(position, yaw));
    }
}
