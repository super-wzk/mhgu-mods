use crate::{fx, sound};
use mhfz_sdk::motion::MotionBanks;
use mhgu_sdk::style::{PASS, action};

// GU 11A91E0 reads this field when choosing the equipped weapon's pose.
pub(super) const SWORD_POSE: u32 = 13096;

static BANKS: MotionBanks = MotionBanks::new([
    [
        c"player\\mot\\frontier_sa\\m_0",
        c"player\\mot\\frontier_sa\\m_2",
        c"player\\mot\\frontier_sa\\m_4",
    ],
    [
        c"player\\mot\\frontier_sa\\f_0",
        c"player\\mot\\frontier_sa\\f_2",
        c"player\\mot\\frontier_sa\\f_4",
    ],
]);

pub(super) unsafe fn source_clip(actor: u32, packed: u16) -> Option<u16> {
    unsafe { BANKS.decode(actor, packed) }
}

const PRIVATE: u32 = 0x1_0000;

const LOCOMOTION: &[(u32, u16)] = &[(5001, 1001), (5002, 1002), (5003, 1003), (5004, 1004)];

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let source = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
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

pub(super) unsafe extern "C" fn preload_draw(actor: u32) {
    unsafe {
        BANKS.preload(actor, 1002);
        BANKS.preload(actor, 1412);
        sound::preload_draw();
        fx::preload_draw();
    }
}
