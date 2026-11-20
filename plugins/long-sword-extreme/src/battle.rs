//! Local w07 state machine. GU owns accepted transitions, collisions and terrain.
use crate::{
    collision, discharge, fx, instant, motion, native,
    peer::{LOCAL, Slot},
    program::{
        self, BRAKE, CMD_A, CMD_EVADE_BACK, CMD_EVADE_LEFT, CMD_EVADE_RIGHT, CMD_EVADE_SLASH,
        CMD_PARRY, CMD_RELEASE, CMD_SPRINT, CMD_STEP_X, CMD_X, SPRINT,
    },
    read, release, sound, write,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicBool, AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use mhfz_sdk::{
    action::Cursor,
    combat::HitWindow,
    sound::Clock,
    sprint::{self, Flick, Resume, STICK_DEADZONE},
};
use mhgu_sdk::combat::collision::HitShape;
use mhgu_sdk::style::{
    ActionRequest, Input, NATIVE_ACTION, PASS,
    action::{
        self, Attack,
        command::{BufferedCommand, CommandInput, CommandSet, MAX_COMMAND_FRAME},
        melee::{BLADE_CONTACT_OFFSETS, take_deflection},
    },
    guard,
    movement::{self, CLIMBS, stamina},
};

static SPRINT_TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, CLIMBS);
const BLADE_CONTACT: u32 = BLADE_CONTACT_OFFSETS[0];
// 811FA9FE uses the same gates for the forward and both lateral rolls.
const ROLL_X: BufferedCommand = BufferedCommand {
    group: action::GROUP,
    slot: action::slot(0),
    input: CommandInput::Weapon(0),
    accept_from: 6,
    execute_from: 36,
    accept_until: MAX_COMMAND_FRAME,
};
static ROLL_ATTACKS: CommandSet = CommandSet::new(&[
    ROLL_X,
    BufferedCommand {
        slot: action::slot(20),
        input: CommandInput::Weapon(2),
        ..ROLL_X
    },
])
.retaining_native(1, CLIMBS);

static OWNER: AtomicU32 = AtomicU32::new(0);
static CUT: AtomicU32 = AtomicU32::new(0);
static HUD_CLOCK: AtomicU32 = AtomicU32::new(0);
static CUT_FLASH: AtomicU32 = AtomicU32::new(0);
static FULL_TIME: AtomicU32 = AtomicU32::new(0);
static GUARD_FRAME: AtomicU32 = AtomicU32::new(0);
static STRIKE: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn bind_commands(actor: u32, table: u32) -> *const CommandSet {
    let (group, slot) = unsafe { action::current(actor) };
    if table == 2000 && matches!(action::custom_id(group, slot), Some(SPRINT | 71 | 82)) {
        &SPRINT_TERRAIN
    } else if (group, slot, table) == (1, 6, 2023) && unsafe { native!(0x2a2444, actor) } != 0 {
        &ROLL_ATTACKS
    } else {
        core::ptr::null()
    }
}

pub(super) fn cut(actor: u32) -> u32 {
    if actor == OWNER.load(Relaxed) {
        CUT.load(Relaxed)
    } else {
        0
    }
}

pub(super) fn hud_state(actor: u32) -> (u32, u32, u32, bool) {
    let tick = (f32::from_bits(HUD_CLOCK.load(Relaxed)) * 0.5) as u32;
    let flash = 30 - libm::ceilf(0.5 * f32::from_bits(CUT_FLASH.load(Relaxed))) as u32;
    (
        cut(actor),
        tick,
        flash,
        actor == OWNER.load(Relaxed) && FULL_TIME.load(Relaxed) != 0,
    )
}

unsafe fn gain_cut(actor: u32, amount: u32) {
    let scale = match unsafe { action::ChargeSkill::equipped(actor) } {
        action::ChargeSkill::Focus => 1.2,
        action::ChargeSkill::Slow => 0.8,
        action::ChargeSkill::Normal => 1.,
    };
    let gain = (amount as f32 * program::CUT_FACTOR * scale) as u32;
    if let Ok(old) = CUT.try_update(Relaxed, Relaxed, |points| {
        Some((points + gain).min(program::CUT_MAX))
    }) && old < program::CUT_MAX
        && old + gain >= program::CUT_MAX
    {
        CUT_FLASH.store(60f32.to_bits(), Relaxed);
    }
}
#[derive(Default)]
pub(super) struct Battle {
    slot: Slot,
    equipment: u32,
    cursor: Option<Cursor>,
    pending_blade: Option<u16>,
    packed: u16,
    audio: Clock,
    trail: crate::trail::History,
    motion_fx: mhfz_sdk::effects::MotionClock,
    stab_fx: Option<fx::Stab>,
    flick: Flick,
    sprint_buffer: f32,
    resume_sprint: Resume,
    spirit_hold: Option<f32>,
    evade_yaw: Option<u16>,
    charge: f32,
    charge_limit: f32,
    dash_slash: Option<fx::DashSlash>,
    dash_charge_effect: Option<fx::DashCharge>,
    poise: bool,
    discharge_lead: u16,
    discharge_frame: f32,
    discharge_yaw: u16,
    thrust_released: bool,
    side_sample: i16,
    side_turn: i16,
    roll_command: Option<u16>,
    deflected: bool,
    guard_follow: Option<(u8, f32)>,
    ground_clock: f32,
    ground_sound_frames: f32,
}

fn is_sprinting(state: u8) -> bool {
    matches!(state, SPRINT | 71 | 82)
}

// 8172C2D4: straight ahead, straight behind and neutral do not turn the hunter.
fn side(yaw: i16) -> i16 {
    if yaw == i16::MIN { 0 } else { yaw.signum() }
}

fn discharge_state(spirit: i32, chain: u16) -> u8 {
    54 + 3 * chain as u8 + u8::from(spirit >= 75) + u8::from(spirit >= 100)
}

unsafe fn turn(actor: u32, offset: u16) {
    unsafe {
        let yaw = read::<u16>(actor, 4076).wrapping_add(offset);
        face(actor, yaw);
    }
}

unsafe fn face(actor: u32, yaw: u16) {
    unsafe {
        write(actor, 4076, u32::from(yaw));
        write(actor, 5280, u32::from(yaw));
    }
}

unsafe fn relative(actor: u32, input: &Input) -> i16 {
    if input.stick_strength < STICK_DEADZONE {
        return 0;
    }
    unsafe { read::<u16>(actor, 4076).wrapping_sub(native!(0x27f5d0, actor) as u16) as i16 }
}

unsafe fn spirit(actor: u32) -> i32 {
    unsafe { read(actor, 10112) }
}

// Route all spirit changes through GU to retain its skill and gauge rules.
unsafe fn change_spirit(actor: u32, amount: i32) {
    unsafe {
        native!(0x2a309c, actor, amount as u32, 0);
    }
}

unsafe fn clear_blade_color(actor: u32) {
    unsafe {
        write(actor, 13104, 0u32);
        write(actor, 13100, 0f32);
    }
}

unsafe fn aim(actor: u32) -> u16 {
    unsafe {
        if native!(0x27e608, actor, 0) != 0 {
            native!(0x27f5d0, actor) as u16
        } else {
            read(actor, 4076)
        }
    }
}

fn dash_charge_limit(skill: action::ChargeSkill) -> f32 {
    // 811F8EEA adjusts the integer base before dividing into 30 Hz ticks.
    let base = program::CHARGE_TICKS;
    let adjusted = match skill {
        action::ChargeSkill::Focus => base * 8 / 10,
        action::ChargeSkill::Slow => base * 12 / 10,
        action::ChargeSkill::Normal => base,
    };
    (2 * (adjusted / 2)) as f32
}

impl Battle {
    unsafe fn commands(&mut self, actor: u32, input: &Input, dt: f32) -> u32 {
        unsafe {
            let mut bits = 1u32; // unconditional branches are tested by their eligibility predicate
            let yaw = relative(actor, input);
            if input.command(3) && read::<f32>(actor, 5088) >= 150. {
                bits |= 1 << 1;
            }
            if self.flick.update(input, dt) {
                // The native idle-to-run transition can occupy the frame on
                // which the second push arrives. Carry it to the next ground tick.
                self.sprint_buffer = 12.;
            } else {
                self.sprint_buffer = (self.sprint_buffer - dt).max(0.);
            }
            if input.stick_strength < STICK_DEADZONE {
                self.sprint_buffer = 0.;
            }
            if self.sprint_buffer > 0. && read::<f32>(actor, 5088) > 0. {
                bits |= 1 << CMD_SPRINT;
            }
            // GU w07 1199114 resolves X, A and the X+A chord into distinct weapon commands.
            if input.weapon_command(1) {
                bits |= 1 << CMD_EVADE_SLASH;
                if yaw.unsigned_abs() > 0x6000 {
                    bits |= 1 << CMD_EVADE_BACK;
                } else if yaw > 0x2000 {
                    bits |= 1 << CMD_EVADE_RIGHT;
                } else if yaw < -0x2000 {
                    bits |= 1 << CMD_EVADE_LEFT;
                }
            } else if input.weapon_command(0) {
                bits |= 1 << CMD_X;
                if input.stick_strength >= STICK_DEADZONE {
                    bits |= 1 << CMD_STEP_X;
                }
            } else if input.weapon_command(2) {
                bits |= 1 << CMD_A;
                if input.stick_strength >= STICK_DEADZONE {
                    bits |= 1 << 11;
                }
                if (-0x6000..-0x2000).contains(&yaw) {
                    bits |= 1 << 12;
                }
                if (0x2001..=0x6000).contains(&yaw) {
                    bits |= 1 << 13;
                }
            }
            if input.weapon_command(0) || input.weapon_command(1) || input.weapon_command(2) {
                bits |= 1 << 20;
            }
            if input.command(70) {
                self.spirit_hold = Some(0.);
            }
            if let Some(held) = self.spirit_hold.as_mut() {
                if input.command(27) {
                    *held += dt;
                    if *held >= 28. {
                        bits |= 1 << if spirit(actor) >= 50 { 15 } else { 14 };
                        self.spirit_hold = None;
                    }
                } else {
                    bits |= 1 << 14;
                    self.spirit_hold = None;
                }
            }
            if !input.command(27) {
                bits |= 1 << 19;
            }
            if input.auxiliary_pressed != 0 {
                bits |= 1 << CMD_PARRY;
            }
            if input.command(8) {
                bits |= 1 << CMD_RELEASE;
            }
            bits
        }
    }

    pub(super) fn clear_action_effects(&mut self) {
        self.dash_charge_effect = None;
        self.dash_slash = None;
        self.stab_fx = None;
    }

    fn clear_action(&mut self) {
        self.clear_action_effects();
        self.motion_fx.seek(0, 0.);
        self.cursor = None;
        self.guard_follow = None;
        GUARD_FRAME.store(0, Relaxed);
        self.deflected = false;
        self.thrust_released = false;
        self.side_turn = 0;
        self.evade_yaw = None;
        self.poise = false;
        instant::reset(self.slot);
        release::disarm(self.slot);
        STRIKE.store(0, Relaxed);
    }

    unsafe fn finish(&mut self, actor: u32) {
        self.clear_action();
        self.resume_sprint.set(false);
        unsafe {
            native!(0x2b6e5c, actor);
        }
    }

    unsafe fn begin(&mut self, actor: u32, state: u8) -> bool {
        unsafe {
            if !program::ACTIONS.contains(state) {
                return false;
            }
            let discharge_yaw = if (54..=65).contains(&state) {
                let facing = read::<u16>(actor, 4076);
                let relative = facing.wrapping_sub(aim(actor));
                match relative {
                    0 | 0x8000 => facing,
                    1..=0x7fff => facing.wrapping_sub(0x1000),
                    _ => facing.wrapping_add(0x1000),
                }
            } else {
                0
            };
            if !(action::current(actor) == (action::GROUP, action::slot(state))
                && read::<u8>(actor, 5) == 0)
                && !action::transition_to(actor, action::GROUP, action::slot(state))
            {
                return false;
            }
            if (54..=65).contains(&state) {
                // 811F2206 applies one signed 1/16-turn to the pre-transition facing.
                self.discharge_yaw = discharge_yaw;
                self.select_discharge_lead(actor);
                change_spirit(actor, -spirit(actor));
            }
            let charged_slash = state == 72 && self.dash_charge_ready();
            if charged_slash {
                change_spirit(actor, -program::DASH_COST);
            }
            self.clear_action();
            if state == 72 {
                self.dash_slash = Some(fx::DashSlash::new(self.slot, charged_slash));
            }
            if state == instant::ACTION {
                instant::begin(self.slot, actor);
            }
            self.resume_sprint.set(false);
            self.cursor = Some(program::cursor(state));
            self.charge = if state == 47 { 2. } else { 0. };
            self.side_sample = if matches!(state, 33 | 37 | 41 | 67) {
                -1
            } else {
                1
            };
            self.ground_sound_frames = 0.;
            if matches!(state, 47 | 71 | 82) {
                let skill = action::ChargeSkill::equipped(actor);
                self.charge_limit = if state == 47 {
                    2. * match skill {
                        action::ChargeSkill::Focus => 50.,
                        action::ChargeSkill::Slow => 75.,
                        action::ChargeSkill::Normal => 60.,
                    }
                } else {
                    dash_charge_limit(skill)
                };
            }
            write(actor, 5, 1u8);
            write(actor, BLADE_CONTACT + 176, 0u32);
            let running = is_sprinting(state);
            native!(0x282fd8, actor, 0, u32::from(!running), 0);
            if native!(0x2a2444, actor) == 0 {
                native!(0x2a264c, actor, 1);
            } else {
                native!(0x2a2560, actor, 1);
            }
            if running {
                native!(0x283238, actor, 0, 0x1180, 0);
                native!(0x2e9b7c, actor, 2000, 0);
                native!(0x2a25a8, actor, 1);
                face(actor, aim(actor));
                self.sprint_buffer = 0.;
                self.spirit_hold = None;
            }
            self.play_step(actor);
            if (54..=65).contains(&state) {
                fx::spirit_flash(actor, false);
            }
            true
        }
    }

    unsafe fn activate_art(&mut self, actor: u32) -> bool {
        unsafe {
            if !action::activate_hunter_art(actor) {
                return false;
            }
            self.clear_action();
            self.spirit_hold = None;
            self.sprint_buffer = 0.;
            self.resume_sprint.set(false);
            true
        }
    }

    unsafe fn play_step(&mut self, actor: u32) {
        unsafe {
            let Some(c) = &mut self.cursor else { return };
            if let Some(step) = program::step(c.state, c.time.step, self.discharge_lead) {
                if step.op == 2 {
                    if self.slot != LOCAL {
                        return;
                    }
                    let next = match step.clip {
                        103 => 34,
                        104 => 38,
                        105 => 42,
                        106 => 69,
                        id => id as u8,
                    };
                    let heading = if matches!(step.clip, 103..=106) {
                        self.side_turn
                    } else {
                        0
                    };
                    if self.begin(actor, next) && heading != 0 {
                        turn(actor, (heading * -8192) as u16);
                    }
                    return;
                } else if matches!(step.op, 3..=5)
                    && let Some(packed) = motion::play(
                        actor,
                        if c.state == 12 && step.clip == 1401 {
                            1460
                        } else {
                            step.clip
                        },
                        step.start,
                        step.blend,
                    )
                {
                    self.packed = packed;
                    self.pending_blade = None;
                    self.motion_fx.seek(step.clip, f32::from(step.start));
                    if (54..=65).contains(&c.state) {
                        face(actor, self.discharge_yaw);
                    }
                    if step.clip == 1437 {
                        self.discharge_frame = -1.;
                    }
                    c.time.cycles = 0;
                    c.time.frame = f32::from(step.start);
                    c.time.blend = f32::from(step.blend) / 2.;
                    sound::update(
                        &mut self.audio,
                        actor,
                        Some(step.clip),
                        c.time.frame,
                        Some(c.time.frame),
                    );
                    self.events(actor);
                    return;
                }
            }
            if self.slot == LOCAL {
                self.finish(actor);
            } else {
                self.cursor = None;
            }
        }
    }

    unsafe fn events(&mut self, actor: u32) {
        unsafe {
            while let Some(c) = &mut self.cursor
                && let Some((kind, param, late)) = c.next_event()
            {
                if self.slot != LOCAL && !matches!(kind, 1 | 2 | 3 | 10 | 17 | 20..=26) {
                    continue;
                }
                let frames = (2. * f32::from(param) - late).max(0.);
                match kind {
                    1 => {
                        native!(
                            0x282fd8,
                            actor,
                            u32::from(param & 2),
                            u32::from(param & 1),
                            0
                        );
                    }
                    2 => {
                        native!(0x2a2560, actor, u32::from(param != 0));
                    }
                    3 => turn(actor, param),
                    4 => {
                        self.poise = false;
                        write(actor, 10288, frames);
                    }
                    5 => self.poise = true,
                    6..=8 => {
                        let extra = match native!(0x2a69dc, actor) as i32 {
                            2.. => 10.,
                            1 => 6.,
                            _ => 0.,
                        };
                        let inv = match kind {
                            6 => frames,
                            7 => frames + extra,
                            _ => (14. + extra - late).max(0.),
                        };
                        write(actor, 11072, inv);
                    }
                    10 => {
                        let active = match param {
                            3 => true,
                            2 => [168, 169, 279, 297]
                                .iter()
                                .any(|&id| native!(0x2864c4, actor, id) != 0),
                            1 => [168, 279, 297]
                                .iter()
                                .any(|&id| native!(0x2864c4, actor, id) != 0),
                            _ => false,
                        };
                        if active {
                            native!(0x283238, actor, 0, 16, 0);
                        } else {
                            native!(0x28321c, actor, 0, 16, 0);
                        }
                    }
                    // The registry clears the accepted GU recoil on transition.
                    // Drop its saved impact as FZ 8182A760 starts a new guard.
                    11 => fx::clear_impact(),
                    12 => stamina(actor, -150., false),
                    13..=16 => {
                        let cost: i32 = match kind {
                            13 => 15,
                            14 => 20,
                            _ => 25,
                        };
                        change_spirit(actor, -cost);
                    }
                    // The vertical slash and thrust arm the blade on clip entry,
                    // including when the clip starts partway through its motion.
                    17 if matches!(param, 57 | 75) => self.pending_blade = Some(param),
                    17 => strike(self.slot, actor, param, c.time.frame, late),
                    // 817C426E's extra dodge and the +996 attack modifier
                    // require Frontier equipment effects (816D6482/816D699A).
                    18 => {}
                    19 => crate::camera::start(actor, param),
                    20 | 21 => self.trail.start(self.slot, actor, frames, kind == 21),
                    22 | 23 => fx::spirit_flash(actor, kind == 23),
                    24 => {
                        fx::joint(actor, 1, 0);
                        sound::weapon(actor, 12);
                    }
                    25 => release::arm(self.slot, c.state),
                    26 => fx::parry(actor),
                    _ => {}
                }
            }
            if let (Some(id), Some(c)) = (self.pending_blade, &self.cursor)
                && let Some(row) = program::strike(id)
                && c.time.blend <= 0.
                && c.time.frame >= f32::from(row.start)
            {
                self.pending_blade = None;
                strike(self.slot, actor, id, c.time.frame, 0.);
            }
        }
    }

    unsafe fn evade(&mut self, actor: u32, yaw: u16) -> bool {
        if !unsafe { motion::ROLL.enter(actor, yaw) } {
            return false;
        }
        self.clear_action();
        true
    }

    unsafe fn branch(&mut self, actor: u32, input: &Input, id: u16) -> bool {
        unsafe {
            let gauge = spirit(actor);
            let yaw = relative(actor, input);

            let to = match id {
                0..=84 => id as u8,
                85 => return self.evade(actor, self.evade_yaw.unwrap_or(yaw as u16)),
                86 => SPRINT,
                87 => 47,
                88 => 4,
                89 => 20,
                90 => 24,
                91 => 32,
                92 => {
                    if gauge >= 15 {
                        7
                    } else {
                        18
                    }
                }
                93 => {
                    if gauge >= 15 {
                        28
                    } else {
                        29
                    }
                }
                94 => 8,
                95 => 45,
                96 => 27,
                97 => 9,
                98 => 10,
                99 => 46,
                100 => 23,
                101 => 15,
                102 => 70,
                107..=110 => discharge_state(gauge, id - 107),
                111 => program::release(cut(actor)),
                112 => {
                    turn(actor, (yaw as u16).wrapping_neg());
                    77
                }
                113 => 78,
                _ => return false,
            };
            let accepted = self.begin(actor, to);
            if accepted && matches!(id, 87..=102 | 113) {
                turn(actor, (side(yaw) * -4096) as u16);
            }
            accepted
        }
    }

    fn select_discharge_lead(&mut self, actor: u32) {
        unsafe {
            self.discharge_lead = if native!(0x2a2444, actor) == 0 {
                1447
            } else {
                match self.cursor.as_ref().map(|c| c.state) {
                    None => 1443,
                    Some(7 | 18 | 28..=31) => 1441,
                    Some(9 | 27 | 45) => 1442,
                    Some(10 | 20 | 32) => 1446,
                    _ => 0,
                }
            };
        }
    }

    unsafe fn discharge_hits(&mut self, actor: u32, state: u8, clip: u16, frame: f32) {
        unsafe {
            if (54..=65).contains(&state) && clip == 1437 {
                face(actor, self.discharge_yaw);
                let last = self.discharge_frame;
                self.discharge_frame = last.max(frame);
                if last < 122. && frame >= 122. {
                    if self.slot == LOCAL {
                        crate::camera::start(actor, ((state - 54) / 3).saturating_sub(1).into());
                    }
                    strike(
                        self.slot,
                        actor,
                        203 + u16::from((state - 54) / 3),
                        frame,
                        frame - 122.,
                    );
                }
                discharge::frame(self.slot, actor, state, last, frame, self.discharge_yaw);
                if last < 136. && frame >= 136. {
                    self.trail
                        .start(self.slot, actor, (20. - (frame - 136.)).max(0.), true);
                }
            }
        }
    }

    fn dash_charge_ready(&self) -> bool {
        self.cursor.as_ref().is_some_and(|c| c.state == 71) && self.charge >= self.charge_limit
    }

    unsafe fn dash_hit(&self, actor: u32, last: f32, frame: f32) {
        let Some(slash) = &self.dash_slash else {
            return;
        };
        unsafe {
            if last < 2. && frame >= 2. {
                slash.opening(actor);
            }
            if last < 18. && frame >= 18. {
                strike(
                    self.slot,
                    actor,
                    if slash.charged { 249 } else { 248 },
                    frame,
                    frame - 18.,
                );
            }
            if last < 24. && frame >= 24. {
                slash.impact(actor);
            }
        }
    }

    unsafe fn blade_effects(&mut self, actor: u32, state: u8, last: f32, frame: f32) {
        unsafe {
            if state == 71 && self.charge >= self.charge_limit {
                self.dash_charge_effect
                    .get_or_insert_with(|| fx::DashCharge::new(self.slot))
                    .update(actor);
            }
            if read::<f32>(actor, 1268) > 0. {
                return;
            }
            let (start, end) = match state {
                48..=50 => (16u16, 16),
                // The counter's family 108:5 shares the strong thrust window.
                51..=53 | 81 => (36, 60),
                _ => return,
            };
            for (index, tick) in (start..=end).step_by(2).enumerate() {
                let at = f32::from(tick);
                if !(last < at && at <= frame) {
                    continue;
                }
                if state == 81 {
                    fx::counter_tip(actor);
                } else {
                    self.stab_fx
                        .get_or_insert_with(|| fx::Stab::new(self.slot))
                        .pulse(actor, state >= 51, index);
                }
            }
        }
    }

    unsafe fn advance_step(
        &mut self,
        actor: u32,
        step: mhfz_sdk::action::Step,
        sample: action::MotionFrame,
        wrapped: bool,
    ) -> bool {
        unsafe {
            let c = self.cursor.as_mut().unwrap();
            if step.complete(c.time, sample, wrapped) {
                c.time.step += 1;
                self.play_step(actor);
                return true;
            }
            false
        }
    }

    pub(super) fn peer(slot: Slot) -> Self {
        Self {
            slot,
            ..Default::default()
        }
    }

    // 1FF308 has already applied the sender's position, facing and accepted action.
    // Only the sender chooses the next action and consumes inputs, gauges and skills.
    pub(super) unsafe fn play_received(&mut self, actor: u32, state: u8) {
        unsafe {
            self.trail.tick(actor, read::<f32>(actor, 28).max(0.));
            if read::<u8>(actor, 5) == 0 {
                clear_blade_color(actor);
                instant::reset(self.slot);
                release::disarm(self.slot);
                let (previous_group, previous_slot) = action::previous(actor);
                if self.cursor.as_ref().map(|c| c.state)
                    != action::custom_id(previous_group, previous_slot)
                {
                    self.cursor = None;
                }
                if (54..=65).contains(&state) {
                    self.discharge_yaw = read(actor, 4076);
                    self.select_discharge_lead(actor);
                }
                let charged_slash = state == 72 && self.dash_charge_ready();
                if state == 71 {
                    self.charge = 0.;
                    self.charge_limit = dash_charge_limit(action::ChargeSkill::equipped(actor));
                }
                if state == instant::ACTION {
                    instant::begin(self.slot, actor);
                }
                self.clear_action_effects();
                if state == 72 {
                    self.dash_slash = Some(fx::DashSlash::new(self.slot, charged_slash));
                }
                self.cursor = Some(program::cursor(state));
                write(actor, 5, 1u8);
                native!(0x282fd8, actor, 0, u32::from(!is_sprinting(state)), 0);
                if native!(0x2a2444, actor) == 0 {
                    native!(0x2a264c, actor, 1);
                } else {
                    native!(0x2a2560, actor, 1);
                }
                self.play_step(actor);
                if (54..=65).contains(&state) {
                    fx::spirit_flash(actor, false);
                }
                return;
            }
            let Some(c) = self.cursor.as_mut().filter(|c| c.state == state) else {
                return;
            };
            let Some(step) = program::step(state, c.time.step, self.discharge_lead) else {
                return;
            };
            if step.op == 2 || read::<u16>(actor, 1204) != self.packed {
                return;
            }
            let sample = action::MotionFrame::sample(actor);
            let frame = sample.frame;
            let last = c.time.frame;
            let wrapped = c.time.observe(frame, sample.blend);
            if state == 71 {
                self.charge += read::<f32>(actor, 28).max(0.);
            }
            sound::update(&mut self.audio, actor, Some(step.clip), frame, None);
            self.events(actor);
            fx::motion_events(
                &mut self.motion_fx,
                actor,
                self.slot,
                &mut self.trail,
                step.clip,
                sample,
            );
            self.discharge_hits(actor, state, step.clip, frame);
            self.blade_effects(actor, state, last, frame);
            self.dash_hit(actor, last, frame);
            self.advance_step(actor, step, sample, wrapped);
        }
    }

    unsafe fn advance(&mut self, actor: u32, input: &Input, bits: u32, dt: f32) -> bool {
        unsafe {
            let c = self.cursor.as_ref().unwrap();
            let state = c.state;
            let Some(step) = program::step(state, c.time.step, self.discharge_lead) else {
                self.finish(actor);
                return true;
            };
            if step.op == 2 {
                // GU can defer the transition during hitstop. Keep the selected
                // follow-up and the saved turn until it accepts the transition.
                self.play_step(actor);
                return true;
            }
            self.deflected |= take_deflection(actor);
            if self.deflected && self.begin(actor, 3) {
                action::melee::clear_hitstop(actor);
                return true;
            }
            let c = self.cursor.as_mut().unwrap();
            let sample = action::MotionFrame::sample(actor);
            let frame = sample.frame;
            let last = c.time.frame;
            let wrapped = c.time.observe(frame, sample.blend);
            sound::update(&mut self.audio, actor, Some(step.clip), frame, None);
            if matches!(state, 48 | 50 | 51 | 53) && read::<f32>(actor, 1268) <= 0. {
                // 824EDE3C/64: turn 1024 units per FZ tick for eight motion frames.
                let start = if state >= 51 { 12. } else { 0. };
                let ticks = |f: f32| ((f - start).clamp(0., 8.) * 512.) as u16;
                let angle = ticks(frame).saturating_sub(ticks(last));
                turn(
                    actor,
                    if matches!(state, 48 | 51) {
                        angle
                    } else {
                        angle.wrapping_neg()
                    },
                );
            }
            self.events(actor);
            fx::motion_events(
                &mut self.motion_fx,
                actor,
                self.slot,
                &mut self.trail,
                step.clip,
                sample,
            );
            self.discharge_hits(actor, state, step.clip, frame);
            if self.poise {
                write(actor, 10288, read::<f32>(actor, 10288).max(8.));
            }
            let gauge = spirit(actor);
            let enough = read::<f32>(actor, 5088) >= 150.;
            let eligible = |id| match id {
                0 | 6 => true,
                2 => enough,
                3 => gauge >= 20,
                4 => gauge >= 25,
                5 => gauge >= 50,
                9 => cut(actor) > 0,
                _ => false,
            };
            let c = self.cursor.as_mut().unwrap();
            if (state == SPRINT || c.art_window(eligible)) && self.activate_art(actor) {
                return true;
            }
            if matches!(state, 71 | 82) {
                if input.stick_strength < STICK_DEADZONE || read::<f32>(actor, 5088) <= 0. {
                    return self.begin(actor, 72);
                }
                if bits & (1 << 1) != 0 && motion::ROLL.enter(actor, 0) {
                    self.clear_action();
                    return true;
                }
                if bits & (1 << 19) != 0 {
                    return self.begin(actor, 72);
                }
            }
            let c = self.cursor.as_mut().unwrap();
            let next = c.branch(bits, eligible);
            if c.pending_action() == Some(85) {
                self.evade_yaw
                    .get_or_insert_with(|| relative(actor, input) as u16);
            } else {
                self.evade_yaw = None;
            }
            if let Some(next) = next
                && self.branch(actor, input, next)
            {
                return true;
            }
            if state == SPRINT {
                if bits & (1 << CMD_PARRY) != 0 {
                    return self.begin(actor, 78);
                }
                if bits & (1 << CMD_RELEASE) != 0 && cut(actor) > 0 {
                    return self.begin(actor, program::release(cut(actor)));
                }
                if input.command(27) {
                    return self.begin(actor, if gauge >= program::DASH_COST { 71 } else { 82 });
                }
                // 817D3974 calls the w07 sprint dispatcher (811FA7A2)
                // once clip 1409 passes the weapon's 817332DA gate.
                if frame >= 30. && read::<f32>(actor, 1268) <= 0. {
                    if bits & (1 << CMD_X) != 0 {
                        return self.begin(actor, 0);
                    }
                    if bits & (1 << CMD_A) != 0 {
                        return self.begin(actor, 20);
                    }
                }
                if input.command(13) {
                    return action::transition_to(actor, 1, 2);
                }
                if bits & (1 << 1) != 0 {
                    return self.evade(actor, relative(actor, input) as u16);
                }
            }
            if is_sprinting(state) {
                let has_direction = movement::steer(actor);
                if state == SPRINT
                    && sprint::can_brake(frame, self.cursor.as_ref().unwrap().time.cycles)
                    && (!has_direction || read::<f32>(actor, 5088) <= 0.)
                {
                    return self.begin(actor, BRAKE);
                }
                if dt > 0. {
                    movement::drain_running_stamina(actor);
                }
            }
            if state == 47 {
                self.thrust_released |=
                    !a_held(actor) || gauge < 100 || self.charge >= self.charge_limit;
                if self.thrust_released && frame >= 30. {
                    let direction = side(relative(actor, input)) + 1;
                    let base = if self.charge >= self.charge_limit {
                        51
                    } else {
                        48
                    };
                    return self.begin(actor, base + direction as u8);
                }
            }
            if !self.thrust_released {
                self.charge += dt;
            }
            if matches!(state, 71 | 82) {
                let charged = state == 71 && self.charge >= self.charge_limit;
                self.ground_sound_frames += dt;
                if (charged || frame >= 28.)
                    && self.ground_clock < dt
                    && dt > 0.
                    && fx::dash_ground(actor, charged, self.ground_sound_frames >= 32.)
                {
                    self.ground_sound_frames = 0.;
                }
            }
            self.blade_effects(actor, state, last, frame);
            self.dash_hit(actor, last, frame);
            if !self.advance_step(actor, step, sample, wrapped)
                && matches!(state, 33 | 35 | 37 | 39 | 41 | 43 | 67 | 68)
            {
                let sample = side(relative(actor, input));
                if sample != self.side_sample {
                    self.side_turn = sample;
                    self.side_sample = sample;
                }
            }
            true
        }
    }

    unsafe fn after_roll(&mut self, actor: u32, bits: u32) -> bool {
        unsafe {
            let frame = read::<f32>(actor, 1280);
            let can_execute = frame >= f32::from(ROLL_X.execute_from);
            if self.roll_command.is_none() && frame >= f32::from(ROLL_X.accept_from) {
                self.roll_command = [CMD_SPRINT, CMD_X, CMD_A, CMD_PARRY, CMD_RELEASE]
                    .into_iter()
                    .find(|&cmd| bits & (1 << cmd) != 0 && (cmd != CMD_PARRY || can_execute));
            }
            if can_execute {
                match self.roll_command {
                    Some(CMD_SPRINT) => return self.begin(actor, SPRINT),
                    Some(CMD_PARRY) => return self.begin(actor, 78),
                    Some(CMD_RELEASE) if cut(actor) > 0 => {
                        return self.begin(actor, program::release(cut(actor)));
                    }
                    _ => {}
                }
            }
            // GU handles movement, terrain and the bound X/A branches.
            false
        }
    }

    unsafe fn tick(&mut self, actor: u32, input: &Input) -> bool {
        unsafe {
            clear_blade_color(actor);
            let dt = read::<f32>(actor, 28).max(0.);
            let clock = (f32::from_bits(HUD_CLOCK.load(Relaxed)) + dt) % 120.;
            HUD_CLOCK.store(clock.to_bits(), Relaxed);
            let flash = (f32::from_bits(CUT_FLASH.load(Relaxed)) - dt).max(0.);
            CUT_FLASH.store(flash.to_bits(), Relaxed);
            self.ground_clock = (self.ground_clock + dt) % 4.;
            let left = (f32::from_bits(FULL_TIME.load(Relaxed)) - dt).max(0.);
            FULL_TIME.store(left.to_bits(), Relaxed);
            self.trail.tick(actor, dt);
            let current = action::current(actor);
            if current != (1, 6) {
                motion::ROLL.clear();
                self.roll_command = None;
            }
            let custom = action::custom_id(current.0, current.1);
            let drawn = native!(0x2a2444, actor) != 0;
            if !drawn || read::<i16>(actor, 5078) <= 0 {
                CUT.store(0, Relaxed);
                CUT_FLASH.store(0, Relaxed);
            }
            if let Some(c) = &self.cursor
                && c.state == 78
            {
                if self.guard_follow.is_none()
                    && let Some(contact) = guard::take(actor)
                {
                    // GU has already switched to recoil. Its animation clock is
                    // unrelated to the stance frame captured by the collision policy.
                    let frame = f32::from_bits(GUARD_FRAME.swap(0, Relaxed)).max(c.time.frame);
                    if frame < 36. {
                        self.guard_follow = Some((
                            if matches!(contact, guard::Contact::Monster) {
                                80
                            } else {
                                79
                            },
                            (8. - frame).max(0.),
                        ));
                    }
                } else if let Some((_, wait)) = &mut self.guard_follow {
                    *wait = (*wait - dt).max(0.);
                }
                if let Some((dest, wait)) = self.guard_follow {
                    if current != (action::GROUP, action::slot(78))
                        && !matches!(current, (32, 3 | 4 | 5 | 8))
                    {
                        self.guard_follow = None;
                    } else {
                        if wait <= 0. && self.begin(actor, dest) && dest == 80 {
                            gain_cut(actor, program::GUARD_GAIN);
                        }
                        return true;
                    }
                }
            }
            if matches!(current, (action::GROUP, 51..=62 | 90)) {
                // The retail art phases own input and their native contacts.
                if self.cursor.is_some() {
                    self.clear_action();
                }
                self.spirit_hold = None;
                self.sprint_buffer = 0.;
                self.resume_sprint.set(false);
                return false;
            }
            let bits = self.commands(actor, input, dt);
            if let Some(state) = custom {
                if self.cursor.as_ref().is_some_and(|c| c.state == state)
                    && read::<u16>(actor, 1204) == self.packed
                    && read::<u8>(actor, 5) != 0
                {
                    return self.advance(actor, input, bits, dt);
                }
                if matches!(state, 0 | 20) && action::previous(actor) == (1, 6) {
                    turn(actor, (side(relative(actor, input)) * -4096) as u16);
                }
                return self.begin(actor, state);
            }
            let was_sprinting = self.cursor.as_ref().is_some_and(|c| is_sprinting(c.state));
            if self.cursor.is_some() {
                self.clear_action();
            }
            let clip = motion::source_clip(actor, read(actor, 1204));
            let sample = action::MotionFrame::sample(actor);
            sound::update(&mut self.audio, actor, clip, sample.frame, None);
            if let Some(clip) = clip {
                fx::motion_events(
                    &mut self.motion_fx,
                    actor,
                    self.slot,
                    &mut self.trail,
                    clip,
                    sample,
                );
            } else {
                self.motion_fx.seek(0, 0.);
            }
            if current == (1, 6) && drawn && read::<u8>(actor, 5) != 0 {
                return self.after_roll(actor, bits);
            }
            if matches!(current, (64, 6 | 7))
                && read::<u8>(actor, 5) == 2
                && bits & (1 << 1) != 0
                && read::<u8>(actor, 11988) != 0
                && drawn
                && read::<i16>(actor, 5078) > 0
                && native!(0x53a058, actor) == 1015
            {
                return self.begin(actor, if current.1 == 6 { 75 } else { 74 });
            }
            let ground = movement::ground_idle(current)
                || (matches!(current.0, 1 | 2)
                    && read::<u8>(actor, 5) != 0
                    && matches!(native!(0x53a058, actor), 5001 | 5004));
            self.resume_sprint
                .on_native(actor, current, ground, drawn, was_sprinting);
            if ground && self.activate_art(actor) {
                return true;
            }
            // FZ 811FA176 also accepts draw-parry from weapon-down movement.
            if bits & (1 << CMD_PARRY) != 0
                && (ground || (!drawn && movement::sheathed_movement(current)))
            {
                return self.begin(actor, 78);
            }
            if !drawn || !ground {
                return false;
            }
            let resume_sprint = self.resume_sprint.ready(
                native!(0x27e608, actor, 0) > 0,
                read::<f32>(actor, 5088) > 0.,
            );
            let gauge = spirit(actor);
            let state = if bits & (1 << CMD_RELEASE) != 0 && cut(actor) > 0 {
                program::release(cut(actor))
            } else if bits & (1 << CMD_SPRINT) != 0 {
                SPRINT
            } else if bits & (1 << 15) != 0 {
                discharge_state(gauge, 0)
            } else if bits & (1 << 14) != 0 {
                if gauge >= 15 { 7 } else { 18 }
            } else if bits & (1 << CMD_EVADE_SLASH) != 0 && enough_evade(actor) {
                32
            } else if bits & (1 << 11) != 0 {
                4
            } else if bits & (1 << CMD_A) != 0 {
                47
            } else if bits & (1 << CMD_X) != 0 {
                if input.stick_strength >= STICK_DEADZONE {
                    12
                } else {
                    0
                }
            } else if resume_sprint && bits & (1 << 1) == 0 && !input.command(13) {
                SPRINT
            } else {
                return false;
            };
            self.begin(actor, state)
        }
    }
}

unsafe fn enough_evade(actor: u32) -> bool {
    unsafe { read::<f32>(actor, 5088) >= 150. }
}
unsafe fn a_held(actor: u32) -> bool {
    unsafe { read::<u16>(actor, 5108) & 0x100 != 0 }
}
unsafe fn strike(slot: Slot, actor: u32, id: u16, frame: f32, late: f32) {
    unsafe {
        let Some(row) = program::strike(id) else {
            return;
        };
        let Some(HitWindow { delay, duration }) =
            HitWindow::new(row.start, row.duration, frame, late)
        else {
            return;
        };
        let path = match row.shape {
            0 => cr"player\hit\frontier_ls\blade_0",
            32 => cr"player\hit\frontier_ls\blade_32",
            33 => cr"player\hit\frontier_ls\blade_33",
            _ => return,
        };
        let Some(shape) = HitShape::load(path) else {
            return;
        };
        if shape.rows_in_first_group(3).is_none() {
            return;
        }
        let (group, action_slot) = action::current(actor);
        let state = action::custom_id(group, action_slot);
        let deflectable = matches!(
            state,
            Some(
                0..=2
                | 4
                | 5
                | 8
                | 10
                | 12
                | 17..=22
                | 24..=26
                | 29
                | 31
                | 32
                | 34
                | 38
                | 42
                | 66
                | 69
                | 70,
            )
        );
        let attack = Attack {
            template: collision::BLADE_TEMPLATE,
            power: row.power,
            delay,
            duration,
            flags: 7 | Attack::NO_MOUNT | if deflectable { Attack::DEFLECTABLE } else { 0 },
            shape: shape.as_raw(),
            no_sharpness_cost: 0,
        };
        if attack.submit(actor) {
            collision::clear_native_spirit(actor + BLADE_CONTACT);
            if slot == LOCAL {
                STRIKE.store(u32::from(id), Relaxed);
            }
        }
    }
}

struct Local(UnsafeCell<Option<Battle>>);
unsafe impl Sync for Local {}
static BATTLE: Local = Local(UnsafeCell::new(None));
static DISPATCHING: AtomicBool = AtomicBool::new(false);

fn reset_session() {
    motion::ROLL.clear();
    release::reset(LOCAL);
    crate::camera::reset();
    instant::reset(LOCAL);
    discharge::reset(LOCAL);
    fx::clear_impact();
    HUD_CLOCK.store(0, Relaxed);
    CUT_FLASH.store(0, Relaxed);
    FULL_TIME.store(0, Relaxed);
    CUT.store(0, Relaxed);
    STRIKE.store(0, Relaxed);
}

pub(super) unsafe extern "C" fn dispatch(actor: u32, raw: *const Input) -> u32 {
    if DISPATCHING.swap(true, Acquire) {
        return PASS;
    }
    let result = unsafe {
        let state = &mut *BATTLE.0.get();
        if let Some(input) = raw.as_ref() {
            let equipment = read(actor, 5060);
            if state.as_ref().is_none_or(|s| s.equipment != equipment)
                || actor != OWNER.load(Relaxed)
            {
                *state = Some(Battle {
                    equipment,
                    ..Default::default()
                });
                reset_session();
                OWNER.store(actor, Relaxed);
            }
            if state.as_mut().unwrap().tick(actor, input) {
                0
            } else {
                PASS
            }
        } else {
            if actor != 0
                && actor == OWNER.load(Relaxed)
                && let Some(b) = state
            {
                let (group, slot) = action::current(actor);
                if action::custom(group, slot) {
                    b.finish(actor);
                }
            }
            reset_session();
            *state = None;
            OWNER.store(0, Relaxed);
            PASS
        }
    };
    DISPATCHING.store(false, Release);
    result
}

pub(super) unsafe extern "C" fn request(actor: u32, raw: *const ActionRequest) -> u32 {
    unsafe {
        let r = &*raw;
        match r.slot {
            32..=34 | 37..=40 | 42..=47 | 69..=81 => NATIVE_ACTION,
            3 if native!(0x2a2444, actor) == 0 => 19,
            16 | 17 | 20 | 21 if native!(0x2a2444, actor) == 0 => {
                if spirit(actor) >= 15 {
                    30
                } else {
                    31
                }
            }
            _ => PASS,
        }
    }
}

pub(super) unsafe extern "C" fn guard_policy(
    actor: u32,
    result: *mut guard::GuardResult,
) -> guard::Decision {
    unsafe {
        if action::current(actor) != (action::GROUP, action::slot(78)) {
            return guard::Decision::Native;
        }
        if read::<u32>(actor, 9488) & 16 == 0 {
            return guard::Decision::Reject;
        }
        fx::remember_impact(actor);
        GUARD_FRAME.store(read::<f32>(actor, 1280).to_bits(), Relaxed);
        result.write(guard::GuardResult {
            invulnerability: 8.,
            recoil: 0,
            sharpness_power: 40,
        });
        guard::Decision::Precise
    }
}

#[forge::hook(offset = 0x119ed98)]
unsafe fn change_color(actor: u32, delta: u32) -> u32 {
    if crate::local_actor() == Some(actor) {
        unsafe {
            clear_blade_color(actor);
        }
        return 0;
    }
    original!(actor, delta)
}

#[forge::hook(offset = 0x119ed8c)]
unsafe fn set_color(actor: u32, level: u32) -> u32 {
    if crate::local_actor() == Some(actor) {
        return original!(actor, 0);
    }
    original!(actor, level)
}

// w07's virtual at +148 receives the resolved contact after the monster hit.
#[forge::hook(offset = 0x119f574)]
unsafe fn contact(actor: u32, hit: u32, target: u32, kind: u32) -> u32 {
    let result = original!(actor, hit, target, kind);
    if actor == 0 || target == 0 || kind != 1 {
        return result;
    }
    let blade = hit == actor + BLADE_CONTACT;
    if actor != OWNER.load(Relaxed) {
        let slot = Slot::of(actor);
        if slot != LOCAL && blade {
            unsafe {
                release::mark(slot, actor, hit, target);
            }
        }
        return result;
    }

    unsafe {
        let instant_spirit = instant::hit_gain(LOCAL, actor, hit);
        if instant_spirit != 0 {
            gain_cut(actor, u32::from(program::instant().power));
        }
        let spirit = discharge::hit_gain(LOCAL, actor, hit) + instant_spirit;
        if spirit != 0 {
            change_spirit(actor, i32::from(spirit));
        }

        if !blade {
            return result;
        }
        let id = STRIKE.load(Relaxed) as u16;
        if id != 0
            && let Some(row) = program::strike(id)
        {
            if matches!(id, 250 | 263 | 264) {
                if release::mark(LOCAL, actor, hit, target) {
                    CUT.store(0, Relaxed);
                }
            } else {
                gain_cut(actor, u32::from(row.power));
            }
            if row.spirit != 0 {
                change_spirit(actor, i32::from(row.spirit));
            }
        }
    }
    result
}

// GU calls this virtual when either the native or style gauge reaches 100.
#[forge::hook(offset = 0x119ee7c)]
unsafe fn full(actor: u32) -> u32 {
    let result = original!(actor);
    if actor != 0 && actor == OWNER.load(Relaxed) {
        // Frontier 818472DE refreshes mhfdat[0x660]+240, in 30 Hz ticks.
        let previous = FULL_TIME.swap(program::FULL_DURATION.to_bits(), Relaxed);
        unsafe {
            write(actor, 13108, program::FULL_DURATION);
            if previous == 0 {
                fx::spirit_full(actor);
            }
        }
    }
    result
}

#[forge::hook(offset = 0x1198ab4)]
unsafe fn full_spirit_status(actor: u32) -> u32 {
    let result = original!(actor);
    if crate::local_actor() == Some(actor) {
        unsafe {
            // Keep GU's gauge timer. Its full-spirit flag changes the cutting
            // coefficient from 100 to 113 on top of Frontier's +10 raw.
            write(actor, 10336, read::<u32>(actor, 10336) & !0x200);
        }
    }
    result
}

unsafe extern "C" fn physical(actor: u32, raw: i32) -> i32 {
    if actor == OWNER.load(Relaxed) && FULL_TIME.load(Relaxed) != 0 {
        raw + 10
    } else {
        raw
    }
}
pub(super) fn install() {
    collision::install();
    fx::install();
    crate::trail::install();
    crate::camera::install();
    let base = forge::mem::text_addr();
    use mhgu_sdk::style::physical::{self as power, Rule, Scope};
    static POWER: Rule = Rule {
        scope: Scope::WeaponContacts,
        adjust: physical,
    };
    if !unsafe { power::bind(crate::STYLE_ID, &POWER) } {
        forge::log::prelude::warn!("Extreme LS: physical power service unavailable");
    }
    forge::install_hook!(base, full);
    forge::install_hook!(base, full_spirit_status);
    forge::install_hook!(base, change_color);
    forge::install_hook!(base, set_color);
    forge::install_hook!(base, contact);
}
