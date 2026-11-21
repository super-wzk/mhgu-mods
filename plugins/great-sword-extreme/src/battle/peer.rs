//! Play the action accepted from netPlAction. The sender selects all branches.
use super::*;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhgu_sdk::style::remote::{self, PeerStates, Roster};

static PEERS: PeerStates<Battle> = PeerStates::new();
static LEVEL: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);

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
    if OWNER.load(Relaxed) == actor {
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
    if level <= 4 {
        unsafe { with_peer(actor, |b| b.level = level) };
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
            let delta = if wrapped {
                read::<f32>(p, 28).max(0.)
            } else {
                (sample.frame - previous).max(0.)
            };
            sound::update(&mut self.audio, p, Some(step.clip), sample.frame, None);
            fx::motion_events(&mut self.motion_fx, p, step.clip, previous, sample);
            self.hit |= read::<u32>(p, 12320 + 176) != 0 && read::<u8>(p, 12320 + 180) == 1;
            let was_charging = self.charging;
            self.events(p);
            if self.charging {
                let level = self.charge_level(was_charging, delta, charge_cap(state));
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
unsafe extern "C" fn observe(local: u32) {
    unsafe {
        PEERS.visit_live(0, |_, actor, b| {
            let (group, slot) = action::current(actor);
            if !action::custom(group, slot) || read::<u8>(actor, 4180) != read::<u8>(local, 4180) {
                b.cancel();
            }
        })
    }
}
pub(crate) fn install() {
    static STATE: remote::ActionState = remote::ActionState { encode, decode };
    unsafe {
        remote::bind(1, tick);
        remote::observe(1, observe);
        remote::bind_action_state(1, &STATE);
    }
}
