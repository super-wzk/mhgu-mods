//! Long-sword state and persistent contacts for each receiving hunter.
use crate::battle::Battle;
use mhgu_sdk::style::{
    action, hunter,
    remote::{PLAYER_COUNT, PeerStates, Playback, Roster},
};

pub(super) static PLAYBACK: Playback = Playback {
    tick,
    observe: None,
    action_state: None,
};

static PEERS: PeerStates<Battle> = PeerStates::new();

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Slot(pub usize);
pub(super) const LOCAL: Slot = Slot(0);
pub(super) const COUNT: usize = 1 + PLAYER_COUNT;

impl Slot {
    pub(super) fn of(actor: u32) -> Self {
        unsafe {
            Roster::current()
                .and_then(|roster| {
                    let index = roster.peer_index(actor)?;
                    PEERS.live(index, &roster, crate::WEAPON)?;
                    Some(Self(index + 1))
                })
                .unwrap_or(LOCAL)
        }
    }

    pub(super) fn live(self) -> Option<u32> {
        let Some(index) = self.0.checked_sub(1) else {
            return crate::local_actor();
        };
        unsafe { PEERS.live(index, &Roster::current()?, crate::WEAPON) }
    }

    fn reset_contacts(self) {
        crate::instant::reset(self);
        crate::discharge::reset(self);
        crate::release::reset(self);
    }
}

unsafe extern "C" fn tick(actor: u32, state: u8) {
    unsafe {
        let Some(index) = Roster::current().and_then(|roster| roster.peer_index(actor)) else {
            return;
        };
        if hunter::equipped_weapon(actor) != Some(crate::WEAPON) {
            return;
        }
        let slot = Slot(index + 1);
        PEERS.update(index, actor, |playback, changed| {
            if changed {
                slot.reset_contacts();
            }
            playback
                .get_or_insert_with(|| Battle::peer(slot))
                .play_received(actor, state);
        });
    }
}

// Native recoil and arts never call the style's peer action handler.
pub(super) unsafe fn clear_interrupted_effects(slot: Slot) {
    let Some(index) = slot.0.checked_sub(1) else {
        return;
    };
    let actor = slot.live().unwrap_or(0);
    PEERS.update(index, actor, |playback, changed| {
        if changed {
            slot.reset_contacts();
        }
        if let Some(battle) = playback {
            let (group, index) = unsafe { action::current(actor) };
            if !matches!(action::custom_id(group, index), Some(48..=53 | 71 | 72)) {
                battle.clear_action_effects();
            }
        }
    });
}
