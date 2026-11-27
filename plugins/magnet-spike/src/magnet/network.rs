//! Replay the firing hunter's magnetic shot and confirmed magnetic evade.
use super::{MARK_FRAMES, Projectile, mark_clock_running};
use crate::{
    STYLE,
    fx::{EvadeFlash, RepelEffect},
    program::Mode,
};
use alloc::vec::Vec;
use core::{
    f32::consts::TAU,
    sync::atomic::{AtomicU8, Ordering},
};
use mhgu_sdk::{
    combat::collision::hurt::{HurtScene, Target},
    geometry::Angle,
    weapon::style::{
        hunter, movement, particle,
        remote::{self, PeerStates, PositionEvent, Roster, TargetEvent},
    },
};

const SERIAL_MASK: u8 = 63;
// Launch uses all 26 low bits. Only control events set bit 26.
const STOP: u32 = 1 << 26;
const EVADE: u32 = STOP | 1 << 25;
const AREA_SHIFT: u32 = 6;
const AREA_MASK: u32 = 0xff << AREA_SHIFT;
const YAW_MASK: u32 = (1 << 11) - 1;
const PITCH_MASK: u32 = (1 << 9) - 1;
static NEXT: AtomicU8 = AtomicU8::new(0);
static NEXT_EVADE: AtomicU8 = AtomicU8::new(0);
static SHOTS: PeerStates<Replay> = PeerStates::new();

pub(super) fn marked_target(actor: u32, member: usize) -> Option<Target> {
    let mut target = None;
    SHOTS.update(member, actor, |peer, _| {
        target = peer
            .as_ref()
            .and_then(|peer| peer.mark.map(|(target, _)| target));
    });
    target
}

pub(super) fn next_serial() -> u8 {
    NEXT.fetch_add(1, Ordering::Relaxed) & SERIAL_MASK
}

pub(super) fn launch(actor: u32, serial: u8, [x, y, z]: [f32; 3]) {
    let yaw = libm::roundf(libm::atan2f(x, z) * (2048. / TAU)) as i32 as u32 & YAW_MASK;
    let pitch = libm::roundf(libm::atan2f(y, libm::hypotf(x, z)) * (1024. / TAU)).clamp(-256., 255.)
        as i32 as u32
        & PITCH_MASK;
    remote::send_position_event(
        STYLE.id,
        actor,
        u32::from(serial) | (yaw << 6) | (pitch << 17),
    );
}

pub(super) fn stop(actor: u32, serial: u8) {
    remote::send_position_event(STYLE.id, actor, STOP | u32::from(serial));
}

pub(super) fn evade(actor: u32, area: u8) {
    let serial = NEXT_EVADE.fetch_add(1, Ordering::Relaxed) & SERIAL_MASK;
    remote::send_position_event(
        STYLE.id,
        actor,
        EVADE | u32::from(serial) | (u32::from(area) << AREA_SHIFT),
    );
}

fn direction(payload: u32) -> [f32; 3] {
    let yaw = ((payload >> 6) & YAW_MASK) as u16;
    let pitch = ((payload >> 17) & PITCH_MASK) as u16;
    let (sin_yaw, cos_yaw) = Angle::from_units(yaw << 5).sin_cos();
    let signed_pitch = ((pitch << 7) as i16) >> 7;
    let (sin_pitch, cos_pitch) = Angle::from_units((signed_pitch as u16) << 6).sin_cos();
    [sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch]
}

#[derive(Default)]
struct Replay {
    newest: Option<u8>,
    launches: u32,
    stops: u32,
    last_mark: u32,
    shots: Vec<Projectile>,
    mark: Option<(Target, f32)>,
    repel: Option<RepelEffect>,
    last_evade: Option<u8>,
    evade_flash: Option<EvadeFlash>,
}

impl Replay {
    // Each bit tracks a shot relative to the newest serial. Retain both
    // phases so a duplicate launch cannot resurrect a completed shot.
    fn bit(&mut self, serial: u8) -> Option<u32> {
        let old = self.newest.get_or_insert(serial);
        let advance = serial.wrapping_sub(*old) & SERIAL_MASK;
        if advance < 32 {
            *old = serial;
            self.launches <<= advance;
            self.stops <<= advance;
            self.last_mark <<= advance;
            Some(1)
        } else {
            1u32.checked_shl(64 - u32::from(advance))
        }
    }

    fn stop(&mut self, serial: u8, bit: u32) {
        self.stops |= bit;
        self.shots.retain(|shot| shot.serial != serial);
    }

    fn advance_mark(&mut self, actor: u32, scene: &HurtScene<'_>, frames: f32) -> Option<[f32; 3]> {
        let (target, remaining) = self.mark.as_mut()?;
        if mark_clock_running(actor) {
            *remaining -= frames;
        }
        if *remaining > 0. {
            if let Ok(point) = scene.refresh_mark(target) {
                return point;
            }
        }
        self.mark = None;
        None
    }
}

pub(crate) extern "C" fn receive_target(event: &TargetEvent) {
    let actor = event.actor;
    let Some(index) = (unsafe { Roster::current() }).and_then(|roster| roster.peer_index(actor))
    else {
        return;
    };
    if event.serial > SERIAL_MASK {
        return;
    }
    SHOTS.update(index, actor, |saved, _| {
        let replay = saved.get_or_insert_with(Replay::default);
        let Some(bit) = replay.bit(event.serial) else {
            return;
        };
        replay.stop(event.serial, bit);
        if replay.last_mark == 0 || bit < replay.last_mark {
            replay.last_mark = bit;
            replay.mark = Some((event.target, MARK_FRAMES));
        }
    });
}

pub(crate) unsafe fn begin_repel(index: usize, actor: u32, mode: Mode) {
    let area = unsafe { hunter::area(actor) };
    SHOTS.update(index, actor, |saved, _| {
        saved.get_or_insert_with(Replay::default).repel = Some(RepelEffect::new(area, mode));
    });
}

pub(crate) extern "C" fn receive(event: &PositionEvent) {
    let actor = event.actor;
    let payload = event.payload;
    let control = payload & EVADE;
    let control_mask = if control == EVADE {
        EVADE | AREA_MASK
    } else {
        STOP
    };
    if control & STOP != 0 && payload & !(control_mask | u32::from(SERIAL_MASK)) != 0 {
        return;
    }
    let Some(index) = (unsafe { Roster::current() }).and_then(|roster| roster.peer_index(actor))
    else {
        return;
    };
    let area = unsafe { hunter::area(actor) };
    let same_area = unsafe { hunter::local().is_some_and(|local| hunter::area(local) == area) };
    if !same_area {
        return;
    }
    if control == EVADE && (payload & AREA_MASK) >> AREA_SHIFT != u32::from(area) {
        return;
    }
    let serial = payload as u8 & SERIAL_MASK;
    SHOTS.update(index, actor, |saved, _| {
        let replay = saved.get_or_insert_with(Replay::default);
        if control == EVADE {
            if replay
                .last_evade
                .is_none_or(|last| matches!(serial.wrapping_sub(last) & SERIAL_MASK, 1..=31))
            {
                replay.last_evade = Some(serial);
                replay.evade_flash = unsafe { EvadeFlash::emit(actor, area) };
            }
            return;
        }
        let Some(bit) = replay.bit(serial) else {
            return;
        };
        if control == STOP {
            replay.stop(serial, bit);
        } else if (replay.launches | replay.stops) & bit == 0 {
            replay.launches |= bit;
            let position = unsafe { movement::position(actor) };
            let muzzle = unsafe { particle::joint_position(actor, 12) }.unwrap_or(position);
            let origin = core::array::from_fn(|i| event.position[i] + (muzzle[i] - position[i]));
            replay
                .shots
                .push(Projectile::new(serial, area, origin, direction(payload)));
        }
    });
}

pub(crate) unsafe fn observe(local: u32) {
    let roster = unsafe { Roster::current() };
    let scene = roster
        .as_ref()
        .and_then(|_| unsafe { HurtScene::from_raw(local) });
    for index in 0..remote::PLAYER_COUNT {
        let actor = roster
            .as_ref()
            .and_then(|roster| roster.peer(index))
            .filter(|&actor| remote::is_position_event_peer(STYLE.id, actor))
            .unwrap_or(0);
        SHOTS.update(index, actor, |saved, _| {
            if let (Some(replay), Some(scene)) = (saved, &scene) {
                let frames = scene.frames();
                let area = scene.area();
                if unsafe { hunter::area(actor) } != area {
                    replay.shots.clear();
                    replay.advance_mark(actor, scene, frames);
                    replay.repel = None;
                    replay.evade_flash = None;
                    return;
                }
                if let Some(flash) = &replay.evade_flash
                    && !unsafe { flash.advance(actor, area) }
                {
                    replay.evade_flash = None;
                }
                replay.shots.retain_mut(|shot| {
                    if shot.area != area {
                        return false;
                    }
                    if frames == 0. {
                        return true;
                    }
                    let step = frames.min(shot.remaining);
                    shot.advance(shot.end_at(step), step)
                });
                let point = replay.advance_mark(actor, scene, frames);
                if let Some(repel) = &mut replay.repel
                    && !unsafe { repel.advance(actor, area, frames, point) }
                {
                    replay.repel = None;
                }
            }
        });
    }
}
