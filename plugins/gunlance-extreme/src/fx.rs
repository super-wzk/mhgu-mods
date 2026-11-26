use crate::{native, read};
use mhfz_sdk::effects::{MotionClock, world_pose_angles};
use mhgu_sdk::weapon::style::{
    action::MotionFrame,
    particle::{self, Anchor, Bank, TrackedUnits},
    remote::PLAYER_COUNT,
};

static BANK: Bank = Bank::new(
    cr"effect\frontier_gl\particles",
    include_bytes!("../target/assets/effects/routes.bin"),
);
const TRACKS: &[[u8; 18]] = include_bytes!("../target/assets/effects/motion_fx.bin")
    .as_chunks()
    .0;
const BARREL: &[u16] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 11, 13, 14, 15, 16, 17, 18, 22, 23, 24, 25,
];
const GROUND: &[u16] = &[2, 4, 11, 16];
const FLASH: usize = BARREL.len() + 2 * GROUND.len() + 3;
const BLADE: usize = FLASH + 8;
const CASING: usize = BLADE + 12;
const WINDUP: usize = CASING + 1;
const CHAMBER: usize = CASING + 2;
const VAPOUR: usize = CASING + 3;
const STEAM: usize = CASING + 4;
const UPWARD_SHELL: usize = STEAM + 1;
const CORE_LAYERS: usize = 2;
const FLAME_LAYERS: usize = CORE_LAYERS + 7;
const BARREL_ATTACHMENTS: u32 = 0x188ff10;
const BARREL_ATTACHMENT_COUNT: u32 = 26;
// Vita 81FBE17C's [128, 25, 201] and the accessory origin [105, 15, 167],
// expressed in the blade basis of 81FBE3D4 (pitch -5 degrees, yaw 0.546288).
const FZ_BLADE: [f32; 3] = [4.947385, 4.140475, 239.51697];
const FZ_ACCESSORY: [f32; 3] = [2.958482, -2.248039, 197.80089];
static FLAMES: TrackedUnits<{ FLAME_LAYERS * (1 + PLAYER_COUNT) }> = TrackedUnits::new();
const BARREL_LAYERS: usize = 4;
static MUZZLES: TrackedUnits<{ BARREL_LAYERS * (1 + PLAYER_COUNT) }> = TrackedUnits::new();

pub fn install() {
    extern "C" fn released(unit: u32) {
        FLAMES.forget(unit);
        MUZZLES.forget(unit);
    }
    FLAMES.bind(crate::STYLE_ID, released);
    MUZZLES.bind(crate::STYLE_ID, released);
}

#[derive(Default)]
pub struct Muzzle {
    owner: Option<usize>,
    pending: u32,
    route: usize,
}

impl Muzzle {
    pub fn stop(&mut self) {
        if let Some(owner) = self.owner.take() {
            let first = owner * BARREL_LAYERS;
            unsafe { MUZZLES.stop_slots(first..first + BARREL_LAYERS) };
        }
    }

    pub fn start_charge(&mut self, owner: usize) {
        self.start(owner, WINDUP);
    }

    pub fn start_steam(&mut self, owner: usize) {
        self.start(owner, STEAM);
    }

    fn start(&mut self, owner: usize, route: usize) {
        self.stop();
        self.owner = Some(owner);
        self.route = route;
        self.pending = (1 << BANK.layer_count(route).unwrap_or(0)) - 1;
    }

    pub unsafe fn update(&mut self, actor: u32) {
        if let Some(owner) = self.owner
            && self.pending != 0
            && let Some(anchor) = unsafe { muzzle(actor) }
        {
            self.pending = unsafe {
                MUZZLES.emit_layers(
                    &BANK,
                    self.route,
                    anchor,
                    self.pending,
                    owner * BARREL_LAYERS,
                )
            };
        }
    }
}

impl Drop for Muzzle {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn muzzle(actor: u32) -> Option<Anchor> {
    unsafe {
        let barrel = Barrel::equipped(actor)?;
        // GU 11B0C38 uses the third w09 equipment parameter for the muzzle
        // position. Convert that model-space point to the live barrel joint.
        let variant = barrel.variant(2);
        if variant >= 4 {
            return None;
        }
        let point = read(forge::mem::text_addr(), 0x1890250 + 16 * variant);
        Some(barrel.anchor(point))
    }
}

pub unsafe fn chamber(actor: u32, inserted: bool) {
    if let Some(anchor) = unsafe { muzzle(actor) } {
        unsafe { BANK.emit(if inserted { CHAMBER } else { VAPOUR }, anchor) };
    }
}

pub unsafe fn muzzle_flash(actor: u32, large: bool) {
    if let Some(anchor) = unsafe { muzzle(actor) } {
        unsafe { BANK.emit(UPWARD_SHELL + usize::from(large), anchor) };
    }
}

pub unsafe fn casings(actor: u32, count: u8) {
    if count == 0 {
        return;
    }
    unsafe {
        let Some(matrix) = particle::joint_matrix(actor, 8) else {
            return;
        };
        let pose = read(matrix, 0);
        let angles = world_pose_angles(&pose);
        for index in 0..count {
            let offset = [
                12. + 8. * f32::from(index % 3),
                8.,
                42. + 7. * f32::from(index),
            ];
            BANK.emit(
                CASING,
                Anchor::RotatedWorld(particle::transform_offset(&pose, offset), angles),
            );
        }
    }
}

#[derive(Default)]
pub struct Flame {
    owner: Option<usize>,
    core_route: usize,
    pending: [u32; 3],
    remaining: f32,
}

struct Barrel {
    weapon: u32,
    data: u32,
    parent: u32,
    joint: i16,
    bind: [f32; 16],
    min: [f32; 3],
    span: f32,
    tip: f32,
}

impl Barrel {
    unsafe fn variant(&self, field: u32) -> u32 {
        unsafe {
            native!(
                0x13f614,
                self.data,
                u32::from(crate::WEAPON),
                read(self.weapon, 4992),
                field
            )
        }
    }

    fn anchor(&self, point: [f32; 3]) -> Anchor {
        let position = core::array::from_fn(|axis| (point[axis] - self.min[axis]) / self.span);
        Anchor::JointPose {
            parent: self.parent,
            joint: self.joint,
            offset: Some(particle::transform_offset(&self.bind, position)),
        }
    }

    unsafe fn equipped(actor: u32) -> Option<Self> {
        unsafe {
            let weapon = read(actor, 9120);
            let parent = particle::parent(weapon)?;
            let model: u32 = read(weapon, 240);
            if model == 0 {
                return None;
            }
            let base = forge::mem::text_addr();
            let data: u32 = read(base, 0x1856b18);
            if data == 0 {
                return None;
            }
            // GU 455EB8 selects the animated barrel attachment for each weapon variant.
            let variant = native!(
                0x13f614,
                data,
                u32::from(crate::WEAPON),
                read(weapon, 4992),
                1
            );
            if variant >= BARREL_ATTACHMENT_COUNT {
                return None;
            }
            let joint: i16 = read(base, BARREL_ATTACHMENTS + 32 * variant);
            let logical = u8::try_from(joint).ok()?;
            let remap = read(weapon, 1176);
            let bone = u32::from(read::<u8>(remap, u32::from(logical)));
            let inverse_binds: u32 = read(model, 112);
            if bone == 255 || bone >= read(model, 104) || inverse_binds == 0 {
                return None;
            }
            // B490CC loads the MOD's quantization span, bounds and inverse binds.
            // The inverse bind expects normalized mesh coordinates, not model units.
            let span: f32 = read(model, 168);
            let min: [f32; 3] = read(model, 192);
            let tip: f32 = read(model, 216);
            if !span.is_finite() || span <= 0. || !tip.is_finite() || tip <= 0. {
                return None;
            }
            Some(Self {
                weapon,
                data,
                parent,
                joint,
                bind: read(inverse_binds, 64 * bone),
                min,
                span,
                tip,
            })
        }
    }
}

unsafe fn blade_anchors(actor: u32, fire: bool) -> Option<[Anchor; 3]> {
    unsafe {
        let barrel = Barrel::equipped(actor)?;
        // 827BF144 uses separate main and halo offsets for non-fire elements.
        // Map the source forward reference to the GU model's tip.
        let [main, halo] = if fire { [1., 1.] } else { [0.86, 0.98] };
        let factor = barrel.tip / FZ_BLADE[2];
        Some(
            [(FZ_BLADE, main), (FZ_BLADE, halo), (FZ_ACCESSORY, 1.)]
                .map(|(point, ratio)| barrel.anchor(point.map(|v| v * ratio * factor))),
        )
    }
}

impl Flame {
    pub fn stop(&mut self) {
        if let Some(owner) = self.owner.take() {
            let first = FLAME_LAYERS * owner;
            unsafe { FLAMES.stop_slots(first..first + FLAME_LAYERS) };
        }
    }

    pub unsafe fn start(&mut self, actor: u32, owner: usize) {
        self.stop();
        self.owner = Some(owner);
        self.remaining = 100.; // Type 31/4 lives for 50 FZ logic ticks.
        unsafe {
            // GU 2ACFB8 reads primary element from property 17 and strength from 2.
            let element = native!(0x27c708, actor, 17);
            let row = if (1..=5).contains(&element) && native!(0x27c708, actor, 2) as i32 > 0 {
                element as usize - 1
            } else {
                5 // FZ 81202256 selects row 7 for a weapon without an element.
            };
            self.core_route = BLADE + 2 * row;
            let accessory_count = BANK.layer_count(self.core_route + 1).unwrap_or(0);
            self.pending = [0b01, 0b10, (1 << accessory_count) - 1];
            self.update(actor, 0.);
        }
    }

    pub unsafe fn update(&mut self, actor: u32, elapsed: f32) {
        let Some(owner) = self.owner else { return };
        self.remaining -= elapsed;
        if self.remaining <= 0. {
            self.stop();
            return;
        }
        if self.pending == [0; 3] {
            return;
        }
        unsafe {
            let Some([main, halo, accessories]) = blade_anchors(actor, self.core_route == BLADE)
            else {
                return;
            };
            // Main and halo share a PEL route but use separate attachment offsets.
            let first_slot = FLAME_LAYERS * owner;
            let layers = [
                (self.core_route, first_slot, main),
                (self.core_route, first_slot, halo),
                (self.core_route + 1, first_slot + CORE_LAYERS, accessories),
            ];
            for ((route, slot, anchor), pending) in layers.into_iter().zip(&mut self.pending) {
                if *pending != 0 {
                    *pending = FLAMES.emit_layers(&BANK, route, anchor, *pending, slot);
                }
            }
        }
    }
}

impl Drop for Flame {
    fn drop(&mut self) {
        self.stop();
    }
}

pub unsafe fn ignition(actor: u32) {
    unsafe { mhfz_sdk::effects::sword_flash(&BANK, actor, FLASH) };
}

pub unsafe fn preload() {
    unsafe { BANK.preload() };
}

fn route(bank: u16, selector: u16) -> Option<usize> {
    let (first, ids) = match bank {
        4027 => (0, BARREL),
        4035 => (BARREL.len(), GROUND),
        4036 => (BARREL.len() + GROUND.len(), GROUND),
        _ => return None,
    };
    ids.iter().position(|&id| id == selector).map(|i| first + i)
}

pub unsafe fn update(clock: &mut MotionClock, actor: u32, clip: u16, sample: MotionFrame) {
    unsafe {
        for event in clock.advance(TRACKS, clip, sample) {
            let joint = match event.node {
                0 => -1,
                2 => 0,
                5 => 16,
                8 => 19,
                10 => 2,
                14 => 8,
                21 => 0,
                18 => 12,
                _ => continue,
            };
            if matches!(event.kind, 3 | 7) && event.mode == 0 {
                if let (Some(parent), Some(route)) = (
                    particle::parent(if event.node == 21 {
                        read(actor, 9120)
                    } else {
                        actor
                    }),
                    route(event.bank, event.selector),
                ) {
                    BANK.emit(
                        route,
                        Anchor::JointPose {
                            parent,
                            joint,
                            offset: Some(event.offset),
                        },
                    );
                }
                continue;
            }
            let (mut pos, yaw) = particle::world_offset(actor, event.offset);
            let mut angles = [0, yaw, 0];
            if joint >= 0 {
                let Some(matrix) = particle::joint_matrix(actor, joint as u32) else {
                    continue;
                };
                let pose: [f32; 16] = read(matrix, 0);
                pos = particle::transform_offset(&pose, event.offset);
                if matches!(event.mode, 0 | 2) {
                    angles = world_pose_angles(&pose);
                }
            }
            let mut wet = false;
            if matches!(event.kind, 1 | 5) {
                if let Some((floor, water)) = particle::ground_heights(actor, pos) {
                    wet = water.is_some();
                    pos[1] = water.unwrap_or(floor);
                } else if event.kind == 1 {
                    continue;
                }
            }
            let index = if event.kind == 1 {
                let foot = BARREL.len() + 2 * GROUND.len();
                if wet {
                    pos[1] += 5.;
                    foot
                } else if event.bank == 7 {
                    foot + 2
                } else {
                    foot + 1
                }
            } else if let Some(index) = route(
                event.bank + u16::from(wet && event.bank == 4035),
                event.selector,
            ) {
                index
            } else {
                continue;
            };
            if event.mode == 4 {
                angles[1] = yaw.wrapping_add(0x4000);
            } else if event.mode == 5 {
                // 817BD21C leaves the rotation callback empty in mode 5.
                angles = [0; 3];
            }
            BANK.emit(index, Anchor::RotatedWorld(pos, angles));
        }
    }
}
