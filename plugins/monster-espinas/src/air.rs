//! Motion stages and ordinary species-80 projectile frame events.
use super::*;
use espinas_behavior::Request;
unsafe fn real(p: u32, off: u32) -> f32 {
    unsafe { f32::from_bits(u32at(p, off)) }
}
pub(super) struct Air {
    phase: Request,
    e: u32,
    frame: f32,
    clock: f32,
    vy: f32,
    vz: f32,
    descent: u8,
}
pub(super) enum Next {
    Wait,
    Play(Request),
    Think,
}
impl Air {
    pub(super) fn new(r: Request) -> Option<Self> {
        matches!(r, Request(3, 5 | 6 | 7 | 17) | Request(2, 1)).then_some(Self {
            phase: r,
            e: 0,
            frame: -1.,
            clock: 0.,
            vy: 0.,
            vz: 0.,
            descent: 0,
        })
    }
    pub(super) fn bind(&mut self, e: u32, r: Request) {
        self.e = e;
        self.phase = r;
        self.frame = -1.;
        self.clock = match r {
            Request(3, 17) => 76.,
            Request(252, 116) => 70.,
            _ => 0.,
        };
    }
    unsafe fn slope(d: &Directory, e: u32, offset: u32, t: f32) -> Option<f32> {
        unsafe {
            let p = d.rows() + 32 * u32at(e, offset);
            let n = u32at(p, 0);
            for i in 1..n {
                let row = p + 4 + 8 * i;
                let b = real(row, 0);
                if t <= b {
                    return Some((real(row, 4) - real(row - 8, 4)) / (b - real(row - 8, 0)));
                }
            }
            None
        }
    }
    pub(super) unsafe fn tick(
        &mut self,
        d: &Directory,
        update: MonsterUpdate,
        state: MonsterState,
        frame: f32,
        end: bool,
        ticks: u16,
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
                if crossed(t) {
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
                    if state.posture() != Posture::Flight {
                        update.set_posture(Posture::Flight);
                    }
                    if ticks == 0 {
                        return Next::Wait;
                    }
                    let ground = state.ground_height();
                    let (sin_yaw, cos_yaw) = state.yaw().sin_cos();
                    for _ in 0..ticks {
                        let vertical = Self::slope(d, self.e, 8, self.clock);
                        let forward = Self::slope(d, self.e, 12, self.clock);
                        self.vy = vertical.map_or((self.vy - 1.).max(-50.), |v| v * 2.);
                        if let Some(v) = forward {
                            self.vz = v * 2.;
                        }
                        let [x, y, z] = state.position();
                        let height = y + self.vy;
                        state.set_position([
                            x + sin_yaw * self.vz,
                            height.max(ground),
                            z + cos_yaw * self.vz,
                        ]);
                        self.clock += 2.;
                        if (vertical.is_none() || forward.is_none()) && height <= ground {
                            update.set_posture(Posture::Ground);
                            return Next::Play(Request(252, 57));
                        }
                    }
                    Next::Wait
                }
                Request(2, 1) => {
                    let ground = state.ground_height();
                    if last < 0. {
                        self.vy = -10.;
                        let mut h = state.height() - ground - 1000.;
                        while h > 0. && self.vy >= -200. {
                            self.vy -= 0.75;
                            h += self.vy;
                        }
                    }
                    for _ in 0..ticks {
                        state.set_height((state.height() + self.vy).max(ground));
                        if self.vy < -10. {
                            self.vy += 0.75;
                        }
                    }
                    if state.height() < ground + 630. && [10., 86., 160.].into_iter().any(crossed) {
                        self.vy = (ground - state.height()) / 30.;
                        if self.vy >= 0. {
                            self.vy = -10.;
                        }
                        self.descent = 0;
                        Next::Play(Request(252, 11))
                    } else {
                        Next::Wait
                    }
                }
                Request(252, 11) => {
                    for _ in 0..ticks {
                        self.descent += 1;
                        state.set_height((state.height() + self.vy).max(state.ground_height()));
                        if self.descent > 30 {
                            state.set_height(state.ground_height());
                            update.set_posture(Posture::Ground);
                            return Next::Play(Request(252, 19));
                        }
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
            update.joint_world_position(byte(src, 28), [0., real(src, 20), real(src, 24)])
        else {
            return;
        };
        let yaw = state.yaw().offset(i32::from(yaw_offset));
        rathian::fire(
            update,
            origin,
            Angle::from_units(pitch),
            yaw,
            real(src, 16) * 0.5,
        );
    }
}
