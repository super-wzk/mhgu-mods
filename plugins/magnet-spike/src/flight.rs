//! FZ aerial actions, integrated by GU's movement and landing.
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

#[derive(Default)]
pub(super) struct AxisMotion {
    speed: f32,
    ticks: f32,
    pub damped: bool,
}

impl AxisMotion {
    pub const fn new() -> Self {
        Self {
            speed: 0.,
            ticks: 0.,
            damped: false,
        }
    }

    pub fn set_speed(&mut self, value: u16) {
        // FZ 812261BA/812261DC decode a signed byte without -128.
        self.speed = f32::from(value.wrapping_add(u16::from(value > 127)) as i8);
        self.ticks = 0.;
    }

    pub fn moving(&self) -> bool {
        self.speed != 0.
    }

    pub fn distance(&mut self, frames: f32) -> f32 {
        if !self.moving() || frames <= 0. || !frames.is_finite() {
            return 0.;
        }
        let elapsed = frames / 2.;
        let start = self.ticks;
        self.ticks += elapsed;
        if !self.damped {
            return self.speed * elapsed;
        }
        // FZ 8121F57C samples each 30 Hz tick at 0.97^n.
        const DRAG: f32 = 0.97;
        let area = |time| {
            let whole = libm::floorf(time);
            let speed = libm::powf(DRAG, whole);
            (1. - speed) / (1. - DRAG) + (time - whole) * speed
        };
        self.speed * (area(self.ticks) - area(start))
    }
}

#[derive(PartialEq)]
enum Phase {
    Windup,
    Guided,
    Coast,
    DropWindup,
    Drop,
    Landed,
}

pub(super) struct Flight {
    target: Option<Target>,
    phase: Phase,
    direction: [f32; 3],
    velocity: [f32; 3],
    elapsed: f32,
    vertical: AxisMotion,
    fall_speed: f32,
    fall_ticks: f32,
    gravity: f32,
    landing_delay: f32,
    evade_until_landing: bool,
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
    pub fn attraction(target: Target) -> Self {
        Self::new(Some(target), Phase::Windup)
    }

    pub fn bind_drop() -> Self {
        Self::new(None, Phase::DropWindup)
    }

    fn new(target: Option<Target>, phase: Phase) -> Self {
        Self {
            target,
            phase,
            direction: [0.; 3],
            velocity: [0.; 3],
            elapsed: 0.,
            vertical: AxisMotion::new(),
            fall_speed: 0.,
            fall_ticks: 0.,
            gravity: GRAVITY,
            landing_delay: 0.,
            evade_until_landing: false,
        }
    }

    pub fn launched(&self) -> bool {
        matches!(self.phase, Phase::Guided | Phase::Coast)
    }

    pub fn coasting(&self) -> bool {
        self.phase == Phase::Coast
    }

    fn coast(&mut self) {
        self.phase = Phase::Coast;
    }

    pub fn follow_up(&mut self, state: u8) {
        if matches!(state, 59 | 73) {
            // 81224122 and 81224070 bypass the magnetic guidance routine.
            self.phase = Phase::Drop;
            self.velocity = [0.; 3];
            self.elapsed = 0.;
        } else {
            self.coast();
        }
    }

    pub fn event(&mut self, id: u16, value: u16) {
        match id {
            1 if value == 3 && self.phase == Phase::DropWindup => self.phase = Phase::Drop,
            9 => self.evade_until_landing = true,
            25 => {
                self.fall_speed = -f32::from(value as u8) / 10.;
                self.fall_ticks = 0.;
            }
            28 => self.vertical.set_speed(value),
            30 => self.vertical.damped = value == 1,
            // 812214B6 applies 812262BC's gravity override in coast only.
            31 => self.gravity = -f32::from(value) / 400.,
            // 8122630C sets the time before 8121FC88 may test for landing.
            33 => self.landing_delay = f32::from(value) * 2.,
            // 8122636E clears the magnetic heading for emergency landings.
            36 => {
                self.direction = [0.; 3];
                self.velocity = [0.; 3];
            }
            _ => {}
        }
    }

    fn fall_distance(&mut self, frames: f32) -> f32 {
        if self.fall_speed == 0. {
            return 0.;
        }
        // 8121F57C subtracts rate × tick, starting at tick zero. The
        // integral includes fractional ticks when GU updates at 60 Hz.
        let area = |time| {
            let whole = libm::floorf(time);
            whole * (whole - 1.) / 2. + (time - whole) * whole
        };
        let start = self.fall_ticks;
        self.fall_ticks += frames / 2.;
        self.fall_speed * (area(self.fall_ticks) - area(start))
    }

    fn destination(&self, actor: u32, origin: [f32; 3]) -> Option<(Bearing, [f32; 3])> {
        // The saved Target checks its area, roster slot, part and living status.
        let [x, y, z] = unsafe { self.target?.position_for(actor) }?;
        let bearing = Bearing::between_xz((origin[0], origin[2]), (x, z));
        let (sin, cos) = bearing.yaw.sin_cos();
        let point = [x - STANDOFF * sin, y + HEIGHT_OFFSET, z - STANDOFF * cos];
        Some((bearing, displacement(origin, point)))
    }

    /// True after floor contact, or if the mark disappears before takeoff.
    pub unsafe fn advance(&mut self, actor: u32, state: u8, forward: &mut AxisMotion) -> bool {
        if self.phase == Phase::Landed {
            return true;
        }
        let elapsed = unsafe { movement::motion_frames(actor) };
        if !elapsed.is_finite() || elapsed <= 0. {
            return false;
        }
        if self.evade_until_landing {
            // Event 9 (81225D52) keeps at least two FZ ticks of immunity.
            unsafe { movement::grant_evade_invulnerability(actor, 4.) };
        }
        let can_land = self.landing_delay == 0.;
        self.landing_delay = (self.landing_delay - elapsed).max(0.);
        if self.phase == Phase::DropWindup {
            // 8121F57C applies event 25 even before event 1 enables landing.
            let distance = self.fall_distance(elapsed);
            unsafe { movement::displace_vertical(actor, distance) };
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
            acceleration[1] = self.gravity;
        }
        let landing_tolerance = if state == 73 {
            // 812240BA first checks for a floor on tick 26. 8121FC88
            // allows the hammer to contact it 164 units below the root.
            self.elapsed += elapsed;
            (self.elapsed >= 26. * 2.).then_some(164.)
        } else {
            can_land.then_some(0.)
        };
        let extra = [
            forward.distance(elapsed),
            self.fall_distance(elapsed) + self.vertical.distance(elapsed),
        ];
        let step = unsafe {
            movement::advance_airborne(actor, self.velocity, acceleration, extra, landing_tolerance)
        };
        self.velocity = step.velocity;
        if step.landed {
            self.phase = Phase::Landed;
        } else if step.blocked && self.phase != Phase::Drop {
            self.coast();
        }
        step.landed
    }
}
