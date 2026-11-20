//! Carry one accepted style block through GU collision, cost and recoil.
use super::local_style;
use core::sync::atomic::{
    AtomicPtr, AtomicU32,
    Ordering::{Acquire, Relaxed, Release},
};
use mhgu_sdk::style::{
    action,
    guard::{self, Decision, GuardResult},
};

const GROUP: u16 = 32;
static ACTOR: AtomicU32 = AtomicU32::new(0);
static INVULNERABILITY: AtomicU32 = AtomicU32::new(0);
static RECOIL_TIER: AtomicU32 = AtomicU32::new(0);
static SHARPNESS_POWER: AtomicU32 = AtomicU32::new(0);

// GU writes the solved intersection to hit+32 in both branches of 16BB38
// before invoking the style's block policy. Keep the borrow scoped to that pass.
struct IncomingHit {
    actor: u32,
    hit: u32,
}
static INCOMING: AtomicPtr<IncomingHit> = AtomicPtr::new(core::ptr::null_mut());
static IMPACT: guard::Impact = guard::Impact(impact_position);

#[forge::hook(offset = 0x16bb38)]
unsafe fn hunter_collision(manager: u32, hit: u32, actor: u32) -> u32 {
    let mut context = IncomingHit { actor, hit };
    let previous = INCOMING.swap(&mut context, Relaxed);
    let value = original!(manager, hit, actor);
    INCOMING.store(previous, Relaxed);
    value
}

unsafe extern "C" fn impact_position(actor: u32, out: *mut [f32; 3]) -> bool {
    unsafe {
        let Some(&IncomingHit { actor: owner, hit }) = INCOMING.load(Relaxed).as_ref() else {
            return false;
        };
        if actor != owner || hit == 0 || ((hit + 176) as *const u32).read() != actor {
            return false;
        }
        out.write(((hit + 32) as *const [f32; 3]).read_unaligned());
        true
    }
}

fn pending(actor: u32) -> bool {
    actor != 0 && ACTOR.load(Acquire) == actor
}

unsafe fn recoil_slot(actor: u32) -> Option<u16> {
    match unsafe { action::current(actor) } {
        (GROUP, slot @ (3 | 4 | 5 | 8)) => Some(slot),
        _ => None,
    }
}

pub(super) fn clear(actor: u32) {
    let _ = ACTOR.compare_exchange(actor, 0, Relaxed, Relaxed);
}

#[forge::hook(offset = 0x2f2ba0)]
unsafe fn block(actor: u32, angle: u32) -> u32 {
    let accepted = original!(actor, angle);
    unsafe {
        if accepted == 0 {
            return accepted;
        }
        let Some(policy) = local_style(actor).and_then(|s| s.guard) else {
            return accepted;
        };
        let (group, slot) = action::current(actor);
        if group != GROUP && !action::custom(group, slot) {
            return accepted;
        }
        clear(actor);
        let mut result = GuardResult::default();
        match policy(actor, &mut result) {
            Decision::Native => accepted,
            Decision::Reject => 0,
            Decision::Precise => {
                INVULNERABILITY.store(result.invulnerability.to_bits(), Relaxed);
                RECOIL_TIER.store(result.recoil, Relaxed);
                SHARPNESS_POWER.store(result.sharpness_power, Relaxed);
                ACTOR.store(actor, Release);
                accepted
            }
        }
    }
}

#[forge::hook(offset = 0x2f4b20)]
unsafe fn reaction(actor: u32, recoil: u32, impact: u32, damage: *mut i32) -> u32 {
    let recoil = if pending(actor) {
        RECOIL_TIER.load(Relaxed)
    } else {
        recoil
    };
    original!(actor, recoil, impact, damage)
}

#[forge::hook(offset = 0x2ad288)]
unsafe fn sharpness(actor: u32, amount: i32, fixed: u32) -> u32 {
    unsafe {
        if amount < 0
            && fixed == 0
            && pending(actor)
            && ((actor + 12820) as *const u16).read() & 0x20 == 0
        {
            let power: unsafe extern "C" fn(u32) -> u32 =
                core::mem::transmute(forge::mem::text_addr() + 0x2f4a2c);
            if power(actor) < SHARPNESS_POWER.load(Relaxed) {
                return 0;
            }
        }
    }
    original!(actor, amount, fixed)
}

pub(super) unsafe fn after_change(actor: u32, group: u32, slot: u32) {
    if !pending(actor) {
        return;
    }
    unsafe {
        if group == u32::from(GROUP) && recoil_slot(actor).map(u32::from) == Some(slot) {
            let timer = (actor + 11072) as *mut f32;
            timer.write(
                timer
                    .read()
                    .max(f32::from_bits(INVULNERABILITY.load(Relaxed))),
            );
        } else {
            clear(actor);
        }
    }
}

pub(super) unsafe extern "C" fn take(actor: u32) -> u32 {
    unsafe {
        if !pending(actor) || recoil_slot(actor).is_none() {
            return 0;
        }
        clear(actor);
        let monster = ((actor + 12826) as *const u16).read() != 0xffff;
        // Finish native recoil setup before the style starts its follow-up.
        let recoil: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(forge::mem::text_addr() + 0x2c4eec);
        recoil(actor);
        if monster { 2 } else { 1 }
    }
}

pub(super) fn install(base: u32) {
    if unsafe { guard::publish_impact(&IMPACT) } {
        forge::install_hook!(base, hunter_collision);
    }
    forge::install_hook!(base, block);
    forge::install_hook!(base, reaction);
    forge::install_hook!(base, sharpness);
}
