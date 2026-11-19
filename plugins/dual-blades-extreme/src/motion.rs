use crate::{native, read};
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhgu_sdk::style::{
    PASS,
    action::{self, MotionBanks},
};

static BANKS: MotionBanks = MotionBanks::new([
    [
        c"player\\mot\\frontier_db\\m_0",
        c"player\\mot\\frontier_db\\m_2",
        c"player\\mot\\frontier_db\\m_4",
    ],
    [
        c"player\\mot\\frontier_db\\f_0",
        c"player\\mot\\frontier_db\\f_2",
        c"player\\mot\\frontier_db\\f_4",
    ],
]);

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

// Hunters are word aligned. Keep the choice made before GU enters the roll.
static ROLL: AtomicU32 = AtomicU32::new(0);

pub(super) fn remember_roll(actor: u32, relative_yaw: u16) {
    let side = match relative_yaw {
        9102..=32768 => 2,
        32769..=56434 => 1,
        _ => 0,
    };
    ROLL.store(actor | side, Relaxed);
}

pub(super) fn clear_roll() {
    ROLL.store(0, Relaxed);
}

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
        let flags = read::<u8>(actor, 1206);
        let end = read::<f32>(actor, 1284);
        // 951A80 stores the previous and current playheads separately. Only
        // repair a stopped layer once both have reached its authored end.
        if flags & 4 == 0
            && end > 0.
            && read::<f32>(actor, 1276) == end
            && read::<f32>(actor, 1280) == end
        {
            crate::write(actor, 1206, flags | 4);
        }
    }
}

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let source = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
    } else if request == 28 && unsafe { action::current(actor) } == (1, 6) {
        let mut saved = ROLL.load(Relaxed);
        if saved & !3 != actor {
            unsafe {
                if read::<u8>(actor, 10057) & 1 == 0 || native!(0x27e608, actor, 0) == 0 {
                    return PASS;
                }
                remember_roll(
                    actor,
                    read::<u16>(actor, 4076).wrapping_sub(native!(0x27f5d0, actor) as u16),
                );
            }
            saved = ROLL.load(Relaxed);
        }
        match saved & 3 {
            1 => 1012,
            2 => 1013,
            _ => return PASS,
        }
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
