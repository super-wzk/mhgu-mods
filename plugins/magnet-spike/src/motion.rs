use crate::{program::Mode, sound};
use mhfz_sdk::{
    action::{Cursor, Step},
    motion::MotionBanks,
    sound::Clock,
};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::{
        PASS, action,
        part_motion::{Clip, Motions, Pose},
    },
};

const PRIVATE: u32 = 0x10000;
static BANKS: MotionBanks = MotionBanks::new([
    c"player\\mot\\frontier_ms\\0",
    c"player\\mot\\frontier_ms\\2",
    c"player\\mot\\frontier_ms\\4",
]);

#[derive(Clone, Copy, Default)]
pub(super) struct WeaponPose {
    pub mode: Mode,
    pub coast: Option<u8>,
    pub boosts: u8,
}

static LOCAL_POSE: Shared<Option<(u32, WeaponPose)>> = Shared::new(None);

pub fn set_mode(actor: u32, mode: Mode) {
    LOCAL_POSE.with(|active| {
        *active = (actor != 0).then_some((
            actor,
            WeaponPose {
                mode,
                ..WeaponPose::default()
            },
        ));
    });
}

pub fn local_pose(actor: u32) -> Option<WeaponPose> {
    LOCAL_POSE.with(|active| {
        active
            .filter(|&(owner, _)| owner == actor)
            .map(|(_, pose)| pose)
    })
}

pub fn coast(actor: u32, state: u8) {
    LOCAL_POSE.with(|active| {
        if let Some((_, pose)) = active.as_mut().filter(|(owner, _)| *owner == actor) {
            pose.coast = Some(state);
        }
    });
}

pub static PARTS: Motions = Motions {
    banks: &[
        c"weapon\\frontier\\magnet_spike\\mot\\0",
        c"weapon\\frontier\\magnet_spike\\mot\\1",
        c"weapon\\frontier\\magnet_spike\\mot\\2",
        c"weapon\\frontier\\magnet_spike\\mot\\4",
    ],
    select: part_clip,
};

extern "C" fn part_clip(pose: Pose) -> Clip {
    // FZ 8121E3EA normally selects hunter clip - 1000. For a common
    // motion, it selects pose 100 (sheathed), 101 (slash) or 102 (impact).
    if pose.motion >> 16 == 1 {
        let source = pose.motion as u16;
        let bank = match source / 100 {
            10 => 0,
            12 => 2,
            14 => 3,
            _ => return Clip::NONE,
        };
        return Clip {
            bank,
            slot: (source % 100) as u8,
        };
    }
    let pose_state = local_pose(pose.actor).or_else(|| unsafe { crate::peer::pose(pose.actor) });
    let impact = pose_state.is_some_and(|state| matches!(state.mode, Mode::Impact));
    Clip {
        bank: 1,
        slot: if pose.drawn { 1 + u8::from(impact) } else { 0 },
    }
}

pub unsafe extern "C" fn preload(_actor: u32) {
    crate::magnet::reset();
    for clip in [1002, 1201, 1440] {
        unsafe { BANKS.preload(clip) };
    }
    unsafe {
        crate::combat::preload();
        crate::fx::preload();
        crate::magnet::preload();
        sound::preload();
    }
}

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    if request >> 16 != 1 {
        return PASS;
    }
    unsafe { BANKS.encode(actor, request as u16) }.map_or(PASS, u32::from)
}

pub unsafe extern "C" fn decode(actor: u32, packed: u32) -> u32 {
    u16::try_from(packed)
        .ok()
        .and_then(|packed| unsafe { BANKS.decode(actor, packed) })
        .map_or(PASS, |source| PRIVATE | u32::from(source))
}

pub unsafe fn play(actor: u32, clip: u16, start: u16, blend: u16) -> Option<u16> {
    let request = if clip < 1000 {
        u32::from(clip)
    } else {
        PRIVATE | u32::from(clip)
    };
    unsafe { action::play_motion(actor, request, f32::from(start), f32::from(blend / 2)) }
}

pub unsafe fn play_step(
    actor: u32,
    cursor: &mut Cursor,
    step: Step,
    audio: &mut Clock,
) -> Option<u16> {
    let packed = unsafe { play(actor, step.clip, step.start, step.blend) }?;
    cursor.time.cycles = 0;
    cursor.time.frame = f32::from(step.start);
    cursor.time.blend = f32::from(step.blend / 2);
    unsafe { sound::sample(audio, actor, step.clip, cursor.time.frame, true) };
    Some(packed)
}
