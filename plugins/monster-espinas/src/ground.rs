//! Ordinary species 80 ground handlers: 81CD3E9A, 81CCE024 and 81CCDA3E.
use super::hunt::target_position;
use super::*;
use espinas_behavior::Request;

#[derive(Clone, Copy)]
struct Leg {
    remaining: f32,
    ticks: Option<f32>,
    goal: Option<(f32, f32)>,
    steer: f32,
    walk: bool,
}
#[derive(Clone, Copy)]
struct Sequence {
    turn: u8,
    count: u8,
    original: Leg,
    start_yaw: Angle,
}
enum Stage {
    Travel {
        leg: Leg,
        series: Option<Sequence>,
        brake: u8,
        prev: f32,
    },
    Pivot(Sequence),
    Tail {
        clip: u8,
        second: bool,
        step: Option<i32>,
    },
    WalkStop,
}
pub(super) struct Ground {
    stage: Stage,
    entry: u32,
}
pub(super) enum Next {
    Wait,
    Rearm(u32),
    Play(Request),
    Think,
}

impl Ground {
    pub(super) fn start(
        update: MonsterUpdate,
        state: MonsterState,
        request: Request,
        rng: u16,
        waypoint: Option<(f32, f32)>,
    ) -> Option<Self> {
        let walk = matches!(request, Request(1, 0) | Request(251, 10));
        let stage = if matches!(request, Request(3, 13 | 14)) {
            Stage::Tail {
                clip: request.1,
                second: false,
                step: None,
            }
        } else if walk || matches!(request, Request(3, 1 | 2 | 18 | 19)) {
            let goal = waypoint.or_else(|| target_position(update, state));
            let distance = goal.map(|p| state.bearing_to(p).distance);
            let remaining = if walk {
                distance.map_or(1000., |d| {
                    if waypoint.is_some() {
                        d
                    } else {
                        (d - 500.).max(0.)
                    }
                })
            } else {
                distance.map_or(1000., |d| (d - 500.).max(1000.))
            };
            let leg = Leg {
                remaining,
                ticks: (!walk).then(|| {
                    if goal.is_some() {
                        (remaining / 30.) as u32 as f32 + 30.
                    } else {
                        60.
                    }
                }),
                goal,
                steer: if walk { 32. } else { 64. },
                walk,
            };
            let series = matches!(request, Request(3, 18 | 19)).then(|| Sequence {
                // 81CCDE30 really selects column 0 again above 60.
                turn: match ((41..=60).contains(&(rng % 100)), rng & 1) {
                    (false, 0) => 122,
                    (true, 0) => 123,
                    (false, _) => 119,
                    (true, _) => 120,
                },
                count: request.1 - 17,
                original: leg,
                start_yaw: Angle::default(),
            });
            Stage::Travel {
                leg,
                series,
                brake: if walk {
                    1
                } else if request.1 == 1 {
                    108
                } else {
                    118
                },
                prev: 0.,
            }
        } else {
            return None;
        };
        Some(Self { stage, entry: 0 })
    }
    pub(super) fn bind(&mut self, e: u32) {
        self.entry = e
    }
    pub(super) unsafe fn update(
        &mut self,
        d: &Directory,
        state: MonsterState,
        frame: f32,
        ended: bool,
        dt: f32,
        ticks: u16,
    ) -> Next {
        unsafe {
            let e = self.entry;
            match &mut self.stage {
                Stage::WalkStop => {
                    if frame >= 2. || ended {
                        Next::Think
                    } else {
                        Next::Wait
                    }
                }
                Stage::Tail { clip, second, step } => {
                    let len = u32at(e, 16) as f32 - 1.;
                    if step.is_none() && frame >= f32::from(byte(e, 7)) * 2. {
                        let sign = if *clip == 13 { -1. } else { 1. };
                        *step = Some((sign * 16384. / ((len - frame).max(2.) * 0.5)) as i32);
                    } else if let Some(rate) = *step {
                        let yaw = state.yaw().offset(rate * i32::from(ticks));
                        state.set_yaw(yaw);
                    }
                    if !ended {
                        Next::Wait
                    } else if *second {
                        Next::Think
                    } else {
                        *second = true;
                        Next::Play(Request(3, *clip))
                    }
                }
                Stage::Pivot(seq) => {
                    let a = f32::from(u16at(e, 20));
                    let b = f32::from(u16at(e, 22));
                    let degrees = i32::from(u16at(e, 24)) * if u16at(e, 28) != 0 { -1 } else { 1 };
                    let portion = (frame - a).clamp(0., b - a);
                    let yaw = seq
                        .start_yaw
                        .offset((degrees as f32 * portion / (b - a)) as i32);
                    state.set_yaw(yaw);
                    if !ended {
                        return Next::Wait;
                    }
                    let mut leg = seq.original;
                    leg.steer = 0.;
                    let q = *seq;
                    self.stage = Stage::Travel {
                        leg,
                        series: Some(q),
                        brake: 118,
                        prev: 0.,
                    };
                    Next::Play(Request(3, if q.count == 1 { 18 } else { 19 }))
                }
                Stage::Travel {
                    leg,
                    series,
                    brake,
                    prev,
                } => {
                    let wrapped = frame < *prev;
                    let delta = d.travel_delta(e, *prev, frame);
                    *prev = frame;
                    if leg.goal.is_some() || leg.walk {
                        leg.remaining -= delta;
                    }
                    if let Some(goal) = leg.goal {
                        state.turn_toward(state.bearing_to(goal).yaw, (leg.steer * dt) as u16);
                    }
                    if let Some(left) = &mut leg.ticks {
                        *left -= dt * 0.5;
                    }
                    let continue_travel = || {
                        if wrapped {
                            Next::Rearm(row_id(u16at(e, 4).into()))
                        } else {
                            Next::Wait
                        }
                    };
                    if !(leg.remaining <= 0. || leg.ticks.is_some_and(|t| t <= 0.)) {
                        return continue_travel();
                    }
                    if let Some(q) = series {
                        let turn_entry = d.lookup(Request(250, q.turn)).unwrap();
                        if (leg.steer != 0. || q.count > 1)
                            && frame < f32::from(u16at(turn_entry, 26))
                        {
                            return continue_travel();
                        }
                        if leg.steer == 0. {
                            q.count -= 1;
                        }
                        if q.count > 0 {
                            q.start_yaw = state.yaw();
                            let next = Request(250, q.turn);
                            self.stage = Stage::Pivot(*q);
                            return Next::Play(next);
                        }
                    }
                    let recovery = *brake;
                    self.stage = Stage::WalkStop;
                    Next::Play(Request(252, recovery))
                }
            }
        }
    }
}
