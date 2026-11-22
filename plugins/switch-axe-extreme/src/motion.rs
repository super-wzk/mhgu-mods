use crate::{WEAPON_ID, fx, native, program, read, sound, write};
use mhfz_sdk::motion::MotionBanks;
use mhgu_sdk::style::{PASS, action};

// GU 11A91E0 reads this field when choosing the equipped weapon's pose.
pub(super) const SWORD_POSE: u32 = 13096;
pub(super) const SPRINT_SHEATHE: u16 = 1499;
pub(super) const SHEATHE_ACTION: (u16, u16) = (1, 2);
const GU_SHEATHE_MOTION: u32 = 5003;

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

pub(super) unsafe fn native_clip(actor: u32) -> Option<u16> {
    let packed = unsafe { crate::read(actor, 1204) };
    // A native action may take over before GU unbinds the private attack clip.
    unsafe { BANKS.decode(actor, packed) }.filter(|&clip| {
        if clip == SPRINT_SHEATHE {
            unsafe { action::current(actor) == SHEATHE_ACTION }
        } else {
            clip == 1009 || LOCOMOTION.iter().any(|&(_, source)| source == clip)
        }
    })
}

const PRIVATE: u32 = 0x1_0000;
const RETURN_TO_AXE_CLIP: u16 = 1441;
const GU_RETRACT_TRACK: u32 = 161;

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global sa_bind_weapon_track",
    ".hidden sa_bind_weapon_track",
    ".type sa_bind_weapon_track,%function",
    "sa_bind_weapon_track:",
    "mov ip,#0",
    "vmov s0,ip",
    "vmov s1,r3",
    "vmov.f32 s2,#1.0",
    "ldr ip,[sp]",
    "mov r3,#0",
    "bx ip",
);
unsafe extern "C" {
    fn sa_bind_weapon_track(model: u32, layer: u32, clip: u32, frame: u32, entry: u32) -> u32;
}

#[forge::hook(offset = 0x308b3c)]
unsafe fn weapon_tracks(model: u32, actor: u32) -> u32 {
    unsafe {
        if action::current(actor) != (action::GROUP, action::slot(program::SWORD_TO_AXE))
            || mhgu_sdk::style::hunter::equipped_weapon(actor) != Some(WEAPON_ID)
            || read::<u32>(actor, 9120) != model
        {
            return original!(model, actor);
        }
        let frame = read::<f32>(actor, 1280);
        if !(program::RETRACT_START..program::RETRACT_END).contains(&frame) {
            return original!(model, actor);
        }
        let decoded = native!(0x53a058, actor);
        if decoded != private_motion_id(RETURN_TO_AXE_CLIP)
            || native!(0x953b10, model, GU_RETRACT_TRACK) == 0
        {
            return original!(model, actor);
        }
        // The retail w08 rigs share the 15-sample sword-to-axe track.
        // Use the same explicit layer playhead as GU 308B3C.
        let track_frame = (frame - program::RETRACT_START)
            * (14. / (program::RETRACT_END - program::RETRACT_START));
        let entry = forge::mem::text_addr() + 0x954640;
        write(model, 5060, decoded);
        for layer in 0..2 {
            let clip = GU_RETRACT_TRACK | (layer << 8);
            let offset = 320 * layer;
            let current = u32::from(read::<u16>(model, 1204 + offset));
            if layer == 0 || native!(0x953b10, model, clip) != 0 {
                if current != clip {
                    sa_bind_weapon_track(model, layer, clip, track_frame.to_bits(), entry);
                }
                write(model, 1276 + offset, track_frame);
                write(
                    model,
                    1212 + offset,
                    read::<u32>(model, 1212 + offset) | 0x10,
                );
            } else if current != 0xffff {
                sa_bind_weapon_track(model, layer, 0xffff, 0, entry);
            }
        }
        0
    }
}

fn private_motion_id(source: u16) -> u32 {
    32000 + u32::from(source - 1000)
}

const LOCOMOTION: &[(u32, u16)] = &[
    (5001, 1001),
    (5002, 1002),
    (GU_SHEATHE_MOTION, 1003),
    (5004, 1004),
];

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let source = if (PRIVATE..PRIVATE * 2).contains(&request) {
        request as u16
    } else if request == GU_SHEATHE_MOTION && unsafe { action::current(actor) } == SHEATHE_ACTION {
        let (group, slot) = unsafe { action::previous(actor) };
        // FZ common action 202 uses w12's dedicated sprint sheathe clip.
        if action::custom_id(group, slot).is_some_and(program::is_sprint) {
            SPRINT_SHEATHE
        } else {
            1003
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
    if source == SPRINT_SHEATHE {
        return GU_SHEATHE_MOTION;
    }
    LOCOMOTION
        .iter()
        .find(|&&(_, clip)| clip == source)
        .map_or_else(|| private_motion_id(source), |&(id, _)| id)
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

pub(super) unsafe extern "C" fn preload(actor: u32) {
    unsafe {
        BANKS.preload_for_hunt(actor, 1412);
        crate::battle::preload_hit_shapes();
        sound::preload();
        fx::preload();
        crate::hud::preload();
    }
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), weapon_tracks);
}
