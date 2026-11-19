//! Motion stages and ordinary species-80 projectile frame events.
use super::{
    byte, f32at,
    package::{Directory, FlightCurve},
};
use espinas_behavior::Request;
use mhgu_sdk::{
    combat::shell::ShellLaunch,
    geometry::{Angle, Bearing},
    monster::{MonsterState, MonsterUpdate, movement::Posture, shell::DirectedShellResource},
};
// A Frontier movement step spans two GU frames, possibly across several updates.
fn take_step_frames(frames_left: &mut f32, step_left: &mut f32) -> f32 {
    if *step_left == 0. {
        *step_left = 2.;
    }
    let elapsed = frames_left.min(*step_left);
    *frames_left -= elapsed;
    *step_left -= elapsed;
    elapsed
}
pub(super) struct Air {
    phase: Request,
    entry: u32,
    frame: f32,
    clock: f32,
    step_left: f32,
    past_curve: bool,
    vy: f32,
    vz: f32,
    landing_frames_left: f32,
    approach: Option<Approach>,
}
// 06 02 00 00 asks for the first area point; an unmapped field keeps the
// monster's own position. The event never substitutes the hunter's coordinates.
struct Approach {
    point: [f32; 3],
    ticks_left: u32,
    velocity: [f32; 3],
    bob: f32,
}

impl Approach {
    fn new(update: MonsterUpdate, state: MonsterState) -> Self {
        update.enter_free_flight();
        let point = update
            .monster()
            .patrol_route(0)
            .and_then(|r| r.point(0))
            .unwrap_or_else(|| state.position());
        let horizontal = state.bearing_to((point[0], point[2])).distance;
        let distance = Bearing::between_xz((0., state.height()), (horizontal, point[1])).distance;
        Self {
            point,
            ticks_left: 3 * (distance / 20.) as u32,
            velocity: [0.; 3],
            bob: 0.,
        }
    }

    // 81CD2060 moves once at the pitch to the point; 81CE4C4C adds the
    // unpitched hover displacement outside the blend. Test arrival after both.
    fn begin_step(&mut self, state: MonsterState, hover: Option<f32>) {
        let bearing = state.bearing_to((self.point[0], self.point[2]));
        let yaw = if bearing.distance == 0. {
            Angle::from_units(0xc000)
        } else {
            bearing.yaw
        };
        state.turn_toward(yaw, 256);
        let (sin, cos) = state.yaw().sin_cos();
        let (rise, forward) =
            Bearing::between_xz((0., 0.), (self.point[1] - state.height(), bearing.distance))
                .yaw
                .sin_cos();
        let mut speed = 20. * forward - self.bob * rise;
        let mut height = 20. * rise + self.bob * forward;
        if let Some(bob) = hover {
            self.bob = bob;
            speed += 20.;
            height += bob;
        }
        self.velocity = [sin * speed, height, cos * speed];
        self.ticks_left = self.ticks_left.saturating_sub(1);
    }
}
pub(super) enum Next {
    Wait,
    Play(Request),
    Think,
}
impl Air {
    pub(super) fn new(r: Request) -> Option<Self> {
        matches!(r, Request(3, 5 | 6 | 7 | 17) | Request(2, 1 | 7)).then_some(Self {
            phase: r,
            entry: 0,
            frame: -1.,
            clock: 0.,
            step_left: 0.,
            past_curve: false,
            vy: 0.,
            vz: 0.,
            landing_frames_left: 0.,
            approach: None,
        })
    }
    pub(super) fn bind(&mut self, entry: u32, r: Request) {
        self.entry = entry;
        self.phase = r;
        self.frame = -1.;
        self.step_left = 0.;
        self.past_curve = false;
        self.clock = match r {
            Request(3, 17) => 76.,
            Request(252, 116) => 70.,
            _ => 0.,
        };
    }
    pub(super) unsafe fn tick(
        &mut self,
        d: &Directory,
        update: MonsterUpdate,
        state: MonsterState,
        frame: f32,
        end: bool,
        dt: f32,
    ) -> Next {
        unsafe {
            let last = self.frame;
            self.frame = frame;
            let crossed = |t: f32| {
                if frame < last {
                    t > last || t <= frame
                } else {
                    last < t && t <= frame
                }
            };
            let shots: &[(f32, u16, i16)] = match self.phase {
                Request(3, 5) => &[(80., 0, 0)],
                Request(3, 6 | 7) => &[(76., 0x16c1, -0x1332), (122., 0x16c1, 0x1333)],
                Request(252, 115) => &[(26., 0x1c72, 0)],
                Request(252, 116) => &[(50., 0x1e94, 0)],
                _ => &[],
            };
            for &(t, pitch, yaw) in shots {
                // Every shot precedes the flight loop; wrapping to 70 never crosses 50.
                if last < t && t <= frame {
                    fire(d, update, state, pitch, yaw);
                }
            }
            match self.phase {
                Request(3, 6 | 7) if end => {
                    Next::Play(Request(252, if self.phase.1 == 6 { 115 } else { 116 }))
                }
                Request(3, 17) | Request(252, 116) => {
                    let trigger = if self.phase == Request(3, 17) {
                        76.
                    } else {
                        36.
                    };
                    if frame < trigger && !end {
                        return Next::Wait;
                    }
                    // 81CE4E06 samples both tables on FZ logic ticks; motion 116
                    // starts its curve at 70 even though the motion trigger is 36.
                    // BF340 pins posture 3 to terrain and BF828 adds a fixed
                    // visual hover offset. Posture 1 preserves the actual flight height.
                    update.enter_free_flight();
                    let ground = state.ground_height();
                    let (sin_yaw, cos_yaw) = state.yaw().sin_cos();
                    let mut dt = dt;
                    while dt > 0. {
                        if self.step_left == 0. {
                            let vertical =
                                d.curve_slope(self.entry, FlightCurve::Vertical, self.clock);
                            let forward =
                                d.curve_slope(self.entry, FlightCurve::Forward, self.clock);
                            let advance = 2. * update.monster().motion_rate();
                            self.vy = vertical.map_or((self.vy - 1.).max(-50.), |v| v * advance);
                            if let Some(v) = forward {
                                self.vz = v * advance;
                            }
                            self.past_curve = vertical.is_none() || forward.is_none();
                            // 81CE4E06 stores the curve cursor in a short, but
                            // computes velocity with the unrounded animation step.
                            self.clock += (advance as u16) as f32;
                        }
                        // Divide this FZ tick's displacement between GU frames.
                        // Read the live position to retain GU terrain/wall corrections.
                        let fraction = take_step_frames(&mut dt, &mut self.step_left) * 0.5;
                        let [x, y, z] = state.position();
                        let height = y + self.vy * fraction;
                        state.set_position([
                            x + sin_yaw * self.vz * fraction,
                            height.max(ground),
                            z + cos_yaw * self.vz * fraction,
                        ]);
                        if self.past_curve && height <= ground {
                            update.set_posture(Posture::Ground);
                            return Next::Play(Request(252, 57));
                        }
                    }
                    Next::Wait
                }
                Request(2, 7) => {
                    let approach = self
                        .approach
                        .get_or_insert_with(|| Approach::new(update, state));
                    let mut left = dt;
                    while left > 0. {
                        if self.step_left == 0. {
                            let hover = if update.monster().motion_blend_remaining() > 0. {
                                None
                            } else if frame <= 1. {
                                Some(0.)
                            } else {
                                let rate = update.monster().motion_rate();
                                Some(
                                    d.curve_slope(self.entry, FlightCurve::Vertical, frame)
                                        .map_or(approach.bob, |v| v * 2. * rate),
                                )
                            };
                            approach.begin_step(state, hover);
                        }
                        let elapsed = take_step_frames(&mut left, &mut self.step_left);
                        let mut pos = state.position();
                        for (p, v) in pos.iter_mut().zip(approach.velocity) {
                            *p += v * elapsed * 0.5;
                        }
                        pos[1] = pos[1].max(state.ground_height());
                        state.set_position(pos);
                        if self.step_left == 0.
                            && (approach.ticks_left == 0
                                || state
                                    .bearing_to((approach.point[0], approach.point[2]))
                                    .distance
                                    <= 200.)
                        {
                            self.approach = None;
                            return Next::Play(Request(2, 1));
                        }
                    }
                    Next::Wait
                }
                Request(2, 1) => {
                    let ground = state.ground_height();
                    if last < 0. {
                        update.enter_free_flight();
                        self.vy = -10.;
                        let mut h = state.height() - ground - 1000.;
                        while h > 0. && self.vy >= -200. {
                            self.vy -= 0.75;
                            h += self.vy;
                        }
                    }
                    // 81CD1C72 moves with the current velocity before braking
                    // by 0.75 per FZ tick. Preserve that order for partial GU frames.
                    let mut dt = dt;
                    while dt > 0. {
                        let elapsed = take_step_frames(&mut dt, &mut self.step_left);
                        state.set_height((state.height() + self.vy * (elapsed * 0.5)).max(ground));
                        if self.step_left == 0. && self.vy < -10. {
                            self.vy += 0.75;
                        }
                    }
                    if state.height() < ground + 630. && [10., 86., 160.].into_iter().any(crossed) {
                        self.vy = (ground - state.height()) / 30.;
                        if self.vy >= 0. {
                            self.vy = -10.;
                        }
                        // The native counter starts at 30 and switches to 19 when negative.
                        self.landing_frames_left = 62.;
                        Next::Play(Request(252, 11))
                    } else {
                        Next::Wait
                    }
                }
                Request(252, 11) => {
                    let elapsed = dt.min(self.landing_frames_left);
                    self.landing_frames_left -= elapsed;
                    state.set_height(
                        (state.height() + self.vy * (elapsed * 0.5)).max(state.ground_height()),
                    );
                    if self.landing_frames_left == 0. {
                        update.set_posture(Posture::Ground);
                        return Next::Play(Request(252, 19));
                    }
                    Next::Wait
                }
                _ if end => Next::Think,
                _ => Next::Wait,
            }
        }
    }
}
unsafe fn fire(
    d: &Directory,
    update: MonsterUpdate,
    state: MonsterState,
    pitch: u16,
    yaw_offset: i16,
) {
    unsafe {
        let src = d.row(20);
        let Some(origin) =
            update.joint_world_position(byte(src, 28), [0., f32at(src, 20), f32at(src, 24)])
        else {
            return;
        };
        let yaw = state.yaw().offset(i32::from(yaw_offset));
        launch(
            update,
            origin,
            Angle::from_units(pitch),
            yaw,
            f32at(src, 16) * 0.5,
        );
    }
}

fn launch(update: MonsterUpdate, origin: [f32; 3], pitch: Angle, yaw: Angle, speed: f32) {
    // Our ARC authors the asset specifically for the GU directed-shell class.
    let Some(resource) =
        (unsafe { DirectedShellResource::from_asset(update, super::resources::FIREBALL) })
    else {
        return;
    };
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let velocity = [
        sin_yaw * cos_pitch * speed,
        -sin_pitch * speed,
        cos_yaw * cos_pitch * speed,
    ];
    resource.spawn(ShellLaunch {
        origin,
        pitch,
        yaw,
        velocity,
    });
}
