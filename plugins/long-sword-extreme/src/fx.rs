//! Frontier effects played by GU's native particle manager.
use crate::{
    native,
    peer::{COUNT, Slot},
    read,
};
use mhfz_sdk::effects::{self, MotionEvent};
use mhgu_sdk::{
    geometry::Angle,
    style::{
        action,
        particle::{self, Anchor, Bank, TrackedUnits},
    },
};

static PARTICLES: Bank = Bank::new(c"effect\\frontier_ls\\particles", ROUTES);

pub(super) unsafe fn preload() {
    unsafe { PARTICLES.preload() };
}
const ROUTES: &[u8; 332] = include_bytes!("../target/assets/effects/routes.bin");

const fn route_layers(route: usize) -> usize {
    u16::from_le_bytes([ROUTES[route * 4 + 2], ROUTES[route * 4 + 3]]) as usize
}

const DASH_CHARGE_AURA: usize = 19; // 4023:4
const DASH_CHARGE_FLASH: usize = 20; // 4023:5
// 811F9C8A..811F9D82, local Z on FZ node 14 (GU sword joint 8).
const DASH_CHARGE_DISTANCES: [f32; 3] = [100., 160., 220.];
const DASH_CHARGE_AURAS: usize = DASH_CHARGE_DISTANCES.len();
const DASH_CHARGE_FLASH_LAYERS: usize = 23;
const _: () = {
    assert!(route_layers(DASH_CHARGE_AURA) == 1);
    assert!(route_layers(DASH_CHARGE_FLASH) == DASH_CHARGE_FLASH_LAYERS);
};
static DASH_CHARGE_UNITS: [TrackedUnits<{ DASH_CHARGE_FLASH_LAYERS + DASH_CHARGE_AURAS }>; COUNT] =
    [const { TrackedUnits::new() }; COUNT];
// Clip 1439 spawns an authored ground burst at frame 18, followed by the
// release-specific burst and sparks at frame 24. The water burst has more layers.
const DASH_SLASH_BURST_SLOT: usize = route_layers(61);
const DASH_SLASH_SPARK_SLOT: usize = DASH_SLASH_BURST_SLOT + route_layers(23);
const DASH_SLASH_OPENING_SLOT: usize = DASH_SLASH_SPARK_SLOT + route_layers(21);
static DASH_SLASH_UNITS: [TrackedUnits<{ DASH_SLASH_OPENING_SLOT + route_layers(18) }>; COUNT] =
    [const { TrackedUnits::new() }; COUNT];
const _: () = {
    assert!(route_layers(59) <= DASH_SLASH_BURST_SLOT);
    assert!(route_layers(22) <= route_layers(23));
    assert!(route_layers(17) <= route_layers(18));
};
// Strong thrust emits three layers at two-frame intervals from 36 through 60.
const STAB_LAYERS: usize = 3;
const STAB_PULSES: usize = 13;
const _: () = {
    assert!(route_layers(1) == 1);
    assert!(route_layers(2) == STAB_LAYERS);
};
static STAB_UNITS: [TrackedUnits<{ STAB_PULSES * STAB_LAYERS }>; COUNT] =
    [const { TrackedUnits::new() }; COUNT];

fn sword_attachment(parent: u32, distance: f32) -> Anchor {
    Anchor::Joint {
        parent,
        joint: 8,
        offset: Some([0., 0., distance]),
    }
}

pub(super) struct DashCharge {
    slot: Slot,
    pending_flash: u32,
    pending_auras: [u32; DASH_CHARGE_AURAS],
}

impl DashCharge {
    pub(super) fn new(slot: Slot) -> Self {
        Self {
            slot,
            pending_flash: u32::MAX,
            pending_auras: [1; DASH_CHARGE_AURAS],
        }
    }

    pub(super) unsafe fn update(&mut self, actor: u32) {
        if self.pending_flash == 0 && self.pending_auras == [0; DASH_CHARGE_AURAS] {
            return;
        }
        unsafe {
            // Do not start the attached emitters with GU's unmapped-joint root fallback.
            if particle::joint_matrix(actor, 8).is_none() {
                return;
            }
            let Some(parent) = particle::parent(actor) else {
                return;
            };
            let units = &DASH_CHARGE_UNITS[self.slot.0];
            self.pending_flash = units.emit_layers(
                &PARTICLES,
                DASH_CHARGE_FLASH,
                sword_attachment(parent, DASH_CHARGE_DISTANCES[0]),
                self.pending_flash,
                0,
            );
            // FZ 811F8EEA starts all three aura instances on the one full-charge edge.
            for (i, (pending, distance)) in self
                .pending_auras
                .iter_mut()
                .zip(DASH_CHARGE_DISTANCES)
                .enumerate()
            {
                *pending = units.emit_layers(
                    &PARTICLES,
                    DASH_CHARGE_AURA,
                    sword_attachment(parent, distance),
                    *pending,
                    DASH_CHARGE_FLASH_LAYERS + i,
                );
            }
        }
    }
}

impl Drop for DashCharge {
    fn drop(&mut self) {
        unsafe { DASH_CHARGE_UNITS[self.slot.0].stop() };
    }
}

pub(super) struct DashSlash {
    slot: Slot,
    pub(super) charged: bool,
}

impl DashSlash {
    pub(super) fn new(slot: Slot, charged: bool) -> Self {
        Self { slot, charged }
    }

    pub(super) unsafe fn opening(&self, actor: u32) {
        if let Some(parent) = unsafe { particle::parent(actor) } {
            unsafe {
                DASH_SLASH_UNITS[self.slot.0].emit_layers(
                    &PARTICLES,
                    17 + usize::from(self.charged),
                    Anchor::Joint {
                        parent,
                        joint: 2,
                        offset: Some([0.; 3]),
                    },
                    u32::MAX,
                    DASH_SLASH_OPENING_SLOT,
                );
            }
        }
    }

    pub(super) unsafe fn impact(&self, actor: u32) {
        unsafe {
            let units = &DASH_SLASH_UNITS[self.slot.0];
            units.emit_layers(
                &PARTICLES,
                if self.charged { 23 } else { 22 },
                forward_anchor(actor, 0., 50.),
                u32::MAX,
                DASH_SLASH_BURST_SLOT,
            );
            if self.charged {
                units.emit_layers(
                    &PARTICLES,
                    21,
                    forward_anchor(actor, 100., 100.),
                    u32::MAX,
                    DASH_SLASH_SPARK_SLOT,
                );
            }
        }
    }
}

impl Drop for DashSlash {
    fn drop(&mut self) {
        unsafe { DASH_SLASH_UNITS[self.slot.0].stop() };
    }
}

pub(super) struct Stab {
    slot: Slot,
}

impl Stab {
    pub(super) fn new(slot: Slot) -> Self {
        Self { slot }
    }

    pub(super) unsafe fn pulse(&self, actor: u32, strong: bool, counter: bool, index: usize) {
        if let Some(parent) = unsafe { particle::parent(actor) } {
            let z = crate::program::STAB_REACH * if strong { 0.75 } else { 1. };
            unsafe {
                STAB_UNITS[self.slot.0].emit_layers(
                    &PARTICLES,
                    if strong { 2 } else { 1 },
                    if counter {
                        // Clip 1452 switches wrists at frame 24. Turn the
                        // 4007:2 layers (including their local Z offsets) with the blade.
                        Anchor::JointPose {
                            parent,
                            joint: 12,
                            offset: Some([0., 0., z]),
                        }
                    } else {
                        sword_attachment(parent, z)
                    },
                    u32::MAX,
                    index * STAB_LAYERS,
                );
            }
        }
    }
}

impl Drop for Stab {
    fn drop(&mut self) {
        unsafe { STAB_UNITS[self.slot.0].stop() };
    }
}

extern "C" fn release_tracked(unit: u32) {
    for units in &DASH_CHARGE_UNITS {
        units.forget(unit);
    }
    for units in &DASH_SLASH_UNITS {
        units.forget(unit);
    }
    for units in &STAB_UNITS {
        units.forget(unit);
    }
}

pub(super) fn install() {
    let mut ready = true;
    for units in &DASH_CHARGE_UNITS {
        ready &= units.bind(crate::STYLE_ID, release_tracked);
    }
    for units in &DASH_SLASH_UNITS {
        ready &= units.bind(crate::STYLE_ID, release_tracked);
    }
    for units in &STAB_UNITS {
        ready &= units.bind(crate::STYLE_ID, release_tracked);
    }
    if !ready {
        forge::log::prelude::warn!("Extreme LS: particle release service unavailable");
    }
}

pub(super) unsafe fn joint(actor: u32, joint: i16, route: usize) {
    if let Some(parent) = unsafe { particle::parent(actor) } {
        unsafe {
            PARTICLES.emit(
                route,
                Anchor::Joint {
                    parent,
                    joint,
                    offset: Some([0.; 3]),
                },
            );
        }
    }
}

pub(super) unsafe fn world(actor: u32, pos: [f32; 3], route: usize) {
    unsafe {
        PARTICLES.emit(route, Anchor::World(pos, read(actor, 4076)));
    }
}

pub(super) unsafe fn mark(
    actor: u32,
    monster: u32,
    bone: u16,
    offset: [f32; 3],
    position: [f32; 3],
    route: usize,
) {
    unsafe {
        // 6BE88 exposes the common actor's particle parent on hunter and monster vtables.
        let parent = read::<u32>(read(monster, 0), 304);
        if bone < 255 && parent == forge::mem::text_addr() + 0x6be88 {
            let anchor = Anchor::Joint {
                parent: native!(0x6be88, monster),
                joint: bone as i16,
                offset: Some(offset),
            };
            PARTICLES.emit(route, anchor);
        } else {
            world(actor, position, route);
        }
    }
}

// FZ 81F967E0 type 7 uses the grounded hunter's floor height for the sparks.
// Returns true when the footfall cue has been submitted for this dry step.
pub(super) unsafe fn dash_ground(actor: u32, charged: bool, sound_due: bool) -> bool {
    unsafe {
        // FZ uses node 0's evaluated position, and the hunter's current floor height.
        // GU's equivalent is the evaluated root (176), before the next movement update.
        let [x, _, z]: [f32; 3] = read(actor, 224);
        let yaw = read(actor, 4076);
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        let pos = [
            x - 120. * cos - 40. * sin,
            read(actor, 68),
            z + 120. * sin - 40. * cos,
        ];
        let Some((_, water)) = particle::ground_heights(actor, pos) else {
            return false;
        };
        if water.is_some_and(|surface| surface >= pos[1] - 18.) {
            return false;
        }
        PARTICLES.emit(if charged { 16 } else { 15 }, Anchor::World(pos, yaw));
        if sound_due {
            crate::sound::blade_at(actor, if charged { 9 } else { 8 }, pos);
        }
        sound_due
    }
}

pub(super) unsafe fn bone(actor: u32, id: u32) -> Option<[f32; 16]> {
    unsafe { particle::joint_matrix(actor, id).map(|matrix| read(matrix, 0)) }
}

unsafe fn forward_anchor(actor: u32, height: f32, distance: f32) -> Anchor {
    unsafe {
        let [x, y, z] = read::<[f32; 3]>(actor, 64);
        let yaw = read(actor, 4076);
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        Anchor::World([x + sin * distance, y + height, z + cos * distance], yaw)
    }
}

pub(super) unsafe fn discharge(actor: u32, tier: usize, yaw: u16) {
    unsafe {
        // Frontier samples the sword hand plus half the standard reach. Its
        // dry impact uses 4007:4; the two common water effects replace it.
        let [x, y, z] = read::<[f32; 3]>(actor, 64);
        let hand = bone(actor, 8).map_or([x, y, z], |m| [m[12], m[13], m[14]]);
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        let mut pos = [
            hand[0] + sin * crate::program::DISCHARGE_REACH,
            y,
            hand[2] + cos * crate::program::DISCHARGE_REACH,
        ];
        if let Some((_, Some(h))) = particle::ground_heights(actor, pos) {
            if y - 18. <= h {
                pos[1] = h + 5.;
            }
            PARTICLES.emit(34, Anchor::World(pos, yaw));
        } else {
            PARTICLES.emit(4, Anchor::World(hand, yaw));
        }
        // FZ 81FD39A0/81FD3B8E take position from the hand and
        // rotation from hunter yaw. Inheriting wrist rotation tilts the cut.
        PARTICLES.emit([13, 12, 11, 14][tier], Anchor::World(hand, yaw));
    }
}

pub(super) unsafe fn motion_events(
    clock: &mut effects::MotionClock,
    actor: u32,
    slot: Slot,
    trail: &mut crate::trail::History,
    clip: u16,
    sample: mhgu_sdk::style::action::MotionFrame,
) {
    const ROWS: &[[u8; 18]] = include_bytes!("../target/assets/effects/motion_fx.bin")
        .as_chunks::<18>()
        .0;
    for event in clock.advance(ROWS, clip, sample) {
        unsafe { motion_event(actor, slot, trail, clip, event) };
    }
}

unsafe fn motion_event(
    actor: u32,
    slot: Slot,
    trail: &mut crate::trail::History,
    clip: u16,
    event: MotionEvent,
) {
    unsafe {
        let MotionEvent {
            kind,
            bank,
            selector,
            node,
            mode,
            offset,
            ..
        } = event;
        let dash_slash_burst = (clip, kind, bank, selector) == (1439, 5, 4035, 2);
        // A native transition can still sample the outgoing clip.
        if dash_slash_burst && action::current(actor) != (action::GROUP, action::slot(72)) {
            return;
        }
        if kind == 0 {
            trail.swing(slot, actor, 2. * (f32::from(selector) + 2.));
            return;
        }
        let yaw = read::<u16>(actor, 4076);
        // w07 nodes 5, 8 and 10 map to the left leg, right leg and weapon-side hand.
        let joint_id = match node {
            0 => None,
            5 => Some(16),
            8 => Some(19),
            10 => Some(2),
            _ => return,
        };
        if kind == 3 {
            if let Some(parent) = particle::parent(actor) {
                PARTICLES.emit(
                    15 + usize::from(selector),
                    Anchor::JointPose {
                        parent,
                        joint: joint_id.map_or(-1, |id| id as i16),
                        offset: Some(offset),
                    },
                );
            }
            return;
        }
        let (mut position, rotation) = if let Some(joint_id) = joint_id {
            let Some(m) = bone(actor, joint_id) else {
                return;
            };
            (
                particle::transform_offset(&m, offset),
                if mode == 0 {
                    effects::world_pose_angles(&m)
                } else {
                    [0; 3]
                },
            )
        } else {
            let (position, _) = particle::world_offset(actor, offset);
            (
                position,
                [
                    0,
                    if mode == 4 {
                        yaw.wrapping_add(0x4000)
                    } else {
                        yaw
                    },
                    0,
                ],
            )
        };
        let mut wet = false;
        if matches!(kind, 1 | 5) {
            if let Some((floor, water)) = particle::ground_heights(actor, position) {
                position[1] = water.unwrap_or(floor);
                wet = water.is_some();
            } else if kind == 1 {
                return;
            }
            // 8170469E keeps the input node height when the terrain probe misses.
        }
        let route = match kind {
            1 if wet => {
                position[1] += 5.;
                75
            }
            1 if bank == 7 => 77,
            1 => 76,
            5 => match (selector, wet) {
                (2, false) => 59,
                (6, false) => 60,
                (2, true) => 61,
                (6, true) => 62,
                (11, false) => 72,
                (13, false) => 73,
                (14, false) => 74,
                (11, true) => 80,
                (13, true) => 81,
                (14, true) => 82,
                _ => return,
            },
            6 if bank == 4035 => 59,
            6 => 15 + usize::from(selector),
            _ => return,
        };
        let anchor = Anchor::RotatedWorld(position, rotation);
        if dash_slash_burst {
            DASH_SLASH_UNITS[slot.0].emit_layers(&PARTICLES, route, anchor, u32::MAX, 0);
        } else {
            PARTICLES.emit(route, anchor);
        }
    }
}

pub(super) unsafe fn trail(actor: u32, channel: usize) {
    unsafe {
        PARTICLES.emit(
            [35, 63, 64, 65, 66, 67, 68, 69, 70, 71][channel],
            Anchor::World(read(actor, 64), 0),
        );
    }
}

static IMPACT: mhfz_sdk::sync::Shared<Option<(u32, [f32; 3])>> = mhfz_sdk::sync::Shared::new(None);

pub(super) fn clear_impact() {
    IMPACT.with(|impact| *impact = None);
}

pub(super) unsafe fn remember_impact(actor: u32) {
    if let Some(pos) = unsafe { mhgu_sdk::style::guard::impact_position(actor) } {
        IMPACT.with(|impact| *impact = Some((actor, pos)));
    }
}

pub(super) unsafe fn parry(actor: u32) {
    unsafe {
        let pos = IMPACT
            .with(|impact| {
                if impact.is_some_and(|(owner, _)| owner == actor) {
                    impact.take()
                } else {
                    None
                }
            })
            .map(|(_, point)| point)
            .or_else(|| bone(actor, 0).map(|m| [m[12], m[13], m[14]]))
            .unwrap_or_else(|| read(actor, 64));
        world(actor, pos, 52);
        for start in [53, 56] {
            world(actor, pos, start + (particle::random_u32() % 3) as usize);
        }
    }
}

pub(super) unsafe fn spirit_flash(actor: u32, blue: bool) {
    unsafe {
        let first = if blue { 44 } else { 36 };
        mhfz_sdk::effects::sword_flash(&PARTICLES, actor, first);
        crate::sound::weapon(actor, 12);
    }
}

pub(super) unsafe fn spirit_full(actor: u32) {
    unsafe {
        if let Some(parent) = particle::parent(actor) {
            let anchor = Anchor::Joint {
                parent,
                joint: 2,
                offset: Some([45., 10., 32.]),
            };
            PARTICLES.emit(78, anchor);
            PARTICLES.emit(79, anchor);
        }
    }
}
