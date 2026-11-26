//! Magnetic shot and attached mark, updated on the GU scene clock.
use crate::program;
use alloc::vec::Vec;
use mhgu_sdk::{
    combat::collision::hurt::{HurtScene, Target},
    geometry::Angle,
    sync::Shared,
    weapon::style::{hunter, movement, particle},
};

extern crate alloc;

struct Projectile {
    area: u8,
    position: [f32; 3],
    velocity: [f32; 3],
    remaining: f32,
}

struct Mark {
    target: Target,
    remaining: f32,
}

struct Gauge {
    units: u16,
    refill_time: f32,
}

impl Gauge {
    const fn new() -> Self {
        // FZ 81228740 initializes the hunt's charge and clears the regen timer.
        Self {
            units: program::parameter(18),
            refill_time: 0.,
        }
    }

    fn add(&mut self, units: u32) {
        self.units = u32::from(self.units)
            .saturating_add(units)
            .min(u32::from(program::parameter(17))) as u16;
        if self.units == program::parameter(17) {
            self.refill_time = 0.;
        }
    }

    fn refill(&mut self, frames: f32) {
        if self.units == program::parameter(17) {
            return;
        }
        // FZ 8122880E gives parameter 20 after each parameter-21 interval at 30 Hz.
        let interval = f32::from(program::parameter(21)) * 2.;
        let elapsed = self.refill_time + frames;
        let ticks = (elapsed / interval) as u32;
        self.refill_time = elapsed % interval;
        self.add(ticks.saturating_mul(u32::from(program::parameter(20))));
    }
}

struct Magnet {
    owner: u32,
    shots: Vec<Projectile>,
    mark: Option<Mark>,
    gauge: Gauge,
}

impl Magnet {
    const fn new() -> Self {
        Self {
            owner: 0,
            shots: Vec::new(),
            mark: None,
            gauge: Gauge::new(),
        }
    }

    fn select_owner(&mut self, owner: u32) {
        if self.owner != owner || owner == 0 {
            self.shots.clear();
            self.mark = None;
            self.gauge = Gauge::new();
            self.owner = owner;
        }
    }
}

static LOCAL: Shared<Magnet> = Shared::new(Magnet::new());

pub fn reset() {
    LOCAL.with(|state| state.select_owner(0));
}

fn cost(segments: u8) -> u16 {
    u16::from(segments).saturating_mul(program::parameter(19))
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
        true
    })
}

pub extern "C" fn melee_contact(actor: u32, power: u16) {
    LOCAL.with(|state| {
        state.select_owner(actor);
        // FZ 812281D4 converts the contact's motion power to charge.
        state
            .gauge
            .add(u32::from(power) * u32::from(program::parameter(37)) / 100);
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
    let (duration, _) = program::SHOT;
    let speed = f32::from(program::parameter(16)) / 2.;
    LOCAL.with(|state| {
        state.select_owner(actor);
        state.shots.push(Projectile {
            area,
            position,
            velocity: direction.map(|component| component * speed),
            remaining: duration,
        });
    });
}

pub extern "C" fn update(scene: &HurtScene<'_>) {
    LOCAL.with(|state| {
        state.select_owner(scene.owner());
        let frames = scene.frames();
        if frames == 0. {
            return;
        }
        state.gauge.refill(frames);
        let mark = &mut state.mark;
        if let Some(current) = mark {
            current.remaining -= frames;
            if current.remaining <= 0. || scene.position(&current.target).is_none() {
                *mark = None;
            }
        }
        let (_, radius) = program::SHOT;
        state.shots.retain_mut(|shot| {
            if shot.area != scene.area() {
                return false;
            }
            let step = frames.min(shot.remaining);
            let end = core::array::from_fn(|i| shot.position[i] + step * shot.velocity[i]);
            // FZ 81165A8E stops at stage geometry or the higher floor/water surface.
            if scene.terrain(shot.position, end).is_some()
                || scene
                    .surface_height(end)
                    .is_some_and(|height| end[1] <= height)
            {
                return false;
            }
            if let Some(target) = scene.sweep(shot.position, end, radius) {
                // FZ 81227846 has one mark per owner. Parameter 14 counts
                // native simulation ticks (30 Hz), while GU time is 60 Hz.
                *mark = Some(Mark {
                    target,
                    remaining: 2. * f32::from(program::parameter(14)),
                });
                return false;
            }
            shot.remaining -= step;
            shot.position = end;
            shot.remaining > 0.
        });
    });
}
