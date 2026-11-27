//! Play the firing hunter's magnetic shot without resolving contacts twice.
use super::Projectile;
use crate::STYLE;
use alloc::vec::Vec;
use core::{
    f32::consts::TAU,
    sync::atomic::{AtomicU8, Ordering},
};
use mhgu_sdk::{
    combat::collision::hurt::HurtScene,
    geometry::Angle,
    weapon::style::{
        hunter, movement, particle,
        remote::{self, PeerStates, PositionEvent, Roster},
    },
};

const SERIAL_MASK: u8 = 63;
const STOP: u32 = 1 << 26;
const YAW_MASK: u32 = (1 << 11) - 1;
const PITCH_MASK: u32 = (1 << 9) - 1;
static NEXT: AtomicU8 = AtomicU8::new(0);
static SHOTS: PeerStates<Replay> = PeerStates::new();

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
    shots: Vec<Projectile>,
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
            Some(1)
        } else {
            1u32.checked_shl(64 - u32::from(advance))
        }
    }

    fn stop(&mut self, serial: u8, bit: u32) {
        self.stops |= bit;
        self.shots.retain(|shot| shot.serial != serial);
    }
}

pub(crate) extern "C" fn receive(event: &PositionEvent) {
    let actor = event.actor;
    let payload = event.payload;
    let is_stop = payload & STOP != 0;
    if is_stop && payload & !(STOP | u32::from(SERIAL_MASK)) != 0 {
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
    let serial = payload as u8 & SERIAL_MASK;
    SHOTS.update(index, actor, |saved, _| {
        let replay = saved.get_or_insert_with(Replay::default);
        let Some(bit) = replay.bit(serial) else {
            return;
        };
        if is_stop {
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
    let area = scene.as_ref().map(HurtScene::area);
    let frames = scene.as_ref().map_or(0., HurtScene::frames);
    for index in 0..remote::PLAYER_COUNT {
        let actor = roster
            .as_ref()
            .and_then(|roster| roster.peer(index))
            .filter(|&actor| remote::is_position_event_peer(STYLE.id, actor))
            .unwrap_or(0);
        SHOTS.update(index, actor, |saved, _| {
            if let Some(replay) = saved {
                replay.shots.retain_mut(|shot| {
                    if Some(shot.area) != area || unsafe { hunter::area(actor) } != shot.area {
                        return false;
                    }
                    if frames == 0. {
                        return true;
                    }
                    let step = frames.min(shot.remaining);
                    shot.advance(shot.end_at(step), step)
                });
            }
        });
    }
}
