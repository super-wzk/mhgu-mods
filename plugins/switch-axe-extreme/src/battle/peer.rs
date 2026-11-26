use super::{Battle, EXPLOSIONS, OWNER, scaled_gain};
use crate::{
    WEAPON_ID, fx, motion, native,
    program::{self, DischargeTier},
    read,
};
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering::Relaxed};
use mhgu_sdk::weapon::style::{
    self,
    action::{self, MotionFrame},
    remote,
};

const PHIAL_LIT: u8 = 1 << 2;
static PHIAL_STATE: AtomicU8 = AtomicU8::new(0);
static PEERS: remote::PeerStates<Battle> = remote::PeerStates::new();

pub(super) unsafe fn index_of(actor: u32) -> Option<usize> {
    unsafe { remote::Roster::current()?.peer_index(actor) }
}

impl Battle {
    unsafe fn peer(actor: u32, equipment: u32, index: usize) -> Self {
        let mut battle = Self::new(equipment);
        battle.peer = true;
        battle.slot = (index + 1) as u8;
        battle.gauges.sword = unsafe { read::<u32>(actor, motion::SWORD_POSE) } != 0;
        battle
    }

    pub(super) fn publish_phial(&self) {
        if !self.peer {
            let tier = self.gauges.discharge_tier() as u8;
            let lit = self.gauges.unlocked && self.gauges.light > 0;
            PHIAL_STATE.store(tier | if lit { PHIAL_LIT } else { 0 }, Relaxed);
        }
    }
}

pub(super) unsafe fn hit(actor: u32) {
    if let Some(index) = unsafe { index_of(actor) } {
        PEERS.update(index, actor, |saved, _| {
            if let Some(battle) = saved {
                let kind = BLADE_KIND[index].load(Relaxed);
                if kind & 1 != 0 {
                    unsafe { battle.gauges.gain_light(scaled_gain(actor, 30)) };
                }
                if kind & 2 != 0 {
                    battle.gauges.discharge_hits = battle.gauges.discharge_hits.saturating_add(1);
                }
            }
        });
    }
}

pub(super) static BLADE_KIND: [AtomicU32; remote::PLAYER_COUNT] =
    [const { AtomicU32::new(0) }; remote::PLAYER_COUNT];

unsafe extern "C" fn send_phial(actor: u32) -> u8 {
    if actor != OWNER.load(Relaxed) {
        return u8::MAX;
    }
    PHIAL_STATE.load(Relaxed)
}

unsafe fn with_peer_battle(actor: u32, index: usize, visit: impl FnOnce(&mut Battle, bool)) {
    PEERS.update(index, actor, |saved, changed| unsafe {
        let equipment = read(actor, 5060);
        let replaced = changed || saved.as_ref().is_some_and(|b| b.equipment != equipment);
        if replaced {
            fx::clear((index + 1) as u8);
            *saved = None;
        }
        let battle = saved.get_or_insert_with(|| Battle::peer(actor, equipment, index));
        visit(battle, replaced);
    });
}

unsafe extern "C" fn receive_phial(actor: u32, bits: u8) {
    unsafe {
        let Some(index) = index_of(actor) else {
            return;
        };
        if bits == u8::MAX {
            // A native/older sender has no W08 snapshot. Retire only state that
            // actually came from the extension, leaving legacy action playback intact.
            PEERS.update(index, actor, |saved, changed| {
                if changed || saved.as_ref().is_some_and(|b| b.peer_light.is_some()) {
                    fx::clear((index + 1) as u8);
                    *saved = None;
                }
            });
            return;
        }
        with_peer_battle(actor, index, |battle, _| {
            battle.gauges.unlocked = bits & PHIAL_LIT != 0;
            // The packet transmits the visible light and the two hit thresholds.
            // The peer never owns this hunter's resource consumption or HUD.
            battle.peer_light = Some(battle.gauges.unlocked);
            battle.gauges.discharge_hits = DischargeTier::from_packet(bits).min_hits();
        });
    }
}

unsafe extern "C" fn peer_frame(local: u32) {
    unsafe {
        fx::scene_frame(read::<f32>(local, 28).max(0.));
        let roster = remote::Roster::current();
        for index in 0..remote::PLAYER_COUNT {
            let actor = roster
                .as_ref()
                .and_then(|r| PEERS.live(index, r, WEAPON_ID))
                .unwrap_or(0);
            PEERS.update(index, actor, |playback, changed| {
                if changed {
                    fx::clear((index + 1) as u8);
                    BLADE_KIND[index].store(0, Relaxed);
                }
                let Some(battle) = playback else { return };
                if read::<u8>(actor, 4180) != read::<u8>(local, 4180) {
                    battle.clear_visuals();
                    return;
                }
                let (group, slot) = action::current(actor);
                if !action::custom(group, slot) {
                    let clip = motion::native_clip(actor);
                    battle.cursor = None;
                    battle.native_visual(
                        actor,
                        clip,
                        MotionFrame::sample(actor),
                        native!(0x2a2444, actor) != 0,
                        read::<f32>(actor, 28).max(0.),
                    );
                }
            });
        }
    }
}

unsafe extern "C" fn receive(actor: u32, id: u8) {
    unsafe {
        let Some(index) = index_of(actor) else {
            return;
        };
        if style::hunter::equipped_weapon(actor) != Some(WEAPON_ID) {
            return;
        }
        with_peer_battle(actor, index, |battle, replaced| {
            if replaced {
                EXPLOSIONS.with(|slots| slots[index + 1] = None);
            }
            if read::<u8>(actor, 5) == 0 {
                battle.start(actor, id);
                return;
            }
            let Some(c) = battle.cursor.as_ref().filter(|c| c.state == id) else {
                return;
            };
            let Some(step) = program::step(id, c.time.step) else {
                return;
            };
            if read::<u16>(actor, 1204) != battle.packed {
                return;
            }
            if matches!(id, 112 | 113 | 163 | 164) {
                battle.charge += read::<f32>(actor, 28).max(0.);
            }
            let (sample, wrapped) = battle.sample(actor, step.clip);
            let c = battle.cursor.as_mut().unwrap();
            if program::step_complete(step, c.time, sample, wrapped) {
                c.time.step += 1;
                battle.play(actor);
            }
        });
    }
}

pub(crate) static PLAYBACK: remote::Playback = remote::Playback {
    tick: receive,
    observe: Some(peer_frame),
    action_state: Some(&remote::ActionState {
        encode: send_phial,
        decode: receive_phial,
    }),
};
