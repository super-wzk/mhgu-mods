//! Wolf's Maw follow-ups and recovery from confirmed monster contacts.
use super::{BLADE_CONTACT_OFFSETS, OWNER, Relaxed};
use crate::{native, read, write};
use core::sync::atomic::AtomicU32;

const HAND_STATE: [u32; 2] = [13156, 13160];
const HEAL_AMOUNT: u32 = 1;
static CONTACTS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];

pub(super) fn clear() {
    for contact in &CONTACTS {
        contact.store(0, Relaxed);
    }
}

pub(super) fn clear_contact(hand: usize) {
    CONTACTS[hand].store(0, Relaxed);
}

pub(super) unsafe fn arm(actor: u32, hand: usize) {
    // 11BACE8 arms stock moves at action change. Extreme moves arm each
    // hand when constructing the contact that 11BA9C8 can queue.
    unsafe {
        let active = read::<f32>(actor, 13144) > 0.;
        write(actor, HAND_STATE[hand], u32::from(active));
    }
}

unsafe fn contact_key(hit: u32) -> u32 {
    // Reserve zero for an empty slot.
    1 + u32::from(unsafe { read::<u16>(hit, 52) })
}

pub(super) unsafe fn recover_on_contact(actor: u32, hit: u32, hand: usize) {
    unsafe {
        if CONTACTS[hand]
            .compare_exchange(contact_key(hit), 0, Relaxed, Relaxed)
            .is_ok()
            && read::<i16>(actor, 5078) > 0
        {
            native!(0x2a451c, actor, HEAL_AMOUNT, 0);
        }
    }
}

#[forge::hook(offset = 0x11ba074)]
unsafe fn spawn_maw_followup(actor: u32) -> u32 {
    if actor == 0 || OWNER.load(Relaxed) != actor {
        return original!(actor);
    }
    unsafe {
        let queued = HAND_STATE.map(|offset| read::<u32>(actor, offset) == 2);
        let result = original!(actor);
        // GU clears the queue and assigns a new serial when a follow-up fires.
        for (hand, was_queued) in queued.into_iter().enumerate() {
            if was_queued && read::<u32>(actor, HAND_STATE[hand]) == 0 {
                let hit = actor + BLADE_CONTACT_OFFSETS[hand];
                CONTACTS[hand].store(contact_key(hit), Relaxed);
            }
        }
        result
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, spawn_maw_followup);
}
