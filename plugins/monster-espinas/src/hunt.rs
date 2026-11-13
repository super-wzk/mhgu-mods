//! Runtime for the single combined hunt resource (ESH5). ESP1 replays never enter here.
use super::*;
use espinas_behavior::{Pending, Request, Spawn, SpawnPhase, Step};

enum Dispatch {
    Play {
        motion: MotionId,
        blend: u8,
        attack: Option<u32>,
    },
    Rearm(u32),
}

struct Hunter {
    spawn: Spawn,
    pending: Pending,
    playing: Option<MotionId>,
    missing: Option<Request>,
    fractional_ticks: f32,
    random: u16,
    turn: Option<Angle>,
    ground: Option<ground::Ground>,
    air: Option<air::Air>,
    waypoint: Option<(f32, f32)>,
    walking_to_point: bool,
    sleep_scan: u16,
    sleep_hp: i32,
    sleep_pose: bool,
}
static HUNTERS: Actors<Hunter> = Actors::new();

pub(super) fn forget(actor: u32) {
    HUNTERS.with(|all| {
        all.remove(&actor);
    });
}

pub(super) fn target_position(update: MonsterUpdate, state: MonsterState) -> Option<(f32, f32)> {
    update
        .quest()
        .player_position_in_area(update.monster().area(), state.target_player())
        .map(|[x, _, z]| (x, z))
}
fn target(update: MonsterUpdate, state: MonsterState) -> Option<Bearing> {
    target_position(update, state).map(|p| state.bearing_to(p))
}
// GU's native area-node resource: same selection predicates as B6DA0.
fn patrol(update: MonsterUpdate, state: MonsterState, rng: u16) -> Option<(f32, f32)> {
    let points = update.patrol_points();
    if points.is_empty() {
        return None;
    }
    let mut roll = rng;
    for _ in 0..4 {
        let [x, _, z] = points[roll as usize % points.len()];
        let p = (x, z);
        if state.bearing_to(p).distance > 500. {
            return Some(p);
        }
        roll = update.quest().random_u32() as u16;
    }
    None
}

pub(super) enum ActionOutcome {
    UseOriginal,
    Handled,
}

pub(super) fn update_action(update: MonsterUpdate) -> ActionOutcome {
    let monster = update.monster();
    let Some(directory) = Directory::read(monster) else {
        return ActionOutcome::UseOriginal;
    };
    let Some(state) = monster.state() else {
        return ActionOutcome::UseOriginal;
    };
    let hp = state.hp();
    if hp <= 0 {
        return ActionOutcome::UseOriginal;
    }

    let dispatch = HUNTERS.with(|all| {
        all.entry(monster.as_raw())
            .or_insert_with(|| Hunter::new(hp))
            .advance(update, state, hp, &directory)
    });
    if let Some(dispatch) = dispatch {
        dispatch.apply(update, state);
    }
    // Waiting for an animation is still a handled update.
    ActionOutcome::Handled
}

impl Dispatch {
    fn apply(self, update: MonsterUpdate, state: MonsterState) {
        let attack = match self {
            Self::Play {
                motion,
                blend,
                attack,
            } => {
                state.mark_action_started();
                if !update.play_motion(motion, f32::from(blend)) {
                    return;
                }
                attack
            }
            Self::Rearm(id) => Some(id),
        };
        if let Some(id) = attack {
            update.arm_primary_attack(id);
        }
    }
}

impl Hunter {
    fn new(hp: i32) -> Self {
        Self {
            spawn: Spawn::new(hp, false),
            pending: Pending::default(),
            playing: None,
            missing: None,
            fractional_ticks: 0.,
            random: 0,
            turn: None,
            ground: None,
            air: None,
            waypoint: None,
            walking_to_point: false,
            sleep_scan: 0,
            sleep_hp: hp,
            sleep_pose: false,
        }
    }

    /// Choose an idle pose while asleep. The side draw is only needed after damage
    /// without a nearby hunter; the chance draw follows once a pose is available.
    fn sleeping_pose(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        hp: i32,
        ticks: u16,
        ended: bool,
    ) -> Option<Request> {
        if self.sleep_pose {
            if !ended {
                return None;
            }
            self.sleep_pose = false;
            self.sleep_scan = 0;
            return self.spawn.phase.motion().map(|m| Request(255, m));
        }
        if self.sleep_scan >= ticks {
            self.sleep_scan -= ticks;
            return None;
        }
        self.sleep_scan = 60;
        let quest = update.quest();
        let area = update.monster().area();
        let side = (0..4).find_map(|i| {
            let [x, _, z] = quest.player_position_in_area(area, i)?;
            let Bearing { distance, yaw } = state.bearing_to((x, z));
            if !(distance > 100. && distance < 1000.) {
                return None;
            }
            let angle = state.yaw().delta_to(yaw).unsigned_abs();
            Some(if angle >= 0x4000 { 41 } else { 42 })
        });
        let chipped = hp < self.sleep_hp;
        self.sleep_hp = hp;
        let id = match side {
            Some(id) => id,
            None if chipped => {
                if quest.random_u32() & 1 != 0 {
                    41
                } else {
                    42
                }
            }
            None => return None,
        };
        if quest.random_u32() % 100 > if chipped { 50 } else { 25 } {
            return None;
        }
        self.sleep_pose = true;
        Some(Request(0, id))
    }

    fn advance(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        hp: i32,
        d: &Directory,
    ) -> Option<Dispatch> {
        if self.missing.is_some() {
            return None;
        }
        let monster = update.monster();
        let current = monster.motion();
        let ended = monster.motion_finished();
        let dt = monster.frame_delta();
        let reentry = state.action_stage() == 0;
        self.fractional_ticks += dt * 0.5;
        let ticks = self.fractional_ticks as u16;
        self.fractional_ticks -= f32::from(ticks);
        let finished = ended && self.playing == Some(current);
        let request = match self.spawn.advance(hp, ticks, finished, false) {
            Step::Play(m) => {
                self.sleep_pose = false;
                Request(255, m)
            }
            Step::Keep if reentry && self.playing != Some(current) => {
                Request(255, self.spawn.phase.motion()?)
            }
            Step::Keep if self.spawn.phase == SpawnPhase::Sleeping => {
                self.sleeping_pose(update, state, hp, ticks, ended)?
            }
            Step::Keep => return None,
            Step::Think => {
                let phase = if let Some(a) = &mut self.air {
                    if self.playing != Some(current) {
                        self.air = None;
                        self.pending.clear();
                        None
                    } else {
                        match unsafe {
                            a.tick(d, update, state, monster.motion_frame(), ended, ticks)
                        } {
                            air::Next::Wait => return None,
                            air::Next::Play(r) => Some(r),
                            air::Next::Think => {
                                self.air = None;
                                self.playing = None;
                                None
                            }
                        }
                    }
                } else if let Some(g) = &mut self.ground {
                    if self.playing != Some(current) {
                        self.ground = None;
                        self.pending.clear();
                        self.waypoint = None;
                        self.walking_to_point = false;
                        self.turn = None;
                        None
                    } else {
                        match unsafe {
                            g.update(d, state, monster.motion_frame(), ended, dt, ticks)
                        } {
                            ground::Next::Wait => return None,
                            ground::Next::Rearm(id) => return Some(Dispatch::Rearm(id)),
                            ground::Next::Play(r) => {
                                if matches!(r, Request(252, 108 | 118)) {
                                    self.ground = None;
                                }
                                Some(r)
                            }
                            ground::Next::Think => {
                                self.ground = None;
                                self.playing = None;
                                None
                            }
                        }
                    }
                } else {
                    None
                };
                if let Some(r) = phase {
                    r
                } else {
                    let mut repeat_turn = false;
                    if let Some(goal) = self.turn {
                        let yaw = state.yaw();
                        let diff = yaw.delta_to(goal);
                        // Source 816A36CA rotates only inside the clip's first
                        // 44 source frames. A complete large turn may take another clip.
                        let frame = monster.motion_frame();
                        if self.playing == Some(current) && !ended && (0.0..44.).contains(&frame) {
                            state.turn_toward(goal, (372. * dt) as u16);
                        }
                        if !ended && !reentry {
                            return None;
                        }
                        if diff.unsigned_abs() < 744 {
                            self.turn = None;
                        } else {
                            repeat_turn = true;
                        }
                    } else if self.playing == Some(current) && !ended && !reentry {
                        return None;
                    }
                    if repeat_turn {
                        Request(1, 1)
                    } else if self.walking_to_point {
                        self.walking_to_point = false;
                        Request(1, 0)
                    } else {
                        let routes = if self.pending.is_empty() {
                            let policy = unsafe { d.policy() };
                            if state.anger_mode() == AngerMode::Calm
                                && state.anger_gauge() >= unsafe { u16at(d.physiology(), 0) }
                            {
                                update.enter_rage();
                            }
                            let point = target(update, state);
                            let angle =
                                point.map_or(0, |b| state.yaw().delta_to(b.yaw).unsigned_abs());
                            let random = update.quest().random_u32() as u16;
                            self.random = random;
                            self.pending.select(
                                &policy,
                                state.anger_mode() == AngerMode::Enraged,
                                state.posture().is_airborne(),
                                point.map(|b| b.distance),
                                angle,
                                random,
                            );
                            policy.routes
                        } else {
                            unsafe { d.routes() }
                        };
                        self.pending.next(routes)?
                    }
                }
            }
        };
        let request = if request == Request(0, 9) && self.random & 1 != 0 {
            Request(254, 9)
        } else {
            request
        };
        let request = if request == Request(1, 11) {
            let point = patrol(update, state, self.random);
            if point.is_none() || state.anger_mode() == AngerMode::Enraged {
                self.playing = None;
                return None; // native 1:11 completes when no usable area point exists
            }
            self.waypoint = point;
            self.walking_to_point = true;
            Request(1, 1)
        } else if request == Request(1, 0) && hp as f32 <= state.max_hp() as f32 * 0.2 {
            Request(251, 10)
        } else if request == Request(3, 13) && self.random & 1 != 0 {
            Request(3, 14)
        } else {
            request
        };
        let request = if request == Request(1, 1) {
            let goal = if let Some(yaw) = self.turn {
                yaw
            } else {
                self.waypoint
                    .map(|p| state.bearing_to(p).yaw)
                    .or_else(|| target(update, state).map(|b| b.yaw))?
            };
            self.turn = Some(goal);
            let delta = state.yaw().delta_to(goal);
            if delta.unsigned_abs() <= 3641 {
                Request(1, 1)
            } else if delta < 0 {
                Request(253, 5)
            } else {
                Request(253, 6)
            }
        } else {
            request
        };
        let Some(e) = (unsafe { d.lookup(request) }) else {
            self.missing = Some(request);
            return None;
        };
        if self.ground.is_none() {
            self.ground = ground::Ground::start(
                update,
                state,
                request,
                self.random,
                if matches!(request, Request(1, 0) | Request(251, 10)) {
                    self.waypoint.take()
                } else {
                    None
                },
            );
        }
        if let Some(g) = &mut self.ground {
            g.bind(e);
        }
        if self.air.is_none() {
            self.air = air::Air::new(request);
        }
        if let Some(a) = &mut self.air {
            a.bind(e, request);
        }
        let (motion, blend) = unsafe { Directory::playback(e) };
        self.playing = Some(motion);
        let n = unsafe { byte(e, 6) };
        Some(Dispatch::Play {
            motion,
            blend,
            attack: (n != 0).then(|| row_id(u32::from(unsafe { u16at(e, 4) }))),
        })
    }
}
