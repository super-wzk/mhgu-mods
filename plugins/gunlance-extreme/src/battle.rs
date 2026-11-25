//! Drive the w09 table through GU input, playback and native contacts.
use crate::{fx, motion, native, program, read, shell, sound, write};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhfz_sdk::{
    action::Cursor,
    combat::{HitWindow, evade_bonus_frames},
    effects::MotionClock,
    sound::Clock,
    sprint::{self, Flick, Resume},
};
use mhgu_sdk::{
    style::{
        ActionRequest, Input, NATIVE_ACTION, PASS,
        action::{self, Attack, MotionFrame, command::CommandSet, melee},
        hunter,
        input::{attack_chord_pressed, attack_chord_with_shoulder},
        movement,
        remote::{self, PeerStates, Roster},
    },
    sync::Shared,
};

const SHEATHE: u8 = 13;
const TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, movement::CLIMBS);

#[derive(Default)]
struct Battle {
    actor: u32,
    equipment: u32,
    cursor: Option<Cursor>,
    packed: u16,
    motion_fx: MotionClock,
    sound: Clock,
    flick: Flick,
    resume: Resume,
    local: bool,
    fx_owner: usize,
    flame: fx::Flame,
    muzzle: fx::Muzzle,
    next_vapour: f32,
    steam: f32,
    next_steam: f32,
    reload_rounds: u8,
    received_ammo: Option<(u8, u8)>,
    synced_ammo: bool,
    area: u8,
    rounds: u8,
    chambered: u8,
    burst: u8,
    burst_started: bool,
    overchambered: bool,
    heat_blade: f32,
    ignition_cooldown: f32,
    quick_reload: f32,
    poise: f32,
    guard_pair: Option<(u16, u16)>,
    windup: f32,
    aimed_yaw: i32,
    fired: bool,
}

struct Local(UnsafeCell<Option<Battle>>);
unsafe impl Sync for Local {}
static LOCAL: Local = Local(UnsafeCell::new(None));
static BUSY: AtomicBool = AtomicBool::new(false);
static IGNITION_HUD: Shared<Option<IgnitionHud>> = Shared::new(None);
static PEERS: PeerStates<Battle> = PeerStates::new();

#[derive(Clone, Copy)]
struct IgnitionHud {
    owner: (u32, u32),
    active: f32,
    cooldown: f32,
}

pub(super) fn ignition_hud(actor: u32, equipment: u32) -> (bool, f32) {
    let Some(state) = IGNITION_HUD.with(|state| state.filter(|s| s.owner == (actor, equipment)))
    else {
        return (false, 1.);
    };
    if state.active > 0. {
        let remaining = state.active / program::timer(program::IGNITION);
        (true, remaining.clamp(0., 1.))
    } else {
        let remaining = state.cooldown / program::timer(program::IGNITION_COOLDOWN);
        (false, (1. - remaining).clamp(0., 1.))
    }
}

pub unsafe extern "C" fn commands(actor: u32, table: u32) -> *const CommandSet {
    if table == 2000
        && unsafe { action::current(actor) } == (action::GROUP, action::slot(program::SPRINT))
    {
        &TERRAIN
    } else {
        core::ptr::null()
    }
}

impl Battle {
    fn replay_shells(&self) -> bool {
        self.local || self.synced_ammo
    }

    fn begin(&mut self, state: u8) -> bool {
        if !program::ACTIONS.contains(state)
            || !unsafe { action::accept_custom_start(self.actor, state) }
        {
            return false;
        }
        self.start(state);
        true
    }

    fn start(&mut self, state: u8) {
        self.cursor = Some(program::cursor(state));
        self.guard_pair = None;
        let resumed = state == program::SPRINT && self.resume.pending();
        self.resume.set(false);
        self.rounds = 0;
        self.burst = 0;
        self.burst_started = false;
        self.overchambered = false;
        self.reload_rounds = 0;
        self.next_vapour = 0.;
        if self.steam == 0. {
            self.muzzle.stop();
        }
        self.poise = 0.;
        self.windup = 0.;
        self.fired = false;
        if !matches!(state, 47..=50 | 73 | 74) {
            self.chambered = 0;
        }
        if matches!(state, 3 | 37 | 38) {
            self.aimed_yaw = 0;
        }
        unsafe {
            let a = self.actor;
            let count = self
                .received_ammo
                .take()
                .filter(|&(action, _)| action == state);
            self.synced_ammo = count.is_some();
            if let Some((_, count)) = count {
                shell::sync_ammo(a, count);
            }
            write(a, 5, 1u8);
            native!(0x282fd8, a, 0, u32::from(state != program::SPRINT), 0);
            if native!(0x2a2444, a) == 0 {
                native!(0x2a264c, a, 1);
            }
            native!(0x2a2560, a, 1);
            if self.replay_shells() && matches!(state, 47 | 49) {
                self.chambered = shell::consume(a, 1);
            }
            if self.replay_shells() && matches!(state, 30 | 31 | 42..=45) {
                self.burst = shell::ammo(a).min(program::burst_limit(shell::shell_type(a)));
            }
            if self.local && state == program::SPRINT {
                native!(0x283238, a, 0, 0x1180, 0);
                native!(0x2e9b7c, a, 2000, 0);
                movement::steer(a);
            }
        }
        self.play_step_at(resumed.then_some(56));
    }

    fn finish(&mut self) {
        self.cursor = None;
        self.poise = 0.;
        if self.local {
            unsafe { native!(0x2b6e5c, self.actor) };
        }
    }

    unsafe fn update_muzzle(&mut self, state: Option<u8>, dt: f32, visible: bool) {
        unsafe {
            if self.steam > 0. {
                self.steam = if self.local {
                    shell::cooldown(self.actor).max(0.)
                } else {
                    (self.steam - dt).max(0.)
                };
                self.next_steam = (self.next_steam - dt).max(0.);
                if self.steam > 0. && visible {
                    if self.next_steam == 0. {
                        self.muzzle.start_steam(self.fx_owner);
                        self.next_steam = 74.; // Four delayed layers, 37 FZ ticks in all.
                    }
                    self.muzzle.update(self.actor);
                    return;
                }
                self.next_steam = 0.;
                self.muzzle.stop();
            } else if visible && matches!(state, Some(3 | 37 | 38 | 47 | 49)) {
                self.muzzle.update(self.actor);
            } else {
                self.muzzle.stop();
            }
        }
    }

    unsafe fn cooldown_steam(&mut self) {
        // 31/6 or 31/8 follows the visible muzzle while Wyvern Fire cools.
        // Peers get the initial steam without reading an unreplicated timer.
        self.steam = if self.local {
            unsafe { shell::cooldown(self.actor) }.max(0.)
        } else {
            74.
        };
        self.next_steam = 0.;
        unsafe { self.update_muzzle(None, 0., true) };
    }

    fn play_step(&mut self) {
        self.play_step_at(None);
    }

    fn play_step_at(&mut self, start: Option<u16>) {
        let Some(cursor) = self.cursor.as_mut() else {
            return;
        };
        let Some(step) = program::step(cursor.state, cursor.time.step) else {
            self.finish();
            return;
        };
        if step.op == 2 {
            if !self.local {
                self.cursor = None;
            } else {
                // Stay on this step if GU defers the native transition.
                self.transition(step.clip, unsafe {
                    movement::live_relative_stick_yaw(self.actor)
                });
            }
            return;
        }
        if step.op == 0 {
            self.finish();
            return;
        }
        if matches!(cursor.state, 47 | 49) && cursor.time.step == 1 {
            self.windup = program::timer(program::FIRST_CHAMBER);
        }
        let start = start.unwrap_or(step.start);
        let Some(packed) = (unsafe { motion::play(self.actor, step.clip, start, step.blend) })
        else {
            self.finish();
            return;
        };
        if matches!(cursor.state, 47 | 49) && cursor.time.step == 0 {
            self.muzzle.start_charge(self.fx_owner);
        }
        self.packed = packed;
        cursor.time.cycles = 0;
        cursor.time.frame = f32::from(start);
        cursor.time.blend = f32::from(step.blend / 2);
        self.motion_fx.seek(step.clip, cursor.time.frame);
        unsafe {
            sound::update(
                &mut self.sound,
                self.actor,
                Some(step.clip),
                cursor.time.frame,
                Some(cursor.time.frame),
            )
        };
        self.events();
    }

    fn wyvern_fire(&mut self, state: u8) {
        if self.fired {
            return;
        }
        unsafe {
            if self.replay_shells() {
                let kind = match state {
                    50 | 60 | 61 => 1,
                    73 | 74 => 3,
                    _ => 0,
                };
                let cooldown = self.local.then(|| {
                    program::fire_cooldown(shell::shell_type(self.actor), self.chambered, kind == 3)
                });
                if !shell::wyvern_fire(self.actor, kind, 1., cooldown) {
                    return;
                }
            }
            fx::muzzle_flash(self.actor, true);
            self.fired = true;
            self.chambered = 0;
            self.quick_reload = 0.;
        }
    }

    fn native_step(&mut self, state: u8, bits: u32, yaw: Option<u16>, dt: f32) -> bool {
        let actor = self.actor;
        let phase = self.cursor.as_ref().unwrap().time.step;
        unsafe {
            match state {
                3 | 37 | 38 if phase == 1 => {
                    self.windup += dt;
                    if self.windup >= program::timer(program::WYVERN_WINDUP) {
                        let turn = self.aimed_yaw as u16;
                        if self.begin(match state {
                            3 => 59,
                            37 => 60,
                            _ => 61,
                        }) {
                            movement::turn(actor, turn);
                            return true;
                        }
                    }
                    if let Some(yaw) = yaw {
                        let side = match yaw {
                            0x2000..=0x6000 => -1,
                            0xa000..=0xe000 => 1,
                            _ => 0,
                        };
                        self.aimed_yaw =
                            (self.aimed_yaw + side * (82. * dt) as i32).clamp(-8192, 8192);
                    }
                    if bits & (1 << 6) != 0
                        && self.transition(if state == 3 { 104 } else { 105 }, yaw)
                    {
                        return true;
                    }
                }
                47 | 49 if phase == 1 => {
                    if bits & (1 << 6) != 0 && !self.overchambered {
                        if self.chambered < 8 && shell::consume(actor, 1) != 0 {
                            self.chambered += 1;
                            self.windup = program::timer(program::NEXT_CHAMBER);
                            fx::chamber(actor, true);
                            fx::casings(actor, 1);
                        } else {
                            self.overchambered = true;
                            self.windup = 0.;
                        }
                    }
                    self.windup = (self.windup - dt).max(0.);
                    if self.windup == 0. {
                        return self.begin(match (state, self.overchambered) {
                            (47, true) => 73,
                            (47, false) => 48,
                            (_, true) => 74,
                            (_, false) => 50,
                        });
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn fire_burst(&mut self, state: u8) {
        let actor = self.actor;
        unsafe {
            self.burst_started = true;
            let spread = matches!(state, 43 | 45);
            let sequential = matches!(state, 31 | 44 | 45);
            let (kind, variant) = if spread { (3, 0) } else { (2, 0) };
            // Wide simultaneous bursts divide their shells among four
            // contacts. GU accepts at most three requests per update.
            let groups = if sequential { 1 } else { 4 };
            for remaining in (1..=groups).rev() {
                if self.burst == 0 || !shell::available(actor) {
                    break;
                }
                let rounds = if spread && !sequential {
                    self.burst.div_ceil(remaining)
                } else {
                    1
                };
                if shell::consume(actor, rounds) == 0 {
                    self.burst = 0;
                    break;
                }
                if shell::queue(actor, kind, variant, f32::from(rounds))
                    && (sequential || !self.fired)
                {
                    shell::feedback(actor, kind);
                    sound::full_burst(actor);
                    self.fired = true;
                }
                self.burst -= rounds;
            }
            self.quick_reload = 0.;
        }
    }

    fn events(&mut self) {
        let actor = self.actor;
        let replay_shells = self.replay_shells();
        while let Some(cursor) = self.cursor.as_mut()
            && let Some((kind, value, late)) = cursor.next_event()
        {
            let state = cursor.state;
            unsafe {
                let span = (2. * f32::from(value) - late).max(0.);
                match kind {
                    1 => {
                        native!(
                            0x282fd8,
                            actor,
                            u32::from(value & 2),
                            u32::from(value & 1),
                            0
                        );
                    }
                    2 => {
                        native!(0x2a2560, actor, u32::from(value != 0));
                    }
                    3 if self.local => self.poise = span,
                    4 if self.local => self.poise = f32::INFINITY,
                    6 | 7 if self.local => {
                        let bonus = if kind == 7 {
                            evade_bonus_frames(actor)
                        } else {
                            0.
                        };
                        write(actor, 11072, span + bonus);
                    }
                    9 if self.local => {
                        movement::stamina(actor, -150., false);
                        movement::defer_stamina_recovery(actor);
                    }
                    10 => {
                        if let Some(row) = program::strike(value)
                            && let Some(window) =
                                HitWindow::new(row.start, row.duration, cursor.time.frame, late)
                        {
                            let heat_blade = self.heat_blade > 0.;
                            let mut flags = if heat_blade { 0 } else { Attack::DEFLECTABLE };
                            if cursor.state == program::DRAW {
                                // GU w09 HDP 7 uses -30 KO on the draw thrust.
                                flags |= Attack::AFFINITY | Attack::PUNISHING_DRAW;
                            }
                            Attack {
                                template: 1,
                                power: row.power,
                                delay: window.delay,
                                duration: window.duration,
                                flags,
                                shape: 0,
                                no_sharpness_cost: u32::from(heat_blade),
                            }
                            .submit_with_hitstop(
                                actor,
                                mhfz_sdk::combat::attacker_hitstop(row.source_hitstop),
                            );
                        }
                    }
                    12 => {
                        self.reload_rounds = if replay_shells {
                            let missing = shell::capacity(actor).saturating_sub(shell::ammo(actor));
                            shell::reload(actor, true);
                            self.quick_reload = 0.;
                            missing
                        } else {
                            // Older peers still get the animation and its cue.
                            1
                        };
                    }
                    13 => {
                        self.reload_rounds = if replay_shells {
                            let missing = shell::can_reload(actor);
                            shell::reload(actor, false);
                            u8::from(missing)
                        } else {
                            1
                        };
                    }
                    14 if replay_shells => self.rounds = shell::consume(actor, 1),
                    15 if replay_shells => {
                        self.quick_reload = program::timer(program::QUICK_RELOAD)
                    }
                    16 => {
                        self.heat_blade = program::timer(program::IGNITION);
                        self.ignition_cooldown = 0.;
                        fx::ignition(actor);
                        sound::ignition(actor);
                    }
                    17 | 18 => {
                        let angle = match state {
                            12 | 25 | 35 | 54 | 57 => 1,
                            13 | 26 | 36 | 55 | 58 => 2,
                            _ => 0,
                        };
                        let charged = kind == 18;
                        if replay_shells {
                            if self.rounds == 0 {
                                continue;
                            }
                            let variant = match (charged, angle) {
                                (true, 2) => 2,
                                (true, _) => 1,
                                (false, 0) if self.quick_reload > 0. => 3,
                                _ => 0,
                            };
                            let fired = shell::queue(actor, angle, variant, 1.);
                            self.rounds = 0;
                            self.quick_reload = 0.;
                            if !fired {
                                continue;
                            }
                            if angle == 0 {
                                shell::feedback(actor, angle);
                            } else {
                                native!(0x2a063c, actor, 17, 1024);
                            }
                        }
                        if angle != 0 {
                            fx::muzzle_flash(actor, charged || shell::shell_type(actor) != 0);
                        }
                    }
                    19 if replay_shells => {
                        shell::fire(actor, 2, 0, 1.);
                    }
                    21 if self.local => mhfz_sdk::camera::start(actor, value),
                    23 if self.heat_blade > 0. => self.flame.start(actor, self.fx_owner),
                    26 => fx::casings(actor, core::mem::take(&mut self.reload_rounds)),
                    27 => self.cooldown_steam(),
                    _ => {}
                }
            }
        }
    }

    fn advance(&mut self, input: Option<&Input>, bits: u32, yaw: Option<u16>, dt: f32) {
        let actor = self.actor;
        let Some(cursor) = self.cursor.as_mut() else {
            return;
        };
        let state = cursor.state;
        let Some(step) = program::step(state, cursor.time.step) else {
            self.finish();
            return;
        };
        if step.op == 2 {
            self.play_step();
            return;
        }
        let sample = unsafe { MotionFrame::sample(actor) };
        let previous = cursor.time.frame;
        let wrapped = cursor.time.observe(sample.frame, sample.blend);
        let phase = cursor.time.step;
        unsafe {
            // 81207560 starts the blade at frame 100 on both cycles of 1427.
            if step.clip == 1427 && previous < 100. && sample.frame >= 100. {
                self.flame.start(actor, self.fx_owner);
            }
            match (state, phase) {
                (3 | 37 | 38, 0) => {
                    // FZ 81206EFE reads the three thresholds at 824F05AC + 8.
                    let at = if state == 3 { 42. } else { 22. };
                    if previous < at && sample.frame >= at {
                        self.muzzle.start_charge(self.fx_owner);
                        self.muzzle.update(actor);
                    }
                }
                (47 | 49, 0) if previous < 32. && sample.frame >= 32. => {
                    // 812031BA ejects a casing while the first shell is inserted.
                    fx::casings(actor, 1);
                    sound::first_chamber(actor);
                }
                (47 | 49, 1) => {
                    // Selector 8 of FZ bank 4004 runs every three FZ ticks
                    // while additional shells can be loaded.
                    if dt > 0. {
                        self.next_vapour -= dt;
                        if self.next_vapour <= 0. {
                            fx::chamber(actor, false);
                            self.next_vapour = 6.;
                        }
                    }
                }
                (48 | 50 | 59..=61, _) if previous < 44. && sample.frame >= 44. => {
                    self.cooldown_steam();
                }
                _ => {}
            }
            sound::update(&mut self.sound, actor, Some(step.clip), sample.frame, None);
            fx::update(&mut self.motion_fx, actor, step.clip, sample);
        }
        self.events();
        match state {
            30 | 31 | 42..=45
                if self.replay_shells() && (sample.frame >= 50. || self.burst_started) =>
            {
                self.fire_burst(state);
            }
            48 | 50 => self.wyvern_fire(state),
            59..=61 if sample.frame >= 2. => self.wyvern_fire(state),
            73 | 74 if phase == 1 && sample.frame >= 30. => self.wyvern_fire(state),
            _ => {}
        }
        if let Some(input) = input {
            if self.native_step(state, bits, yaw, dt) {
                return;
            }
            let heat_blade = self.heat_blade > 0.;
            let eligible = |condition| program::eligible(condition, heat_blade);
            let cursor = self.cursor.as_mut().unwrap();
            if cursor.art_window(eligible) && unsafe { action::activate_hunter_art(actor) } {
                self.cursor = None;
                return;
            }
            if unsafe { melee::take_deflection(actor) } && self.begin(program::RECOIL) {
                unsafe { melee::clear_hitstop(actor) };
                return;
            }
            let cursor = self.cursor.as_mut().unwrap();
            if let Some(dest) = cursor.branch(bits, eligible)
                && self.transition(dest, yaw)
            {
                return;
            }
            if state == program::SPRINT {
                unsafe {
                    if movement::step_off_ledge(actor, 0) {
                        self.resume.set(true);
                        return;
                    }
                    if input.command(SHEATHE) && action::transition_to(actor, 1, 2) {
                        return;
                    }
                    if sample.frame >= 30. && sample.blend <= 0. {
                        // FZ 81207FE8 handles the drawn run's own follow-ups.
                        let next = if bits & 1 != 0 {
                            Some(66)
                        } else if bits & (1 << 5) != 0 {
                            Some(76)
                        } else if bits & (1 << 3) != 0 {
                            Some(75)
                        } else if bits & (1 << 6) != 0 {
                            Some(if !self.can_fire() {
                                78
                            } else if self.heat_blade > 0. {
                                79
                            } else {
                                77
                            })
                        } else if bits & (1 << 8) != 0 && shell::can_reload(actor) {
                            Some(67)
                        } else {
                            None
                        };
                        if let Some(next) = next
                            && self.begin(next)
                        {
                            return;
                        }
                        if bits & (1 << 11) != 0 && action::transition_to(actor, 32, 0) {
                            return;
                        }
                    }
                    if (!movement::steer(actor) || read::<f32>(actor, 5088) <= 0.)
                        && sprint::can_brake(
                            sample.frame,
                            self.cursor.as_ref().unwrap().time.cycles,
                        )
                        && self.begin(program::BRAKE)
                    {
                        return;
                    }
                    if dt > 0. {
                        movement::drain_running_stamina(actor);
                    }
                }
            }
        }
        let cursor = self.cursor.as_mut().unwrap();
        if self.local && self.poise > 0. {
            unsafe { write(actor, 10288, read::<f32>(actor, 10288).max(4.)) };
        }
        if step.complete(cursor.time, sample, wrapped) && self.burst == 0 {
            cursor.time.step += 1;
            self.play_step();
        }
    }

    unsafe fn can_fire(&self) -> bool {
        unsafe {
            shell::ammo(self.actor) != 0
                && (hunter::sharpness_grade(self.actor) > 0 || self.heat_blade > 0.)
        }
    }

    fn transition(&mut self, dest: u16, yaw: Option<u16>) -> bool {
        let actor = self.actor;
        unsafe {
            match dest {
                82 => {
                    let accepted = action::transition_to(actor, 1, 6);
                    if accepted {
                        self.cursor = None;
                    }
                    return accepted;
                }
                83 | 84 => {
                    let run = yaw.is_some() && read::<f32>(actor, 5088) > 0.;
                    return self.begin(if run { program::SPRINT } else { program::BRAKE });
                }
                88 | 89 => return action::transition_to(actor, 32, 0),
                90 => return action::transition_to(actor, 1, 0),
                _ => {}
            }
            let sharp = hunter::sharpness_grade(actor) > 0 || self.heat_blade > 0.;
            let state = program::destination(
                dest,
                self.can_fire(),
                shell::can_reload(actor),
                self.heat_blade > 0.,
                shell::shell_type(actor),
                sharp && shell::cooled(actor),
                sharp && self.ignition_cooldown == 0.,
            );
            if (85..=87).contains(&dest) && yaw.is_some() {
                let turn = if (yaw.unwrap() as i16) < 0 {
                    5461
                } else {
                    -5461
                };
                if self.begin(state) {
                    movement::turn(actor, turn as u16);
                    return true;
                }
                return false;
            }
            self.begin(state)
        }
    }

    fn tick_ignition(&mut self, dt: f32) {
        let cooling_time = (dt - self.heat_blade).max(0.);
        if self.heat_blade > 0. && dt >= self.heat_blade {
            self.ignition_cooldown = program::timer(program::IGNITION_COOLDOWN);
        }
        self.heat_blade = (self.heat_blade - dt).max(0.);
        self.ignition_cooldown = (self.ignition_cooldown - cooling_time).max(0.);
    }

    fn tick(&mut self, input: &Input) -> bool {
        let actor = self.actor;
        unsafe {
            let dt = read::<f32>(actor, 28).max(0.);
            let yaw = movement::live_relative_stick_yaw(actor);
            self.tick_ignition(dt);
            self.quick_reload = (self.quick_reload - dt).max(0.);
            self.poise = (self.poise - dt).max(0.);
            let sprint = self.flick.update(input, dt);
            let current = action::current(actor);
            let (group, slot) = current;
            // 27CA28 resolves R+X (0x80) and R+A (0x100) before seeing a
            // third key on the next poll. Keep that poll available if the
            // pair did not already start a move or leave this action.
            let completing_guard_pair =
                self.guard_pair == Some(current) && attack_chord_pressed(actor);
            self.guard_pair = (read::<u32>(actor, 5188) & 0x180 != 0).then_some(current);
            let buttons = commands_pressed(actor, input, yaw, sprint, completing_guard_pair);
            let area = read(actor, 4180);
            let same_area = self.area == area;
            let state = action::custom_id(group, slot);
            if !same_area || state.is_none() {
                self.flame.stop();
            } else {
                self.flame.update(actor, dt);
            }
            self.area = area;
            self.update_muzzle(state, dt, same_area && read::<u8>(actor, 10057) & 1 != 0);
            if let Some(state) = state {
                if self.cursor.as_ref().is_none_or(|c| c.state != state)
                    || read::<u8>(actor, 5) == 0
                {
                    self.start(state);
                } else if read::<u16>(actor, 1204) == self.packed {
                    self.advance(Some(input), buttons, yaw, dt);
                } else {
                    self.finish();
                }
                return true;
            }
            let was_sprint = self
                .cursor
                .take()
                .is_some_and(|c| c.state == program::SPRINT);
            let drawn = native!(0x2a2444, actor) != 0;
            let ground = movement::ground_idle((group, slot));
            self.resume
                .on_native(actor, (group, slot), ground, drawn, was_sprint);
            let clip = motion::source(actor, read(actor, 1204));
            let sample = MotionFrame::sample(actor);
            sound::update(&mut self.sound, actor, clip, sample.frame, None);
            if let Some(clip) = clip {
                fx::update(&mut self.motion_fx, actor, clip, sample);
            }
            // FZ evasion uses 812080B4 after the hop's initial
            // movement. Let the native GU hop provide that movement first.
            if drawn && (group, slot) == (1, 6) && sample.frame >= 30. {
                if buttons & (1 << 8 | 1 << 6) != 0 {
                    return self.transition(87, yaw);
                }
                if buttons & (1 << 5) != 0 {
                    return self.transition(86, yaw);
                }
                if buttons & (1 << 3) != 0 {
                    return self.transition(85, yaw);
                }
                if buttons & 1 != 0 {
                    return self.begin(match yaw {
                        Some(0xa000..=0xffff) => 64,
                        Some(0..=0x6000) => 65,
                        _ => 63,
                    });
                }
            }
            let guarding = matches!((group, slot), (32, 0 | 1 | 9));
            if !(ground || guarding) || !drawn {
                return false;
            }
            if action::activate_hunter_art(actor) {
                return true;
            }
            if guarding && buttons & 2 != 0 {
                return self.begin(66);
            }
            if guarding && buttons & (1 << 8) != 0 && shell::can_reload(actor) {
                return self.begin(5);
            }
            if guarding && buttons & ((1 << 3) | (1 << 5)) != 0 {
                return self.begin(2);
            }
            if !guarding && buttons & (1 << 8) != 0 && shell::can_reload(actor) {
                return self.begin(67);
            }
            if buttons & (1 << 7) != 0 {
                return self.transition(101, yaw);
            }
            if buttons & (1 << 12) != 0 {
                return self.transition(106, yaw);
            }
            if buttons & (1 << 6) != 0 {
                return self.transition(91, yaw);
            }
            if sprint && read::<f32>(actor, 5088) > 0.
                || self
                    .resume
                    .ready(yaw.is_some(), read::<f32>(actor, 5088) > 0.)
            {
                return self.begin(program::SPRINT);
            }
            if buttons & (1 << 5) != 0 {
                return self.begin(4);
            }
            if buttons & (1 << 4) != 0 {
                return self.begin(6);
            }
            if buttons & (1 << 3) != 0 {
                return self.begin(0);
            }
            false
        }
    }
}

fn commands_pressed(
    actor: u32,
    input: &Input,
    yaw: Option<u16>,
    sprint: bool,
    completing_guard_pair: bool,
) -> u32 {
    let evade = input.command(3) && unsafe { read::<f32>(actor, 5088) >= 150. };
    let r = input.command(27);
    let x = input.weapon_command(0) || input.weapon_command(1);
    let a = input.weapon_command(2);
    let guard_shot = input.weapon_command(3) || (r && input.weapon_command(24));
    let guard_thrust = r && (input.weapon_command(7) || input.command(16));
    let rising = input.weapon_command(5);
    // GU may emit ordinary X+A when the guard shoulder is already held.
    let wyvern_fire =
        (r && (rising || completing_guard_pair)) || unsafe { attack_chord_with_shoulder(actor) };
    let mut bits =
        u32::from(evade) | (u32::from(evade && yaw.is_some()) << 1) | (u32::from(sprint) << 2);
    if wyvern_fire {
        bits |= 1 << 7;
    } else if guard_shot {
        bits |= 1 << 8;
    } else if rising || guard_thrust {
        bits |= 1 << 5;
    } else if a {
        bits |= 1 << 6;
    } else if x {
        bits |= 1 << 3;
        if yaw.is_some() {
            bits |= 1 << 4;
        }
    }
    if r && !wyvern_fire && !guard_thrust && !guard_shot {
        bits |= (1 << 9) | (1 << 11);
    }
    if input.command(8) {
        bits |= 1 << 12;
    }
    bits
}

pub unsafe extern "C" fn dispatch(actor: u32, input: *const Input) -> u32 {
    if BUSY.swap(true, Ordering::Acquire) {
        return PASS;
    }
    let mut handled = false;
    unsafe {
        let saved = &mut *LOCAL.0.get();
        if let Some(input) = input.as_ref() {
            let equipment = read(actor, 5060);
            if saved
                .as_ref()
                .is_none_or(|b| (b.actor, b.equipment) != (actor, equipment))
            {
                *saved = Some(Battle {
                    actor,
                    equipment,
                    local: true,
                    ..Battle::default()
                });
            }
            handled = saved.as_mut().unwrap().tick(input);
        } else if let Some(old) = saved.take() {
            if old.actor == actor && actor != 0 {
                let (group, slot) = action::current(actor);
                if action::custom_id(group, slot).is_some() {
                    native!(0x2b6e5c, actor);
                }
            }
            mhfz_sdk::camera::reset();
        }
        let hud = saved.as_ref().map(|battle| IgnitionHud {
            owner: (battle.actor, battle.equipment),
            active: battle.heat_blade,
            cooldown: battle.ignition_cooldown,
        });
        IGNITION_HUD.with(|current| *current = hud);
    }
    BUSY.store(false, Ordering::Release);
    if handled { 0 } else { PASS }
}

pub unsafe extern "C" fn request(actor: u32, raw: *const ActionRequest) -> u32 {
    let request = unsafe { &*raw };
    let (group, current) = unsafe { action::current(actor) };
    if request.draw_route {
        return if request.slot == 3 {
            u32::from(program::DRAW)
        } else {
            PASS
        };
    }
    if unsafe { native!(0x2a2444, actor) } == 0 && request.slot == 3 {
        return u32::from(program::DRAW);
    }
    // GU 11AFFE4 requests the tier's Wyvern Fire at the end of the
    // AA Flare lift. 11B0244 owns firing, barrel feedback and recovery.
    if group == action::GROUP && matches!(current, 98..=100) && request.slot == current + 37 {
        return NATIVE_ACTION;
    }
    if matches!(request.slot, 50 | 58 | 94 | 65..=67 | 77..=82 | 95..=97)
        || (group == action::GROUP
            && matches!(current, 65..=89 | 95..=103)
            && matches!(request.slot, 65..=89 | 95..=103))
    {
        NATIVE_ACTION
    } else {
        PASS
    }
}

// GU sends its action before the first animation event consumes or reloads shells.
unsafe extern "C" fn encode_ammo(actor: u32) -> u8 {
    let (group, slot) = unsafe { action::current(actor) };
    if action::custom_id(group, slot).is_some() {
        unsafe { shell::ammo(actor) }
    } else {
        u8::MAX
    }
}

unsafe extern "C" fn decode_ammo(actor: u32, count: u8) {
    unsafe {
        if read::<u8>(actor, 5) != 0 {
            return;
        }
        let (group, slot) = action::current(actor);
        let Some(state) = action::custom_id(group, slot) else {
            return;
        };
        let Some(index) = Roster::current().and_then(|r| r.peer_index(actor)) else {
            return;
        };
        PEERS.update(index, actor, |saved, _| {
            let battle = saved.get_or_insert_with(|| Battle {
                actor,
                fx_owner: index + 1,
                ..Battle::default()
            });
            battle.received_ammo = (count != u8::MAX).then_some((state, count));
        });
    }
}

pub(super) static PLAYBACK: remote::Playback = remote::Playback {
    tick: peer,
    observe: Some(observe),
    action_state: Some(remote::ActionState {
        encode: encode_ammo,
        decode: decode_ammo,
    }),
};

unsafe extern "C" fn peer(actor: u32, state: u8) {
    unsafe {
        let Some(index) = Roster::current().and_then(|r| r.peer_index(actor)) else {
            return;
        };
        if !program::ACTIONS.contains(state)
            || hunter::equipped_weapon(actor) != Some(crate::WEAPON)
            || !hunter::local()
                .is_some_and(|local| read::<u8>(local, 4180) == read::<u8>(actor, 4180))
        {
            return;
        }
        PEERS.update(index, actor, |state_slot, _| {
            let battle = state_slot.get_or_insert_with(|| Battle {
                actor,
                fx_owner: index + 1,
                ..Battle::default()
            });
            if battle
                .cursor
                .as_ref()
                .is_none_or(|cursor| cursor.state != state)
                || read::<u8>(actor, 5) == 0
            {
                battle.start(state);
            } else if read::<u16>(actor, 1204) == battle.packed {
                let dt = read::<f32>(actor, 28).max(0.);
                battle.advance(None, 0, None, dt);
            }
        });
    }
}

unsafe extern "C" fn observe(local: u32) {
    unsafe {
        PEERS.visit_live(crate::WEAPON, |_, actor, battle| {
            let elapsed = read::<f32>(actor, 28).max(0.);
            battle.heat_blade = (battle.heat_blade - elapsed).max(0.);
            battle.quick_reload = (battle.quick_reload - elapsed).max(0.);
            let (group, slot) = action::current(actor);
            let state = action::custom_id(group, slot);
            let same_area = read::<u8>(actor, 4180) == read::<u8>(local, 4180);
            battle.update_muzzle(
                state,
                elapsed,
                same_area && read::<u8>(actor, 10057) & 1 != 0,
            );
            if state.is_none() || !same_area {
                battle.flame.stop();
                battle.cursor = None;
                if !same_area {
                    battle.quick_reload = 0.;
                }
            } else {
                battle.flame.update(actor, elapsed);
            }
        });
    }
}
