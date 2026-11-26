mod timeline;
use crate::{
    STYLE_ID, motion, native,
    program::{Gauges, charge_clip},
};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use mhfz_sdk::effects::{MotionClock, world_pose_angles};
use mhgu_sdk::weapon::style::{
    action::MotionFrame,
    particle::{self, Anchor, Bank},
};

const SELECTORS: &[[u8; 4]] = include_bytes!("../target/assets/effects/selectors.bin")
    .as_chunks::<4>()
    .0;
const EVENTS: &[[u8; 18]] = include_bytes!("../target/assets/effects/motion_fx.bin")
    .as_chunks::<18>()
    .0;
static BANK: Bank = Bank::new(
    cr"effect\frontier_sa\particles",
    include_bytes!("../target/assets/effects/routes.bin"),
);

pub(super) unsafe fn preload() {
    unsafe { BANK.preload() };
}

fn effect_route(bank: u16, selector: u16) -> Option<usize> {
    let key = u32::from(bank) | (u32::from(selector) << 16);
    SELECTORS
        .iter()
        .position(|row| u32::from_le_bytes(*row) == key)
}

// FZ 817BC5B4 maps the first 21 nodes to the hunter skeleton. Node 21
// resolves to the equipped model's first node (8120FBEE / 81EC78EE).
// GU w08 11A72BC submits its own effects on the same equipment parent/joint 0.
unsafe fn attachment(actor: u32, node: u16) -> Option<(u32, i16)> {
    let binding = match node {
        0 => (actor, -1),
        2 => (actor, 0),
        5 => (actor, 16),
        8 => (actor, 19),
        10 => (actor, 2),
        14 => (actor, 8),
        17 => (actor, 11),
        18 => (actor, 12),
        21 => (unsafe { crate::read(actor, 9120) }, 0),
        _ => return None,
    };
    (binding.0 != 0).then_some(binding)
}

unsafe fn attached(actor: u32, node: u16, offset: [f32; 3]) -> Option<Anchor> {
    unsafe {
        let (model, joint) = attachment(actor, node)?;
        // FZ 8181818A uses 81FD3A54 to sample the joint basis on every update.
        // Motion kind 7 mode 0 and the left-hand cartridge also use the live pose.
        Some(Anchor::JointPose {
            parent: particle::parent(model)?,
            joint,
            offset: Some(offset),
        })
    }
}

unsafe fn matrix(actor: u32, node: u16) -> Option<[f32; 16]> {
    unsafe {
        let (model, joint) = attachment(actor, node)?;
        let ptr = particle::joint_matrix(model, joint as u32)?;
        Some(crate::read(ptr, 0))
    }
}

fn angle_units(radians: f32) -> u16 {
    (radians * (65536. / core::f32::consts::TAU)) as i32 as u16
}

fn swing_angles(yaw: u16, [pitch, roll]: [f32; 2]) -> [u16; 3] {
    if pitch == 0. {
        return [0, yaw, (roll * (65536. / 360.)) as i32 as u16];
    }
    // FZ applies Rz * Rx in hunter space before applying the hunter yaw.
    // Re-express that local rotation in GU's order, then add the yaw.
    let (sp, cp) = libm::sincosf(pitch.to_radians());
    let (sr, cr) = libm::sincosf(roll.to_radians());
    let zx = sr * sp;
    [
        angle_units(libm::atan2f(cr * sp, libm::hypotf(zx, cp))),
        yaw.wrapping_add(angle_units(libm::atan2f(zx, cp))),
        angle_units(libm::atan2f(sr, cr * cp)),
    ]
}

pub unsafe fn update(
    clock: &mut MotionClock,
    slot: u8,
    actor: u32,
    clip: u16,
    sample: MotionFrame,
) {
    for event in clock.advance(EVENTS, clip, sample) {
        unsafe {
            let (mut pos, yaw) = particle::world_offset(actor, event.offset);
            let mut rotation = [0, yaw, 0];
            if event.node != 0 {
                let Some(pose) = matrix(actor, u16::from(event.node)) else {
                    continue;
                };
                pos = particle::transform_offset(&pose, event.offset);
                if matches!(event.mode, 0 | 2) {
                    rotation = world_pose_angles(&pose);
                }
            }
            let mut bank = event.bank;
            let mut wet = false;
            if matches!(event.kind, 1 | 5) {
                let Some((floor, water)) = particle::ground_heights(actor, pos) else {
                    continue;
                };
                wet = water.is_some();
                pos[1] = water.unwrap_or(floor);
                if wet {
                    bank += 1;
                }
                if event.mode == 4 {
                    rotation[1] = yaw.wrapping_add(0x4000);
                }
            }
            let route = if event.kind == 1 {
                SELECTORS.len()
                    + if wet {
                        0
                    } else if event.bank == 7 {
                        2
                    } else {
                        1
                    }
            } else {
                let Some(index) = effect_route(bank, event.selector) else {
                    continue;
                };
                index
            };
            let anchor = if event.kind == 7 {
                let Some(anchor) = attached(actor, u16::from(event.node), event.offset) else {
                    continue;
                };
                anchor
            } else {
                Anchor::RotatedWorld(pos, rotation)
            };
            let life = match (clip, event.kind) {
                (1055 | 1479, 5) => Lifetime::Sprint,
                (1480, 6 | 7) => Lifetime::Brake,
                (motion::SPRINT_SHEATHE, 7) => Lifetime::SprintSheathe,
                _ => Lifetime::Natural,
            };
            spawn(route, anchor, slot, life);
        }
    }
}

/// FZ 81214E4E, the flash on entry to the highest discharge tier.
pub unsafe fn discharge_flash(actor: u32, slot: u8) {
    unsafe {
        if let Some(route) = effect_route(4014, 21)
            && let Some(pose) = matrix(actor, 21)
        {
            spawn(
                route,
                Anchor::RotatedWorld([pose[12], pose[13], pose[14]], world_pose_angles(&pose)),
                slot,
                Lifetime::Clip1456,
            );
        }
    }
}

// FZ family 107 creates these world-space effects from the burst center.
// 81164796 calls 81214D62 for row 1 and 81214DD8 for row 2.
pub(super) unsafe fn phial_burst(actor: u32, row: u16, center: [f32; 3]) {
    let (accent, cue) = if row == 1 { (3, 79) } else { (4, 78) };
    unsafe {
        let phial = Phial::equipped(actor);
        emit(4013, phial.accent(accent), Anchor::World(center, 0));
        crate::sound::phial_burst(actor, cue, center);
    }
}

fn emit(bank: u16, selector: u16, anchor: Anchor) {
    if let Some(route) = effect_route(bank, selector) {
        unsafe {
            BANK.emit(route, anchor);
        }
    }
}

fn crossed_frame(previous: f32, sample: MotionFrame, frame: f32) -> bool {
    if sample.blend > 0. {
        return false;
    }
    if previous <= sample.frame {
        (previous..sample.frame).contains(&frame)
    } else {
        (previous..sample.end).contains(&frame) || (sample.restart..sample.frame).contains(&frame)
    }
}

// 81216E36 dispatches these effects independently of the motion FX table.
// The 1068 lead only crosses the first slash. 1065 and 1490 may wrap to
// the same step while event 20 alternates the two arcs.
pub unsafe fn charged_swing(
    actor: u32,
    clip: u16,
    previous: f32,
    sample: MotionFrame,
    previous_phase: bool,
    current_phase: bool,
) {
    if !matches!(clip, 1065 | 1068 | 1490 | 1498) {
        return;
    }
    for (i, frame) in [24., 54.].into_iter().enumerate() {
        if !crossed_frame(previous, sample, frame) || (clip == 1068 && i == 1) {
            continue;
        }
        let sword = matches!(clip, 1065 | 1068);
        let in_head = sample.frame < previous && frame < sample.frame && sample.restart <= frame;
        let phase = clip != 1068
            && if in_head {
                current_phase
            } else {
                previous_phase
            };
        let (offset, pitch, roll) = if sword {
            (
                [0., 120., 20.],
                0,
                match (phase, i) {
                    (false, 0) => 140,
                    (false, _) => 250,
                    (true, 0) => 130,
                    _ => 240,
                },
            )
        } else {
            match (phase, i) {
                (false, 0) => ([220., 370., 50.], 15, 140),
                (false, _) => ([-310., 320., 50.], 10, 240),
                (true, 0) => ([240., 320., 80.], 5, 130),
                _ => ([-320., 230., 120.], -5, 250),
            }
        };
        unsafe {
            // GU 11A7F5C reads equipped property 27. FZ 824F0978 orders the
            // four style banks by the equipped Frontier phial type.
            let bank = Phial::equipped(actor).bank;
            let (position, yaw) = particle::world_offset(actor, offset);
            emit(
                bank,
                if sword { 31 } else { 13 },
                Anchor::ScaledWorld {
                    position,
                    angles: swing_angles(yaw, [pitch as f32, roll as f32]),
                    scale: if sword { 1.1 } else { 1. },
                },
            );
            if sword {
                let (pos, yaw) = particle::world_offset(actor, [0.; 3]);
                emit(bank, if i == 0 { 27 } else { 26 }, Anchor::World(pos, yaw));
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Phial {
    bank: u16,
    accents: [u16; 7],
}
impl Phial {
    unsafe fn equipped(actor: u32) -> Self {
        let (bank, accents) = match unsafe { native!(0x27c708, actor, 27) } {
            1 | 3 => (4018, [8, 9, 10, 11, 18, 24, 25]),
            2 | 5 => (4016, [0, 1, 2, 3, 16, 20, 21]),
            4 => (4017, [4, 5, 6, 7, 17, 22, 23]),
            _ => (4019, [12, 13, 14, 15, 19, 26, 27]),
        };
        Self { bank, accents }
    }

    fn accent(self, kind: usize) -> u16 {
        self.accents[kind]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Lifetime {
    Natural,
    Hunter,
    BladeGlow,
    Sprint,
    Brake,
    SprintSheathe,
    Clip1423,
    Clip1455,
    Clip1456,
    Guard,
    Hold,
    HoldStart,
    Charge,
    ChargeStart,
    Cartridge,
}

impl Lifetime {
    fn active(self, clip: u16, frame: f32, charge: u8) -> bool {
        match self {
            Self::Natural | Self::Hunter | Self::BladeGlow => true,
            Self::Sprint => matches!(clip, 1055 | 1479),
            Self::Brake => clip == 1480,
            Self::SprintSheathe => clip == motion::SPRINT_SHEATHE,
            Self::Cartridge => cartridge_window(clip, frame),
            Self::Clip1423 => clip == 1423,
            Self::Clip1455 => clip == 1455,
            Self::Clip1456 => clip == 1456 && frame < 140.,
            Self::Guard => matches!(clip, 1472 | 1473),
            Self::Hold | Self::HoldStart => {
                matches!(clip, 1447 | 1448) && (self == Self::Hold || frame < 40.)
            }
            Self::Charge | Self::ChargeStart => {
                charge_clip(clip) && charge < 3 && (self == Self::Charge || charge < 2)
            }
        }
    }
}

unsafe fn release_accent(actor: u32, strong: bool) {
    unsafe {
        let phial = Phial::equipped(actor);
        if let Some(anchor) = attached(actor, 21, [0., 0., 20.]) {
            let (bank, sel) = if strong {
                (4013, phial.accent(1))
            } else {
                (phial.bank, 21)
            };
            emit(bank, sel, anchor);
            crate::sound::effect(actor, 0, if strong { 17 } else { 24 });
        }
    }
}

fn cartridge_window(clip: u16, frame: f32) -> bool {
    match clip {
        1414 => (68. ..84.).contains(&frame),
        1456 => (100. ..142.).contains(&frame),
        _ => false,
    }
}

struct Unit {
    handle: u32,
    slot: u8,
    life: Lifetime,
}
static UNITS: mhfz_sdk::sync::Shared<Vec<Unit>> = mhfz_sdk::sync::Shared::new(Vec::new());
static TRACKING: AtomicBool = AtomicBool::new(false);

extern "C" fn released(handle: u32) {
    UNITS.with(|units| {
        if let Some(index) = units.iter().position(|u| u.handle == handle) {
            units.swap_remove(index);
        }
    });
}

pub fn install() {
    TRACKING.store(particle::on_release(STYLE_ID, released), Ordering::Release);
}

unsafe fn retire(slot: u8, keep: impl Fn(Lifetime) -> bool) {
    // Native stop can synchronously deliver a release notification.
    loop {
        let next = UNITS.with(|units| {
            let index = units.iter().position(|u| u.slot == slot && !keep(u.life))?;
            Some(units.swap_remove(index).handle)
        });
        let Some(unit) = next else { break };
        unsafe { particle::stop_unit(unit) };
    }
}

pub unsafe fn clear(slot: u8) {
    unsafe { retire(slot, |_| false) };
}

pub unsafe fn retire_for_sprint_sheathe(slot: u8) {
    unsafe { retire(slot, |life| life == Lifetime::SprintSheathe) };
}

fn spawn(route: usize, anchor: Anchor, slot: u8, life: Lifetime) {
    unsafe {
        if life == Lifetime::Natural {
            BANK.emit(route, anchor);
        } else if TRACKING.load(Ordering::Acquire) {
            BANK.emit_tracked(route, anchor, |handle| {
                UNITS.with(|units| units.push(Unit { handle, slot, life }));
            });
        }
    }
}

struct Visuals {
    actor: u32,
    slot: u8,
    previous: f32,
    sample: MotionFrame,
    phial: Phial,
    swing_window: core::ops::Range<f32>,
}

impl Visuals {
    fn crossed(&self, frame: f32) -> bool {
        crossed_frame(self.previous, self.sample, frame)
    }

    fn spawn(&self, bank: u16, selector: u16, anchor: Anchor, life: Lifetime) {
        if let Some(route) = effect_route(bank, selector) {
            spawn(route, anchor, self.slot, life);
        }
    }

    fn joint(&self, bank: u16, selector: u16, node: u16, offset: [f32; 3], life: Lifetime) {
        if let Some(anchor) = unsafe { attached(self.actor, node, offset) } {
            self.spawn(bank, selector, anchor, life);
        }
    }

    fn pose(&self, bank: u16, selector: u16, node: u16, offset: [f32; 3], life: Lifetime) {
        self.rotated_pose(bank, selector, node, offset, life, None);
    }

    fn rotated_pose(
        &self,
        bank: u16,
        selector: u16,
        node: u16,
        offset: [f32; 3],
        life: Lifetime,
        rotation: Option<i16>,
    ) {
        unsafe {
            let (mut pos, yaw) = particle::world_offset(self.actor, offset);
            let mut angles = [0, yaw, 0];
            if node != 0 {
                let Some(pose) = matrix(self.actor, node) else {
                    return;
                };
                pos = particle::transform_offset(&pose, offset);
                angles = world_pose_angles(&pose);
            }
            if let Some(delta) = rotation {
                angles = [0, yaw.wrapping_add_signed(delta), 0];
            }
            self.spawn(bank, selector, Anchor::RotatedWorld(pos, angles), life);
        }
    }

    fn swing(&self, bank: u16, selector: u16, offset: [f32; 3], pitch_roll: [f32; 2], scale: f32) {
        let (position, yaw) = unsafe { particle::world_offset(self.actor, offset) };
        self.spawn(
            bank,
            selector,
            Anchor::ScaledWorld {
                position,
                angles: swing_angles(yaw, pitch_roll),
                scale,
            },
            Lifetime::Natural,
        );
    }

    fn ground(&self, selector: u16, offset: [f32; 3]) {
        unsafe {
            let (pos, yaw) = particle::world_offset(self.actor, offset);
            let wet = particle::ground_heights(self.actor, pos)
                .and_then(|(_, water)| water)
                .is_some_and(|water| water - pos[1] > -0.5);
            self.spawn(
                if wet { 4036 } else { 4035 },
                selector,
                Anchor::World(pos, yaw),
                Lifetime::Natural,
            );
        }
    }

    fn cartridge(&self) {
        if UNITS.with(|units| {
            units
                .iter()
                .any(|u| u.slot == self.slot && u.life == Lifetime::Cartridge)
        }) {
            return;
        }
        unsafe {
            if TRACKING.load(Ordering::Acquire)
                && let Some(anchor) = attached(self.actor, 18, [-9., -3., 0.])
            {
                BANK.emit_tracked(SELECTORS.len() + 3, anchor, |handle| {
                    UNITS.with(|units| {
                        units.push(Unit {
                            handle,
                            slot: self.slot,
                            life: Lifetime::Cartridge,
                        })
                    });
                });
            }
        }
    }

    fn sound(&self, bank: usize, cue: u8) {
        unsafe { crate::sound::effect(self.actor, bank, cue) };
    }
}

// 83A8F6E4 is shared by all FZ hunters. GU dispatches the local hunter once
// per simulation update, including when another weapon is equipped.
#[derive(Clone, Copy, Default)]
struct SceneClock {
    remainder: f32,
    start: u16,
    end: u16,
}
static SCENE: mhfz_sdk::sync::Shared<SceneClock> = mhfz_sdk::sync::Shared::new(SceneClock {
    remainder: 0.,
    start: 0,
    end: 0,
});

pub fn scene_frame(dt: f32) {
    SCENE.with(|clock| {
        let elapsed = clock.remainder + dt;
        clock.start = clock.end;
        clock.end = clock.end.wrapping_add((elapsed / 2.) as u32 as u16);
        clock.remainder = elapsed % 2.;
    });
}

#[derive(Default)]
pub struct WeaponFx {
    last_tick: Option<u16>,
    swing_clock: MotionClock,
    previous_tier: u8,
    charge_stage: u8,
    sound_delay: f32,
    release: Option<bool>,
    sword_accent: bool,
}

impl WeaponFx {
    pub unsafe fn enter(&mut self, actor: u32, slot: u8, state: u8, clip: u16, frame: f32) {
        self.swing_clock.seek(clip, frame);
        // The two short-lived FZ objects 8102AE94/8102AF3E watch the release
        // destination. Keep them alive while standing/walking variants alternate.
        if let Some(hold) = self.release {
            let still_holding = if hold {
                matches!(clip, 1447 | 1448)
            } else {
                charge_clip(clip)
            };
            if !still_holding {
                let strong = if hold {
                    matches!(state, 127 | 130 | 133 | 136 | 158..=161)
                } else {
                    matches!(state, 116 | 122 | 125 | 155 | 157 | 166)
                };
                unsafe { release_accent(actor, strong) };
                self.release = None;
            }
        }
        unsafe { retire(slot, |life| life.active(clip, frame, self.charge_stage)) };
        if clip != 1455 {
            self.previous_tier = 1;
        }
        if !charge_clip(clip) {
            self.charge_stage = 0;
        }
    }

    pub unsafe fn update(
        &mut self,
        slot: u8,
        actor: u32,
        clip: u16,
        state: u8,
        previous: f32,
        sample: MotionFrame,
        dt: f32,
        gauges: Gauges,
        charge: u8,
        drawn: bool,
    ) {
        if unsafe { crate::read::<u32>(actor, 5060) } == 0 {
            unsafe { clear(slot) };
            return;
        }
        let visible = unsafe { crate::read::<u32>(actor, motion::SWORD_POSE) } != 0;
        let unlocked = gauges.unlocked;
        let tier = match state {
            85 => 2,
            86 => 3,
            _ => gauges.discharge_tier() as u8 + 1,
        };
        // 817330BA checks the current pose and its next playhead. Sampling
        // last-to-current emits the slash from the following pose on a moving hunter.
        let v = Visuals {
            actor,
            slot,
            previous,
            sample,
            phial: unsafe { Phial::equipped(actor) },
            swing_window: self.swing_clock.window(clip, sample),
        };
        let frame = sample.frame;
        let normal = sample.blend <= 0.;
        let glowing = drawn
            && visible
            && unlocked
            && gauges.light > 0
            && !(clip == 1456 && normal && frame >= 138.);
        unsafe {
            retire(slot, |life| {
                life.active(clip, frame, charge) && (life != Lifetime::BladeGlow || glowing)
            })
        };
        timeline::play(&v, clip, state, tier, unlocked);
        if normal && cartridge_window(clip, frame) {
            v.cartridge();
        }
        // 8121A3FE plays the transform accent once on entry to visible sword.
        if clip == 1440 && unlocked && visible && !self.sword_accent {
            v.sound(0, 26);
            self.sword_accent = true;
        } else if clip != 1440 {
            self.sword_accent = false;
        }
        if matches!(clip, 1447 | 1448) && v.crossed(40.) {
            self.release = Some(true);
        }
        if clip == 1455 && normal && frame >= 60. {
            if tier > self.previous_tier && tier >= 2 {
                v.joint(4014, 17 + u16::from(tier), 21, [0.; 3], Lifetime::Natural);
            }
            self.previous_tier = tier;
        } else {
            self.previous_tier = 1;
        }
        let holding = charge_clip(clip);
        let stage = self.charge_stage;
        if holding {
            if charge > stage {
                let (bank, selector, life) = match charge {
                    1 => (4014, 25, Lifetime::ChargeStart),
                    2 => (4014, 24, Lifetime::Charge),
                    _ => (v.phial.bank, 21, Lifetime::Natural),
                };
                v.joint(bank, selector, 21, [0., 0., 20.], life);
                if charge == 2 {
                    self.release = Some(false);
                }
                if charge == 3 {
                    self.release = None;
                }
                if charge >= 2 {
                    v.sound(0, 21 + charge);
                }
            }
            self.charge_stage = charge;
        } else {
            self.charge_stage = 0;
        }
        self.sound_delay = (self.sound_delay - dt).max(0.);
        let cue = match clip {
            1455 if normal && (114. ..216.).contains(&frame) => Some((28, 40.)),
            1055 | 1479 if normal && frame >= 22. => Some((20, 30.)),
            1447 | 1448 if normal && frame >= 10. => {
                Some((if frame >= 40. { 22 } else { 21 }, 30.))
            }
            _ if holding && (matches!(clip, 1489 | 1064) || normal && frame >= 52.) => {
                Some((if stage >= 2 { 22 } else { 21 }, 30.))
            }
            _ => None,
        };
        if let Some((cue, delay)) = cue
            && self.sound_delay == 0.
        {
            v.sound(0, cue);
            self.sound_delay = delay;
        }
        let clock = SCENE.with(|c| *c);
        let ticks = clock.end.wrapping_sub(clock.start);
        let new_tick = ticks > 0 && self.last_tick != Some(clock.end);
        self.last_tick = Some(clock.end);
        let due = |period: u16| {
            new_tick
                && (ticks >= period
                    || (1..=ticks).any(|i| clock.start.wrapping_add(i) % period == 0))
        };
        if glowing && due(5) {
            v.joint(v.phial.bank, 8, 21, [-10., 0., 20.], Lifetime::BladeGlow);
        }
        if clip == 1455
            && normal
            && (114. ..216.).contains(&frame)
            && due(if tier < 2 { 5 } else { 4 })
        {
            v.joint(
                v.phial.bank,
                3 + u16::from(tier),
                21,
                [-20., 0., 40.],
                Lifetime::Clip1455,
            );
        }
        if normal
            && match clip {
                1423 => (22. ..126.).contains(&frame),
                1455 => (114. ..216.).contains(&frame),
                1456 => (88. ..124.).contains(&frame),
                _ => false,
            }
            && due(4)
        {
            v.ground(17, [0., 0., -40.]);
        }
        if matches!(clip, 1055 | 1479) && normal && frame >= 34. {
            if due(27) {
                v.joint(4013, v.phial.accent(0), 21, [0., 0., 60.], Lifetime::Sprint);
            }
            if due(5) {
                v.rotated_pose(
                    v.phial.bank,
                    25,
                    21,
                    [0., 0., 40.],
                    Lifetime::Sprint,
                    Some(i16::MIN),
                );
            }
        }
        if due(8) {
            if holding && stage == 2 {
                v.joint(v.phial.bank, 20, 21, [0., 0., 20.], Lifetime::Charge);
            }
            if matches!(clip, 1447 | 1448) && normal && frame >= 50. {
                v.joint(v.phial.bank, 20, 21, [0., 0., 20.], Lifetime::Hold);
            }
        }
    }
}
