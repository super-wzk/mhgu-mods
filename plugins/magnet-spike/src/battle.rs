use crate::{
    combat,
    flight::Flight,
    fx,
    magnet::{self, AimedMark, MarkRange},
    motion, program, sound,
};
use mhfz_sdk::{action::Cursor, sound::Clock};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::{Input, PASS, action, guard, hunter, input, movement},
};

const EVADE: u8 = 0;
const X: u8 = 2;
const A: u8 = 3;
const CHORD: u8 = 4;
const CHANGE_MODE: u8 = 5;
const MAGNET_ATTACK: u8 = 9;
const BOOST: u8 = 11;
const GUARD: u8 = 13;
const SHOT: u8 = 19;
const GUARD_FOLLOW_UP: u8 = 20;
const REPEL: u8 = 23;
const ATTRACT: u8 = 24;

fn pressed(commands: u32, key: u8) -> bool {
    commands & (1 << key) != 0
}

unsafe fn commands(actor: u32, input: &Input, guard_pressed: bool) -> u32 {
    unsafe {
        // GU's resolved pad path is shared across weapon kinds. The weapon
        // command bank is produced by the retail fallback weapon's mapper.
        let buttons = input::attack_buttons_pressed(actor);
        let drawn = action::weapon_drawn(actor);
        let auxiliary = drawn && input::auxiliary_shoulder_held(actor);
        let shoulder = input.command(27);
        let special = input.command(8);
        let boost = drawn && !auxiliary && shoulder && special;
        let shot = auxiliary && buttons & 0x100 != 0;
        let magnet_attack = auxiliary && !shot && buttons & 0x20 != 0;
        let chord = !shot
            && !magnet_attack
            && (buttons == 0x120
                || input::attack_chord_pressed(actor)
                || input::attack_chord_with_shoulder(actor));
        let x = !shot && !magnet_attack && !chord && buttons & 0x20 != 0;
        let a = !shot && !chord && buttons & 0x100 != 0;
        let attract = auxiliary && input.command(3);
        let evade = !auxiliary && input.command(3) && movement::has_evade_stamina(actor, 150.);
        [
            (X, x),
            (A, a),
            (CHORD, chord),
            (SHOT, shot),
            (REPEL, auxiliary && special),
            (ATTRACT, attract),
            (MAGNET_ATTACK, magnet_attack || !drawn && shoulder && x),
            (BOOST, boost),
            (GUARD, drawn && !auxiliary && !special && guard_pressed),
            (CHANGE_MODE, !auxiliary && !boost && special),
            (EVADE, evade),
        ]
        .into_iter()
        .fold(0, |bits, (id, down)| bits | (u32::from(down) << id))
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
struct Battle {
    mode: program::Mode,
    cursor: Option<Cursor>,
    aim: Option<[f32; 3]>,
    flight: Option<Flight>,
    audio: Clock,
    guard_held: bool,
    guard_hits: u8,
    guard_from_idle: bool,
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
        if unsafe { motion::play_step(actor, cursor, step, &mut self.audio) }.is_none() {
            unsafe { self.finish(actor) };
            return false;
        }
        unsafe { Self::events(actor, cursor, &mut self.aim) };
        true
    }

    unsafe fn events(actor: u32, cursor: &mut Cursor, aim: &mut Option<[f32; 3]>) {
        let state = cursor.state;
        unsafe {
            let mode = motion::local_pose(actor).unwrap_or_default().mode;
            let bonus = program::power_bonus(magnet::boosts(actor), mode);
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
                // FZ 81225D6A consumes the discrete evade cost.
                11 => movement::stamina(actor, -150., false),
                // Use the aiming cue at event 17 to save GU's camera direction.
                17 if matches!(state, 82 | 84) => *aim = Some(magnet::aim(actor)),
                4 if value == 1 => {
                    let direction = aim.take().unwrap_or_else(|| magnet::aim(actor));
                    magnet::launch(actor, direction);
                }
                _ => {}
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
            if !program::is_attraction(id) && program::flight_landing(id).is_some() {
                let Some(flight) = self.flight.as_mut().filter(|flight| flight.launched()) else {
                    self.finish(actor);
                    return true;
                };
                flight.coast();
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
                        MarkRange::Attract => self.flight = Some(Flight::new(aim.target)),
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
            self.mode = self.mode.for_state(id);
            self.aim = None;
            self.guard_hits = 0;
            self.guard_from_idle = false;
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
        let target = if target == 207 {
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
            if unsafe { action::transition_to(actor, 1, 6) } {
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
            return unsafe { self.start(actor, id, aim) };
        }
        false
    }

    unsafe fn advance(&mut self, actor: u32, state: u8, commands: u32) -> bool {
        let sample = unsafe { action::MotionFrame::sample(actor) };
        // R also modifies Boost. Resolve a staggered chord during an unused
        // guard started from idle, just as on a simultaneous press.
        if self.guard_from_idle
            && self.guard_hits == 0
            && pressed(commands, BOOST)
            && self
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.state == state)
            && unsafe { guard_open(actor, state, sample.frame) }
            && let Some(boost) = self.mode.opener(BOOST, true)
            && unsafe { self.branch(actor, u16::from(boost)) }
        {
            return true;
        }
        let Some(cursor) = self.cursor.as_mut().filter(|cursor| cursor.state == state) else {
            let aim = mark_range(state).and_then(|range| unsafe { range.aim(actor) });
            return unsafe { self.start(actor, state, aim) };
        };
        let Some(step) = program::step(state, cursor.time.step) else {
            unsafe { self.finish(actor) };
            return true;
        };
        if step.op == 2 {
            return unsafe { self.branch(actor, step.clip) };
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
        unsafe { sound::sample(&mut self.audio, actor, clip, sample.frame, false) };
        let wrapped = if in_coast_pose {
            // The new pose has its own loop. Keep checking the authored
            // action's pending branches and events on its uninterrupted clock.
            let elapsed = unsafe { movement::motion_frames(actor) };
            cursor.time.frame += elapsed.max(0.);
            false
        } else {
            cursor.time.observe(sample.frame, sample.blend)
        };
        unsafe { Self::events(actor, cursor, &mut self.aim) };
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
            if unsafe { flight.advance(actor) } {
                unsafe { self.branch(actor, u16::from(landing)) };
                return true;
            }
            if launch_clip
                && !in_coast_pose
                && flight.coasting()
                && unsafe { motion::play(actor, self.mode.coast_clip(), 0, 0) }.is_some()
            {
                motion::coast(actor, state);
                unsafe { sound::sample(&mut self.audio, actor, self.mode.coast_clip(), 0., true) };
            }
        } else if program::step_complete(step, cursor.time, sample, wrapped) {
            // FZ 8121F57C advances 81815D60 before checking branch windows.
            cursor.time.step += 1;
            if !unsafe { self.play_step(actor) } {
                return true;
            }
        }
        if unsafe { action::melee::take_deflection(actor) }
            && unsafe { self.branch(actor, self.mode.recoil()) }
        {
            unsafe { action::melee::clear_hitstop(actor) };
            return true;
        }
        let Some(cursor) = self.cursor.as_mut() else {
            return true;
        };
        if !drives_flight
            && cursor.art_window(|condition| condition == 0)
            && unsafe { action::activate_hunter_art(actor) }
        {
            self.cursor = None;
            return true;
        }
        // FZ command 20 is always true. Conditions 6 and 8 read accepted guard
        // contacts; conditions 1 through 5 check stamina and magnetic segments.
        let guard_hits = self.guard_hits;
        if let Some(next) =
            cursor.branch(
                commands | (1 << GUARD_FOLLOW_UP),
                |condition| match condition {
                    0 => true,
                    1..=5 => {
                        (unsafe { movement::has_stamina(actor) })
                            && magnet::can_spend(actor, condition as u8)
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
                    _ => false,
                },
            )
        {
            unsafe { self.branch(actor, next) };
        }
        true
    }

    unsafe fn tick(&mut self, actor: u32, input: &Input) -> bool {
        let current = unsafe { action::current(actor) };
        let held = input.command(27);
        let was_held = core::mem::replace(&mut self.guard_held, held);
        let guard_pressed = held && !was_held;
        let commands = unsafe { commands(actor, input, guard_pressed) };
        if let Some(state) = action::custom_id(current.0, current.1) {
            return unsafe { self.advance(actor, state, commands) };
        }
        self.cursor = None;
        self.flight = None;
        let drawn = unsafe { action::weapon_drawn(actor) };
        if !drawn {
            self.mode = program::Mode::Slash;
            motion::set_mode(actor, self.mode);
        }
        if !(movement::ground_idle(current) || !drawn && movement::sheathed_movement(current)) {
            return false;
        }
        if unsafe { action::activate_hunter_art(actor) } {
            return true;
        }
        let Some(id) = [
            REPEL,
            SHOT,
            ATTRACT,
            MAGNET_ATTACK,
            BOOST,
            CHORD,
            X,
            A,
            CHANGE_MODE,
            GUARD,
        ]
        .into_iter()
        .find_map(|key| {
            if !pressed(commands, key) {
                return None;
            }
            self.mode.opener(key, drawn)
        }) else {
            return false;
        };
        if drawn {
            if unsafe { self.branch(actor, u16::from(id)) } {
                self.guard_from_idle = matches!(id, 14 | 112);
                return true;
            }
            // A magnetic command owns its pad edge even when it lacks a mark
            // or charge. Passing it to GU could start a stock ground action.
            pressed(commands, ATTRACT) || pressed(commands, REPEL) || pressed(commands, BOOST)
        } else if unsafe { action::enter_draw_attack(actor, id) } {
            unsafe { self.start(actor, id, None) }
        } else {
            false
        }
    }
}

static LOCAL: Shared<Battle> = Shared::new(Battle {
    mode: program::Mode::Slash,
    cursor: None,
    aim: None,
    flight: None,
    audio: Clock::new(),
    guard_held: false,
    guard_hits: 0,
    guard_from_idle: false,
});

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

unsafe extern "C" fn record_guard(actor: u32) {
    let first = LOCAL.with(|battle| {
        battle.guard_hits = battle.guard_hits.saturating_add(1).min(3);
        battle.guard_hits == 1
    });
    let (group, slot) = unsafe { action::current(actor) };
    match action::custom_id(group, slot) {
        Some(14) if first => unsafe {
            sound::slash_counter(actor);
            fx::guard(actor, program::Mode::Slash);
        },
        Some(112) => unsafe { fx::guard(actor, program::Mode::Impact) },
        _ => {}
    }
}

pub unsafe extern "C" fn dispatch(actor: u32, input: *const Input) -> u32 {
    LOCAL.with(|battle| {
        let Some(input) = (unsafe { input.as_ref() }) else {
            *battle = Battle::default();
            magnet::reset();
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
