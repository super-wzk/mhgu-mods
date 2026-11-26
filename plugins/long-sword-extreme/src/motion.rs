use mhfz_sdk::motion::{DrawnRoll, MotionBanks, sprint_sheathe};
use mhgu_sdk::weapon::style::{PASS, action};

// FZ common action 202, weapon row in 825901F4.
const SPRINT_SHEATHE: u16 = 1412;
pub(super) static SHEATHE_MOTIONS: mhgu_sdk::weapon::style::remote::Sheathe =
    mhfz_sdk::motion::sheathe_motions(&[crate::program::SPRINT], SPRINT_SHEATHE);

static BANKS: MotionBanks = MotionBanks::new([
    c"player\\mot\\frontier_ls\\0",
    c"player\\mot\\frontier_ls\\2",
    c"player\\mot\\frontier_ls\\4",
]);

pub(super) unsafe extern "C" fn preload(_actor: u32) {
    unsafe {
        BANKS.preload_for_hunt(1415);
        crate::collision::preload();
        crate::sound::preload();
        crate::fx::preload();
        crate::hud::preload();
    }
}

pub(super) unsafe fn source_clip(actor: u32, packed: u16) -> Option<u16> {
    unsafe { BANKS.decode(actor, packed) }
}

const PRIVATE: u32 = 0x1_0000;

// FZ 817E386C selects w07's lateral clips for common actions 50 and 49.
pub(super) static ROLL: DrawnRoll = DrawnRoll::new(1006, 1007);

const LOCOMOTION: &[(u32, u16)] = &[
    (5001, 1001),
    (5002, 1002),
    (5003, 1003),
    (5004, 1004),
    (5012, 1006),
    (5013, 1007),
];

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let source = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
    } else if unsafe { sprint_sheathe(actor, request, &SHEATHE_MOTIONS) } {
        SPRINT_SHEATHE
    } else if request == 28 {
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
    LOCOMOTION
        .iter()
        .find(|&&(_, clip)| clip == source)
        .map_or(32000 + u32::from(source - 1000), |&(id, _)| id)
}

pub(super) extern "C" fn finish_sheathe(slot: u16, phase: u8, motion: u32) -> bool {
    matches!((slot, phase, motion), (2, 1, 5003))
}

pub unsafe fn play(actor: u32, clip: u16, start: u16, blend: u16) -> Option<u16> {
    unsafe {
        action::play_motion(
            actor,
            PRIVATE | u32::from(clip),
            f32::from(start),
            f32::from(blend / 2),
        )
    }
}
