//! Replay the sender's accepted actions on GU's remote hunters.
use crate::{
    bind, combat, fx, magnet,
    motion::{self, WeaponPose},
    program::{self, Mode},
    sound,
};
use mhfz_sdk::{action::Cursor, effects::MotionClock};
use mhgu_sdk::{
    combat::collision::hurt::HurtScene,
    weapon::style::{
        action, hunter,
        remote::{self, PeerStates, Roster},
    },
};

pub(super) static PLAYBACK: remote::Playback = remote::Playback {
    tick,
    observe: Some(observe),
    action_state: Some(&remote::ActionState {
        encode: encode_mode,
        decode: decode_mode,
    }),
};

pub(super) static POSITION: remote::PositionState = remote::PositionState {
    encode: encode_position,
    decode: decode_position,
};

static PEERS: PeerStates<Replay> = PeerStates::new();
// Pose selection can run from inside the native motion call. Keep the
// published mode outside the exclusively borrowed replay cursor.
static POSES: PeerStates<PeerPose> = PeerStates::new();

#[derive(Default)]
struct PeerPose {
    pose: WeaponPose,
    boost_effects: fx::BoostEffects,
}

fn index_of(actor: u32) -> Option<usize> {
    unsafe { Roster::current()?.peer_index(actor) }
}

pub(super) unsafe fn pose(actor: u32) -> Option<WeaponPose> {
    let index = index_of(actor)?;
    let mut result = None;
    POSES.update(index, actor, |saved, _| {
        result = saved.as_ref().map(|saved| saved.pose)
    });
    result
}

unsafe extern "C" fn encode_mode(actor: u32) -> u8 {
    let (group, slot) = unsafe { action::current(actor) };
    let previous = motion::local_pose(actor).unwrap_or_default().mode;
    // GU can build the packet during the action transition, before our
    // controller has published the new mode.
    action::custom_id(group, slot)
        .filter(|&state| program::ACTIONS.contains(state))
        .map_or(previous, |state| previous.for_state(state)) as u8
}

unsafe extern "C" fn decode_mode(actor: u32, value: u8) {
    let Some(index) = index_of(actor) else { return };
    let mode = match value {
        0 => Mode::Slash,
        1 => Mode::Impact,
        _ => return,
    };
    POSES.update(index, actor, |saved, _| {
        let saved = saved.get_or_insert_with(PeerPose::default);
        saved.pose.mode = mode;
        saved.pose.coast = None;
    });
}

unsafe extern "C" fn encode_position(actor: u32) -> remote::PositionSample {
    let (group, slot) = unsafe { action::current(actor) };
    let action = action::custom_id(group, slot)
        .filter(|&id| program::is_attraction(id))
        .map_or(u8::MAX, |id| {
            u8::from(motion::local_pose(actor).is_some_and(|pose| pose.coast == Some(id)))
        });
    remote::PositionSample {
        action,
        weapon: magnet::boosts(actor),
    }
}

unsafe extern "C" fn decode_position(actor: u32, sample: remote::PositionSample) {
    if let Some(index) = index_of(actor) {
        POSES.update(index, actor, |saved, _| {
            let saved = saved.get_or_insert_with(PeerPose::default);
            if sample.weapon <= 3 {
                saved.pose.boosts = sample.weapon;
            }
            if sample.action == 1 {
                let (group, slot) = unsafe { action::current(actor) };
                saved.pose.coast =
                    action::custom_id(group, slot).filter(|&id| program::is_attraction(id));
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
    finisher_submitted: bool,
    audio: MotionClock,
    particles: MotionClock,
    native: crate::native_media::Playback,
    charge: crate::battle::Charge,
    motion_accent: bool,
}

impl Replay {
    unsafe fn play_step(&mut self, actor: u32, index: usize) {
        let Some(cursor) = self.cursor.as_mut() else {
            return;
        };
        self.packed = None;
        let Some(step) = program::step(cursor.state, cursor.time.step).filter(|step| step.op != 2)
        else {
            // The sender will supply the next accepted action.
            return;
        };
        self.packed = unsafe {
            motion::play_step(
                actor, cursor, step, &mut self.audio, &mut self.particles, &mut self.native,
            )
        };
        if self.packed.is_some() {
            unsafe {
                events(
                    actor,
                    index,
                    cursor,
                    &mut self.finisher_submitted,
                    &mut self.charge,
                )
            };
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
                let pose = &mut saved.get_or_insert_with(PeerPose::default).pose;
                pose.mode = pose.mode.for_state(state);
            });
            self.airborne = !program::is_attraction(state)
                && !program::is_bind_drop(state)
                && program::flight_landing(state).is_some();
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
            self.finisher_submitted = false;
            self.charge = Default::default();
            self.motion_accent = false;
            // Only the confirmed counter and perfect guard actions carry peer feedback.
            match state {
                57 | 76 | 77 => unsafe {
                    sound::slash_counter(actor, &mut self.motion_accent);
                    fx::guard(actor, Mode::Slash);
                },
                113 => unsafe { fx::guard(actor, Mode::Impact) },
                66 | 67 => unsafe {
                    let mode = pose(actor).unwrap_or_default().mode;
                    magnet::network::begin_repel(index, actor, mode);
                },
                _ => {}
            }
        }
        // Position updates keep moving the hunter through GU's interpolation.
        // A received coast pose replaces the launch clip within the same action.
        if self.coast_played {
            if let Some(pose) = unsafe { pose(actor) } {
                let frame = unsafe { action::MotionFrame::sample(actor) };
                unsafe {
                    sound::sample(&mut self.audio, actor, pose.mode.coast_clip(), frame);
                    fx::motion(
                        &mut self.particles,
                        &mut self.motion_accent,
                        actor,
                        pose.mode.coast_clip(),
                        frame,
                    )
                };
            }
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
            self.audio.seek(pose.mode.coast_clip(), 0.);
            return;
        }
        let Some(packed) = self.packed else {
            unsafe { self.play_step(actor, index) };
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
        unsafe { sound::sample(&mut self.audio, actor, step.clip, sample) };
        unsafe {
            fx::motion(
                &mut self.particles,
                &mut self.motion_accent,
                actor,
                step.clip,
                sample,
            )
        };
        let pose = unsafe { pose(actor) }.unwrap_or_default();
        unsafe {
            self.native.sample(
                actor,
                state,
                step.clip,
                sample,
                pose.mode,
                pose.boosts,
                Some(index),
                &mut self.motion_accent,
            )
        };
        unsafe {
            self.charge
                .update(actor, ((actor + 28) as *const f32).read().max(0.), false)
        };
        let wrapped = cursor.time.observe(sample.frame, sample.blend * 0.5);
        unsafe {
            events(
                actor,
                index,
                cursor,
                &mut self.finisher_submitted,
                &mut self.charge,
            )
        };
        if !self.airborne && matches!(step.clip, 1460 | 1472) && sample.frame >= 34. {
            self.airborne = true;
            unsafe { action::start_airborne_weapon_state(actor) };
        }
        if program::step_complete(step, cursor.time, sample, wrapped) {
            cursor.time.step += 1;
            unsafe { self.play_step(actor, index) };
        }
    }
}

unsafe fn events(
    actor: u32,
    index: usize,
    cursor: &mut Cursor,
    finisher_submitted: &mut bool,
    charge: &mut crate::battle::Charge,
) {
    let bonus = unsafe { power_bonus(actor) };
    let finishing = matches!(cursor.state, 105 | 157);
    unsafe {
        combat::events(actor, cursor, bonus, |event, _, _| {
            if event == 26 {
                charge.start(actor, false);
            }
            if !finishing || *finisher_submitted {
                return;
            }
            // FZ 812264B4 fires event 44 on the held monster's controller
            // when this hunter is a peer. Event 43 covers the normal frame.
            if event == 43
                || event == 44 && bind::hosts_peer_finisher(actor, index, hunter::area(actor))
            {
                *finisher_submitted = combat::bind_finisher(actor, bonus);
            }
        });
    }
}

pub(super) unsafe fn launch_sound(actor: u32, index: usize, offset: [f32; 3]) {
    PEERS.update(index, actor, |replay, _| {
        if let Some(replay) = replay {
            unsafe { sound::motion_accent(actor, &mut replay.motion_accent, 8, offset) };
        }
    });
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
    unsafe { magnet::network::observe(local) };
    let roster = unsafe { Roster::current() };
    let scene = roster
        .as_ref()
        .and_then(|_| unsafe { HurtScene::from_raw(local) });
    let area = scene.as_ref().map(HurtScene::area);
    let frames = scene.as_ref().map_or(0., HurtScene::frames);
    for index in 0..remote::PLAYER_COUNT {
        let actor = roster
            .as_ref()
            .and_then(|r| r.peer(index))
            .filter(|&actor| remote::is_position_event_peer(crate::STYLE.id, actor))
            .unwrap_or(0);
        POSES.update(index, actor, |saved, _| {
            if let (Some(saved), Some(area)) = (saved, area) {
                unsafe {
                    saved.boost_effects.advance(
                        actor,
                        area,
                        frames,
                        saved.pose.mode,
                        saved.pose.boosts,
                        false,
                    )
                };
            }
        });
        let present =
            actor != 0 && unsafe { hunter::is_quest(actor) && Some(hunter::area(actor)) == area };
        PEERS.update(index, if present { actor } else { 0 }, |saved, _| {
            if !present {
                return;
            }
            let (group, slot) = unsafe { action::current(actor) };
            if action::custom(group, slot) {
                return;
            }
            if let Some(clip) = unsafe { motion::public_source(actor) } {
                let replay = saved.get_or_insert_with(Replay::default);
                replay.cursor = None;
                let frame = unsafe { action::MotionFrame::sample(actor) };
                unsafe {
                    sound::sample(&mut replay.audio, actor, clip, frame);
                    fx::motion(
                        &mut replay.particles,
                        &mut replay.motion_accent,
                        actor,
                        clip,
                        frame,
                    );
                }
            } else {
                *saved = None;
            }
        });
    }
}
