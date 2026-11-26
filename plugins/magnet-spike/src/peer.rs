//! Replay the sender's accepted actions on GU's remote hunters.
use crate::{
    combat, magnet,
    motion::{self, WeaponPose},
    program::{self, Mode},
};
use mhfz_sdk::action::Cursor;
use mhgu_sdk::weapon::style::{
    action, hunter,
    remote::{self, PeerStates, Roster},
};

pub(super) static PLAYBACK: remote::Playback = remote::Playback {
    tick,
    observe: Some(observe),
    action_state: Some(&remote::ActionState {
        encode: encode_mode,
        decode: decode_mode,
    }),
};

pub(super) static POSITION: remote::ActionState = remote::ActionState {
    encode: encode_position,
    decode: decode_position,
};

static PEERS: PeerStates<Replay> = PeerStates::new();
// Pose selection can run from inside the native motion call. Keep the
// published mode outside the exclusively borrowed replay cursor.
static POSES: PeerStates<WeaponPose> = PeerStates::new();

fn index_of(actor: u32) -> Option<usize> {
    unsafe { Roster::current()?.peer_index(actor) }
}

pub(super) unsafe fn pose(actor: u32) -> Option<WeaponPose> {
    let index = index_of(actor)?;
    let mut result = None;
    POSES.update(index, actor, |pose, _| result = *pose);
    result
}

unsafe extern "C" fn encode_mode(actor: u32) -> u8 {
    let (group, slot) = unsafe { action::current(actor) };
    let previous = motion::local_pose(actor).unwrap_or_default().mode;
    // GU can build the packet during the action transition, before our
    // controller has published the new mode.
    let mode = action::custom_id(group, slot)
        .filter(|&state| program::ACTIONS.contains(state))
        .map_or(previous, |state| previous.for_state(state));
    let mut boosts = magnet::boosts(actor);
    if let Some(mode) = action::custom_id(group, slot).and_then(program::boost_mode)
        && magnet::boost_ready(actor, mode)
    {
        boosts |= 1 << mode as u8;
    }
    mode as u8 | boosts << 1
}

unsafe extern "C" fn decode_mode(actor: u32, value: u8) {
    let Some(index) = index_of(actor) else { return };
    let mode = if value & 1 == 0 {
        Mode::Slash
    } else {
        Mode::Impact
    };
    POSES.update(index, actor, |saved, _| {
        *saved = Some(WeaponPose {
            mode,
            coast: None,
            boosts: value >> 1,
        });
    });
}

unsafe extern "C" fn encode_position(actor: u32) -> u8 {
    let (group, slot) = unsafe { action::current(actor) };
    let Some(id) = action::custom_id(group, slot) else {
        return u8::MAX;
    };
    let coast = motion::local_pose(actor).is_some_and(|pose| pose.coast == Some(id));
    u8::from(coast) | magnet::boosts(actor) << 1
}

unsafe extern "C" fn decode_position(actor: u32, value: u8) {
    let (group, slot) = unsafe { action::current(actor) };
    let Some(id) = action::custom_id(group, slot) else {
        return;
    };
    if let Some(index) = index_of(actor) {
        POSES.update(index, actor, |saved, _| {
            let pose = saved.get_or_insert_with(WeaponPose::default);
            pose.boosts = value >> 1;
            if value & 1 != 0 && program::is_attraction(id) {
                pose.coast = Some(id);
            }
        });
    }
}

unsafe fn power_bonus(actor: u32) -> u8 {
    let pose = unsafe { pose(actor) }.unwrap_or_default();
    program::power_bonus(pose.boosts, pose.mode)
}

#[derive(Default)]
struct Replay {
    cursor: Option<Cursor>,
    packed: Option<u16>,
    airborne: bool,
    coast_played: bool,
}

impl Replay {
    unsafe fn play_step(&mut self, actor: u32) {
        let Some(cursor) = self.cursor.as_mut() else {
            return;
        };
        self.packed = None;
        let Some(step) = program::step(cursor.state, cursor.time.step).filter(|step| step.op != 2)
        else {
            // The sender will supply the next accepted action.
            return;
        };
        self.packed = unsafe { motion::play_step(actor, cursor, step) };
        if self.packed.is_some() {
            unsafe { combat::events(actor, cursor, power_bonus(actor), |_, _, _| {}) };
        }
    }

    unsafe fn tick(&mut self, actor: u32, index: usize, state: u8) {
        if unsafe { action::waiting_to_start(actor) }
            || self
                .cursor
                .as_ref()
                .is_none_or(|cursor| cursor.state != state)
        {
            POSES.update(index, actor, |saved, _| {
                let pose = saved.get_or_insert_with(WeaponPose::default);
                pose.mode = pose.mode.for_state(state);
            });
            self.airborne =
                !program::is_attraction(state) && program::flight_landing(state).is_some();
            unsafe {
                if self.airborne {
                    action::start_airborne_weapon_state(actor);
                } else {
                    action::start_weapon_state(actor);
                }
            }
            self.cursor = Some(program::cursor(state));
            self.packed = None;
            self.coast_played = false;
        }
        // Position updates keep moving the hunter through GU's interpolation.
        // A received coast pose replaces the launch clip within the same action.
        if self.coast_played {
            return;
        }
        if program::is_attraction(state)
            && let Some(pose) = unsafe { pose(actor) }.filter(|pose| pose.coast == Some(state))
            && unsafe { motion::play(actor, pose.mode.coast_clip(), 0, 0) }.is_some()
        {
            if !self.airborne {
                unsafe { action::start_airborne_weapon_state(actor) };
                self.airborne = true;
            }
            self.coast_played = true;
            return;
        }
        let Some(packed) = self.packed else {
            unsafe { self.play_step(actor) };
            return;
        };
        if packed != unsafe { action::packed_motion(actor) } {
            return;
        }
        let cursor = self.cursor.as_mut().unwrap();
        let Some(step) = program::step(state, cursor.time.step) else {
            return;
        };
        let sample = unsafe { action::MotionFrame::sample(actor) };
        let wrapped = cursor.time.observe(sample.frame, sample.blend);
        unsafe { combat::events(actor, cursor, power_bonus(actor), |_, _, _| {}) };
        if !self.airborne && matches!(step.clip, 1460 | 1472) && sample.frame >= 34. {
            self.airborne = true;
            unsafe { action::start_airborne_weapon_state(actor) };
        }
        if program::step_complete(step, cursor.time, sample, wrapped) {
            cursor.time.step += 1;
            unsafe { self.play_step(actor) };
        }
    }
}

unsafe extern "C" fn tick(actor: u32, state: u8) {
    let Some(index) = index_of(actor) else { return };
    if !unsafe { hunter::local().is_some_and(|local| hunter::area(local) == hunter::area(actor)) } {
        return;
    }
    PEERS.update(index, actor, |replay, _| unsafe {
        replay
            .get_or_insert_with(Replay::default)
            .tick(actor, index, state);
    });
}

unsafe extern "C" fn observe(local: u32) {
    let roster = unsafe { Roster::current() };
    let area = roster.as_ref().map(|_| unsafe { hunter::area(local) });
    for index in 0..remote::PLAYER_COUNT {
        let actor = roster.as_ref().and_then(|r| r.peer(index)).unwrap_or(0);
        POSES.update(index, actor, |_, _| {});
        let replaying = actor != 0
            && unsafe {
                hunter::is_quest(actor) && Some(hunter::area(actor)) == area && {
                    let (group, slot) = action::current(actor);
                    action::custom(group, slot)
                }
            };
        PEERS.update(index, if replaying { actor } else { 0 }, |_, _| {});
    }
}
