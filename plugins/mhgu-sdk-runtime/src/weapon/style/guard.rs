//! Carry one accepted style block through GU collision, cost and recoil.
use super::local_entry;
use core::sync::atomic::{
    AtomicPtr, AtomicU32,
    Ordering::{Acquire, Relaxed, Release},
};
use mhgu_sdk::weapon::style::{
    action,
    guard::{self, Decision, GuardResult},
    hunter, movement,
};

const GROUP: u16 = 32;
const GUARD_ACTIVE: u32 = 16;
const MIN_GUARD_STAMINA: f32 = 150.;
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

unsafe fn has_guard_up(actor: u32) -> bool {
    unsafe {
        if *((actor + 5068) as *const u8) & 8 != 0 {
            let has: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(forge::mem::text_addr() + 0x2864e0);
            has(actor, 18) != 0
        } else {
            hunter::has_skill(actor, 171) || hunter::has_skill(actor, 317)
        }
    }
}

// Preserve GU 2F2BA0's hit exclusions and Guard Up requirement when a
// registered style supplies its own guard cone.
unsafe fn guardable(actor: u32, angle: u16, half_angle: u16) -> bool {
    unsafe {
        let flags = *((actor + 12820) as *const u16);
        let kind = *((actor + 12822) as *const u8);
        if *((actor + 12828) as *const i16) < 0
            || flags & 0x100 != 0
            || matches!(kind, 0x19..=0x1d | 0x3a | 0x43 | 0x44 | 0x46)
        {
            return false;
        }
        if flags & 0x20 != 0 && !has_guard_up(actor) {
            return false;
        }
        let facing = *((actor + 5280) as *const u16);
        flags & 0x40 != 0 || (facing.wrapping_sub(angle) as i16).unsigned_abs() <= half_angle
    }
}

unsafe fn guard_below_stamina_limit(actor: u32) -> bool {
    // 2F2BA0 checks 2A5CBC's stamina threshold before the guard flag at 282F58.
    unsafe {
        *((actor + 9488) as *const u32) & GUARD_ACTIVE != 0
            && *((actor + 5088) as *const f32) < MIN_GUARD_STAMINA
    }
}

#[forge::hook(offset = 0x2f2ba0)]
unsafe fn block(actor: u32, angle: u32) -> u32 {
    let accepted = original!(actor, angle);
    unsafe {
        let (group, slot) = action::current(actor);
        let custom = action::custom(group, slot);
        if !custom && group != GROUP {
            return accepted;
        }
        let Some(entry) = local_entry(actor) else {
            return accepted;
        };
        let Some(policy) = entry.style.guard else {
            return accepted;
        };
        let half_angle = entry.rules().map_or(0, |rules| rules.guard_half_angle);
        if accepted == 0 {
            if half_angle == 0
                || (!custom && !guard_below_stamina_limit(actor))
                || !guardable(actor, angle as u16, half_angle)
            {
                return 0;
            }
        }
        clear(actor);
        let mut result = GuardResult::default();
        match policy(actor, &mut result) {
            Decision::Native => accepted,
            Decision::Reject => 0,
            Decision::InPlace => u32::from(
                custom
                    && entry.kind.is_some()
                    && half_angle != 0
                    && (accepted == 0 || guardable(actor, angle as u16, half_angle)),
            ),
            Decision::Precise => {
                INVULNERABILITY.store(result.invulnerability.to_bits(), Relaxed);
                RECOIL_TIER.store(result.recoil, Relaxed);
                SHARPNESS_POWER.store(result.sharpness_power, Relaxed);
                ACTOR.store(actor, Release);
                accepted.max(1)
            }
        }
    }
}

#[forge::hook(offset = 0x2f0670)]
unsafe fn settle(actor: u32, kind: u32, damage: *mut i32, special: *mut u8) -> u32 {
    unsafe {
        let (group, slot) = action::current(actor);
        if action::custom(group, slot)
            && ((actor + 5068) as *const u32).read() & 1 != 0
            && let Some(entry) = local_entry(actor).filter(|entry| entry.kind.is_some())
            && let Some(policy) = entry.style.guard
            && let Some(rules) = entry.rules().filter(|rules| rules.guard_half_angle != 0)
            && guardable(
                actor,
                ((actor + 12824) as *const u16).read(),
                rules.guard_half_angle,
            )
        {
            let mut result = GuardResult::default();
            if matches!(policy(actor, &mut result), Decision::InPlace) {
                // GU has selected this hit for settlement. Returning the native
                // guarded result keeps the current move and skips damage/recoil.
                damage.write(0);
                special.write(0);
                movement::grant_evade_invulnerability(actor, result.invulnerability);
                if let Some(on_block) = result.on_block {
                    on_block(actor);
                }
                return 1;
            }
        }
    }
    original!(actor, kind, damage, special)
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
        if amount < 0 && fixed == 0 && pending(actor) {
            let threshold = SHARPNESS_POWER.load(Relaxed);
            if threshold == GuardResult::NO_SHARPNESS_COST {
                return 0;
            }
            if ((actor + 12820) as *const u16).read() & 0x20 == 0 {
                let power: unsafe extern "C" fn(u32) -> u32 =
                    core::mem::transmute(forge::mem::text_addr() + 0x2f4a2c);
                if power(actor) < threshold {
                    return 0;
                }
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
        let monster = guard::is_monster_contact(actor);
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
    forge::install_hook!(base, settle);
    forge::install_hook!(base, reaction);
    forge::install_hook!(base, sharpness);
}
