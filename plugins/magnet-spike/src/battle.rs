use crate::{
    bind::BindInput,
    combat,
    flight::{AxisMotion, Flight},
    fx,
    magnet::{self, AimedMark, MarkRange},
    motion, program, sound,
};
use mhfz_sdk::{
    action::Cursor,
    effects::MotionClock,
    sprint::{Flick, STICK_DEADZONE},
};
use mhgu_sdk::{
    combat::collision::attack::AttackScene,
    geometry::Bearing,
    sync::Shared,
    weapon::style::{
        Input, PASS, action, guard, hunter,
        input::{self, Pad, PadButton},
        movement,
    },
};

const EVADE: u16 = 0;
const X: u16 = 2;
const A: u16 = 3;
const CHORD: u16 = 4;
const CHANGE_MODE: u16 = 5;
const STEP_FORWARD: u16 = 7;
const STEP_RIGHT: u16 = 8;
const MAGNET_ATTACK: u16 = 9;
const STEP_BACK: u16 = 10;
const BOOST: u16 = 11;
const GUARD: u16 = 13;
const STEP_LEFT: u16 = 17;
const SHOT: u16 = 19;
const GUARD_FOLLOW_UP: u16 = 20;
const REPEL: u16 = 23;
const ATTRACT: u16 = 24;
const RESERVED: u32 = (1 << REPEL)
    | (1 << ATTRACT)
    | (1 << SHOT)
    | (1 << STEP_FORWARD)
    | (1 << STEP_RIGHT)
    | (1 << STEP_LEFT)
    | (1 << STEP_BACK)
    | (1 << MAGNET_ATTACK)
    | (1 << BOOST)
    | (1 << GUARD);

fn command_mask<const N: usize>(inputs: [(u16, bool); N]) -> u32 {
    inputs
        .into_iter()
        .fold(0, |bits, (id, down)| bits | (u32::from(down) << id))
}

unsafe fn commands(
    actor: u32,
    input: &Input,
    sprint: bool,
    charging: bool,
    stroke: Option<crate::controls::Stroke>,
) -> u32 {
    unsafe {
        let pad = Pad::sample(actor);
        let held =
            |key: PadButton| stroke.map_or_else(|| pad.held(key), |s| s.held & key as u16 != 0);
        let pressed = |key: PadButton| {
            stroke.map_or_else(|| pad.pressed(key), |s| s.pressed & key as u16 != 0)
        };
        let drawn = action::weapon_drawn(actor);
        if stroke.is_none() && drawn && held(PadButton::R) {
            input::reserve_dash_shoulder(actor);
        }
        let x_and_a = held(PadButton::X)
            && held(PadButton::A)
            && (pressed(PadButton::X) || pressed(PadButton::A));
        let independent = command_mask([
            (15, charging && !pad.held(PadButton::A)),
            (GUARD, drawn && pressed(PadButton::Zr)),
        ]);
        if drawn && held(PadButton::Zl) {
            return independent
                | command_mask([
                    (SHOT, pressed(PadButton::A)),
                    (ATTRACT, pressed(PadButton::X)),
                    (REPEL, pressed(PadButton::B)),
                ]);
        }
        let shoulder = held(PadButton::R);
        let special =
            input.command(8) || stroke.is_some_and(|s| s.pressed & crate::controls::SPECIAL != 0);
        if shoulder && x_and_a && !drawn {
            return 1 << MAGNET_ATTACK;
        }
        if shoulder && drawn {
            // Feed every command in this input sample to the current action's windows.
            return independent
                | command_mask([
                    (BOOST, special),
                    (MAGNET_ATTACK, x_and_a),
                    (STEP_FORWARD, !x_and_a && pressed(PadButton::X)),
                    (STEP_RIGHT, !x_and_a && pressed(PadButton::A)),
                    (STEP_LEFT, pressed(PadButton::Y)),
                    (STEP_BACK, pressed(PadButton::B)),
                ]);
        }
        // Keep GU's filtered face buttons and common movement outside the chords.
        let buttons = stroke.map_or_else(
            || input::attack_buttons_pressed(actor),
            |s| s.pressed & 0x120,
        );
        let chord = x_and_a
            || buttons == 0x120
            || stroke.is_none()
                && (input::attack_chord_pressed(actor) || input::attack_chord_with_shoulder(actor));
        let x = !chord && buttons & 0x20 != 0;
        let a = !chord && buttons & 0x100 != 0;
        let evade = stroke.map_or(input.command(3), |_| pressed(PadButton::B))
            && movement::has_evade_stamina(actor, 150.);
        independent
            | command_mask([
                (X, x),
                (A, a),
                (
                    6,
                    a && stroke.map_or(input.stick_strength, |s| s.strength) > STICK_DEADZONE,
                ),
                (1, drawn && sprint),
                (CHORD, chord),
                (CHANGE_MODE, special),
                (EVADE, evade),
            ])
    }
}

fn branch_available(actor: u32, condition: u16, guard_hits: u8, evaded: bool) -> bool {
    match condition {
        0 => true,
        1..=5 => {
            (unsafe { movement::has_stamina(actor) }) && magnet::can_spend(actor, condition as u8)
        }
        6 | 8 => guard_hits != 0,
        10 | 11 => {
            let range = if condition == 10 {
                MarkRange::Attract
            } else {
                MarkRange::Repel
            };
            (unsafe { movement::has_stamina(actor) })
                && magnet::can_spend(actor, 1)
                && unsafe { range.aim(actor) }.is_some()
        }
        12 => evaded,
        _ => false,
    }
}

fn mark_range(state: u8) -> Option<MarkRange> {
    if program::is_attraction(state) {
        Some(MarkRange::Attract)
    } else if matches!(state, 66 | 67) {
        Some(MarkRange::Repel)
    } else {
        None
    }
}

#[derive(Default)]
enum Evade {
    #[default]
    Inactive,
    Open(f32),
    Confirmed,
}

#[derive(Default)]
pub(super) struct Charge {
    frames: Option<f32>,
    charged: bool,
}

impl Charge {
    const NEW: Self = Self {
        frames: None,
        charged: false,
    };

    pub(super) unsafe fn start(&mut self, actor: u32, local: bool) {
        if self.frames.is_some() {
            return;
        }
        self.frames = Some(0.);
        if local {
            unsafe {
                fx::charge(actor, 18);
                sound::bind_caster(actor, 6);
            }
        }
    }

    pub(super) unsafe fn update(&mut self, actor: u32, elapsed: f32, local: bool) -> bool {
        if let Some(frames) = &mut self.frames {
            let previous = (*frames / 2.) as u32;
            *frames += elapsed;
            let until = (*frames / 2.) as u32;
            if previous != until {
                let skill = unsafe { action::ChargeSkill::equipped(actor) };
                let rate = match skill {
                    action::ChargeSkill::Focus => 0.8,
                    action::ChargeSkill::Slow => 1.2,
                    action::ChargeSkill::Normal => 1.,
                };
                let threshold = (f32::from(program::parameter(33)) * rate) as u32;
                // 81221FC0 compares the counter before incrementing it.
                if !self.charged && until > threshold {
                    self.charged = true;
                    unsafe {
                        if local {
                            fx::charge(actor, 17);
                        }
                        crate::native_media::charge_sparks(actor);
                        sound::charge(actor);
                    }
                }
                if until > threshold + u32::from(program::parameter(34)) {
                    self.charged = program::parameter(35) != 0;
                    if local && !self.charged {
                        unsafe { fx::charge(actor, 14) };
                    }
                    self.frames = None;
                    return true;
                }
            }
        }
        false
    }
}

#[derive(Default)]
struct StateTimers {
    poise: bool,
    charge: Charge,
    sprint_frames: f32,
}

impl StateTimers {
    const fn new() -> Self {
        Self {
            poise: false,
            charge: Charge::NEW,
            sprint_frames: 0.,
        }
    }

    unsafe fn update(&mut self, actor: u32, state: u8, sample: action::MotionFrame) -> Option<u16> {
        let elapsed = unsafe { ((actor + 28) as *const f32).read() }.max(0.);
        if self.poise {
            let timer = (actor + 10288) as *mut f32;
            unsafe { timer.write(timer.read().max(8.)) };
        }
        if unsafe { self.charge.update(actor, elapsed, true) } {
            return Some(208);
        }
        if matches!(state, 40 | 41 | 44 | 45) {
            let base = if state < 44 { 3 } else { 6 };
            let previous = (self.sprint_frames * 0.5) as u32;
            self.sprint_frames += elapsed;
            let until = (self.sprint_frames * 0.5) as u32;
            if sample.blend <= 0. && sample.frame >= 16. {
                unsafe { movement::drain_running_stamina(actor) };
            }
            let delay = u32::from(program::parameter(base + 1));
            if until >= delay {
                let direction = unsafe { movement::live_relative_stick_yaw(actor) };
                let Some(yaw) = direction.filter(|_| unsafe { movement::has_stamina(actor) })
                else {
                    return Some(if state < 44 { 42 } else { 46 });
                };
                let mut delta = i32::from(yaw.wrapping_neg() as i16);
                let limit = i32::from(program::parameter(base));
                let deadzone = (i32::from(program::parameter(base + 2)) * 65536 + 180) / 360;
                for _ in previous.max(delay - 1)..until {
                    if delta.abs() < deadzone {
                        break;
                    }
                    let step = delta.clamp(-limit, limit);
                    unsafe { movement::turn(actor, step as u16) };
                    delta -= step;
                }
            }
        }
        None
    }
}

#[derive(Default)]
struct Battle {
    roll_pending: u32,
    flick: Option<Flick>,
    timers: StateTimers,
    mode: program::Mode,
    cursor: Option<Cursor>,
    aim: Option<[f32; 3]>,
    flight: Option<Flight>,
    forward: AxisMotion,
    audio: MotionClock,
    particles: MotionClock,
    native: crate::native_media::Playback,
    motion_accent: bool,
    guard_hits: u8,
    evade: Evade,
    bind_engaged: bool,
}

impl Battle {
    // Returns true while the same state has another clip to process.
    unsafe fn play_step(&mut self, actor: u32) -> bool {
        let cursor = self.cursor.as_mut().unwrap();
        let Some(step) = program::step(cursor.state, cursor.time.step) else {
            unsafe { self.finish(actor) };
            return false;
        };
        if step.op == 2 {
            // FZ 81815D60 treats operand 1 as a state for opcode 2.
            unsafe { self.branch(actor, step.clip) };
            return false;
        }
        if unsafe {
            motion::play_step(
                actor,
                cursor,
                step,
                &mut self.audio,
                &mut self.particles,
                &mut self.native,
            )
        }
        .is_none()
        {
            unsafe { self.finish(actor) };
            return false;
        }
        unsafe {
            Self::events(
                actor,
                cursor,
                &mut self.aim,
                self.flight.as_mut(),
                &mut self.forward,
                &mut self.evade,
                &mut self.timers,
            )
        };
        true
    }

    unsafe fn events(
        actor: u32,
        cursor: &mut Cursor,
        aim: &mut Option<[f32; 3]>,
        mut flight: Option<&mut Flight>,
        forward: &mut AxisMotion,
        evade: &mut Evade,
        timers: &mut StateTimers,
    ) {
        let state = cursor.state;
        unsafe {
            let mode = motion::local_pose(actor).unwrap_or_default().mode;
            let bonus = program::power_bonus(magnet::boosts(actor), mode.motion_mode(state));
            combat::events(actor, cursor, bonus, |event, value, late| match event {
                // FZ 81225CCE / 81225CEA set the hurt immunity timer. Event 8
                // adds the equipped Evasion bonus from the w13 parameters.
                7 | 8 => {
                    let bonus = if event == 8 {
                        match hunter::evasion_level(actor) {
                            2.. => program::parameter(10),
                            1 => program::parameter(9),
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    movement::grant_evade_invulnerability(
                        actor,
                        f32::from(value + bonus) * 2. - late,
                    );
                }
                // FZ 81225D6A and 812264FE charge stamina before shortening its waiver.
                11 | 45 => {
                    let (cost, waiver) = if event == 11 {
                        (75, 150)
                    } else {
                        (program::parameter(80), program::parameter(81))
                    };
                    movement::spend_stamina(actor, 2. * f32::from(cost), 2. * f32::from(waiver));
                }
                // FZ 81225D86 selects the shake preset from MSPR[0..=2].
                12 => {
                    let preset = if value <= 2 { usize::from(value) } else { 0 };
                    mhfz_sdk::camera::start(actor, program::parameter(preset));
                }
                4 if value == 1 => {
                    let direction = aim.take().unwrap_or_else(|| magnet::aim(actor));
                    magnet::launch(actor, direction);
                }
                4 if value == 3 && state == 98 => magnet::begin_bind(actor, late),
                39 if matches!(state, 105 | 157) => {
                    if let Some([x, _, z]) = magnet::bind_finisher_target(actor) {
                        let [px, _, pz] = movement::position(actor);
                        let facing = Bearing::between_xz((px, pz), (x, z));
                        if facing.distance > 0. {
                            movement::set_facing(actor, facing.yaw.units());
                        }
                    }
                }
                43 if matches!(state, 105 | 157) => {
                    if magnet::bind_finisher_target(actor).is_some() {
                        combat::bind_finisher(actor, bonus);
                    }
                }
                6 => {
                    timers.poise = true;
                    let timer = (actor + 10288) as *mut f32;
                    timer.write(timer.read().max(8.));
                }
                26 => timers.charge.start(actor, true),
                41 => {
                    movement::steer(actor);
                }
                27 => forward.set_speed(value),
                29 => forward.damped = value == 1,
                37 => *forward = AxisMotion::new(),
                // FZ 81226438 builds family 121 row 11.
                42 => *evade = Evade::Open(f32::from(value) * 2. - late),
                _ => {
                    if let Some(flight) = flight.as_deref_mut() {
                        flight.event(event, value);
                    }
                }
            });
        }
    }

    unsafe fn start(&mut self, actor: u32, id: u8, aim: Option<AimedMark>) -> bool {
        unsafe {
            if let Some(mode) = program::boost_mode(id)
                && !magnet::activate_boost(actor, mode)
            {
                self.finish(actor);
                return true;
            }
            if program::is_bind_drop(id) {
                action::start_weapon_state(actor);
                self.flight = Some(Flight::bind_drop());
            } else if !program::is_attraction(id) && program::flight_landing(id).is_some() {
                let Some(flight) = self.flight.as_mut().filter(|flight| flight.launched()) else {
                    self.finish(actor);
                    return true;
                };
                flight.follow_up(id);
                action::start_airborne_weapon_state(actor);
            } else {
                action::start_weapon_state(actor);
                self.flight = None;
                if let Some(range) = mark_range(id) {
                    let Some(aim) = aim else {
                        self.finish(actor);
                        return true;
                    };
                    match range {
                        MarkRange::Attract => self.flight = Some(Flight::attraction(aim.target)),
                        // FZ 81220328 faces the mark once. GU applies the
                        // backward movement from clip 1429 or 1444's root track.
                        MarkRange::Repel => movement::set_facing(actor, aim.yaw),
                    }
                }
            }
            let cost = program::cost(id);
            if cost != 0 && !magnet::spend(actor, cost) {
                self.finish(actor);
                return true;
            }
            self.timers = StateTimers::new();
            self.mode = self.mode.for_state(id);
            // 8121F010 calls 81220B34 on entry to both shot states.
            self.aim = matches!(id, 82 | 84).then(|| {
                let direction = magnet::aim(actor);
                let bearing = Bearing::between_xz((0., 0.), (direction[0], direction[2]));
                if bearing.distance > 0. {
                    movement::set_facing(actor, bearing.yaw.units());
                }
                direction
            });
            self.forward = AxisMotion::new();
            self.guard_hits = 0;
            self.motion_accent = false;
            self.evade = Evade::Inactive;
            self.bind_engaged = false;
            if matches!(id, 105 | 157) {
                magnet::begin_bind_finisher(actor);
                // 8121EFE6 plays we13 request 36 after either accepted finisher.
                sound::bind_caster(actor, 36);
            }
            motion::set_mode(actor, self.mode);
            self.cursor = Some(program::cursor(id));
            if self.play_step(actor) && matches!(id, 66 | 67) {
                magnet::begin_repel(actor, self.mode);
            }
        }
        true
    }

    unsafe fn finish(&mut self, actor: u32) {
        self.cursor = None;
        self.flight = None;
        unsafe { action::finish_weapon_state(actor) };
    }

    unsafe fn branch(&mut self, actor: u32, target: u16) -> bool {
        unsafe { self.transition(actor, target, false) }
    }

    unsafe fn transition(&mut self, actor: u32, target: u16, combo: bool) -> bool {
        // The miss can reach 100 either in the controller or via opcode 2
        // at the end of 1442. Resolve both, and the finisher, at the same exit.
        let target = if self.cursor.as_ref().is_some_and(|c| c.state == 99)
            && self.mode == program::Mode::Impact
            && matches!(target, 100 | 105)
        {
            if target == 100 { 156 } else { 157 }
        } else if target == 208 {
            if self.timers.charge.charged { 35 } else { 36 }
        } else if target == 207 {
            // FZ 8122570A selects the counter attack row by contact count.
            match self.guard_hits {
                0 => return false,
                1 => 57,
                2 => 76,
                _ => 77,
            }
        } else {
            target
        };
        if matches!(target, 202 | 203) {
            if unsafe { motion::enter_roll(actor, target == 202) } {
                self.roll_pending = 0;
                self.cursor = None;
                return true;
            }
            return false;
        }
        let Ok(id) = u8::try_from(target) else {
            return false;
        };
        if !program::ACTIONS.contains(id) {
            return false;
        }
        if let Some(mode) = program::boost_mode(id)
            && !magnet::boost_ready(actor, mode)
        {
            return false;
        }
        let cost = program::cost(id);
        if cost != 0 && !(unsafe { movement::has_stamina(actor) } && magnet::can_spend(actor, cost))
        {
            return false;
        }
        let aim = if let Some(range) = mark_range(id) {
            let Some(aim) = (unsafe { range.aim(actor) }) else {
                return false;
            };
            Some(aim)
        } else {
            None
        };
        if unsafe { action::accept_custom_start(actor, id) } {
            if combo && let Some(yaw) = unsafe { movement::attack_turn_yaw(actor) } {
                unsafe { movement::turn(actor, program::combo_turn(id, yaw)) };
            }
            return unsafe { self.start(actor, id, aim) };
        }
        false
    }

    unsafe fn advance(
        &mut self,
        actor: u32,
        state: u8,
        commands: u32,
        input: &Input,
        press_time: Option<mhfz_sdk::action::Time>,
    ) -> bool {
        let sample = unsafe { action::MotionFrame::sample(actor) };
        let Some(cursor) = self.cursor.as_mut().filter(|cursor| cursor.state == state) else {
            let aim = mark_range(state).and_then(|range| unsafe { range.aim(actor) });
            return unsafe { self.start(actor, state, aim) };
        };
        if let Some(target) = unsafe { self.timers.update(actor, state, sample) } {
            return unsafe { self.transition(actor, target, target == 208) };
        }
        let Some(step) = program::step(state, cursor.time.step) else {
            unsafe { self.finish(actor) };
            return true;
        };
        if step.op == 2 {
            return unsafe { self.branch(actor, step.clip) };
        }
        use mhfz_sdk::combat::{EvadeClock, EvadeDistance as Travel};
        let value = |i| f32::from(program::parameter(i));
        let side = |i, sign| Travel::side(sign * value(i), value(i + 1), value(i + 2));
        let forward = |i, sign| Travel::forward(sign * value(i), value(i + 1), value(i + 2));
        let travel = match step.clip {
            1006 | 1010 => Some(side(62, 1.)),
            1007 | 1011 => Some(side(62, -1.)),
            1021 => Some(forward(77, 1.)),
            1447 => Some(forward(74, -1.)),
            1464 => Some(forward(68, -1.)),
            1480 => Some(side(71, 1.)),
            1481 => Some(side(71, -1.)),
            1493 => Some(side(65, 1.)),
            1494 => Some(side(65, -1.)),
            _ => None,
        };
        if let Some(travel) = travel {
            unsafe { travel.apply(actor, sample, EvadeClock::WeaponTick) };
        }
        let landing = program::flight_landing(state);
        let launch_clip = matches!(step.clip, 1460 | 1472);
        let in_coast_pose =
            launch_clip && motion::local_pose(actor).is_some_and(|pose| pose.coast == Some(state));
        let clip = if in_coast_pose {
            self.mode.coast_clip()
        } else {
            step.clip
        };
        unsafe { sound::sample(&mut self.audio, actor, clip, sample) };
        unsafe {
            fx::motion(
                &mut self.particles,
                &mut self.motion_accent,
                actor,
                clip,
                sample,
            )
        };
        if !in_coast_pose {
            unsafe {
                self.native.sample(
                    actor,
                    state,
                    clip,
                    sample,
                    self.mode.motion_mode(state),
                    magnet::boosts(actor),
                    None,
                    &mut self.motion_accent,
                )
            };
        }
        let wrapped = if in_coast_pose {
            // The new pose has its own loop. Keep checking the authored
            // action's pending branches and events on its uninterrupted clock.
            let elapsed = unsafe { movement::motion_frames(actor) };
            cursor.time.frame += elapsed.max(0.);
            false
        } else {
            cursor.time.observe(sample.frame, sample.blend * 0.5)
        };
        unsafe {
            Self::events(
                actor,
                cursor,
                &mut self.aim,
                self.flight.as_mut(),
                &mut self.forward,
                &mut self.evade,
                &mut self.timers,
            )
        };
        // FZ 81224140 starts movement at 34 in either flight clip. Launch
        // and its follow-ups stay airborne until GU reports floor contact.
        let drives_flight = if program::is_attraction(state) {
            launch_clip && cursor.time.frame >= 34.
        } else {
            landing.is_some()
        };
        if let (Some(flight), Some(landing)) =
            (self.flight.as_mut().filter(|_| drives_flight), landing)
        {
            if unsafe { flight.advance(actor, state, &mut self.forward) } {
                unsafe { self.branch(actor, u16::from(landing)) };
                return true;
            }
            if launch_clip
                && !in_coast_pose
                && flight.coasting()
                && unsafe { motion::play(actor, self.mode.coast_clip(), 0, 0) }.is_some()
            {
                motion::coast(actor, state);
                self.audio.seek(self.mode.coast_clip(), 0.);
            }
        }
        let evaded = matches!(self.evade, Evade::Confirmed);
        let guard_hits = self.guard_hits;
        let eligible = |c| branch_available(actor, c, guard_hits, evaded);
        let mut finish_after_input = false;
        if (!drives_flight || program::is_bind_drop(state))
            && program::step_complete(step, cursor.time, sample, wrapped)
        {
            // 8121F57C advances to the next authored clip before checking its rows.
            // At the very end, preserve a valid press until GU's chord window closes.
            let mut waiting_in_window = false;
            if program::step(state, cursor.time.step + 1).is_none() {
                if let Some(stroke) = crate::controls::pending(actor)
                    && let Some(time) = stroke.time
                {
                    let waiting =
                        unsafe { self::commands(actor, input, false, false, Some(stroke)) };
                    waiting_in_window = cursor.press_window(waiting, time, eligible);
                }
                finish_after_input =
                    press_time.is_some_and(|t| cursor.press_window(commands, t, eligible));
            }
            if !waiting_in_window && !finish_after_input {
                cursor.time.step += 1;
                if !unsafe { self.play_step(actor) } {
                    return true;
                }
            }
        }
        if state == 99 {
            // 81224332 holds the pose while the joined monster is restrained.
            // 812243D4 exits at frame 154 if the contact missed.
            if let Some(command) = magnet::bind_input(actor, input, &unsafe { Pad::sample(actor) })
            {
                if matches!(command, BindInput::Pending) {
                    return true;
                }
                self.bind_engaged = true;
                unsafe { movement::grant_evade_invulnerability(actor, 8.) };
                if matches!(command, BindInput::Finish) {
                    // FZ 8121EF40 restores the form saved by event 40.
                    unsafe { self.branch(actor, 105) };
                }
                return true;
            }
            if self.bind_engaged || sample.blend <= 0. && sample.frame >= 154. {
                unsafe { self.branch(actor, 100) };
                return true;
            }
        }
        if unsafe { action::melee::take_deflection(actor) }
            && unsafe { self.branch(actor, self.mode.recoil()) }
        {
            unsafe { action::melee::clear_hitstop(actor) };
            return true;
        }
        if self.flight.is_none() && self.forward.moving() {
            let distance = self
                .forward
                .distance(unsafe { movement::motion_frames(actor) });
            unsafe { movement::displace_forward(actor, distance) };
        }
        let Some(cursor) = self.cursor.as_mut() else {
            return true;
        };
        if !drives_flight
            && commands & RESERVED == 0
            && (matches!(state, 40..=42 | 44..=46) || cursor.art_window(|condition| condition == 0))
            && unsafe { action::activate_hunter_art(actor) }
        {
            self.cursor = None;
            return true;
        }
        // FZ command 20 is always true. Conditions 6 and 8 read accepted guard
        // contacts; conditions 1 through 5 check stamina and magnetic segments.

        if let Some(next) =
            cursor.branch_with_press_time(commands | (1 << GUARD_FOLLOW_UP), eligible, |cmd| {
                match cmd {
                    0 | 2..=11 | 17 | 19 | 23 | 24 => press_time,
                    _ => None,
                }
            })
        {
            let next = if evaded {
                program::confirmed_evade_branch(state, next)
            } else {
                next
            };
            unsafe { self.transition(actor, next, true) };
        } else if finish_after_input {
            unsafe { self.finish(actor) };
        }
        true
    }

    unsafe fn tick(&mut self, actor: u32, input: &Input) -> bool {
        let current = unsafe { action::current(actor) };
        let elapsed = unsafe { ((actor + 28) as *const f32).read() };
        let sprint = self
            .flick
            .get_or_insert_with(Flick::default)
            .update(input, elapsed);
        let charging = action::custom_id(current.0, current.1) == Some(34);
        let stroke = crate::controls::take(actor);
        let commands = unsafe { commands(actor, input, sprint, charging, stroke) };
        let press_time = stroke.and_then(|s| s.time);
        if action::custom_id(current.0, current.1).is_none() {
            if let Some(clip) = unsafe { motion::public_source(actor) } {
                let sample = unsafe { action::MotionFrame::sample(actor) };
                unsafe {
                    sound::sample(&mut self.audio, actor, clip, sample);
                    fx::motion(
                        &mut self.particles,
                        &mut self.motion_accent,
                        actor,
                        clip,
                        sample,
                    );
                }
            } else {
                self.audio = MotionClock::new();
                self.particles = MotionClock::new();
            }
        }
        if current == (1, 6) && unsafe { action::weapon_drawn(actor) } {
            self.cursor = None;
            self.flight = None;
            let sample = unsafe { action::MotionFrame::sample(actor) };
            let execute = sample.blend <= 0. && sample.frame >= 40.;
            let allowed = commands & if execute { u32::MAX } else { !(1 << GUARD) };
            self.roll_pending |= allowed;
            if execute {
                for (key, id) in self.mode.roll_branches() {
                    if self.roll_pending & (1 << key) != 0
                        && unsafe { self.branch(actor, id.into()) }
                    {
                        self.roll_pending = 0;
                        return true;
                    }
                }
            }
            return false;
        }
        self.roll_pending = 0;
        motion::ROLL.clear_for(actor);
        if let Some(state) = action::custom_id(current.0, current.1) {
            if matches!(state, 40..=42 | 44..=46)
                && stroke.map_or(input.command(13), |s| {
                    s.pressed & PadButton::Y as u16 != 0
                        && s.held & (PadButton::R as u16 | PadButton::Zl as u16) == 0
                })
                && !unsafe { Pad::sample(actor) }.held(PadButton::R)
                && unsafe { action::transition_to(actor, 1, 2) }
            {
                self.cursor = None;
                return true;
            }
            return unsafe { self.advance(actor, state, commands, input, press_time) };
        }
        self.cursor = None;
        self.flight = None;
        let drawn = unsafe { action::weapon_drawn(actor) };
        let recoveries = if self.mode == program::Mode::Impact {
            [104, 103]
        } else {
            [102, 101]
        };
        if let Some(id) = unsafe {
            mhfz_sdk::combat::knockback_recovery(actor, current, drawn, input, recoveries)
        } {
            return unsafe { self.branch(actor, id.into()) };
        }
        if !drawn && !matches!(current, (1, 2 | 3)) {
            self.mode = program::Mode::Slash;
            motion::set_mode(actor, self.mode);
        }
        if !(movement::ground_idle(current) || !drawn && movement::sheathed_movement(current)) {
            return false;
        }
        if commands & RESERVED == 0 && unsafe { action::activate_hunter_art(actor) } {
            return true;
        }
        let Some((key, id)) = program::COMMAND_ORDER
            .iter()
            .copied()
            .filter(|&key| commands & (1 << key) != 0)
            .find_map(|key| self.mode.opener(key, drawn).map(|id| (key, id)))
        else {
            return false;
        };
        if drawn {
            if unsafe { self.branch(actor, u16::from(id)) } {
                return true;
            }
            // A magnetic command owns its pad edge even when it lacks a mark
            // or charge. Passing it to GU could start a stock ground action.
            program::cost(id) != 0 || matches!(key, BOOST | SHOT)
        } else if key == X
            && matches!(current, (1, 0 | 46))
            && input.stick_strength <= STICK_DEADZONE
            && stroke.is_none_or(|s| s.strength <= STICK_DEADZONE)
        {
            // GU's stationary draw owns the drawn flag and the return to weapon idle.
            unsafe { action::transition_to(actor, 1, 1) }
        } else if unsafe { action::enter_draw_attack(actor, id) } {
            unsafe { self.start(actor, id, None) }
        } else {
            false
        }
    }
}

static LOCAL: Shared<Battle> = Shared::new(Battle {
    roll_pending: 0,
    flick: None,
    timers: StateTimers::new(),
    mode: program::Mode::Slash,
    cursor: None,
    aim: None,
    flight: None,
    forward: AxisMotion::new(),
    audio: MotionClock::new(),
    particles: MotionClock::new(),
    native: crate::native_media::Playback::NEW,
    motion_accent: false,
    guard_hits: 0,
    evade: Evade::Inactive,
    bind_engaged: false,
});

pub(super) fn press_time(actor: u32, id: u8) -> Option<mhfz_sdk::action::Time> {
    LOCAL.with(|state| {
        let mut time = state.cursor.as_ref().filter(|c| c.state == id)?.time;
        if motion::local_pose(actor).is_some_and(|pose| pose.coast == Some(id)) {
            // advance() keeps the launch clock while displaying the separate coast loop.
            time.frame += unsafe { movement::motion_frames(actor) }.max(0.);
        } else {
            let sample = unsafe { action::MotionFrame::sample(actor) };
            time.observe(sample.frame, sample.blend * 0.5);
        }
        Some(time)
    })
}

pub extern "C" fn evade_contact(scene: &AttackScene<'_>) {
    let actor = scene.owner();
    let confirmed = LOCAL.with(|battle| {
        let Evade::Open(frames) = &mut battle.evade else {
            return false;
        };
        let (group, slot) = unsafe { action::current(actor) };
        if battle.cursor.as_ref().map(|cursor| cursor.state) != action::custom_id(group, slot) {
            battle.evade = Evade::Inactive;
            return false;
        }
        let elapsed = scene.frames();
        if elapsed == 0. {
            return false;
        }
        // FZ 816F2F1E expands selector 11 at node 0, the hunter's world root.
        if elapsed <= *frames
            && scene.overlaps_monster_attack(
                unsafe { movement::position(actor) },
                program::EVADE_RADIUS,
            )
        {
            battle.evade = Evade::Confirmed;
            return true;
        }
        *frames -= elapsed;
        if *frames <= 0. {
            battle.evade = Evade::Inactive;
        }
        false
    });
    if confirmed {
        unsafe { magnet::flash_evade(actor) };
    }
}

unsafe fn guard_open(actor: u32, state: u8, frame: f32) -> bool {
    // FZ event 21 closes at 26/32, extended by two frames per Guard tier.
    let end = match state {
        14 => 26,
        112 => 32,
        _ => return false,
    };
    frame < f32::from(end + unsafe { mhfz_sdk::combat::guard_bonus_frames(actor) })
}

pub unsafe extern "C" fn guard_policy(
    actor: u32,
    result: *mut guard::GuardResult,
) -> guard::Decision {
    unsafe {
        let (group, slot) = action::current(actor);
        let Some(state) = action::custom_id(group, slot) else {
            return guard::Decision::Reject;
        };
        if action::waiting_to_start(actor)
            || !guard_open(actor, state, action::MotionFrame::sample(actor).frame)
        {
            return guard::Decision::Reject;
        }
        result.write(guard::GuardResult {
            // FZ 81744DA0 applies four simulation ticks of protection.
            invulnerability: 8.,
            on_block: Some(record_guard),
            ..guard::GuardResult::default()
        });
        guard::Decision::InPlace
    }
}

pub(super) unsafe fn launch_sound(actor: u32, offset: [f32; 3]) {
    LOCAL.with(|battle| unsafe {
        sound::motion_accent(actor, &mut battle.motion_accent, 8, offset)
    });
}

unsafe extern "C" fn record_guard(actor: u32) {
    let first = LOCAL.with(|battle| {
        battle.guard_hits = battle.guard_hits.saturating_add(1).min(3);
        battle.guard_hits == 1
    });
    let (group, slot) = unsafe { action::current(actor) };
    match action::custom_id(group, slot) {
        Some(14) if first => unsafe {
            LOCAL.with(|battle| sound::slash_counter(actor, &mut battle.motion_accent));
            fx::guard(actor, program::Mode::Slash);
        },
        Some(112) => unsafe { fx::guard(actor, program::Mode::Impact) },
        _ => {}
    }
}

pub unsafe extern "C" fn dispatch(actor: u32, input: *const Input) -> u32 {
    LOCAL.with(|battle| {
        let Some(input) = (unsafe { input.as_ref() }) else {
            crate::controls::reset(actor);
            *battle = Battle::default();
            magnet::reset();
            crate::shock::reset(actor);
            mhfz_sdk::camera::reset();
            crate::bind::reset_camera();
            motion::set_mode(0, program::Mode::Slash);
            return PASS;
        };
        if unsafe { battle.tick(actor, input) } {
            0
        } else {
            PASS
        }
    })
}
