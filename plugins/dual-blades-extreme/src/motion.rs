use crate::read;
use mhfz_sdk::motion::{DrawnRoll, MotionBanks};
use mhgu_sdk::style::{PASS, action, movement::relative_stick_yaw};

static BANKS: MotionBanks = MotionBanks::new([
    c"player\\mot\\frontier_db\\0",
    c"player\\mot\\frontier_db\\2",
    c"player\\mot\\frontier_db\\4",
]);

pub(super) unsafe extern "C" fn preload(_actor: u32) {
    unsafe {
        BANKS.preload_for_hunt(1413);
        crate::sound::preload();
        crate::fx::preload();
        crate::hud::preload();
    }
}

pub(super) unsafe fn source_clip(actor: u32, packed: u16) -> Option<u16> {
    unsafe { BANKS.decode(actor, packed) }
}

const PRIVATE: u32 = 0x1_0000;

const LOCOMOTION: &[(u32, u16)] = &[
    (5001, 1001),
    (5002, 1002),
    (5003, 1003),
    (5004, 1004),
    (5012, 1012),
    (5013, 1013),
];

pub(super) static ROLL: DrawnRoll = DrawnRoll::new(1012, 1013);

pub(super) unsafe fn finish_blood_wind_frame(actor: u32, current: (u16, u16)) {
    // GU's 11BEF70 advances the spin, finisher and recovery on the layer's
    // end bit. Leave the transitions and their cleanup to that handler.
    let packed = match current {
        (action::GROUP, 39..=41) => 0x0701,
        (action::GROUP, 42) => 0x0703,
        (action::GROUP, 43) => 0x0704,
        _ => return,
    };
    unsafe {
        if read::<u8>(actor, 5) != 1 || read::<u16>(actor, 1204) != packed {
            return;
        }
        action::signal_stopped_motion(actor);
    }
}

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let source = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
    } else if request == 28 {
        let clip = unsafe {
            ROLL.clip(actor, || {
                (read::<u8>(actor, 10057) & 1 != 0)
                    .then(|| relative_stick_yaw(actor))
                    .flatten()
            })
        };
        let Some(clip) = clip else { return PASS };
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
    LOCOMOTION
        .iter()
        .find(|&&(_, clip)| clip == source)
        .map_or(32000 + u32::from(source - 1000), |&(id, _)| id)
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
