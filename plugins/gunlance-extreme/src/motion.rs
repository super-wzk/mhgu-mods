use mhfz_sdk::motion::{MotionBanks, sprint_sheathe};
use mhgu_sdk::weapon::style::{PASS, action};

const PRIVATE: u32 = 0x1_0000;
// FZ common action 202, weapon row in 825901F4.
const SPRINT_SHEATHE: u16 = 1032;
pub(super) static SHEATHE_MOTIONS: mhgu_sdk::weapon::style::remote::Sheathe =
    mhfz_sdk::motion::sheathe_motions(&[crate::program::SPRINT], SPRINT_SHEATHE);

static BANKS: MotionBanks = MotionBanks::new([
    c"player\\mot\\frontier_gl\\0",
    c"player\\mot\\frontier_gl\\2",
    c"player\\mot\\frontier_gl\\4",
]);

const GROUND: &[(u32, u16)] = &[(5001, 1001), (5002, 1002), (5003, 1003), (5004, 1004)];

pub unsafe extern "C" fn preload(_actor: u32) {
    unsafe {
        BANKS.preload_for_hunt(1405);
        crate::sound::preload();
        crate::fx::preload();
    }
}

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let clip = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
    } else if unsafe { sprint_sheathe(actor, request, &SHEATHE_MOTIONS) } {
        SPRINT_SHEATHE
    } else if let Some(&(_, clip)) = GROUND.iter().find(|&&(key, _)| key == request) {
        clip
    } else {
        return PASS;
    };
    unsafe { BANKS.encode(actor, clip) }.map_or(PASS, u32::from)
}

pub unsafe fn source(actor: u32, packed: u16) -> Option<u16> {
    unsafe { BANKS.decode(actor, packed) }
}

pub unsafe extern "C" fn decode(actor: u32, packed: u32) -> u32 {
    let Some(clip) = u16::try_from(packed)
        .ok()
        .and_then(|v| unsafe { source(actor, v) })
    else {
        return PASS;
    };
    if clip == SPRINT_SHEATHE {
        return 5003;
    }
    GROUND
        .iter()
        .find(|&&(_, value)| value == clip)
        .map_or(u32::from(clip) + 36000, |&(key, _)| key)
}

pub extern "C" fn finish_ground(slot: u16, phase: u8, motion: u32) -> bool {
    matches!((slot, phase, motion), (1, 1, 5002) | (2, 1, 5003))
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

#[forge::hook(offset = 0x11a9d08)]
unsafe fn draw_route(actor: u32, kind: u32) -> u32 {
    static DRAW: [u16; 3] = [action::GROUP, action::slot(crate::program::DRAW), 0];
    // w09_m rows 0, 1 and 3 draw with a thrust. GU owns the guard and
    // airborne rows of this same table.
    if matches!(kind, 0 | 1 | 3) && unsafe { crate::active(actor) } {
        return DRAW.as_ptr() as u32;
    }
    original!(actor, kind)
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), draw_route);
}
