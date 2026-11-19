//! Local hunter state. GU owns transitions, hits, movement and Hunter Arts.
mod sharpen;

use crate::{
    fx, motion, native,
    program::{self, AERIAL_DANCE, AERIAL_FINISH, AERIAL_MOUNT_FINISH, BRAKE, Cursor, SPRINT},
    read, sound, write,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicBool, AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use mhgu_sdk::style::{
    ActionRequest, Input, NATIVE_ACTION, PASS,
    action::{
        self, Attack,
        command::{BufferedCommand, CommandInput, CommandSet},
    },
    sound::Clock,
};

const DASH_SLASH: u8 = 82;
const NATIVE_AERIAL_SLOTS: &[u16] = &[31, 32, 33, 66, 67];
const CONTACTS: [u32; 2] = [12320, 12504];
const CLIMBS: &[u16] = &[137, 138, 139];
const ROLL_OFF_LEDGE: (u16, u16) = (4, 14);
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
static FORWARD_ROLL: CommandSet = CommandSet::new(&[roll_attack(44)]);
static SIDE_ROLL: CommandSet = CommandSet::new(&[roll_attack(42)]);

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

const CMD_SPRINT: u16 = 1;
const CMD_SLASH_DOWN: u16 = 2;
const CMD_COMBO: u16 = 5;
const CMD_SWEEP: u16 = 9;
const STICK_DEADZONE: i16 = 48;

static PHASE: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static FLAME: AtomicU32 = AtomicU32::new(0);
static MONSTER_HIT: AtomicBool = AtomicBool::new(false);

fn reset_gauges() {
    sharpen::clear_active_level();
    PHASE.store(0, Relaxed);
    FLAME.store(0, Relaxed);
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

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global frontier_db_stamina",
    ".hidden frontier_db_stamina",
    ".type frontier_db_stamina,%function",
    "frontier_db_stamina:",
    "vmov s0,r1",
    "mov r1,#0",
    "bx r2",
);
unsafe extern "C" {
    fn frontier_db_stamina(actor: u32, amount: u32, entry: u32);
}

unsafe fn stamina(actor: u32, amount: f32, continuous: bool) {
    let offset = if continuous { 0x2a5ce4 } else { 0x2a5ebc };
    unsafe { frontier_db_stamina(actor, amount.to_bits(), forge::mem::text_addr() + offset) }
}

#[derive(Default)]
struct Flick {
    direction: u16,
    remaining: f32,
    released: bool,
    deflected: bool,
}

impl Flick {
    fn update(&mut self, input: &Input, elapsed: f32) -> bool {
        // Frontier 81ECC418 gives the first push and the return to neutral
        // separate 12-tick windows. GU measures elapsed time in 60 Hz frames.
        const WINDOW: f32 = 24.;
        self.remaining = (self.remaining - elapsed).max(0.);
        if input.stick_strength < STICK_DEADZONE {
            if self.deflected && self.remaining > 0. {
                self.remaining = WINDOW;
            }
            self.deflected = false;
            self.released = true;
            return false;
        }
        if input.stick_strength < 80 || self.deflected || !self.released {
            return false;
        }
        self.deflected = true;
        let angle = input.stick_angle;
        let doubled = self.remaining > 0.
            && (angle.wrapping_sub(self.direction) as i16).unsigned_abs() <= 0x3000;
        self.remaining = if doubled { 0. } else { WINDOW };
        self.direction = angle;
        doubled
    }
}

#[derive(Clone, Copy)]
struct Commands {
    bits: u32,
    direction: usize,
    relative_yaw: u16,
    stick_yaw: Option<u16>,
}

impl Commands {
    fn has(self, id: u16) -> bool {
        self.bits & (1 << id) != 0
    }
}

#[derive(Default)]
struct Battle {
    owner: u32,
    equipment: u32,
    cursor: Option<Cursor>,
    packed: u16,
    flick: Flick,
    elapsed: f32,
    activation_phase: u8,
    finish_window: f32,
    finish_requested: bool,
    hit: bool,
    evade_yaw: Option<u16>,
    resume_sprint: bool,
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

    unsafe fn commands(&mut self, actor: u32, input: &Input, chaining: bool) -> Commands {
        unsafe {
            // GU's 27F678 also sets the target facing. Only sample the stick
            // here, including while an attack or a native action is running.
            let stick_yaw =
                (input.stick_strength >= STICK_DEADZONE).then(|| native!(0x27f5d0, actor) as u16);
            let facing = read::<u16>(actor, 4076);
            let yaw = stick_yaw.unwrap_or(facing);
            // Frontier 8172C2D4 measures the stick from the hunter as facing minus stick yaw.
            let relative_yaw = facing.wrapping_sub(yaw);
            let direction = direction(relative_yaw);
            let mut bits = 0;
            if input.command(3) && read::<f32>(actor, 5088) >= 150. {
                bits |= 1 | 1 << [22, 23, 24, 25][direction];
                self.evade_yaw = Some(relative_yaw);
            }
            if self.flick.update(input, self.elapsed) && read::<f32>(actor, 5088) > 0. {
                bits |= 1 << CMD_SPRINT;
            }
            if input.command(8) {
                bits |= 1 << 13;
            } else if input.weapon_command(X_AND_A) {
                bits |= 1 << CMD_COMBO;
                if direction >= 2 {
                    bits |= 1 << (direction + 4);
                }
            } else {
                // Use GU's press event in the attack chain. The later weapon
                // command would replay the same press after combo resolution.
                let slash = if chaining {
                    input.command(X_PRESSED)
                } else {
                    input.weapon_command(X)
                };
                if slash {
                    bits |= 1 << CMD_SLASH_DOWN;
                    if direction >= 2 {
                        bits |= 1 << (direction + 1);
                    }
                }
                if input.weapon_command(A) || input.weapon_command(A_BACK) {
                    bits |= 1 << CMD_SWEEP;
                    if direction != 2 {
                        bits |= 1 << 8;
                    }
                }
                if input.weapon_command(SPECIAL) {
                    bits |= 1 << 13;
                }
            }
            if input.command(13) {
                bits |= (1 << 10) | (1 << 27);
            }
            // Frontier 811EE87C tests the level for both held and released.
            // An early tap must still count as released when its window opens.
            if input.command(70) {
                bits |= 1 << 19;
            }
            bits |= 1 << if input.command(27) { 18 } else { 20 };
            Commands {
                bits,
                direction,
                relative_yaw,
                stick_yaw,
            }
        }
    }

    unsafe fn clear_action(&mut self, actor: u32) {
        self.cursor = None;
        self.resume_sprint = false;
        MONSTER_HIT.store(false, Relaxed);
        self.keep_evade = false;
        self.keep_poise = false;
        self.polish.leave_action();
        for contact in CONTACTS {
            unsafe { write(actor, contact + 5, 0u8) };
        }
    }

    unsafe fn finish(&mut self, actor: u32) {
        unsafe {
            self.clear_action(actor);
            native!(0x2b6e5c, actor);
        }
    }

    unsafe fn begin(&mut self, actor: u32, state: u8) -> bool {
        unsafe {
            let requested = action::current(actor) == (action::GROUP, action::slot(state))
                && read::<u8>(actor, 5) == 0;
            if !requested && !action::transition_to(actor, action::GROUP, action::slot(state)) {
                return false;
            }
            self.clear_action(actor);
            self.cursor = Some(Cursor::new(state));
            self.evade_yaw = None;
            self.hit = false;
            self.finish_window = 0.;
            self.finish_requested = false;
            if state == 18 {
                self.activation_phase = phase(actor);
            }
            write(actor, 5, 1u8);
            for contact in CONTACTS {
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
            if was_sheathed && matches!(state, 16 | 62 | 67 | DASH_SLASH | AERIAL_MOUNT_FINISH) {
                native!(0x2a264c, actor, 1);
            } else {
                native!(0x2a2560, actor, 1);
            }
            if sprint {
                native!(0x283238, actor, 0, 0x180, 0);
                native!(0x2e9b7c, actor, 2000, 0);
            }
            if sprint || matches!(state, 36..=40 | 73..=77 | 83..=88) {
                // GU's ground collision uses the roll flag to select 4:14 or 4:23 at a ledge.
                native!(0x283238, actor, 0, 0x1000, 0);
            }
            if let Some(aerial) = aerial {
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
                        sound::update(&mut self.audio, actor, Some(step.clip), frame, Some(frame));
                        self.events(actor);
                        return true;
                    }
                }
                Some(step) if step.op == 2 => return self.begin(actor, step.clip as u8),
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
                    3 => turn(actor, parameter),
                    4 => {
                        self.keep_poise = false;
                        write(actor, 10288, frames);
                    }
                    5 => self.keep_poise = true,
                    6 | 7 => {
                        self.keep_evade = false;
                        let extra = if kind == 7 {
                            match native!(0x2a69dc, actor) as i32 {
                                2.. => 10.,
                                1 => 6.,
                                _ => 0.,
                            }
                        } else {
                            0.
                        };
                        let remaining = (2. * f32::from(parameter) + extra - late).max(0.);
                        write(actor, 11072, read::<f32>(actor, 11072).max(remaining));
                    }
                    8 => self.keep_evade = true,
                    9 => stamina(actor, -150., false),
                    11 | 12 => {
                        if let Some(row) = program::strike(parameter) {
                            let other = matches!(row.shape, 10 | 35) ^ (kind == 12);
                            let template = match (row.shape, other) {
                                (34 | 35, false) => 62,
                                (34 | 35, true) => 61,
                                (_, false) => 1,
                                (_, true) => 2,
                            };
                            let distance = if row.start == 0 {
                                -late
                            } else {
                                f32::from(row.start) - read::<f32>(actor, 1280)
                            };
                            let delay = distance.max(0.);
                            let duration = f32::from(row.duration) + distance.min(0.);
                            if duration > 0. {
                                let mut flags = 7;
                                if other {
                                    flags |= Attack::SECONDARY;
                                }
                                if state != AERIAL_MOUNT_FINISH {
                                    flags |= Attack::NO_MOUNT;
                                }
                                Attack {
                                    template,
                                    power: row.power,
                                    delay,
                                    duration,
                                    flags,
                                    shape: 0,
                                    no_sharpness_cost: 0,
                                }
                                .submit(actor);
                            }
                        }
                    }
                    13 => {
                        self.set_phase(actor, parameter as u8);
                        self.effects.demon(actor, phase(actor), 0.);
                    }
                    14 => {
                        native!(0x2ad288, actor, u32::from(parameter), 1);
                    }
                    15 => self.polish.add_stack(),
                    16 => {
                        self.effects.sharpen(actor, !self.polish.active());
                        sound::sharpen(actor);
                        self.polish.activate(2. * f32::from(parameter));
                    }
                    18 => fx::sharpen_slash(actor),
                    19 if phase(actor) == 3 => fx::evade(actor),
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
                    motion::remember_roll(actor, yaw);
                    if !action::transition_to(actor, 1, 6) {
                        motion::clear_roll();
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
        unsafe {
            let phase = phase(actor);
            let state = match target {
                0..=88 => target as u8,
                89 | 90 => {
                    let yaw = self.evade_yaw.take().unwrap_or(command.relative_yaw);
                    return self.evade(actor, yaw, target == 90);
                }
                91 => SPRINT,
                92..=95 => attack_variant(phase, target),
                96..=98 => {
                    turn(actor, [0, 0x2000, 0xe000][usize::from(target - 96)]);
                    if phase == 0 { 15 } else { 21 }
                }
                99 | 103 if phase > 0 => 21,
                100..=102 => {
                    turn(actor, [0, 0x2000, 0xe000][usize::from(target - 100)]);
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
            program::ACTIONS.contains(state) && self.begin(actor, state)
        }
    }

    unsafe fn roll_off_ledge(&mut self, actor: u32, resume_sprint: bool) -> bool {
        unsafe {
            let (group, slot) = ROLL_OFF_LEDGE;
            native!(0x2831e0, actor, u32::from(group), u32::from(slot), 0xc);
            if action::current(actor) != ROLL_OFF_LEDGE {
                return false;
            }
            self.clear_action(actor);
            self.resume_sprint = resume_sprint;
            true
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
        unsafe {
            let facing = read::<u32>(actor, 4076);
            let yaw = (facing as u16).wrapping_add(offset);
            let airborne = read::<u8>(actor, 9477) == 2;
            if !airborne {
                // GU's ledge probe caches a result for the current facing.
                if offset != 0 {
                    write(actor, 4076, u32::from(yaw));
                    write(actor, 11956, 0u32);
                }
                let ledge = native!(0x2a8458, actor) == 1;
                if offset != 0 {
                    write(actor, 4076, facing);
                    write(actor, 11956, 0u32);
                }
                if !ledge {
                    return false;
                }
            }
            let target_yaw = read::<u32>(actor, 5280);
            set_facing(actor, yaw);
            if self.roll_off_ledge(actor, false) {
                return true;
            }
            write(actor, 4076, facing);
            write(actor, 5280, target_yaw);
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
            if self.elapsed > 0. && self.slide_off_ledge(actor, step.clip) {
                return true;
            }
            let cursor = self.cursor.as_mut().unwrap();
            let frame = read::<f32>(actor, 1280);
            sound::update(&mut self.audio, actor, Some(step.clip), frame, None);
            if MONSTER_HIT.swap(false, Relaxed) {
                self.hit = true;
                if state == AERIAL_DANCE {
                    self.finish_window = program::finish_window();
                }
            }
            if frame < cursor.time.frame {
                cursor.time.cycles = cursor.time.cycles.saturating_add(1);
            }
            cursor.time.frame = frame;
            cursor.time.blend = read(actor, 1268);
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
                    2 => turn(actor, angle),
                    3 => turn(actor, angle.wrapping_neg()),
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

            let mode = phase(actor);
            let vitality = read::<i16>(actor, 5078) > 1;
            let sustained = can_sustain(actor, mode);
            let enough_stamina = read::<f32>(actor, 5088) >= 150.;
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
            let cursor = self.cursor.as_mut().unwrap();
            if (cursor.art_window(eligible) || state == SPRINT)
                && action::transition(actor, action::Transition::HUNTER_ART)
            {
                self.clear_action(actor);
                return true;
            }
            if let Some(target) = cursor.branch(commands.bits, eligible)
                && self.branch(actor, commands, target)
            {
                return true;
            }

            if state == SPRINT {
                if let Some(yaw) = commands.stick_yaw {
                    set_facing(actor, yaw);
                    if native!(0x2a8458, actor) == 1 && self.roll_off_ledge(actor, true) {
                        return true;
                    }
                }
                let attack = if commands.has(CMD_SWEEP) {
                    enough_stamina.then_some(AERIAL_DANCE)
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
                if commands.has(0) && enough_stamina {
                    return self.evade(actor, commands.relative_yaw, false);
                }
                if commands.has(10) && action::transition_to(actor, 1, 2) {
                    self.clear_action(actor);
                    return true;
                }
                if commands.stick_yaw.is_none() || read::<f32>(actor, 5088) <= 0. {
                    return self.begin(actor, BRAKE);
                }
            }
            let cursor = self.cursor.as_mut().unwrap();
            let finished = if step.op == 5 {
                aerial.is_none() && step.loops != 0 && cursor.time.cycles >= step.loops
            } else if step.until != 0 {
                frame >= f32::from(step.until)
            } else {
                read::<u8>(actor, 1206) & 4 != 0 || frame >= read::<f32>(actor, 1284)
            };
            if finished {
                cursor.time.step += 1;
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
                native!(0x2a25a8, actor, 1);
                stamina(actor, -1., true);
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
                return false;
            }
            self.elapsed = read::<f32>(actor, 28).max(0.);
            let current = action::current(actor);
            if self.elapsed > 0. {
                motion::finish_blood_wind_frame(actor, current);
            }
            if current != (1, 6) {
                motion::clear_roll();
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
            self.polish.tick(self.elapsed, drawn, in_attack_chain);
            let mode = phase(actor);
            self.effects
                .demon(actor, if drawn { mode } else { 0 }, self.elapsed);
            if drawn && mode >= 2 {
                self.flame_subframe += self.elapsed;
                let ticks = (self.flame_subframe / 2.) as u32;
                if ticks > 0 {
                    self.flame_subframe %= 2.;
                    add_flame(ticks.saturating_mul(program::FLAME_PER_TICK));
                }
            }
            if drawn && (mode != 0 || sprint) && self.elapsed > 0. {
                let clip = motion::source_clip(actor, read(actor, 1204));
                self.upkeep(actor, mode, sprint, clip);
            }
            let chaining = custom.is_some_and(|id| !matches!(id, SPRINT | BRAKE));
            let commands = self.commands(actor, input, chaining);
            if let Some(id) = custom {
                if self
                    .cursor
                    .as_ref()
                    .is_some_and(|cursor| cursor.state == id)
                    && read::<u16>(actor, 1204) == self.packed
                    && read::<u8>(actor, 5) != 0
                {
                    return self.advance(actor, commands);
                }
                return self.begin(actor, id);
            }
            sound::update(
                &mut self.audio,
                actor,
                motion::source_clip(actor, read(actor, 1204)),
                read(actor, 1280),
                None,
            );
            let terrain = sprint_terrain(current);
            let ground = matches!(current, (1, 0 | 46) | (2, 0 | 30));
            if let Some(cursor) = self.cursor.take()
                && cursor.state == SPRINT
            {
                let previous = (read(actor, 9480), read(actor, 9484));
                self.resume_sprint = terrain || (ground && sprint_terrain(previous));
            }
            self.resume_sprint &= drawn && (ground || terrain);
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
            self.resume_sprint &= commands.stick_yaw.is_some() && has_stamina;
            if action::transition(actor, action::Transition::HUNTER_ART) {
                self.resume_sprint = false;
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
            if (commands.has(CMD_SPRINT) || self.resume_sprint) && has_stamina {
                return self.begin(actor, SPRINT);
            }
            false
        }
    }
}

fn sprint_terrain((group, slot): (u16, u16)) -> bool {
    (group == 1 && CLIMBS.contains(&slot)) || matches!((group, slot), (4, 3 | 14 | 23))
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

unsafe fn turn(actor: u32, amount: u16) {
    unsafe {
        let yaw = read::<u16>(actor, 4076).wrapping_add(amount);
        set_facing(actor, yaw);
    }
}

unsafe fn set_facing(actor: u32, yaw: u16) {
    unsafe {
        write(actor, 4076, u32::from(yaw));
        write(actor, 5280, u32::from(yaw));
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
            motion::clear_roll();
            OWNER.store(0, Relaxed);
            reset_gauges();
            return PASS;
        };
        let equipment = read(actor, 5060);
        if battle
            .as_ref()
            .is_none_or(|old| old.owner != actor || old.equipment != equipment)
        {
            motion::clear_roll();
            reset_gauges();
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
            if buttons & (1 << X_AND_A) != 0 {
                if mode == 3 {
                    return u32::from(AERIAL_DANCE);
                }
            } else if buttons & (1 << X) != 0 || read::<u32>(actor, 5140) & (1 << X_PRESSED) != 0 {
                return u32::from(AERIAL_MOUNT_FINISH);
            }
            return NATIVE_ACTION;
        }
        if native!(0x2a2444, actor) != 0 {
            return PASS;
        }
        match request.slot {
            7..=9 => 16,
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

// GU records a resolved monster contact once per target. Polling the two hand
// slots on the next controller tick would lose successive contacts in a frame.
#[forge::hook(offset = 0x169d44)]
unsafe fn record_contact(hit: u32, target: u32, kind: u32) -> u32 {
    let actor = OWNER.load(Relaxed);
    if actor != 0 && kind == 1 && target != 0 && CONTACTS.contains(&hit.wrapping_sub(actor)) {
        let (group, slot) = unsafe { action::current(actor) };
        if action::custom(group, slot) {
            MONSTER_HIT.store(true, Relaxed);
        }
        if phase(actor) >= 2 {
            add_flame(program::FLAME_PER_HIT);
        }
    }
    original!(hit, target, kind)
}

#[forge::hook(offset = 0x11bad38)]
unsafe fn adjust_strike(actor: u32, hit: u32, row: u32, flags: u32) -> u32 {
    unsafe {
        // 2B76CC calls this with the HDP row pointer and flags.
        if OWNER.load(Relaxed) != actor
            || !CONTACTS.contains(&hit.wrapping_sub(actor))
            || flags & 1 == 0
        {
            return original!(actor, hit, row, flags);
        }
        let power = read::<i16>(hit, 56);
        let result = original!(actor, hit, row, flags);
        let (group, slot) = action::current(actor);
        let Some(state) = action::custom_id(group, slot) else {
            return result;
        };
        if program::aerial_motion(state).is_some() {
            // 2B76CC adds secondary buildup to the aerial templates.
            write(hit, 86, 0u8);
            write(hit, 89, 0u8);
        }
        let mode = phase(actor);
        if mode != 0 {
            // 81DBDE7A / 81DBE87E snapshot the motion value at hit creation.
            let factor = if mode >= 2 && FLAME.load(Relaxed) == program::FLAME_MAX {
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
    forge::install_hook!(base, adjust_strike);
    forge::install_hook!(base, native_demon_upkeep);
    forge::install_hook!(base, native_demon_status);
    unsafe { sharpen::install(base) };
}
