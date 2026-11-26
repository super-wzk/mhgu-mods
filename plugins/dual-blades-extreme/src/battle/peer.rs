//! Received actions replay the w06 program on an independent hunter clock.
use super::*;
use mhgu_sdk::weapon::style::remote::{self, PeerStates, Roster};

static PEERS: PeerStates<Battle> = PeerStates::new();
static DEMON: [AtomicU32; remote::PLAYER_COUNT] =
    [const { AtomicU32::new(0) }; remote::PLAYER_COUNT];

pub(super) fn slot_of(actor: u32) -> Option<usize> {
    unsafe {
        let roster = Roster::current()?;
        let index = roster.peer_index(actor)?;
        (PEERS.live(index, &roster, crate::WEAPON) == Some(actor)).then_some(index + 1)
    }
}
pub(super) fn bits(slot: usize) -> u8 {
    DEMON[slot - 1].load(Relaxed) as u8
}
pub(super) fn set_phase(slot: usize, actor: u32, phase: u8) {
    let full = if phase >= 2 { bits(slot) & 4 } else { 0 };
    DEMON[slot - 1].store(u32::from(phase | full), Relaxed);
    unsafe {
        let flags = read::<u8>(actor, 13168) & !3;
        write(
            actor,
            13168,
            flags | u8::from(phase != 0) | (u8::from(phase >= 2) << 1),
        );
    }
}
unsafe fn with_peer(actor: u32, visit: impl FnOnce(&mut Battle)) {
    unsafe {
        let Some(index) = Roster::current().and_then(|r| r.peer_index(actor)) else {
            return;
        };
        let equipment = read(actor, 5060);
        PEERS.update(index, actor, |saved, _| {
            if saved.as_ref().is_none_or(|b| b.equipment != equipment) {
                *saved = None;
                DEMON[index].store(0, Relaxed);
                let mut b = Battle {
                    owner: actor,
                    equipment,
                    peer_slot: index + 1,
                    ..Battle::default()
                };
                b.polish.slot = index + 1;
                b.effects.set_slot(index + 1);
                *saved = Some(b);
            }
            visit(saved.as_mut().unwrap());
        });
    }
}
unsafe extern "C" fn encode(actor: u32) -> u8 {
    if actor == OWNER.load(Relaxed) {
        phase(actor) | (u8::from(flame_full()) << 2)
    } else {
        u8::MAX
    }
}
unsafe extern "C" fn decode(actor: u32, value: u8) {
    if value <= 7 {
        unsafe {
            with_peer(actor, |b| {
                DEMON[b.peer_slot - 1].store(u32::from(value), Relaxed);
                set_phase(b.peer_slot, actor, value & 3);
            })
        }
    } else if let Some(slot) = slot_of(actor) {
        // A stock or legacy sender no longer declares an active custom mode.
        PEERS.update(slot - 1, actor, |b, _| *b = None);
        DEMON[slot - 1].store(0, Relaxed);
    }
}
unsafe extern "C" fn tick(actor: u32, state: u8) {
    unsafe { with_peer(actor, |b| b.play_received(actor, state)) };
}
impl Battle {
    unsafe fn play_received(&mut self, actor: u32, state: u8) {
        unsafe {
            if read::<u8>(actor, 5) == 0 {
                self.begin(actor, state);
                return;
            }
            let Some(cursor) = self.cursor.as_mut().filter(|c| c.state == state) else {
                return;
            };
            if self.packed != read::<u16>(actor, 1204) {
                return;
            }
            let Some(step) = program::step(state, cursor.time.step) else {
                return;
            };
            let sample = action::MotionFrame::sample(actor);
            let wrapped = cursor.time.observe(sample.frame, sample.blend);
            let finished = if step.op == 3 && step.until != 0 {
                step.reached_frame(sample.frame)
            } else {
                step.complete(cursor.time, sample, wrapped)
            };
            sound::update(&mut self.audio, actor, Some(step.clip), sample.frame, None);
            self.effects.motion_events(actor, step.clip, sample);
            self.events(actor);
            if program::aerial_motion(state).is_some() && step.op == 5 && sample.ended {
                self.start_step(actor);
            } else if finished {
                self.cursor.as_mut().unwrap().time.step += 1;
                self.start_step(actor);
            }
        }
    }
}
unsafe extern "C" fn observe(local: u32) {
    unsafe {
        PEERS.visit_live(crate::WEAPON, |_, actor, b| {
            let current = action::current(actor);
            let custom = action::custom_id(current.0, current.1);
            let drawn = native!(0x2a2444, actor) != 0;
            let dt = read::<f32>(actor, 28).max(0.);
            if custom.is_none() && b.cursor.take().is_some() {
                b.polish.leave_action();
                b.effects.clear_action();
            }
            b.polish.tick(
                actor,
                dt,
                drawn,
                current.0 == action::GROUP && !matches!(custom, Some(SPRINT | BRAKE)),
            );
            let visible = drawn && read::<u8>(local, 4180) == read::<u8>(actor, 4180);
            let mode = if visible { bits(b.peer_slot) } else { 0 };
            b.effects.demon(actor, mode & 3, mode & 4 != 0, dt);
        })
    }
}
pub(crate) static PLAYBACK: remote::Playback = remote::Playback {
    tick: tick,
    observe: Some(observe),
    action_state: Some(&remote::ActionState {
        encode: encode,
        decode: decode,
    }),
};
