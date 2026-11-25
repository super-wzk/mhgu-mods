mod arts;
mod landing;
mod peer;
pub(super) use peer::PLAYBACK;

use crate::{
    STYLE_ID, fx, motion, native,
    program::{self, AXE_BRAKE, AXE_SPRINT, Gauges, SWORD_BRAKE, SWORD_SPRINT, SprintForm},
    read, sound, write,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering::*},
};
use mhfz_sdk::{
    action::Cursor,
    combat::{HitWindow, evade_bonus_frames},
    effects::MotionClock,
    sound::Clock,
    sprint::{Flick, Resume},
};
use mhgu_sdk::{
    combat::collision::{CachedHitShape, Contact},
    style::{
        self, ActionRequest, Input,
        action::{self, Attack, MotionFrame},
        guard, input as pad, movement,
    },
};

static BLADES: [CachedHitShape; 7] = [
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_0"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_1"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_2"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_3"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_4"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_5"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_106_6"),
];
static BURSTS: [CachedHitShape; 4] = [
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_107_0"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_107_1"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_107_2"),
    CachedHitShape::new(cr"player\hit\frontier_sa\hit_107_3"),
];

pub(super) unsafe fn preload_hit_shapes() {
    for shape in BLADES.iter().chain(&BURSTS) {
        unsafe { shape.preload() };
    }
}
const DIRECTIONAL_SLASHES: u32 = (1 << 17) | (1 << 18) | (1 << 19);
const EVADE_STAMINA_COST: f32 = 150.;
// FZ 8121C8D6 spends param 31 in half the units GU uses for stamina.
const AXE_SWING_STAMINA_COST: f32 = (2 * program::param(31)) as f32;
const SPRINT_COMMAND_TABLE: u32 = 2000;
const FORWARD_ROLL: (u16, u16) = (1, 6);
const SPRINT_STEER_DELAY: f32 = 52.;
const SPRINT_TURN_THRESHOLD: u16 = 910;

fn is_light_unlock_opener(state: u8) -> bool {
    matches!(state, 93 | 94 | 146)
}

#[derive(Clone, Copy)]
struct Burst {
    actor: u32,
    shape: u32,
    center: [f32; 3],
    radius: f32,
}
static EXPLOSIONS: mhfz_sdk::sync::Shared<[Option<Burst>; 5]> =
    mhfz_sdk::sync::Shared::new([None; 5]);
unsafe extern "C" fn expand(actor: u32, hit: u32) -> u32 {
    unsafe {
        if hit != actor + action::melee::BLADE_CONTACT_OFFSETS[1] {
            return 0;
        }
        let shape = read::<u32>(hit, 132);
        if let Some(burst) = EXPLOSIONS.with(|slots| {
            slots
                .iter()
                .flatten()
                .find(|e| e.actor == actor && e.shape == shape)
                .copied()
        }) && let Some(contact) = Contact::from_raw(hit)
        {
            return u32::from(contact.submit_world_sphere(burst.center, burst.radius));
        }
        0
    }
}

unsafe fn strike(actor: u32, burst: bool, id: u16, frame: f32, late: f32, in_area: bool) {
    let Some(row) = program::strike(burst, id) else {
        return;
    };
    let Some(window) = HitWindow::new(row.start, row.duration, frame, late) else {
        return;
    };
    let shapes = if burst {
        BURSTS.as_slice()
    } else {
        BLADES.as_slice()
    };
    let Some(shape) = shapes.get(row.shape) else {
        return;
    };
    unsafe {
        let Some(shape) = shape.load() else {
            return;
        };
        if burst && let Some((offset, radius)) = program::burst_geometry(id) {
            let index = peer::index_of(actor).map_or(0, |i| i + 1);
            let (center, _) = style::particle::world_offset(actor, offset);
            if in_area && matches!(id, 1 | 2) {
                fx::phial_burst(actor, id, center);
            }
            EXPLOSIONS.with(|slots| {
                slots[index] = Some(Burst {
                    actor,
                    shape: shape.as_raw(),
                    center,
                    radius,
                })
            });
        }
        let sword_pose = read::<u32>(actor, motion::SWORD_POSE) != 0;
        let mut flags = Attack::PUNISHING_DRAW;
        if burst {
            flags = Attack::SECONDARY | Attack::NO_ATTACKER_HITSTOP;
        } else if row.shape == 0 || sword_pose {
            flags |= Attack::DEFLECTABLE;
        }
        // GU 2B76CC applies the equipped sword phial and Demon Riot through bit 3.
        let phial = if sword_pose { 8 } else { 0 };
        let attack = Attack {
            template: 1,
            power: row.power,
            delay: window.delay,
            duration: window.duration,
            flags: 7 | phial | Attack::NO_MOUNT | flags,
            shape: shape.as_raw(),
            no_sharpness_cost: u32::from(burst),
        };
        let submitted =
            attack.submit_with_hitstop(actor, mhfz_sdk::combat::attacker_hitstop(row.source_hitstop));
        if submitted && !burst {
            let kind = u32::from(!sword_pose) | (u32::from(program::counts_for_discharge(id)) << 1);
            if OWNER.load(Relaxed) == actor {
                BLADE_KIND.store(kind, Relaxed);
            } else if let Some(index) = peer::index_of(actor) {
                peer::BLADE_KIND[index].store(kind, Relaxed);
            }
        }
    }
}

unsafe fn take_recoil(actor: u32, state: u8) -> Option<u8> {
    unsafe {
        // Consume the contact even when the current action is exempt.
        if !action::melee::take_deflection(actor) || program::ignores_deflection(state) {
            return None;
        }
        if read::<u32>(actor, motion::SWORD_POSE) == 0 {
            return Some(program::AXE_RECOIL);
        }
        (style::hunter::sharpness_grade(actor) < program::LIGHT_SWORD_RECOIL_CUTOFF)
            .then_some(program::LIGHT_SWORD_RECOIL)
    }
}

static OWNER: AtomicU32 = AtomicU32::new(0);
// Bit 0 credits the axe gauges. Bit 1 identifies the 106 rows counted by 81164044.
static BLADE_KIND: AtomicU32 = AtomicU32::new(0);
static DISCHARGE_HITS: AtomicU32 = AtomicU32::new(0);
static HITS: AtomicU32 = AtomicU32::new(0);
static HUD: AtomicU32 = AtomicU32::new(0);
struct Battle {
    equipment: u32,
    peer: bool,
    peer_light: Option<bool>,
    alternate_swing: bool,
    gauges: Gauges,
    cursor: Option<Cursor>,
    previous_action: Option<u8>,
    guard_slash: u32,
    branch_aim: Option<(u16, u16)>,
    x_press_aim: Option<u16>,
    roll_followup: Option<u8>,
    landing_followup: Option<landing::Pending>,
    packed: u16,
    model_motion: u8,
    charge: f32,
    evades: u8,
    poise: bool,
    flick: Flick,
    resume_sprint: Resume,
    sprint_elapsed: f32,
    sprint_form: SprintForm,
    audio: Clock,
    particles: MotionClock,
    visual: fx::WeaponFx,
    slot: u8,
}
impl Battle {
    fn new(equipment: u32) -> Self {
        Self {
            equipment,
            peer: false,
            peer_light: None,
            alternate_swing: false,
            gauges: Gauges {
                light: program::param(20),
                ..Default::default()
            },
            cursor: None,
            previous_action: None,
            guard_slash: 0,
            branch_aim: None,
            x_press_aim: None,
            roll_followup: None,
            landing_followup: None,
            packed: 0,
            model_motion: program::AXE_IDLE_MOTION,
            charge: 0.,
            evades: 0,
            poise: false,
            flick: Flick::default(),
            resume_sprint: Resume::default(),
            sprint_elapsed: 0.,
            sprint_form: SprintForm::Axe,
            audio: Clock::default(),
            particles: MotionClock::default(),
            visual: fx::WeaponFx::default(),
            slot: 0,
        }
    }

    fn visual_gauges(&self) -> Gauges {
        let mut gauges = self.gauges;
        if let Some(lit) = self.peer_light {
            gauges.unlocked = lit;
            gauges.light = u16::from(lit);
        }
        gauges
    }

    unsafe fn in_visual_area(&self, actor: u32) -> bool {
        !self.peer
            || unsafe {
                style::hunter::local()
                    .is_some_and(|local| read::<u8>(local, 4180) == read::<u8>(actor, 4180))
            }
    }

    unsafe fn clear_visuals(&mut self) {
        unsafe { fx::clear(self.slot) };
        self.visual = fx::WeaponFx::default();
    }

    fn publish_gauges(&self) {
        self.publish_phial();
        HUD.store(
            u32::from(self.gauges.light) | (u32::from(self.gauges.unlocked) << 16),
            Relaxed,
        );
    }

    unsafe fn begin(&mut self, actor: u32, id: u8) -> bool {
        unsafe { self.begin_facing(actor, id, None) }
    }

    unsafe fn begin_facing(&mut self, actor: u32, id: u8, facing: Option<u16>) -> bool {
        if !program::ACTIONS.contains(id) {
            return false;
        }
        unsafe {
            // FZ 81214070 calls 817C5926 before entering either sprint.
            // Use the current stick heading when a buffered branch executes.
            let facing = if matches!(id, AXE_SPRINT | SWORD_SPRINT) {
                let yaw = movement::live_relative_stick_yaw(actor).unwrap_or(0);
                Some(read::<u16>(actor, 4076).wrapping_sub(yaw))
            } else {
                facing
            };
            let (group, slot) = action::current(actor);
            let previous = action::custom_id(group, slot);
            // Only the draw charge and its release are one draw attack.
            let accepted = if matches!(previous, Some(111..=113))
                && previous != Some(id)
                && matches!(id, 112 | 113 | 116 | 121 | 122 | 125)
                && native!(0x2a263c, actor) != 0
            {
                action::enter_draw_attack(actor, id)
            } else {
                action::accept_custom_start(actor, id)
            };
            if !accepted {
                return false;
            }
            if let Some(yaw) = facing {
                movement::set_facing(actor, yaw);
            }
            self.start(actor, id);
        }
        true
    }

    unsafe fn start(&mut self, actor: u32, id: u8) {
        unsafe {
            let previous = self.cursor.as_ref().map(|c| program::source_state(c.state));
            self.previous_action = previous;
            self.guard_slash = 0;
            self.branch_aim = None;
            self.x_press_aim = None;
            self.roll_followup = None;
            // The sender can only enter the light-sword unlock with a full meter.
            if self.peer && is_light_unlock_opener(id) {
                self.gauges.light = program::param(18);
            }
            if matches!(id, 111 | 139 | 162) {
                self.charge = 0.;
            }
            if !matches!(id, 36 | 37 | 119 | 120) {
                self.evades = 0;
            } else {
                self.evades = self.evades.saturating_add(1);
            }
            if self.peer {
                peer::BLADE_KIND[self.slot as usize - 1].store(0, Relaxed);
            } else {
                BLADE_KIND.store(0, Relaxed);
            }
            if matches!(id, 70 | 77 | 98) {
                self.gauges.discharge_hits = 0;
                if !self.peer {
                    DISCHARGE_HITS.store(0, Relaxed);
                }
            }
            if matches!(id, 116 | 122 | 155 | 157) {
                self.alternate_swing = false;
            }
            self.resume_sprint.set(false);
            let source = program::source_state(id);
            self.sprint_elapsed = if id == source {
                0.
            } else {
                SPRINT_STEER_DELAY + 2.
            };
            self.poise = false;
            write(actor, 5, 1u8);
            let sprint = SprintForm::from_action(id);
            native!(0x282fd8, actor, 0, u32::from(sprint.is_none()), 0);
            if let Some(form) = sprint {
                self.sprint_form = form;
                self.gauges.sword = form == SprintForm::Sword;
                native!(0x283238, actor, 0, 0x1180, 0);
                native!(0x2e9b7c, actor, SPRINT_COMMAND_TABLE, 0);
                native!(0x2a25a8, actor, 1);
            } else if matches!(id, 36 | 37 | 119 | 120) {
                native!(0x283238, actor, 0, 0x1000, 0);
            }
            if native!(0x2a2444, actor) == 0 {
                native!(0x2a264c, actor, 1);
            } else {
                native!(0x2a2560, actor, 1);
            }
            self.cursor = Some(program::cursor(id));
            self.play(actor);
        }
    }

    unsafe fn finish(&mut self, actor: u32) {
        self.cursor = None;
        self.poise = false;
        if !self.peer {
            unsafe {
                native!(0x2b6e5c, actor);
            }
        }
    }

    unsafe fn play(&mut self, actor: u32) {
        let in_area = unsafe { self.in_visual_area(actor) };
        let Some(c) = &mut self.cursor else { return };
        let Some(step) = program::step(c.state, c.time.step) else {
            unsafe { self.finish(actor) };
            return;
        };
        unsafe {
            let Some(packed) = motion::play(actor, step.clip, step.start, step.blend) else {
                self.finish(actor);
                return;
            };
            self.packed = packed;
            if program::is_fade_slash_charge(c.state) {
                // FZ 812132C4 calls 8181848C for the four fade-slash charges.
                let rate = match action::ChargeSkill::equipped(actor) {
                    action::ChargeSkill::Focus => 1.25,
                    action::ChargeSkill::Slow => 5. / 6.,
                    action::ChargeSkill::Normal => 1.,
                };
                action::set_motion_rate(actor, rate);
            }
            if in_area {
                self.visual
                    .enter(actor, self.slot, c.state, step.clip, f32::from(step.start));
                if c.state == 86 && c.time.step == 0 {
                    fx::discharge_flash(actor, self.slot);
                }
            }
            c.time.cycles = 0;
            c.time.frame = f32::from(step.start);
            c.time.blend = f32::from(step.blend) / 2.;
            let (model_motion, sword) = program::equipment_motion(step.clip, self.gauges.sword);
            self.model_motion = model_motion;
            // FZ 812113BE restores the axe if an unlock opener ends before state
            // 110. Its animated sword pose can still unfold during the opener.
            // The axe sprint also unfolds temporarily and retains its starting form.
            if !program::is_sprint(c.state) {
                self.gauges.sword = sword && !is_light_unlock_opener(c.state);
            }
            let unlock = c.state == 110
                && (self.previous_action.is_some_and(is_light_unlock_opener)
                    || self.peer && self.previous_action.is_none());
            if unlock {
                if self.peer && self.previous_action.is_none() {
                    self.gauges.light = program::param(18);
                }
                self.gauges.sword = true;
                self.gauges.unlocked = true;
            }
            write(
                actor,
                motion::SWORD_POSE,
                u32::from(program::sword_pose(self.model_motion, c.time.frame)),
            );
            self.particles.seek(step.clip, c.time.frame);
            sound::update(
                &mut self.audio,
                actor,
                Some(step.clip),
                c.time.frame,
                Some(c.time.frame),
            );
            if !in_area {
                self.clear_visuals();
            }
            self.events(actor, in_area);
            if unlock && !self.peer {
                self.publish_gauges();
            } else {
                self.publish_phial();
            }
        }
    }

    unsafe fn events(&mut self, actor: u32, in_area: bool) {
        let Some(c) = &mut self.cursor else { return };
        // A resumed sprint starts beyond the opener. Retain poise and charge
        // the running cost from the first wrap, with no catch-up cost or camera cue.
        let resume_entry = c.state != program::source_state(c.state) && c.time.cycles == 0;
        while let Some((kind, value, late)) = c.next_event() {
            let duration = (2. * f32::from(value) - late).max(0.);
            unsafe {
                if self.peer && !matches!(kind, 1 | 2 | 4 | 5 | 15..=17 | 20) {
                    continue;
                }
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
                    3 => {
                        let active = match value {
                            0 => true,
                            3 => false,
                            4 => mhfz_sdk::combat::guard_bonus_frames(actor) >= 2,
                            5 => mhfz_sdk::combat::guard_bonus_frames(actor) >= 4,
                            _ => continue,
                        };
                        if active {
                            native!(0x283238, actor, 0, 16, 0);
                        } else {
                            native!(0x28321c, actor, 0, 16, 0);
                        }
                    }
                    4 | 5 => strike(actor, kind == 5, value, c.time.frame, late, in_area),
                    6 => write(actor, 10288, duration),
                    7 => self.poise = true,
                    8 | 10 => write(actor, 11072, duration),
                    9 => write(actor, 11072, duration + evade_bonus_frames(actor)),
                    12 | 14 => movement::stamina(actor, -EVADE_STAMINA_COST, false),
                    13 => movement::stamina(actor, -AXE_SWING_STAMINA_COST, false),
                    15 | 17 if !resume_entry => {
                        let slash = program::uses_slash_gauge(c.state);
                        if !slash && arts::riot_active(actor) {
                            continue;
                        }
                        let cost = if kind == 17 {
                            value
                        } else {
                            program::param(23 + usize::from(value.min(3)))
                        };
                        if !slash {
                            self.gauges.light = self.gauges.light.saturating_sub(cost);
                        } else if !self.peer {
                            arts::change_slash(actor, -i32::from(cost));
                        }
                    }
                    16 if value == 4 && matches!(c.state, 90 | 92) => {
                        self.gauges.gain_light(scaled_gain(actor, 45))
                    }
                    18 if !resume_entry => {
                        let preset = program::param(72 + usize::from(value.min(2)));
                        mhfz_sdk::camera::start(actor, preset);
                    }
                    20 => self.alternate_swing = !self.alternate_swing,
                    _ => {}
                }
            }
        }
    }

    unsafe fn destination(&self, actor: u32, id: u16, yaw: i16) -> Option<u8> {
        Some(match id {
            201 => return None,
            202 => {
                if yaw >= 0 {
                    36
                } else {
                    37
                }
            }
            204 => {
                if yaw >= 0 {
                    119
                } else {
                    120
                }
            }
            205 if self.gauges.sword => program::SWORD_TO_AXE,
            205 if self.gauges.unlocked => 30,
            205 if self.gauges.full() => 94,
            205 => return None,
            210 => self.gauges.discharge_tier().release_state(),
            211..=214 => program::charge_release(
                id,
                self.gauges.sword,
                unsafe { self.charge_level(actor) } == 2,
            ),
            // 206..209 require the Earth mask; 215 explicitly requires Heaven.
            206..=209 | 215 => return None,
            _ => u8::try_from(id)
                .ok()
                .filter(|&id| program::ACTIONS.contains(id))?,
        })
    }

    unsafe fn accept_branch(&mut self, actor: u32, id: u16, yaw: i16) -> bool {
        unsafe {
            let forward_evade = matches!(id, 202 | 204) && program::stick_degrees(yaw).abs() < 45;
            if id == 203 || forward_evade {
                let accepted = action::transition_to(actor, 1, 6);
                if accepted {
                    self.cursor = None;
                }
                return accepted;
            }
            let Some(next) = self.destination(actor, id, yaw) else {
                return false;
            };
            let turn = program::turn_delta(id, self.previous_action, yaw);
            // The guard cone follows body facing even when the turn is zero.
            let facing = (matches!(id, 89 | 91) || turn != 0)
                .then(|| read::<u16>(actor, 4076).wrapping_add_signed(turn));
            self.begin_facing(actor, next, facing)
        }
    }

    unsafe fn charge_level(&self, actor: u32) -> u8 {
        let scale = match unsafe { action::ChargeSkill::equipped(actor) } {
            action::ChargeSkill::Focus => 80,
            action::ChargeSkill::Slow => 120,
            _ => 100,
        };
        let frames = |param| 2. * f32::from(program::param(param) * scale / 100);
        let ready = frames(37);
        let start = 2. * ((ready * 0.5 * 0.066667) as u16 as f32);
        if self.charge >= frames(36) {
            3
        } else if self.charge >= ready {
            2
        } else if self.charge >= start {
            1
        } else {
            0
        }
    }

    unsafe fn sample(&mut self, actor: u32, clip: u16) -> (MotionFrame, bool) {
        unsafe {
            let in_area = self.in_visual_area(actor);
            let c = self.cursor.as_mut().unwrap();
            let sample = MotionFrame::sample(actor);
            let previous = c.time.frame;
            let state = c.state;
            let previous_phase = self.alternate_swing;
            let wrapped = c.time.observe(sample.frame, sample.blend);
            write(
                actor,
                motion::SWORD_POSE,
                u32::from(program::sword_pose(self.model_motion, sample.frame)),
            );
            sound::update(&mut self.audio, actor, Some(clip), sample.frame, None);
            if in_area {
                fx::update(&mut self.particles, self.slot, actor, clip, sample);
            } else {
                self.particles.seek(clip, sample.frame);
            }
            // Apply a wrap's authored phase toggle before its restarted head.
            self.events(actor, in_area);
            self.publish_phial();
            let dt = read::<f32>(actor, 28).max(0.);
            if !in_area {
                self.clear_visuals();
                return (sample, wrapped);
            }
            let level = if program::charge_clip(clip) {
                self.charge_level(actor)
            } else {
                0
            };
            let visual_gauges = self.visual_gauges();
            self.visual.update(
                self.slot,
                actor,
                clip,
                state,
                previous,
                sample,
                dt,
                visual_gauges,
                level,
                true,
            );
            fx::charged_swing(
                actor,
                clip,
                previous,
                sample,
                previous_phase,
                self.alternate_swing,
            );
            (sample, wrapped)
        }
    }

    unsafe fn advance(
        &mut self,
        actor: u32,
        mut bits: u32,
        direction: Option<i16>,
        dt: f32,
    ) -> bool {
        unsafe {
            let yaw = direction.unwrap_or(0);
            let c = self.cursor.as_mut().unwrap();
            let state = c.state;
            let step = program::step(state, c.time.step).unwrap();
            if matches!(state, 112 | 113 | 163 | 164) {
                self.charge = (self.charge + dt).min(2000.);
                movement::stamina(actor, -1., true);
            }
            let (sample, wrapped) = self.sample(actor, step.clip);
            // FZ 812132C4 advances the motion program before 81815F30 reads
            // commands. An X+A press on the end tick sees the new clip's window.
            let c = self.cursor.as_mut().unwrap();
            let complete = program::step_complete(step, c.time, sample, wrapped);
            let advance_before_input = complete && sample.ended;
            if advance_before_input {
                c.time.step += 1;
                self.play(actor);
            }
            let Some(c) = &self.cursor else { return true };
            let (frame, blend) = (c.time.frame, c.time.blend);
            // Monster absorption opens its X+A rows at motion frame 10.
            if matches!(state, 90 | 92) && frame < 10. {
                let slash = bits & DIRECTIONAL_SLASHES;
                if slash != 0 {
                    self.guard_slash = slash;
                }
            } else {
                if bits & DIRECTIONAL_SLASHES == 0 {
                    bits |= self.guard_slash;
                }
                self.guard_slash = 0;
            }
            if matches!(state, 36 | 37 | 119 | 120) && frame > 0. {
                let offset = if matches!(state, 36 | 119) {
                    0xc000
                } else {
                    0x4000
                };
                if movement::step_off_ledge(actor, offset) {
                    self.cursor = None;
                    return true;
                }
            }
            if self.poise {
                write(actor, 10288, read::<f32>(actor, 10288).max(4.));
            }
            if let Some(recoil) = take_recoil(actor, state)
                && self.begin(actor, recoil)
            {
                action::melee::clear_hitstop(actor);
                return true;
            }
            let c = self.cursor.as_mut().unwrap();
            let stamina = read::<f32>(actor, 5088) > 0.;
            let slash = arts::slash(actor);
            let energy = if program::uses_slash_gauge(state) {
                slash
            } else {
                self.gauges.energy(slash)
            };
            let eligible = |id| self.gauges.eligible(id, energy, stamina, self.evades);
            if c.art_window(eligible) && action::activate_hunter_art(actor) {
                self.cursor = None;
                return true;
            }
            let follows_fade_charge = self
                .previous_action
                .is_some_and(program::is_fade_slash_charge);
            let stick_aim = direction.map(|yaw| read::<u16>(actor, 4076).wrapping_sub(yaw as u16));
            let mut x_aim = None;
            if follows_fade_charge {
                // GU 27CA28 can wait for a chord before 11A03A8 reports X.
                if read::<u16>(actor, 5112) & 0x20 != 0 {
                    self.x_press_aim = stick_aim;
                }
                if bits & (1 << 2) != 0 {
                    x_aim = self.x_press_aim.take();
                } else if !pad::attack_combo_pending(actor) {
                    self.x_press_aim = None;
                }
            }
            let pending_before = c.pending_action();
            let next = c.branch(bits, eligible);
            let pending_turn = c.pending_action().filter(|&id| {
                !program::is_fade_slash_charge(state) && program::accepts_side_turn(id)
            });
            self.branch_aim = pending_turn.and_then(|id| {
                // Only these X follow-ups can use the preceding filtered press.
                let press_aim = x_aim.filter(|_| matches!(id, 29 | 41 | 138 | 142));
                press_aim
                    .or(stick_aim)
                    .filter(|_| !follows_fade_charge || pending_before != Some(id))
                    .map(|aim| (id, aim))
                    .or(self.branch_aim.filter(|&(target, _)| target == id))
            });
            let branch_yaw = direction.unwrap_or_else(|| {
                self.branch_aim.map_or(0, |(_, aim)| {
                    read::<u16>(actor, 4076).wrapping_sub(aim) as i16
                })
            });
            if let Some(id) = next
                && self.accept_branch(actor, id, branch_yaw)
            {
                return true;
            }
            if let Some(form) = SprintForm::from_action(state) {
                let previous_tick = (self.sprint_elapsed * 0.5) as u32;
                self.sprint_elapsed += dt;
                // 812132C4 waits through tick 25 before enabling braking and steering.
                if self.sprint_elapsed > SPRINT_STEER_DELAY {
                    if direction.is_none() {
                        return self.begin(actor, form.brake());
                    }
                    let tick = (self.sprint_elapsed * 0.5) as u32;
                    let mut facing = read::<u16>(actor, 4076);
                    let target = facing.wrapping_sub(yaw as u16);
                    // 81817490 tests the unsigned gap before choosing the turn sign.
                    // Evaluate once per FZ tick so the small negative turn overshoots
                    // by the same amount at either GU update rate.
                    for _ in previous_tick.max((SPRINT_STEER_DELAY * 0.5) as u32)..tick {
                        let gap = target.wrapping_sub(facing);
                        if gap < SPRINT_TURN_THRESHOLD {
                            break;
                        }
                        let step = program::param(38);
                        facing = if gap < 0x8000 {
                            facing.wrapping_add(step)
                        } else {
                            facing.wrapping_sub(step)
                        };
                    }
                    movement::set_facing(actor, facing);
                }
            }
            // FZ 812136B2 / 81213772 face the stick before entering the
            // moving charge, after the ordinary branches have run.
            if matches!(state, 111 | 139 | 162)
                && direction.is_some()
                && blend <= 0.
                && frame >= 40.
            {
                let facing = read::<u16>(actor, 4076).wrapping_sub(yaw as u16);
                return self.begin_facing(
                    actor,
                    if state == 162 { 164 } else { 113 },
                    Some(facing),
                );
            }
            if matches!(state, 112 | 113 | 163 | 164) {
                let walking = direction.is_some();
                if walking {
                    let facing = read::<u16>(actor, 4076).wrapping_sub(yaw as u16);
                    movement::set_facing(actor, facing);
                }
                let next = match (state, walking) {
                    (112, true) => 113,
                    (113, false) => 112,
                    (163, true) => 164,
                    (164, false) => 163,
                    _ => state,
                };
                if next != state {
                    return self.begin(actor, next);
                }
            }
            if complete && !advance_before_input {
                self.cursor.as_mut().unwrap().time.step += 1;
                self.play(actor);
            }
            true
        }
    }
}

impl Battle {
    unsafe fn native_visual(
        &mut self,
        actor: u32,
        clip: Option<u16>,
        sample: MotionFrame,
        drawn: bool,
        dt: f32,
    ) {
        unsafe {
            let (group, slot) = action::current(actor);
            if clip == Some(motion::SPRINT_SHEATHE) {
                fx::retire_for_sprint_sheathe(self.slot);
                self.visual = fx::WeaponFx::default();
                // The w12 model table assigns motion 5 to clip 1499.
                write(
                    actor,
                    motion::SWORD_POSE,
                    u32::from(program::sword_pose(5, sample.frame)),
                );
                sound::update(&mut self.audio, actor, clip, sample.frame, None);
                fx::update(
                    &mut self.particles,
                    self.slot,
                    actor,
                    motion::SPRINT_SHEATHE,
                    sample,
                );
                return;
            }
            let sheathing =
                matches!((group, slot), (1, 2 | 3)) || matches!(clip, Some(1003 | 1009));
            if sheathing {
                // Cancel the weapon layers before GU draws the sheathe pose.
                self.clear_visuals();
                return;
            }
            if group != action::GROUP {
                // FZ 8121CC50 falls back to the held form on common motions.
                // GU 308B3C reads this pose for weapon tracks on those motions.
                let sword = drawn && self.gauges.sword;
                self.model_motion = program::idle_model_motion(sword);
                write(actor, motion::SWORD_POSE, u32::from(sword));
            }
            if !(matches!(group, 1 | 2 | 4 | 16) || clip.is_some()) {
                self.clear_visuals();
                return;
            }
            let clip = clip.unwrap_or(0);
            self.visual
                .enter(actor, self.slot, u8::MAX, clip, sample.frame);
            let visual_gauges = self.visual_gauges();
            self.visual.update(
                self.slot,
                actor,
                clip,
                u8::MAX,
                read(actor, 1276),
                sample,
                dt,
                visual_gauges,
                0,
                drawn,
            );
        }
    }

    unsafe fn tick(&mut self, actor: u32, input: &Input) -> bool {
        unsafe {
            let dt = read::<f32>(actor, 28).max(0.);
            let current = action::current(actor);
            let drawn = native!(0x2a2444, actor) != 0;
            let old = self.cursor.as_ref().map(|c| c.state);
            let direction = movement::live_relative_stick_yaw(actor).map(|yaw| yaw as i16);
            let degrees = direction.map(program::stick_degrees);
            let left = degrees.is_some_and(|d| (-135..=-45).contains(&d));
            let right = degrees.is_some_and(|d| (45..=135).contains(&d));
            let back = degrees.is_some_and(|d| d.abs() >= 135);
            let slash_dir = if left {
                Some(1)
            } else if right {
                Some(2)
            } else if back {
                Some(0)
            } else {
                None
            };
            let landing_attack = landing::Pending::update(
                &mut self.landing_followup,
                actor,
                current,
                drawn,
                dt,
                landing::Attack::pressed(actor, direction.is_some(), slash_dir),
            );
            // GU w08 11A03A8 can omit the chord when the buttons overlap
            // a held attack or the weapon shoulder. Use GU's filtered pad.
            let chord = input.weapon_command(WEAPON_XA)
                || pad::attack_chord_pressed(actor)
                || pad::attack_chord_with_shoulder(actor);
            let slash_commands = (u32::from(chord && left) << 17)
                | (u32::from(chord && right) << 18)
                | (u32::from(chord && back) << 19);
            if matches!(old, Some(89 | 91))
                && let Some(contact) = guard::take(actor)
            {
                let monster = matches!(contact, guard::Contact::Monster);
                let id = match (old, monster) {
                    (Some(89), true) => 90,
                    (Some(89), false) => 58,
                    (_, true) => 92,
                    _ => 59,
                };
                if self.begin(actor, id) {
                    // GU can report the button on the same tick as the recoil.
                    self.guard_slash = slash_commands;
                    return true;
                }
                return false;
            }
            let hits = HITS.swap(0, Relaxed).min(u32::from(u16::MAX)) as u16;
            if hits > 0 {
                self.gauges
                    .gain_light(scaled_gain(actor, 30).saturating_mul(hits));
            }
            self.gauges.discharge_hits = self
                .gauges
                .discharge_hits
                .saturating_add(DISCHARGE_HITS.swap(0, Relaxed).min(u32::from(u16::MAX)) as u16);
            if !drawn {
                self.gauges.sword = false;
            }
            if self.gauges.light == 0 {
                self.gauges.unlocked = false;
            }
            self.publish_gauges();
            let x = !chord && input.weapon_command(WEAPON_X);
            let a = !chord && input.weapon_command(WEAPON_A);
            let move_stick = direction.is_some();
            let stance = input.command(STANCE);
            let charge = !chord && input.command(CHARGE);
            let dash = self.flick.update(input, dt);
            if let Some(state) = action::custom_id(current.0, current.1) {
                // Honor GU's common sheathe command during sprint and braking.
                if (program::is_sprint(state) || matches!(state, AXE_BRAKE | SWORD_BRAKE))
                    && input.command(SHEATHE)
                    && action::transition_to(
                        actor,
                        motion::SHEATHE_ACTION.0,
                        motion::SHEATHE_ACTION.1,
                    )
                {
                    self.clear_visuals();
                    self.cursor = None;
                    return true;
                }
                if old != Some(state)
                    || read::<u16>(actor, 1204) != self.packed
                    || read::<u8>(actor, 5) == 0
                {
                    return self.begin(actor, state);
                }
                let r = input.command(27);
                // GU 27CA28 reads held X at 0x20. FZ commands 20..22 use it.
                let held_x = read::<u16>(actor, 5108) & 0x20 != 0;
                let evade =
                    input.command(3) && movement::has_evade_stamina(actor, EVADE_STAMINA_COST);
                let forward = degrees.is_some_and(|d| d.abs() <= 45);
                // FZ command IDs from 8121B914. The cursor applies their priority.
                let commands = [
                    (0, true),
                    (1, move_stick),
                    (2, x),
                    (3, a),
                    (4, stance),
                    (5, chord),
                    (6, charge),
                    (7, x && move_stick),
                    (8, x && forward),
                    (9, x && back),
                    (10, a && back),
                    (12, evade),
                    (13, evade && left),
                    (14, evade && right),
                    (15, r && !chord),
                    (16, !r),
                    (20, held_x),
                    (21, !held_x),
                    (22, !held_x && move_stick),
                    (23, dash),
                    (25, a),
                    (26, input.auxiliary_pressed != 0),
                ]
                .into_iter()
                .fold(slash_commands, |mask, (id, active)| {
                    mask | (u32::from(active) << id)
                });
                return self.advance(actor, commands, direction, dt);
            }
            self.cursor = None;
            let sample = MotionFrame::sample(actor);
            let clip = motion::native_clip(actor);
            self.native_visual(actor, clip, sample, drawn, dt);
            if clip != Some(motion::SPRINT_SHEATHE) {
                sound::update(&mut self.audio, actor, clip, sample.frame, None);
                if let Some(clip) = clip {
                    if self.model_motion == program::SWORD_IDLE_MOTION
                        && matches!(clip, 1003 | 1009)
                        && read::<f32>(actor, 1276) < 8.
                        && sample.frame >= 8.
                    {
                        sound::sheathe(actor);
                    }
                    fx::update(&mut self.particles, self.slot, actor, clip, sample);
                }
            }
            let ground = movement::ground_idle(current);
            let sheathed_move = !drawn && movement::sheathed_movement(current);
            self.resume_sprint.on_native(
                actor,
                current,
                ground,
                drawn,
                old.is_some_and(program::is_sprint),
            );
            if drawn && is_forward_roll(actor, current) && read::<u8>(actor, 5) != 0 {
                // FZ 81212D02 / 824F07F0 gives sprint lower priority than
                // attacks, absorption and axe charge. All execute in [50, 60).
                let frame = sample.frame;
                if sample.blend <= 0. && (10. ..60.).contains(&frame) {
                    let sword = self.gauges.sword;
                    if frame >= 30. && self.roll_followup.is_none_or(program::is_sprint) {
                        let followup = match (sword, x, a, chord) {
                            (false, true, _, _) if degrees.is_some_and(|d| d.abs() <= 45) => {
                                Some(3)
                            }
                            (false, true, _, _) | (false, _, _, true) => Some(20),
                            (false, _, true, _) => Some(28),
                            (true, true, _, _) if self.gauges.light > 0 => Some(7),
                            (true, _, true, _) if self.gauges.light > 0 => Some(88),
                            _ if frame >= 50. && input.auxiliary_pressed != 0 => {
                                Some(if sword { 91 } else { 89 })
                            }
                            _ if frame >= 50. && !sword && !chord && input.command(27) => Some(111),
                            _ => None,
                        };
                        self.roll_followup = followup.or(self.roll_followup);
                    }
                    if dash && self.roll_followup.is_none() {
                        self.roll_followup = Some(if sword { SWORD_SPRINT } else { AXE_SPRINT });
                    }
                    if frame >= 50.
                        && let Some(id) = self.roll_followup
                    {
                        return self.begin(actor, id);
                    }
                }
                return false;
            }
            self.roll_followup = None;
            if !ground && !sheathed_move {
                return false;
            }
            let terrain_exit = ground && self.resume_sprint.pending();
            let axe_sprint = self.sprint_form == SprintForm::Axe;
            // FZ 812103F4 waits for the sword to return to neutral, then
            // requests the normal sword-to-axe animation with no input.
            if ground
                && drawn
                && self.gauges.sword
                && self.gauges.light == 0
                && !(terrain_exit && axe_sprint)
            {
                return self.begin(actor, program::SWORD_TO_AXE);
            }
            if action::activate_hunter_art(actor) {
                return true;
            }
            if input.auxiliary_pressed != 0 {
                return self.begin(actor, if self.gauges.sword { 91 } else { 89 });
            }
            // FZ 8121275E / 812128C0 bind the portable layout's X+A to
            // light-sword draw. GU 2A79CC snapshots presses before its sprint filter.
            let stance = if drawn {
                stance
            } else {
                chord || (sheathed_move && read::<u32>(actor, 5124) & 0x120 == 0x120)
            };
            if stance && let Some(id) = self.gauges.stance_action(drawn) {
                return self.begin(actor, id);
            }
            if !drawn
                && current == (1, 0)
                && !move_stick
                && charge
                && read::<u16>(actor, 5112) & native!(0x65f93c) as u16 != 0
            {
                return self.begin(actor, 111);
            }
            if !drawn && !ground {
                return false;
            }
            let sword = self.gauges.sword;
            let slash = arts::slash(actor);
            if let Some(attack) = landing_attack {
                return attack
                    .action(self.gauges, slash)
                    .is_some_and(|id| self.begin(actor, id));
            }
            let id = if terrain_exit {
                let sprint_energy = if axe_sprint { slash } else { self.gauges.light };
                if move_stick && sprint_energy > 0 {
                    self.sprint_form.resume()
                } else {
                    self.sprint_form.brake()
                }
            } else if drawn && dash {
                if sword && self.gauges.unlocked {
                    SWORD_SPRINT
                } else {
                    AXE_SPRINT
                }
            } else if !drawn {
                return false;
            } else if charge {
                if sword { 162 } else { 111 }
            } else if chord && !sword && slash > 0 && let Some(dir) = slash_dir {
                106 + dir
            } else if chord {
                // FZ 81212AA8 starts discharge from light-sword idle in any direction.
                if sword { 70 } else { 2 }
            } else if x {
                landing::ground_x(sword, move_stick)
            } else if a {
                if sword { 78 } else { 1 }
            } else {
                return false;
            };
            self.begin(actor, id)
        }
    }
}

unsafe fn is_forward_roll(actor: u32, current: (u16, u16)) -> bool {
    current == FORWARD_ROLL && unsafe { native!(0x53a058, actor) == 28 }
}

unsafe fn scaled_gain(actor: u32, index: usize) -> u16 {
    let factor = match unsafe { action::ChargeSkill::equipped(actor) } {
        action::ChargeSkill::Normal => 1000,
        action::ChargeSkill::Focus => program::param(46),
        action::ChargeSkill::Slow => program::param(48),
    };
    (u32::from(program::param(index)) * u32::from(factor) / 1000) as u16
}

struct Local(UnsafeCell<Option<Battle>>);
unsafe impl Sync for Local {}
static STATE: Local = Local(UnsafeCell::new(None));
static BUSY: AtomicBool = AtomicBool::new(false);

fn with_local<T>(visit: impl FnOnce(&mut Option<Battle>) -> T) -> Option<T> {
    if BUSY.swap(true, Acquire) {
        return None;
    }
    let result = visit(unsafe { &mut *STATE.0.get() });
    BUSY.store(false, Release);
    Some(result)
}

pub unsafe extern "C" fn dispatch(actor: u32, raw: *const Input) -> u32 {
    let handled = with_local(|state| unsafe {
        if let Some(input) = raw.as_ref() {
            let equipment = read(actor, 5060);
            if actor != OWNER.load(Relaxed)
                || state.as_ref().is_none_or(|s| s.equipment != equipment)
            {
                EXPLOSIONS.with(|slots| slots[0] = None);
                fx::clear(0);
                *state = Some(Battle::new(equipment));
                OWNER.store(actor, Relaxed);
                HITS.store(0, Relaxed);
                DISCHARGE_HITS.store(0, Relaxed);
                BLADE_KIND.store(0, Relaxed);
            }
            let handled = state.as_mut().unwrap().tick(actor, input);
            // Axe wild swings check stamina; light-sword flurries check light.
            // Neither loop should run GU's passive stamina regeneration.
            let (group, slot) = action::current(actor);
            if matches!(action::custom_id(group, slot), Some(8 | 9 | 45 | 155..=157)) {
                movement::defer_stamina_recovery(actor);
            }
            handled
        } else {
            if actor != 0
                && actor == OWNER.load(Relaxed)
                && let Some(battle) = state.as_mut()
            {
                let (group, slot) = action::current(actor);
                if action::custom(group, slot) {
                    battle.finish(actor);
                }
            }
            EXPLOSIONS.with(|slots| slots[0] = None);
            fx::clear(0);
            *state = None;
            OWNER.store(0, Relaxed);
            HITS.store(0, Relaxed);
            mhfz_sdk::camera::reset();
            false
        }
    });
    if handled == Some(true) {
        0
    } else {
        style::PASS
    }
}

pub unsafe extern "C" fn commands(actor: u32, table: u32) -> *const action::command::CommandSet {
    use action::command::CommandSet;
    static TERRAIN: CommandSet = CommandSet::new(&[]).retaining_native(1, movement::CLIMBS);
    // 119FE44 selects these tables from the held form. Preserve their
    // common rows and let the style resolve the weapon attack itself.
    static SWORD_ROLL: CommandSet = CommandSet::new(&[]).replacing_weapon(41);
    static AXE_ROLL: CommandSet = CommandSet::new(&[]).replacing_weapon(72);
    let current = unsafe { action::current(actor) };
    if matches!(table, 2025 | 2026) && unsafe { is_forward_roll(actor, current) } {
        return if table == 2025 {
            &SWORD_ROLL
        } else {
            &AXE_ROLL
        };
    }
    if table == SPRINT_COMMAND_TABLE
        && action::custom_id(current.0, current.1).is_some_and(program::is_sprint)
    {
        &TERRAIN
    } else {
        core::ptr::null()
    }
}

const WEAPON_X: u8 = 14;
const WEAPON_A: u8 = 2;
const WEAPON_XA: u8 = 6;
const CHARGE: u8 = 70;
const STANCE: u8 = 8;
const SHEATHE: u8 = 13;

// 2E9D54 selects stock w08 openers before 282DA8 runs the style. FZ
// uses the special key for stance changes, even when 81214070 rejects them.
unsafe fn style_idle_input(actor: u32) -> bool {
    unsafe {
        let attacks = read::<u32>(actor, 5156);
        attacks & ((1 << WEAPON_X) | (1 << WEAPON_A) | (1 << WEAPON_XA)) != 0
            || pad::attack_chord_pressed(actor)
            || pad::attack_chord_with_shoulder(actor)
            || read::<u32>(actor, 5148) & (1 << (CHARGE % 32)) != 0
            || read::<u32>(actor, 5140) & (1 << STANCE) != 0
    }
}

pub unsafe extern "C" fn request(actor: u32, r: *const ActionRequest) -> u32 {
    let req = unsafe { &*r };
    if !req.draw_route
        && req.group == action::GROUP
        && matches!(req.flags, 0 | 0x1000)
        && matches!(req.slot, 0..=7 | 19 | 20 | 35 | 38 | 800)
    {
        let current = unsafe { action::current(actor) };
        // Keep stock weapon commands from bypassing the landing recovery.
        if unsafe { native!(0x2a2444, actor) != 0 }
            && (unsafe { landing::recovering(actor, current) }
                || (movement::ground_idle(current) && unsafe { style_idle_input(actor) }))
        {
            return style::PASS;
        }
    }
    if req.group == action::GROUP
        && req.flags & 128 == 0
        && unsafe { native!(0x2a2444, actor) } == 0
    {
        return match req.slot {
            14 => 10,
            40 => 89,
            _ if req.draw_route => style::PASS,
            _ => style::NATIVE_ACTION,
        };
    }
    if req.draw_route {
        style::PASS
    } else {
        style::NATIVE_ACTION
    }
}
pub unsafe extern "C" fn guard_policy(
    actor: u32,
    result: *mut guard::GuardResult,
) -> guard::Decision {
    let (group, slot) = unsafe { action::current(actor) };
    if !matches!(action::custom_id(group, slot), Some(89 | 91)) {
        return guard::Decision::Native;
    }
    if unsafe { read::<u32>(actor, 9488) } & 16 == 0 {
        return guard::Decision::Reject;
    }
    unsafe {
        result.write(guard::GuardResult {
            invulnerability: 14.,
            recoil: 0,
            sharpness_power: u32::MAX,
        });
    }
    guard::Decision::Precise
}

unsafe extern "C" fn hit(actor: u32) {
    if OWNER.load(Relaxed) == actor {
        let (g, s) = unsafe { action::current(actor) };
        if action::custom(g, s) {
            let kind = BLADE_KIND.load(Relaxed);
            if kind & 1 != 0 {
                // GU applies the equipped skill and rounds each contact independently.
                unsafe { arts::change_slash(actor, i32::from(program::param(29))) };
                HITS.fetch_add(1, Relaxed);
            }
            if kind & 2 != 0 {
                DISCHARGE_HITS.fetch_add(1, Relaxed);
            }
        }
    } else {
        unsafe { peer::hit(actor) };
    }
}

pub(super) unsafe fn hud_gauges() -> Option<(u16, u16, bool)> {
    let actor = OWNER.load(Relaxed);
    if actor == 0 || unsafe { style::hunter::local() } != Some(actor) {
        return None;
    }
    let value = HUD.load(Relaxed);
    Some((
        unsafe { arts::slash(actor) },
        value as u16,
        value >> 16 != 0,
    ))
}

pub fn install() {
    arts::install();
    motion::install();
    unsafe {
        action::melee::bind_contact(STYLE_ID, hit);
        style::world_contact::bind(STYLE_ID, expand);
    }
    fx::install();
    crate::hud::install();
}
