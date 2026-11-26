//! Local input selects FZ states. Both local and received actions replay the same steps.
use crate::{
    collision, fx, motion, native,
    program::{self, BACKSTEP, BRAKE, JUMP_SLASH, SPIN, SPRINT, SPRINT_SLASH, VACUUM},
    read, sound,
    trail::History,
    write,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhfz_sdk::{
    action::Cursor,
    combat::{DrawnEvade, bladescale_evade, evade_bonus_frames},
    effects::MotionClock,
    sound::Clock,
    sprint::{self, Flick, Resume},
};
use mhgu_sdk::style::{
    self, ActionRequest, Input, NATIVE_ACTION, PASS,
    action::{self, MotionFrame, command::CommandSet, melee},
    hunter, movement,
    remote::{self, PeerStates, Roster},
};

#[derive(Default)]
struct Battle {
    actor: u32,
    equipment: u32,
    index: usize,
    cursor: Option<Cursor>,
    packed: u16,
    sound: Clock,
    fx: MotionClock,
    trail: History,
    flick: Flick,
    sprint_input: f32,
    resume: Resume,
    poise: bool,
    deflectable: bool,
    recoil: bool,
    evade_yaw: Option<u16>,
    roll_branch: Option<(u8, i16)>,
    peer_round_force: bool,
}
static PEERS: PeerStates<Battle> = PeerStates::new();
struct Local(UnsafeCell<Option<Battle>>);
unsafe impl Sync for Local {}
static LOCAL: Local = Local(UnsafeCell::new(None));
static BUSY: AtomicBool = AtomicBool::new(false);
static TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, movement::CLIMBS);

pub unsafe extern "C" fn commands(actor: u32, table: u32) -> *const CommandSet {
    if table == 2000 && unsafe { action::current(actor) } == (action::GROUP, action::slot(SPRINT)) {
        &TERRAIN
    } else {
        core::ptr::null()
    }
}

unsafe fn wave_power(actor: u32, index: usize) -> Option<u16> {
    let (mind_eye, chaos) = unsafe {
        if index == 0 {
            // GU 116E4D8 expires the two oils independently.
            let mind_eye = read::<u32>(actor, 13096) == 4 && read::<f32>(actor, 13104) > 0.;
            let chaos = matches!(read::<u32>(actor, 13100), 5..=7)
                && read::<f32>(actor, 13108) > 0.;
            (mind_eye, chaos)
        } else {
            // GU 1168EEC replicates ordinary oils in the low word and Chaos Oil in the high word.
            let mind_eye = read::<u32>(actor, 10336) & 8 != 0;
            let chaos = read::<u32>(actor, 10340) & 0xe != 0;
            (mind_eye, chaos)
        }
    };
    match (mind_eye, chaos) {
        (true, true) => Some(program::WAVE_POWER),
        (false, false) => None,
        _ => Some(program::WAVE_POWER / 2),
    }
}

impl Battle {
    fn begin(&mut self, id: u8, turn: i16) -> bool {
        let a = self.actor;
        unsafe {
            let id = if id == SPIN && wave_power(a, self.index).is_some() {
                VACUUM
            } else {
                id
            };
            if !program::ACTIONS.contains(id) || !action::accept_custom_start(a, id) {
                return false;
            }
            if turn != 0 {
                movement::turn(a, turn as u16);
            }
            self.start(id);
            true
        }
    }
    fn start(&mut self, id: u8) {
        let a = self.actor;
        self.trail.clear();
        self.cursor = Some(program::cursor(id));
        self.poise = false;
        self.recoil = false;
        self.deflectable = false;
        self.roll_branch = None;
        self.evade_yaw = None;
        self.resume.set(false);
        unsafe {
            write(a, 5, 1u8);
            let mode = if id == JUMP_SLASH { 2 } else { 0 };
            native!(0x282fd8, a, mode, u32::from(id != SPRINT), 0);
            if id == JUMP_SLASH {
                native!(0x2a8144, a); // w01 jump slash inherits velocity and caps gravity.
                motion::damp_jump_momentum(a);
            }
            if native!(0x2a2444, a) == 0 {
                native!(0x2a264c, a, 1);
            } else {
                native!(0x2a2560, a, 1);
            }
            if id == SPRINT && self.index == 0 {
                self.sprint_input = 0.;
                native!(0x283238, a, 0, 0x1180, 0);
                native!(0x2e9b7c, a, 2000, 0);
                movement::steer(a);
            }
        }
        self.play_step();
        if id == BACKSTEP && self.index == 0 && self.cursor.is_some() {
            unsafe {
                movement::stamina(a, -150., false);
                bladescale_evade(a, DrawnEvade::Roll);
                write(a, 11072, 14. + evade_bonus_frames(a));
            }
        }
    }
    fn finish(&mut self) {
        self.trail.clear();
        self.cursor = None;
        if self.index == 0 {
            unsafe {
                native!(0x2b6e5c, self.actor);
            }
        }
    }
    fn play_step(&mut self) {
        let Some(c) = self.cursor.as_mut() else {
            return;
        };
        let Some(s) = program::step(c.state, c.time.step) else {
            self.finish();
            return;
        };
        if s.op == 2 {
            if self.index == 0 && s.clip == 54 {
                unsafe {
                    action::transition_to(self.actor, 32, 0);
                }
            }
            return;
        }
        if !(3..=5).contains(&s.op) {
            self.finish();
            return;
        }
        let Some(packed) = (unsafe {
            motion::play(
                self.actor,
                program::play_clip(c.state, s.clip),
                s.start,
                s.blend,
            )
        }) else {
            self.finish();
            return;
        };
        self.packed = packed;
        c.time.cycles = 0;
        c.time.frame = s.start.into();
        c.time.blend = (s.blend / 2).into();
        self.fx.seek(s.clip, c.time.frame);
        unsafe {
            sound::update(
                &mut self.sound,
                self.actor,
                Some(s.clip),
                c.time.frame,
                Some(c.time.frame),
            );
        }
        self.events();
    }
    fn events(&mut self) {
        let a = self.actor;
        while let Some(c) = self.cursor.as_mut()
            && let Some((kind, param, late)) = c.next_event()
        {
            let local = self.index == 0;
            let frames = (2. * f32::from(param) - late).max(0.);
            unsafe {
                match kind {
                    1 => {
                        native!(0x282fd8, a, u32::from(param & 2), u32::from(param & 1), 0);
                    }
                    2 => {
                        native!(0x2a2560, a, u32::from(param != 0));
                    }
                    3 if local => {
                        let flags = read::<u32>(a, 9488);
                        write(a, 9488, if param != 0 { flags | 16 } else { flags & !16 });
                    }
                    4 if local => movement::turn(a, param),
                    5 if local => {
                        self.poise = false;
                        write(a, 10288, frames);
                    }
                    6 if local => self.poise = true,
                    7 | 9 if local => write(a, 11072, frames),
                    8 if local => write(a, 11072, frames + evade_bonus_frames(a)),
                    11 if local => {
                        movement::stamina(a, -150., false);
                        if matches!(c.state, 37..=39) {
                            bladescale_evade(a, DrawnEvade::Roll);
                        }
                    }
                    12 => {
                        let can_bounce = match c.state {
                            1 | 7 | 9 | 10 => self.deflectable,
                            2 => true,
                            0 | 3 | 4 | 6 | 8 | 11 | 12 | 14 | 21..=30 | 33 | 37..=39 => true,
                            _ => false,
                        };
                        collision::strike(a, c.state, param, c.time.frame, late, can_bounce);
                    }
                    13 if c.state == VACUUM => {
                        if let Some(power) = wave_power(a, self.index) {
                            collision::launch(a, self.index, power);
                        }
                    }
                    14 => mhfz_sdk::camera::start(a, param),
                    15 => self.trail.start(self.index, a, frames),
                    16 => self.deflectable = true,
                    _ => {}
                }
            }
        }
    }

    fn evade_followup(&mut self, sample: MotionFrame, bits: u32, yaw: Option<u16>) -> bool {
        // FZ 817E814C passes these windows to w04's common evade follow-ups.
        if (10. ..48.).contains(&sample.frame) && sample.blend <= 0. && self.roll_branch.is_none() {
            self.roll_branch = if bits & 2 != 0 {
                Some((SPRINT, 0))
            } else if bits & 4 != 0 {
                Some((
                    11,
                    match yaw.map(program::input_sector) {
                        Some(1) => -8192,
                        Some(3) => 8192,
                        _ => 0,
                    },
                ))
            } else {
                None
            };
        }
        sample.frame >= 24.
            && self
                .roll_branch
                .is_some_and(|(id, turn)| self.begin(id, turn))
    }

    fn inputs(&mut self, input: &Input, dt: f32, yaw: Option<u16>) -> u32 {
        let a = self.actor;
        if self.flick.update(input, dt) {
            self.sprint_input = 12.;
        } else {
            self.sprint_input = (self.sprint_input - dt).max(0.);
        }
        if yaw.is_none() {
            self.sprint_input = 0.;
        }
        let enough = unsafe { read::<f32>(a, 5088) >= 150. };
        let mut bits = 0;
        let sector = yaw.map(program::input_sector);
        let shoulder = input.command(27);
        // GU w01 resolves X followed by R as command 4 without a new X edge.
        let shoulder_slash = input.weapon_command(4);
        if input.command(3) && enough {
            bits |= 1;
            if shoulder && let Some(sector @ 1..=3) = sector {
                bits |= 1 << [0, 17, 15, 16][sector];
            }
        }
        if self.sprint_input > 0. && unsafe { read::<f32>(a, 5088) } > 0. {
            bits |= 1 << 1;
        }
        if input.weapon_command(7) || unsafe { style::input::attack_chord_with_shoulder(a) } {
            bits |= 1 << 7;
        } else if input.weapon_command(0) || input.weapon_command(15) || shoulder_slash {
            bits |= 1 << 2;
            if shoulder || shoulder_slash {
                bits |= 1 << 14;
            }
            if sector == Some(2) {
                bits |= 1 << 13;
            }
        } else if input.weapon_command(22) || input.weapon_command(9) || input.weapon_command(26) {
            // GU w01 1169554 emits command 26 for the resolved R+A chord,
            // including simultaneous presses where plain A never fires.
            bits |= 1 << 5;
            if yaw.is_some() {
                bits |= 1 << 6;
            }
            if shoulder
                || input.weapon_command(26)
                || self
                    .cursor
                    .as_ref()
                    .is_some_and(|c| matches!(c.state, 43..=52))
            {
                bits |= 1 << 11;
            }
        }
        if input.command(8) {
            bits |= 1 << 8;
        }
        if bits & 0x1fc != 0 {
            bits |= 1 << 20;
        }
        bits
    }
    fn advance(&mut self, input: Option<&Input>, bits: u32, yaw: Option<u16>, dt: f32) {
        let a = self.actor;
        let Some(c) = self.cursor.as_mut() else {
            return;
        };
        let state = c.state;
        let Some(s) = program::step(state, c.time.step) else {
            self.finish();
            return;
        };
        if s.op == 2 {
            self.play_step();
            return;
        }
        let sample = unsafe { MotionFrame::sample(a) };
        let wrapped = c.time.observe(sample.frame, sample.blend);
        unsafe {
            self.trail.tick(a, dt);
            sound::update(&mut self.sound, a, Some(s.clip), sample.frame, None);
            fx::update(&mut self.fx, a, s.clip, sample);
        }
        self.events();
        if state == JUMP_SLASH && unsafe { read::<u8>(a, 5) } == 1 {
            // GU 116AB98 integrates flight and clears velocity on touchdown.
            // Finish the same FZ clip even when a low ledge lands before hit two.
            unsafe {
                native!(0x539f4c, a);
                if !action::land(a) {
                    if input.is_some() {
                        // w04 state 36 accepts X before execution. Keep that
                        // input while falling, but start the ground slash only after landing.
                        self.cursor.as_mut().unwrap().branch(bits & (1 << 2), |n| n <= 1);
                    }
                    return;
                }
                native!(0x27c6f4, a);
                write(a, 5, 2u8);
            }
        }
        if let Some(input) = input {
            if state == BACKSTEP && self.evade_followup(sample, bits, yaw) {
                return;
            }
            if self.poise {
                unsafe {
                    write(a, 10288, read::<f32>(a, 10288).max(8.));
                }
            }
            self.recoil |= unsafe { melee::take_deflection(a) };
            if self.recoil && self.begin(5, 0) {
                unsafe {
                    melee::clear_hitstop(a);
                }
                return;
            }
            let c = self.cursor.as_mut().unwrap();
            if (state == SPRINT || c.art_window(|n| n <= 1))
                && unsafe { action::activate_hunter_art(a) }
            {
                self.cursor = None;
                return;
            }
            let branch = c.branch(bits, |n| n <= 1);
            if c.pending_action() == Some(53) {
                if self.evade_yaw.is_none() {
                    self.evade_yaw = yaw;
                }
            } else {
                self.evade_yaw = None;
            }
            if let Some(id) = branch {
                if id == 53 {
                    // FZ 811EAD82 selects common 0:145 with neutral or rear input.
                    if self.evade_yaw.is_none_or(|v| (0x6000..0xa000).contains(&v)) {
                        if self.begin(BACKSTEP, 0) {
                            return;
                        }
                    } else if unsafe { motion::evade(a, self.evade_yaw) } {
                        self.cursor = None;
                        return;
                    }
                }
                if let Some((next, turn)) = program::destination(id, yaw)
                    && self.begin(next, turn)
                {
                    return;
                }
            }
            if state == SPRINT {
                if unsafe { movement::step_off_ledge(a, 0) } {
                    self.resume.set(true);
                    return;
                }
                if bits & 1 != 0 && unsafe { motion::evade(a, None) } {
                    return;
                }
                if input.command(13) && unsafe { action::transition_to(a, 1, 2) } {
                    return;
                }
                if sample.frame >= 30. && sample.blend <= 0. {
                    let next = if bits & (1 << 2) != 0 {
                        Some(SPRINT_SLASH)
                    } else if bits & (1 << 5) != 0 {
                        Some(42)
                    } else if bits & (1 << 7) != 0 {
                        Some(1)
                    } else {
                        None
                    };
                    if next.is_some_and(|v| self.begin(v, 0)) {
                        return;
                    }
                }
                let direction = unsafe { movement::steer(a) };
                if sprint::can_brake(sample.frame, self.cursor.as_ref().unwrap().time.cycles)
                    && (!direction || unsafe { read::<f32>(a, 5088) } <= 0.)
                    && self.begin(BRAKE, 0)
                {
                    return;
                }
                if dt > 0. {
                    unsafe {
                        movement::drain_running_stamina(a);
                    }
                }
            }
        }
        let c = self.cursor.as_mut().unwrap();
        if s.complete(c.time, sample, wrapped) {
            c.time.step += 1;
            self.play_step();
        }
    }
    fn tick(&mut self, input: &Input) -> bool {
        let a = self.actor;
        unsafe {
            let current = action::current(a);
            let dt = read::<f32>(a, 28).max(0.);
            let yaw = movement::live_relative_stick_yaw(a);
            let bits = self.inputs(input, dt, yaw);
            if let Some(id) = action::custom_id(current.0, current.1) {
                if self.cursor.as_ref().is_none_or(|c| c.state != id) || read::<u8>(a, 5) == 0 {
                    self.start(id);
                } else if read::<u16>(a, 1204) == self.packed {
                    self.advance(Some(input), bits, yaw, dt);
                } else {
                    self.finish();
                }
                return true;
            }
            self.trail.clear();
            let was_sprint = self.cursor.take().is_some_and(|c| c.state == SPRINT);
            let drawn = native!(0x2a2444, a) != 0;
            if current.0 == action::GROUP {
                return false;
            }
            let ground = movement::ground_idle(current)
                || (matches!(current.0, 1 | 2)
                    && read::<u8>(a, 5) != 0
                    && matches!(native!(0x53a058, a), 5001 | 5004));
            self.resume.on_native(a, current, ground, drawn, was_sprint);
            let clip = motion::source_clip(a, read(a, 1204));
            let sample = MotionFrame::sample(a);
            sound::update(&mut self.sound, a, clip, sample.frame, None);
            if let Some(clip) = clip {
                fx::update(&mut self.fx, a, clip, sample);
            }
            if drawn && current == (1, 6) {
                // At phase zero GU still has to encode motion 28 using the
                // direction saved by the accepted attack cancel.
                if read::<u8>(a, 5) == 0 {
                    self.roll_branch = None;
                    return false;
                }
                return self.evade_followup(sample, bits, yaw);
            }
            self.roll_branch = None;
            motion::clear_roll(a);
            if drawn
                && matches!(current, (64, 6 | 7))
                && read::<u8>(a, 5) == 2
                && bits & 1 != 0
                && read::<u8>(a, 11988) != 0
                && read::<i16>(a, 5078) > 0
                && native!(0x53a058, a) == 1015
            {
                return self.begin(if current.1 == 6 { 41 } else { 40 }, 0);
            }
            let guarding = matches!(current, (32, 0 | 1 | 9));
            if !(ground || guarding) || !drawn {
                return false;
            }
            if action::activate_hunter_art(a) {
                return true;
            }
            let id = if bits & (1 << 11) != 0 {
                if yaw.is_some() { 47 } else { 43 }
            } else if guarding && bits & (1 << 2) != 0 {
                12
            } else if bits & 2 != 0 || self.resume.ready(yaw.is_some(), read::<f32>(a, 5088) > 0.) {
                SPRINT
            } else if bits & (1 << 7) != 0 {
                if guarding { 14 } else { 1 }
            } else if bits & (1 << 8) != 0 {
                12
            } else if bits & (1 << 2) != 0 {
                0
            } else if bits & (1 << 6) != 0 {
                SPIN
            } else if bits & (1 << 5) != 0 {
                13
            } else {
                return false;
            };
            self.begin(id, 0)
        }
    }
}

pub unsafe extern "C" fn dispatch(actor: u32, input: *const Input) -> u32 {
    if BUSY.swap(true, Ordering::Acquire) {
        return PASS;
    }
    let mut result = PASS;
    unsafe {
        let saved = &mut *LOCAL.0.get();
        if let Some(input) = input.as_ref() {
            let eq = read(actor, 5060);
            if saved
                .as_ref()
                .is_none_or(|b| (b.actor, b.equipment) != (actor, eq))
            {
                if let Some(previous) = saved.as_ref() {
                    motion::clear_roll(previous.actor);
                }
                collision::reset(0);
                *saved = Some(Battle {
                    actor,
                    equipment: eq,
                    ..Battle::default()
                });
            }
            if saved.as_mut().unwrap().tick(input) {
                result = 0;
            }
        } else {
            if actor != 0
                && saved.as_ref().is_some_and(|b| b.actor == actor)
                && action::custom_id(action::current(actor).0, action::current(actor).1).is_some()
            {
                native!(0x2b6e5c, actor);
            }
            if let Some(previous) = saved.take() {
                motion::clear_roll(previous.actor);
            }
            collision::reset(0);
            mhfz_sdk::camera::reset();
        }
    }
    BUSY.store(false, Ordering::Release);
    result
}

pub unsafe extern "C" fn request(actor: u32, raw: *const ActionRequest) -> u32 {
    let req = unsafe { &*raw };
    // GU 1169A7C dispatches these three jump slashes to 116AB98.
    // The aerial draw route (kind 5) submits 16:16 with its own transition flags.
    if matches!(req.slot, 16 | 22 | 50) {
        return u32::from(JUMP_SLASH);
    }
    // All three ground draw routes are 16:7.
    if req.slot == 7 && (req.draw_route || unsafe { native!(0x2a2444, actor) } == 0) {
        return u32::from(program::DRAW);
    }
    if req.draw_route {
        return PASS;
    }
    // Guild tables 2000/2023 can submit the stock attack (14) or backsteps
    // (15 and 25) before the controller sees R+A or the special key.
    if matches!(req.slot, 16..=24 | 26..=68) {
        return NATIVE_ACTION;
    }
    PASS
}

unsafe extern "C" fn peer(actor: u32, id: u8) {
    unsafe {
        let Some(slot) = Roster::current().and_then(|r| r.peer_index(actor)) else {
            return;
        };
        if !program::ACTIONS.contains(id)
            || hunter::equipped_weapon(actor) != Some(crate::WEAPON)
            || !hunter::local()
                .is_some_and(|local| read::<u8>(local, 4180) == read::<u8>(actor, 4180))
        {
            return;
        }
        PEERS.update(slot, actor, |b, changed| {
            if changed {
                collision::reset(slot + 1);
            }
            let b = b.get_or_insert_with(|| Battle {
                actor,
                index: slot + 1,
                ..Battle::default()
            });
            b.peer_round_force = false;
            if read::<u8>(actor, 5) == 0 {
                b.start(id);
            } else if b.cursor.as_ref().is_some_and(|c| c.state == id)
                && read::<u16>(actor, 1204) == b.packed
            {
                b.advance(None, 0, None, read::<f32>(actor, 28).max(0.));
            }
        });
    }
}

// Round Force keeps its retail action ID. Mark it in the existing style packet
// field so a native-style SnS peer never emits an Extreme wave.
unsafe extern "C" fn encode_art(actor: u32) -> u8 {
    if matches!(unsafe { action::current(actor) }, (action::GROUP, 43..=45)) {
        1
    } else {
        u8::MAX
    }
}

unsafe extern "C" fn decode_art(actor: u32, bits: u8) {
    unsafe {
        let Some(slot) = Roster::current().and_then(|r| r.peer_index(actor)) else {
            return;
        };
        let marked = bits == 1 && encode_art(actor) == 1;
        PEERS.update(slot, actor, |saved, changed| {
            if changed {
                collision::reset(slot + 1);
            }
            if marked || saved.is_some() {
                let b = saved.get_or_insert_with(|| Battle {
                    actor,
                    index: slot + 1,
                    ..Battle::default()
                });
                b.peer_round_force = marked;
            }
        });
    }
}

#[forge::hook(offset = 0x116d464)]
unsafe fn round_force(actor: u32, tier: u32) -> u32 {
    unsafe {
        // GU advances the PMA cursor once for each accepted attack event.
        // Tables 15/16/17 open at frames 20/28/30 respectively.
        let first = read::<u8>(actor, 5) == 1 && read::<u32>(actor, 12716) == 0;
        let result = original!(actor, tier);
        if first && read::<u32>(actor, 12716) != 0 {
            if hunter::local() == Some(actor) {
                if live(0, actor) && let Some(power) = wave_power(actor, 0) {
                    collision::launch_art(actor, 0, tier, power);
                }
            } else if let Some(slot) = Roster::current().and_then(|r| r.peer_index(actor))
                && live(slot + 1, actor)
                && let Some(power) = wave_power(actor, slot + 1)
            {
                PEERS.update(slot, actor, |saved, _| {
                    if saved.as_ref().is_some_and(|b| b.peer_round_force) {
                        collision::launch_art(actor, slot + 1, tier, power);
                    }
                });
            }
        }
        result
    }
}

pub(super) static PLAYBACK: remote::Playback = remote::Playback {
    tick: peer,
    observe: Some(observe),
    action_state: Some(&remote::ActionState {
        encode: encode_art,
        decode: decode_art,
    }),
};

pub fn install_round_force() {
    forge::install_hook!(forge::mem::text_addr(), round_force);
}

unsafe extern "C" fn observe(local: u32) {
    unsafe {
        PEERS.visit_live(crate::WEAPON, |_, actor, b| {
            let (group, slot) = action::current(actor);
            if action::custom_id(group, slot).is_none()
                || read::<u8>(actor, 4180) != read::<u8>(local, 4180)
            {
                b.cursor = None;
                b.trail.clear();
            }
        });
    }
}

pub unsafe fn live(index: usize, actor: u32) -> bool {
    unsafe {
        if index == 0 {
            style::selected(crate::WEAPON) == crate::STYLE_ID
                && hunter::local() == Some(actor)
                && hunter::is_quest(actor)
                && hunter::equipped_weapon(actor) == Some(crate::WEAPON)
        } else {
            Roster::current().and_then(|roster| PEERS.live(index - 1, &roster, crate::WEAPON))
                == Some(actor)
                && hunter::local()
                    .is_some_and(|local| read::<u8>(local, 4180) == read::<u8>(actor, 4180))
        }
    }
}
