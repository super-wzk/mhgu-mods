//! Magnetic shot and attached mark, updated on the GU scene clock.
use crate::{
    fx::{BoostEffects, Effect, RepelEffect, Route},
    motion,
    program::{self, Mode},
};
use alloc::vec::Vec;
use mhgu_sdk::{
    combat::collision::hurt::{HurtScene, Target},
    geometry::{Angle, Bearing},
    sync::Shared,
    weapon::style::{
        action::{self, melee::StrikePower},
        hud::{GaugePlate, MAX_METER_SEGMENTS, Meter, MeterOverlay, MeterTrack, Signal},
        hunter, movement, particle, remote,
    },
};

pub(super) mod network;

struct Projectile {
    serial: u8,
    area: u8,
    position: [f32; 3],
    velocity: [f32; 3],
    remaining: f32,
    effect: Effect,
}

impl Projectile {
    fn new(serial: u8, area: u8, position: [f32; 3], direction: [f32; 3]) -> Self {
        let speed = f32::from(program::parameter(16)) / 2.;
        let mut effect = Effect::new(Route::Shot);
        effect.move_to(position);
        Self {
            serial,
            area,
            position,
            velocity: direction.map(|component| component * speed),
            remaining: program::SHOT.0,
            effect,
        }
    }

    fn end_at(&self, frames: f32) -> [f32; 3] {
        core::array::from_fn(|i| self.position[i] + frames * self.velocity[i])
    }

    fn advance(&mut self, position: [f32; 3], frames: f32) -> bool {
        self.remaining -= frames;
        self.position = position;
        if self.remaining <= 0. {
            return false;
        }
        self.effect.move_to(position);
        true
    }
}

struct Mark {
    target: Target,
    remaining: f32,
    effect: Effect,
}

const MARK_FRAMES: f32 = 2. * program::parameter(14) as f32;

fn mark_clock_running(actor: u32) -> bool {
    let (group, slot) = unsafe { action::current(actor) };
    // FZ 8122880E / 8121FA3E pause the countdown during these actions.
    !action::custom_id(group, slot)
        .is_some_and(|id| program::is_attraction(id) || matches!(id, 63 | 69 | 73))
}

struct Gauge {
    units: u16,
    tick_fraction: f32,
    refill_ticks: u32,
    spend_flash: u8,
    refill_flashes: [u8; MAX_METER_SEGMENTS],
}

const SEGMENT_UNITS: u16 = program::parameter(19);
const SEGMENTS: u8 = (program::parameter(17) / SEGMENT_UNITS) as u8;
// FZ includes tick zero in the draw window. Zero here means inactive.
const SPEND_FRAMES: u8 = 31;
const REFILL_FRAMES: u8 = 21;
static FLASH_ATLAS: GaugePlate =
    GaugePlate::new(0x465a4d53, c"GUI\\frontier_magnet_spike\\flash", [0.; 4])
        .with_cells(2, [36, 32]);

pub(super) unsafe fn preload() {
    unsafe { FLASH_ATLAS.preload() };
}

impl Gauge {
    const fn new() -> Self {
        // FZ 81228740 initializes the hunt's charge and clears the regen timer.
        Self {
            units: program::parameter(18),
            tick_fraction: 0.,
            refill_ticks: 0,
            spend_flash: 0,
            refill_flashes: [0; MAX_METER_SEGMENTS],
        }
    }

    fn add(&mut self, units: u32) {
        let previous = self.units / SEGMENT_UNITS;
        self.units = u32::from(self.units)
            .saturating_add(units)
            .min(u32::from(program::parameter(17))) as u16;
        let filled = self.units / SEGMENT_UNITS;
        // FZ 812281D4 starts each newly filled cell once while its flash is active.
        for flash in &mut self.refill_flashes[usize::from(previous)..usize::from(filled)] {
            if *flash == 0 {
                *flash = REFILL_FRAMES;
            }
        }
        if self.units == program::parameter(17) {
            self.refill_ticks = 0;
        }
    }

    fn advance_flashes(&mut self, ticks: u32) {
        let ticks = ticks.min(u32::from(SPEND_FRAMES)) as u8;
        self.spend_flash = self.spend_flash.saturating_sub(ticks);
        for flash in &mut self.refill_flashes {
            *flash = flash.saturating_sub(ticks);
        }
    }

    fn advance(&mut self, frames: f32) {
        let elapsed = self.tick_fraction + frames / 2.;
        let mut ticks = elapsed as u32;
        self.tick_fraction = elapsed % 1.;

        // FZ 8122880E grants charge before decrementing flashes on that tick.
        // Process due grants in order so old flashes expire even on a long step.
        let interval = u32::from(program::parameter(21));
        while self.units < program::parameter(17) {
            let until_refill = interval - self.refill_ticks;
            if ticks < until_refill {
                self.refill_ticks += ticks;
                break;
            }
            self.advance_flashes(until_refill - 1);
            self.add(u32::from(program::parameter(20)));
            self.advance_flashes(1);
            self.refill_ticks = 0;
            ticks -= until_refill;
        }
        self.advance_flashes(ticks);
    }

    fn overlay(&self) -> MeterOverlay {
        MeterOverlay {
            atlas: Some(&FLASH_ATLAS),
            spend: if self.spend_flash > 0 {
                1 + SPEND_FRAMES - self.spend_flash
            } else {
                0
            },
            refill: self.refill_flashes.map(|left| {
                if left > 0 {
                    1 + SPEND_FRAMES + REFILL_FRAMES - left
                } else {
                    0
                }
            }),
        }
    }
}

const BOOST_CAPACITY: u16 = program::parameter(56);
const BOOST_DURATION: f32 = program::parameter(60) as f32 * 2.;
const BOOST_COOLDOWN: f32 = program::parameter(59) as f32 * 2.;

#[derive(Clone, Copy)]
enum Boost {
    Charging(u16),
    Active(f32),
    Cooling(f32),
}

impl Boost {
    fn hit(&mut self, power: u16) {
        if let Self::Charging(units) = self {
            // FZ 812283D8 adds 80% of the effective motion power on contact.
            let gain = u32::from(power) * u32::from(program::parameter(58)) / 100;
            *units = (u32::from(*units) + gain).min(BOOST_CAPACITY.into()) as u16;
        }
    }

    fn ready(self) -> bool {
        matches!(self, Self::Charging(BOOST_CAPACITY))
    }

    fn advance(&mut self, frames: f32) {
        // FZ 8122880E advances both forms, including while the other is in use.
        *self = match *self {
            Self::Active(left) if left > frames => Self::Active(left - frames),
            Self::Active(left) if left + BOOST_COOLDOWN > frames => {
                Self::Cooling(left + BOOST_COOLDOWN - frames)
            }
            Self::Cooling(left) if left > frames => Self::Cooling(left - frames),
            Self::Active(_) | Self::Cooling(_) => Self::Charging(0),
            charging => charging,
        };
    }

    fn track(self, color: u32) -> MeterTrack {
        let (fraction, color) = match self {
            Self::Charging(units) => (f32::from(units) / f32::from(BOOST_CAPACITY), color),
            Self::Active(left) => (left / BOOST_DURATION, 0xffffc76b),
            Self::Cooling(left) => (left / BOOST_COOLDOWN, 0xff8d8d8d),
        };
        MeterTrack {
            percent: (100. * fraction) as u8,
            color,
        }
    }
}

struct Magnet {
    owner: u32,
    shots: Vec<Projectile>,
    mark: Option<Mark>,
    repel: Option<RepelEffect>,
    gauge: Gauge,
    range: Option<MarkRange>,
    boosts: [Boost; 2],
    boost_effects: BoostEffects,
}

impl Magnet {
    const fn new() -> Self {
        Self {
            owner: 0,
            shots: Vec::new(),
            mark: None,
            repel: None,
            gauge: Gauge::new(),
            range: None,
            boosts: [Boost::Charging(0); 2],
            boost_effects: BoostEffects::new(),
        }
    }

    fn select_owner(&mut self, owner: u32) {
        if self.owner != owner || owner == 0 {
            self.shots.clear();
            self.mark = None;
            self.repel = None;
            self.gauge = Gauge::new();
            self.range = None;
            self.boosts = [Boost::Charging(0); 2];
            self.boost_effects = BoostEffects::new();
            self.owner = owner;
        }
    }

    fn active_boosts(&self) -> u8 {
        let [slash, impact] = self
            .boosts
            .map(|boost| u8::from(matches!(boost, Boost::Active(_))));
        slash | impact << 1
    }
}

static LOCAL: Shared<Magnet> = Shared::new(Magnet::new());

pub fn reset() {
    LOCAL.with(|state| state.select_owner(0));
}

pub(super) unsafe fn begin_repel(actor: u32, mode: Mode) {
    let area = unsafe { hunter::area(actor) };
    LOCAL.with(|state| {
        state.select_owner(actor);
        state.repel = Some(RepelEffect::new(area, mode));
    });
}

pub fn boost_ready(actor: u32, mode: Mode) -> bool {
    LOCAL.with(|state| state.owner == actor && state.boosts[mode as usize].ready())
}

pub fn activate_boost(actor: u32, mode: Mode) -> bool {
    LOCAL.with(|state| {
        if state.owner != actor || !state.boosts[mode as usize].ready() {
            return false;
        }
        state.boosts[mode as usize] = Boost::Active(BOOST_DURATION);
        true
    })
}

pub fn boosts(actor: u32) -> u8 {
    LOCAL.with(|state| {
        if state.owner != actor {
            return 0;
        }
        state.active_boosts()
    })
}

pub extern "C" fn meter(actor: u32) -> Meter {
    LOCAL.with(|state| {
        let current = state.owner == actor;
        let [slash, impact] = if current {
            state.boosts
        } else {
            [Boost::Charging(0); 2]
        };
        Meter {
            charge: if current {
                state.gauge.units
            } else {
                program::parameter(18)
            },
            units_per_segment: SEGMENT_UNITS,
            segments: SEGMENTS,
            signal: match state.range.filter(|_| current) {
                Some(MarkRange::Repel) => Signal::Near,
                Some(MarkRange::Attract) => Signal::Far,
                None => Signal::None,
            },
            tracks: [slash.track(0xfff26752), impact.track(0xff71b6ef)],
            overlay: if current {
                state.gauge.overlay()
            } else {
                MeterOverlay::default()
            },
        }
    })
}

const REPEL_LIMIT: f32 = 1400.;
pub(super) const ATTRACT_LIMIT: f32 = 4000.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkRange {
    Repel,
    Attract,
}

pub(super) struct AimedMark {
    pub target: Target,
    pub yaw: u16,
}

impl MarkRange {
    pub unsafe fn aim(self, actor: u32) -> Option<AimedMark> {
        let target = LOCAL.with(|state| {
            state.select_owner(actor);
            state.mark.as_ref().map(|mark| mark.target)
        })?;
        let origin = unsafe { movement::position(actor) };
        let point = unsafe { target.position_for(actor) }?;
        if Self::between(origin, point) != Some(self) {
            return None;
        }
        let yaw = Bearing::between_xz((origin[0], origin[2]), (point[0], point[2])).yaw;
        Some(AimedMark {
            target,
            yaw: yaw.units(),
        })
    }

    fn between(origin: [f32; 3], point: [f32; 3]) -> Option<Self> {
        let squared = origin
            .into_iter()
            .zip(point)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>();
        // FZ 8122052C and 81220584 test the 3D range from the hunter to the mark.
        match squared {
            d if 0. < d && d < REPEL_LIMIT * REPEL_LIMIT => Some(Self::Repel),
            d if REPEL_LIMIT * REPEL_LIMIT < d && d < ATTRACT_LIMIT * ATTRACT_LIMIT => {
                Some(Self::Attract)
            }
            _ => None,
        }
    }
}

fn cost(segments: u8) -> u16 {
    u16::from(segments).saturating_mul(SEGMENT_UNITS)
}

pub fn can_spend(actor: u32, segments: u8) -> bool {
    LOCAL.with(|state| {
        state.select_owner(actor);
        state.gauge.units >= cost(segments)
    })
}

pub fn spend(actor: u32, segments: u8) -> bool {
    LOCAL.with(|state| {
        state.select_owner(actor);
        let Some(remaining) = state.gauge.units.checked_sub(cost(segments)) else {
            return false;
        };
        state.gauge.units = remaining;
        if segments > 0 {
            // FZ 81227F04 restarts the spend flash after every accepted payment.
            state.gauge.spend_flash = SPEND_FRAMES;
        }
        true
    })
}

pub extern "C" fn melee_contact(actor: u32, power: StrikePower, affinity: f32) {
    // FZ 81EC73F0 truncates motion power after affinity. 812281D4
    // applies the gauge rate and then the charge skill, truncating each step.
    let power = power.adjusted(affinity);
    let mode = motion::local_pose(actor).unwrap_or_default().mode;
    let skill = match unsafe { action::ChargeSkill::equipped(actor) } {
        action::ChargeSkill::Focus => program::parameter(53),
        action::ChargeSkill::Slow => program::parameter(55),
        action::ChargeSkill::Normal => 100,
    };
    let gain = u32::from(power) * u32::from(program::parameter(37)) / 100;
    LOCAL.with(|state| {
        state.select_owner(actor);
        state.gauge.add(gain * u32::from(skill) / 100);
        state.boosts[mode as usize].hit(power);
    });
}

pub unsafe fn aim(actor: u32) -> [f32; 3] {
    unsafe { movement::camera_direction() }.unwrap_or_else(|| {
        let (_, yaw) = unsafe { particle::world_offset(actor, [0.; 3]) };
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        [sin, 0., cos]
    })
}

pub unsafe fn launch(actor: u32, direction: [f32; 3]) {
    // FZ node 18 is GU logical joint 12 in the converted hunter skeleton.
    let Some(position) = (unsafe { particle::joint_position(actor, 12) }) else {
        return;
    };
    let area = unsafe { hunter::area(actor) };
    let serial = network::next_serial();
    LOCAL.with(|state| {
        state.select_owner(actor);
        state
            .shots
            .push(Projectile::new(serial, area, position, direction));
    });
    network::launch(actor, serial, direction);
}

pub extern "C" fn update(scene: &HurtScene<'_>) {
    let mut ended = Vec::new();
    LOCAL.with(|state| {
        state.select_owner(scene.owner());
        let frames = scene.frames();
        for boost in &mut state.boosts {
            boost.advance(frames);
        }
        let mode = motion::local_pose(scene.owner()).unwrap_or_default().mode;
        let active = state.active_boosts();
        unsafe {
            state
                .boost_effects
                .advance(scene.owner(), scene.area(), frames, mode, active, true)
        };
        if frames == 0. {
            return;
        }
        state.gauge.advance(frames);
        let mark = &mut state.mark;
        if let Some(current) = mark {
            if mark_clock_running(scene.owner()) {
                current.remaining -= frames;
            }
            if current.remaining <= 0. {
                *mark = None;
            }
        }
        let (_, radius) = program::SHOT;
        state.shots.retain_mut(|shot| {
            if shot.area != scene.area() {
                ended.push((shot.serial, None));
                return false;
            }
            let step = frames.min(shot.remaining);
            let end = shot.end_at(step);
            // FZ 81165A8E stops at stage geometry or the higher floor/water surface.
            if scene.terrain(shot.position, end).is_some()
                || scene
                    .surface_height(end)
                    .is_some_and(|height| end[1] <= height)
            {
                ended.push((shot.serial, None));
                return false;
            }
            if let Some(target) = scene.sweep(shot.position, end, radius) {
                // FZ 81227846 has one mark per owner. Parameter 14 counts
                // native simulation ticks (30 Hz), while GU time is 60 Hz.
                *mark = Some(Mark {
                    target,
                    remaining: MARK_FRAMES,
                    effect: Effect::new(Route::Mark),
                });
                ended.push((shot.serial, Some(target)));
                return false;
            }
            let active = shot.advance(end, step);
            if !active {
                ended.push((shot.serial, None));
            }
            active
        });
        // Publish only the range category for the HUD. The scene callback
        // owns the live monster and bone handles used to compute it.
        let point = mark.as_mut().and_then(|current| {
            let point = scene.position(&current.target)?;
            current.effect.move_to(point);
            Some(point)
        });
        state.range = if let Some(point) = point {
            MarkRange::between(unsafe { movement::position(scene.owner()) }, point)
        } else {
            *mark = None;
            None
        };
        if let Some(repel) = &mut state.repel
            && !unsafe { repel.advance(scene.owner(), scene.area(), frames, point) }
        {
            state.repel = None;
        }
    });
    for (serial, target) in ended {
        network::stop(scene.owner(), serial);
        if let Some(target) = target {
            remote::send_target_event(crate::STYLE.id, scene.owner(), serial, &target);
        }
    }
}
