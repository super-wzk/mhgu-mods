//! Frontier blade and shield capsules, and family 87/1's separately moving sphere.
use crate::{battle, fx, native, program, read, sound};
use core::cell::UnsafeCell;
use mhfz_sdk::combat::HitWindow;
use mhgu_sdk::{
    combat::collision::{CachedHitShape, ContactPool},
    geometry::Angle,
    style::{action::Attack, particle, remote::PLAYER_COUNT},
};

const SPRINT_SLASH_FINISH_HIT: u16 = 221;

static SHAPES: [CachedHitShape; 5] = [
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_6"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_7"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_23"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_28"),
    CachedHitShape::new(cr"player\hit\frontier_sns\blade_29"),
];
pub unsafe fn preload() {
    for shape in &SHAPES {
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
            (if shield { 3 } else { 1 }, flags)
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
        if shield {
            mhgu_sdk::style::action::stun::submit(actor, &attack, row.stun);
        } else {
            attack.submit(actor);
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
}
struct Slot(UnsafeCell<Option<Wave>>);
unsafe impl Sync for Slot {}
static WAVES: [Slot; 1 + PLAYER_COUNT] = [const { Slot(UnsafeCell::new(None)) }; 1 + PLAYER_COUNT];
pub fn reset(index: usize) {
    unsafe {
        if let Some(w) = (&mut *WAVES[index].0.get()).as_mut() {
            w.owner = 0;
            for c in w.pool.iter() {
                c.disable();
            }
        }
    }
}

pub unsafe fn launch(actor: u32, index: usize) {
    unsafe {
        let (origin, yaw) = particle::world_offset(actor, [0., program::WAVE_HEIGHT, 0.]);
        fx::vacuum(origin, yaw);
        sound::blade(actor, 22);
        let template = native!(0x2a8a88, actor, 1);
        let Some(shape) = SHAPES[0].load() else {
            return;
        };
        if template == 0 {
            return;
        }
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        let w = (&mut *WAVES[index].0.get()).get_or_insert_with(|| Wave {
            pool: ContactPool::new(1),
            owner: 0,
            area: 0,
            origin,
            direction: [0.; 2],
            center: origin,
            age: 0.,
        });
        w.owner = actor;
        w.area = read(actor, 4180);
        w.origin = origin;
        w.direction = [sin, cos];
        w.age = 0.;
        w.center = origin;
        let mut hdp: [u32; 14] = read(template, 0);
        hdp[0] = u32::from(program::WAVE_DURATION) * 2 << 16;
        hdp[1] = hdp[1] & 0xffff0000 | u32::from(program::WAVE_POWER);
        hdp[8] &= !255;
        hdp[9] &= !(255 << 8);
        w.pool
            .iter()
            .next()
            .unwrap()
            .prepare_world_attack(actor, &hdp, shape.as_raw());
    }
}

pub unsafe extern "C" fn submit_waves(manager: u32) {
    unsafe {
        for (index, slot) in WAVES.iter().enumerate() {
            let Some(w) = (&mut *slot.0.get()).as_mut() else {
                continue;
            };
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
            let travel = (w.age * 0.5) as u32 as f32 * program::WAVE_SPEED;
            w.center = [
                w.origin[0] + travel * w.direction[0],
                w.origin[1],
                w.origin[2] + travel * w.direction[1],
            ];
            w.pool.iter().next().unwrap().enqueue(manager);
            w.age += dt;
        }
    }
}
pub unsafe extern "C" fn expand(owner: u32, hit: u32) -> u32 {
    unsafe {
        for slot in &WAVES {
            if let Some(w) = (&*slot.0.get()).as_ref()
                && w.owner == owner
                && let Some(c) = w.pool.iter().find(|c| c.as_raw() == hit)
            {
                // FZ 82C7CFF0[1] selects the radius-200 sphere at 8279B330.
                return u32::from(c.submit_world_sphere(w.center, 200.));
            }
        }
    }
    0
}
