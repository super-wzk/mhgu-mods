use crate::program::Mode;
use alloc::vec::Vec;
use mhgu_sdk::{
    geometry::{Angle, Bearing},
    sync::Shared,
    weapon::style::{
        action, hunter,
        particle::{self, Anchor, Bank},
    },
};

static BANK: Bank = Bank::new(
    c"effect\\frontier_ms\\particles",
    include_bytes!("../target/assets/frontier_ms_routes.bin"),
);

#[derive(Clone, Copy)]
pub(super) enum Route {
    Shot = 0,
    Mark = 1,
    SlashFront = 2,
    SlashBack = 3,
    Impact = 4,
    GuardDust = 5,
    GuardSplash = 6,
    SlashSparks = 7,
    ImpactSparks = 8,
    RepelWeapon = 9,
    RepelFlash = 10,
    RepelPulse = 11,
    BoostStart = 12,
    SlashBoostPulse = 13,
    ImpactBoostPulse = 14,
    SlashBoostSparks = 15,
    ImpactBoostSparks = 16,
}

struct Units {
    sequence: u64,
    live: Vec<(u64, u32)>,
}

static UNITS: Shared<Units> = Shared::new(Units {
    sequence: 0,
    live: Vec::new(),
});

extern "C" fn released(unit: u32) {
    UNITS.with(|units| units.live.retain(|&(_, address)| address != unit));
}

pub(super) fn bind(style: u32) -> bool {
    particle::on_release(style, released)
}

pub(super) unsafe fn preload() {
    unsafe { BANK.preload() };
}

pub(super) unsafe fn guard(actor: u32, mode: Mode) {
    // FZ joint 14 is GU logical joint 8. The EFL stores the sparks' delays
    // and offsets, letting the native emitter follow the weapon as it moves.
    let parent = unsafe { particle::parent(actor) };
    let attached = |route: Route, offset| {
        if let Some(parent) = parent {
            unsafe {
                BANK.emit(
                    route as usize,
                    Anchor::JointPose {
                        parent,
                        joint: 8,
                        offset,
                    },
                );
            }
        }
    };
    if matches!(mode, Mode::Slash) {
        // 8102CDDC effect A, at the first accepted block.
        attached(Route::SlashFront, Some([30., 0., -70.]));
        attached(Route::SlashBack, Some([40., 0., 100.]));
        attached(Route::SlashSparks, None);
        return;
    }

    // 812208B8 samples FZ joint 0, using its pitch and yaw with zero roll.
    // The root is also available before the skeleton's first pose has loaded.
    let root =
        unsafe { particle::joint_matrix(actor, 0).map(|matrix| &*(matrix as *const [f32; 16])) };
    let (burst, ground, angles) = if let Some(root) = root {
        let [pitch, yaw, _] = mhfz_sdk::effects::world_pose_angles(root);
        (
            particle::transform_offset(root, [0., 120., 100.]),
            particle::transform_offset(root, [0., 0., 100.]),
            [pitch, yaw, 0],
        )
    } else {
        let (burst, yaw) = unsafe { particle::world_offset(actor, [0., 120., 100.]) };
        let (ground, _) = unsafe { particle::world_offset(actor, [0., 0., 100.]) };
        (burst, ground, [0, yaw, 0])
    };
    let wet = unsafe { particle::ground_heights(actor, ground) }
        .is_some_and(|(_, water)| water.is_some());
    let surface = if wet {
        Route::GuardSplash
    } else {
        Route::GuardDust
    };
    unsafe {
        BANK.emit(Route::Impact as usize, Anchor::RotatedWorld(burst, angles));
        BANK.emit(surface as usize, Anchor::RotatedWorld(ground, angles));
    }
    attached(Route::ImpactSparks, None);
}

pub(super) struct RepelEffect {
    area: u8,
    mode: Mode,
    elapsed: f32,
}

impl RepelEffect {
    pub(super) fn new(area: u8, mode: Mode) -> Self {
        Self {
            area,
            mode,
            elapsed: 0.,
        }
    }

    pub(super) unsafe fn advance(
        &mut self,
        actor: u32,
        area: u8,
        frames: f32,
        mark: Option<[f32; 3]>,
    ) -> bool {
        if self.area != area {
            return false;
        }
        // 8102CDDC requests 11/12 evaluate the mark at tick 2 and the
        // hunter root at tick 3, independently of motion playback.
        let previous = self.elapsed;
        self.elapsed += frames;
        if previous < 4.
            && self.elapsed >= 4.
            && let Some(mark) = mark
        {
            unsafe { self.emit_at_mark(actor, mark) };
        }
        if self.elapsed < 6. {
            return true;
        }
        let (point, yaw) = unsafe { root_offset(actor, [0., 100., -100.]) };
        unsafe { BANK.emit(Route::RepelFlash as usize, Anchor::World(point, yaw)) };
        false
    }

    unsafe fn emit_at_mark(&self, actor: u32, mark: [f32; 3]) {
        let (root, _) = unsafe { root_offset(actor, [0.; 3]) };
        let elevated = [root[0], root[1] + 150., root[2]];
        let bearing = Bearing::between_xz((elevated[0], elevated[2]), (mark[0], mark[2]));
        let rise = mark[1] - elevated[1];
        let distance = libm::hypotf(bearing.distance, rise);
        let (offset, scale) = if distance <= 500. {
            (100., 0.8)
        } else if distance <= 1000. {
            (200., 1.)
        } else {
            (300., 1.3)
        };
        let pitch =
            libm::roundf(-libm::atan2f(rise, bearing.distance) * (65536. / core::f32::consts::TAU))
                as i32;
        let pitch = Angle::from_units(pitch.clamp(-5461, 5461) as u16);
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        let (sin_yaw, cos_yaw) = bearing.yaw.sin_cos();
        let position = [
            mark[0] + sin_yaw * cos_pitch * offset,
            mark[1] - sin_pitch * offset,
            mark[2] + cos_yaw * cos_pitch * offset,
        ];
        unsafe {
            BANK.emit(
                Route::RepelPulse as usize,
                Anchor::ScaledWorld {
                    position,
                    angles: [pitch.units(), bearing.yaw.units(), 0],
                    scale,
                },
            );
            if let Some(parent) = particle::parent(actor) {
                let offset = match self.mode {
                    Mode::Slash => [60., 0., 60.],
                    Mode::Impact => [0., 0., 170.],
                };
                BANK.emit(
                    Route::RepelWeapon as usize,
                    Anchor::JointPose {
                        parent,
                        joint: 8,
                        offset: Some(offset),
                    },
                );
            }
            BANK.emit(Route::RepelFlash as usize, Anchor::World(mark, 0));
        }
    }
}

unsafe fn root_offset(actor: u32, offset: [f32; 3]) -> ([f32; 3], u16) {
    let (position, yaw) = unsafe { particle::world_offset(actor, offset) };
    let point = unsafe { particle::joint_position(actor, 0) }.map_or(position, |root| {
        let actor_position = unsafe { particle::world_offset(actor, [0.; 3]) }.0;
        core::array::from_fn(|i| position[i] + root[i] - actor_position[i])
    });
    (point, yaw)
}

#[derive(Clone, Copy)]
struct BoostClock {
    elapsed: f32,
    next_spark: usize,
}

impl BoostClock {
    const NEW: Self = Self {
        elapsed: 0.,
        next_spark: 0,
    };
}

struct BoostPulse {
    offset: [f32; 3],
    effect: Effect,
}

pub(super) struct BoostEffects {
    area: Option<u8>,
    clocks: [BoostClock; 2],
    pulses: Vec<BoostPulse>,
}

impl BoostEffects {
    pub(super) const fn new() -> Self {
        Self {
            area: None,
            clocks: [BoostClock::NEW; 2],
            pulses: Vec::new(),
        }
    }

    pub(super) unsafe fn advance(
        &mut self,
        actor: u32,
        observer_area: u8,
        frames: f32,
        mode: Mode,
        active: u8,
        local: bool,
    ) {
        let area = (unsafe { hunter::area(actor) } == observer_area).then_some(observer_area);
        if self.area != area {
            self.pulses.clear();
            self.area = area;
        }
        if active == 0 && self.pulses.is_empty() {
            self.clocks = [BoostClock::NEW; 2];
            return;
        }
        // FZ 8121D758 resolves node 4 on the *weapon*, including its
        // articulation in each form. 81166E02/811670D6 discard roll.
        let matrix = area.and_then(|_| unsafe { particle::weapon_joint_pose(actor, 4) });
        let angles = matrix.as_ref().map_or([0; 3], |matrix| {
            let [pitch, yaw, _] = mhfz_sdk::effects::world_pose_angles(matrix);
            [pitch, yaw, 0]
        });
        self.pulses.retain_mut(|pulse| {
            let Some(matrix) = &matrix else { return false };
            pulse
                .effect
                .follow_world_pose(particle::transform_offset(matrix, pulse.offset), angles)
        });
        if frames == 0. {
            return;
        }
        let drawn = area.is_some() && unsafe { action::weapon_drawn(actor) };
        for form in [Mode::Slash, Mode::Impact] {
            let clock = &mut self.clocks[form as usize];
            if active & (1 << form as u8) == 0 {
                *clock = BoostClock::NEW;
                continue;
            }
            let (warmup, flash_offset, pulse, sparks, offsets) = match form {
                Mode::Slash => (
                    18,
                    [-80., 0., 40.],
                    Route::SlashBoostPulse,
                    Route::SlashBoostSparks,
                    [
                        [45., 0., 20.],
                        [-45., 0., 20.],
                        [0.; 3],
                        [-90., 0., 30.],
                        [90., 0., 30.],
                    ],
                ),
                Mode::Impact => (
                    17,
                    [20., 0., 80.],
                    Route::ImpactBoostPulse,
                    Route::ImpactBoostSparks,
                    [
                        [0., 0., 140.],
                        [0., 0., 30.],
                        [0., 0., 70.],
                        [0., 0., -10.],
                        [0., 0., 100.],
                    ],
                ),
            };
            let before = clock.elapsed as u32 / 2;
            let visible = drawn && form == mode;
            if visible {
                clock.elapsed += frames;
            } else if before < warmup {
                // The startup delay runs even with the weapon put away.
                clock.elapsed = (clock.elapsed + frames).min((2 * warmup) as f32);
            }
            let after = clock.elapsed as u32 / 2;
            let Some(matrix) = &matrix else { continue };
            let mut emit = |route, offset| {
                let mut effect = Effect::new(route);
                let position = particle::transform_offset(matrix, offset);
                unsafe { effect.emit_pending(Anchor::RotatedWorld(position, angles)) };
                self.pulses.push(BoostPulse { offset, effect });
            };
            if local && before < warmup && after >= warmup {
                emit(Route::BoostStart, flash_offset);
            }
            let start = before.max(warmup);
            let interval = if local { 10 } else { 25 };
            let passed = after
                .div_ceil(interval)
                .saturating_sub(start.div_ceil(interval));
            if passed != 0 {
                // Use the most recent spark when a scene update skips several ticks.
                let last = (clock.next_spark + (passed - 1) as usize) % offsets.len();
                clock.next_spark = (last + 1) % offsets.len();
                emit(sparks, offsets[last]);
            }
            if local && after.div_ceil(20) > start.div_ceil(20) {
                emit(pulse, flash_offset);
            }
        }
    }
}

impl Default for BoostEffects {
    fn default() -> Self {
        Self::new()
    }
}

/// Owns the layers submitted for one scene object. All calls and drops occur
/// in the hunter controller, quest preload or world-contact callback.
pub(super) struct Effect {
    id: u64,
    route: Route,
    pending: u32,
}

impl Effect {
    pub(super) fn new(route: Route) -> Self {
        let id = UNITS.with(|units| {
            units.sequence += 1;
            units.sequence
        });
        Self {
            id,
            route,
            pending: u32::MAX,
        }
    }

    pub(super) fn move_to(&mut self, position: [f32; 3]) {
        UNITS.with(|units| {
            for &(id, unit) in &units.live {
                if id == self.id {
                    // The setter does not release units or acquire SDK locks.
                    unsafe { particle::move_world_unit(unit, position) };
                }
            }
        });
        unsafe { self.emit_pending(Anchor::World(position, 0)) };
    }

    unsafe fn emit_pending(&mut self, anchor: Anchor) {
        if self.pending != 0 {
            self.pending = unsafe {
                BANK.emit_layers(self.route as usize, anchor, self.pending, |_, unit| {
                    UNITS.with(|units| units.live.push((self.id, unit)));
                })
            };
        }
    }

    fn follow_world_pose(&self, position: [f32; 3], angles: [u16; 3]) -> bool {
        UNITS.with(|units| {
            let mut live = false;
            for &(id, unit) in &units.live {
                if id == self.id {
                    unsafe { particle::pose_world_unit(unit, position, angles) };
                    live = true;
                }
            }
            live
        })
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        loop {
            let unit = UNITS.with(|units| {
                let index = units.live.iter().position(|&(id, _)| id == self.id)?;
                Some(units.live.swap_remove(index).1)
            });
            let Some(unit) = unit else { break };
            // GU can release a unit synchronously. Drop the lock before stopping it.
            unsafe { particle::stop_unit(unit) };
        }
    }
}
