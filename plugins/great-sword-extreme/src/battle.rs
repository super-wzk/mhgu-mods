//! Fight logic for the local hunter. GU owns the actor, collision, interruption
//! and equipment; the Frontier program advances only while its clip is current.
use crate::{
    combat,
    fx::{self, ChargeEffect},
    guard_body::Hold,
    motion, program, sound,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhfz_sdk::{
    action::{Cursor, Step},
    effects::MotionClock,
    sound::Clock,
    sprint::{self, Flick},
};
use mhgu_sdk::weapon::style::{
    ActionRequest, Input, NATIVE_ACTION, PASS, action,
    action::{
        MotionFrame,
        command::{BufferedCommand, CommandInput, CommandSet, MAX_COMMAND_FRAME},
    },
    guard,
    input::{attack_chord_pressed, attack_chord_with_shoulder, attack_combo_pending},
    movement,
};
pub(super) const DASH: u8 = 200;
const DASH_SLOT: u16 = action::slot(DASH);
const BRAKE: u8 = 201;
const IDLE: u8 = 255;
const DEFLECTION: u8 = 3;
pub(super) const ACTIONS: action::Actions = action::Actions::new(74).with(DASH).with(BRAKE);
const GUARD_GROUP: u16 = 32;
const GUARD_MOTION: u32 = 5005;
const MONSTER_GUARD_SUCCESS: u8 = 66;
const OTHER_GUARD_SUCCESS: u8 = 72;
const GUARD_CANCEL_FRAME: i16 = 56;
const SHEATHED_JUMP_SLASH: u16 = 38;
const DRAWN_JUMP_SLASH: u16 = 67;
const JUMP_COMBO_START: i16 = 54;
const JUMP_COMBO_END: i16 = 158;
const AIR_FINISHER_BUFFER: f32 = 12.;
const FINISHER_TURN_LIMIT: i16 = 0x2000;
const SWEEP: u8 = 0;
const VERTICAL_FINISHER: u8 = 40;
const RIGHT_FINISHER: u8 = 43;
const LEFT_FINISHER: u8 = 44;
// GU w00 pl_w00.fup float rows 1–3, in 60 Hz clock units.
const GU_CHARGE_THRESHOLDS: [f32; 3] = [50., 100., 140.];
const UPPERCUT_FOURTH_TIER_INTERVALS: f32 = 4.65;

const fn roll_commands(slash: u8, accept_until: i16) -> [BufferedCommand; 2] {
    // FZ 811D91FE buffers from 30 and opens the follow-up at 44.
    let uppercut = BufferedCommand {
        group: action::GROUP,
        slot: action::slot(53),
        input: CommandInput::AttackChord(1),
        accept_from: 30,
        execute_from: 44,
        accept_until,
    };
    [
        uppercut,
        BufferedCommand {
            slot: action::slot(slash),
            input: CommandInput::WeaponAfterCombo(5),
            ..uppercut
        },
    ]
}

// GU table 2011 tries 16:41 before these rows when a staggered chord also sets weapon 12.
static FORWARD_ROLL: CommandSet = CommandSet::new(&roll_commands(26, 54)).replacing_weapon(41);
static LEFT_ROLL: CommandSet = CommandSet::new(&roll_commands(26, 64)).replacing_weapon(41);
static RIGHT_ROLL: CommandSet = CommandSet::new(&roll_commands(27, 64)).replacing_weapon(41);
// The retail drawn GS table has three ledge routes with drawn animations.
static DASH_TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, movement::CLIMBS);
// GU table 2010 contains an immediate 1:6 roll and a stock guard attack.
// Keep its Hunter Art command and use the Frontier window for B.
static GUARD_RECOVERY: CommandSet = CommandSet::new(&[BufferedCommand {
    group: 1,
    slot: 6,
    input: CommandInput::Common(3),
    accept_from: GUARD_CANCEL_FRAME,
    execute_from: GUARD_CANCEL_FRAME,
    accept_until: MAX_COMMAND_FRAME,
}])
.retaining_native(0x8010, &[241]);

// GU's 2008 landing table gives the jump slash a 54..158 combo window.
static JUMP_LANDING: CommandSet = CommandSet::new(&[BufferedCommand {
    group: action::GROUP,
    slot: action::slot(SWEEP),
    input: CommandInput::Weapon(2),
    accept_from: JUMP_COMBO_START,
    execute_from: JUMP_COMBO_START,
    accept_until: JUMP_COMBO_END,
}])
.replacing_weapon(44); // GU strong wide slash

// FZ 817C0982 starts the emitter at frame 84 of clip 1465.
const RADIANCE_CLIP: u16 = 1465;
const RADIANCE_START: f32 = 84.;
const RADIANCE_INTERVAL: f32 = 12.;
const RADIANCE_READY_INTERVAL: f32 = 24.;
// Frames 84 through 276, with the final contact expiring at recovery (288).
const RADIANCE_PULSES: u8 = 17;
const fn synthetic(clip: u16, op: u16) -> Step {
    Step {
        op,
        clip,
        start: 0,
        blend: 4,
        until: 0,
        loops: 1,
    }
}
fn instruction(c: &Cursor) -> Option<Step> {
    match (c.state, c.time.step) {
        (DASH, 0) => Some(synthetic(1030, 5)),
        (BRAKE, 0) => Some(synthetic(1031, 3)),
        _ if c.state < 74 => program::step(c.state, c.time.step),
        _ => None,
    }
}
fn charge_state(state: u8) -> bool {
    matches!(state, 19..=28 | 47..=53 | 70 | 71)
}
fn charge_cap(state: u8) -> u8 {
    if matches!(state, 47..=53 | 70) { 4 } else { 3 }
}

fn charge_animation_rate(cap: u8) -> f32 {
    if cap == 3 { 0.5 } else { 1. }
}

pub(super) unsafe fn read<T: Copy>(p: u32, n: u32) -> T {
    unsafe { ((p + n) as *const T).read_unaligned() }
}
pub(super) unsafe fn write<T>(p: u32, n: u32, x: T) {
    unsafe { ((p + n) as *mut T).write_unaligned(x) }
}
macro_rules! native {
    ($a:expr $(,$x:expr)*)=>{{
        let f:unsafe extern "C" fn($(native!(@t $x)),*)->u32=core::mem::transmute(forge::mem::text_addr()+$a);
        f($($x),*)
    }};
    (@t $x:expr)=>{u32};
}
pub(super) unsafe extern "C" fn commands(p: u32, table_id: u32) -> *const CommandSet {
    let (group, slot) = unsafe { action::current(p) };
    match (table_id, group, slot) {
        (2000, action::GROUP, DASH_SLOT) => &DASH_TERRAIN,
        (2008, action::GROUP, SHEATHED_JUMP_SLASH | DRAWN_JUMP_SLASH) => &JUMP_LANDING,
        (2010, GUARD_GROUP, 0 | 1 | 9) if unsafe { native!(0x53a058, p) } == GUARD_MOTION => {
            &GUARD_RECOVERY
        }
        (2011, 1, 6) => match unsafe { native!(0x53a058, p) } {
            28 => &FORWARD_ROLL,
            5012 => &LEFT_ROLL,
            5013 => &RIGHT_ROLL,
            _ => core::ptr::null(),
        },
        _ => core::ptr::null(),
    }
}

pub(super) unsafe fn local(p: u32) -> bool {
    unsafe {
        if p == 0 || read::<u32>(p, 0) != forge::mem::text_addr() + 0x1749a88 {
            return false;
        }
        mhgu_sdk::weapon::style::hunter::local() == Some(p)
            && mhgu_sdk::weapon::style::hunter::equipped_weapon(p) == Some(0)
            && mhgu_sdk::weapon::style::selected(0) == Some(crate::STYLE_ID)
    }
}
mod peer;
pub(super) use peer::PLAYBACK;
pub(super) use peer::{
    edit_guard_hold as edit_peer_guard_hold, guarding_slot as guarding_peer_slot, peer_action,
};

struct Battle {
    peer_slot: usize,
    peer_guard: peer::GuardBody,
    guard_hold: Option<Hold>,
    audio: Clock,
    motion_fx: MotionClock,
    owner: u32,
    equip: u32,
    guard: u8,
    cursor: Cursor,
    packed: u16,
    charge: f32,
    gu_charge_time: f32,
    charge_interval: f32,
    charging: bool,
    charge_effect: Option<ChargeEffect>,
    draw_released: bool,
    uppercut_recovery_x: bool,
    chord_x_held: bool,
    level: u8,
    hit: bool,
    deflected: bool,
    radiance_delay: Option<f32>,
    radiance_pulses_left: u8,
    radiance_yaw: u16,
    radiance_contact: Option<([f32; 3], u32)>,
    aura_time: f32,
    flick: Flick,
    resume_sprint: sprint::Resume,
    guard_dash: bool,
    roll_yaw: Option<u16>,
    jump_finisher: Option<f32>,
    poise: f32,
    sustain_poise: bool,
}

impl Battle {
    fn charge_level(
        &mut self,
        was_charging: bool,
        delta: f32,
        elapsed: f32,
        cap: u8,
        wrapped: bool,
    ) -> u8 {
        // A newly armed charge already counted the trigger tick in event 30.
        if was_charging {
            // The wrap fallback already uses GU elapsed time. Otherwise undo
            // the vertical charge's animation multiplier before counting FZ ticks.
            let rate = if wrapped {
                1.
            } else {
                charge_animation_rate(cap)
            };
            self.charge += delta * 0.5 / rate;
            if delta > 0. {
                self.gu_charge_time += elapsed;
            }
        }
        // Frontier's uppercut has a fourth tier beyond GU's three thresholds.
        if cap == 4 && self.charge >= UPPERCUT_FOURTH_TIER_INTERVALS * self.charge_interval {
            return 4;
        }
        let skill_rate = self.charge_interval / 20.;
        GU_CHARGE_THRESHOLDS
            .into_iter()
            .take_while(|threshold| self.gu_charge_time >= threshold * skill_rate)
            .count() as u8
    }

    fn fresh(p: u32, equip: u32) -> Self {
        Self {
            peer_slot: 0,
            peer_guard: peer::GuardBody::Off,
            guard_hold: None,
            audio: Default::default(),
            motion_fx: Default::default(),
            owner: p,
            equip,
            guard: 0,
            cursor: program::cursor(IDLE),
            packed: 0xffff,
            charge: 0.,
            gu_charge_time: 0.,
            charge_interval: 20.,
            charging: false,
            charge_effect: None,
            draw_released: false,
            uppercut_recovery_x: false,
            chord_x_held: false,
            level: 0,
            hit: false,
            deflected: false,
            radiance_delay: None,
            radiance_pulses_left: 0,
            radiance_yaw: 0,
            radiance_contact: None,
            aura_time: 0.,
            flick: Default::default(),
            resume_sprint: sprint::Resume::default(),
            guard_dash: false,
            roll_yaw: None,
            jump_finisher: None,
            poise: 0.,
            sustain_poise: false,
        }
    }
    fn cancel(&mut self) {
        // GU's common action transition releases the previous state's stamina
        // latch. Do not clear it here after a guard/damage state has taken over.
        self.charge_effect = None;
        self.radiance_contact = None;
        self.peer_guard = peer::GuardBody::Off;
        self.guard_hold = None;
        self.cursor = program::cursor(IDLE);
        self.motion_fx.seek(0, 0.);
        self.packed = 0xffff;
        self.charging = false;
        self.hit = false;
        self.deflected = false;
        self.roll_yaw = None;
        self.jump_finisher = None;
    }
    unsafe fn cancel_attack(&mut self, p: u32) {
        self.cancel();
        unsafe { write(p, 12325, 0u8) };
    }
    unsafe fn finish(&mut self, p: u32) {
        unsafe {
            self.cancel_attack(p);
            if self.peer_slot == 0 {
                native!(0x2b6e5c, p);
            }
        }
    }
    unsafe fn evade(&mut self, p: u32) -> bool {
        unsafe {
            motion::ROLL.remember(
                p,
                self.roll_yaw.take().unwrap_or_else(|| motion::roll_yaw(p)),
            );
            self.cancel_attack(p);
            // Stock drawn roll: 2CBFCC case 6 -> 2CF8A4, including its stamina,
            // invulnerability, collision and recovery handling.
            native!(0x2ab670, p, 1, 6, 0);
            true
        }
    }
    unsafe fn start_step(&mut self, p: u32, frame: Option<u16>) -> bool {
        unsafe {
            let Some(s) = instruction(&self.cursor) else {
                self.finish(p);
                return true;
            };
            match s.op {
                0 | 1 => {
                    self.finish(p);
                    true
                }
                2 if self.peer_slot == 0 => self.begin(p, s.clip as u8),
                2 => {
                    self.cancel();
                    true
                }
                3..=5 => {
                    let start = frame.unwrap_or(s.start);
                    let blend = if frame.is_some() { 0 } else { s.blend };
                    let Some(id) = motion::play(p, s.clip, start, blend) else {
                        self.finish(p);
                        return true;
                    };
                    self.packed = id;
                    let start = f32::from(start);
                    sound::update(&mut self.audio, p, Some(s.clip), start, Some(start));
                    self.motion_fx.seek(s.clip, start);
                    self.cursor.time.cycles = 0;
                    self.cursor.time.frame = start;
                    self.cursor.time.blend = f32::from(blend / 2);
                    if s.clip == RADIANCE_CLIP {
                        self.radiance_delay = None;
                        self.radiance_pulses_left = RADIANCE_PULSES;
                    }
                    self.events(p);
                    true
                }
                _ => {
                    self.cancel();
                    false
                }
            }
        }
    }
    unsafe fn begin(&mut self, p: u32, state: u8) -> bool {
        unsafe { self.begin_at(p, state, None) }
    }
    unsafe fn begin_at(&mut self, p: u32, state: u8, frame: Option<u16>) -> bool {
        unsafe {
            // An accepted GU request has already run the common transition.
            let requested =
                action::current(p) == (action::GROUP, action::slot(state)) && read::<u8>(p, 5) == 0;
            let cursor = program::cursor(state);
            if instruction(&cursor)
                .and_then(|s| motion::physical(p, s.clip))
                .is_none()
            {
                if requested {
                    self.finish(p);
                }
                return requested;
            }
            if !requested && !action::enter(p, state) {
                return false;
            }
            self.resume_sprint.set(false);
            self.charge_effect = None;
            self.radiance_contact = None;
            self.cursor = cursor;
            self.guard_hold = None;
            if !is_guard_success(state) {
                self.peer_guard = peer::GuardBody::Off;
            }
            self.roll_yaw = None;
            self.jump_finisher = None;
            self.hit = false;
            self.deflected = false;
            self.charging = false;
            self.draw_released = false;
            self.uppercut_recovery_x = false;
            if charge_state(state) {
                self.level = 0;
                self.charge = 0.;
                // GU's own GS charge timer (11681DC) checks these Focus and
                // Slow Charge equipment skills. FZ changes 40/20 to 32/16 or 48/24.
                self.charge_interval = match action::ChargeSkill::equipped(p) {
                    action::ChargeSkill::Focus => 16.,
                    action::ChargeSkill::Slow => 24.,
                    action::ChargeSkill::Normal => 20.,
                };
            }
            self.poise = 0.;
            self.sustain_poise = false;
            write(p, 12325, 0u8);
            write(p, 12320 + 176, 0u32);
            write(p, 5, 1u8);
            native!(
                0x282fd8,
                p,
                0,
                u32::from(!matches!(state, DASH | DEFLECTION)),
                0
            );
            native!(0x2a2560, p, 1);
            if matches!(state, 13 | 20 | 38 | 47) {
                native!(0x2a264c, p, 1); // GU draw-attack bookkeeping/skills
            }
            if state == DASH && self.peer_slot == 0 {
                // GU's ground collision (2A9098) uses the roll bit for ledge drops.
                native!(0x283238, p, 0, 0x1180, 0);
                native!(0x2e9b7c, p, 2000, 0);
                native!(0x2a25a8, p, 1);
            }
            self.start_step(p, frame)
        }
    }
    unsafe fn events(&mut self, p: u32) {
        unsafe {
            if self.cursor.state >= 74 {
                return;
            }
            while let Some((kind, param, late)) = self.cursor.next_event() {
                match kind {
                    5 => {
                        self.sustain_poise = false;
                        self.poise = (f32::from(param) * 2. - late).max(0.);
                        write(p, 10288, self.poise.max(read::<f32>(p, 10288)));
                    }
                    6 => self.sustain_poise = true,
                    // The GU evasion initializer (2A6844) writes this clock;
                    // 2F5780 reads it in the incoming damage decision.
                    7 | 8 if param != 0 => {
                        let remaining = (f32::from(param) * 2. - late).max(0.);
                        write(p, 11072, remaining.max(read::<f32>(p, 11072)));
                        if let peer::GuardBody::Pending(area) = self.peer_guard {
                            self.peer_guard = peer::GuardBody::Active { area, remaining };
                        }
                        if self.peer_slot == 0
                            && is_guard_success(self.cursor.state)
                            && remaining > 0.
                        {
                            crate::guard_body::arm_impact(p);
                        }
                    }
                    12 if self.peer_slot == 0 => movement::stamina(p, -150., false),
                    4 if self.peer_slot == 0 => {
                        let dir = read::<u32>(p, 4076).wrapping_add(u32::from(param)) & 0xffff;
                        write(p, 4076, dir);
                        write(p, 5280, dir);
                    }
                    13 => {
                        self.charge = 0.;
                        self.level = 0;
                    }
                    14 => self.level = param as u8,
                    16 | 17 => {
                        // 811D84FE passes the deflection flag to 811D83CE in these states.
                        let deflectable = matches!(self.cursor.state, 0..=2 | 5..=18 | 29..=32 | 37..=39 | 68 | 69);
                        combat::emit(p, param, late, deflectable);
                    }
                    18 => {
                        combat::emit(p, program::charged_row(self.level), late, false);
                    }
                    19 => {
                        combat::emit(p, 110 + u16::from(self.level.min(5)), late, false);
                    }
                    24..=26 if !self.hit => fx::ground_strike(p, kind, self.level),
                    28 if param == 4 && self.level >= 2 => fx::charged_slash(p),
                    // Frontier event 29 requests the hunter origin (joint argument 0).
                    29 => fx::emit(p, param, None),
                    30 => {
                        self.charging = true;
                        // 811D84FE increments +2852 on the tick that event 30 arms it.
                        self.charge = 1. + late * 0.5;
                        self.gu_charge_time = read::<f32>(p, 28).max(0.) + late;
                        let rate = charge_animation_rate(charge_cap(self.cursor.state));
                        if rate != 1. {
                            action::set_motion_rate(p, rate);
                        }
                    }
                    31 => {
                        crate::sound::play(p, false, if self.guard < 2 { 2 } else { 3 });
                        if self.guard < 3 {
                            self.guard += 1;
                            if self.guard == 3 {
                                fx::radiance_ready(p);
                                self.aura_time = 0.;
                            }
                        }
                    }
                    32 => self.guard = 0,
                    _ => {}
                }
            }
        }
    }
    unsafe fn begin_finisher(&mut self, p: u32) -> bool {
        unsafe {
            let offset = movement::live_relative_stick_yaw(p)
                .map(|yaw| (yaw as i16).wrapping_neg())
                .unwrap_or(0);
            // The side clips already turn the hunter by 45 degrees through event 4.
            let state = if offset >= FINISHER_TURN_LIMIT {
                RIGHT_FINISHER
            } else if offset <= -FINISHER_TURN_LIMIT {
                LEFT_FINISHER
            } else {
                VERTICAL_FINISHER
            };
            let facing = read::<u16>(p, 4076);
            if !action::transition_to(p, action::GROUP, action::slot(state)) {
                return false;
            }
            if state == VERTICAL_FINISHER {
                movement::set_facing(p, facing.wrapping_add_signed(offset));
            }
            self.begin(p, state)
        }
    }

    unsafe fn jump_followup(&mut self, p: u32, phase: u8, x_pressed: bool) -> bool {
        unsafe {
            let frame = read::<f32>(p, 1280);
            if !matches!(phase, 1 | 2) || (phase == 2 && frame >= f32::from(JUMP_COMBO_END)) {
                self.jump_finisher = None;
                return false;
            }
            if x_pressed {
                self.jump_finisher = Some(AIR_FINISHER_BUFFER);
            } else if phase == 1 {
                let elapsed = read::<f32>(p, 28).max(0.);
                self.jump_finisher = self
                    .jump_finisher
                    .map(|remaining| remaining - elapsed)
                    .filter(|remaining| *remaining > 0.);
            }
            phase == 2
                && frame >= f32::from(JUMP_COMBO_START)
                && self.jump_finisher.is_some()
                && self.begin_finisher(p)
        }
    }

    unsafe fn begin_branch(&mut self, p: u32, branch: u16) -> bool {
        unsafe {
            let next = match branch {
                40 | 43 | 44 => return self.begin_finisher(p),
                65 => {
                    if !action::transition_to(p, GUARD_GROUP, 0) {
                        return false;
                    }
                    self.cancel();
                    return true;
                }
                74 => {
                    motion::ROLL.remember(p, self.roll_yaw.unwrap_or_else(|| motion::roll_yaw(p)));
                    if !action::transition_to(p, 1, 6) {
                        motion::ROLL.clear_for(p);
                        return false;
                    }
                    self.cancel_attack(p);
                    return true;
                }
                75 => DASH,
                76 => 22,
                77 => 24,
                78 => 21,
                79 => 25,
                80 => 49,
                81 => 50,
                82 => 51,
                83 => 52,
                84 => 32,
                85 => match self.cursor.state {
                    22 => 30,
                    24 => 31,
                    _ => 29,
                },
                86 => 54 + self.level.min(4),
                87 => 68,
                88 | 89 => 69,
                n if n < 74 => n as u8,
                _ => return false,
            };
            peer::publish(p, self.level);
            // The draw windup and release are one attack. GU's command table
            // drops the draw flag, so pass it to the common transition here.
            let accepted = if matches!((self.cursor.state, branch), (20, 85) | (47, 86)) {
                action::enter_draw_attack(p, next)
            } else {
                action::transition_to(p, action::GROUP, action::slot(next))
            };
            if !accepted {
                return false;
            }
            motion::aim(p, branch);
            self.begin(p, next)
        }
    }
    unsafe fn advance(
        &mut self,
        p: u32,
        input: &Input,
        dash_input: bool,
        uppercut_input: bool,
    ) -> bool {
        unsafe {
            let state = self.cursor.state;
            self.deflected |= action::melee::take_deflection(p);
            if self.deflected && action::transition_to(p, action::GROUP, action::slot(DEFLECTION)) {
                self.level = 0;
                action::melee::clear_hitstop(p);
                self.begin(p, DEFLECTION);
                return true;
            }
            let cap = charge_cap(state);
            let uppercut_charge = cap == 4;
            let Some(s) = instruction(&self.cursor) else {
                return false;
            };
            let sample = MotionFrame::sample(p);
            let frame = sample.frame;
            let previous = self.cursor.time.frame;
            if state == 20 && self.cursor.time.step == 0 {
                // GU's draw-charge dispatcher latches a release during the
                // first ten motion frames (1161A14..1161A34). Use its mapped
                // charge command; a tap continues through FZ's normal draw
                // at the current frame, never through the charge loop.
                if previous < 10. && !input.weapon_command(5) {
                    self.draw_released = true;
                }
                if frame >= 10.
                    && self.draw_released
                    && action::transition_to(p, action::GROUP, action::slot(13))
                {
                    return self.begin_at(p, 13, Some(frame.min(36.) as u16));
                }
            }
            let wrapped = self.cursor.time.observe(frame, sample.blend);
            let elapsed = read::<f32>(p, 28).max(0.);
            let delta = if wrapped {
                elapsed
            } else {
                (frame - previous).max(0.)
            };
            write(p, 9472, read::<u32>(p, 9472) & !1);
            fx::motion_events(&mut self.motion_fx, p, s.clip, previous, sample);
            // 169D44 records the actual target and its type in this sword's
            // volume. Type 1 is a monster, type 0 is a hunter. The allocator
            // clears the record on each swing. No shared DPS hook is needed.
            self.hit |= read::<u32>(p, 12320 + 176) != 0 && read::<u8>(p, 12320 + 180) == 1;
            self.poise = (self.poise - delta).max(0.);
            let was_charging = self.charging;
            self.events(p);
            if (matches!(state, DASH | BRAKE)
                || (state < 74 && program::hunter_art_window(&self.cursor, self.hit)))
                && action::activate_hunter_art(p)
            {
                self.cancel_attack(p);
                return true;
            }
            if self.sustain_poise {
                self.poise = self.poise.max(8.);
                write(p, 10288, self.poise.max(read::<f32>(p, 10288)));
            }
            // The extra shoulder has no GU combat command; the registry uses
            // GU's own binding query for Frontier's additional brace action.
            let brace = self.poise > 0.
                && state != DEFLECTION
                && state < 74
                && input.auxiliary_pressed != 0
                && read::<f32>(p, 5088) >= 150.
                && read::<f32>(p, 10296) <= 0.;
            if brace {
                movement::stamina(p, -150., false);
                crate::sound::play(p, false, 1);
                fx::emit(p, 4, Some(0));
                let duration = if charge_state(state) {
                    240.
                } else {
                    self.poise
                };
                write(p, 10296, duration);
            }
            // A buffered X release can wait for the swing gate. Keep the
            // earned tier during that wait, even with GU's earlier thresholds.
            if self.charging && delta > 0. && (uppercut_charge || input.weapon_command(5)) {
                let old = self.level;
                self.level = self.charge_level(was_charging, delta, elapsed, cap, wrapped);
                if self.level > old && self.level == 4 {
                    fx::emit(p, 0, Some(8));
                    crate::sound::play(p, true, 210);
                }
                let timeout = self.charge_interval * f32::from(cap + 1) + 10.;
                let expired = if uppercut_charge {
                    self.charge > timeout
                } else {
                    self.charge >= timeout
                };
                if expired {
                    let branch = if uppercut_charge { 86 } else { 85 };
                    if uppercut_charge {
                        // 811D84FE resets +2856 to level two on overcharge;
                        // manual release during the fourth tier still uses row 168.
                        self.level = 2;
                    }
                    if self.begin_branch(p, branch) {
                        return true;
                    }
                }
            }
            if state == DASH {
                let has_direction = movement::steer(p);
                if delta > 0. {
                    // The native continuous drain already multiplies by GU's
                    // frame delta. Disable recovery and supply the rate once.
                    movement::drain_running_stamina(p);
                }
                if sprint::can_brake(frame, self.cursor.time.cycles) {
                    if input.command(13) {
                        self.cancel();
                        native!(0x2ab670, p, 1, 2, 0); // shared sheathe and equipment change
                        return true;
                    }
                    if input.command(3) {
                        return self.evade(p);
                    }
                    if uppercut_input {
                        return self.begin(p, 70);
                    }
                    if input.command(70) {
                        self.cancel();
                        native!(0x2ab670, p, u32::from(GUARD_GROUP), 0, 0);
                        return true;
                    }
                    if input.weapon_command(12) {
                        return self.begin(p, 64);
                    }
                    if read::<f32>(p, 5088) <= 0. || !has_direction {
                        return self.begin(p, BRAKE);
                    }
                }
            } else if state < 74 {
                let mut cmd = 0u32;
                let x_held = input.weapon_command(5);
                let a_held = input.weapon_command(22);
                let x_pressed = input.weapon_command(12);
                // The GS mapper reports held X before GU resolves the chord.
                let single_x =
                    !uppercut_input && !self.chord_x_held && !a_held && !attack_combo_pending(p);
                let vertical_input = if matches!(state, 54..=58) {
                    // Only an X resolved by GU can start a held vertical
                    // follow-up. The X held by an early chord cannot queue it.
                    self.uppercut_recovery_x =
                        single_x && x_held && (x_pressed || self.uppercut_recovery_x);
                    single_x && (x_pressed || self.uppercut_recovery_x)
                } else {
                    single_x && (x_held || x_pressed)
                };
                let side = if (x_held || a_held) && native!(0x27e608, p, 0) > 0 {
                    // GU measures sectors relative to the camera and the actor,
                    // including the alternate controller setup.
                    if native!(0x27e638, p, 0, 0x2000, 0x6000, 1) != 0 {
                        1
                    } else if native!(0x27e638, p, 0, 0xa000, 0xe000, 1) != 0 {
                        2
                    } else {
                        0
                    }
                } else {
                    0
                };
                if input.command(8) {
                    cmd |= 1 << 18;
                } else if uppercut_input {
                    cmd |= 1 << 15;
                    if input.command(0) {
                        cmd |= 1 << 28 | 1 << (28 + side);
                    }
                    if side == 2 {
                        cmd |= 1 << 16;
                    }
                } else if vertical_input {
                    cmd |= 1 << 10 | 1 << (10 + side);
                    if x_pressed {
                        cmd |= 1 << 3 | 1 << (3 + side) | 1 << 23 | 1 << (23 + side);
                    }
                } else if input.weapon_command(2) {
                    cmd |= 1 << 13;
                    if input.command(0) {
                        cmd |= 1 << 26;
                        if side == 1 {
                            cmd |= 1 << 27;
                        }
                    }
                    if side == 1 {
                        cmd |= 1 << 14;
                    }
                }
                // w00 command 19 accepts any new blade attack during recoil.
                if state == DEFLECTION
                    && (input.weapon_command(12)
                        || input.weapon_command(1)
                        || input.weapon_command(2))
                {
                    cmd |= 1 << 19;
                }
                if !x_held {
                    cmd |= 1 << 6
                }
                if !(x_held && a_held) {
                    cmd |= 1 << 17
                }
                if !uppercut_input && input.command(70) {
                    cmd |= 1 << 31
                }
                if input.command(3) {
                    cmd |= 1 << 1
                }
                if dash_input {
                    cmd |= 1 << 2
                }
                let hit = self.hit;
                let guard = self.guard;
                let branch = self.cursor.branch(cmd, |q| match q {
                    0 | 4 => true,
                    2 => hit,
                    7 => guard >= 3,
                    _ => false,
                });
                if self.cursor.pending_action() == Some(74) {
                    self.roll_yaw.get_or_insert_with(|| motion::roll_yaw(p));
                } else {
                    self.roll_yaw = None;
                }
                if let Some(branch) = branch
                    && self.begin_branch(p, branch)
                {
                    return true;
                }
            }
            // For the six-loop emitter, GU's layer-end flag leads the displayed
            // wrap by one tick. Keep its last active window through that tick.
            let completion = MotionFrame {
                ended: s.clip != RADIANCE_CLIP && sample.ended,
                ..sample
            };
            if s.complete(self.cursor.time, completion, wrapped) {
                self.cursor.time.step += 1;
                return self.start_step(p, None);
            }
            if s.clip == RADIANCE_CLIP {
                self.radiance(p, previous, frame);
            }
            if self.charging {
                let held_charge =
                    input.weapon_command(5) && (!uppercut_charge || input.weapon_command(22));
                // Resolve the release branch before creating the charge proof or
                // playing its start cue. A tapped draw reaches the charge step
                // with X already released; it must not flash a new charge effect.
                if held_charge {
                    self.charge_effect
                        .get_or_insert_with(|| ChargeEffect::start(p, self.peer_slot))
                        .update(p, self.charge, self.level);
                } else {
                    self.charge_effect = None;
                }
            }
            true
        }
    }
    unsafe fn radiance(&mut self, p: u32, previous: f32, frame: f32) {
        if self.radiance_pulses_left == 0 {
            return;
        }
        unsafe {
            let yaw = read(p, 4076);
            let late = if let Some(delay) = self.radiance_delay {
                // The emitter outlives motion pauses. Six Frontier ticks are
                // twelve units of GU game time at either playback rate.
                let remaining = delay - read::<f32>(p, 28).max(0.);
                if remaining > 0. {
                    self.radiance_delay = Some(remaining);
                    return;
                }
                -remaining % RADIANCE_INTERVAL
            } else if previous < RADIANCE_START && frame >= RADIANCE_START {
                self.radiance_yaw = yaw;
                (frame - RADIANCE_START) % RADIANCE_INTERVAL
            } else {
                return;
            };
            self.radiance_delay = Some(RADIANCE_INTERVAL - late);
            self.radiance_pulses_left -= 1;
            let origin = fx::shockwave_origin(p, yaw);
            self.radiance_contact =
                combat::emit(p, program::RADIANCE, late, false).map(|shapes| (origin, shapes));
            fx::radiance(p, origin, self.radiance_yaw);
        }
    }
    unsafe fn tick(&mut self, p: u32, input: &Input) -> bool {
        unsafe {
            let current = action::current(p);
            let (group, act) = current;
            let jump_slash =
                group == action::GROUP && matches!(act, SHEATHED_JUMP_SLASH | DRAWN_JUMP_SLASH);
            // With the stick held, GU's drawn climb exits directly to walk 2:30.
            let ground_movement = movement::ground_idle(current);
            if current != (1, 6) {
                motion::ROLL.clear_for(p);
            }
            let phase: u8 = read(p, 5);
            let physical: u16 = read(p, 1204);
            let clip = motion::source_clip(p, physical);
            let uppercut_input = input.weapon_command(1)
                || attack_chord_with_shoulder(p)
                // GU can report standalone X if A is still held from the sweep.
                || (clip == Some(1406) && attack_chord_pressed(p));
            // Releasing A first must not turn an early chord into a new X.
            self.chord_x_held = input.weapon_command(5) && (uppercut_input || self.chord_x_held);
            if self.guard != 3 {
                self.aura_time = 0.;
            } else if native!(0x2a2444, p) == 0 {
                // FZ 8115B4D0 primes the next pulse while the sword is sheathed.
                self.aura_time = RADIANCE_READY_INTERVAL;
            } else {
                if self.aura_time >= RADIANCE_READY_INTERVAL {
                    self.aura_time %= RADIANCE_READY_INTERVAL;
                    fx::radiance_ready(p);
                }
                self.aura_time += read::<f32>(p, 28).max(0.);
            }
            sound::update(&mut self.audio, p, clip, read(p, 1280), None);
            let dash_input = self.flick.update(input, read::<f32>(p, 28).max(0.));
            let was_sprinting = self.cursor.state == DASH;
            if self.cursor.state != IDLE {
                if current == (action::GROUP, action::slot(self.cursor.state))
                    && physical == self.packed
                    && phase != 0
                    && read::<u8>(p, 9493) & 4 == 0
                {
                    return self.advance(p, input, dash_input, uppercut_input);
                }
                self.cancel(); // damage, sheathing, warp or another GU action took ownership
            }
            // Preserve a pending run through the native jump slash and its landing window.
            if !jump_slash {
                self.jump_finisher = None;
                let drawn = native!(0x2a2444, p) != 0;
                self.resume_sprint
                    .on_native(p, current, ground_movement, drawn, was_sprinting);
                // A normal roll off a ledge can buffer a fresh run on landing.
                if drawn
                    && dash_input
                    && matches!(current, (4, 3) | (1, 138))
                    && matches!(action::previous(p), (4, 14 | 23))
                {
                    self.resume_sprint.set(true);
                }
            }
            if let Some(id) = action::custom_id(group, act) {
                if phase == 0 {
                    return self.begin(p, id);
                }
                // A lost clip or a style switched during a weapon action must
                // return through the shared lifecycle, never the stock GS table.
                self.finish(p);
                return true;
            }
            if group == action::GROUP {
                if !jump_slash {
                    return false;
                }
                let x_pressed = input.weapon_command(12) && !uppercut_input && !self.chord_x_held;
                if self.jump_followup(p, phase, x_pressed) {
                    return true;
                }
                if phase == 2
                    && dash_input
                    && (f32::from(JUMP_COMBO_START)..f32::from(JUMP_COMBO_END))
                        .contains(&read::<f32>(p, 1280))
                    && read::<f32>(p, 5088) > 0.
                {
                    return self.begin(p, DASH);
                }
                return false;
            }
            if group == GUARD_GROUP && matches!(act, 0 | 1 | 9 | 12 | 14) {
                if phase == 0 {
                    self.guard_dash = false;
                }
                if phase != 0 && motion::decode(p, u32::from(physical)) == GUARD_MOTION {
                    let frame = read::<f32>(p, 1280);
                    self.guard_dash |= frame >= 20. && dash_input;
                    if frame >= f32::from(GUARD_CANCEL_FRAME)
                        && self.guard_dash
                        && read::<f32>(p, 5088) > 0.
                    {
                        return self.begin(p, DASH);
                    }
                    if read::<u8>(p, 1206) & 4 != 0 || frame >= read::<f32>(p, 1284) {
                        self.finish(p);
                    }
                    write(p, 9472, read::<u32>(p, 9472) & !1);
                    return true;
                }
                return false;
            }
            if group == GUARD_GROUP {
                if phase == 0
                    && let Some(block) = guard::take(p)
                {
                    return self.begin(
                        p,
                        match block {
                            guard::Contact::Monster => MONSTER_GUARD_SUCCESS,
                            guard::Contact::Other => OTHER_GUARD_SUCCESS,
                        },
                    );
                }
                return false;
            }
            if read::<u8>(p, 9493) & 4 != 0 {
                return false;
            }
            // FZ 817E9CB4 / 817EAA30: recover forwards or backwards from a
            // launch. Wait for GU's own landing test (2BB164), never replace
            // a lethal hit, an airborne trajectory or a different reaction.
            if group == 64
                && matches!(act, 6 | 7)
                && phase == 2
                && input.command(3)
                && read::<u8>(p, 11988) != 0
                && native!(0x2a2444, p) != 0
                && read::<i16>(p, 5078) > 0
                && read::<f32>(p, 5088) >= 150.
                && native!(0x53a058, p) == 1015
            {
                return self.begin(p, if act == 6 { 63 } else { 62 });
            }
            let has_stamina = read::<f32>(p, 5088) > 0.;
            let terrain_dash_ready = ground_movement
                && self
                    .resume_sprint
                    .ready(native!(0x27e608, p, 0) > 0, has_stamina);
            let accepts_ground_input = terrain_dash_ready
                || (phase != 0
                    && matches!(group, 1 | 2)
                    && matches!(native!(0x53a058, p), 5001 | 5004));
            if !accepts_ground_input {
                return false;
            }
            self.resume_sprint.set(false);
            if action::activate_hunter_art(p) {
                return true;
            }
            if input.command(13) {
                native!(0x2ab670, p, 1, 2, 0); // shared sheathe, motion 5003
                return true;
            }
            if self.guard >= 3 && input.command(8) {
                return self.begin(p, 59);
            }
            if uppercut_input {
                return self.begin(p, 48);
            }
            if input.command(70) {
                native!(0x2ab670, p, u32::from(GUARD_GROUP), 0, 0);
                return true;
            }
            if input.weapon_command(12) {
                return self.begin(p, 19);
            }
            if input.weapon_command(2) {
                return self.begin(p, SWEEP);
            }
            let wants_dash = dash_input || (terrain_dash_ready && !input.command(3));
            wants_dash && has_stamina && self.begin(p, DASH)
        }
    }
}

// The game can re-enter its action dispatcher while changing state. Only the
// outer call borrows the controller; contact callbacks publish through atomics.
struct Controller(AtomicBool, UnsafeCell<Option<Battle>>);
unsafe impl Sync for Controller {}
static CONTROL: Controller = Controller(AtomicBool::new(false), UnsafeCell::new(None));

pub(super) fn edit_local_guard_hold(actor: u32, edit: impl FnOnce(&mut Option<Hold>)) {
    if CONTROL.0.swap(true, Ordering::Acquire) {
        return;
    }
    if let Some(battle) = unsafe { &mut *CONTROL.1.get() }
        && battle.owner == actor
    {
        edit(&mut battle.guard_hold);
    }
    CONTROL.0.store(false, Ordering::Release);
}

pub(super) fn radiance_origin(owner: u32, shapes: u32) -> Option<[f32; 3]> {
    if let Some(origin) = unsafe { peer::radiance_origin(owner, shapes) } {
        return Some(origin);
    }
    if CONTROL.0.swap(true, Ordering::Acquire) {
        return None;
    }
    let origin = unsafe { &*CONTROL.1.get() }
        .as_ref()
        .filter(|battle| battle.owner == owner)
        .and_then(|battle| battle.radiance_contact)
        .filter(|&(_, resource)| resource == shapes)
        .map(|(origin, _)| origin);
    CONTROL.0.store(false, Ordering::Release);
    origin
}

pub(super) unsafe extern "C" fn request(p: u32, raw: *const ActionRequest) -> u32 {
    unsafe {
        let request = &*raw;
        // GU resolves the aerial input and selects the sheathed or drawn move.
        if !request.draw_route && matches!(request.slot, SHEATHED_JUMP_SLASH | DRAWN_JUMP_SLASH) {
            return NATIVE_ACTION;
        }
        if native!(0x2a2444, p) != 0 {
            return PASS;
        }
        let commands = read::<u32>(p, 5156);
        if !request.draw_route && !matches!(request.slot, 0 | 1 | 3 | 19 | 20) {
            return PASS;
        }
        if commands & (1 << 1) != 0 {
            47
        } else if request.draw_route {
            PASS
        } else if commands & (1 << 2) != 0 && commands & (1 << 12) == 0 {
            38
        } else if request.slot == 3 || commands & (1 << 5) == 0 {
            13
        } else {
            20
        }
    }
}
pub(super) unsafe extern "C" fn dispatch(p: u32, input: *const Input) -> u32 {
    if CONTROL.0.swap(true, Ordering::Acquire) {
        return PASS;
    }
    let handled = unsafe {
        let slot = &mut *CONTROL.1.get();
        if let Some(input) = input.as_ref() {
            let eq = read::<u32>(p, 5060);
            if slot.as_ref().is_some_and(|b| b.owner != p || b.equip != eq) {
                *slot = None;
            }
            let battle = slot.get_or_insert_with(|| Battle::fresh(p, eq));
            let result = battle.tick(p, input);
            peer::publish(p, battle.level);
            result
        } else {
            peer::publish(0, 0);
            if let Some(mut battle) = slot.take()
                && p != 0
                && battle.owner == p
            {
                let (group, slot) = action::current(p);
                if action::custom(group, slot) {
                    battle.finish(p);
                }
            }
            motion::ROLL.clear_for(p);
            false
        }
    };
    CONTROL.0.store(false, Ordering::Release);
    if handled { 0 } else { PASS }
}

pub(super) unsafe extern "C" fn guard_policy(
    p: u32,
    result: *mut guard::GuardResult,
) -> guard::Decision {
    unsafe {
        let packed = read::<u16>(p, 1204);
        if motion::decode(p, u32::from(packed)) != GUARD_MOTION {
            return guard::Decision::Native;
        }

        // FZ state 65 event 10 closes at frame 28, 30 with Guard +1
        // or 32 with Guard +2.
        let frame = read::<f32>(p, 1280);
        if frame >= 28. && frame >= 28. + f32::from(mhfz_sdk::combat::guard_bonus_frames(p)) {
            return guard::Decision::Reject;
        }
        // FZ 81744DA0 grants four simulation ticks before the follow-up
        // animation starts. Light recoil suppresses chip damage.
        result.write(guard::GuardResult {
            invulnerability: 8.,
            recoil: 0,
            sharpness_power: guard::GuardResult::NO_SHARPNESS_COST,
        });
        guard::Decision::Precise
    }
}

fn is_guard_success(state: u8) -> bool {
    matches!(state, MONSTER_GUARD_SUCCESS | OTHER_GUARD_SUCCESS)
}

unsafe fn guard_success_action(p: u32) -> bool {
    let (group, slot) = unsafe { action::current(p) };
    action::custom_id(group, slot).is_some_and(is_guard_success)
}

pub(super) unsafe fn guard_body_active(p: u32) -> bool {
    // These states are entered only after consuming a confirmed precise block.
    unsafe { guard_success_action(p) && read::<f32>(p, 11072) > 0. }
}
