//! Runtime for the single combined hunt resource (ESH5). ESP1 replays never enter here.
use super::{
    Actors, air, byte, ground,
    package::{Directory, row_id},
    u16at, u32at,
};
use espinas_behavior::{Pending, Request, Spawn, SpawnPhase, Step};
use mhgu_sdk::{
    geometry::{Angle, Bearing},
    monster::{MonsterState, MonsterUpdate, anger::AngerMode, motion::MotionId},
};

#[derive(Clone, Copy)]
enum Dispatch {
    Play {
        motion: MotionId,
        blend: u8,
        attack: Option<u32>,
        is_attack: bool,
    },
    Rearm(u32),
}

struct Hunter {
    spawn: Spawn,
    pending: Pending,
    playing: Option<MotionId>,
    waiting: Option<(MotionId, Dispatch)>,
    missing: Option<Request>,
    previous_frame: f32,
    fractional_ticks: f32,
    rest_frames: Option<f32>,
    followup: Option<Request>,
    selection_roll: u16,
    is_attack: bool,
    roar_after_landing: bool,
    turn: Option<Turn>,
    ground: Option<ground::Ground>,
    air: Option<air::Air>,
    waypoint: Option<(f32, f32)>,
    walking_to_point: bool,
    waiting_for_hunter: bool,
    sleep_scan: u16,
    breath_ticks: u16,
    sleep_hp: i32,
    sleep_pose: bool,
}
static HUNTERS: Actors<Hunter> = Actors::new();

struct Turn {
    goal: Angle,
    enraged: bool,
    window: f32,
}

impl Turn {
    // FZ 826B92D4: calm and enraged turns have different angle cutoffs,
    // motions and rotation windows. Latch the profile for this action.
    fn request(&self, yaw: Angle) -> Request {
        let delta = yaw.delta_to(self.goal);
        match (self.enraged, delta.unsigned_abs(), delta < 0) {
            (false, 0..=3641, _) => Request(1, 1),
            (false, _, true) => Request(253, 5),
            (false, _, false) => Request(253, 6),
            (true, 0..=8192, _) => Request(253, 2),
            (true, _, true) => Request(253, 7),
            (true, _, false) => Request(253, 8),
        }
    }
}

pub(super) fn forget(actor: u32) {
    HUNTERS.remove(actor);
}

// Bind the chosen slot as well as reading its coordinates: the native action
// dispatcher also consults the primary target. The GU initializer leaves it unset.
fn acquire(update: MonsterUpdate, state: MonsterState) -> Option<(u8, (f32, f32))> {
    let area = update.monster().area();
    let position = |slot| {
        update
            .quest()
            .player_position_in_area(area, slot)
            .map(|[x, _, z]| (slot, (x, z)))
    };
    if let Some(slot) = state.target_player()
        && let Some([x, _, z]) = update.engage_hunter(slot)
    {
        return Some((slot, (x, z)));
    }
    let nearest = (0..4).filter_map(position).min_by(|(_, a), (_, b)| {
        state
            .bearing_to(*a)
            .distance
            .total_cmp(&state.bearing_to(*b).distance)
    });
    if let Some((slot, _)) = nearest {
        let [x, _, z] = update.engage_hunter(slot)?;
        Some((slot, (x, z)))
    } else {
        update.disengage_hunter();
        None
    }
}
pub(super) fn target_position(update: MonsterUpdate, state: MonsterState) -> Option<(f32, f32)> {
    let slot = state.target_player()?;
    let [x, _, z] = update
        .quest()
        .player_position_in_area(update.monster().area(), slot)?;
    Some((x, z))
}
fn turn_goal(
    waypoint: Option<(f32, f32)>,
    update: MonsterUpdate,
    state: MonsterState,
) -> Option<Angle> {
    let point = waypoint
        .or_else(|| target_position(update, state))
        .or_else(|| acquire(update, state).map(|(_, point)| point))?;
    Some(state.bearing_to(point).yaw)
}

// 81CD3E9A (1:11) chooses from the species' route for this area. With
// no entry it completes immediately; GU stage markers are not that route.
fn patrol(update: MonsterUpdate, state: MonsterState) -> Option<(f32, f32)> {
    let route = update.monster().patrol_route(0)?;
    if route.is_empty() || state.anger_mode() == AngerMode::Enraged {
        return None;
    }
    // FZ refreshes the actor roll on dispatch (816991EC). 1:11 reuses
    // that same roll even on its near-point retries, so it never resamples.
    let index = update.quest().random_u32() as usize % route.len();
    let [x, _, z] = route.point(index)?;
    (state.bearing_to((x, z)).distance > 500.).then_some((x, z))
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

    let (dispatch, retrying, breathe) = HUNTERS.with(|all| {
        let hunter = all
            .entry(monster.as_raw())
            .or_insert_with(|| Hunter::new(hp));
        hunter.check_interruption(monster.motion());
        if hunter.playing == Some(monster.motion()) {
            monster.set_base_motion_rate(motion_rate(state, &directory, hunter.is_attack));
        }
        let retrying = hunter.waiting.is_some();
        let previous_breath = hunter.breath_ticks;
        let dispatch = hunter
            .waiting
            .take()
            .map(|(_, dispatch)| dispatch)
            .or_else(|| hunter.advance(update, state, hp, &directory));
        (
            dispatch,
            retrying,
            hunter.breath_ticks < previous_breath && hunter.spawn.phase == SpawnPhase::Sleeping,
        )
    });
    if breathe {
        sleep_breath(update, unsafe { byte(directory.row(20), 29) });
    }
    if let Some(dispatch) = dispatch
        && !dispatch.apply(update, state, &directory)
    {
        if !retrying && let Dispatch::Play { motion, .. } = dispatch {
            forge::log::prelude::warn!(
                "Espinas: waiting for motion {} (current {})",
                motion.raw(),
                monster.motion().raw()
            );
        }
        HUNTERS.with(|all| {
            if let Some(hunter) = all.get_mut(&monster.as_raw()) {
                hunter.waiting = Some((monster.motion(), dispatch));
            }
        });
    }
    // Waiting for an animation is still a handled update.
    ActionOutcome::Handled
}

// FZ 81CC52C6 overrides only group 3 at the species limp threshold.
fn motion_rate(state: MonsterState, directory: &Directory, attack: bool) -> f32 {
    if attack && state.max_hp() > 0 && state.hp() <= state.max_hp() / 5 {
        1.5
    } else if state.anger_mode() == AngerMode::Enraged {
        unsafe { directory.enraged_motion_rate() }
    } else {
        1.
    }
}

impl Dispatch {
    fn apply(self, update: MonsterUpdate, state: MonsterState, directory: &Directory) -> bool {
        let attack = match self {
            Self::Play {
                motion,
                blend,
                attack,
                is_attack,
            } => {
                let blend_frames = if blend == 0 {
                    0.
                } else {
                    2. * (f32::from(blend) + 1.)
                };
                if !update.play_motion(motion, blend_frames) {
                    return false;
                }
                super::motion::begin(update.monster());
                update
                    .monster()
                    .set_base_motion_rate(motion_rate(state, directory, is_attack));
                update.note_action_progress();
                state.mark_action_started();
                HUNTERS.with(|all| {
                    if let Some(hunter) = all.get_mut(&update.monster().as_raw()) {
                        hunter.playing = Some(motion);
                        hunter.is_attack = is_attack;
                        hunter.previous_frame = 0.;
                    }
                });
                attack
            }
            Self::Rearm(id) => Some(id),
        };
        if let Some(id) = attack {
            update.arm_primary_attack(id);
        }
        true
    }
}

impl Hunter {
    fn new(hp: i32) -> Self {
        Self {
            spawn: Spawn::new(hp, false),
            pending: Pending::default(),
            playing: None,
            waiting: None,
            missing: None,
            previous_frame: 0.,
            fractional_ticks: 0.,
            rest_frames: None,
            followup: None,
            selection_roll: 0,
            is_attack: false,
            roar_after_landing: false,
            turn: None,
            ground: None,
            air: None,
            waypoint: None,
            walking_to_point: false,
            waiting_for_hunter: false,
            sleep_scan: 0,
            breath_ticks: 0,
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

    fn next_request(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        directory: &Directory,
        motion_running: bool,
        dt: f32,
    ) -> Option<Request> {
        if let Some(left) = &mut self.rest_frames {
            // 81CC604A starts a four-tick thought gate. 816A135C tests
            // before decrementing: only the fifth FZ tick may resume the script.
            *left -= dt;
            if *left > 0. {
                return None;
            }
            self.rest_frames = None;
            self.playing = None;
            return self.continue_route(update, state, directory);
        }
        // 816A36CA turns toward the live target even if the hunter crosses
        // the monster while the clip plays.
        if let Some(turn) = &mut self.turn {
            if let Some(goal) = turn_goal(self.waypoint, update, state) {
                turn.goal = goal;
                let per_tick = (32768. / turn.window * motion_rate(state, directory, false)) as u16;
                let remaining = state.yaw().delta_to(goal).unsigned_abs();
                let frame = update.monster().motion_frame();
                if motion_running {
                    if super::motion::advance(update.monster()) > 0.
                        && (0.0..turn.window).contains(&frame)
                    {
                        state.turn_toward(goal, (f32::from(per_tick) * dt * 0.5) as u16);
                    }
                    return None;
                }
                if remaining >= per_tick {
                    return Some(Request(1, 1));
                }
            } else {
                self.pending.clear();
            }
            self.turn = None;
        }
        if motion_running {
            return None;
        }
        if state.posture().is_airborne() {
            return self.continue_route(update, state, directory);
        }
        self.rest_frames = Some(10.);
        Some(if state.anger_mode() == AngerMode::Enraged {
            Request(0, 17)
        } else {
            Request(0, 1)
        })
    }

    fn continue_route(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        directory: &Directory,
    ) -> Option<Request> {
        if core::mem::take(&mut self.walking_to_point) {
            return Some(Request(1, 0));
        }
        self.select_request(update, state, directory)
    }

    fn rage_event(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        directory: &Directory,
    ) -> Option<Request> {
        let threshold = unsafe { directory.anger_threshold() };
        if state.anger_mode() == AngerMode::Calm && state.anger_gauge() >= threshold {
            update.enter_rage();
        }
        // FZ 816A5474 dispatches the rage event (11:0) ahead of battle 0:3.
        if !super::physiology::take_rage_entry(update.monster().as_raw()) {
            return None;
        }
        self.cancel_route();
        self.waiting_for_hunter = false;
        self.roar_after_landing = state.posture().is_airborne();
        Some(if self.roar_after_landing {
            Request(2, 7)
        } else {
            Request(3, 3)
        })
    }

    fn select_request(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        directory: &Directory,
    ) -> Option<Request> {
        let monster = update.monster();
        let selected = acquire(update, state);
        let point = selected.map(|(_, p)| state.bearing_to(p));
        if let Some(request) = self.rage_event(update, state, directory) {
            return Some(request);
        }
        if self.roar_after_landing {
            if state.anger_mode() != AngerMode::Enraged {
                self.roar_after_landing = false;
            } else if state.posture().is_airborne() {
                return Some(Request(2, 1));
            } else {
                self.roar_after_landing = false;
                return Some(Request(3, 3));
            }
        }
        if point.is_none() && !state.posture().is_airborne() {
            self.pending.clear();
            self.waypoint = None;
            self.turn = None;
            if !self.waiting_for_hunter {
                forge::log::prelude::info!("Espinas: no hunter in area {}", monster.area());
            }
            self.waiting_for_hunter = true;
            return Some(Request(255, 1));
        }
        if !self.pending.is_empty() {
            return self.pending.next(unsafe { directory.routes() });
        }
        let policy = unsafe { directory.policy() };
        let angle = point.map_or(0, |b| state.yaw().delta_to(b.yaw).unsigned_abs());
        let random = update.quest().random_u32() as u16;
        self.selection_roll = random;
        forge::log::prelude::info!(
            "Espinas think: area={} hunter={:?} range={} turn={} rage={:?} gauge={}/{} roll={} pos={:?} target={:?} yaw={}",
            monster.area(),
            selected.map(|(slot, _)| slot),
            point.map_or(-1, |b| b.distance as i32),
            angle,
            state.anger_mode(),
            state.anger_gauge(),
            unsafe { directory.anger_threshold() },
            random & 31,
            state.position().map(|v| v as i32),
            selected.map(|(_, (x, z))| (x as i32, z as i32)),
            state.yaw().units()
        );
        self.pending.select(
            &policy,
            state.anger_mode() == AngerMode::Enraged,
            state.posture().is_airborne(),
            point.map(|b| b.distance),
            angle,
            random,
        );
        self.pending.next(policy.routes)
    }

    fn cancel_route(&mut self) {
        self.rest_frames = None;
        self.followup = None;
        self.pending.clear();
        self.turn = None;
        self.waypoint = None;
        self.walking_to_point = false;
        self.ground = None;
        self.air = None;
    }

    // A detection notification can re-enter the same GU action with the motion
    // intact. A different motion means GU took ownership (e.g. damage); a turn,
    // patrol or queued attack from before that interruption no longer applies.
    fn check_interruption(&mut self, current: MotionId) {
        let expected = self.waiting.map(|(motion, _)| motion).or(self.playing);
        if expected.is_some_and(|motion| motion != current) {
            self.playing = None;
            self.waiting = None;
            self.cancel_route();
        }
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
        // 76154 resets the GU action stage even when perception reselects 1:0
        // without replacing the ongoing motion. Resume its frame window and route.
        if state.action_stage() == 0 && self.playing == Some(current) {
            state.mark_action_started();
        }
        let frame = monster.motion_frame();
        // Sleep and targetless idle can legitimately outlast GU's action timeout.
        // A stalled clip never wraps and still gets the engine's normal recovery.
        if self.playing == Some(current) && frame < self.previous_frame {
            update.note_action_progress();
        }
        self.previous_frame = frame;
        if self.waiting_for_hunter {
            if acquire(update, state).is_some() {
                self.waiting_for_hunter = false;
                self.playing = None;
            } else if self.playing == Some(current) {
                return None;
            }
        }
        let dt = monster.frame_delta();
        self.fractional_ticks += dt * 0.5;
        let ticks = self.fractional_ticks as u16;
        self.fractional_ticks -= f32::from(ticks);
        // 81CD8D14 emits request 87 every 135 logic ticks, even when a tagged
        // sleeping pose interrupts the 34 loop. Keep the clock off the animation.
        self.breath_ticks = ((u32::from(self.breath_ticks) + u32::from(ticks)) % 135) as u16;
        let finished = ended && self.playing == Some(current);
        // FZ 0:40 and 0:43 run 81CC60D6(1) through the last rising tick.
        // Hits in 0:25 (AfterRise) already count toward the first rage.
        if matches!(self.spawn.phase, SpawnPhase::Sleeping | SpawnPhase::Rising)
            && state.anger_gauge() != 0
        {
            state.set_anger_gauge(0);
        }
        let request = match self.spawn.advance(hp, ticks, finished, false) {
            Step::Play(m) => {
                self.sleep_pose = false;
                Request(255, m)
            }
            Step::Keep if self.playing != Some(current) => {
                self.sleep_pose = false;
                Request(255, self.spawn.phase.motion()?)
            }
            Step::Keep if self.spawn.phase == SpawnPhase::Sleeping => {
                self.sleeping_pose(update, state, hp, ticks, ended)?
            }
            Step::Keep => return None,
            Step::Think => {
                if state.posture().is_airborne()
                    && let Some(r) = self.rage_event(update, state, d)
                {
                    return self.play_request(update, state, hp, d, r);
                }
                if finished && let Some(next) = self.followup.take() {
                    return self.play_request(update, state, hp, d, next);
                }
                let phase = if let Some(a) = &mut self.air {
                    match unsafe { a.tick(d, update, state, frame, ended, dt) } {
                        air::Next::Wait => return None,
                        air::Next::Play(r) => Some(r),
                        air::Next::Think => {
                            self.air = None;
                            self.playing = None;
                            None
                        }
                    }
                } else if let Some(g) = &mut self.ground {
                    match unsafe {
                        g.update(
                            d,
                            state,
                            frame,
                            ended,
                            dt,
                            ticks,
                            monster.motion_rate(),
                            monster.motion_blend_remaining() > 0.,
                        )
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
                } else {
                    None
                };
                match phase {
                    Some(request) => request,
                    None => self.next_request(
                        update,
                        state,
                        d,
                        self.playing == Some(current) && !ended,
                        dt,
                    )?,
                }
            }
        };
        self.play_request(update, state, hp, d, request)
    }

    fn play_request(
        &mut self,
        update: MonsterUpdate,
        state: MonsterState,
        hp: i32,
        d: &Directory,
        request: Request,
    ) -> Option<Dispatch> {
        let monster = update.monster();
        let request = match request {
            Request(0, 9) if update.quest().random_u32() & 1 != 0 => Request(254, 9),
            Request(1, 11) => {
                let Some(point) = patrol(update, state) else {
                    forge::log::prelude::info!("Espinas patrol: area={} skipped", monster.area());
                    self.playing = None;
                    return None; // native 1:11 completes when no usable area point exists
                };
                forge::log::prelude::info!(
                    "Espinas patrol: area={} point=({}, {})",
                    monster.area(),
                    point.0 as i32,
                    point.1 as i32
                );
                self.waypoint = Some(point);
                self.walking_to_point = true;
                Request(1, 1)
            }
            Request(1, 0) if hp as f32 <= state.max_hp() as f32 * 0.2 => Request(251, 10),
            // 81CE4220 resolves the first tail direction *before* dispatch refreshes the actor roll.
            Request(3, 13) if self.ground.is_none() && self.selection_roll & 1 != 0 => {
                Request(3, 14)
            }
            _ => request,
        };
        let request = if request == Request(1, 1) {
            if self.turn.is_none() {
                let goal = turn_goal(self.waypoint, update, state)?;
                self.turn = Some(Turn {
                    goal,
                    enraged: state.anger_mode() == AngerMode::Enraged,
                    window: 44.,
                });
            }
            self.turn.as_ref()?.request(state.yaw())
        } else {
            request
        };
        let Some(e) = (unsafe { d.lookup(request) }) else {
            forge::log::prelude::warn!("Espinas: missing request {}:{}", request.0, request.1);
            self.missing = Some(request);
            return None;
        };
        if let Some(turn) = &mut self.turn
            && turn.enraged
        {
            // GU stores the number of samples; Vita 81007FF2 returns the
            // last source frame (the baker includes the sample at frame zero).
            turn.window = unsafe { u32at(e, 16) }.saturating_sub(1).max(1) as f32;
        }
        if self.ground.is_none() {
            self.ground = ground::Ground::start(
                update,
                state,
                request,
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
        let (motion, _) = unsafe { Directory::playback(e) };
        let blend = unsafe { d.blend(e, monster.motion()) };
        self.followup = unsafe {
            let group = byte(e, 30);
            (group != 0).then(|| Request(group, byte(e, 31)))
        };
        forge::log::prelude::info!(
            "Espinas: phase={:?} request={}:{} hp={} pos={:?} yaw={}",
            self.spawn.phase,
            request.0,
            request.1,
            hp,
            state.position().map(|v| v as i32),
            state.yaw().units()
        );
        let n = unsafe { byte(e, 6) };
        Some(Dispatch::Play {
            motion,
            blend,
            is_attack: matches!(
                request,
                Request(3 | 248 | 250, _) | Request(252, 57 | 108 | 115 | 116 | 118)
            ),
            attack: (n != 0).then(|| row_id(u32::from(unsafe { u16at(e, 4) }))),
        })
    }
}

/// GU 4EFC70 queues an actor-positioned request from the bound slot-zero bank.
fn sleep_breath(update: MonsterUpdate, joint: u8) {
    let base = forge::mem::text_addr();
    unsafe {
        let manager = super::u32at(base, 0x1896f5c);
        if manager != 0 {
            let actor = update.monster().as_raw();
            let play: unsafe extern "C" fn(u32, u32, u32, i32, u32) =
                core::mem::transmute(base + 0x4efc70);
            play(
                manager,
                4096 + 87,
                actor,
                i32::from(joint),
                u32::from(super::byte(actor, 4180)),
            );
        }
    }
}
