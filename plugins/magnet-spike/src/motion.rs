use crate::{program::Mode, sound};
use mhfz_sdk::{
    action::{Cursor, Step},
    effects::MotionClock,
    motion::{DrawnRoll, MotionBanks},
};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::{
        PASS, action,
        part_motion::{Clip, Motions, Pose, Socket},
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
    socket: Some(socket),
};

pub(super) static ROLL: DrawnRoll = DrawnRoll::new(1006, 1007);

pub(super) unsafe fn enter_roll(actor: u32, directional: bool) -> bool {
    let yaw = if directional {
        unsafe { mhgu_sdk::weapon::style::movement::live_relative_stick_yaw(actor) }.unwrap_or(0)
    } else {
        0
    };
    // 8122570A converts the signed stick angle to whole degrees before
    // choosing common 49 or 50. Beyond 135 degrees the side reverses.
    let angle = i32::from(yaw as i16) * 360 / 65536;
    let sector = match angle {
        45..=135 | -180..=-136 => 0x4000,
        -135..=-45 | 136..=179 => 0xc000,
        _ => 0,
    };
    unsafe { ROLL.enter(actor, sector) }
}

fn common_clip(actor: u32, request: u32) -> Option<u16> {
    if !matches!(request, 28 | 5001 | 5003 | 5004 | 5009 | 5012 | 5013) {
        return None;
    }
    let impact = local_pose(actor)
        .or_else(|| unsafe { crate::peer::pose(actor) })
        .is_some_and(|pose| pose.mode == Mode::Impact);
    // FZ 8121E112, 8121E1B2 and 8121E1F6.
    match request {
        28 => unsafe { ROLL.clip(actor, || None) }
            .map(|clip| clip + if impact { 4 } else { 0 })
            .or_else(|| (impact && unsafe { action::current(actor) } == (1, 6)).then_some(1021)),
        5012 => Some(if impact { 1010 } else { 1006 }),
        5013 => Some(if impact { 1011 } else { 1007 }),
        5001 => Some(if impact { 1020 } else { 1001 }),
        5003 if unsafe { action::current(actor) } == (1, 2) && {
            let (group, slot) = unsafe { action::previous(actor) };
            matches!(action::custom_id(group, slot), Some(40..=42 | 44..=46))
        } =>
        {
            Some(if impact { 1463 } else { 1462 })
        }
        5009 => Some(if impact { 1009 } else { 1008 }),
        5003 => Some(if impact { 1005 } else { 1003 }),
        5004 => Some(if impact { 1024 } else { 1004 }),
        _ => None,
    }
}

pub extern "C" fn finish_sheathe(slot: u16, phase: u8, motion: u32) -> bool {
    matches!((slot, phase, motion), (2, 1, 5003) | (3, 1, 5009))
}

extern "C" fn socket(pose: Pose, frame: f32, blend: f32) -> Socket {
    // 8121DB38 checks the advanced hunter clock. GU asks for a socket for the
    // displayed w13w pose. Switch on the first key expressed in the new
    // socket space; the 30 Hz hunter test is two or three frames later.
    let reached = |threshold| blend <= 0. && frame >= threshold;
    let chest = |attached| if attached { 3 } else { 1 };
    let default = if pose.drawn { 1 } else { 2 };
    // FZ 8121DB38 starts with the drawn flag and overrides it only on the
    // branches that explicitly select a wrist or the unadjusted chest socket.
    let grip = match source_motion(pose) {
        Some(1016) if !reached(12.) => default,
        Some(1016) => u8::from(reached(54.)),
        Some(1043) => u8::from(!reached(11.) || reached(140.)),
        Some(1035) => u8::from(reached(103.)),
        Some(1061) => u8::from(reached(9.)),
        Some(1470) => u8::from(reached(10.)),
        Some(1068) => 1,
        Some(1081) => u8::from(reached(109.)),
        Some(1083) if !reached(28.) => 0,
        Some(1402) if !reached(13.) => 0,
        Some(1406) if reached(31.) => 0,
        Some(1412 | 1418) => 0,
        Some(1416) => u8::from(reached(26.)),
        Some(1417) => u8::from(reached(20.)),
        Some(1446) => u8::from(!reached(21.)),
        Some(1467) if !reached(21.) => 0,
        Some(1468) if reached(33.) => 0,
        Some(1003) => chest(reached(74.)),
        Some(1005) => chest(reached(84.)),
        Some(1008) => chest(reached(19.)),
        Some(1009) => chest(reached(40.)),
        Some(1462) => chest(reached(30.)),
        Some(1463) => chest(reached(38.)),
        Some(1023) => chest(!reached(18.)),
        Some(1440) => chest(!reached(24.)),
        Some(1441) => chest(!reached(22.)),
        _ => default,
    };
    if grip == 2 {
        Socket {
            joint: 2,
            offset: [0., -8.5, -45.6],
            rotation: [0.01742554, -0.7658338, 0.6428017, -0.0009531543],
            scale: 0.875,
            blend_motion: false,
        }
    } else {
        Socket {
            joint: match grip {
                0 => 12,
                3 => 2,
                _ => 8,
            },
            offset: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: 1.,
            blend_motion: grip != 3,
        }
    }
}

fn source_motion(pose: Pose) -> Option<u16> {
    if pose.motion >> 16 == 1 {
        Some(pose.motion as u16)
    } else {
        // GU decodes public locomotion to 500x even if the mode has
        // changed since the clip was selected (especially during sheathing).
        unsafe { current_source(pose.actor) }
    }
}

pub(super) unsafe fn current_source(actor: u32) -> Option<u16> {
    unsafe { BANKS.decode(actor, action::packed_motion(actor)) }
}

pub(super) unsafe fn public_source(actor: u32) -> Option<u16> {
    unsafe { current_source(actor) }
        .filter(|clip| matches!(clip, 1001 | 1003..=1011 | 1020 | 1021 | 1024 | 1462 | 1463))
}

extern "C" fn part_clip(pose: Pose) -> Clip {
    // FZ 8121E3EA normally selects hunter clip - 1000. For a common
    // motion, it selects pose 100 (sheathed), 101 (slash) or 102 (impact).
    if let Some(source) = source_motion(pose) {
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

pub unsafe extern "C" fn preload(actor: u32) {
    ROLL.clear();
    crate::magnet::reset();
    crate::native_media::reset(None);
    crate::shock::reset(actor);
    for clip in [1002, 1201, 1440] {
        unsafe { BANKS.preload(clip) };
    }
    unsafe {
        crate::combat::preload();
        crate::shock::preload();
        crate::fx::preload();
        crate::magnet::preload();
        sound::preload();
    }
}

pub unsafe extern "C" fn encode(actor: u32, request: u32) -> u32 {
    let clip = if request >> 16 == 1 {
        request as u16
    } else if let Some(clip) = common_clip(actor, request) {
        clip
    } else {
        return PASS;
    };
    unsafe { BANKS.encode(actor, clip) }.map_or(PASS, u32::from)
}

pub unsafe extern "C" fn decode(actor: u32, packed: u32) -> u32 {
    u16::try_from(packed)
        .ok()
        .and_then(|packed| unsafe { BANKS.decode(actor, packed) })
        .map_or(PASS, |source| match source {
            1001 | 1020 => 5001,
            1003 | 1005 | 1462 | 1463 => 5003,
            1008 | 1009 => 5009,
            1021 => 28,
            1006 | 1007 | 1010 | 1011 if unsafe { action::current(actor) } == (1, 6) => 28,
            1006 | 1010 => 5012,
            1007 | 1011 => 5013,
            1004 | 1024 => 5004,
            _ => PRIVATE | u32::from(source),
        })
}

// 8183E574 halves the row value. 817315F8 adds the final interpolation
// tick only when a blend was actually requested.
fn blend_ticks(row: u16) -> f32 {
    let ticks = row / 2;
    f32::from(ticks + u16::from(ticks != 0))
}

pub unsafe fn play(actor: u32, clip: u16, start: u16, blend: u16) -> Option<u16> {
    let request = if clip < 1000 {
        u32::from(clip)
    } else {
        PRIVATE | u32::from(clip)
    };
    let ticks = blend_ticks(blend);
    let packed = unsafe { action::play_motion(actor, request, f32::from(start), 2. * ticks) }?;
    unsafe { mhgu_sdk::weapon::style::part_motion::begin_motion(actor, packed, 2. * ticks) };
    Some(packed)
}

pub unsafe fn play_step(
    actor: u32,
    cursor: &mut Cursor,
    step: Step,
    audio: &mut MotionClock,
    particles: &mut MotionClock,
    native: &mut crate::native_media::Playback,
) -> Option<u16> {
    let packed = unsafe { play(actor, step.clip, step.start, step.blend) }?;
    unsafe { native.enter_clip(actor, cursor, step) };
    cursor.time.cycles = 0;
    cursor.time.frame = f32::from(step.start);
    cursor.time.blend = blend_ticks(step.blend);
    particles.seek(step.clip, cursor.time.frame);
    audio.seek(step.clip, cursor.time.frame);
    Some(packed)
}
