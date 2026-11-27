//! Feedback issued by the w13 controller in addition to the motion file.
use crate::{fx, magnet, program::Mode, sound};
use mhgu_sdk::weapon::style::{
    action::MotionFrame,
    particle::{self, Anchor},
};
mod rows;

#[derive(Clone, Copy)]
enum Audience {
    Everyone,
    Boosted,
    Local,
    Peer,
    LocalBoost,
    LocalNormal,
    ExceptLocalBoost,
    ExceptLocalNormal,
}

impl Audience {
    fn visible(self, local: bool, boosted: bool) -> bool {
        match self {
            Self::Everyone => true,
            Self::Boosted => boosted,
            Self::Local => local,
            Self::Peer => !local,
            Self::LocalBoost => local && boosted,
            Self::LocalNormal => local && !boosted,
            Self::ExceptLocalBoost => !local || !boosted,
            Self::ExceptLocalNormal => !local || boosted,
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Sound(u8),
    World(u16, u16),
    Joint(u16, u16),
    Part(u16),
    Mark,
    Line,
    Shock,
    LateralImpact,
    Counter,
    AdvancingImpact,
}
type Event = (u8, Audience, Kind, u8, [i16; 3]);

#[derive(Default)]
pub(super) struct Playback {
    next: usize,
}

impl Playback {
    pub(super) const NEW: Self = Self { next: 0 };

    pub(super) unsafe fn enter_clip(
        &mut self,
        actor: u32,
        cursor: &mhfz_sdk::action::Cursor,
        step: mhfz_sdk::action::Step,
    ) {
        let state = cursor.state;
        if crate::program::is_attraction(state) {
            // 812241AC queues request 18/19 once per action, including during blend.
            // The muzzle, line and two wrist particles share byte +5 across its clips.
            if cursor.time.step == 0 {
                self.next = 0;
                let offset = if matches!(state, 58 | 61 | 62) {
                    [30., 0., -20.]
                } else {
                    [20., 0., 140.]
                };
                unsafe { queue(actor, Request::Launch(offset)) };
            }
            return;
        }
        self.next =
            rows::rows(state, step.clip).partition_point(|row| u16::from(row.0) < step.start);
        // 81223B90 dispatches the rear request on entering 1082, even during blend.
        if step.clip == 1082 && matches!(state, 161 | 162) {
            unsafe { crate::shock::start(actor, state == 162) };
        }
    }

    pub(super) unsafe fn sample(
        &mut self,
        actor: u32,
        state: u8,
        clip: u16,
        sample: MotionFrame,
        mode: Mode,
        boosts: u8,
        peer: Option<usize>,
        accent: &mut bool,
    ) {
        if sample.blend > 0. {
            return;
        }
        let events = rows::rows(state, clip);
        let boosted = boosts & (1 << mode as u8) != 0;
        while let Some(&(frame, audience, kind, node, xyz)) = events.get(self.next) {
            if sample.frame < f32::from(frame) {
                break;
            }
            self.next += 1;
            if !audience.visible(peer.is_none(), boosted) {
                continue;
            }
            let offset = xyz.map(f32::from);
            let joint = match node {
                14 => 8,
                10 => 2,
                _ => -1,
            };
            unsafe {
                match kind {
                    Kind::Sound(13) => sound::motion_accent(actor, accent, joint as u32, offset),
                    Kind::Sound(cue) => sound::at(actor, cue, position(actor, joint, offset).0),
                    Kind::World(bank, selector) => {
                        let (mut point, yaw) = fx::root_offset(actor, offset);
                        if (bank, selector) == (4037, 40) {
                            // 81223B2E traces from 200 above the actor's forward point.
                            let Some((floor, _)) = particle::ground_heights(actor, point) else {
                                continue;
                            };
                            point[1] = floor;
                        }
                        fx::native(bank, selector, Anchor::World(point, yaw));
                    }
                    Kind::Joint(bank, selector) => attached(actor, bank, selector, joint, offset),
                    Kind::Counter => {
                        // 81167412 spawns 100 units forward. Family 121 projectile
                        // parameters 0..2 move all three ranks 40 units per original tick.
                        let (point, yaw) = particle::world_offset(actor, [0., 0., 100.]);
                        let (sin, cos) = mhgu_sdk::geometry::Angle::from_units(yaw).sin_cos();
                        let row = match state {
                            57 => 5,
                            76 => 6,
                            _ => 7,
                        };
                        crate::shock::detached(
                            actor,
                            row,
                            point,
                            [40. * sin, 0., 40. * cos],
                            0.,
                            peer.is_none(),
                        );
                    }
                    Kind::AdvancingImpact => {
                        let (origin, yaw) = fx::root_offset(actor, [0.; 3]);
                        queue(actor, Request::AdvancingImpact { origin, yaw });
                    }
                    Kind::Shock => queue(actor, Request::Impact),
                    Kind::LateralImpact => {
                        crate::shock::start(actor, matches!(state, 159 | 160));
                    }
                    Kind::Mark | Kind::Part(_) | Kind::Line => {
                        let Some(target) = magnet::marked_target(actor, peer) else {
                            continue;
                        };
                        let Some(mark) = target.position_for(actor) else {
                            continue;
                        };
                        let (origin, _) = fx::root_offset(actor, [0.; 3]);
                        let delta: [f32; 3] = core::array::from_fn(|i| mark[i] - origin[i]);
                        let distance = libm::sqrtf(delta.iter().map(|v| v * v).sum());
                        if distance <= 1. {
                            continue;
                        }
                        match kind {
                            Kind::Mark => fx::native(4037, 7, Anchor::World(mark, 0)),
                            Kind::Part(selector @ (2 | 5)) => {
                                let (point, _) = position(actor, joint, offset);
                                let angles = aimed_angles(point, mark);
                                let scale = if selector == 2 { distance / 4000. } else { 1. };
                                if let Some(effect) =
                                    fx::Effect::aimed(selector, point, angles, scale)
                                {
                                    queue(
                                        actor,
                                        Request::Aimed {
                                            effect,
                                            target,
                                            joint,
                                            offset,
                                        },
                                    );
                                }
                            }
                            Kind::Part(selector) => attached(actor, 4037, selector, joint, offset),
                            Kind::Line if distance > 500. => {
                                let (sin, cos) =
                                    Bearing::between_xz((origin[0], origin[2]), (mark[0], mark[2]))
                                        .yaw
                                        .sin_cos();
                                queue(
                                    actor,
                                    Request::Line {
                                        origin,
                                        direction: delta.map(|v| v / distance),
                                        side: [50. * cos, 0., -50. * sin],
                                        last: ((distance - 300.) / 220.).min(254.) as u8,
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

unsafe fn position(actor: u32, joint: i16, offset: [f32; 3]) -> ([f32; 3], [u16; 3]) {
    if joint >= 0
        && let Some(p) = unsafe { particle::joint_matrix(actor, joint as u32) }
    {
        let mat = unsafe { &*(p as *const [f32; 16]) };
        return (
            particle::transform_offset(mat, offset),
            mhfz_sdk::effects::world_pose_angles(mat),
        );
    }
    let (point, yaw) = unsafe { fx::root_offset(actor, offset) };
    (point, [0, yaw, 0])
}

pub(super) unsafe fn attached(actor: u32, bank: u16, selector: u16, joint: i16, offset: [f32; 3]) {
    if let Some(parent) = unsafe { particle::parent(actor) } {
        unsafe {
            fx::native(
                bank,
                selector,
                Anchor::JointPose {
                    parent,
                    joint,
                    offset: Some(offset),
                },
            )
        };
    }
}

fn aimed_angles(point: [f32; 3], mark: [f32; 3]) -> [u16; 3] {
    let bearing = Bearing::between_xz((point[0], point[2]), (mark[0], mark[2]));
    let pitch = libm::roundf(
        -libm::atan2f(mark[1] - point[1], bearing.distance) * (65536. / core::f32::consts::TAU),
    ) as i32;
    [pitch as u16, bearing.yaw.units(), 0]
}

enum Request {
    Aimed {
        effect: fx::Effect,
        target: mhgu_sdk::combat::collision::hurt::Target,
        joint: i16,
        offset: [f32; 3],
    },
    Launch([f32; 3]),
    Charge,
    Impact,
    AdvancingImpact {
        origin: [f32; 3],
        yaw: u16,
    },
    Line {
        origin: [f32; 3],
        direction: [f32; 3],
        side: [f32; 3],
        last: u8,
    },
}

struct Pending {
    actor: u32,
    area: u8,
    age: f32,
    next: u8,
    request: Request,
}

use alloc::vec::Vec;
use mhgu_sdk::{
    geometry::Bearing,
    sync::Shared,
    weapon::style::{
        hunter,
        remote::{self, Roster},
    },
};
static PENDING: Shared<Vec<Pending>> = Shared::new(Vec::new());

unsafe fn queue(actor: u32, request: Request) {
    let area = unsafe { hunter::area(actor) };
    PENDING.with(|pending| {
        pending.push(Pending {
            actor,
            area,
            age: 0.,
            next: 0,
            request,
        })
    });
}

pub(super) unsafe fn charge_sparks(actor: u32) {
    unsafe { queue(actor, Request::Charge) };
}

pub(super) fn reset(owner: Option<u32>) {
    PENDING.with(|pending| match owner {
        Some(owner) => pending.retain(|p| p.actor != owner),
        None => pending.clear(),
    });
}

// 8102CDDC requests 9, 17, and 18/19 use a 30 Hz scene clock.
pub(super) unsafe fn advance(
    local: Option<u32>,
    area: Option<u8>,
    roster: Option<&Roster>,
    frames: f32,
) {
    PENDING.with(|pending| {
        pending.retain_mut(|p| unsafe {
            let local_owner = local == Some(p.actor);
            if area != Some(p.area)
                || !local_owner
                    && !roster.is_some_and(|r| {
                        r.peer_index(p.actor).is_some()
                            && remote::is_position_event_peer(crate::STYLE.id, p.actor)
                    })
                || hunter::area(p.actor) != p.area
                || !hunter::is_alive(p.actor)
            {
                return false;
            }
            if frames <= 0. {
                return true;
            }
            let age = p.age;
            p.age += frames;
            match p.request {
                Request::Aimed {
                    ref effect,
                    target,
                    joint,
                    offset,
                } => {
                    let Some(mark) = target.position_for(p.actor) else {
                        return false;
                    };
                    let (point, _) = position(p.actor, joint, offset);
                    effect.follow_world_pose(point, aimed_angles(point, mark))
                }
                Request::Launch(offset) => {
                    if age < 2. {
                        return true;
                    }
                    attached(p.actor, 4037, 3, 8, offset);
                    if local_owner {
                        crate::battle::launch_sound(p.actor, offset);
                    } else if let Some(index) = roster.and_then(|r| r.peer_index(p.actor)) {
                        crate::peer::launch_sound(p.actor, index, offset);
                    }
                    false
                }
                Request::AdvancingImpact { origin, yaw } => {
                    // 81167600/8116776C: family 121 object 10 and its two
                    // descendants share the first root and yaw.
                    let (sin, cos) = mhgu_sdk::geometry::Angle::from_units(yaw).sin_cos();
                    for &(x, z) in [(-80., 350.), (70., 450.), (-20., 600.)]
                        .iter()
                        .skip(usize::from(p.next))
                    {
                        if age < 6. * f32::from(p.next) {
                            return true;
                        }
                        let point = [
                            origin[0] + cos * x + sin * z,
                            origin[1],
                            origin[2] - sin * x + cos * z,
                        ];
                        fx::native(4037, 94, Anchor::World(point, yaw));
                        crate::shock::detached(
                            p.actor,
                            10,
                            point,
                            [0.; 3],
                            (age - 6. * f32::from(p.next)).max(0.),
                            local_owner,
                        );
                        p.next += 1;
                    }
                    false
                }
                Request::Charge => {
                    while p.next < 3 {
                        let stage = f32::from(p.next + 1);
                        if age < 8. * stage {
                            return true;
                        }
                        attached(p.actor, 4037, 21, 8, [0., 0., 40. * stage]);
                        p.next += 1;
                    }
                    false
                }
                Request::Impact => {
                    let pulses = [
                        (0, 50, [-40., 0., -120.]),
                        (0, 50, [-190., 0., -50.]),
                        (2, 50, [-180., 0., 110.]),
                        (6, 50, [-80., 0., 270.]),
                        (8, 51, [150., 0., 260.]),
                    ];
                    while let Some(&(time, id, offset)) = pulses.get(usize::from(p.next)) {
                        if age < time as f32 {
                            return true;
                        }
                        let (point, yaw) = fx::root_offset(p.actor, offset);
                        fx::native(4038, id, Anchor::World(point, yaw));
                        p.next += 1;
                    }
                    false
                }
                Request::Line {
                    origin,
                    direction,
                    side,
                    last,
                } => {
                    while p.next <= last {
                        if age < 4. * f32::from(p.next) {
                            return true;
                        }
                        let distance = 300. + 220. * f32::from(p.next);
                        let point =
                            core::array::from_fn(|i| origin[i] + distance * direction[i] + side[i]);
                        fx::native(4037, 0, Anchor::World(point, 0));
                        p.next += 1;
                    }
                    false
                }
            }
        })
    });
}
