//! GU owns the w08 slash gauge. Frontier owns the light gauge used by Demon Riot.
use super::{Battle, OWNER, with_local};
use crate::{STYLE_ID, WEAPON_ID, motion, native, program, read, write};
use core::sync::atomic::Ordering::Relaxed;
use mhgu_sdk::weapon::style::{self, action, movement};

const SLASH_GAUGE: u32 = 13100;
const SLASH_RECOVERY_TIMER: u32 = 13104;
const RIOT_LEVEL: u32 = 13120;
const RIOT_STARTED: u32 = 13124;
const RIOT_TIMER: u32 = 13128;
const ENERGY_CHARGE_TIMER: u32 = 13132;
const TEMPEST_AXE_TIMER: u32 = 13140;
const RESET_SLASH_RECOVERY: u32 = 0x11a7910;
const SLASH_RECOVERY: i32 = program::param(16) as i32;
const RIOT_SLASH_RECOVERY: i32 = 25;
const RIOT_DRAIN_SCALE: u16 = program::param(18) / program::param(15);
const _: () = assert!(program::param(15) == 100 && program::param(18) == 300);

// Forge is soft-float. GU's gauge and timer routines take their amount in
// s0, and the w08 float parameter getter returns its value in s0.
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global sa_float_arg",
    ".hidden sa_float_arg",
    ".type sa_float_arg,%function",
    "sa_float_arg:",
    "vmov s0,r2",
    "bx r3",
    ".global sa_float_param",
    ".hidden sa_float_param",
    ".type sa_float_param,%function",
    "sa_float_param:",
    "push {{r4,lr}}",
    "blx r2",
    "vmov r0,s0",
    "pop {{r4,pc}}",
);
unsafe extern "C" {
    fn sa_float_arg(actor: u32, arg: u32, bits: u32, entry: u32) -> u32;
    fn sa_float_param(actor: u32, index: u32, entry: u32) -> u32;
}

pub(super) unsafe fn slash(actor: u32) -> u16 {
    unsafe { read::<i32>(actor, SLASH_GAUGE) }.clamp(0, 100) as u16
}

pub(super) unsafe fn change_slash(actor: u32, delta: i32) {
    unsafe {
        sa_float_arg(
            actor,
            0,
            (delta as f32).to_bits(),
            forge::mem::text_addr() + 0x11a7968,
        )
    };
}

pub(super) unsafe fn riot_active(actor: u32) -> bool {
    matches!(unsafe { read::<u32>(actor, RIOT_LEVEL) }, 1..=3)
}

fn selected(actor: u32) -> bool {
    actor != 0 && actor == OWNER.load(Relaxed) && style::selected(WEAPON_ID) == Some(STYLE_ID)
}

unsafe fn with_equipped_battle<T>(actor: u32, visit: impl FnOnce(&mut Battle) -> T) -> Option<T> {
    with_local(|slot| {
        slot.as_mut()
            .filter(|battle| battle.equipment == unsafe { read(actor, 5060) })
            .map(visit)
    })
    .flatten()
}

#[forge::hook(offset = 0x2b4e7c)]
unsafe fn slash_empty(actor: u32) -> u32 {
    // A light sword's automatic morph uses the light gauge and the FZ action.
    // Keep GU's slash-gauge morph from taking over while that action is pending.
    if !selected(actor) || unsafe { read::<u32>(actor, motion::SWORD_POSE) } == 0 {
        return original!(actor);
    }
    let light_sword = unsafe {
        with_equipped_battle(actor, |battle| {
            let gauges = battle.gauges;
            gauges.sword || (gauges.unlocked && gauges.light != 0)
        })
    };
    if light_sword == Some(true) {
        return 0;
    }
    original!(actor)
}

#[forge::hook(offset = 0x11a4d8c)]
unsafe fn demon_riot(actor: u32, tier: u32) -> u32 {
    unsafe {
        if !selected(actor) {
            return original!(actor, tier);
        }
        let reserve = slash(actor);
        let active = riot_active(actor);
        let result = original!(actor, tier);
        // Replace any stock reload with our own recovery at the effect frame,
        // when 11A4F24 commits the tier.
        let after = slash(actor);
        if after > reserve {
            change_slash(actor, i32::from(reserve) - i32::from(after));
        }
        if !active && riot_active(actor) {
            with_equipped_battle(actor, |battle| {
                change_slash(actor, RIOT_SLASH_RECOVERY);
                battle.gauges.light = program::param(18);
                battle.gauges.unlocked = true;
                battle.gauges.sword = true;
                battle.model_motion = program::SWORD_IDLE_MOTION;
                write(actor, motion::SWORD_POSE, 1u32);
                battle.publish_gauges();
            });
        }
        result
    }
}

unsafe fn tick_timer(actor: u32, offset: u32) -> u32 {
    unsafe {
        sa_float_arg(
            actor,
            actor + offset,
            1f32.to_bits(),
            forge::mem::text_addr() + 0x53aa84,
        )
    }
}

#[forge::hook(offset = 0x11a75f8)]
unsafe fn weapon_upkeep(actor: u32) -> u32 {
    unsafe {
        if !selected(actor) {
            return original!(actor);
        }
        if !riot_active(actor) {
            // GU 11A75F8 skips passive recovery while a drawn sword is visible.
            // FZ 81210CF6 allows light-sword idle and movement to keep counting.
            if read::<u32>(actor, motion::SWORD_POSE) == 1
                && movement::ground_idle(action::current(actor))
                && read::<i16>(actor, 5078) > 0
                && native!(0x2a2444, actor) == 1
                && native!(0x27bcd0, actor) == 1
                && tick_timer(actor, SLASH_RECOVERY_TIMER) != 0
            {
                native!(RESET_SLASH_RECOVERY, actor);
                change_slash(actor, SLASH_RECOVERY);
            }
            return original!(actor);
        }
        let ready = native!(0x27bcd0, actor);
        if ready != 1 {
            return ready;
        }
        with_equipped_battle(actor, |battle| {
            // The active branch of 11A75F8 reads interval and drain from pl_w08.
            if tick_timer(actor, RIOT_TIMER) != 0 {
                let tier = read::<u32>(actor, RIOT_LEVEL) - 1;
                let interval = sa_float_param(actor, tier, forge::mem::text_addr() + 0x282870);
                write(actor, RIOT_TIMER, f32::from_bits(interval));
                let cost = native!(0x282850, actor, tier + 3) as u16;
                battle.gauges.light = battle
                    .gauges
                    .light
                    .saturating_sub(cost.saturating_mul(RIOT_DRAIN_SCALE));
                write(actor, RIOT_STARTED, 1u8);
            }
            if battle.gauges.light == 0 {
                native!(RESET_SLASH_RECOVERY, actor);
                write(actor, RIOT_STARTED, 0u8);
                write(actor, RIOT_LEVEL, 0u32);
                // Preserve the held form for the shared empty-light morph at neutral.
                battle.gauges.unlocked = false;
            }
            battle.publish_gauges();
        });
        tick_timer(actor, ENERGY_CHARGE_TIMER);
        tick_timer(actor, TEMPEST_AXE_TIMER)
    }
}

pub(super) fn install() {
    let base = forge::mem::text_addr();
    forge::install_hook!(base, slash_empty);
    forge::install_hook!(base, demon_riot);
    forge::install_hook!(base, weapon_upkeep);
}
