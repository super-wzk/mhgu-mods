//! Frontier blade and shield capsules, and family 87/1's separately moving sphere.
use crate::{battle, fx, native, program, read, sound, write};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use mhfz_sdk::combat::{HitWindow, attacker_hitstop};
use mhgu_sdk::{
    combat::collision::{CachedHitShape, ContactPool},
    geometry::Angle,
    weapon::style::{action::Attack, particle, remote::PLAYER_COUNT},
};

const SPRINT_SLASH_FINISH_HIT: u16 = 221;
static RING_SHAPE: CachedHitShape = CachedHitShape::new(cr"player\hit\frontier_sns\round_force");

static SHAPES: [CachedHitShape; 5] = [
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_6"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_7"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_23"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_28"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_29"),
];
pub unsafe fn preload() {
    for shape in SHAPES.iter().chain([&RING_SHAPE]) {
        unsafe {
            shape.preload();
        }
    }
}
pub unsafe fn strike(actor: u32, state: u8, id: u16, frame: f32, late: f32, deflect: bool) {
    let Some(row) = program::strike(id) else {
        return;
    };
    let Some(index) = [6, 7, 23, 28, 29].iter().position(|&s| s == row.shape) else {
        return;
    };
    let Some(window) = HitWindow::new(row.start, row.duration, frame, late) else {
        return;
    };
    unsafe {
        let Some(shape) = SHAPES[index].load() else {
            return;
        };
        let shield = index != 0;
        // Only the aerial finisher uses GU 116AB98's HDP 14 and flags-table
        // entry 3 (162891C[3] = 0x8007) for mounting.
        let (template, flags) = if state == program::JUMP_SLASH && id == SPRINT_SLASH_FINISH_HIT {
            (14, 0x8007)
        } else {
            let flags = if id == 179 {
                // 1437 opens the blade and shield windows together.
                Attack::SECONDARY
            } else if deflect {
                Attack::DEFLECTABLE
            } else {
                0
            };
            if shield {
                (3, flags)
            } else {
                let draw_ko = if state == program::DRAW {
                    Attack::PUNISHING_DRAW
                } else {
                    0
                };
                (1, flags | Attack::WEAPON_MODIFIERS | draw_ko)
            }
        };
        let attack = Attack {
            template,
            power: row.power,
            delay: window.delay,
            duration: window.duration,
            flags,
            shape: shape.as_raw(),
            no_sharpness_cost: u32::from(shield),
        };
        let hitstop = attacker_hitstop(row.source_hitstop);
        if shield {
            mhgu_sdk::weapon::style::action::stun::submit_with_hitstop(actor, &attack, row.stun, hitstop);
        } else {
            attack.submit_with_hitstop(actor, hitstop);
        }
    }
}

struct Wave {
    pool: ContactPool,
    owner: u32,
    area: u8,
    origin: [f32; 3],
    direction: [f32; 2],
    center: [f32; 3],
    age: f32,
    ring_reach: Option<f32>,
    ring_radius: f32,
}
struct Slot(UnsafeCell<Vec<Wave>>);
unsafe impl Sync for Slot {}
static WAVES: [Slot; 1 + PLAYER_COUNT] =
    [const { Slot(UnsafeCell::new(Vec::new())) }; 1 + PLAYER_COUNT];
pub fn reset(index: usize) {
    unsafe {
        for w in &mut *WAVES[index].0.get() {
            w.owner = 0;
            for c in w.pool.iter() {
                c.disable();
            }
        }
    }
}

pub unsafe fn launch(actor: u32, index: usize, power: u16) {
    unsafe { spawn(actor, index, power, None) }
}

pub unsafe fn launch_art(actor: u32, index: usize, tier: u32, power: u16) {
    // Retail w01 HTS groups 10..12 have their furthest endpoints on the
    // diagonals (±d, ±d), each with capsule radius 125.
    if let Some(diagonal) = [175., 200., 300.].get(tier as usize) {
        let reach = (diagonal * core::f32::consts::SQRT_2 + 125.) * 1.15;
        unsafe { spawn(actor, index, power, Some(reach)) };
    }
}

unsafe fn spawn(actor: u32, index: usize, power: u16, ring_reach: Option<f32>) {
    unsafe {
        let (origin, yaw) = particle::world_offset(actor, [0., program::WAVE_HEIGHT, 0.]);
        let shape = if let Some(reach) = ring_reach {
            fx::round_force(origin, reach);
            &RING_SHAPE
        } else {
            fx::vacuum(origin, yaw);
            &SHAPES[0]
        };
        sound::blade(actor, 22);
        let template = native!(0x2a8a88, actor, 1);
        let Some(shape) = shape.load() else {
            return;
        };
        if template == 0 {
            return;
        }
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        // Round Force can start during the previous spin wave's flight.
        let waves = &mut *WAVES[index].0.get();
        let w = if let Some(w) = waves.iter().position(|w| w.owner == 0) {
            &mut waves[w]
        } else {
            waves.push(Wave {
                pool: ContactPool::new(1),
                owner: 0,
                area: 0,
                origin,
                direction: [0.; 2],
                center: origin,
                age: 0.,
                ring_reach,
                ring_radius: 0.,
            });
            waves.last_mut().unwrap()
        };
        w.owner = actor;
        w.area = read(actor, 4180);
        w.origin = origin;
        w.direction = [sin, cos];
        w.age = 0.;
        w.center = origin;
        w.ring_reach = ring_reach;
        let mut hdp: [u32; 14] = read(template, 0);
        hdp[0] = u32::from(program::WAVE_DURATION) * 2 << 16;
        hdp[1] = hdp[1] & 0xffff0000 | u32::from(power);
        hdp[8] &= !255;
        hdp[9] &= !(255 << 8);
        let contact = w.pool.iter().next().unwrap();
        contact.prepare_world_attack(actor, &hdp, shape.as_raw());
        let hit = contact.as_raw();
        // 2B76CC has applied the oils and rolled this wave's weapon status.
        write(hit, 86, read::<u8>(hit, 86) / 2); // KO
        write(hit, 89, read::<u8>(hit, 89) / 2); // exhaust
        // 2ACFB8 uses +80 for either element damage or status buildup.
        write(hit, 80, read::<u16>(hit, 80) / 2);
    }
}

pub unsafe extern "C" fn submit_waves(manager: u32) {
    unsafe {
        for (index, slot) in WAVES.iter().enumerate() {
            for w in &mut *slot.0.get() {
                let a = w.owner;
                if a == 0 {
                    continue;
                }
                if !battle::live(index, a)
                    || read::<u8>(a, 4180) != w.area
                    || read::<i16>(a, 5078) <= 0
                    || w.age >= 2. * f32::from(program::WAVE_DURATION)
                {
                    w.owner = 0;
                    for c in w.pool.iter() {
                        c.disable();
                    }
                    continue;
                }
                let clock = read::<u32>(forge::mem::text_addr(), 0x2126764);
                if clock == 0 {
                    continue;
                }
                let dt = read::<f32>(clock, 104);
                if dt <= 0. {
                    continue;
                }
                if let Some(reach) = w.ring_reach {
                    // Match mesh 175's scale keys in the round-force EFL.
                    w.ring_radius = reach * (0.10 + 0.78 * (w.age / 30.).min(1.));
                } else {
                    let travel = (w.age * 0.5) as u32 as f32 * program::WAVE_SPEED;
                    w.center = [
                        w.origin[0] + travel * w.direction[0],
                        w.origin[1],
                        w.origin[2] + travel * w.direction[1],
                    ];
                }
                w.pool.iter().next().unwrap().enqueue(manager);
                w.age += dt;
            }
        }
    }
}
pub unsafe extern "C" fn expand(owner: u32, hit: u32) -> u32 {
    unsafe {
        for slot in &WAVES {
            for w in &*slot.0.get() {
                if w.owner == owner
                    && let Some(c) = w.pool.iter().find(|c| c.as_raw() == hit)
                {
                    if let Some(reach) = w.ring_reach {
                        let [x, y, z] = w.origin;
                        let vertices: [_; 8] = core::array::from_fn(|i| {
                            let (sin, cos) = Angle::from_units((i as u16) << 13).sin_cos();
                            [x + sin * w.ring_radius, y, z + cos * w.ring_radius]
                        });
                        let edges: [_; 8] =
                            core::array::from_fn(|i| [vertices[i], vertices[(i + 1) % 8]]);
                        return c.submit_world_capsules(&edges, 0.12 * reach);
                    }
                    // FZ 82C7CFF0[1] selects the radius-200 sphere at 8279B330.
                    return u32::from(c.submit_world_sphere(w.center, 200.));
                }
            }
        }
    }
    0
}
