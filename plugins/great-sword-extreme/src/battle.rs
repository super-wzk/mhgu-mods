//! Fight logic for the local hunter. GU owns the actor, collision, interruption
//! and equipment; the Frontier program advances only while its clip is current.
use crate::{
    combat,
    fx::{self, ChargeEffect},
    motion,
    program::{self, Cursor, Step},
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhgu_sdk::style::{
    ActionRequest, Input, NATIVE_ACTION, PASS, action,
    action::command::{BufferedCommand, CommandInput, CommandSet, MAX_COMMAND_FRAME},
    guard,
};
const DASH: u8 = 200;
const DASH_SLOT: u16 = action::slot(DASH);
const BRAKE: u8 = 201;
const IDLE: u8 = 255;
pub(super) const ACTIONS: action::Actions = action::Actions::new(74).with(DASH).with(BRAKE);
const GUARD_GROUP: u16 = 32;
const GUARD_MOTION: u32 = 5005;
const GUARD_CANCEL_FRAME: i16 = 56;
const LEDGE_CLIMBS: &[u16] = &[137, 138, 139];
const ROLL_OFF_LEDGE: (u16, u16) = (4, 14);
const LEDGE_LANDING: (u16, u16) = (4, 3);
const SHEATHED_JUMP_SLASH: u16 = 38;
const DRAWN_JUMP_SLASH: u16 = 67;
const JUMP_COMBO_START: i16 = 54;
const JUMP_COMBO_END: i16 = 158;
const SWEEP: u8 = 0;
const VERTICAL_FINISHER: u8 = 40;

const fn roll_commands(slash: u8, accept_until: i16) -> [BufferedCommand; 2] {
    // FZ 811D91FE buffers from 30 and opens the follow-up at 44.
    let uppercut = BufferedCommand {
        group: action::GROUP,
        slot: action::slot(53),
        input: CommandInput::Weapon(1),
        accept_from: 30,
        execute_from: 44,
        accept_until,
    };
    [
        uppercut,
        BufferedCommand {
            slot: action::slot(slash),
            input: CommandInput::Weapon(5),
            ..uppercut
        },
    ]
}

static FORWARD_ROLL: CommandSet = CommandSet::new(&roll_commands(26, 54));
static LEFT_ROLL: CommandSet = CommandSet::new(&roll_commands(26, 64));
static RIGHT_ROLL: CommandSet = CommandSet::new(&roll_commands(27, 64));
// The retail drawn GS table has three ledge routes with drawn animations.
static DASH_TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, LEDGE_CLIMBS);
static GUARD_RECOVERY: CommandSet = CommandSet::new(&[BufferedCommand {
    group: 1,
    slot: 6,
    input: CommandInput::Common(3),
    accept_from: GUARD_CANCEL_FRAME,
    execute_from: GUARD_CANCEL_FRAME,
    accept_until: MAX_COMMAND_FRAME,
}]);

// GU's 2008 landing table gives the jump slash a 54..158 combo window.
const JUMP_FINISHER: BufferedCommand = BufferedCommand {
    group: action::GROUP,
    slot: action::slot(VERTICAL_FINISHER),
    input: CommandInput::Weapon(12),
    accept_from: JUMP_COMBO_START,
    execute_from: JUMP_COMBO_START,
    accept_until: JUMP_COMBO_END,
};
static JUMP_LANDING: CommandSet = CommandSet::new(&[
    JUMP_FINISHER,
    BufferedCommand {
        slot: action::slot(SWEEP),
        input: CommandInput::Weapon(2),
        ..JUMP_FINISHER
    },
])
.replacing_weapon(44); // GU strong wide slash

// FZ 817C0982 starts the emitter at frame 84 of clip 1465.
const RADIANCE_CLIP: u16 = 1465;
const RADIANCE_START: f32 = 84.;
const RADIANCE_INTERVAL: f32 = 12.;
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
fn instruction(c: Cursor) -> Option<Step> {
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
unsafe fn read<T: Copy>(p: u32, n: u32) -> T {
    unsafe { ((p + n) as *const T).read_unaligned() }
}
unsafe fn write<T>(p: u32, n: u32, x: T) {
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
        let mgr = read::<u32>(forge::mem::text_addr(), 0x1885b58);
        let eq = read::<u32>(p, 5060);
        mgr != 0
            && eq != 0
            && read::<u8>(eq, 1236) == 0
            && native!(0x276278, mgr) == p
            && mhgu_sdk::style::selected(0) == 1
    }
}
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global extreme_stamina",
    ".hidden extreme_stamina",
    ".type extreme_stamina,%function",
    "extreme_stamina:",
    "vmov s0,r1",
    "mov r1,#0",
    "bx r2"
);
unsafe extern "C" {
    fn extreme_stamina(p: u32, amount: u32, entry: u32);
}
unsafe fn drain_stamina(p: u32, rate: f32) {
    unsafe {
        extreme_stamina(p, rate.to_bits(), forge::mem::text_addr() + 0x2a5ce4);
    }
}
unsafe fn spend_stamina(p: u32, cost: f32) {
    unsafe {
        // Discrete costs use GU's Constitution/evade path, without multiplying
        // by the frame delta as continuous sprint drain does.
        extreme_stamina(p, (-cost).to_bits(), forge::mem::text_addr() + 0x2a5ebc);
    }
}
struct Battle {
    audio: crate::sound::Clock,
    owner: u32,
    equip: u32,
    guard: u8,
    cursor: Cursor,
    packed: u16,
    charge: f32,
    charge_interval: f32,
    charging: bool,
    charge_effect: Option<ChargeEffect>,
    draw_released: bool,
    level: u8,
    hit: bool,
    radiance_delay: Option<f32>,
    radiance_yaw: u16,
    radiance_contact: Option<([f32; 3], u32)>,
    aura_time: f32,
    flick: program::Flick,
    terrain_dash_pending: bool,
    guard_dash: bool,
    roll_side: Option<u32>,
    poise: f32,
    sustain_poise: bool,
}
impl Battle {
    fn fresh(p: u32, equip: u32) -> Self {
        Self {
            audio: Default::default(),
            owner: p,
            equip,
            guard: 0,
            cursor: Cursor::new(IDLE),
            packed: 0xffff,
            charge: 0.,
            charge_interval: 20.,
            charging: false,
            charge_effect: None,
            draw_released: false,
            level: 0,
            hit: false,
            radiance_delay: None,
            radiance_yaw: 0,
            radiance_contact: None,
            aura_time: 0.,
            flick: Default::default(),
            terrain_dash_pending: false,
            guard_dash: false,
            roll_side: None,
            poise: 0.,
            sustain_poise: false,
        }
    }
    fn cancel(&mut self) {
        // GU's common action transition releases the previous state's stamina
        // latch. Do not clear it here after a guard/damage state has taken over.
        self.charge_effect = None;
        self.radiance_contact = None;
        self.cursor = Cursor::new(IDLE);
        self.packed = 0xffff;
        self.charging = false;
        self.hit = false;
        self.roll_side = None;
    }
    unsafe fn cancel_attack(&mut self, p: u32) {
        self.cancel();
        unsafe { write(p, 12325, 0u8) };
    }
    unsafe fn finish(&mut self, p: u32) {
        unsafe {
            self.cancel_attack(p);
            native!(0x2b6e5c, p);
        }
    }
    unsafe fn evade(&mut self, p: u32) -> bool {
        unsafe {
            motion::remember_roll(
                p,
                self.roll_side
                    .take()
                    .unwrap_or_else(|| motion::roll_side(p)),
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
            let Some(s) = instruction(self.cursor) else {
                self.finish(p);
                return true;
            };
            match s.op {
                0 | 1 => {
                    self.finish(p);
                    true
                }
                2 => self.begin(p, s.clip as u8),
                3..=5 => {
                    let start = frame.unwrap_or(s.start);
                    let blend = if frame.is_some() { 0 } else { s.blend };
                    let Some(id) = motion::play(p, s.clip, start, blend) else {
                        self.finish(p);
                        return true;
                    };
                    self.packed = id;
                    let start = f32::from(start);
                    self.audio.update(p, Some(s.clip), start, Some(start));
                    self.cursor.time.cycles = 0;
                    self.cursor.time.frame = start;
                    self.cursor.time.blend = f32::from(blend / 2);
                    if s.clip == RADIANCE_CLIP {
                        self.radiance_delay = None;
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
            self.terrain_dash_pending = false;
            self.charge_effect = None;
            self.radiance_contact = None;
            self.cursor = Cursor::new(state);
            self.roll_side = None;
            let Some(s) = instruction(self.cursor) else {
                self.cancel();
                return false;
            };
            if motion::physical(p, s.clip).is_none() {
                self.cancel();
                return false;
            }
            // An accepted GU request has already run the common transition.
            let requested =
                action::current(p) == (action::GROUP, action::slot(state)) && read::<u8>(p, 5) == 0;
            if !requested && !action::enter(p, state) {
                self.cancel();
                return false;
            }
            self.hit = false;
            self.charging = false;
            self.draw_released = false;
            if charge_state(state) {
                self.level = 0;
                self.charge = 0.;
                // GU's own GS charge timer (11681DC) checks these Focus and
                // Slow Charge equipment skills. FZ changes 40/20 to 32/16 or 48/24.
                self.charge_interval = if [152, 266, 284, 293]
                    .iter()
                    .any(|&id| native!(0x2864c4, p, id) != 0)
                {
                    16.
                } else if native!(0x2864c4, p, 153) != 0 {
                    24.
                } else {
                    20.
                };
            }
            self.poise = 0.;
            self.sustain_poise = false;
            write(p, 12325, 0u8);
            write(p, 12320 + 176, 0u32);
            write(p, 5, 1u8);
            native!(0x282fd8, p, 0, u32::from(state != DASH), 0);
            native!(0x2a2560, p, 1);
            if matches!(state, 13 | 20 | 38 | 47) {
                native!(0x2a264c, p, 1); // GU draw-attack bookkeeping/skills
            }
            if state == DASH {
                // Match GU sprint collision flags and use its shared terrain
                // commands for mounting a ledge while the run clip is active.
                native!(0x283238, p, 0, 0x180, 0);
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
                    }
                    12 => spend_stamina(p, 150.),
                    4 => {
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
                        combat::emit(p, param, late);
                    }
                    18 => {
                        combat::emit(p, program::charged_row(self.level), late);
                    }
                    19 => {
                        combat::emit(p, 110 + u16::from(self.level.min(5)), late);
                    }
                    // Frontier event 29 requests the hunter origin (joint argument 0).
                    29 => fx::emit(p, param, None),
                    30 => {
                        self.charging = true;
                        self.charge = late * 0.5;
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
    unsafe fn begin_branch(&mut self, p: u32, branch: u16) -> bool {
        unsafe {
            let next = match branch {
                65 => {
                    if action::transition_to(p, GUARD_GROUP, 0) {
                        self.cancel();
                    }
                    return true;
                }
                74 => {
                    motion::remember_roll(
                        p,
                        self.roll_side.unwrap_or_else(|| motion::roll_side(p)),
                    );
                    if action::transition_to(p, 1, 6) {
                        self.cancel_attack(p);
                    } else {
                        motion::forget_roll(p);
                    }
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
                _ => return true,
            };
            if action::transition_to(p, action::GROUP, action::slot(next)) {
                motion::aim(p, branch);
                self.begin(p, next)
            } else {
                true
            }
        }
    }
    unsafe fn advance(&mut self, p: u32, input: &Input, dash_input: bool) -> bool {
        unsafe {
            let state = self.cursor.state;
            let uppercut_charge = matches!(state, 47..=53 | 70);
            let Some(s) = instruction(self.cursor) else {
                return false;
            };
            let frame = read::<f32>(p, 1280);
            let end = read::<f32>(p, 1284);
            let previous = self.cursor.time.frame;
            let wrapped = frame < previous;
            let ended = read::<u8>(p, 1206) & 4 != 0;
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
            if wrapped {
                self.cursor.time.cycles = self.cursor.time.cycles.saturating_add(1)
            }
            let delta = if wrapped {
                read::<f32>(p, 28).max(0.)
            } else {
                (frame - previous).max(0.)
            };
            write(p, 9472, read::<u32>(p, 9472) & !1);
            self.cursor.time.frame = frame;
            self.cursor.time.blend = read(p, 1268);
            if previous < 50. && frame >= 50. && s.clip == 1449 {
                fx::emit(p, 3, None);
                if read::<u8>(p, 11988) == 0 {
                    fx::emit(p, 5, None);
                }
            }
            if previous < 70. && frame >= 70. && s.clip == 1450 && read::<u8>(p, 11988) == 0 {
                fx::emit(p, 6, None);
            }
            // 169D44 records the actual target and its type in this sword's
            // volume. Type 1 is a monster, type 0 is a hunter. The allocator
            // clears the record on each swing. No shared DPS hook is needed.
            self.hit |= read::<u32>(p, 12320 + 176) != 0 && read::<u8>(p, 12320 + 180) == 1;
            self.poise = (self.poise - delta).max(0.);
            let was_charging = self.charging;
            self.events(p);
            if state < 74
                && self.cursor.hunter_art_window(self.hit)
                && action::transition(p, action::Transition::HUNTER_ART)
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
                && state != 3
                && state < 74
                && input.auxiliary_pressed != 0
                && read::<f32>(p, 5088) >= 150.
                && read::<f32>(p, 10296) <= 0.;
            if brace {
                spend_stamina(p, 150.);
                crate::sound::play(p, false, 1);
                fx::emit(p, 4, Some(0));
                let duration = if charge_state(state) {
                    240.
                } else {
                    self.poise
                };
                write(p, 10296, duration);
            }
            if self.charging && delta > 0. {
                // FZ increments +2852 once per simulation tick (two MOT frames).
                if was_charging {
                    self.charge += delta * 0.5;
                }
                let cap = if uppercut_charge { 4 } else { 3 };
                let old = self.level;
                self.level = ((self.charge / self.charge_interval) as u8)
                    .saturating_sub(1)
                    .min(cap);
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
                    return self.begin_branch(p, branch);
                }
            }
            if state == DASH {
                let has_direction = native!(0x27e608, p, 0) > 0;
                if has_direction {
                    let yaw = native!(0x27f678, p) & 0xffff;
                    write(p, 4076, yaw);
                    // GU's sprint probe feeds the drawn roll-off action used
                    // by the native roll's terrain handler (2A9098).
                    if native!(0x2a8458, p) == 1 {
                        self.cancel();
                        self.terrain_dash_pending = true;
                        write(p, 5280, yaw);
                        let (group, slot) = ROLL_OFF_LEDGE;
                        native!(0x2831e0, p, u32::from(group), u32::from(slot), 0xc);
                        return true;
                    }
                }
                if delta > 0. {
                    // The native continuous drain already multiplies by GU's
                    // frame delta. Disable recovery and supply the rate once.
                    native!(0x2a25a8, p, 1);
                    drain_stamina(p, -1.);
                }
                if frame >= 30. || self.cursor.time.cycles > 0 {
                    if input.command(13) {
                        self.cancel();
                        native!(0x2ab670, p, 1, 2, 0); // shared sheathe and equipment change
                        return true;
                    }
                    if input.command(3) {
                        return self.evade(p);
                    }
                    if input.command(70) {
                        self.cancel();
                        native!(0x2ab670, p, u32::from(GUARD_GROUP), 0, 0);
                        return true;
                    }
                    if input.weapon_command(1) {
                        return self.begin(p, 70);
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
                let side = if (input.weapon_command(5) || input.weapon_command(22))
                    && native!(0x27e608, p, 0) > 0
                {
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
                } else if input.weapon_command(1) {
                    cmd |= 1 << 15;
                    if input.command(0) {
                        cmd |= 1 << 28 | 1 << (28 + side);
                    }
                    if side == 2 {
                        cmd |= 1 << 16;
                    }
                } else if input.weapon_command(5) || input.weapon_command(12) {
                    cmd |= 1 << 10 | 1 << (10 + side);
                    if input.weapon_command(12) {
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
                if !input.weapon_command(5) {
                    cmd |= 1 << 6
                }
                if !(input.weapon_command(5) && input.weapon_command(22)) {
                    cmd |= 1 << 17
                }
                if input.command(70) {
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
                if input.command(3) && self.roll_side.is_none() && self.cursor.buffered_roll() {
                    self.roll_side = Some(motion::roll_side(p));
                }
                if let Some(branch) = branch {
                    return self.begin_branch(p, branch);
                }
            }
            let cycles = self
                .cursor
                .time
                .cycles
                .saturating_add(u16::from(ended && !wrapped));
            let next = match s.op {
                3 => ended || (end > 0. && frame >= end),
                4 => cycles >= s.loops && (frame >= f32::from(s.until) || ended),
                _ => false,
            };
            if next {
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
                        .get_or_insert_with(|| ChargeEffect::start(p))
                        .update(p, self.charge, self.level);
                } else {
                    self.charge_effect = None;
                }
            }
            true
        }
    }
    unsafe fn radiance(&mut self, p: u32, previous: f32, frame: f32) {
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
            // FZ uses scene node 0, the unanimated parent of the motion
            // carrier. Its equivalent here is the GU actor origin.
            let [x, y, z] = read::<[f32; 3]>(p, 64);
            let (sin, cos) = mhgu_sdk::geometry::Angle::from_units(yaw).sin_cos();
            let origin = [x + 200. * sin, y, z + 200. * cos];
            self.radiance_contact =
                combat::emit(p, program::RADIANCE, late).map(|shapes| (origin, shapes));
            fx::radiance(p, origin, self.radiance_yaw);
        }
    }
    unsafe fn tick(&mut self, p: u32, input: &Input) -> bool {
        unsafe {
            let current = action::current(p);
            let (group, act) = current;
            let climbing = group == 1 && LEDGE_CLIMBS.contains(&act);
            let jump_slash =
                group == action::GROUP && matches!(act, SHEATHED_JUMP_SLASH | DRAWN_JUMP_SLASH);
            // With the stick held, GU's drawn climb exits directly to walk 2:30.
            let ground_movement = matches!(current, (1, 0 | 46) | (2, 0 | 30));
            if current != (1, 6) {
                motion::forget_roll(p);
            }
            let phase: u8 = read(p, 5);
            let physical: u16 = read(p, 1204);
            if self.guard == 3 && native!(0x2a2444, p) != 0 {
                self.aura_time += read::<f32>(p, 28).max(0.);
                if self.aura_time >= 24. {
                    self.aura_time %= 24.;
                    fx::radiance_ready(p);
                }
            } else {
                self.aura_time = 0.;
            }
            self.audio
                .update(p, motion::source_clip(p, physical), read(p, 1280), None);
            let dash_input = self.flick.sample(
                input.stick_strength,
                input.stick_angle,
                read::<f32>(p, 28).max(0.) * 0.5,
            );
            // GU can land a short roll-off directly in 1:138.
            let buffered_roll_dash = dash_input
                && matches!(current, LEDGE_LANDING | (1, 138))
                && (read(p, 9480), read(p, 9484)) == ROLL_OFF_LEDGE
                && native!(0x2a2444, p) != 0;
            let keeps_terrain_dash = climbing
                || jump_slash
                || ground_movement
                || matches!(current, ROLL_OFF_LEDGE | LEDGE_LANDING);
            self.terrain_dash_pending = buffered_roll_dash
                || (climbing && self.cursor.state == DASH)
                || (self.terrain_dash_pending && keeps_terrain_dash);
            if self.cursor.state != IDLE {
                if current == (action::GROUP, action::slot(self.cursor.state))
                    && physical == self.packed
                    && phase != 0
                    && read::<u8>(p, 9493) & 4 == 0
                {
                    return self.advance(p, input, dash_input);
                }
                self.cancel(); // damage, sheathing, warp or another GU action took ownership
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
                if jump_slash
                    && phase == 2
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
                            guard::Contact::Monster => 66,
                            guard::Contact::Other => 72,
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
            let terrain_dash_ready =
                self.terrain_dash_pending && ground_movement && native!(0x2a2444, p) != 0;
            let accepts_ground_input = terrain_dash_ready
                || (phase != 0
                    && matches!(group, 1 | 2)
                    && matches!(native!(0x53a058, p), 5001 | 5004));
            if !accepts_ground_input {
                return false;
            }
            let resume_dash = core::mem::take(&mut self.terrain_dash_pending);
            if action::transition(p, action::Transition::HUNTER_ART) {
                return true;
            }
            if input.command(13) {
                native!(0x2ab670, p, 1, 2, 0); // shared sheathe, motion 5003
                return true;
            }
            if self.guard >= 3 && input.command(8) {
                return self.begin(p, 59);
            }
            if input.command(70) {
                native!(0x2ab670, p, u32::from(GUARD_GROUP), 0, 0);
                return true;
            }
            if input.weapon_command(1) {
                return self.begin(p, 48);
            }
            if input.weapon_command(12) {
                return self.begin(p, 19);
            }
            if input.weapon_command(2) {
                return self.begin(p, SWEEP);
            }
            let wants_dash =
                dash_input || (resume_dash && !input.command(3) && native!(0x27e608, p, 0) > 0);
            wants_dash && read::<f32>(p, 5088) > 0. && self.begin(p, DASH)
        }
    }
}

// The game can re-enter its action dispatcher while changing state. Only the
// outer call borrows the controller; contact callbacks publish through atomics.
struct Controller(AtomicBool, UnsafeCell<Option<Battle>>);
unsafe impl Sync for Controller {}
static CONTROL: Controller = Controller(AtomicBool::new(false), UnsafeCell::new(None));

pub(super) fn radiance_origin(owner: u32, shapes: u32) -> Option<[f32; 3]> {
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
            slot.get_or_insert_with(|| Battle::fresh(p, eq))
                .tick(p, input)
        } else {
            if let Some(mut battle) = slot.take()
                && p != 0
                && battle.owner == p
            {
                let (group, slot) = action::current(p);
                if action::custom(group, slot) {
                    battle.finish(p);
                }
            }
            motion::forget_roll(p);
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
        if frame >= 32. || (frame >= 28. && !extended_guard(p, frame)) {
            return guard::Decision::Reject;
        }
        // FZ 81744DA0 grants four simulation ticks before the follow-up
        // animation starts. Light recoil suppresses chip damage.
        result.write(guard::GuardResult {
            invulnerability: 8.,
            recoil: 0,
            sharpness_power: 40,
        });
        guard::Decision::Precise
    }
}
unsafe fn extended_guard(p: u32, frame: f32) -> bool {
    unsafe {
        [168, 279, 297]
            .iter()
            .any(|&id| native!(0x2864c4, p, id) != 0)
            || (frame < 30. && native!(0x2864c4, p, 169) != 0)
    }
}
