//! Play the action accepted from netPlAction. The sender selects all branches.
use super::*;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhgu_sdk::style::remote::{self, PeerStates, Roster};

static PEERS: PeerStates<Battle> = PeerStates::new();
static LEVEL: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
// 0..=4 are the original charge tiers. Only successful guard actions use 5.
const GUARD_BODY: u8 = 5;

pub(super) enum GuardBody {
    Off,
    Pending(u8),
    Active { area: u8, remaining: f32 },
}

pub(crate) unsafe fn peer_action(actor: u32) -> bool {
    unsafe {
        let (group, slot) = action::current(actor);
        action::custom_id(group, slot).is_some_and(|id| ACTIONS.contains(id))
            && Roster::current().is_some_and(|r| r.peer_index(actor).is_some())
    }
}
pub(super) fn publish(actor: u32, level: u8) {
    LEVEL.store(u32::from(level), Relaxed);
    OWNER.store(actor, Relaxed);
}
unsafe extern "C" fn encode(actor: u32) -> u8 {
    if unsafe { guard_success_action(actor) } {
        GUARD_BODY
    } else if OWNER.load(Relaxed) == actor {
        LEVEL.load(Relaxed) as u8
    } else {
        u8::MAX
    }
}
unsafe fn with_peer(actor: u32, visit: impl FnOnce(&mut Battle)) {
    unsafe {
        let Some(index) = Roster::current().and_then(|r| r.peer_index(actor)) else {
            return;
        };
        let equip = read(actor, 5060);
        PEERS.update(index, actor, |saved, _| {
            if saved.as_ref().is_none_or(|b| b.equip != equip) {
                let mut b = Battle::fresh(actor, equip);
                b.peer_slot = index + 1;
                *saved = Some(b);
            }
            visit(saved.as_mut().unwrap());
        });
    }
}
unsafe extern "C" fn decode(actor: u32, level: u8) {
    unsafe {
        if guard_success_action(actor) {
            // GU has accepted the packet and queued the fresh motion start.
            if read::<u8>(actor, 5) == 0 {
                with_peer(actor, |b| {
                    b.peer_guard = if level == GUARD_BODY {
                        GuardBody::Pending(read(actor, 4180))
                    } else {
                        GuardBody::Off
                    };
                });
            }
        } else if level <= 4 {
            with_peer(actor, |b| {
                b.peer_guard = GuardBody::Off;
                b.level = level;
            });
        }
    }
}
unsafe extern "C" fn tick(actor: u32, state: u8) {
    unsafe { with_peer(actor, |b| b.play_received(actor, state)) };
}
impl Battle {
    unsafe fn play_received(&mut self, p: u32, state: u8) {
        unsafe {
            if read::<u8>(p, 5) == 0 {
                if state == 13 && self.cursor.state == 20 && self.cursor.time.step == 0 {
                    let draw_frame = read::<f32>(p, 1280).clamp(10., 36.) as u16;
                    self.begin_at(p, state, Some(draw_frame));
                } else {
                    self.begin(p, state);
                }
                return;
            }
            if self.cursor.state != state || self.packed != read::<u16>(p, 1204) {
                return;
            }
            let Some(step) = instruction(&self.cursor) else {
                return;
            };
            let sample = MotionFrame::sample(p);
            let previous = self.cursor.time.frame;
            let wrapped = self.cursor.time.observe(sample.frame, sample.blend);
            let elapsed = read::<f32>(p, 28).max(0.);
            let delta = if wrapped {
                elapsed
            } else {
                (sample.frame - previous).max(0.)
            };
            sound::update(&mut self.audio, p, Some(step.clip), sample.frame, None);
            fx::motion_events(&mut self.motion_fx, p, step.clip, previous, sample);
            self.hit |= read::<u32>(p, 12320 + 176) != 0 && read::<u8>(p, 12320 + 180) == 1;
            let was_charging = self.charging;
            self.events(p);
            if self.charging {
                let level = self.charge_level(was_charging, delta, elapsed, charge_cap(state), wrapped);
                self.level = self.level.max(level);
                self.charge_effect
                    .get_or_insert_with(|| ChargeEffect::start(p, self.peer_slot))
                    .update(p, self.charge, self.level);
            }
            let end = MotionFrame {
                ended: step.clip != RADIANCE_CLIP && sample.ended,
                ..sample
            };
            if step.complete(self.cursor.time, end, wrapped) {
                self.cursor.time.step += 1;
                self.start_step(p, None);
            } else if step.clip == RADIANCE_CLIP {
                self.radiance(p, previous, sample.frame);
            }
        }
    }
}
pub(super) unsafe fn radiance_origin(actor: u32, shape: u32) -> Option<[f32; 3]> {
    let roster = unsafe { Roster::current()? };
    let index = roster.peer_index(actor)?;
    if unsafe { PEERS.live(index, &roster, 0) } != Some(actor) {
        return None;
    }
    let mut origin = None;
    PEERS.update(index, actor, |saved, _| {
        origin = saved
            .as_ref()
            .and_then(|b| b.radiance_contact)
            .filter(|&(_, s)| s == shape)
            .map(|(p, _)| p);
    });
    origin
}

pub(crate) unsafe fn guarding_slot(actor: u32, roster: &Roster) -> Option<usize> {
    unsafe {
        let index = roster.peer_index(actor)?;
        if PEERS.live(index, roster, 0) != Some(actor) || !guard_success_action(actor) {
            return None;
        }
        let mut active = false;
        PEERS.update(index, actor, |saved, _| {
            active = saved.as_ref().is_some_and(|b| {
                b.equip == read::<u32>(actor, 5060)
                    && b.packed == read::<u16>(actor, 1204)
                    && matches!(b.peer_guard, GuardBody::Active { area, remaining }
                        if area == read::<u8>(actor, 4180) && remaining > 0.)
            });
        });
        active.then_some(index)
    }
}

pub(crate) fn edit_guard_hold(index: usize, actor: u32, edit: impl FnOnce(&mut Option<Hold>)) {
    PEERS.update(index, actor, |saved, _| {
        if let Some(battle) = saved {
            edit(&mut battle.guard_hold);
        }
    });
}

unsafe extern "C" fn observe(local: u32) {
    unsafe {
        let elapsed = read::<f32>(local, 28).max(0.);
        PEERS.visit_live(0, |_, actor, b| {
            let (group, slot) = action::current(actor);
            if !action::custom(group, slot) || read::<u8>(actor, 4180) != read::<u8>(local, 4180) {
                b.cancel();
            } else if !guard_success_action(actor) {
                b.peer_guard = GuardBody::Off;
            } else if let GuardBody::Active { remaining, .. } = &mut b.peer_guard {
                *remaining = (*remaining - elapsed).max(0.);
            }
        })
    }
}
pub(crate) static PLAYBACK: remote::Playback = remote::Playback {
    tick: tick,
    observe: Some(observe),
    action_state: Some(remote::ActionState {
        encode: encode,
        decode: decode,
    }),
};
