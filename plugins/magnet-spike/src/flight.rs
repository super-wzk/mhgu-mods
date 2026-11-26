//! FZ magnetic attraction, integrated by GU's airborne movement and landing.
use crate::magnet::ATTRACT_LIMIT;
use mhgu_sdk::{
    combat::collision::hurt::Target,
    geometry::Bearing,
    weapon::style::{action, movement, particle},
};

// FZ 8121FEC2 offsets the flight destination from the raw mark.
const STANDOFF: f32 = 200.;
const HEIGHT_OFFSET: f32 = 100.;
// 812212E8 uses 90 units per FZ tick, a five-tick ease-out, and -2.3 gravity.
const SPEED: f32 = 90. / 2.;
const ACCELERATION_FRAMES: f32 = 5. * 2.;
const GRAVITY: f32 = -2.3 / 4.;
const MAX_GUIDED_FRAMES: f32 = ATTRACT_LIMIT / SPEED + ACCELERATION_FRAMES;

#[derive(PartialEq)]
enum Phase {
    Windup,
    Guided,
    Coast,
    Landed,
}

pub(super) struct Flight {
    target: Target,
    phase: Phase,
    direction: [f32; 3],
    velocity: [f32; 3],
    elapsed: f32,
}

fn length(vector: [f32; 3]) -> f32 {
    libm::sqrtf(
        vector
            .into_iter()
            .map(|component| component * component)
            .sum(),
    )
}

fn displacement(from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    core::array::from_fn(|i| to[i] - from[i])
}

impl Flight {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            phase: Phase::Windup,
            direction: [0.; 3],
            velocity: [0.; 3],
            elapsed: 0.,
        }
    }

    pub fn launched(&self) -> bool {
        matches!(self.phase, Phase::Guided | Phase::Coast)
    }

    pub fn coasting(&self) -> bool {
        self.phase == Phase::Coast
    }

    pub fn coast(&mut self) {
        self.phase = Phase::Coast;
    }

    fn destination(&self, actor: u32, origin: [f32; 3]) -> Option<(Bearing, [f32; 3])> {
        // The saved Target checks its area, roster slot, part and living status.
        let [x, y, z] = unsafe { self.target.position_for(actor) }?;
        let bearing = Bearing::between_xz((origin[0], origin[2]), (x, z));
        let (sin, cos) = bearing.yaw.sin_cos();
        let point = [x - STANDOFF * sin, y + HEIGHT_OFFSET, z - STANDOFF * cos];
        Some((bearing, displacement(origin, point)))
    }

    /// True after floor contact, or if the mark disappears before takeoff.
    pub unsafe fn advance(&mut self, actor: u32) -> bool {
        if self.phase == Phase::Landed {
            return true;
        }
        let elapsed = unsafe { movement::motion_frames(actor) };
        if !elapsed.is_finite() || elapsed <= 0. {
            return false;
        }
        let start = self.phase == Phase::Windup;
        if start || self.phase == Phase::Guided {
            let origin = unsafe { movement::position(actor) };
            let guidance = self
                .destination(actor, origin)
                .filter(|(_, delta)| length(*delta) > STANDOFF && self.elapsed < MAX_GUIDED_FRAMES);
            if let Some((bearing, mut delta)) = guidance {
                if start {
                    delta[1] = delta[1].max(-0.176327 * bearing.distance);
                    unsafe { action::start_airborne_weapon_state(actor) };
                }
                let distance = length(delta);
                let mut direction = delta.map(|value| value / distance);
                if !start {
                    direction =
                        core::array::from_fn(|i| 0.2 * self.direction[i] + 0.8 * direction[i]);
                }
                if let Some((floor, _)) = unsafe { particle::ground_heights(actor, origin) } {
                    let height = (origin[1] - floor).clamp(0., 90.);
                    direction[1] = direction[1].max(-height / 90.);
                }
                let norm = length(direction);
                if norm > 0. && norm.is_finite() {
                    self.direction = direction.map(|value| value / norm);
                    let yaw =
                        Bearing::between_xz((0., 0.), (self.direction[0], self.direction[2])).yaw;
                    unsafe { movement::set_facing(actor, yaw.units()) };
                }
                self.phase = Phase::Guided;
                if start {
                    // The initialization tick only prepares the direction.
                    return false;
                }
                let t = (self.elapsed / ACCELERATION_FRAMES).min(1.);
                self.velocity = self.direction.map(|value| value * SPEED * t * (2. - t));
                self.elapsed += elapsed;
            } else if start {
                self.phase = Phase::Landed;
                return true;
            } else {
                self.coast();
            }
        }

        let mut acceleration = [0.; 3];
        if self.coasting() {
            let drag = libm::powf(0.97, elapsed / 2.);
            self.velocity[0] *= drag;
            self.velocity[2] *= drag;
            acceleration[1] = GRAVITY;
        }
        let step = unsafe { movement::advance_airborne(actor, self.velocity, acceleration) };
        self.velocity = step.velocity;
        if step.landed {
            self.phase = Phase::Landed;
        } else if step.blocked {
            self.coast();
        }
        step.landed
    }
}
