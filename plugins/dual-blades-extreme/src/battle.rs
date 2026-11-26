//! Local hunter state. GU owns transitions, hits, movement and Hunter Arts.
mod maw;
mod peer;
pub(super) use peer::PLAYBACK;
mod sharpen;

use crate::{
    fx, motion, native,
    program::{self, AERIAL_DANCE, AERIAL_FINISH, AERIAL_MOUNT_FINISH, BRAKE, SPRINT},
    read, sound, write,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicBool, AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use mhfz_sdk::{
    action::{Cursor, Time},
    combat::{DrawnEvade, HitWindow, bladescale_evade, evade_bonus_frames, knockback_recovery},
    sound::Clock,
    sprint::{self, Flick, Resume},
};
use mhgu_sdk::weapon::style::{
    ActionRequest, Input, NATIVE_ACTION, PASS,
    action::{
        self, Attack,
        command::{BufferedCommand, CommandInput, CommandSet},
        melee::{self, BLADE_CONTACT_OFFSETS},
    },
    input::{
        attack_buttons_pressed, attack_chord_pressed, attack_chord_with_shoulder,
        attack_combo_pending,
    },
    movement::{self, CLIMBS, stamina},
};

const DRAW_SLASH: u8 = 16;
const DASH_SLASH: u8 = 82;
const NATIVE_AERIAL_SLOTS: &[u16] = &[31, 32, 33, 66, 67];
const DEFLECTION: u8 = 11;
static SPRINT_TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, CLIMBS);

// 817E4840 (forward) and 817E386C (side) pass these windows to 811F0CC6.
const fn roll_attack(execute: i16) -> BufferedCommand {
    BufferedCommand {
        group: action::GROUP,
        slot: action::slot(6),
        input: CommandInput::Common(X_PRESSED),
        accept_from: 10,
        execute_from: execute,
        accept_until: execute,
    }
}
// GU table 2025 can queue 16:11 ahead of the Frontier follow-up on the same X press.
static FORWARD_ROLL: CommandSet = CommandSet::new(&[roll_attack(44)]).replacing_weapon(11);
static SIDE_ROLL: CommandSet = CommandSet::new(&[roll_attack(42)]).replacing_weapon(11);

pub(super) unsafe extern "C" fn bind_commands(actor: u32, table: u32) -> *const CommandSet {
    let (group, slot) = unsafe { action::current(actor) };
    match (table, group, slot) {
        (2000, action::GROUP, slot) if slot == action::slot(SPRINT) => &SPRINT_TERRAIN,
        (2025, 1, 6) if unsafe { native!(0x2a2444, actor) } != 0 => {
            match unsafe { native!(0x53a058, actor) } {
                28 => &FORWARD_ROLL,
                5012 | 5013 => &SIDE_ROLL,
                _ => core::ptr::null(),
            }
        }
        _ => core::ptr::null(),
    }
}
// 11BAEF0 maps buffered X to 0, A to 1/2, and X+A to 11.
const X: u8 = 0;
const A: u8 = 1;
const A_BACK: u8 = 2;
const X_AND_A: u8 = 11;
const SPECIAL: u8 = 3;
const X_PRESSED: u8 = 30;
const PAD_X: u16 = 0x20;
const PAD_A: u16 = 0x100;
const EVADE_STAMINA_COST: f32 = 150.;

const CMD_SPRINT: u16 = 1;
const CMD_SLASH_DOWN: u16 = 2;
const CMD_COMBO: u16 = 5;
const CMD_SWEEP: u16 = 9;
const CMD_SHOULDER_HELD: u16 = 18;

fn single_attack_commands(buttons: u16, relative_yaw: u16) -> u32 {
    let mut bits = 0;
    if buttons & PAD_X != 0 {
        bits |= 1 << CMD_SLASH_DOWN;
        let direction = direction(relative_yaw);
        if direction >= 2 {
            bits |= 1 << (direction + 1);
        }
    }
    if buttons & PAD_A != 0 {
        bits |= 1 << CMD_SWEEP;
        // FZ 811EE87C picks command 8 in stick sector 0x800, or 9 otherwise.
        if (0xa000..0xe000).contains(&relative_yaw) {
            bits |= 1 << 8;
        }
    }
    bits
}

static PHASE: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static FLAME: AtomicU32 = AtomicU32::new(0);
static MONSTER_HIT: AtomicBool = AtomicBool::new(false);

fn reset_local_state() {
    sharpen::clear_active_level();
    PHASE.store(0, Relaxed);
    FLAME.store(0, Relaxed);
    maw::clear();
}

fn add_flame(amount: u32) {
    let _ = FLAME.try_update(Relaxed, Relaxed, |points| {
        (points < program::FLAME_MAX).then(|| points.saturating_add(amount).min(program::FLAME_MAX))
    });
}

pub(super) fn hud_state(actor: u32) -> (u8, u32, u32) {
    if OWNER.load(Relaxed) == actor {
        (phase(actor), FLAME.load(Relaxed), sharpen::active_level())
    } else {
        (0, 0, 0)
    }
}

fn phase(actor: u32) -> u8 {
    if OWNER.load(Relaxed) == actor {
        PHASE.load(Relaxed) as u8
    } else {
        0
    }
}

fn demon_phase(actor: u32, slot: usize) -> u8 {
    if slot == 0 {
        phase(actor)
    } else {
        peer::bits(slot) & 3
    }
}

fn flame_full() -> bool {
    FLAME.load(Relaxed) == program::FLAME_MAX
}

#[derive(Clone, Copy)]
struct Commands {
    bits: u32,
    direction: usize,
    relative_yaw: u16,
    waiting_press: u16,
    press_time: Option<Time>,
}

impl Commands {
    fn has(self, id: u16) -> bool {
        self.bits & (1 << id) != 0
    }
}

struct AttackPress {
    buttons: u16,
    yaw: u16,
    time: Time,
}

#[derive(Default)]
struct Battle {
    peer_slot: usize,
    owner: u32,
    equipment: u32,
    cursor: Option<Cursor>,
    packed: u16,
    flick: Flick,
    elapsed: f32,
    shoulder_used_by_chord: bool,
    chord_pending: bool,
    attack_press: Option<AttackPress>,
    activation_phase: u8,
    finish_window: f32,
    finish_requested: bool,
    hit: bool,
    deflected: bool,
    evade_yaw: Option<u16>,
    resume_sprint: Resume,
    keep_poise: bool,
    keep_evade: bool,
    polish: sharpen::Polish,
    health_clock: f32,
    flame_subframe: f32,
    effects: fx::Effects,
    audio: Clock,
}

impl Battle {
    unsafe fn set_phase(&mut self, actor: u32, phase: u8) {
        unsafe {
            if self.peer_slot != 0 {
                peer::set_phase(self.peer_slot, actor, phase);
                return;
            }
            self.health_clock = 0.;
            if phase < 2 {
                self.flame_subframe = 0.;
                FLAME.store(0, Relaxed);
            }
            let previous = PHASE.swap(u32::from(phase), Relaxed);
            if previous == 1 && phase >= 2 {
                // A held R can upgrade the phase within the same action.
                native!(0x2a25f0, actor, 1);
            }
            // Native weapon queries are also used by the GU art handlers.
            let flags = read::<u8>(actor, 13168) & !3;
            write(
                actor,
                13168,
                flags | u8::from(phase != 0) | (u8::from(phase >= 2) << 1),
            );
        }
    }

    unsafe fn commands(&mut self, actor: u32, input: &Input) -> Commands {
        unsafe {
            let relative_yaw = movement::live_relative_stick_yaw(actor).unwrap_or(0);
            let direction = direction(relative_yaw);
            let mut bits = 0;
            if input.command(3) && movement::has_evade_stamina(actor, EVADE_STAMINA_COST) {
                bits |= 1 | 1 << [22, 23, 24, 25][direction];
            }
            if self.flick.update(input, self.elapsed) && read::<f32>(actor, 5088) > 0. {
                bits |= 1 << CMD_SPRINT;
            }
            let chord = input.weapon_command(X_AND_A)
                || attack_chord_pressed(actor)
                || attack_chord_with_shoulder(actor);
            let pressed = attack_buttons_pressed(actor);
            let waiting = attack_combo_pending(actor);
            // A chord completed against an already-held button can still
            // produce a delayed single-button command from GU.
            let suppress_single = self.chord_pending && pressed == 0;
            self.chord_pending = waiting && (chord || suppress_single);
            let mut resolved = 0;
            let shoulder_held = input.command(27);
            self.shoulder_used_by_chord = shoulder_held && (chord || self.shoulder_used_by_chord);
            if chord
                && shoulder_held
                && let Some(cursor) = &mut self.cursor
            {
                // R may have queued a higher-priority follow-up one tick before
                // GU resolves it as part of X+A.
                cursor.discard_pending_command(CMD_SHOULDER_HELD);
            }
            if input.command(8) {
                bits |= 1 << 13;
            } else if chord {
                bits |= 1 << CMD_COMBO;
                if direction >= 2 {
                    bits |= 1 << (direction + 4);
                }
            } else {
                // Wait for 27CA28 to resolve X or X+A before changing attacks.
                if !suppress_single {
                    if input.weapon_command(X) {
                        resolved |= PAD_X;
                    }
                    if input.weapon_command(A) || input.weapon_command(A_BACK) {
                        resolved |= PAD_A;
                    }
                }
                if input.weapon_command(SPECIAL) {
                    bits |= 1 << 13;
                }
            }
            let press = if resolved != 0 || !waiting || chord {
                self.attack_press
                    .take()
                    .filter(|press| press.buttons & resolved != 0)
            } else {
                None
            };
            bits |= single_attack_commands(
                resolved,
                press.as_ref().map_or(relative_yaw, |press| press.yaw),
            );
            if input.command(13) {
                bits |= 1 << 10;
            }
            // w06 command 27 is the attack-edge follow-up from recoil.
            if bits & ((1 << CMD_SLASH_DOWN) | (1 << CMD_SWEEP) | (1 << CMD_COMBO)) != 0 {
                bits |= 1 << 27;
            }
            // Frontier 811EE87C tests the level for both held and released.
            // An early tap must still count as released when its window opens.
            if input.command(70) && !chord {
                bits |= 1 << 19;
            }
            if !shoulder_held {
                bits |= 1 << 20;
            } else if !self.shoulder_used_by_chord {
                bits |= 1 << CMD_SHOULDER_HELD;
            }
            Commands {
                bits,
                direction,
                relative_yaw,
                waiting_press: if waiting && !chord { pressed } else { 0 },
                press_time: press.map(|press| press.time),
            }
        }
    }

    unsafe fn clear_action(&mut self, actor: u32) {
        self.cursor = None;
        self.attack_press = None;
        self.effects.clear_action();
        self.resume_sprint.set(false);
        if self.peer_slot == 0 {
            MONSTER_HIT.store(false, Relaxed);
        }
        self.deflected = false;
        self.keep_evade = false;
        self.keep_poise = false;
        self.polish.leave_action();
        for contact in BLADE_CONTACT_OFFSETS {
            unsafe { write(actor, contact + 5, 0u8) };
        }
    }

    unsafe fn finish(&mut self, actor: u32) {
        unsafe {
            self.clear_action(actor);
            if self.peer_slot == 0 {
                native!(0x2b6e5c, actor);
            }
        }
    }

    unsafe fn begin(&mut self, actor: u32, state: u8) -> bool {
        unsafe { self.begin_facing(actor, state, None) }
    }

    unsafe fn begin_facing(&mut self, actor: u32, state: u8, facing: Option<u16>) -> bool {
        unsafe {
            if !action::accept_custom_start(actor, state) {
                return false;
            }
            if let Some(yaw) = facing {
                movement::set_facing(actor, yaw);
            }
            self.clear_action(actor);
            self.cursor = Some(program::cursor(state));
            self.evade_yaw = None;
            self.hit = false;
            self.finish_window = 0.;
            self.finish_requested = false;
            if state == 18 {
                self.activation_phase = phase(actor);
            }
            write(actor, 5, 1u8);
            for contact in BLADE_CONTACT_OFFSETS {
                write(actor, contact + 176, 0u32);
            }
            let was_sheathed = native!(0x2a2444, actor) == 0;
            let aerial = program::aerial_motion(state);
            let sprint = state == SPRINT;
            native!(
                0x282fd8,
                actor,
                if aerial.is_some() { 2 } else { 0 },
                u32::from(!sprint),
                0
            );
            if was_sheathed && matches!(state, DRAW_SLASH | 62 | 67 | DASH_SLASH | AERIAL_MOUNT_FINISH) {
                native!(0x2a264c, actor, 1);
            } else {
                native!(0x2a2560, actor, 1);
            }
            if sprint && self.peer_slot == 0 {
                native!(0x283238, actor, 0, 0x180, 0);
                native!(0x2e9b7c, actor, 2000, 0);
            }
            if sprint || matches!(state, 36..=40 | 73..=77 | 83..=88) {
                // GU's ground collision uses the roll flag to select 4:14 or 4:23 at a ledge.
                native!(0x283238, actor, 0, 0x1000, 0);
            }
            if let Some(aerial) = aerial.filter(|_| self.peer_slot == 0) {
                let yaw = f32::from(read::<u16>(actor, 4076)) * (core::f32::consts::TAU / 65536.);
                let (x, z) = libm::sincosf(yaw);
                let speed = aerial.forward_speed;
                write(actor, 4112, [x * speed, aerial.vertical_speed, z * speed]);
                write(actor, 4128, [0., aerial.vertical_acceleration, 0.]);
                write(actor, 10057, read::<u8>(actor, 10057) | 0x40);
            }
            self.start_step(actor)
        }
    }

    unsafe fn start_step(&mut self, actor: u32) -> bool {
        unsafe {
            let Some(cursor) = &mut self.cursor else {
                return false;
            };
            match program::step(cursor.state, cursor.time.step) {
                Some(step) if matches!(step.op, 3..=5) => {
                    if let Some(packed) = motion::play(actor, step.clip, step.start, step.blend) {
                        // Draw into release (67) has no drawn-weapon event in
                        // the source table. Its second clip is the drawn 1448.
                        if cursor.state == 67 && step.clip == 1448 {
                            native!(0x2a2560, actor, 1);
                        }
                        self.packed = packed;
                        cursor.time.cycles = 0;
                        cursor.time.frame = f32::from(step.start);
                        cursor.time.blend = f32::from(step.blend / 2);
                        let frame = f32::from(step.start);
                        self.effects.seek(step.clip, frame);
                        sound::update(&mut self.audio, actor, Some(step.clip), frame, Some(frame));
                        self.events(actor);
                        return true;
                    }
                }
                Some(step) if step.op == 2 && self.peer_slot == 0 => {
                    return self.begin(actor, step.clip as u8);
                }
                _ => {}
            }
            self.finish(actor);
            true
        }
    }

    unsafe fn events(&mut self, actor: u32) {
        unsafe {
            while let Some(cursor) = &mut self.cursor
                && let Some((kind, parameter, late)) = cursor.next_event()
            {
                let state = cursor.state;
                let frames = (2. * f32::from(parameter) - late).max(0.);
                match kind {
                    1 => {
                        native!(
                            0x282fd8,
                            actor,
                            u32::from(parameter & 2),
                            u32::from(parameter & 1),
                            0
                        );
                    }
                    2 => {
                        native!(0x2a2560, actor, u32::from(parameter != 0));
                    }
                    3 if self.peer_slot == 0 => movement::turn(actor, parameter),
                    4 => {
                        self.keep_poise = false;
                        write(actor, 10288, frames);
                    }
                    5 => self.keep_poise = true,
                    6 | 7 => {
                        self.keep_evade = false;
                        let extra = if kind == 7 {
                            evade_bonus_frames(actor)
                        } else {
                            0.
                        };
                        let remaining = (2. * f32::from(parameter) + extra - late).max(0.);
                        write(actor, 11072, read::<f32>(actor, 11072).max(remaining));
                    }
                    8 => self.keep_evade = true,
                    9 if self.peer_slot == 0 => {
                        stamina(actor, -EVADE_STAMINA_COST, false);
                        if matches!(state, 36..=40 | 73..=75 | 83..=88) {
                            // GU w11 11C17DC uses category 6 on entry to its demon step.
                            bladescale_evade(actor, DrawnEvade::DemonStep);
                        }
                    }
                    11 | 12 => submit_strike(actor, state, parameter, late, kind == 12),
                    13 => {
                        let phase = parameter as u8;
                        self.set_phase(actor, phase);
                        let full = if self.peer_slot == 0 {
                            flame_full()
                        } else {
                            peer::bits(self.peer_slot) & 4 != 0
                        };
                        self.effects.demon(actor, phase, full, 0.);
                    }
                    14 if self.peer_slot == 0 => {
                        native!(0x2ad288, actor, u32::from(parameter), 1);
                    }
                    15 => self.polish.add_stack(),
                    16 => {
                        self.effects.sharpen(actor, !self.polish.active());
                        sound::sharpen(actor);
                        self.polish.activate(2. * f32::from(parameter));
                    }
                    18 => self.effects.sharpen_slash(actor, parameter),
                    19 if demon_phase(actor, self.peer_slot) == 3 => fx::evade(actor),
                    _ => {}
                }
            }
        }
    }

    unsafe fn evade(&mut self, actor: u32, yaw: u16, chain: bool) -> bool {
        unsafe {
            let direction = direction(yaw);
            let state = match phase(actor) {
                0 | 1 => {
                    if !motion::ROLL.enter(actor, yaw) {
                        return false;
                    }
                    self.clear_action(actor);
                    return true;
                }
                2 => [36, 37, 38, 39][direction],
                _ if chain => [85, 86, 87, 88][direction],
                _ => [83, 73, 74, 75][direction],
            };
            self.begin(actor, state)
        }
    }

    unsafe fn branch(&mut self, actor: u32, command: Commands, target: u16) -> bool {
        const SIDE_TURNS: [u16; 3] = [0, 0x2000, 0xe000];
        unsafe {
            let phase = phase(actor);
            let mut turn_by = None;
            let state = match target {
                0..=88 => target as u8,
                89 | 90 => {
                    let yaw = self.evade_yaw.unwrap_or(command.relative_yaw);
                    return self.evade(actor, yaw, target == 90);
                }
                91 => SPRINT,
                92..=95 => attack_variant(phase, target),
                // FZ 811EF340 skips the turn when dispatching the normal sweep.
                96..=98 if phase == 0 => 15,
                96..=98 => {
                    turn_by = Some(SIDE_TURNS[usize::from(target - 96)]);
                    21
                }
                99 | 103 if phase > 0 => 21,
                100..=102 => {
                    turn_by = Some(SIDE_TURNS[usize::from(target - 100)]);
                    15
                }
                104 => {
                    if phase > 0 {
                        18
                    } else {
                        66
                    }
                }
                105 if self.activation_phase == 1 => 68,
                105 if self.activation_phase == 2 => 69,
                _ => return false,
            };
            let facing = turn_by.map(|offset| read::<u16>(actor, 4076).wrapping_add(offset));
            program::ACTIONS.contains(state) && self.begin_facing(actor, state, facing)
        }
    }

    unsafe fn slide_off_ledge(&mut self, actor: u32, clip: u16) -> bool {
        // These root motions move relative to the hunter's facing.
        let offset = match clip {
            1426 => 0,
            1429 | 1447 => 0x8000,
            1427 | 1445 => 0x4000,
            1428 | 1446 => 0xc000,
            _ => return false,
        };
        if unsafe { movement::step_off_ledge(actor, offset) } {
            unsafe { self.clear_action(actor) };
            true
        } else {
            false
        }
    }

    unsafe fn advance(&mut self, actor: u32, commands: Commands) -> bool {
        unsafe {
            let Some(cursor) = &self.cursor else {
                return false;
            };
            let state = cursor.state;
            let Some(step) = program::step(state, cursor.time.step) else {
                self.finish(actor);
                return true;
            };
            if self.deflected && self.begin(actor, DEFLECTION) {
                melee::clear_hitstop(actor);
                return true;
            }
            if self.elapsed > 0. && self.slide_off_ledge(actor, step.clip) {
                return true;
            }
            let cursor = self.cursor.as_mut().unwrap();
            let sample = action::MotionFrame::sample(actor);
            let frame = sample.frame;
            sound::update(&mut self.audio, actor, Some(step.clip), frame, None);
            if MONSTER_HIT.swap(false, Relaxed) {
                self.hit = true;
                if state == AERIAL_DANCE {
                    self.finish_window = program::finish_window();
                }
            }
            let wrapped = cursor.time.observe(frame, sample.blend);
            self.effects.motion_events(actor, step.clip, sample);
            self.events(actor);

            if self.keep_poise {
                write(actor, 10288, read::<f32>(actor, 10288).max(8.));
            }
            if self.keep_evade {
                write(actor, 11072, read::<f32>(actor, 11072).max(4.));
            }
            if self.polish.active() && matches!(state, 0..=6 | 8 | 12..=17 | 20..=23 | 43 | 54 | 55)
            {
                // Frontier applies 128 angle units per 30 Hz logic frame.
                let angle = (64. * self.elapsed) as u16;
                match commands.direction {
                    2 => movement::turn(actor, angle),
                    3 => movement::turn(actor, angle.wrapping_neg()),
                    _ => {}
                }
            }

            let aerial = program::aerial_motion(state);
            if let Some(aerial) = &aerial {
                // 811EF80C starts integrating the opening leap at frame 50.
                if step.clip != 1450 || frame >= 50. {
                    native!(0x539f4c, actor);
                    if read::<f32>(actor, 4116) < 0. && action::land(actor) {
                        return self.begin(actor, aerial.landing);
                    }
                }
                if self.finish_window > 0. {
                    self.finish_requested |= commands.has(CMD_SWEEP);
                    if self.finish_requested && self.begin(actor, AERIAL_FINISH) {
                        return true;
                    }
                    self.finish_window = (self.finish_window - self.elapsed).max(0.);
                    self.finish_requested &= self.finish_window > 0.;
                }
                if step.op == 5 && read::<u8>(actor, 1206) & 4 != 0 {
                    return self.start_step(actor);
                }
            }

            let cursor = self.cursor.as_mut().unwrap();
            let finished = if step.op == 3 && step.until != 0 {
                // The w06 side slide (1445) has an authored early exit at 46.
                step.reached_frame(frame)
            } else {
                step.complete(cursor.time, sample, wrapped)
            };
            // 811EF80C advances the motion program before checking branches.
            // Let an authored follow-up see the next clip on the transition tick.
            let advance_before_input = finished
                && program::step(state, cursor.time.step + 1)
                    .is_some_and(|next| matches!(next.op, 3..=5));
            if advance_before_input {
                cursor.time.step += 1;
                self.start_step(actor);
            }
            let Some(cursor) = &mut self.cursor else {
                return true;
            };
            if commands.waiting_press != 0 {
                self.attack_press = Some(AttackPress {
                    buttons: commands.waiting_press,
                    yaw: commands.relative_yaw,
                    time: cursor.time,
                });
            }

            let mode = phase(actor);
            let vitality = read::<i16>(actor, 5078) > 1;
            let sustained = can_sustain(actor, mode);
            let eligible = |id| match id {
                0 | 1 | 15 => true,
                2 => mode == 0,
                3 => mode == 1,
                4 => mode == 2,
                5 => mode == 3,
                6 | 22 => mode > 0,
                7 => mode >= 2,
                9 => mode <= 2,
                8 => mode <= 1,
                10 | 20 => mode != 0 && sustained,
                11..=13 => sustained && mode == (id - 10) as u8,
                17 => matches!(self.activation_phase, 1 | 2),
                23 => self.hit,
                _ => false,
            };
            if (cursor.art_window(eligible) || state == SPRINT)
                && action::activate_hunter_art(actor)
            {
                self.clear_action(actor);
                return true;
            }
            let target =
                cursor.branch_with_press_time(commands.bits, eligible, |command| match command {
                    2..=4 | 8 | 9 | 27 => commands.press_time,
                    _ => None,
                });
            // Keep a finished action alive while GU resolves a press that
            // was still inside one of its input windows.
            let wait_for_attack = finished
                && self.attack_press.as_ref().is_some_and(|press| {
                    cursor.press_window(
                        single_attack_commands(press.buttons, press.yaw) | (1 << 27),
                        press.time,
                        eligible,
                    )
                });
            if matches!(cursor.pending_action(), Some(89 | 90)) {
                self.evade_yaw.get_or_insert(commands.relative_yaw);
            } else {
                self.evade_yaw = None;
            }
            if let Some(target) = target
                && self.branch(actor, commands, target)
            {
                return true;
            }

            if state == SPRINT {
                let has_direction = movement::steer(actor);
                // FZ 811F0C32 gives the forward evade priority over X and A.
                if commands.has(0) {
                    return match mode {
                        0 | 1 => self.evade(actor, 0, false),
                        2 => self.begin(actor, 40),
                        _ => self.begin(actor, 84),
                    };
                }
                let attack = if commands.has(CMD_SWEEP) {
                    movement::has_evade_stamina(actor, EVADE_STAMINA_COST).then_some(AERIAL_DANCE)
                } else if commands.has(CMD_SLASH_DOWN) {
                    Some(DASH_SLASH)
                } else if commands.has(CMD_COMBO) {
                    // Frontier has no X+A branch from sprint.
                    None
                } else {
                    idle_attack(commands, mode, vitality)
                };
                if let Some(next) = attack {
                    return self.begin(actor, next);
                }
                if commands.has(10) && action::transition_to(actor, 1, 2) {
                    self.clear_action(actor);
                    return true;
                }
                if sprint::can_brake(frame, self.cursor.as_ref().unwrap().time.cycles)
                    && (!has_direction || read::<f32>(actor, 5088) <= 0.)
                {
                    return self.begin(actor, BRAKE);
                }
            }
            if finished && !advance_before_input && !wait_for_attack {
                self.cursor.as_mut().unwrap().time.step += 1;
                self.start_step(actor);
            }
            true
        }
    }

    unsafe fn upkeep(&mut self, actor: u32, mode: u8, sprint: bool, clip: Option<u16>) {
        unsafe {
            // 811EE0E8 suspends upkeep during these activation and release motions.
            let active = !matches!(clip, Some(1018 | 1440 | 1425 | 1442 | 1448));
            if sprint || (active && mode == 1) {
                movement::drain_running_stamina(actor);
            }
            if mode < 2 {
                return;
            }
            // Suppress natural red-health recovery while released, including startup.
            let recovery = read::<f32>(actor, 10140);
            write(actor, 10140, recovery.max(self.elapsed + 1.));
            if !active {
                return;
            }
            // 8181D952 schedules one HP every 24 Frontier ticks (48 GU frames).
            self.health_clock += self.elapsed;
            let ticks = (self.health_clock / 48.) as i32;
            if ticks > 0 {
                self.health_clock %= 48.;
                let hp = i32::from(read::<i16>(actor, 5078));
                let cost = ticks.min((hp - 1).max(0));
                if cost > 0 {
                    native!(0x2a451c, actor, (-cost) as u32, 4);
                }
                if ticks > cost && read::<i16>(actor, 5078) == 1 {
                    let red = i32::from(read::<i16>(actor, 5082));
                    write(actor, 5082, (red - (ticks - cost)).max(1) as i16);
                }
            }
        }
    }

    unsafe fn tick(&mut self, actor: u32, input: &Input) -> bool {
        unsafe {
            if read::<i16>(actor, 5078) <= 0 {
                self.set_phase(actor, 0);
                self.polish = sharpen::Polish::default();
                sharpen::clear_active_level();
                self.cursor = None;
                self.attack_press = None;
                self.chord_pending = false;
                self.effects.clear_action();
                return false;
            }
            self.elapsed = read::<f32>(actor, 28).max(0.);
            let current = action::current(actor);
            motion::finish_blood_wind_frame(actor, current);
            if current != (1, 6) {
                motion::ROLL.clear();
            }
            let drawn = native!(0x2a2444, actor) != 0;
            let custom = action::custom_id(current.0, current.1);
            let sprint = custom == Some(SPRINT);
            if (!drawn && custom.is_none())
                || (current.0 == 64 && phase(actor) != 0 && read::<u8>(actor, 13168) & 1 == 0)
            {
                self.set_phase(actor, 0);
            }
            if custom != self.cursor.as_ref().map(|c| c.state) {
                self.polish.leave_action();
            }
            let in_attack_chain =
                current.0 == action::GROUP && !matches!(custom, Some(SPRINT | BRAKE));
            // 816F27BC suppresses recoil while w06's blade-polish timer is live.
            // The collision occurred before this tick advances or expires the timer.
            if custom.is_some() && melee::take_deflection(actor) && !self.polish.active() {
                self.deflected = true;
            }
            self.polish
                .tick(actor, self.elapsed, drawn, in_attack_chain);
            let mode = phase(actor);
            if drawn && mode >= 2 {
                self.flame_subframe += self.elapsed;
                let ticks = (self.flame_subframe / 2.) as u32;
                if ticks > 0 {
                    self.flame_subframe %= 2.;
                    add_flame(ticks.saturating_mul(program::FLAME_PER_TICK));
                }
            }
            self.effects.demon(
                actor,
                if drawn { mode } else { 0 },
                flame_full(),
                self.elapsed,
            );
            if drawn && (mode != 0 || sprint) && self.elapsed > 0. {
                let clip = motion::source_clip(actor, read(actor, 1204));
                self.upkeep(actor, mode, sprint, clip);
            }
            let commands = self.commands(actor, input);
            if let Some(id) = custom {
                if read::<u8>(actor, 5) == 0 {
                    return self.begin(actor, id);
                }
                if self
                    .cursor
                    .as_ref()
                    .is_some_and(|cursor| cursor.state == id)
                    && read::<u16>(actor, 1204) == self.packed
                {
                    return self.advance(actor, commands);
                }
                // GU retained the custom slot but replaced its playing clip.
                self.finish(actor);
                return true;
            }
            sound::update(
                &mut self.audio,
                actor,
                motion::source_clip(actor, read(actor, 1204)),
                read(actor, 1280),
                None,
            );
            self.attack_press = None;
            let ground = movement::ground_idle(current);
            let was_sprinting = self
                .cursor
                .take()
                .is_some_and(|cursor| cursor.state == SPRINT);
            self.resume_sprint
                .on_native(actor, current, ground, drawn, was_sprinting);
            // FZ w06 states 76 / 77 use the landed launch direction.
            if let Some(state) = knockback_recovery(actor, current, drawn, input, [77, 76]) {
                return self.begin(actor, state);
            }
            if drawn
                && (matches!(current, (4, 14 | 23))
                    || (current.0 == 16 && NATIVE_AERIAL_SLOTS.contains(&current.1)))
                && read::<u8>(actor, 5) != 0
            {
                if mode == 3 && commands.has(CMD_COMBO) {
                    return self.begin(actor, AERIAL_DANCE);
                }
                // GU owns the falling trajectory until contact. The grounded
                // slash and step both reset movement mode in their first event.
                let landed = read::<u8>(actor, 9477) != 2;
                if commands.has(CMD_SLASH_DOWN) {
                    let state = if landed {
                        DASH_SLASH
                    } else {
                        AERIAL_MOUNT_FINISH
                    };
                    return self.begin(actor, state);
                }
                if landed && mode >= 2 && commands.has(0) {
                    return self.begin(actor, if mode == 3 { 84 } else { 40 });
                }
            }
            if !drawn || !ground {
                return false;
            }
            let has_stamina = read::<f32>(actor, 5088) > 0.;
            let resume_sprint = self
                .resume_sprint
                .ready(native!(0x27e608, actor, 0) > 0, has_stamina);
            if action::activate_hunter_art(actor) {
                self.resume_sprint.set(false);
                return true;
            }
            if mode != 0 && !can_sustain(actor, mode) {
                return self.begin(actor, 18);
            }
            if commands.has(0) {
                return self.evade(actor, commands.relative_yaw, false);
            }
            if let Some(state) = idle_attack(commands, mode, read::<i16>(actor, 5078) > 1) {
                return self.begin(actor, state);
            }
            if (commands.has(CMD_SPRINT) || resume_sprint) && has_stamina {
                return self.begin(actor, SPRINT);
            }
            false
        }
    }
}

unsafe fn can_sustain(actor: u32, phase: u8) -> bool {
    unsafe {
        match phase {
            1 => read::<f32>(actor, 5088) > 0.,
            2 | 3 => read::<i16>(actor, 5078) > 1 || read::<i16>(actor, 5082) > 1,
            _ => false,
        }
    }
}

fn direction(relative_yaw: u16) -> usize {
    match relative_yaw {
        0x6000..=0xa000 => 1,
        0x2000..0x6000 => 3,
        0xa001..=0xe000 => 2,
        _ => 0,
    }
}

fn attack_variant(phase: u8, target: u16) -> u8 {
    match phase {
        0 => [12, 14, 4, 13][usize::from(target - 92)],
        3 => 72,
        _ if target < 94 => 20,
        _ => 8,
    }
}

fn idle_attack(commands: Commands, phase: u8, vitality: bool) -> Option<u8> {
    Some(if commands.has(19) {
        match phase {
            0 if vitality => 65,
            0 => 7,
            _ => 18,
        }
    } else if commands.has(CMD_SWEEP) {
        attack_variant(phase, if commands.has(8) { 92 } else { 94 })
    } else if commands.has(CMD_SLASH_DOWN) {
        0
    } else if commands.has(CMD_COMBO) {
        if phase == 0 { 15 } else { 21 }
    } else if commands.has(13) {
        24
    } else {
        return None;
    })
}

// FZ event 12 swaps the hands selected by event 11 for the same strike row.
unsafe fn submit_strike(actor: u32, state: u8, strike_id: u16, late: f32, swap_hands: bool) {
    let Some(row) = program::strike(strike_id) else {
        return;
    };
    let secondary = matches!(row.shape, 10 | 35) ^ swap_hands;
    let template = match (row.shape, secondary) {
        (34 | 35, false) => 62,
        (34 | 35, true) => 61,
        (_, false) => 1,
        (_, true) => 2,
    };
    let frame = unsafe { read::<f32>(actor, 1280) };
    let Some(HitWindow { delay, duration }) = HitWindow::new(row.start, row.duration, frame, late)
    else {
        return;
    };
    let mut flags = 7;
    if matches!(state, DRAW_SLASH | DASH_SLASH) {
        // The paired GU draw and dash hits use -8 KO in w11 HDP 11/12 and 116/117.
        flags |= Attack::punishing_draw(8);
    }
    // 811EF80C enables the hard-contact transition in 811EF762 for these states.
    if matches!(state, 0..=6 | 8 | 12..=17 | 20 | 43 | 54 | 55) {
        flags |= Attack::DEFLECTABLE;
    }
    if secondary {
        flags |= Attack::SECONDARY;
    }
    if state != AERIAL_MOUNT_FINISH {
        flags |= Attack::NO_MOUNT;
    }
    let attack = Attack {
        template,
        power: row.power,
        delay,
        duration,
        flags,
        shape: 0,
        no_sharpness_cost: 0,
    };
    unsafe {
        attack.submit_with_hitstop(actor, mhfz_sdk::combat::attacker_hitstop(row.source_hitstop));
    }
}

struct LocalBattle(UnsafeCell<Option<Battle>>);
// The registry invokes the local weapon controller on the game thread.
unsafe impl Sync for LocalBattle {}
static BATTLE: LocalBattle = LocalBattle(UnsafeCell::new(None));
static DISPATCHING: AtomicBool = AtomicBool::new(false);

pub(super) unsafe extern "C" fn dispatch(actor: u32, raw: *const Input) -> u32 {
    if DISPATCHING.swap(true, Acquire) {
        return PASS;
    }
    let result = unsafe { dispatch_frame(actor, raw) };
    DISPATCHING.store(false, Release);
    result
}

unsafe fn dispatch_frame(actor: u32, raw: *const Input) -> u32 {
    unsafe {
        let battle = &mut *BATTLE.0.get();
        let Some(input) = raw.as_ref() else {
            if actor != 0
                && let Some(old) = battle.as_mut().filter(|old| old.owner == actor)
            {
                old.set_phase(actor, 0);
                let (group, slot) = action::current(actor);
                if action::custom(group, slot) {
                    old.finish(actor);
                }
            }
            *battle = None;
            motion::ROLL.clear();
            OWNER.store(0, Relaxed);
            reset_local_state();
            return PASS;
        };
        let equipment = read(actor, 5060);
        if battle
            .as_ref()
            .is_none_or(|old| old.owner != actor || old.equipment != equipment)
        {
            motion::ROLL.clear();
            reset_local_state();
            OWNER.store(actor, Relaxed);
            *battle = Some(Battle {
                owner: actor,
                equipment,
                ..Battle::default()
            });
        }
        if battle.as_mut().unwrap().tick(actor, input) {
            0
        } else {
            PASS
        }
    }
}

pub(super) unsafe extern "C" fn request(actor: u32, raw: *const ActionRequest) -> u32 {
    unsafe {
        let request = &*raw;
        let mode = phase(actor);
        if NATIVE_AERIAL_SLOTS.contains(&request.slot) {
            let buttons = read::<u32>(actor, 5156);
            if buttons & (1 << X_AND_A) != 0
                || attack_chord_pressed(actor)
                || attack_chord_with_shoulder(actor)
            {
                if mode == 3 {
                    return u32::from(AERIAL_DANCE);
                }
            } else if attack_combo_pending(actor) {
                return PASS;
            } else if buttons & (1 << X) != 0 {
                return u32::from(AERIAL_MOUNT_FINISH);
            }
            return NATIVE_ACTION;
        }
        if native!(0x2a2444, actor) != 0 {
            return PASS;
        }
        match request.slot {
            7..=9 => u32::from(DRAW_SLASH),
            10 | 13 | 14 => 67,
            73 => u32::from(DASH_SLASH),
            _ => PASS,
        }
    }
}

#[forge::hook(offset = 0x11c1454)]
unsafe fn native_demon_gauge(actor: u32, amount: u32, kind: u32) -> u32 {
    if crate::local_actor() == Some(actor) {
        return 0;
    }
    original!(actor, amount, kind)
}

fn blade_hand(actor: u32, hit: u32) -> Option<usize> {
    let offset = hit.wrapping_sub(actor);
    BLADE_CONTACT_OFFSETS
        .iter()
        .position(|&blade| blade == offset)
}

// GU records a resolved monster contact once per target. Polling the two hand
// slots on the next controller tick would lose successive contacts in a frame.
#[forge::hook(offset = 0x169d44)]
unsafe fn record_contact(hit: u32, target: u32, kind: u32) -> u32 {
    let actor = OWNER.load(Relaxed);
    if actor != 0
        && kind == 1
        && target != 0
        && let Some(hand) = blade_hand(actor, hit)
    {
        let (group, slot) = unsafe { action::current(actor) };
        if action::custom(group, slot) {
            MONSTER_HIT.store(true, Relaxed);
        }
        if phase(actor) >= 2 {
            add_flame(program::FLAME_PER_HIT);
        }
        unsafe { maw::recover_on_contact(actor, hit, hand) };
    }
    original!(hit, target, kind)
}

#[forge::hook(offset = 0x11bad38)]
unsafe fn adjust_strike(actor: u32, hit: u32, row: u32, flags: u32) -> u32 {
    unsafe {
        // 2B76CC calls this with the HDP row pointer and flags.
        let local = OWNER.load(Relaxed) == actor;
        let peer = if local { None } else { peer::slot_of(actor) };
        if (!local && peer.is_none()) || flags & 1 == 0 {
            return original!(actor, hit, row, flags);
        }
        let Some(hand) = blade_hand(actor, hit) else {
            return original!(actor, hit, row, flags);
        };
        if local {
            maw::clear_contact(hand);
        }
        let power = read::<i16>(hit, 56);
        let result = original!(actor, hit, row, flags);
        let (group, slot) = action::current(actor);
        let Some(state) = action::custom_id(group, slot) else {
            return result;
        };
        maw::arm(actor, hand);
        if program::aerial_motion(state).is_some() {
            // 2B76CC adds secondary buildup to the aerial templates.
            write(hit, 86, 0u8);
            write(hit, 89, 0u8);
        }
        let bits = peer.map_or_else(|| phase(actor) | (u8::from(flame_full()) << 2), peer::bits);
        let mode = bits & 3;
        if mode != 0 {
            // 81DBDE7A / 81DBE87E snapshot the motion value at hit creation.
            let factor = if mode >= 2 && bits & 4 != 0 {
                1.45
            } else {
                1.35
            };
            write(hit, 56, (f32::from(power) * factor) as i16);
        }
        result
    }
}

#[forge::hook(offset = 0x11c11d0)]
unsafe fn native_demon_upkeep(actor: u32) -> u32 {
    if phase(actor) != 0 {
        // The active branch's remaining work is the shared weapon timer.
        // Frontier's controller owns the cost and the depletion transition.
        return unsafe { native!(0x53aa84, actor, actor + 13144) };
    }
    original!(actor)
}

#[forge::hook(offset = 0x11c12ec)]
unsafe fn native_demon_status(actor: u32) -> u32 {
    // GU's demon status update blocks passive stamina recovery. Keep any
    // block already set by a move such as sprinting.
    if phase(actor) < 2 || unsafe { read::<u8>(actor, 10057) } & 2 != 0 {
        return original!(actor);
    }
    let result = original!(actor);
    unsafe { native!(0x2a25f0, actor, 1) };
    result
}

pub(super) fn install() {
    let base = forge::mem::text_addr();
    forge::install_hook!(base, native_demon_gauge);
    forge::install_hook!(base, record_contact);
    maw::install(base);
    forge::install_hook!(base, adjust_strike);
    forge::install_hook!(base, native_demon_upkeep);
    forge::install_hook!(base, native_demon_status);
    unsafe { sharpen::install() };
}
