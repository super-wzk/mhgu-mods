use crate::{program::JUMP_SLASH, read, write};
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhfz_sdk::motion::{DrawnRoll, MotionBanks, sprint_sheathe};
use mhgu_sdk::weapon::style::{PASS, action};

static BANKS: MotionBanks = MotionBanks::new([
    c"player\\mot\\frontier_sns\\0",
    c"player\\mot\\frontier_sns\\2",
    c"player\\mot\\frontier_sns\\4",
]);

pub(super) unsafe extern "C" fn preload(_actor: u32) {
    unsafe {
        BANKS.preload_for_hunt(1401);
        crate::collision::preload();
        crate::sound::preload();
        crate::fx::preload();
    }
}

pub(super) unsafe fn source_clip(actor: u32, packed: u16) -> Option<u16> {
    unsafe { BANKS.decode(actor, packed) }
}

const SPRINT_SHEATHE: u16 = 1450;
pub(super) static SHEATHE_MOTIONS: mhgu_sdk::weapon::style::remote::Sheathe =
    mhfz_sdk::motion::sheathe_motions(&[crate::program::SPRINT], SPRINT_SHEATHE);

const PRIVATE: u32 = 0x1_0000;
const JUMP_TRAVEL_SCALE: f32 = 0.5;

pub(super) unsafe fn damp_jump_momentum(actor: u32) {
    // 539F4C integrates these horizontal velocity and acceleration components.
    for offset in [4112, 4120, 4128, 4136] {
        unsafe {
            write(
                actor,
                offset,
                read::<f32>(actor, offset) * JUMP_TRAVEL_SCALE,
            )
        };
    }
}

#[forge::hook(offset = 0x281ef8)]
unsafe fn update_hunter_motion(actor: u32) {
    unsafe {
        if action::current(actor) != (action::GROUP, action::slot(JUMP_SLASH))
            || read::<u8>(actor, 5) == 0
            || source_clip(actor, read(actor, 1204)) != Some(1447)
        {
            original!(actor);
            return;
        }
        // 281EF8 applies root motion in 951168's layer update and in 53A5BC
        // during blending. Flight velocity is integrated separately by 539F4C.
        let before: [f32; 3] = read(actor, 64);
        original!(actor);
        for axis in [0, 2] {
            let offset = 64 + 4 * axis as u32;
            let delta = read::<f32>(actor, offset) - before[axis];
            write(actor, offset, before[axis] + delta * JUMP_TRAVEL_SCALE);
        }
    }
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), update_hunter_motion);
}

// FZ 817E386C selects w04's lateral clips for common actions 50 and 49.
pub(super) static ROLL: DrawnRoll = DrawnRoll::new(1420, 1421);

static FRONT_STEP: AtomicU32 = AtomicU32::new(0);

pub unsafe fn evade(actor: u32, yaw: Option<u16>) -> bool {
    // 811EAD82 resolves ordinary B before GU enters the roll. Keep w04's
    // exact four-sector boundaries when saving the lateral clip.
    let sector = yaw.map(|angle| angle.wrapping_add(0x2000) >> 14);
    FRONT_STEP.store(if sector == Some(0) { actor } else { 0 }, Relaxed);
    let roll_angle = match sector {
        Some(1) => 0x4000,
        Some(3) => 0xc000,
        _ => 0,
    };
    let accepted = unsafe { ROLL.enter(actor, roll_angle) };
    if !accepted {
        clear_roll(actor);
    }
    accepted
}

pub fn clear_roll(actor: u32) {
    ROLL.clear_for(actor);
    let _ = FRONT_STEP.compare_exchange(actor, 0, Relaxed, Relaxed);
}

const LOCOMOTION: &[(u32, u16)] = &[
    (5001, 1001),
    (5002, 1002),
    (5003, 1003),
    (5004, 1004),
    (5012, 1420),
    (5013, 1421),
];

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let source = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
    } else if unsafe { sprint_sheathe(actor, request, &SHEATHE_MOTIONS) } {
        SPRINT_SHEATHE
    } else if request == 28 {
        if FRONT_STEP.load(Relaxed) == actor && unsafe { action::current(actor) } == (1, 6) {
            return unsafe { BANKS.encode(actor, 1419) }.map_or(PASS, u32::from);
        }
        let Some(clip) = (unsafe { ROLL.clip(actor, || None) }) else {
            return PASS;
        };
        clip
    } else {
        let Some(&(_, source)) = LOCOMOTION.iter().find(|&&(id, _)| id == request) else {
            return PASS;
        };
        source
    };
    unsafe { BANKS.encode(actor, source) }.map_or(PASS, u32::from)
}

pub unsafe extern "C" fn decode(actor: u32, packed: u32) -> u32 {
    let Some(source) = u16::try_from(packed)
        .ok()
        .and_then(|packed| unsafe { BANKS.decode(actor, packed) })
    else {
        return PASS;
    };
    if source == SPRINT_SHEATHE {
        return 5003;
    }
    if source == 1419 {
        return 28;
    }
    LOCOMOTION
        .iter()
        .find(|&&(_, clip)| clip == source)
        .map_or(32000 + u32::from(source - 1000), |&(id, _)| id)
}

pub unsafe fn play(actor: u32, clip: u16, start: u16, blend: u16) -> Option<u16> {
    unsafe { mhfz_sdk::motion::play_step(actor, PRIVATE | u32::from(clip), start, blend) }
}
