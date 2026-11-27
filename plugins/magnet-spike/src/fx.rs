use crate::program::Mode;
use alloc::vec::Vec;
use mhfz_sdk::effects::MotionClock;
use mhgu_sdk::{
    combat::collision::hurt::Monster,
    geometry::{Angle, Bearing},
    sync::Shared,
    weapon::style::{
        action::{self, MotionFrame},
        hunter,
        particle::{self, Anchor, Bank},
    },
};

static BANK: Bank = Bank::new(
    c"effect\\frontier_ms\\particles",
    include_bytes!("../target/assets/frontier_ms_routes.bin"),
);
const MOTION_ROWS: &[[u8; 18]] = include_bytes!("../target/assets/frontier_ms_motion_fx.bin")
    .as_chunks::<18>()
    .0;

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
    EvadeFlash = 17,
    ShockOpening = 18,
    ShockFirst = 19,
    ShockMiddle = 20,
    ShockFar = 21,
    ShockLocal = 22,
    ImpactOpening = 23,
    ImpactMiddle = 24,
    ImpactFar = 25,
    ImpactLocal = 26,
    BindReady = 27,
    BindAura = 40,
    BindPulse = 41,
    BindRelease = 42,
    BindCasterConfirm = 43,
    BindCasterRelease = 44,
    BindCasterPulse = 45,
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

pub(super) unsafe fn shock(route: Route, point: [f32; 3], yaw: u16) {
    unsafe { BANK.emit(route as usize, Anchor::World(point, yaw)) };
}

pub(super) fn bind_ready(position: [f32; 3]) {
    // 8102CDDC request 7 samples the monster joint and emits 4037:54.
    unsafe { BANK.emit(Route::BindReady as usize, Anchor::World(position, 0)) };
}

pub(super) struct BoundEffects(EffectUnits);

impl BoundEffects {
    pub(super) fn new() -> Self {
        Self(EffectUnits::new())
    }

    pub(super) fn aura(&self, [x, y, z]: [f32; 3]) {
        unsafe {
            BANK.emit_tracked(
                Route::BindAura as usize,
                Anchor::World([x, y + 200., z], 0),
                |unit| self.0.track(unit),
            )
        };
    }

    pub(super) fn pulse(&self, monster: &Monster<'_>, joint: u16) {
        monster.emit_joint_particle(&BANK, Route::BindPulse as usize, joint, |unit| {
            self.0.track(unit)
        });
    }

    pub(super) fn release(&self, monster: &Monster<'_>, joint: u16) {
        self.0.stop();
        monster.emit_joint_particle(&BANK, Route::BindRelease as usize, joint, |_| {});
    }
}

pub(super) fn contest_goal(actor: u32, monster: &Monster<'_>, joint: u16) {
    // 811437EC and 81143800: once when the struggle reaches parameter 41.
    for selector in [74, 73] {
        if let Some(route) = motion_route(4037, selector) {
            monster.emit_joint_particle(&BANK, route, joint, |_| {});
        }
    }
    unsafe { crate::sound::bind_caster(actor, 24) };
}

pub(super) struct BindCaster {
    pulses: Vec<Effect>,
    engaged: bool,
}

impl BindCaster {
    pub(super) unsafe fn confirm(actor: u32) -> Self {
        unsafe {
            Self::flash(actor, Route::BindCasterConfirm);
            crate::sound::bind_caster(actor, 18);
        }
        Self {
            pulses: Vec::new(),
            engaged: false,
        }
    }

    unsafe fn flash(actor: u32, route: Route) {
        // 8116648E samples a world point 100 units ahead of the caster.
        let (point, _) = unsafe { particle::world_offset(actor, [0., 0., 100.]) };
        unsafe { BANK.emit(route as usize, Anchor::World(point, 0)) };
    }

    pub(super) unsafe fn advance(
        &mut self,
        actor: u32,
        previous: f32,
        elapsed: f32,
        engaged: bool,
        target: [f32; 3],
    ) {
        self.engaged |= engaged;
        let crossed = |period| (elapsed / period) as u32 != (previous / period) as u32;
        // 81166A2A and 811672A2 evaluate the hunter-to-monster pose both
        // at emission and on subsequent updates of each 4038:30 pulse.
        let ([x, y, z], _) = unsafe { particle::world_offset(actor, [0., 50., 100.]) };
        let bearing = Bearing::between_xz((x, z), (target[0], target[2]));
        let pitch = libm::roundf(
            -libm::atan2f(target[1] - y, bearing.distance) * (65536. / core::f32::consts::TAU),
        ) as i32 as u16;
        let point = [x, y, z];
        let angles = [pitch, bearing.yaw.units(), 0];
        self.pulses
            .retain(|effect| effect.follow_world_pose(point, angles));
        if crossed(24.) {
            let mut effect = Effect::new(Route::BindCasterPulse);
            unsafe { effect.emit_pending(Anchor::RotatedWorld(point, angles)) };
            self.pulses.push(effect);
        }
        if crossed(104.) {
            unsafe { crate::sound::bind_caster(actor, 19) };
        }
    }

    pub(super) unsafe fn release(&self, actor: u32) {
        if self.engaged {
            unsafe { Self::flash(actor, Route::BindCasterRelease) };
        }
    }
}

const SELECTORS: &[[u8; 6]] = include_bytes!("../target/assets/frontier_ms_selectors.bin")
    .as_chunks::<6>()
    .0;

fn motion_route(bank: u16, selector: u16) -> Option<usize> {
    let index = SELECTORS
        .binary_search_by_key(&(bank, selector), |&[a, b, c, d, _, _]| {
            (u16::from_le_bytes([a, b]), u16::from_le_bytes([c, d]))
        })
        .ok()?;
    Some(usize::from(u16::from_le_bytes([
        SELECTORS[index][4],
        SELECTORS[index][5],
    ])))
}

pub(super) unsafe fn native(bank: u16, selector: u16, anchor: Anchor) {
    if let Some(route) = motion_route(bank, selector) {
        unsafe { BANK.emit(route, anchor) };
    }
}

pub(super) unsafe fn charge(actor: u32, selector: u16) {
    if let Some(route) = motion_route(4038, selector)
        && let Some(parent) = unsafe { particle::parent(actor) }
    {
        unsafe {
            BANK.emit(
                route,
                Anchor::JointPose {
                    parent,
                    joint: 2,
                    offset: None,
                },
            )
        };
    }
}

pub(super) unsafe fn motion(
    clock: &mut MotionClock,
    accent: &mut bool,
    actor: u32,
    clip: u16,
    sample: MotionFrame,
) {
    for event in clock.advance(MOTION_ROWS, clip, sample) {
        // FSKL -> GU logical joints, shared with the hunter motion conversion.
        let joint = match event.node {
            0 => -1,
            2 => 0,
            4 => 15,
            5 => 16,
            7 => 18,
            8 => 19,
            10 => 2,
            14 => 8,
            15 => 9,
            18 => 12,
            _ => continue,
        };
        // 817BD900 calls 812210B2 for every w13 row, independently of
        // the particle emitter. 8121F010 clears bit 0x40000 on action entry.
        if event.bank == 4037
            && matches!(event.selector, 3 | 4 | 70 | 71)
            && matches!(event.node, 14 | 18)
        {
            unsafe { crate::sound::motion_accent(actor, accent, joint as u32, event.offset) };
        }
        // 81F96C06. Trial model 6451 uses normal reach (equipment row 16881,
        // column zero). Type 12 uses the midpoint of row 3 in mhfdat+216;
        // type 14 takes the signed vector at mhfdat+220.
        let offset = match (event.kind, event.bank) {
            (1, 12) => [0., 0., 125.],
            (1, 14) => [44., 0., 123.],
            _ => event.offset,
        };
        let (mut point, yaw) = unsafe { particle::world_offset(actor, offset) };
        let mut angles = [
            0,
            yaw.wrapping_add(if event.mode == 4 { 0x4000 } else { 0 }),
            0,
        ];
        let mut bank = event.bank;
        let mut wet = false;
        let anchor = if event.kind == 7 {
            let Some(parent) = (unsafe { particle::parent(actor) }) else {
                continue;
            };
            let offset = Some(event.offset);
            if matches!(event.mode, 0 | 2) {
                Anchor::JointPose {
                    parent,
                    joint,
                    offset,
                }
            } else {
                Anchor::JointYaw {
                    parent,
                    joint,
                    offset,
                    yaw: angles[1],
                }
            }
        } else {
            if joint >= 0 {
                let Some(matrix) = (unsafe { particle::joint_matrix(actor, joint as u32) }) else {
                    continue;
                };
                let pose = unsafe { (matrix as *const [f32; 16]).read_unaligned() };
                point = particle::transform_offset(&pose, offset);
                if matches!(event.mode, 0 | 2) {
                    angles = mhfz_sdk::effects::world_pose_angles(&pose);
                }
            }
            if matches!(event.kind, 1 | 5) {
                if let Some((floor, water)) = unsafe {
                    particle::ground_heights_with_tolerance(
                        actor,
                        point,
                        if event.kind == 1 { 18. } else { 1. },
                    )
                } {
                    wet = water.is_some();
                    point[1] = water.unwrap_or(floor);
                    if wet && event.kind == 1 {
                        point[1] += 5.;
                        unsafe { crate::sound::splash(actor, event.bank, point) };
                    }
                    bank += u16::from(wet);
                } else if event.kind == 1 {
                    continue;
                }
            }
            if event.kind == 1 {
                // 817BCEF4 passes the selector as a percentage to 81F977FC.
                Anchor::ScaledWorld {
                    position: point,
                    angles,
                    scale: f32::from(event.selector) / 100.,
                }
            } else {
                Anchor::RotatedWorld(point, angles)
            }
        };
        let route = if event.kind == 1 {
            if wet {
                46
            } else if event.bank == 14 {
                // The last compiled surface route emits three dust particles.
                include_bytes!("../target/assets/frontier_ms_routes.bin").len() / 4 - 1
            } else if matches!(event.bank, 7 | 12) {
                48
            } else {
                47
            }
        } else {
            let Some(route) = motion_route(bank, event.selector) else {
                continue;
            };
            route
        };
        unsafe { BANK.emit(route, anchor) };
    }
}

pub(super) struct EvadeFlash {
    area: u8,
    effect: Effect,
}

impl EvadeFlash {
    pub(super) unsafe fn emit(actor: u32, area: u8) -> Option<Self> {
        let (position, angles) = unsafe { Self::pose(actor) }?;
        let mut effect = Effect::new(Route::EvadeFlash);
        unsafe { effect.emit_pending(Anchor::RotatedWorld(position, angles)) };
        Some(Self { area, effect })
    }

    pub(super) unsafe fn advance(&self, actor: u32, area: u8) -> bool {
        area == self.area
            && unsafe { Self::pose(actor) }
                .is_some_and(|(position, angles)| self.effect.follow_world_pose(position, angles))
    }

    unsafe fn pose(actor: u32) -> Option<([f32; 3], [u16; 3])> {
        // FZ 8116783E / 811671D4: bank 4038 selector 34 follows weapon node 4.
        let matrix = unsafe { particle::weapon_joint_pose(actor, 4) }?;
        let [pitch, yaw, _] = mhfz_sdk::effects::world_pose_angles(&matrix);
        Some((
            particle::transform_offset(&matrix, [0., 0., 55.]),
            [pitch, yaw, 0],
        ))
    }
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

pub(super) unsafe fn root_offset(actor: u32, offset: [f32; 3]) -> ([f32; 3], u16) {
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
    units: EffectUnits,
    route: usize,
    pending: u32,
}

impl Effect {
    pub(super) fn new(route: Route) -> Self {
        Self::from_route(route as usize)
    }

    fn from_route(route: usize) -> Self {
        Self {
            units: EffectUnits::new(),
            route,
            pending: u32::MAX,
        }
    }

    pub(super) unsafe fn aimed(
        selector: u16,
        point: [f32; 3],
        angles: [u16; 3],
        scale: f32,
    ) -> Option<Self> {
        let mut effect = Self::from_route(motion_route(4037, selector)?);
        unsafe {
            effect.emit_pending(Anchor::ScaledWorld {
                position: point,
                angles,
                scale,
            })
        };
        Some(effect)
    }

    pub(super) fn move_to(&mut self, position: [f32; 3]) {
        UNITS.with(|units| {
            for &(id, unit) in &units.live {
                if id == self.units.0 {
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
                BANK.emit_layers(self.route, anchor, self.pending, |_, unit| {
                    self.units.track(unit);
                })
            };
        }
    }

    pub(super) fn follow_world_pose(&self, position: [f32; 3], angles: [u16; 3]) -> bool {
        UNITS.with(|units| {
            let mut live = false;
            for &(id, unit) in &units.live {
                if id == self.units.0 {
                    unsafe { particle::pose_world_unit(unit, position, angles) };
                    live = true;
                }
            }
            live
        })
    }
}

struct EffectUnits(u64);

impl EffectUnits {
    fn new() -> Self {
        Self(UNITS.with(|units| {
            units.sequence += 1;
            units.sequence
        }))
    }

    fn track(&self, unit: u32) {
        UNITS.with(|units| units.live.push((self.0, unit)));
    }

    fn stop(&self) {
        loop {
            let unit = UNITS.with(|units| {
                let index = units.live.iter().position(|&(id, _)| id == self.0)?;
                Some(units.live.swap_remove(index).1)
            });
            let Some(unit) = unit else { break };
            // GU can release a unit synchronously. Drop the lock before stopping it.
            unsafe { particle::stop_unit(unit) };
        }
    }
}

impl Drop for EffectUnits {
    fn drop(&mut self) {
        self.stop();
    }
}
